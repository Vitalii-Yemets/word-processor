//! The table of contents.
//!
//! # Why it is a field and not just a list
//!
//! Word writes a table of contents as a `TOC` field: an instruction saying
//! which heading levels to gather, wrapped round the entries somebody last
//! generated. Writing only the entries would give a list that looks right and
//! is dead — Word would not offer to update it, and neither could this.
//!
//! So both go in. The instruction is what makes it a table of contents; the
//! entries are what a reader sees before anybody presses Update.
//!
//! # Where the entries come from
//!
//! The document's own outline levels, the same ones the navigation pane reads.
//! A heading is not a paragraph whose style happens to be called "Heading 1" —
//! it is a paragraph with an outline level, however its style is named. That is
//! what makes this work on a document written in any language.

use wp_xml::tree::Element;

use crate::history::EditKind;
use crate::model::{
    Alignment, Block, Paragraph, ParagraphProperties, Run, RunProperties, TabAlignment, TabLeader,
    TabStop,
};
use crate::{edit, position, read, Document, TextPosition};

/// One line of a table of contents.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The heading's text.
    pub text: String,
    /// How deep it is, counted from zero.
    pub level: u8,
    /// Which paragraph it is, so a reader can be taken there.
    pub paragraph: usize,
    /// Which page it fell on, when that is known.
    pub page: usize,
}

/// The instruction Word writes for an ordinary table of contents.
///
/// `\o "1-3"` gathers outline levels one to three; `\h` makes the entries
/// links; `\z` hides the page numbers in web layout; `\u` uses the outline
/// levels rather than only the built-in heading styles.
const INSTRUCTION: &str = "TOC \\o \"1-3\" \\h \\z \\u";

impl Document {
    /// The headings a table of contents would gather.
    ///
    /// `pages` says which page each paragraph fell on, which only the layout
    /// knows; pass an empty slice when that has not been worked out and the
    /// entries will carry no page numbers.
    #[must_use]
    pub fn contents_entries(&self, levels: u8, pages: &[usize]) -> Vec<Entry> {
        (0..self.paragraph_count())
            .filter_map(|index| {
                let level = self.outline_level(index)?;
                if level >= levels {
                    return None;
                }
                let text = self.paragraph_text(index)?;
                let text = text.trim();
                if text.is_empty() {
                    return None;
                }
                Some(Entry {
                    text: text.to_owned(),
                    level,
                    paragraph: index,
                    page: pages.get(index).copied().unwrap_or(0),
                })
            })
            .collect()
    }

    /// Whether the document already has a table of contents.
    #[must_use]
    pub fn has_contents(&self) -> bool {
        self.contents_range().is_some()
    }

    /// Puts a table of contents at the caret, replacing one that is there.
    ///
    /// Returns how many entries it gathered.
    pub fn insert_contents(&mut self, levels: u8, pages: &[usize]) -> usize {
        let entries = self.contents_entries(levels, pages);
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);

        // An existing one is replaced rather than added to, which is what
        // Update Table has to do.
        let at = match self.contents_range() {
            Some((first, last)) => {
                self.remove_paragraph_range(first, last);
                first
            }
            None => caret.paragraph,
        };

        let prefix = self.prefix();
        // The page number goes against the right margin with dots leading to
        // it, which is what a table of contents looks like — so the entries
        // need to know how wide the text is.
        let setup = self.setup_here();
        let text_width = (setup.width - setup.margin_left - setup.margin_right).max(720);
        let blocks = contents_blocks(&entries, text_width);
        let Some(path) = position::paragraph_path(
            &self.tree().root,
            at.min(self.paragraph_count().saturating_sub(1)),
        ) else {
            return 0;
        };
        let Some((position, parent_path)) = path.split_last() else { return 0 };
        let position = *position;
        let parent_path = parent_path.to_vec();
        let Some(parent) = edit::element_at_path_mut(&mut self.tree_mut().root, &parent_path)
        else {
            return 0;
        };

        for (offset, block) in blocks.iter().enumerate() {
            parent.insert_element(position + offset, edit::block_element(block, prefix.as_deref()));
        }

        self.set_caret(TextPosition::new(at, 0));
        self.mark_modified();
        entries.len()
    }

    /// Takes the table of contents out again.
    pub fn remove_contents(&mut self) -> bool {
        let Some((first, last)) = self.contents_range() else { return false };
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);
        self.remove_paragraph_range(first, last);
        self.set_caret(TextPosition::new(first.min(self.paragraph_count().saturating_sub(1)), 0));
        self.mark_modified();
        true
    }

    /// The first and last paragraph of the table of contents, if there is one.
    ///
    /// Found by its field: every paragraph of a generated table carries a run
    /// whose instruction starts with `TOC`, and they are consecutive.
    #[must_use]
    fn contents_range(&self) -> Option<(usize, usize)> {
        let mut first = None;
        let mut last = 0usize;

        for (index, paragraph) in self.paragraph_elements().into_iter().enumerate() {
            if !holds_contents_field(paragraph) {
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
    fn remove_paragraph_range(&mut self, first: usize, last: usize) {
        for _ in first..=last {
            if !position::remove_paragraph(&mut self.tree_mut().root, first) {
                break;
            }
        }
    }
}

/// Whether a paragraph is part of a generated table of contents.
fn holds_contents_field(paragraph: &Element) -> bool {
    fn search(element: &Element) -> bool {
        for child in element.child_elements() {
            if child.namespace.as_deref() != Some(read::W) {
                continue;
            }
            if child.local_name() == "fldSimple" {
                let instruction =
                    child.attribute(Some(read::W), "instr").unwrap_or_default().trim_start();
                // A `TOC` field with a `  switch gathers a sequence rather
                // than the headings — it is a table of figures, and replacing
                // one with the other is exactly the mistake to avoid.
                if instruction.starts_with("TOC") && !instruction.contains(r"\c ") {
                    return true;
                }
            }
            if search(child) {
                return true;
            }
        }
        false
    }
    search(paragraph)
}

/// The paragraphs a table of contents is made of.
///
/// Every one of them carries the field, so the whole table can be found again
/// and replaced when it is updated.
fn contents_blocks(entries: &[Entry], text_width: i32) -> Vec<Block> {
    let mut blocks = Vec::new();

    // The heading over the table, which Word puts there and every reader
    // expects. It is part of the field too, so Update replaces it as well.
    blocks.push(Block::Paragraph(contents_paragraph(
        vec![heading_run("Contents")],
        0,
        true,
        text_width,
    )));

    if entries.is_empty() {
        blocks.push(Block::Paragraph(contents_paragraph(
            vec![Run::field(INSTRUCTION, "No headings in this document")],
            0,
            false,
            text_width,
        )));
        return blocks;
    }

    for entry in entries {
        let line = if entry.page > 0 {
            format!("{}\t{}", entry.text, entry.page)
        } else {
            entry.text.clone()
        };
        blocks.push(Block::Paragraph(contents_paragraph(
            vec![Run::field(INSTRUCTION, &line)],
            entry.level,
            false,
            text_width,
        )));
    }
    blocks
}

/// One line of the table, indented by its level.
///
/// The page number is put against the right margin with dots leading to it,
/// which is the whole look of a table of contents: a right-hand tab stop at the
/// width of the text, with a dotted leader. The stop is measured from the left
/// margin rather than from the indent, so every level's numbers line up in one
/// column however deep the entry is — which is what Word does and what makes
/// the right-hand edge straight.
fn contents_paragraph(runs: Vec<Run>, level: u8, heading: bool, text_width: i32) -> Paragraph {
    let stops = if heading {
        Vec::new()
    } else {
        vec![TabStop { position: text_width, alignment: TabAlignment::End, leader: TabLeader::Dot }]
    };
    Paragraph {
        properties: ParagraphProperties {
            // Half an inch of indent per level, which is what Word's built-in
            // table of contents styles use.
            indent_start: Some(i32::from(level) * 360),
            alignment: if heading { Some(Alignment::Start) } else { None },
            space_after: Some(0),
            tab_stops: stops,
            ..ParagraphProperties::default()
        },
        runs,
    }
}

/// The run that carries the word "Contents" over the table.
fn heading_run(text: &str) -> Run {
    Run {
        properties: RunProperties {
            bold: Some(true),
            size_half_points: Some(28),
            ..RunProperties::default()
        },
        content: vec![crate::model::RunContent::Text(text.to_owned())],
        field: Some(INSTRUCTION.to_owned()),
        revision: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_instruction_says_which_levels_to_gather() {
        assert!(INSTRUCTION.starts_with("TOC"));
        assert!(INSTRUCTION.contains("\\o \"1-3\""));
    }

    #[test]
    fn an_empty_table_still_carries_its_field() {
        let blocks = contents_blocks(&[], 9360);
        assert_eq!(blocks.len(), 2, "a heading and a line saying there is nothing");
        let Block::Paragraph(paragraph) = &blocks[1] else { panic!("a paragraph") };
        assert!(paragraph.runs[0].field.is_some());
    }

    #[test]
    fn deeper_entries_are_indented_further() {
        let entries = [
            Entry { text: "One".to_owned(), level: 0, paragraph: 0, page: 1 },
            Entry { text: "Two".to_owned(), level: 1, paragraph: 1, page: 2 },
        ];
        let blocks = contents_blocks(&entries, 9360);
        let Block::Paragraph(first) = &blocks[1] else { panic!("a paragraph") };
        let Block::Paragraph(second) = &blocks[2] else { panic!("a paragraph") };
        assert!(second.properties.indent_start > first.properties.indent_start);
    }

    #[test]
    fn a_page_number_is_put_after_a_tab() {
        let entries = [Entry { text: "One".to_owned(), level: 0, paragraph: 0, page: 4 }];
        let blocks = contents_blocks(&entries, 9360);
        assert_eq!(blocks[1].plain_text(), "One\t4");
    }
}
