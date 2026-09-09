//! Bookmarks: named places in the text.
//!
//! # Why they matter more than they look
//!
//! On their own a bookmark is a way of jumping somewhere. But it is also what
//! everything that points *into* a document is built on: a cross-reference is a
//! `REF` field naming a bookmark, a page reference is a `PAGEREF` field naming
//! one, and a table of contents entry that can be clicked is a link to one.
//!
//! Like a comment anchor, a bookmark takes up no room in the text at all — it
//! is a pair of empty elements sitting between runs. That is what lets a name
//! be attached to a stretch of words without moving a single caret position.

use wp_xml::tree::{Element, Node};

use crate::history::EditKind;
use crate::{edit, format, position, read, Document, TextPosition};

/// A named stretch of a document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bookmark {
    pub id: i32,
    pub name: String,
    /// Where it begins and ends.
    pub range: (TextPosition, TextPosition),
}

impl Bookmark {
    /// Whether this is one of the names Word keeps for itself.
    ///
    /// `_GoBack` is written on every save to remember where the caret was, and
    /// showing it in a list of bookmarks would puzzle everybody.
    #[must_use]
    pub fn is_hidden(&self) -> bool {
        self.name.starts_with('_')
    }
}

impl Document {
    /// Every bookmark in the document, in the order they begin.
    #[must_use]
    pub fn bookmarks(&self) -> Vec<Bookmark> {
        let mut starts: Vec<(i32, String, TextPosition)> = Vec::new();
        let mut ends: std::collections::HashMap<i32, TextPosition> =
            std::collections::HashMap::new();

        for (index, paragraph) in self.paragraph_elements().into_iter().enumerate() {
            let mut offset = 0usize;
            walk_bookmarks(paragraph, index, &mut offset, &mut starts, &mut ends);
        }

        let mut out: Vec<Bookmark> = starts
            .into_iter()
            .map(|(id, name, start)| Bookmark {
                id,
                name,
                // A bookmark whose end is missing covers nothing, which is what
                // an empty range says.
                range: (start, ends.get(&id).copied().unwrap_or(start)),
            })
            .collect();
        out.sort_by_key(|mark| (mark.range.0.paragraph, mark.range.0.offset));
        out
    }

    /// The bookmark of a given name, if the document has one.
    #[must_use]
    pub fn bookmark(&self, name: &str) -> Option<Bookmark> {
        self.bookmarks().into_iter().find(|mark| mark.name == name)
    }

    /// The text a bookmark covers.
    #[must_use]
    pub fn bookmark_text(&self, name: &str) -> Option<String> {
        let mark = self.bookmark(name)?;
        let (start, end) = mark.range;
        if start.paragraph != end.paragraph {
            // Across paragraphs, the first one is what a reference shows.
            let text = self.paragraph_text(start.paragraph)?;
            return Some(text.get(start.offset..)?.trim().to_owned());
        }
        let text = self.paragraph_text(start.paragraph)?;
        Some(text.get(start.offset..end.offset)?.trim().to_owned())
    }

    /// Names a stretch of the document, or the caret when nothing is selected.
    ///
    /// Returns whether anything was named. A name already in use is moved
    /// rather than duplicated, which is what Word does.
    pub fn add_bookmark(&mut self, name: &str) -> bool {
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);
        self.add_bookmark_quietly(name)
    }

    /// The same without recording history, for callers that already did.
    ///
    /// Adding a caption names it as part of the same action; two entries in the
    /// history would mean pressing undo twice to take back one thing.
    pub(crate) fn add_bookmark_quietly(&mut self, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() {
            return false;
        }
        let caret = self.caret();

        // The old one goes first, so a name is never in two places.
        self.remove_bookmark_quietly(name);

        let (start, end) = self.selection().unwrap_or((caret, caret));
        let id = self.bookmarks().iter().map(|mark| mark.id).max().unwrap_or(-1) + 1;
        let prefix = self.prefix();

        // The end goes in first: putting the start in first would shift it.
        if start.paragraph == end.paragraph {
            self.insert_bookmark_anchor(start.paragraph, end.offset, id, None, prefix.as_deref());
        } else {
            let length =
                self.paragraph_text(start.paragraph).map_or(start.offset, |text| text.len());
            self.insert_bookmark_anchor(start.paragraph, length, id, None, prefix.as_deref());
        }
        self.insert_bookmark_anchor(
            start.paragraph,
            start.offset,
            id,
            Some(name),
            prefix.as_deref(),
        );

        self.mark_modified();
        true
    }

    /// Takes a bookmark away.
    pub fn remove_bookmark(&mut self, name: &str) -> bool {
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);
        let removed = self.remove_bookmark_quietly(name);
        if removed {
            self.mark_modified();
        }
        removed
    }

    /// The same without recording history, for callers that already did.
    fn remove_bookmark_quietly(&mut self, name: &str) -> bool {
        let Some(mark) = self.bookmark(name) else { return false };
        let wanted = mark.id.to_string();

        let mut changed = false;
        for index in 0..self.paragraph_count() {
            let Some(path) = position::paragraph_path(&self.tree().root, index) else { continue };
            let Some(paragraph) = edit::element_at_path_mut(&mut self.tree_mut().root, &path)
            else {
                continue;
            };
            let before = paragraph.children.len();
            paragraph.children.retain(|node| {
                let Node::Element(element) = node else { return true };
                if element.namespace.as_deref() != Some(read::W) {
                    return true;
                }
                !(matches!(element.local_name(), "bookmarkStart" | "bookmarkEnd")
                    && element.attribute(Some(read::W), "id") == Some(wanted.as_str()))
            });
            changed |= paragraph.children.len() != before;
        }
        changed
    }

    /// Puts one end of a bookmark into a paragraph at a text offset.
    ///
    /// `name` is given for the start and left out for the end, which is exactly
    /// how the format writes them.
    fn insert_bookmark_anchor(
        &mut self,
        paragraph_index: usize,
        offset: usize,
        id: i32,
        name: Option<&str>,
        prefix: Option<&str>,
    ) {
        let Some(path) = position::paragraph_path(&self.tree().root, paragraph_index) else {
            return;
        };
        let Some(paragraph) = edit::element_at_path_mut(&mut self.tree_mut().root, &path) else {
            return;
        };

        format::split_runs_at_offset(paragraph, offset);
        let local = if name.is_some() { "bookmarkStart" } else { "bookmarkEnd" };
        let mut anchor = Element::new(&edit::name_with(prefix, local), Some(read::W));
        anchor.set_namespaced_attribute(&edit::name_with(prefix, "id"), read::W, &id.to_string());
        if let Some(name) = name {
            anchor.set_namespaced_attribute(&edit::name_with(prefix, "name"), read::W, name);
        }

        let position = edit::child_position_at_offset(paragraph, offset);
        paragraph.insert_element(position, anchor);
    }
}

/// Finds both ends of every bookmark in a paragraph.
fn walk_bookmarks(
    element: &Element,
    paragraph: usize,
    offset: &mut usize,
    starts: &mut Vec<(i32, String, TextPosition)>,
    ends: &mut std::collections::HashMap<i32, TextPosition>,
) {
    for node in &element.children {
        let Node::Element(child) = node else { continue };
        if child.namespace.as_deref() != Some(read::W) || child.local_name() == "del" {
            continue;
        }

        match child.local_name() {
            "bookmarkStart" => {
                if let Some(id) =
                    child.attribute(Some(read::W), "id").and_then(|text| text.parse().ok())
                {
                    let name =
                        child.attribute(Some(read::W), "name").unwrap_or_default().to_owned();
                    starts.push((id, name, TextPosition::new(paragraph, *offset)));
                }
            }
            "bookmarkEnd" => {
                if let Some(id) =
                    child.attribute(Some(read::W), "id").and_then(|text| text.parse().ok())
                {
                    ends.insert(id, TextPosition::new(paragraph, *offset));
                }
            }
            "t" => *offset += child.text_content().len(),
            _ => {
                if let Some(text) = edit::atomic_text(child) {
                    *offset += text.len();
                } else {
                    walk_bookmarks(child, paragraph, offset, starts, ends);
                }
            }
        }
    }
}

/// A name a bookmark can legally carry.
///
/// The format allows letters, digits and underscores, and insists a name start
/// with a letter. Anything else is quietly turned into an underscore rather
/// than refused: a person naming a bookmark "Figure 1" means something perfectly
/// clear, and correcting them is not the job.
#[must_use]
pub fn sanitise_name(wanted: &str) -> String {
    let mut out = String::new();
    for character in wanted.chars() {
        if character.is_alphanumeric() || character == '_' {
            out.push(character);
        } else {
            out.push('_');
        }
        if out.len() >= 40 {
            break;
        }
    }
    if out.is_empty() || !out.starts_with(|c: char| c.is_alphabetic() || c == '_') {
        out.insert(0, '_');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_keeps_its_letters_and_digits() {
        assert_eq!(sanitise_name("Figure1"), "Figure1");
    }

    #[test]
    fn spaces_and_punctuation_become_underscores() {
        assert_eq!(sanitise_name("Figure 1: the map"), "Figure_1__the_map");
    }

    #[test]
    fn a_name_that_would_start_with_a_digit_gets_an_underscore_first() {
        assert_eq!(sanitise_name("1st"), "_1st");
    }

    #[test]
    fn an_empty_name_still_comes_out_usable() {
        assert_eq!(sanitise_name(""), "_");
    }

    #[test]
    fn the_names_word_keeps_for_itself_are_hidden() {
        let mark = Bookmark {
            id: 0,
            name: "_GoBack".to_owned(),
            range: (TextPosition::default(), TextPosition::default()),
        };
        assert!(mark.is_hidden());
    }
}
