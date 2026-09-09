//! Stretches of the document only one person may edit.
//!
//! # What Word's Block Authors is
//!
//! When several people have the same document open, Block Authors locks the
//! part you have selected so that nobody else can change it while you are
//! working on it. The format writes that as a pair of markers round the text —
//! `w:permStart` and `w:permEnd` — with the marker naming who is allowed in.
//!
//! # Why the markers are a pair and not an attribute
//!
//! Because a locked stretch does not have to be a whole paragraph. It can start
//! halfway through one and end halfway through another, and the only way to say
//! that in this format is to put a mark at each end and let the text between
//! them be what is locked. Bookmarks work the same way, and so does the code:
//! see [`crate::bookmarks`].
//!
//! # What is not here
//!
//! Enforcement between people. This program is not a shared editing server, and
//! there is nobody on the other end to be kept out. What it does is write the
//! marks so that Word keeps them out, read them back so a person can see what
//! is locked, and refuse to type into a stretch locked to somebody else — which
//! is the part that can be done honestly on one machine.

use wp_xml::tree::{Element, Node};

use crate::history::EditKind;
use crate::{edit, format, position, read, Document, TextPosition};

/// A locked stretch of the document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Locked {
    /// The number the pair of markers share.
    pub id: i32,
    /// Who may edit it. Empty means the marker named a group instead, which
    /// this program reads but does not write.
    pub editor: String,
    pub start: TextPosition,
    pub end: TextPosition,
}

impl Locked {
    /// Whether a position falls inside the locked stretch.
    #[must_use]
    pub fn covers(&self, position: TextPosition) -> bool {
        position >= self.start && position <= self.end
    }
}

impl Document {
    /// Every locked stretch, in the order they appear.
    #[must_use]
    pub fn locked_regions(&self) -> Vec<Locked> {
        let mut starts: Vec<(i32, String, TextPosition)> = Vec::new();
        let mut ends: std::collections::HashMap<i32, TextPosition> =
            std::collections::HashMap::new();

        for (index, paragraph) in self.paragraph_elements().into_iter().enumerate() {
            let mut offset = 0usize;
            walk_permissions(paragraph, index, &mut offset, &mut starts, &mut ends);
        }

        let mut out: Vec<Locked> = starts
            .into_iter()
            .map(|(id, editor, start)| {
                let end = ends.get(&id).copied().unwrap_or(start);
                Locked { id, editor, start, end }
            })
            .collect();
        out.sort_by_key(|locked| locked.start);
        out
    }

    /// The locked stretch the caret is in, if it is in one.
    #[must_use]
    pub fn locked_here(&self) -> Option<Locked> {
        let caret = self.caret();
        self.locked_regions().into_iter().find(|locked| locked.covers(caret))
    }

    /// Locks the selection so that only `editor` may change it.
    ///
    /// Returns whether anything was locked. Nothing selected locks nothing:
    /// a stretch of no characters is one nobody could be kept out of.
    pub fn block_authors(&mut self, editor: &str) -> bool {
        let editor = editor.trim();
        if editor.is_empty() {
            return false;
        }
        let Some((start, end)) = self.selection() else { return false };
        if start == end {
            return false;
        }

        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);

        let id = self.locked_regions().iter().map(|locked| locked.id).max().unwrap_or(-1) + 1;
        let prefix = self.prefix();

        // The end goes in first: putting the start in first would shift it.
        if start.paragraph == end.paragraph {
            self.insert_permission(start.paragraph, end.offset, id, None, prefix.as_deref());
        } else {
            self.insert_permission(end.paragraph, end.offset, id, None, prefix.as_deref());
        }
        self.insert_permission(start.paragraph, start.offset, id, Some(editor), prefix.as_deref());

        self.mark_modified();
        true
    }

    /// Unlocks the stretch the caret is in.
    pub fn unblock_authors(&mut self) -> bool {
        let Some(locked) = self.locked_here() else { return false };
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);

        let removed = remove_permission(&mut self.tree_mut().root, locked.id);
        if removed {
            self.mark_modified();
        }
        removed
    }

    /// Puts one end of a locked stretch into a paragraph.
    fn insert_permission(
        &mut self,
        paragraph_index: usize,
        offset: usize,
        id: i32,
        editor: Option<&str>,
        prefix: Option<&str>,
    ) {
        let Some(path) = position::paragraph_path(&self.tree().root, paragraph_index) else {
            return;
        };
        let Some(paragraph) = edit::element_at_path_mut(&mut self.tree_mut().root, &path) else {
            return;
        };

        format::split_runs_at_offset(paragraph, offset);
        let local = if editor.is_some() { "permStart" } else { "permEnd" };
        let mut anchor = Element::new(&edit::name_with(prefix, local), Some(read::W));
        anchor.set_namespaced_attribute(&edit::name_with(prefix, "id"), read::W, &id.to_string());
        if let Some(editor) = editor {
            anchor.set_namespaced_attribute(&edit::name_with(prefix, "ed"), read::W, editor);
        }

        let position = edit::child_position_at_offset(paragraph, offset);
        paragraph.insert_element(position, anchor);
    }
}

/// Finds both ends of every locked stretch in a paragraph.
fn walk_permissions(
    element: &Element,
    paragraph: usize,
    offset: &mut usize,
    starts: &mut Vec<(i32, String, TextPosition)>,
    ends: &mut std::collections::HashMap<i32, TextPosition>,
) {
    for node in &element.children {
        let Node::Element(child) = node else { continue };
        if child.namespace.as_deref() != Some(read::W) {
            continue;
        }

        match child.local_name() {
            "permStart" => {
                let Some(id) = numbered(child) else { continue };
                let editor = child.attribute(Some(read::W), "ed").unwrap_or_default().to_owned();
                starts.push((id, editor, TextPosition::new(paragraph, *offset)));
            }
            "permEnd" => {
                if let Some(id) = numbered(child) {
                    ends.insert(id, TextPosition::new(paragraph, *offset));
                }
            }
            "t" => *offset += child.text_content().len(),
            // A tab and a break each stand for one character of the text, the
            // same as they do everywhere else a position is counted.
            "tab" | "br" | "cr" => *offset += 1,
            _ => walk_permissions(child, paragraph, offset, starts, ends),
        }
    }
}

/// The number a marker carries.
fn numbered(element: &Element) -> Option<i32> {
    element.attribute(Some(read::W), "id")?.trim().parse().ok()
}

/// Takes both markers of one locked stretch out, wherever they are.
fn remove_permission(element: &mut Element, id: i32) -> bool {
    let mut removed = false;
    element.children.retain(|node| {
        let Node::Element(child) = node else { return true };
        let ours = child.namespace.as_deref() == Some(read::W)
            && matches!(child.local_name(), "permStart" | "permEnd")
            && numbered(child) == Some(id);
        if ours {
            removed = true;
        }
        !ours
    });

    for child in element.child_elements_mut() {
        if remove_permission(child, id) {
            removed = true;
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_position_inside_a_locked_stretch_is_covered() {
        let locked = Locked {
            id: 0,
            editor: "Ann".to_owned(),
            start: TextPosition::new(0, 2),
            end: TextPosition::new(1, 4),
        };
        assert!(locked.covers(TextPosition::new(0, 2)), "the first character");
        assert!(locked.covers(TextPosition::new(1, 0)), "the paragraph between");
        assert!(locked.covers(TextPosition::new(1, 4)), "the last character");
        assert!(!locked.covers(TextPosition::new(0, 1)), "before it");
        assert!(!locked.covers(TextPosition::new(1, 5)), "after it");
    }

    #[test]
    fn a_marker_without_a_number_is_not_a_marker() {
        let element = Element::new("w:permStart", Some(read::W));
        assert_eq!(numbered(&element), None);
    }
}
