//! Copying and pasting with the formatting kept.
//!
//! # Why the plain text is not enough
//!
//! The system clipboard carries text. Copying a bold heading and pasting it
//! gives back the words and nothing else — not the size, not the weight, not
//! the style it was in. Inside one program that is a loss nobody accepts: a
//! paragraph moved from one page to another has to arrive looking the way it
//! left.
//!
//! So a copy does two things. It puts the words on the system clipboard, so
//! that every other program on the machine can have them; and it keeps the
//! formatted content here, so that a paste back into this program can put it
//! down whole. Which of the two a paste uses is decided by whether the
//! clipboard still holds the words that were copied — if something else has
//! copied since, the system's text wins, because that is what the person last
//! asked for.
//!
//! # What comes across
//!
//! Paragraphs, their properties, their runs and everything in a run: the
//! formatting, a picture's reference, a shape, an equation. Not a table as a
//! table — selecting across one copies the text of its cells as paragraphs,
//! which is what the plain text of a table is anyway.

use crate::model::{Block, Paragraph, ParagraphProperties, Run, RunContent, RunProperties};
use crate::{read, Document, TextPosition};

/// How much of the copied formatting comes across on a paste.
///
/// Word offers this every time, because the answer depends on why the text was
/// copied. Text taken from a heading and dropped into a paragraph is usually
/// wanted as a paragraph; a paragraph moved from one page to another is wanted
/// exactly as it was.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Formatting {
    /// Word's Keep Source Formatting: it arrives looking the way it left — the
    /// runs and the shape of the paragraphs both.
    #[default]
    Source,
    /// Word's Merge Formatting: the emphasis comes across and nothing else, so
    /// the text takes the font, the size, the colour and the style of where it
    /// lands.
    ///
    /// What counts as emphasis is what Word keeps: bold, italic, underline,
    /// the two strikethroughs, and whether the text rides above or below the
    /// line. A word that was bold in a heading is still bold in a paragraph;
    /// it is not still twenty-eight point.
    Merged,
}

impl Document {
    /// The selection as blocks, with everything about it kept.
    ///
    /// Empty when nothing is selected.
    #[must_use]
    pub fn copy_selection(&self) -> Vec<Block> {
        let Some((start, end)) = self.selection() else { return Vec::new() };
        let mut out = Vec::new();

        for index in start.paragraph..=end.paragraph {
            let Some(element) = self.paragraph_element(index) else { continue };
            let (from, to) = self.range_within(index, start, end);
            let whole = read::read_paragraph(element);
            out.push(Block::Paragraph(slice(&whole, from, to)));
        }
        out
    }

    /// Puts blocks in at the caret, one paragraph after another, keeping the
    /// formatting they were copied with.
    ///
    /// Returns whether anything was put in. The selection is replaced, the same
    /// way typing over a selection replaces it.
    pub fn paste_blocks(&mut self, blocks: &[Block]) -> bool {
        self.paste_blocks_as(blocks, Formatting::Source)
    }

    /// The same, told how much of the copied formatting to bring.
    pub fn paste_blocks_as(&mut self, blocks: &[Block], formatting: Formatting) -> bool {
        let paragraphs: Vec<Paragraph> = blocks
            .iter()
            .filter_map(|block| match block {
                Block::Paragraph(paragraph) => Some(paragraph.clone()),
                Block::Table(_) => None,
            })
            .collect();
        if paragraphs.is_empty() {
            return false;
        }

        // One gesture: a paste is one thing, however many paragraphs it is
        // made of, and one undo has to take the whole of it back. The paste
        // options depend on that — choosing another one takes the last paste
        // back and puts it down again the other way.
        self.begin_gesture();
        if self.selection().is_some() {
            self.delete_selection();
        }

        let mut changed = false;
        for (index, paragraph) in paragraphs.iter().enumerate() {
            if index > 0 {
                self.press_enter();
                changed = true;
            }

            // Whether this paragraph brings its own shape — its style, its
            // alignment, its indents — or takes the one it lands in.
            //
            // Every paragraph after the first was made by this paste and has no
            // shape of its own to lose. The first one is different: it is a
            // paragraph that was already there, and Word overwrites its shape
            // only when there is nothing in it to disagree with the pasted one.
            let empty = self
                .paragraph_text(self.caret().paragraph)
                .is_none_or(|text| text.trim().is_empty());
            if formatting == Formatting::Source && (index > 0 || empty) {
                self.reshape_paragraph(&paragraph.properties);
            }

            let runs: Vec<Run> = match formatting {
                Formatting::Source => paragraph.runs.clone(),
                Formatting::Merged => paragraph.runs.iter().map(merged).collect(),
            };
            if self.insert_runs(&runs) {
                changed = true;
            }
        }
        self.end_gesture();
        changed
    }

    /// Gives the paragraph the caret is in the shape a copied one had.
    ///
    /// Everything it had before goes first. A paragraph that is being made to
    /// look like another one has to lose what the other one does not say as
    /// well as gain what it does — otherwise a heading pasted into a
    /// right-aligned paragraph comes out right-aligned, which is neither where
    /// it came from nor what was asked for.
    fn reshape_paragraph(&mut self, wanted: &ParagraphProperties) {
        let caret = self.caret();
        let prefix = self.prefix();
        let Some(path) = crate::position::paragraph_path(&self.tree().root, caret.paragraph) else {
            return;
        };
        let Some(paragraph) = crate::edit::element_at_path_mut(&mut self.tree_mut().root, &path)
        else {
            return;
        };

        paragraph.remove_children_named(Some(read::W), "pPr");
        crate::format::set_paragraph_style(paragraph, wanted.style.as_deref(), prefix.as_deref());
        crate::format::set_paragraph_numbering(paragraph, wanted.numbering, prefix.as_deref());
        crate::format::apply_paragraph_properties(paragraph, wanted, prefix.as_deref());
        self.mark_modified();
    }

    /// Puts formatted runs in at the caret.
    ///
    /// The run under the caret is cut in two so there is a place between them,
    /// which is how everything that is not text goes into a paragraph — see
    /// [`Document::insert_equation`].
    pub fn insert_runs(&mut self, runs: &[Run]) -> bool {
        let runs: Vec<&Run> = runs.iter().filter(|run| !run.content.is_empty()).collect();
        if runs.is_empty() {
            return false;
        }

        let caret = self.caret();
        self.record(crate::history::EditKind::Structural, caret, false);
        let prefix = self.prefix();

        let Some(path) = crate::position::paragraph_path(&self.tree().root, caret.paragraph) else {
            return false;
        };
        let Some(paragraph) = crate::edit::element_at_path_mut(&mut self.tree_mut().root, &path)
        else {
            return false;
        };

        crate::format::split_runs_at_offset(paragraph, caret.offset);
        let position = crate::edit::child_position_at_offset(paragraph, caret.offset);
        let mut length = 0usize;
        for (offset, run) in runs.iter().enumerate() {
            length += run.plain_text().len();
            paragraph.insert_element(
                position + offset,
                crate::edit::run_element(run, prefix.as_deref()),
            );
        }

        self.set_caret(TextPosition::new(caret.paragraph, caret.offset + length));
        self.mark_modified();
        true
    }
}

/// A run with everything but its emphasis taken off.
///
/// See [`Formatting::Merged`] for what is kept and why.
#[must_use]
fn merged(run: &Run) -> Run {
    Run {
        properties: RunProperties {
            bold: run.properties.bold,
            italic: run.properties.italic,
            underline: run.properties.underline.clone(),
            strike: run.properties.strike,
            double_strike: run.properties.double_strike,
            vertical_align: run.properties.vertical_align,
            ..RunProperties::default()
        },
        content: run.content.clone(),
        field: run.field.clone(),
        revision: run.revision.clone(),
    }
}

/// The part of a paragraph between two offsets, formatting and all.
#[must_use]
fn slice(paragraph: &Paragraph, from: usize, to: usize) -> Paragraph {
    let mut out =
        Paragraph { properties: paragraph.properties.clone(), runs: Vec::with_capacity(2) };
    let mut offset = 0usize;

    for run in &paragraph.runs {
        let mut kept = Run {
            properties: run.properties.clone(),
            content: Vec::new(),
            field: run.field.clone(),
            revision: run.revision.clone(),
        };

        for piece in &run.content {
            match piece {
                RunContent::Text(text) => {
                    let start = offset;
                    let finish = start + text.len();
                    offset = finish;

                    let cut_from = from.max(start);
                    let cut_to = to.min(finish);
                    if cut_to <= cut_from {
                        continue;
                    }
                    let local_from = cut_from - start;
                    let local_to = cut_to - start;
                    if !text.is_char_boundary(local_from) || !text.is_char_boundary(local_to) {
                        continue;
                    }
                    kept.content.push(RunContent::Text(text[local_from..local_to].to_owned()));
                }
                // Everything else stands for one character, and comes across
                // whole when that character is inside the selection.
                other => {
                    let start = offset;
                    offset += 1;
                    if start >= from && start < to {
                        kept.content.push(other.clone());
                    }
                }
            }
        }

        if !kept.content.is_empty() {
            out.runs.push(kept);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::RunProperties;

    fn paragraph() -> Paragraph {
        Paragraph {
            properties: crate::model::ParagraphProperties::default(),
            runs: vec![
                Run::text("plain "),
                Run {
                    properties: RunProperties { bold: Some(true), ..RunProperties::default() },
                    content: vec![RunContent::Text("bold".to_owned())],
                    field: None,
                    revision: None,
                },
                Run::text(" after"),
            ],
        }
    }

    #[test]
    fn the_whole_paragraph_comes_back_whole() {
        let whole = paragraph();
        let copied = slice(&whole, 0, whole.plain_text().len());
        assert_eq!(copied.plain_text(), whole.plain_text());
        assert_eq!(copied.runs.len(), 3);
    }

    #[test]
    fn a_part_of_one_run_comes_back_as_that_part() {
        let copied = slice(&paragraph(), 0, 5);
        assert_eq!(copied.plain_text(), "plain");
        assert_eq!(copied.runs.len(), 1);
    }

    #[test]
    fn the_formatting_of_every_run_comes_with_it() {
        let copied = slice(&paragraph(), 6, 10);
        assert_eq!(copied.plain_text(), "bold");
        assert_eq!(copied.runs[0].properties.bold, Some(true));
    }

    #[test]
    fn a_selection_across_runs_keeps_each_run_as_itself() {
        let copied = slice(&paragraph(), 3, 12);
        assert_eq!(copied.plain_text(), "in bold a");
        assert_eq!(copied.runs.len(), 3, "{:?}", copied.runs);
        assert_eq!(copied.runs[1].properties.bold, Some(true));
    }

    #[test]
    fn nothing_selected_is_nothing_copied() {
        assert!(slice(&paragraph(), 4, 4).runs.is_empty());
    }

    #[test]
    fn the_paragraph_keeps_its_own_properties() {
        let mut whole = paragraph();
        whole.properties.style = Some("Heading1".to_owned());
        assert_eq!(slice(&whole, 0, 5).properties.style.as_deref(), Some("Heading1"));
    }

    #[test]
    fn a_picture_inside_the_selection_comes_with_it() {
        let mut whole = paragraph();
        whole.runs.push(Run {
            properties: RunProperties::default(),
            content: vec![RunContent::Picture(crate::model::Picture::default())],
            field: None,
            revision: None,
        });
        // "plain bold after" is sixteen characters, and the picture is the
        // seventeenth.
        let copied = slice(&whole, 0, 17);
        let pictures = copied
            .runs
            .iter()
            .flat_map(|run| &run.content)
            .filter(|piece| matches!(piece, RunContent::Picture(_)))
            .count();
        assert_eq!(pictures, 1);
    }

    #[test]
    fn a_picture_outside_the_selection_stays_behind() {
        let mut whole = paragraph();
        whole.runs.push(Run {
            properties: RunProperties::default(),
            content: vec![RunContent::Picture(crate::model::Picture::default())],
            field: None,
            revision: None,
        });
        let copied = slice(&whole, 0, 5);
        assert!(copied
            .runs
            .iter()
            .flat_map(|run| &run.content)
            .all(|piece| !matches!(piece, RunContent::Picture(_))));
    }
}
