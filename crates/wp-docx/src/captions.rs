//! Captions, and the cross-references that point at them.
//!
//! # Why a caption is not just a line of text under a picture
//!
//! Because the number in it counts. "Figure 3" is only Figure 3 while it is the
//! third one; put a picture in front of it and it becomes Figure 4, without
//! anybody retyping anything. The format does that with a `SEQ` field: an
//! instruction saying which sequence to count, and the last number somebody
//! worked out. What a reader sees is worked out afresh every time the document
//! is laid out.
//!
//! # And a cross-reference is not just the word "above"
//!
//! `REF` names a bookmark and shows the text it covers; `PAGEREF` names one and
//! shows which page it is on. Both go stale the moment the document changes,
//! which is exactly why they are fields and not typed-out words.

use crate::bookmarks::sanitise_name;
use crate::history::EditKind;
use crate::model::{Block, Paragraph, ParagraphProperties, Run, RunContent, RunProperties};
use crate::{edit, position, Document, TextPosition};

/// What a caption is counting.
///
/// Word's three built-in sequences, which are the ones its Caption dialog
/// offers and the ones a Table of Figures knows how to gather.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Label {
    Figure,
    Table,
    Equation,
}

impl Label {
    pub const ALL: &'static [Label] = &[Label::Figure, Label::Table, Label::Equation];

    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Figure => "Figure",
            Self::Table => "Table",
            Self::Equation => "Equation",
        }
    }

    /// The instruction that counts this sequence.
    #[must_use]
    pub fn instruction(self) -> String {
        format!("SEQ {} \\* ARABIC", self.word())
    }
}

/// Which way a cross-reference points at its target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reference {
    /// The text the bookmark covers — a heading, a caption.
    Text,
    /// Which page it is on.
    Page,
}

impl Reference {
    /// The instruction for pointing at a named place this way.
    #[must_use]
    pub fn instruction(self, name: &str) -> String {
        match self {
            // `\h` makes it a link, which is what Word writes and what lets a
            // reader click through to the target.
            Self::Text => format!("REF {name} \\h"),
            Self::Page => format!("PAGEREF {name} \\h"),
        }
    }
}

impl Document {
    /// Puts a caption below the caret's paragraph and names it.
    ///
    /// The bookmark is what makes the caption something a cross-reference can
    /// point at; without it "see Figure 2" would have nothing to find.
    pub fn add_caption(&mut self, label: Label, text: &str) -> bool {
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);

        let prefix = self.prefix();
        let mut runs = vec![
            Run::text(&format!("{} ", label.word())),
            // The number: a field, so it counts rather than being typed.
            Run::field(&label.instruction(), "1"),
        ];
        if !text.trim().is_empty() {
            runs.push(Run::text(&format!(": {}", text.trim())));
        }

        let caption = Paragraph {
            properties: ParagraphProperties {
                style: Some("Caption".to_owned()),
                ..ParagraphProperties::default()
            },
            runs,
        };

        let Some(path) = position::paragraph_path(&self.tree().root, caret.paragraph) else {
            return false;
        };
        let Some((at, parent_path)) = path.split_last() else { return false };
        let at = *at;
        let parent_path = parent_path.to_vec();
        let Some(parent) = edit::element_at_path_mut(&mut self.tree_mut().root, &parent_path)
        else {
            return false;
        };
        parent.insert_element(
            at + 1,
            edit::block_element(&Block::Paragraph(caption), prefix.as_deref()),
        );

        // Named so it can be referred to. The name has to be unique, and the
        // caption's own text is the most useful thing to build it from.
        let index = caret.paragraph + 1;
        let name = self.unused_bookmark_name(label, text);
        self.set_caret(TextPosition::new(index, 0));
        let length = self.paragraph_text(index).map_or(0, |line| line.len());
        self.extend_selection_to(TextPosition::new(index, length));
        self.add_bookmark_quietly(&name);
        self.clear_selection();
        self.set_caret(TextPosition::new(index, length));

        self.mark_modified();
        true
    }

    /// Every caption in the document, in reading order.
    #[must_use]
    pub fn captions(&self) -> Vec<Caption> {
        let mut counts = std::collections::HashMap::new();
        let mut out = Vec::new();

        for index in 0..self.paragraph_count() {
            let Some(paragraph) = self.paragraph_element(index) else { continue };
            let Some(label) = caption_label(paragraph) else { continue };
            let number = counts.entry(label.word()).or_insert(0usize);
            *number += 1;

            let text = self.paragraph_text(index).unwrap_or_default();
            let bookmark = self
                .bookmarks()
                .into_iter()
                .find(|mark| mark.range.0.paragraph == index)
                .map(|mark| mark.name);

            out.push(Caption {
                label,
                number: *number,
                text: text.trim().to_owned(),
                paragraph: index,
                bookmark,
            });
        }
        out
    }

    /// Puts a cross-reference to a named place at the caret.
    pub fn add_cross_reference(&mut self, name: &str, kind: Reference) -> bool {
        let shown = match kind {
            Reference::Text => self.bookmark_text(name).unwrap_or_else(|| name.to_owned()),
            Reference::Page => "1".to_owned(),
        };
        let run = Run::field(&kind.instruction(name), &shown);

        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);
        let prefix = self.prefix();

        let Some(path) = position::paragraph_path(&self.tree().root, caret.paragraph) else {
            return false;
        };
        let Some(paragraph) = edit::element_at_path_mut(&mut self.tree_mut().root, &path) else {
            return false;
        };
        crate::format::split_runs_at_offset(paragraph, caret.offset);
        let at = edit::child_position_at_offset(paragraph, caret.offset);

        // A field is a wrapper round runs, so it goes in as one element.
        let mut field = wp_xml::tree::Element::new(
            &edit::name_with(prefix.as_deref(), "fldSimple"),
            Some(crate::read::W),
        );
        field.set_namespaced_attribute(
            &edit::name_with(prefix.as_deref(), "instr"),
            crate::read::W,
            &format!(" {} ", kind.instruction(name)),
        );
        field.push_element(edit::run_element(&run, prefix.as_deref()));
        paragraph.insert_element(at, field);

        self.set_caret(TextPosition::new(caret.paragraph, caret.offset + shown.len()));
        self.mark_modified();
        true
    }

    /// A bookmark name nothing is using.
    fn unused_bookmark_name(&self, label: Label, text: &str) -> String {
        let stem = if text.trim().is_empty() {
            format!("_{}", label.word())
        } else {
            format!("_{}_{}", label.word(), sanitise_name(text.trim()))
        };
        let taken: Vec<String> = self.bookmarks().into_iter().map(|mark| mark.name).collect();
        if !taken.contains(&stem) {
            return stem;
        }
        let mut number = 2usize;
        loop {
            let candidate = format!("{stem}_{number}");
            if !taken.contains(&candidate) {
                return candidate;
            }
            number += 1;
        }
    }
}

/// One caption in the document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Caption {
    pub label: Label,
    /// What number it is, counting captions of the same label in order.
    pub number: usize,
    pub text: String,
    pub paragraph: usize,
    /// The name a cross-reference would use to point at it.
    pub bookmark: Option<String>,
}

/// Which sequence a paragraph is a caption for, if it is one.
fn caption_label(paragraph: &wp_xml::tree::Element) -> Option<Label> {
    fn search(element: &wp_xml::tree::Element) -> Option<String> {
        for child in element.child_elements() {
            if child.namespace.as_deref() != Some(crate::read::W) {
                continue;
            }
            if child.local_name() == "fldSimple" {
                if let Some(instruction) = child.attribute(Some(crate::read::W), "instr") {
                    return Some(instruction.trim().to_owned());
                }
            }
            if let Some(found) = search(child) {
                return Some(found);
            }
        }
        None
    }

    let instruction = search(paragraph)?;
    let mut parts = instruction.split_whitespace();
    if parts.next()? != "SEQ" {
        return None;
    }
    let name = parts.next()?;
    Label::ALL.iter().copied().find(|label| label.word() == name)
}

/// The number a `SEQ` instruction is counting, if it is one.
///
/// Returns the sequence's name — `SEQ Figure \* ARABIC` counts "Figure".
#[must_use]
pub fn sequence_name(instruction: &str) -> Option<&str> {
    let mut parts = instruction.split_whitespace();
    if !parts.next()?.eq_ignore_ascii_case("SEQ") {
        return None;
    }
    parts.next()
}

/// What a `REF` or `PAGEREF` instruction points at.
///
/// Returns the bookmark's name and which way it points.
#[must_use]
pub fn reference_target(instruction: &str) -> Option<(&str, Reference)> {
    let mut parts = instruction.split_whitespace();
    let kind = match parts.next()? {
        word if word.eq_ignore_ascii_case("REF") => Reference::Text,
        word if word.eq_ignore_ascii_case("PAGEREF") => Reference::Page,
        _ => return None,
    };
    Some((parts.next()?, kind))
}

/// Kept so this module can name the content it builds runs from.
const _: fn(&str) -> RunContent = |text| RunContent::Text(text.to_owned());
const _: fn() -> RunProperties = RunProperties::default;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sequence_instruction_names_what_it_counts() {
        assert_eq!(sequence_name("SEQ Figure \\* ARABIC"), Some("Figure"));
        assert_eq!(sequence_name("seq Table"), Some("Table"));
        assert_eq!(sequence_name("PAGE"), None);
    }

    #[test]
    fn a_reference_instruction_names_its_bookmark_and_its_kind() {
        assert_eq!(reference_target("REF _Figure_map \\h"), Some(("_Figure_map", Reference::Text)));
        assert_eq!(
            reference_target("PAGEREF _Figure_map \\h"),
            Some(("_Figure_map", Reference::Page))
        );
        assert_eq!(reference_target("TOC \\o"), None);
    }

    #[test]
    fn each_label_counts_its_own_sequence() {
        assert_ne!(Label::Figure.instruction(), Label::Table.instruction());
        assert!(Label::Figure.instruction().contains("Figure"));
    }
}
