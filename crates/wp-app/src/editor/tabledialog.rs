//! Word's Table Properties: a tab for each thing a table is made of.
//!
//! # Why tabs and not one list
//!
//! There was a flat list here instead, and its own comment said why: Word's
//! dialog has tabs and a person looking for "make the first row a header" has
//! to guess which one. That was true, and it is still true — but the answer is
//! not to invent a different dialog. Somebody who knows Word knows the row
//! settings are under Row, and a list that puts them somewhere else is a list
//! they have to read rather than a place they already know.
//!
//! # What each tab asks about
//!
//! Table is about the table on the page: how wide, where across, how far in.
//! Row and Cell are about the row and the cell the caret is in, which is why
//! they show what they show and change nothing else. Alt Text is about
//! somebody who cannot see the table at all, and is the only tab whose absence
//! is invisible to the person filling it in.
//!
//! Word has a fifth, Column, which sets the preferred width of a whole column.
//! It is not here: the file has no such property — a column's width is the
//! widths of its cells — and setting one would mean walking every row. See
//! **C6** in the roadmap.

use wp_docx::model::Alignment;
use wp_docx::table_properties::CellAlignment;
use wp_shell::Response;

use crate::chrome::dialog::{Answer, Button, Dialog, Field};
use crate::measure;

use super::dialogs::Asking;
use super::Editor;

// Table.
const TAB_TABLE: usize = 0;
const ROW_TABLE: usize = 1;
const TABLE_WIDTH: usize = 2;
const TABLE_INDENT: usize = 3;
const TABLE_ALIGNMENT: usize = 4;

// Row.
const TAB_ROW: usize = 5;
const ROW_ROW: usize = 6;
const ROW_HEIGHT: usize = 7;
const ROW_HEIGHT_RULE: usize = 8;
const ROW_BREAK: usize = 9;
const ROW_HEADER: usize = 10;

// Cell.
const TAB_CELL: usize = 11;
const ROW_CELL: usize = 12;
const CELL_WIDTH: usize = 13;
const CELL_ALIGNMENT: usize = 14;

// Alt Text.
const TAB_ALT: usize = 15;
const ALT_TITLE: usize = 16;
const ALT_DESCRIPTION: usize = 17;

/// The button that hands over to Borders and Shading, as Word's does.
pub(super) const BORDERS: &str = "Borders…";

/// Word's four alignments for a table across the page. Justified means nothing
/// to a table, so Word does not offer it and neither does this.
const ALIGNMENTS: &[(&str, Alignment)] =
    &[("Left", Alignment::Start), ("Center", Alignment::Center), ("Right", Alignment::End)];

/// And its three for the text inside a cell.
const CELL_ALIGNMENTS: &[(&str, CellAlignment)] = &[
    ("Top", CellAlignment::Top),
    ("Center", CellAlignment::Middle),
    ("Bottom", CellAlignment::Bottom),
];

/// The two rules Word offers for a row's height.
const HEIGHT_RULES: &[&str] = &["At least", "Exactly"];

impl Editor {
    /// Opens Word's Table Properties on the table the caret is in.
    pub(super) fn open_table_properties(&mut self) -> Response {
        if self.document.table_here().is_none() {
            return self.report("Put the caret in a table first");
        }
        let dialog = self.table_dialog();
        self.ask(Asking::Table, dialog)
    }

    /// The dialog itself.
    pub(super) fn table_dialog(&self) -> Dialog {
        let document = &self.document;
        let choice = |label: &str, items: &[&str], current: usize| Field::Choice {
            label: label.to_owned(),
            items: items.iter().map(|item| (*item).to_owned()).collect(),
            current,
        };
        let number = |label: &str, value: String, unit: &'static str| Field::Number {
            label: label.to_owned(),
            value,
            unit,
        };

        let (title, description) = document.table_alt_text();
        // A width of nothing is Word's "auto", and it shows as an empty box
        // rather than as a zero: a table nought per cent wide is not a table.
        let width =
            document.table_width_percent().map_or_else(String::new, |percent| percent.to_string());
        let unit = self.unit;
        let shown = |twips: i32| measure::format(twips, unit);
        let height = document.table_row_height().map_or_else(String::new, shown);
        let cell_width = document.cell_width().map_or_else(String::new, shown);

        let fields = vec![
            // --- Table -----------------------------------------------------
            Field::Tab("Table".to_owned()),
            Field::Columns(2),
            number("Preferred width", width, "%"),
            number(
                "Indent from left",
                measure::format(document.table_indent(), self.unit),
                self.unit.mark(),
            ),
            choice(
                "Alignment",
                &ALIGNMENTS.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
                document
                    .table_alignment()
                    .and_then(|wanted| ALIGNMENTS.iter().position(|(_, found)| *found == wanted))
                    .unwrap_or(0),
            ),
            // --- Row -------------------------------------------------------
            Field::Tab("Row".to_owned()),
            Field::Columns(2),
            number("Specify height", height, self.unit.mark()),
            choice(
                "Row height is",
                HEIGHT_RULES,
                usize::from(document.table_row_height_is_exact()),
            ),
            Field::Check {
                label: "Allow row to break across pages".to_owned(),
                on: document.row_can_break(),
            },
            Field::Check {
                label: "Repeat as header row at the top of each page".to_owned(),
                on: document.table_header_row().unwrap_or(false),
            },
            // --- Cell ------------------------------------------------------
            Field::Tab("Cell".to_owned()),
            Field::Columns(2),
            number("Preferred width", cell_width, self.unit.mark()),
            choice(
                "Vertical alignment",
                &CELL_ALIGNMENTS.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
                document
                    .cell_alignment()
                    .and_then(|wanted| {
                        CELL_ALIGNMENTS.iter().position(|(_, found)| *found == wanted)
                    })
                    .unwrap_or(0),
            ),
            // --- Alt Text --------------------------------------------------
            Field::Tab("Alt Text".to_owned()),
            Field::Text { label: "Title".to_owned(), value: title },
            Field::Text { label: "Description".to_owned(), value: description },
        ];

        crate::chrome::dialog::check_rows(
            "Table Properties",
            &fields,
            &[
                (TAB_TABLE, "a tab"),
                (ROW_TABLE, "a row"),
                (TABLE_WIDTH, "a number"),
                (TABLE_INDENT, "a number"),
                (TABLE_ALIGNMENT, "a list"),
                (TAB_ROW, "a tab"),
                (ROW_ROW, "a row"),
                (ROW_HEIGHT, "a number"),
                (ROW_HEIGHT_RULE, "a list"),
                (ROW_BREAK, "a tick box"),
                (ROW_HEADER, "a tick box"),
                (TAB_CELL, "a tab"),
                (ROW_CELL, "a row"),
                (CELL_WIDTH, "a number"),
                (CELL_ALIGNMENT, "a list"),
                (TAB_ALT, "a tab"),
                (ALT_TITLE, "a box"),
                (ALT_DESCRIPTION, "a box"),
            ],
        );

        Dialog::with_buttons(
            "Table Properties",
            fields,
            vec![
                Button { label: "OK".to_owned(), answer: Answer::Accept, default: true },
                Button {
                    label: BORDERS.to_owned(),
                    answer: Answer::Named(BORDERS),
                    default: false,
                },
                Button { label: "Cancel".to_owned(), answer: Answer::Cancel, default: false },
            ],
        )
        .wide(520.0)
    }

    /// Puts what the dialog says onto the table, the row and the cell.
    pub(super) fn apply_table_dialog(&mut self, dialog: &Dialog) -> Response {
        // An empty box is Word's "auto" rather than a zero, on both widths.
        let percent = dialog.said(TABLE_WIDTH).trim().parse::<i32>().ok();
        // An empty box comes back as nothing rather than as a nought, which is
        // how a width that is left to be worked out is told from one set to
        // zero.
        let height = measure::parse(&dialog.said(ROW_HEIGHT), self.unit);
        let cell = measure::parse(&dialog.said(CELL_WIDTH), self.unit);

        let mut changed = self.document.set_table_width_percent(percent);
        changed |= self
            .document
            .set_table_indent(measure::parse(&dialog.said(TABLE_INDENT), self.unit).unwrap_or(0));
        if let Some((_, alignment)) = ALIGNMENTS.get(dialog.chose(TABLE_ALIGNMENT)) {
            changed |= self.document.set_table_alignment(*alignment);
        }

        changed |= self.document.set_table_row_height(height, dialog.chose(ROW_HEIGHT_RULE) == 1);
        changed |= self.document.set_row_can_break(dialog.ticked(ROW_BREAK));
        changed |= self.document.set_table_header_row(dialog.ticked(ROW_HEADER));

        changed |= self.document.set_cell_width(cell);
        if let Some((_, alignment)) = CELL_ALIGNMENTS.get(dialog.chose(CELL_ALIGNMENT)) {
            changed |= self.document.set_cell_alignment(*alignment);
        }

        changed |= self
            .document
            .set_table_alt_text(&dialog.said(ALT_TITLE), &dialog.said(ALT_DESCRIPTION));

        self.relayout();
        self.edited(changed, "Table properties")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::Document;
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event, Key, Modifiers};

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    /// An editor with the caret inside a table.
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

    fn type_number(editor: &mut Editor, row: usize, text: &str) {
        if let Some(dialog) = &mut editor.dialog {
            if let Some(Field::Number { value, .. }) = dialog.fields.get_mut(row) {
                *value = text.to_owned();
            }
        }
    }

    fn type_text(editor: &mut Editor, row: usize, text: &str) {
        if let Some(dialog) = &mut editor.dialog {
            if let Some(Field::Text { value, .. }) = dialog.fields.get_mut(row) {
                *value = text.to_owned();
            }
        }
    }

    fn accept(editor: &mut Editor) {
        editor.handle(Event::KeyDown { key: Key::Enter, modifiers: Modifiers::default() });
    }

    #[test]
    fn the_dialog_refuses_to_open_outside_a_table() {
        let mut editor = editor();
        editor.document.set_caret(wp_docx::TextPosition::new(0, 0));
        editor.open_table_properties();
        assert!(!editor.in_dialog(), "a dialog about no table opened");
    }

    #[test]
    fn the_table_tab_reaches_the_table() {
        let mut editor = editor();
        editor.open_table_properties();
        type_number(&mut editor, TABLE_WIDTH, "80");
        type_number(&mut editor, TABLE_INDENT, "0.5");
        accept(&mut editor);

        assert_eq!(editor.document.table_width_percent(), Some(80));
        assert_eq!(editor.document.table_indent(), 720);
    }

    #[test]
    fn an_empty_width_is_a_table_that_fits_itself() {
        // Word's "auto". A table nought per cent wide is not a table.
        let mut editor = editor();
        editor.document.set_table_width_percent(Some(50));
        editor.open_table_properties();
        type_number(&mut editor, TABLE_WIDTH, "");
        accept(&mut editor);

        assert_eq!(editor.document.table_width_percent(), None);
    }

    #[test]
    fn the_row_tab_reaches_the_row() {
        let mut editor = editor();
        editor.open_table_properties();
        type_number(&mut editor, ROW_HEIGHT, "0.25");
        if let Some(dialog) = &mut editor.dialog {
            if let Some(Field::Check { on, .. }) = dialog.fields.get_mut(ROW_BREAK) {
                *on = false;
            }
            if let Some(Field::Check { on, .. }) = dialog.fields.get_mut(ROW_HEADER) {
                *on = true;
            }
        }
        accept(&mut editor);

        assert_eq!(editor.document.table_row_height(), Some(360));
        assert!(!editor.document.row_can_break(), "the row may still break");
        assert_eq!(editor.document.table_header_row(), Some(true));
    }

    #[test]
    fn the_alt_text_tab_reaches_the_table_and_survives_the_file() {
        let mut editor = editor();
        editor.open_table_properties();
        type_text(&mut editor, ALT_TITLE, "Quarterly figures");
        type_text(&mut editor, ALT_DESCRIPTION, "Sales by region for each quarter");
        accept(&mut editor);

        let bytes = editor.document.save().expect("saving");
        let reopened = Document::open(&bytes).expect("reopening");
        let text = String::from_utf8_lossy(
            reopened.package().part("word/document.xml").expect("a document part"),
        )
        .into_owned();
        assert!(text.contains("Quarterly figures"), "the title did not reach the file");
        assert!(text.contains("Sales by region"), "the description did not reach the file");
    }

    #[test]
    fn cancelling_changes_nothing() {
        let mut editor = editor();
        let before = editor.document.table_indent();
        editor.open_table_properties();
        type_number(&mut editor, TABLE_INDENT, "2");
        editor.handle(Event::KeyDown { key: Key::Escape, modifiers: Modifiers::default() });

        assert!(!editor.in_dialog());
        assert_eq!(editor.document.table_indent(), before);
    }
}
