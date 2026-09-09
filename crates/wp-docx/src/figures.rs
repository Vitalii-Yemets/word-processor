//! The table of figures, and the index.
//!
//! # Why they live beside the table of contents
//!
//! All three are the same idea: gather something scattered through the document
//! into a list, and keep the list a *field* so it can be gathered again. Only
//! what is gathered differs — headings, captions, or the entries somebody has
//! marked.
//!
//! Word writes a table of figures as a `TOC` field too, with a `\c` switch
//! naming the sequence to gather. That is what tells one apart from the other,
//! and it is why generating one has to be careful not to replace the other.

use wp_xml::tree::{Element, Node};

use crate::captions::Label;
use crate::history::EditKind;
use crate::model::{
    Block, Paragraph, ParagraphProperties, Run, RunProperties, TabAlignment, TabLeader, TabStop,
};
use crate::{edit, position, read, Document, TextPosition};

/// The instruction for a table of figures of one label.
#[must_use]
pub fn figures_instruction(label: Label) -> String {
    format!("TOC \\h \\z \\c \"{}\"", label.word())
}

/// The instruction that generates an index.
const INDEX_INSTRUCTION: &str = "INDEX \\h \"A\" \\c \"1\" \\z \"1049\"";

/// One thing somebody marked for the index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexEntry {
    /// The words as they should be listed.
    pub text: String,
    pub paragraph: usize,
    pub offset: usize,
    /// Which page it fell on, when that is known.
    pub page: usize,
}

impl Document {
    /// Puts a table of figures at the caret, replacing one that is there.
    pub fn insert_figures(&mut self, label: Label, pages: &[usize]) -> usize {
        let captions = self.captions();
        let wanted: Vec<(String, usize)> = captions
            .iter()
            .filter(|caption| caption.label == label)
            .map(|caption| {
                (caption.text.clone(), pages.get(caption.paragraph).copied().unwrap_or(0))
            })
            .collect();

        let instruction = figures_instruction(label);
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);

        let at = match self.field_table_range(&instruction) {
            Some((first, last)) => {
                self.remove_paragraphs(first, last);
                first
            }
            None => caret.paragraph,
        };

        // Where the page numbers are put: against the right-hand edge of the
        // text, with dots leading to them.
        let setup = self.setup_here();
        let text_width = (setup.width - setup.margin_left - setup.margin_right).max(720);
        let mut blocks = vec![Block::Paragraph(field_paragraph(
            vec![heading_run(&format!("Table of {}s", label.word()), &instruction)],
            0,
            None,
        ))];
        if wanted.is_empty() {
            blocks.push(Block::Paragraph(field_paragraph(
                vec![Run::field(&instruction, &format!("No {}s in this document", label.word()))],
                0,
                None,
            )));
        }
        for (text, page) in &wanted {
            let line = if *page > 0 { format!("{text}\t{page}") } else { text.clone() };
            blocks.push(Block::Paragraph(field_paragraph(
                vec![Run::field(&instruction, &line)],
                0,
                Some(text_width),
            )));
        }

        self.write_generated(at, &blocks);
        wanted.len()
    }

    /// Whether the document already has a table of figures for a label.
    #[must_use]
    pub fn has_figures(&self, label: Label) -> bool {
        self.field_table_range(&figures_instruction(label)).is_some()
    }

    /// Marks the selection, or the word at the caret, for the index.
    pub fn mark_index_entry(&mut self, text: &str) -> bool {
        let text = text.trim();
        if text.is_empty() {
            return false;
        }
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

        // An index mark shows nothing and takes up nothing: it is a field with
        // no result, which is exactly what the format says one is.
        let mut field =
            Element::new(&edit::name_with(prefix.as_deref(), "fldSimple"), Some(read::W));
        field.set_namespaced_attribute(
            &edit::name_with(prefix.as_deref(), "instr"),
            read::W,
            &format!(" XE \"{}\" ", text.replace('"', "'")),
        );
        paragraph.insert_element(at, field);

        self.mark_modified();
        true
    }

    /// Everything marked for the index, in reading order.
    #[must_use]
    pub fn index_entries(&self, pages: &[usize]) -> Vec<IndexEntry> {
        let mut out = Vec::new();
        for index in 0..self.paragraph_count() {
            let Some(paragraph) = self.paragraph_element(index) else { continue };
            let mut offset = 0usize;
            walk_index_marks(paragraph, index, &mut offset, pages, &mut out);
        }
        out
    }

    /// Puts an index at the caret, replacing one that is there.
    pub fn insert_index(&mut self, pages: &[usize]) -> usize {
        let mut entries = self.index_entries(pages);
        // Sorted the way an index is read, and with the same words gathered
        // into one line however many times they were marked.
        entries.sort_by(|left, right| {
            left.text
                .to_lowercase()
                .cmp(&right.text.to_lowercase())
                .then(left.page.cmp(&right.page))
        });

        let mut lines: Vec<(String, Vec<usize>)> = Vec::new();
        for entry in &entries {
            match lines.last_mut() {
                Some((text, seen)) if text.eq_ignore_ascii_case(&entry.text) => {
                    if entry.page > 0 && !seen.contains(&entry.page) {
                        seen.push(entry.page);
                    }
                }
                _ => {
                    let pages = if entry.page > 0 { vec![entry.page] } else { Vec::new() };
                    lines.push((entry.text.clone(), pages));
                }
            }
        }

        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);
        let at = match self.field_table_range(INDEX_INSTRUCTION) {
            Some((first, last)) => {
                self.remove_paragraphs(first, last);
                first
            }
            None => caret.paragraph,
        };

        // Where the page numbers are put: against the right-hand edge of the
        // text, with dots leading to them.
        let setup = self.setup_here();
        let text_width = (setup.width - setup.margin_left - setup.margin_right).max(720);
        let mut blocks = vec![Block::Paragraph(field_paragraph(
            vec![heading_run("Index", INDEX_INSTRUCTION)],
            0,
            None,
        ))];
        if lines.is_empty() {
            blocks.push(Block::Paragraph(field_paragraph(
                vec![Run::field(INDEX_INSTRUCTION, "Nothing has been marked for the index")],
                0,
                None,
            )));
        }

        // A letter heading before each run of entries, which is what makes an
        // index findable rather than a long alphabetical list.
        let mut letter = None;
        for (text, pages) in &lines {
            let first = text.chars().next().map(|c| c.to_uppercase().to_string());
            if first != letter {
                if let Some(first) = &first {
                    blocks.push(Block::Paragraph(field_paragraph(
                        vec![heading_run(first, INDEX_INSTRUCTION)],
                        0,
                        None,
                    )));
                }
                letter = first;
            }

            let line = if pages.is_empty() {
                text.clone()
            } else {
                let numbers: Vec<String> = pages.iter().map(ToString::to_string).collect();
                format!("{text}\t{}", numbers.join(", "))
            };
            blocks.push(Block::Paragraph(field_paragraph(
                vec![Run::field(INDEX_INSTRUCTION, &line)],
                1,
                Some(text_width),
            )));
        }

        self.write_generated(at, &blocks);
        lines.len()
    }

    /// Whether the document already has an index.
    #[must_use]
    pub fn has_index(&self) -> bool {
        self.field_table_range(INDEX_INSTRUCTION).is_some()
    }

    /// The paragraphs a generated table of a given instruction occupies.
    pub(crate) fn field_table_range(&self, instruction: &str) -> Option<(usize, usize)> {
        let mut first = None;
        let mut last = 0usize;

        for index in 0..self.paragraph_count() {
            let Some(paragraph) = self.paragraph_element(index) else { continue };
            if !holds_field(paragraph, instruction) {
                if first.is_some() {
                    break;
                }
                continue;
            }
            first.get_or_insert(index);
            last = index;
        }
        first.map(|first| (first, last))
    }

    /// Removes a run of whole paragraphs.
    pub(crate) fn remove_paragraphs(&mut self, first: usize, last: usize) {
        for _ in first..=last {
            if !position::remove_paragraph(&mut self.tree_mut().root, first) {
                break;
            }
        }
    }

    /// Writes a generated table into the document at a paragraph.
    pub(crate) fn write_generated(&mut self, at: usize, blocks: &[Block]) {
        let prefix = self.prefix();
        let at = at.min(self.paragraph_count().saturating_sub(1));
        let Some(path) = position::paragraph_path(&self.tree().root, at) else { return };
        let Some((position, parent_path)) = path.split_last() else { return };
        let position = *position;
        let parent_path = parent_path.to_vec();
        let Some(parent) = edit::element_at_path_mut(&mut self.tree_mut().root, &parent_path)
        else {
            return;
        };

        for (offset, block) in blocks.iter().enumerate() {
            parent.insert_element(position + offset, edit::block_element(block, prefix.as_deref()));
        }
        self.set_caret(TextPosition::new(at, 0));
        self.mark_modified();
    }
}

/// Whether a paragraph carries a field with exactly this instruction.
fn holds_field(paragraph: &Element, instruction: &str) -> bool {
    fn search(element: &Element, instruction: &str) -> bool {
        for child in element.child_elements() {
            if child.namespace.as_deref() != Some(read::W) {
                continue;
            }
            if child.local_name() == "fldSimple"
                && child.attribute(Some(read::W), "instr").unwrap_or_default().trim() == instruction
            {
                return true;
            }
            if search(child, instruction) {
                return true;
            }
        }
        false
    }
    search(paragraph, instruction)
}

/// Finds every index mark in a paragraph, with where it sits.
fn walk_index_marks(
    element: &Element,
    paragraph: usize,
    offset: &mut usize,
    pages: &[usize],
    out: &mut Vec<IndexEntry>,
) {
    for node in &element.children {
        let Node::Element(child) = node else { continue };
        if child.namespace.as_deref() != Some(read::W) || child.local_name() == "del" {
            continue;
        }

        if child.local_name() == "fldSimple" {
            let instruction = child.attribute(Some(read::W), "instr").unwrap_or_default().trim();
            if let Some(text) = index_entry_text(instruction) {
                out.push(IndexEntry {
                    text,
                    paragraph,
                    offset: *offset,
                    page: pages.get(paragraph).copied().unwrap_or(0),
                });
                // An index mark shows nothing, so it takes up nothing.
                continue;
            }
        }

        if child.local_name() == "t" {
            *offset += child.text_content().len();
        } else if let Some(text) = edit::atomic_text(child) {
            *offset += text.len();
        } else {
            walk_index_marks(child, paragraph, offset, pages, out);
        }
    }
}

/// The words an `XE` instruction marks, if it is one.
#[must_use]
pub fn index_entry_text(instruction: &str) -> Option<String> {
    let rest = instruction.strip_prefix("XE").or_else(|| instruction.strip_prefix("xe"))?;
    let rest = rest.trim_start();
    let inside = rest.strip_prefix('"')?;
    let end = inside.find('"')?;
    Some(inside[..end].to_owned())
}

/// One paragraph of a generated table, indented by its level.
///
/// `stop_at` puts a right-hand tab stop with a dotted leader at that many twips
/// from the left margin, which is how a line of page numbers is put against the
/// right-hand edge with dots leading to it. Nothing means no stop: a heading
/// over the table has no number to place.
pub(crate) fn field_paragraph(runs: Vec<Run>, level: u8, stop_at: Option<i32>) -> Paragraph {
    Paragraph {
        properties: ParagraphProperties {
            indent_start: Some(i32::from(level) * 360),
            space_after: Some(0),
            tab_stops: stop_at
                .map(|position| {
                    vec![TabStop { position, alignment: TabAlignment::End, leader: TabLeader::Dot }]
                })
                .unwrap_or_default(),
            ..ParagraphProperties::default()
        },
        runs,
    }
}

/// The run that carries the heading over a generated table.
pub(crate) fn heading_run(text: &str, instruction: &str) -> Run {
    Run {
        properties: RunProperties {
            bold: Some(true),
            size_half_points: Some(26),
            ..RunProperties::default()
        },
        content: vec![crate::model::RunContent::Text(text.to_owned())],
        field: Some(instruction.to_owned()),
        revision: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_index_instruction_names_the_words_it_marks() {
        assert_eq!(index_entry_text("XE \"apples\""), Some("apples".to_owned()));
        assert_eq!(index_entry_text("XE \"apples\" \\b"), Some("apples".to_owned()));
        assert_eq!(index_entry_text("PAGE"), None);
        assert_eq!(index_entry_text("XE apples"), None, "the words have to be quoted");
    }

    #[test]
    fn a_table_of_figures_says_which_sequence_it_gathers() {
        let instruction = figures_instruction(Label::Figure);
        assert!(instruction.starts_with("TOC"));
        assert!(instruction.contains("\\c \"Figure\""));
        assert_ne!(instruction, figures_instruction(Label::Table));
    }
}
