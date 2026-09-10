//! The Table Layout tab: what can be done to a table once it is there.
//!
//! # Nine alignments and not three
//!
//! Word's Alignment group is a three-by-three grid, because a cell has two
//! independent questions to answer: where the text sits across the cell and
//! where it sits up and down it. Three buttons could only answer the first, and
//! a person looking for "align middle centre" found a tab that did not have it.
//! One press here answers both, which is what each of Word's nine does.
//!
//! # Selecting part of a table
//!
//! Word's Select menu takes a cell, a column, a row or the whole table. What it
//! selects is text — a table is paragraphs like everything else — so selecting
//! a row means selecting from the start of its first paragraph to the end of
//! its last. That is why these live here and not in the table model: the model
//! knows where the cells are, and only the editor knows what a selection is.

use wp_docx::model::Alignment;
use wp_docx::table_properties::CellAlignment;
use wp_docx::TextPosition;
use wp_shell::Response;

use crate::chrome::{Choice, Command, Popup};

use super::Editor;

/// Word's nine, in Word's order: across the top row first.
pub(super) const ALIGNMENTS: &[(CellAlignment, Alignment, &str)] = &[
    (CellAlignment::Top, Alignment::Start, "Align Top Left"),
    (CellAlignment::Top, Alignment::Center, "Align Top Center"),
    (CellAlignment::Top, Alignment::End, "Align Top Right"),
    (CellAlignment::Middle, Alignment::Start, "Align Center Left"),
    (CellAlignment::Middle, Alignment::Center, "Align Center"),
    (CellAlignment::Middle, Alignment::End, "Align Center Right"),
    (CellAlignment::Bottom, Alignment::Start, "Align Bottom Left"),
    (CellAlignment::Bottom, Alignment::Center, "Align Bottom Center"),
    (CellAlignment::Bottom, Alignment::End, "Align Bottom Right"),
];

/// What Word's Select menu offers.
const SELECTING: &[&str] = &["Select Cell", "Select Column", "Select Row", "Select Table"];

impl Editor {
    /// One of the nine: across and down in one press.
    pub(super) fn align_cell(&mut self, which: usize) -> Response {
        let Some((down, across, name)) = ALIGNMENTS.get(which).copied() else {
            return Response::Ignored;
        };
        if self.document.table_here().is_none() {
            return self.report("Put the caret in a table first");
        }

        // Both, in one gesture, so one undo takes the whole answer back.
        self.document.begin_gesture();
        let mut changed = self.document.set_cell_alignment(down);
        changed |= self.document.set_alignment_here(across);
        self.document.end_gesture();

        self.relayout();
        self.edited(changed, name)
    }

    /// Which of the nine the cell at the caret is in, so its button can show as
    /// pressed.
    #[must_use]
    pub(super) fn cell_alignment_here(&self) -> Option<usize> {
        let down = self.document.cell_alignment()?;
        let across = self.document.alignment_here();
        ALIGNMENTS
            .iter()
            .position(|(vertical, horizontal, _)| *vertical == down && *horizontal == across)
    }

    /// Repeats the first row at the top of every page the table runs onto.
    pub(super) fn toggle_repeat_header(&mut self) -> Response {
        let Some(header) = self.document.table_header_row() else {
            return self.report("Put the caret in a table first");
        };
        let changed = self.document.set_table_header_row(!header);
        self.relayout();
        self.edited(
            changed,
            if header { "Header row not repeated" } else { "Header row repeated on each page" },
        )
    }

    /// Drops open Word's Select menu.
    pub(super) fn open_table_select(&mut self) -> Response {
        if self.close_popup_if(Choice::TablePart) {
            return Response::Redraw;
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::SelectTablePart) else {
            return Response::Ignored;
        };
        let items = SELECTING.iter().map(|name| (*name).to_owned()).collect();
        self.popup = Some(Popup::new(Choice::TablePart, items, None, left, top, 190.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Selects whichever part of the table was picked.
    pub(super) fn choose_table_part(&mut self, index: usize) -> Response {
        self.popup = None;
        let Some(position) = self.document.table_here() else {
            return self.report("Put the caret in a table first");
        };
        let caret = self.caret();

        // Which paragraphs the wanted part covers. A cell holds at least one
        // paragraph and may hold several, so a part is a range of paragraphs
        // rather than a count of cells.
        let range = match index {
            // The cell the caret is in: the paragraphs of that cell alone.
            0 => self.document.cell_paragraphs(position.row, position.column),
            1 => {
                let mut first = usize::MAX;
                let mut last = 0usize;
                for row in 0..position.rows {
                    let Some((start, end)) = self.document.cell_paragraphs(row, position.column)
                    else {
                        continue;
                    };
                    first = first.min(start);
                    last = last.max(end);
                }
                (first != usize::MAX).then_some((first, last))
            }
            2 => {
                let mut first = usize::MAX;
                let mut last = 0usize;
                for column in 0..position.columns {
                    let Some((start, end)) = self.document.cell_paragraphs(position.row, column)
                    else {
                        continue;
                    };
                    first = first.min(start);
                    last = last.max(end);
                }
                (first != usize::MAX).then_some((first, last))
            }
            3 => self.document.table_paragraphs(),
            _ => None,
        };

        let Some((first, last)) = range else { return Response::Ignored };
        let end = self.document.paragraph_text(last).map_or(0, |text| text.len());
        self.document.set_caret(TextPosition::new(first, 0));
        self.document.extend_selection_to(TextPosition::new(last, end));

        let _ = caret;
        self.needs_redraw = true;
        self.report(SELECTING.get(index).copied().unwrap_or_default())
    }

    /// Word's Convert to Text: the table's cells as paragraphs.
    pub(super) fn convert_table_to_text(&mut self) -> Response {
        if self.document.table_here().is_none() {
            return self.report("Put the caret in a table first");
        }
        let changed = self.document.convert_table_to_text();
        self.relayout();
        self.edited(changed, "Table converted to text")
    }

    /// Shows the boundaries of a table that draws no lines of its own.
    ///
    /// Word's View Gridlines. A table with no borders is invisible, and a
    /// person editing one has to be able to see where the cells are — but the
    /// lines are drawn on the screen only and never printed, which is what
    /// makes them gridlines rather than borders.
    pub(super) fn toggle_table_gridlines(&mut self) -> Response {
        self.show_table_gridlines = !self.show_table_gridlines;
        self.relayout();
        self.report(if self.show_table_gridlines {
            "Table gridlines shown"
        } else {
            "Table gridlines hidden"
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::Document;
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event};

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    fn editor() -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("Before")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor.document.insert_table(3, 3);
        editor.relayout();
        editor
    }

    #[test]
    fn word_has_nine_of_them_and_so_does_this() {
        assert_eq!(ALIGNMENTS.len(), 9);
    }

    #[test]
    fn one_press_answers_both_questions() {
        let mut editor = editor();
        // "Align Bottom Right", which is the last of the nine.
        editor.align_cell(8);

        assert_eq!(editor.document.cell_alignment(), Some(CellAlignment::Bottom));
        assert_eq!(editor.document.alignment_here(), Alignment::End);
        assert_eq!(editor.cell_alignment_here(), Some(8));
    }

    #[test]
    fn both_halves_come_back_with_one_undo() {
        let mut editor = editor();
        editor.align_cell(4);
        assert_eq!(editor.cell_alignment_here(), Some(4));

        editor.document.undo();
        assert_ne!(editor.cell_alignment_here(), Some(4), "one undo left half of it behind");
    }

    #[test]
    fn selecting_a_row_selects_every_cell_of_it() {
        let mut editor = editor();
        editor.document.set_caret(wp_docx::TextPosition::new(2, 0));
        editor.choose_table_part(2);

        let (start, end) = editor.document.selection().expect("a selection");
        assert!(end.paragraph > start.paragraph, "the selection covers one paragraph");
    }

    #[test]
    fn selecting_the_table_selects_all_of_it() {
        let mut editor = editor();
        editor.choose_table_part(3);

        let (start, end) = editor.document.selection().expect("a selection");
        // Nine cells of one paragraph each.
        assert_eq!(end.paragraph - start.paragraph, 8);
    }

    #[test]
    fn repeating_the_header_row_is_a_switch_that_turns_over() {
        let mut editor = editor();
        assert_eq!(editor.document.table_header_row(), Some(false));

        editor.toggle_repeat_header();
        assert_eq!(editor.document.table_header_row(), Some(true));
        editor.toggle_repeat_header();
        assert_eq!(editor.document.table_header_row(), Some(false));
    }

    #[test]
    fn converting_a_table_to_text_leaves_the_words_and_takes_the_table() {
        let mut editor = editor();
        editor.document.set_caret(wp_docx::TextPosition::new(1, 0));
        editor.document.type_text("Kept");
        editor.convert_table_to_text();

        assert!(editor.document.table_here().is_none(), "the table is still there");
        assert!(editor.document.plain_text().contains("Kept"), "the words went with it");
    }

    #[test]
    fn the_gridlines_switch_turns_over() {
        let mut editor = editor();
        let before = editor.show_table_gridlines;
        editor.toggle_table_gridlines();
        assert_ne!(editor.show_table_gridlines, before);
    }
}
