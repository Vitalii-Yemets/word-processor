//! Word's Sort: three keys, and what each of them means.
//!
//! # Why a dialog and not a button
//!
//! Sort used to be a button that put the selected paragraphs in alphabetical
//! order, which is one of the things Word's Sort does and not the thing it is.
//! Sorting a table of figures by the third column, largest first, is the
//! ordinary use of it; ordering a list of words is the special case.
//!
//! # The same dialog in both places
//!
//! Word's Sort asks the same three questions whether the caret is in a table or
//! in a run of paragraphs — which column, read as what, which way round — and
//! only the list of columns differs: the columns of the table, or "Paragraphs"
//! and the fields the tabs separate. So there is one dialog here, filled in
//! from whichever of the two it was opened on.
//!
//! # Why the header row is a tick and not a guess
//!
//! Because a table whose first row is the names of the columns and a table
//! whose first row is data look exactly alike to a program, and sorting the
//! names into the middle of the figures is the kind of mistake nobody forgives.
//! Word asks; so does this.

use wp_docx::sorting::{SortKey, SortKind};
use wp_shell::Response;

use crate::chrome::dialog::{Dialog, Field};

use super::dialogs::Asking;
use super::Editor;

/// The first key: what to sort by.
const BY: usize = 1;
const BY_KIND: usize = 2;
const BY_ORDER: usize = 3;
/// The second, asked only where the first leaves two rows equal.
const THEN: usize = 5;
const THEN_KIND: usize = 6;
const THEN_ORDER: usize = 7;
/// And the third.
const LAST: usize = 9;
const LAST_KIND: usize = 10;
const LAST_ORDER: usize = 11;
/// Whether the first row is the names of the columns.
const HEADER: usize = 12;

/// The rows of the dialog that hold a key, in Word's order.
const KEYS: &[(usize, usize, usize)] =
    &[(BY, BY_KIND, BY_ORDER), (THEN, THEN_KIND, THEN_ORDER), (LAST, LAST_KIND, LAST_ORDER)];

/// What the second and third keys offer when they are not wanted.
const NONE: &str = "(none)";

/// The two directions, in Word's words.
const ORDERS: &[&str] = &["Ascending", "Descending"];

impl Editor {
    /// Opens it on the table at the caret, or on the paragraphs selected.
    pub(super) fn open_sort(&mut self) -> Response {
        let columns = self.sort_columns();
        if columns.is_empty() {
            return self.report("Put the caret in a table, or select the paragraphs to sort");
        }
        let dialog = self.sort_dialog(&columns);
        self.ask(Asking::Sort, dialog)
    }

    /// What there is to sort on: the columns of the table, or the pieces of the
    /// paragraphs.
    ///
    /// An empty list is what says there is nothing to sort at all, which is the
    /// one answer the dialog cannot be opened on.
    fn sort_columns(&self) -> Vec<String> {
        if self.document.table_here().is_some() {
            let rows = self.document.table_rows_text();
            if rows.len() < 2 {
                // One row is already in order, whatever order that is.
                return Vec::new();
            }
            let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
            return (1..=columns).map(|number| format!("Column {number}")).collect();
        }

        let Some((start, end)) = self.document.selection() else { return Vec::new() };
        if end.paragraph <= start.paragraph {
            return Vec::new();
        }
        let lines: Vec<String> = (start.paragraph..=end.paragraph)
            .filter_map(|index| self.document.paragraph_text(index))
            .collect();

        // The whole line, and then whatever the tabs separate — which is one
        // piece for a line with no tabs in it, and that piece is the line.
        let fields = wp_docx::sorting::field_count(&lines);
        let mut out = vec!["Paragraphs".to_owned()];
        out.extend((1..fields).map(|number| format!("Field {number}")));
        out
    }

    /// The dialog itself.
    fn sort_dialog(&self, columns: &[String]) -> Dialog {
        let kinds: Vec<String> = SortKind::ALL.iter().map(|kind| kind.label().to_owned()).collect();
        let orders: Vec<String> = ORDERS.iter().map(|name| (*name).to_owned()).collect();
        // The first key must name something; the other two may name nothing,
        // which is how Word says "one key is enough".
        let with_none = |columns: &[String]| {
            let mut items = vec![NONE.to_owned()];
            items.extend(columns.iter().cloned());
            items
        };

        let key = |label: &str, items: Vec<String>, current: usize| {
            vec![
                Field::Columns(3),
                Field::Choice { label: label.to_owned(), items, current },
                Field::Choice { label: "Type".to_owned(), items: kinds.clone(), current: 0 },
                Field::Choice { label: "Order".to_owned(), items: orders.clone(), current: 0 },
            ]
        };

        let mut fields = key("Sort by", columns.to_vec(), 0);
        fields.extend(key("Then by", with_none(columns), 0));
        fields.extend(key("Then by", with_none(columns), 0));
        fields.push(Field::Check {
            label: "My list has a header row".to_owned(),
            on: self.document.table_here().is_some(),
        });

        crate::chrome::dialog::check_rows(
            "Sort",
            &fields,
            &[
                (0, "a row"),
                (BY, "a list"),
                (BY_KIND, "a list"),
                (BY_ORDER, "a list"),
                (4, "a row"),
                (THEN, "a list"),
                (THEN_KIND, "a list"),
                (THEN_ORDER, "a list"),
                (8, "a row"),
                (LAST, "a list"),
                (LAST_KIND, "a list"),
                (LAST_ORDER, "a list"),
                (HEADER, "a tick box"),
            ],
        );

        Dialog::new("Sort", fields).wide(520.0)
    }

    /// Puts what the dialog says into the document.
    pub(super) fn apply_sort(&mut self, dialog: &Dialog) -> Response {
        let in_table = self.document.table_here().is_some();
        let mut keys = Vec::new();
        for (at, (column, kind, order)) in KEYS.iter().copied().enumerate() {
            let chosen = dialog.chose(column);
            // The second and third lists begin with "(none)", so their columns
            // are one further along than they look.
            let column = if at == 0 { Some(chosen) } else { chosen.checked_sub(1) };
            let Some(column) = column else { continue };
            let kind = SortKind::ALL.get(dialog.chose(kind)).copied().unwrap_or_default();
            keys.push(SortKey::new(column, kind, dialog.chose(order) == 1));
        }
        if keys.is_empty() {
            return Response::Ignored;
        }

        let changed = if in_table {
            self.document.sort_table_rows(&keys, dialog.ticked(HEADER))
        } else {
            let Some((start, end)) = self.document.selection() else { return Response::Ignored };
            // A header row in a selection of paragraphs is its first line, which
            // stays where it is the same way a table's does.
            let from = start.paragraph + usize::from(dialog.ticked(HEADER));
            self.document.sort_paragraphs(from, end.paragraph, &keys)
        };

        self.relayout();
        self.edited(changed, if changed { "Sorted" } else { "Already in order" })
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

    /// An editor holding a table of three rows: a heading and two of figures.
    fn editor() -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("Before")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });

        editor.document.insert_table(3, 2);
        for (paragraph, text) in
            [(1, "Name"), (2, "Score"), (3, "Pear"), (4, "10"), (5, "Apple"), (6, "9")]
        {
            editor.document.set_caret(TextPosition::new(paragraph, 0));
            editor.document.type_text(text);
        }
        editor.document.set_caret(TextPosition::new(1, 0));
        editor.relayout();
        editor
    }

    fn choose(editor: &mut Editor, row: usize, index: usize) {
        if let Some(dialog) = &mut editor.dialog {
            if let Some(Field::Choice { current, .. }) = dialog.fields.get_mut(row) {
                *current = index;
            }
        }
    }

    fn tick(editor: &mut Editor, row: usize, state: bool) {
        if let Some(dialog) = &mut editor.dialog {
            if let Some(Field::Check { on, .. }) = dialog.fields.get_mut(row) {
                *on = state;
            }
        }
    }

    fn accept(editor: &mut Editor) {
        let dialog = editor.dialog.take().expect("a dialog");
        editor.asking = None;
        editor.apply_sort(&dialog);
    }

    /// The first cell of each row of the table.
    fn names(editor: &Editor) -> Vec<String> {
        editor
            .document
            .table_rows_text()
            .into_iter()
            .map(|row| row.first().cloned().unwrap_or_default())
            .collect()
    }

    #[test]
    fn the_dialog_offers_the_columns_of_the_table() {
        let mut editor = editor();
        editor.open_sort();
        let dialog = editor.dialog.as_ref().expect("a dialog");
        match dialog.fields.get(BY) {
            Some(Field::Choice { items, .. }) => {
                assert_eq!(items, &vec!["Column 1".to_owned(), "Column 2".to_owned()]);
            }
            _ => panic!("the first key is not a list"),
        }
    }

    #[test]
    fn a_table_is_sorted_by_the_column_asked_for() {
        let mut editor = editor();
        editor.open_sort();
        accept(&mut editor);
        assert_eq!(names(&editor), vec!["Name", "Apple", "Pear"]);
    }

    #[test]
    fn the_header_row_stays_where_it_is() {
        let mut editor = editor();
        editor.open_sort();
        accept(&mut editor);
        assert_eq!(names(&editor).first().map(String::as_str), Some("Name"));
    }

    #[test]
    fn without_the_tick_the_first_row_is_sorted_with_the_rest() {
        let mut editor = editor();
        editor.open_sort();
        tick(&mut editor, HEADER, false);
        accept(&mut editor);
        assert_eq!(names(&editor), vec!["Apple", "Name", "Pear"]);
    }

    #[test]
    fn the_other_way_round_puts_the_last_first() {
        let mut editor = editor();
        editor.open_sort();
        choose(&mut editor, BY_ORDER, 1);
        accept(&mut editor);
        assert_eq!(names(&editor), vec!["Name", "Pear", "Apple"]);
    }

    #[test]
    fn a_column_of_figures_is_sorted_as_numbers() {
        let mut editor = editor();
        editor.open_sort();
        // The second column, read as numbers: nine before ten, which is not
        // the order the words are in.
        choose(&mut editor, BY, 1);
        choose(&mut editor, BY_KIND, 1);
        accept(&mut editor);
        assert_eq!(names(&editor), vec!["Name", "Apple", "Pear"]);
    }

    #[test]
    fn sorting_a_table_keeps_what_the_cells_were_made_of() {
        // The rows are moved rather than their text rewritten, so everything
        // the cells carry moves with them.
        let mut editor = editor();
        let before = editor.document.save().expect("saving").len();
        editor.open_sort();
        accept(&mut editor);

        let rows = editor.document.table_rows_text();
        assert_eq!(rows.len(), 3, "a row went missing");
        assert!(rows.iter().all(|row| row.len() == 2), "a cell went missing");
        let after = editor.document.save().expect("saving").len();
        assert!(after.abs_diff(before) < before / 4, "the table was rebuilt rather than reordered");
    }

    #[test]
    fn one_undo_puts_the_rows_back() {
        let mut editor = editor();
        let before = names(&editor);
        editor.open_sort();
        accept(&mut editor);
        assert_ne!(names(&editor), before);

        editor.document.undo();
        assert_eq!(names(&editor), before);
    }

    #[test]
    fn paragraphs_are_sorted_where_there_is_no_table() {
        let mut body = Body::default();
        for line in ["Pear", "Apple", "Cherry"] {
            body.blocks.push(Block::Paragraph(Paragraph::text(line)));
        }
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });

        editor.document.set_caret(TextPosition::new(0, 0));
        editor.document.extend_selection_to(TextPosition::new(2, 6));
        editor.open_sort();
        assert!(editor.in_dialog(), "the dialog did not open on a selection");
        tick(&mut editor, HEADER, false);
        accept(&mut editor);

        assert_eq!(editor.document.paragraph_text(0).as_deref(), Some("Apple"));
        assert_eq!(editor.document.paragraph_text(1).as_deref(), Some("Cherry"));
        assert_eq!(editor.document.paragraph_text(2).as_deref(), Some("Pear"));
    }

    #[test]
    fn nothing_to_sort_opens_nothing() {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("Alone")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });

        editor.open_sort();
        assert!(!editor.in_dialog(), "a dialog about nothing opened");
    }
}
