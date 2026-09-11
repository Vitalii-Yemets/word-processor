//! Word's Formula: arithmetic in a cell of a table.
//!
//! # What the dialog asks
//!
//! Three things, which are Word's three. The formula itself, which begins with
//! an equals sign and is guessed at when the dialog opens — a total at the foot
//! of a column of figures is what a formula is nine times in ten. How the
//! answer should be shown, from the list of number formats Word offers. And a
//! function to put into the formula, which is a list that types for you rather
//! than a decision of its own.
//!
//! # Why the formula is guessed at
//!
//! Because Word guesses, and it guesses well: a cell with figures above it gets
//! `=SUM(ABOVE)` and one with figures to its left gets `=SUM(LEFT)`. Anybody who
//! wanted something else is about to type it anyway, and anybody who wanted the
//! total has finished.
//!
//! # What the field holds
//!
//! The instruction and the answer, as every field does. The answer is worked
//! out again at every layout — see [`wp_docx::formula`] — so the one written
//! into the file is what a program that cannot do arithmetic will show, and
//! nothing here reads it back.

use wp_docx::formula;
use wp_shell::Response;

use crate::chrome::dialog::{Dialog, Field};

use super::dialogs::Asking;
use super::Editor;

/// The formula itself.
const FORMULA: usize = 0;
/// How the answer is shown.
const PICTURE: usize = 1;
/// A function to put in, which is a list that types rather than one that
/// decides.
const FUNCTION: usize = 2;

/// The number formats Word's dialog offers, with the picture each writes.
///
/// The first is no format at all: as many figures as the answer needs, which is
/// what a field with no `\#` switch shows.
const PICTURES: &[(&str, &str)] = &[
    ("(as it comes)", ""),
    ("#,##0", "#,##0"),
    ("#,##0.00", "#,##0.00"),
    ("£#,##0.00;(£#,##0.00)", "£#,##0.00;(£#,##0.00)"),
    ("0", "0"),
    ("0%", "0%"),
    ("0.00", "0.00"),
    ("0.00%", "0.00%"),
];

/// The functions the list pastes in, in Word's order.
const FUNCTIONS: &[&str] = &[
    "(paste a function)",
    "ABS",
    "AND",
    "AVERAGE",
    "COUNT",
    "IF",
    "INT",
    "MAX",
    "MIN",
    "MOD",
    "NOT",
    "OR",
    "PRODUCT",
    "ROUND",
    "SIGN",
    "SUM",
];

impl Editor {
    /// Opens it on the cell the caret is in.
    pub(super) fn open_formula(&mut self) -> Response {
        if self.document.table_here().is_none() {
            return self.report("Put the caret in a table cell first");
        }
        let dialog = self.formula_dialog(&self.guessed_formula());
        self.ask(Asking::Formula, dialog)
    }

    /// What Word would suggest: the total of the figures above, or of the ones
    /// to the left.
    fn guessed_formula(&self) -> String {
        let rows = self.document.table_rows_text();
        let Some(place) = self.document.table_here() else { return "=SUM(ABOVE)".to_owned() };

        let numbered = |row: usize, column: usize| {
            rows.get(row)
                .and_then(|row| row.get(column))
                .and_then(|text| wp_docx::sorting::number_in(text))
                .is_some()
        };
        if place.row > 0 && numbered(place.row - 1, place.column) {
            return "=SUM(ABOVE)".to_owned();
        }
        if place.column > 0 && numbered(place.row, place.column - 1) {
            return "=SUM(LEFT)".to_owned();
        }
        "=SUM(ABOVE)".to_owned()
    }

    /// The dialog itself.
    fn formula_dialog(&self, formula: &str) -> Dialog {
        let fields = vec![
            Field::Text { label: "Formula".to_owned(), value: formula.to_owned() },
            Field::Choice {
                label: "Number format".to_owned(),
                items: PICTURES.iter().map(|(name, _)| (*name).to_owned()).collect(),
                current: 0,
            },
            Field::Choice {
                label: "Paste function".to_owned(),
                items: FUNCTIONS.iter().map(|name| (*name).to_owned()).collect(),
                current: 0,
            },
        ];

        crate::chrome::dialog::check_rows(
            "Formula",
            &fields,
            &[(FORMULA, "a box"), (PICTURE, "a list"), (FUNCTION, "a list")],
        );

        Dialog::new("Formula", fields).wide(420.0)
    }

    /// Puts the function that was picked into the formula being typed.
    ///
    /// The list is a way of typing rather than a decision of its own, so it goes
    /// back to its first row afterwards: leaving it showing `SUM` would say the
    /// formula was a sum when the formula is whatever the box now says.
    pub(super) fn formula_dialog_changed(&mut self) {
        let Some(dialog) = &mut self.dialog else { return };
        let chosen = dialog.chose(FUNCTION);
        let Some(name) = FUNCTIONS.get(chosen).filter(|_| chosen > 0) else { return };
        let name = (*name).to_owned();

        if let Some(Field::Text { value, .. }) = dialog.fields.get_mut(FORMULA) {
            if value.trim().is_empty() {
                value.push('=');
            }
            value.push_str(&name);
            value.push_str("()");
        }
        if let Some(Field::Choice { current, .. }) = dialog.fields.get_mut(FUNCTION) {
            *current = 0;
        }
        self.needs_redraw = true;
    }

    /// Puts the formula into the cell.
    pub(super) fn apply_formula(&mut self, dialog: &Dialog) -> Response {
        let typed = dialog.said(FORMULA);
        let typed = typed.trim();
        if typed.is_empty() {
            return Response::Ignored;
        }
        // A formula is a formula whether or not somebody typed the equals sign.
        let body = formula::body_of(typed);
        let picture = PICTURES.get(dialog.chose(PICTURE)).map_or("", |(_, picture)| *picture);
        let instruction = if picture.is_empty() {
            format!("={body}")
        } else {
            format!("={body} \\# \"{picture}\"")
        };

        // What it comes to now, which is what a program that cannot work it out
        // will show. This one works it out again at every layout.
        let shown = self.document.formula_answer(&instruction, self.caret().paragraph);
        let changed = self.document.insert_field(&instruction, &shown);
        self.relayout();
        self.edited(changed, "Formula")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::{Document, TextPosition};
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event};

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    /// An editor with a table of figures and the caret in the cell under them.
    fn editor() -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("Before")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });

        editor.document.insert_table(3, 2);
        for (paragraph, text) in [(1, "Pens"), (2, "3"), (3, "Paper"), (4, "4.5"), (5, "Total")] {
            editor.document.set_caret(TextPosition::new(paragraph, 0));
            editor.document.type_text(text);
        }
        // The cell under the two figures.
        editor.document.set_caret(TextPosition::new(6, 0));
        editor.relayout();
        editor
    }

    fn accept(editor: &mut Editor) {
        let dialog = editor.dialog.take().expect("a dialog");
        editor.asking = None;
        editor.apply_formula(&dialog);
    }

    fn choose(editor: &mut Editor, row: usize, index: usize) {
        if let Some(dialog) = &mut editor.dialog {
            if let Some(Field::Choice { current, .. }) = dialog.fields.get_mut(row) {
                *current = index;
            }
        }
    }

    fn type_formula(editor: &mut Editor, text: &str) {
        if let Some(dialog) = &mut editor.dialog {
            if let Some(Field::Text { value, .. }) = dialog.fields.get_mut(FORMULA) {
                *value = text.to_owned();
            }
        }
    }

    /// What the cell the caret is in now says.
    fn shown(editor: &Editor) -> String {
        editor.document.paragraph_text(editor.caret().paragraph).unwrap_or_default()
    }

    #[test]
    fn the_dialog_guesses_the_total_of_the_figures_above() {
        let mut editor = editor();
        editor.open_formula();
        let dialog = editor.dialog.as_ref().expect("a dialog");
        assert_eq!(dialog.said(FORMULA), "=SUM(ABOVE)");
    }

    #[test]
    fn a_formula_shows_its_answer() {
        let mut editor = editor();
        editor.open_formula();
        accept(&mut editor);
        assert_eq!(shown(&editor), "7.5");
    }

    #[test]
    fn the_answer_changes_when_the_figures_do() {
        let mut editor = editor();
        editor.open_formula();
        accept(&mut editor);
        assert_eq!(shown(&editor), "7.5");

        let drawn = drawn_letters(&editor, 6);

        // A figure above it corrected: the total follows without anything being
        // pressed. The field's own text is what the file holds; what the page
        // shows is worked out again from the cells.
        // At the end of the figure, so three becomes thirty.
        editor.document.set_caret(TextPosition::new(2, 1));
        editor.document.type_text("0");
        editor.relayout();

        assert_eq!(
            editor.document.formula_answer("=SUM(ABOVE)", 6),
            "34.5",
            "the total did not follow the figures"
        );
        assert!(
            drawn_letters(&editor, 6) > drawn,
            "the page is still showing the old total: {drawn} letters"
        );
    }

    /// How many letters one paragraph of the document is drawn with.
    fn drawn_letters(editor: &Editor, paragraph: usize) -> usize {
        editor
            .pages
            .iter()
            .flat_map(|page| &page.lines)
            .filter(|line| line.paragraph == paragraph)
            .map(|line| line.glyphs.len())
            .sum()
    }

    #[test]
    fn the_number_format_reaches_the_answer() {
        let mut editor = editor();
        editor.open_formula();
        // "#,##0.00", the third row.
        choose(&mut editor, PICTURE, 2);
        accept(&mut editor);
        assert_eq!(shown(&editor), "7.50");
    }

    #[test]
    fn a_formula_that_cannot_be_read_says_so() {
        let mut editor = editor();
        editor.open_formula();
        type_formula(&mut editor, "=SUM(");
        accept(&mut editor);
        assert_eq!(shown(&editor), "!Syntax Error");
    }

    #[test]
    fn the_function_list_types_into_the_formula() {
        let mut editor = editor();
        editor.open_formula();
        type_formula(&mut editor, "=");
        // "AVERAGE", which is the fourth row of the list.
        choose(&mut editor, FUNCTION, 3);
        editor.formula_dialog_changed();

        let dialog = editor.dialog.as_ref().expect("a dialog");
        assert_eq!(dialog.said(FORMULA), "=AVERAGE()");
        assert_eq!(dialog.chose(FUNCTION), 0, "the list stayed on the function it pasted");
    }

    #[test]
    fn the_formula_is_kept_in_the_file_as_a_field() {
        let mut editor = editor();
        editor.open_formula();
        accept(&mut editor);

        let bytes = editor.document.save().expect("saving");
        let reopened = Document::open(&bytes).expect("reopening");
        let text = String::from_utf8_lossy(
            reopened.package().part("word/document.xml").expect("a document part"),
        )
        .into_owned();
        assert!(text.contains("=SUM(ABOVE)"), "the instruction did not reach the file");
    }

    #[test]
    fn it_refuses_outside_a_table() {
        let mut editor = editor();
        editor.document.set_caret(TextPosition::new(0, 0));
        editor.open_formula();
        assert!(!editor.in_dialog(), "a formula about no table opened");
    }
}
