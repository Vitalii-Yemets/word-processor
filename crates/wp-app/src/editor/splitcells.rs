//! Word's Split Cells: how many columns and rows a cell becomes.
//!
//! # Why a dialog and not a button
//!
//! The button used to split a merged cell back into what it was made of, and
//! did nothing at all to any other cell — so a person who wanted one cell to
//! be two pressed it and watched nothing happen. Word asks: into how many
//! columns, into how many rows, and, with several cells selected, whether to
//! merge them first. The answer can be any cell into any number that fits.
//!
//! # What it starts out saying
//!
//! What the selection covers, so that OK on its own is the sensible thing:
//! a merged cell is offered as many columns and rows as it was made of, which
//! takes the merge apart again, and a cell of its own is offered Word's two
//! columns and one row. Several cells are offered their rectangle and the
//! merge, which shares out again what they hold. See
//! [`wp_docx::Document::split_defaults`].

use wp_docx::cells::SplitDefaults;
use wp_shell::Response;

use crate::chrome::dialog::{Dialog, Field};

use super::dialogs::Asking;
use super::Editor;

/// How many columns the cell becomes.
const COLUMNS: usize = 0;
/// And how many rows.
const ROWS: usize = 1;
/// Whether several selected cells are merged before they are split. Only
/// there when several are selected, which is when Word offers it.
const MERGE_FIRST: usize = 2;

impl Editor {
    /// Opens it on the cells that are selected, or the cell the caret is in.
    pub(super) fn open_split_cells(&mut self) -> Response {
        let Some(defaults) = self.document.split_defaults() else {
            return self.report("Put the caret in a table first");
        };
        let dialog = split_cells_dialog(defaults);
        self.ask(Asking::SplitCells, dialog)
    }

    /// Splits the cells as the dialog says.
    pub(super) fn apply_split_cells(&mut self, dialog: &Dialog) -> Response {
        let count = |row: usize| dialog.said(row).trim().parse::<usize>().ok();
        let (Some(columns), Some(rows)) = (count(COLUMNS), count(ROWS)) else {
            return self.report("The number of columns and of rows has to be a whole number");
        };
        let changed = self.document.split_cells(columns, rows, dialog.ticked(MERGE_FIRST));
        if !changed {
            // The one rule a person can break: a cell merged down over
            // several rows is shared out over those rows, so the rows it is
            // split into have to divide them.
            return self.report("The cells could not be split into that many columns and rows");
        }
        self.edited(true, "Cells split")
    }
}

/// The dialog itself: Word's three rows, the third only for several cells.
fn split_cells_dialog(defaults: SplitDefaults) -> Dialog {
    let mut fields = vec![
        Field::Number {
            label: "Number of columns".to_owned(),
            value: defaults.columns.to_string(),
            unit: "",
        },
        Field::Number {
            label: "Number of rows".to_owned(),
            value: defaults.rows.to_string(),
            unit: "",
        },
    ];
    let mut rows = vec![(COLUMNS, "a number"), (ROWS, "a number")];
    if defaults.several {
        fields.push(Field::Check { label: "Merge cells before split".to_owned(), on: true });
        rows.push((MERGE_FIRST, "a tick box"));
    }
    crate::chrome::dialog::check_rows("Split Cells", &fields, &rows);

    Dialog::new("Split Cells", fields).wide(320.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chrome::dialog::Answer;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::{Document, TextPosition};
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event, Key, Modifiers};

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    /// An editor with a three by three table, the caret in its first cell.
    fn editor() -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("Before")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let mut editor = Editor::new(library(), Document::open(&bytes).expect("reopening"), None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor.document.set_caret(TextPosition::new(0, 6));
        assert!(editor.document.insert_table(3, 3), "the table went nowhere");
        editor.relayout();
        editor
    }

    fn set(editor: &mut Editor, row: usize, text: &str) {
        if let Some(Field::Number { value, .. }) =
            editor.dialog.as_mut().and_then(|dialog| dialog.fields.get_mut(row))
        {
            *value = text.to_owned();
        }
    }

    #[test]
    fn it_refuses_to_open_outside_a_table() {
        let mut editor = editor();
        editor.document.set_caret(TextPosition::new(0, 0));
        editor.open_split_cells();
        assert!(!editor.in_dialog(), "a dialog about no table opened");
    }

    #[test]
    fn enter_splits_the_cell_as_the_boxes_say() {
        let mut editor = editor();
        editor.open_split_cells();
        set(&mut editor, COLUMNS, "3");
        set(&mut editor, ROWS, "2");
        editor.handle(Event::KeyDown { key: Key::Enter, modifiers: Modifiers::default() });

        assert!(!editor.in_dialog(), "the dialog stayed up");
        let rows = editor.document.table_rows_text();
        assert_eq!(rows.len(), 4, "the cell did not become two rows");
        assert_eq!(rows[0].len(), 5, "nor three columns");
        assert_eq!(rows[1].len(), 5, "the row added holds the other half of it");
        assert_eq!(rows[2].len(), 3, "a row below it changed");
    }

    #[test]
    fn a_count_that_is_no_number_changes_nothing() {
        let mut editor = editor();
        editor.open_split_cells();
        set(&mut editor, COLUMNS, "two");
        editor.finish_dialog(Answer::Accept);
        assert_eq!(editor.document.table_rows_text()[0].len(), 3, "the table changed");
    }

    #[test]
    fn the_picture_of_it_shows_all_three_rows() {
        // The picture is taken over two cells, which is when Word offers to
        // merge them first; a picture of one cell would leave the third row
        // unseen.
        let document = Document::create(&crate::sample::welcome_document()).expect("sample");
        let mut editor = Editor::opened(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor.draw(1400, 900);
        editor.set_view_option("splitcells").expect("the option");
        let dialog = editor.dialog.as_ref().expect("the dialog did not open");
        assert_eq!(dialog.fields.len(), 3, "the tick box is not there");
        assert!(dialog.ticked(MERGE_FIRST));
    }

    #[test]
    fn cancelling_changes_nothing() {
        let mut editor = editor();
        editor.open_split_cells();
        editor.handle(Event::KeyDown { key: Key::Escape, modifiers: Modifiers::default() });
        assert!(!editor.in_dialog());
        assert_eq!(editor.document.table_rows_text()[0].len(), 3, "the table changed");
    }
}
