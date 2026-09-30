//! What Word's outline view asks of the document: where each paragraph
//! stands in the outline, and a heading moved with everything under it.
//!
//! # Why the outline is worked out from the elements
//!
//! A heading is a heading because of its outline level, set on it or taken
//! from its style — not because of its name. The view asks that of every
//! paragraph every time the document is laid out, so it is asked here in
//! one walk of the element tree rather than a paragraph at a time, which
//! would walk the tree from the top once per paragraph.
//!
//! # Why a move takes the elements themselves
//!
//! A heading dragged up the outline carries its body text, its tables and
//! the headings under it. They are taken out of the tree and put back
//! elsewhere as they are, not read and written again, so everything they
//! carry that this program has never heard of goes with them — the same
//! way a table is moved by its handle.

use wp_xml::tree::{Element, Node};

use crate::history::EditKind;
use crate::read::{self, W};
use crate::{edit, position, Document};

/// Where one paragraph stands in the document's outline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Standing {
    /// A heading, at its level counted from nought: Heading 1 is nought
    /// and Heading 9 is eight, which is how `w:outlineLvl` counts.
    Heading(u8),
    /// Body text, in the flow of the document.
    Body,
    /// A paragraph inside a table. The outline shows a table as body text
    /// a row at a time, whatever its cells are formatted as.
    InTable,
}

impl Standing {
    /// The heading level, if it is a heading.
    #[must_use]
    pub fn level(self) -> Option<u8> {
        match self {
            Self::Heading(level) => Some(level),
            Self::Body | Self::InTable => None,
        }
    }
}

impl Document {
    /// Where every paragraph stands in the outline, in reading order: the
    /// numbering a [`crate::TextPosition`] uses.
    #[must_use]
    pub fn outline(&self) -> Vec<Standing> {
        let mut out = Vec::new();
        self.gather_standings(&self.tree().root, false, &mut out);
        out
    }

    /// Walks the elements the way [`position::paragraphs`] does, so that
    /// the n-th standing is the n-th paragraph, noting whether each is in a
    /// table.
    fn gather_standings(&self, element: &Element, in_table: bool, out: &mut Vec<Standing>) {
        for child in element.child_elements() {
            if child.namespace.as_deref() != Some(W) {
                continue;
            }
            if child.is(Some(W), "p") {
                if in_table {
                    out.push(Standing::InTable);
                    continue;
                }
                let direct = child
                    .child(Some(W), "pPr")
                    .map(read::read_paragraph_properties)
                    .unwrap_or_default();
                let level = self.styles().resolve_paragraph(&direct).outline_level;
                out.push(match level.filter(|level| *level < 9) {
                    Some(level) => Standing::Heading(level),
                    None => Standing::Body,
                });
            } else {
                self.gather_standings(child, in_table || child.is(Some(W), "tbl"), out);
            }
        }
    }

    /// Moves the paragraphs from `from` up to but not including `to` so
    /// that they come before the paragraph `before` — or after everything,
    /// when `before` is the number of paragraphs there are. One step to take
    /// back.
    ///
    /// What moves is the blocks those paragraphs are: they have to begin
    /// and end on blocks of one body, which a heading and everything under
    /// it do. Answers false and changes nothing when they do not, when the
    /// place is inside them, or when it is where they already are.
    ///
    /// The caret goes with them if it was in them, so that what was being
    /// worked on is still under it.
    pub fn move_paragraphs(&mut self, from: usize, to: usize, before: usize) -> bool {
        let count = self.paragraph_count();
        // Before the first moved, or before the one after the last, is where
        // they are already; anywhere between is inside them.
        if from >= to || to > count || before > count || (from..=to).contains(&before) {
            return false;
        }
        let root = &self.tree().root;
        let Some(first) = position::paragraph_path(root, from) else { return false };
        let Some(last) = position::paragraph_path(root, to - 1) else { return false };
        let Some((first_at, parent)) = first.split_last() else { return false };
        // The last paragraph may be deep in a table; the block it is in is
        // the one at the parent's depth.
        if last.len() <= parent.len() || !last.starts_with(parent) {
            return false;
        }
        let last_at = last[parent.len()];
        let target_at = if before == count {
            let Some(end) = position::paragraph_path(root, count - 1) else { return false };
            if end.len() <= parent.len() || !end.starts_with(parent) {
                return false;
            }
            end[parent.len()] + 1
        } else {
            let Some(target) = position::paragraph_path(root, before) else { return false };
            if target.len() <= parent.len() || !target.starts_with(parent) {
                return false;
            }
            target[parent.len()]
        };
        let first_at = *first_at;
        if (first_at..=last_at + 1).contains(&target_at) {
            return false;
        }

        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);
        let parent = parent.to_vec();
        let Some(body) = edit::element_at_path_mut(&mut self.tree_to_edit().root, &parent) else {
            return false;
        };
        let moved: Vec<Node> = body.children.drain(first_at..=last_at).collect();
        // Taking them out moved everything after them up, so a place past
        // them is that many fewer than it was.
        let at = if target_at > last_at { target_at - moved.len() } else { target_at };
        body.children.splice(at..at, moved);

        let moved_caret =
            crate::TextPosition::new(moved_to(caret.paragraph, from, to, before), caret.offset);
        self.set_caret(moved_caret);
        self.note_change();
        true
    }
}

/// Where a paragraph is once [`Document::move_paragraphs`] has moved the
/// ones from `from` up to `to` before `before`: one of those moved goes with
/// them, one they moved past goes the other way by as many as moved, and
/// every other stays where it was.
///
/// For whatever is kept by paragraph number beside the document, which the
/// move has to be followed through: the caret, and what is folded in the
/// outline view.
#[must_use]
pub fn moved_to(paragraph: usize, from: usize, to: usize, before: usize) -> usize {
    let length = to.saturating_sub(from);
    let landed = if before < from { before } else { before.saturating_sub(length) };
    if (from..to).contains(&paragraph) {
        paragraph - from + landed
    } else if before < from && (before..from).contains(&paragraph) {
        paragraph + length
    } else if before > to && (to..before).contains(&paragraph) {
        paragraph - length
    } else {
        paragraph
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Block, Body, Paragraph};

    fn document(texts: &[(&str, Option<&str>)]) -> Document {
        let mut body = Body::default();
        for (text, style) in texts {
            let paragraph = match style {
                Some(style) => Paragraph::text(text).with_style(style),
                None => Paragraph::text(text),
            };
            body.blocks.push(Block::Paragraph(paragraph));
        }
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        Document::open(&bytes).expect("reopening")
    }

    fn texts(document: &Document) -> Vec<String> {
        (0..document.paragraph_count())
            .map(|index| document.paragraph_text(index).unwrap_or_default())
            .collect()
    }

    #[test]
    fn headings_stand_at_their_level_and_the_rest_is_body() {
        let document = document(&[
            ("Title", Some("Title")),
            ("One", Some("Heading1")),
            ("text", None),
            ("Two", Some("Heading2")),
        ]);
        assert_eq!(
            document.outline(),
            [Standing::Body, Standing::Heading(0), Standing::Body, Standing::Heading(1)]
        );
    }

    #[test]
    fn a_heading_moves_up_with_what_is_under_it_and_back_in_one_step() {
        let mut document = document(&[
            ("A", Some("Heading1")),
            ("a", None),
            ("B", Some("Heading1")),
            ("b", None),
            ("b2", None),
        ]);
        document.set_caret(crate::TextPosition::new(3, 1));
        assert!(document.move_paragraphs(2, 5, 0));
        assert_eq!(texts(&document), ["B", "b", "b2", "A", "a"]);
        assert_eq!(document.caret(), crate::TextPosition::new(1, 1), "the caret went with it");

        assert!(document.undo());
        assert_eq!(texts(&document), ["A", "a", "B", "b", "b2"]);
    }

    #[test]
    fn a_heading_moves_down_to_the_end() {
        let mut document =
            document(&[("A", Some("Heading1")), ("a", None), ("B", Some("Heading1"))]);
        assert!(document.move_paragraphs(0, 2, 3));
        assert_eq!(texts(&document), ["B", "A", "a"]);
    }

    #[test]
    fn nowhere_new_is_no_move() {
        let mut document = document(&[("A", None), ("B", None), ("C", None)]);
        assert!(!document.move_paragraphs(1, 2, 1), "before itself");
        assert!(!document.move_paragraphs(1, 2, 2), "before the one after it");
        assert!(!document.move_paragraphs(0, 2, 1), "into itself");
        assert!(!document.can_undo(), "a move that did nothing is no step");
    }
}
