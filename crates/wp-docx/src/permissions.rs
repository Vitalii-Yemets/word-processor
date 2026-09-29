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
//! # The same markers, read the other way about
//!
//! A pair of markers says "these people may edit this stretch", and what that
//! amounts to depends on what the rest of the document says. In a document
//! nobody has restricted it is a lock: everybody may edit everywhere except
//! here. In a restricted one it is the opposite — an exception, the one
//! stretch that stays editable while the rest is shut — and that is what
//! Word's Restrict Editing calls it and writes with `w:edGrp="everyone"`.
//!
//! One rule covers both, and it is the rule the format states: inside a pair
//! of markers, only the people they name may edit; outside them, whatever the
//! document says. See `wp_app`'s protection module, where it is applied.
//!
//! # What is not here
//!
//! Enforcement between people. This program is not a shared editing server,
//! and there is nobody on the other end to be kept out. What it does is write
//! the marks so that Word keeps them out, read them back so a person can see
//! what is locked, and refuse to type into a stretch that names somebody else
//! — which is the part that can be done honestly on one machine.

use wp_xml::tree::{Element, Node};

use crate::history::EditKind;
use crate::{edit, format, position, read, Document, TextPosition};

/// The group Word writes for a stretch anybody may edit.
///
/// The format allows several — `administrators`, `owners`, `contributors`,
/// `editors`, `current` — and every one of them but this names people out of
/// a directory that a program on one machine cannot enumerate. This is the one
/// that means what it says without asking anybody.
pub const EVERYONE: &str = "everyone";

/// A stretch of the document with its own rule about who may edit it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Locked {
    /// The number the pair of markers share.
    pub id: i32,
    /// Who may edit it, by name. Empty where the marker named a group.
    pub editor: String,
    /// The group that may edit it, if it named one: `w:edGrp`, which is how
    /// Word writes an exception to a restriction.
    pub group: String,
    pub start: TextPosition,
    pub end: TextPosition,
}

impl Locked {
    /// Whether a position falls inside the stretch.
    #[must_use]
    pub fn covers(&self, position: TextPosition) -> bool {
        position >= self.start && position <= self.end
    }

    /// Whether it is the exception a restriction carries: a stretch anybody
    /// may edit.
    #[must_use]
    pub fn for_everyone(&self) -> bool {
        self.group.eq_ignore_ascii_case(EVERYONE)
    }

    /// Whether the person named may edit it.
    ///
    /// A group that is not `everyone` names people out of a directory, and a
    /// program that guessed whether somebody was in one would be inventing a
    /// permission. So it admits nobody, and whoever is refused is told which
    /// group was wanted.
    #[must_use]
    pub fn admits(&self, person: &str) -> bool {
        self.for_everyone() || (!self.editor.is_empty() && self.editor.eq_ignore_ascii_case(person))
    }

    /// What to call whoever may edit it, for saying why somebody may not.
    #[must_use]
    pub fn named(&self) -> &str {
        if self.editor.is_empty() {
            &self.group
        } else {
            &self.editor
        }
    }
}

impl Document {
    /// Every locked stretch, in the order they appear.
    #[must_use]
    pub fn locked_regions(&self) -> Vec<Locked> {
        let mut starts: Vec<(i32, String, String, TextPosition)> = Vec::new();
        let mut ends: std::collections::HashMap<i32, TextPosition> =
            std::collections::HashMap::new();

        for (index, paragraph) in self.paragraph_elements().into_iter().enumerate() {
            let mut offset = 0usize;
            walk_permissions(paragraph, index, &mut offset, &mut starts, &mut ends);
        }

        let mut out: Vec<Locked> = starts
            .into_iter()
            .map(|(id, editor, group, start)| {
                let end = ends.get(&id).copied().unwrap_or(start);
                Locked { id, editor, group, start, end }
            })
            .collect();
        out.sort_by_key(|locked| locked.start);
        out
    }

    /// The locked stretch the caret is in, if it is in one.
    #[must_use]
    pub fn locked_here(&self) -> Option<Locked> {
        self.locked_at(self.caret())
    }

    /// And the one a given position is in.
    #[must_use]
    pub fn locked_at(&self, position: TextPosition) -> Option<Locked> {
        self.locked_regions().into_iter().find(|locked| locked.covers(position))
    }

    /// Every stretch a position is in, innermost last.
    ///
    /// There can be more than one, because the format gives a marker a single
    /// editor and Word writes a second pair round the same words when a
    /// second person is let in. So "may this person edit here" is a question
    /// about all of them and not about the first one found: one pair naming
    /// somebody else does not shut a person out of a pair that names them.
    #[must_use]
    pub fn locked_all_at(&self, position: TextPosition) -> Vec<Locked> {
        self.locked_regions().into_iter().filter(|locked| locked.covers(position)).collect()
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
        self.mark_permission(Some(editor), None)
    }

    /// Marks the selection as a stretch anybody may edit.
    ///
    /// Word's Restrict Editing exception, written `w:edGrp="everyone"`. The
    /// markers are the same ones [`Document::block_authors`] writes and the
    /// rule is the same rule; what differs is that this one names everybody,
    /// so in a restricted document it is the way in rather than the way out.
    pub fn allow_everyone(&mut self) -> bool {
        self.mark_permission(None, Some(EVERYONE))
    }

    /// Puts a pair of markers round the selection.
    fn mark_permission(&mut self, editor: Option<&str>, group: Option<&str>) -> bool {
        let Some((start, end)) = self.selection() else { return false };
        if start == end {
            return false;
        }

        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);

        let id = self.locked_regions().iter().map(|locked| locked.id).max().unwrap_or(-1) + 1;
        let prefix = self.prefix();

        // The end goes in first: putting the start in first would shift it.
        self.insert_permission(end.paragraph, end.offset, id, None, None, prefix.as_deref());
        self.insert_permission(start.paragraph, start.offset, id, editor, group, prefix.as_deref());

        self.note_change();
        true
    }

    /// Takes the markers off the stretch the caret is in, whichever sort it
    /// is.
    pub fn unblock_authors(&mut self) -> bool {
        let Some(locked) = self.locked_here() else { return false };
        self.remove_locked(locked.id)
    }

    /// And takes off one particular pair, by the number they share.
    ///
    /// Wanted because several pairs can sit round the same words, one for each
    /// person let in, and taking the person off a stretch means taking off
    /// their pair rather than whichever pair happens to be found first.
    pub fn remove_locked(&mut self, id: i32) -> bool {
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);

        let removed = remove_permission(&mut self.tree_to_edit().root, id);
        if removed {
            self.note_change();
        }
        removed
    }

    /// Puts one end of a locked stretch into a paragraph.
    #[allow(clippy::too_many_arguments)]
    fn insert_permission(
        &mut self,
        paragraph_index: usize,
        offset: usize,
        id: i32,
        editor: Option<&str>,
        group: Option<&str>,
        prefix: Option<&str>,
    ) {
        let Some(path) = position::paragraph_path(&self.tree().root, paragraph_index) else {
            return;
        };
        let Some(paragraph) = edit::element_at_path_mut(&mut self.tree_to_edit().root, &path)
        else {
            return;
        };

        format::split_runs_at_offset(paragraph, offset);
        let opens = editor.is_some() || group.is_some();
        let local = if opens { "permStart" } else { "permEnd" };
        let mut anchor = Element::new(&edit::name_with(prefix, local), Some(read::W));
        anchor.set_namespaced_attribute(&edit::name_with(prefix, "id"), read::W, &id.to_string());
        if let Some(editor) = editor {
            anchor.set_namespaced_attribute(&edit::name_with(prefix, "ed"), read::W, editor);
        }
        if let Some(group) = group {
            anchor.set_namespaced_attribute(&edit::name_with(prefix, "edGrp"), read::W, group);
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
    starts: &mut Vec<(i32, String, String, TextPosition)>,
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
                let group = child.attribute(Some(read::W), "edGrp").unwrap_or_default().to_owned();
                starts.push((id, editor, group, TextPosition::new(paragraph, *offset)));
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
            group: String::new(),
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
