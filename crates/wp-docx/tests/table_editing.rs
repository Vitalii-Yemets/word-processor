//! Adding and removing the rows and columns of a table that already exists.
//!
//! Every one of these goes through a full save and reopen, because the thing
//! that matters is not what the tree looks like in memory — it is whether the
//! file that comes out is one that reads back as the table intended.

use wp_docx::model::{Block, Body, Paragraph, TableBorders};
use wp_docx::{Document, TextPosition};

/// A document holding one paragraph and then a table of the given size.
fn document_with_table(rows: usize, columns: usize) -> Document {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("before")));
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    let mut document = Document::open(&bytes).expect("reopening");

    document.set_caret(TextPosition::new(0, 6));
    assert!(document.insert_table(rows, columns));
    document
}

fn round_trip(document: &Document) -> Document {
    let bytes = document.save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

fn table_of(document: &Document) -> wp_docx::model::Table {
    document
        .body()
        .blocks
        .into_iter()
        .find_map(|block| match block {
            Block::Table(table) => Some(*table),
            Block::Paragraph(_) => None,
        })
        .expect("a table")
}

/// Rows and columns of the table, as a shape to compare.
fn shape(document: &Document) -> (usize, Vec<usize>) {
    let table = table_of(document);
    (table.rows.len(), table.rows.iter().map(|row| row.cells.len()).collect())
}

#[test]
fn the_caret_knows_which_cell_it_is_in() {
    let document = document_with_table(3, 4);
    // insert_table leaves the caret in the first cell.
    let place = document.table_here().expect("the caret is in the table");
    assert_eq!((place.row, place.column), (0, 0));
    assert_eq!((place.rows, place.columns), (3, 4));
}

#[test]
fn a_caret_outside_a_table_is_not_in_one() {
    let document = document_with_table(2, 2);
    let mut document = document;
    document.set_caret(TextPosition::new(0, 0));
    assert!(document.table_here().is_none(), "the paragraph before the table is not in it");
}

#[test]
fn a_row_can_be_added_below_the_one_the_caret_is_in() {
    let mut document = document_with_table(2, 3);
    assert!(document.insert_table_row(true));
    assert_eq!(shape(&round_trip(&document)), (3, vec![3, 3, 3]));
}

#[test]
fn a_row_can_be_added_above_it() {
    let mut document = document_with_table(2, 3);
    assert!(document.insert_table_row(false));
    let reopened = round_trip(&document);
    assert_eq!(shape(&reopened), (3, vec![3, 3, 3]));
}

#[test]
fn an_added_row_is_empty_even_when_the_one_it_was_copied_from_is_not() {
    let mut document = document_with_table(2, 2);
    assert!(document.type_text("filled"));
    assert!(document.insert_table_row(true));

    let table = table_of(&round_trip(&document));
    let second = &table.rows[1];
    let text: String = second
        .cells
        .iter()
        .flat_map(|cell| cell.blocks.iter())
        .map(wp_docx::model::Block::plain_text)
        .collect();
    assert!(text.trim().is_empty(), "the new row came out holding {text:?}");
}

#[test]
fn a_column_can_be_added_and_every_row_gains_a_cell() {
    let mut document = document_with_table(3, 2);
    assert!(document.insert_table_column(true));

    let reopened = round_trip(&document);
    assert_eq!(shape(&reopened), (3, vec![3, 3, 3]));
    assert_eq!(table_of(&reopened).grid.len(), 3, "the grid gains a column too");
}

#[test]
fn adding_a_column_keeps_the_table_the_width_it_was() {
    let mut document = document_with_table(2, 2);
    let before: i32 = table_of(&document).grid.iter().sum();
    assert!(document.insert_table_column(true));
    let after: i32 = table_of(&round_trip(&document)).grid.iter().sum();
    assert_eq!(before, after);
}

#[test]
fn a_row_can_be_taken_out() {
    let mut document = document_with_table(3, 2);
    assert!(document.delete_table_row());
    assert_eq!(shape(&round_trip(&document)), (2, vec![2, 2]));
}

#[test]
fn a_column_can_be_taken_out() {
    let mut document = document_with_table(2, 3);
    assert!(document.delete_table_column());
    let reopened = round_trip(&document);
    assert_eq!(shape(&reopened), (2, vec![2, 2]));
    assert_eq!(table_of(&reopened).grid.len(), 2);
}

#[test]
fn removing_the_last_row_removes_the_table() {
    // A table with no rows is a file Word refuses to open, so the whole table
    // goes rather than being left in that state.
    let mut document = document_with_table(1, 2);
    assert!(document.delete_table_row());

    let reopened = round_trip(&document);
    assert!(
        reopened.body().blocks.iter().all(|block| matches!(block, Block::Paragraph(_))),
        "the table should be gone"
    );
}

#[test]
fn removing_the_last_column_removes_the_table() {
    let mut document = document_with_table(2, 1);
    assert!(document.delete_table_column());
    let reopened = round_trip(&document);
    assert!(reopened.body().blocks.iter().all(|block| matches!(block, Block::Paragraph(_))));
}

#[test]
fn a_table_can_be_removed_outright() {
    let mut document = document_with_table(3, 3);
    assert!(document.delete_table());
    let reopened = round_trip(&document);
    assert!(reopened.body().blocks.iter().all(|block| matches!(block, Block::Paragraph(_))));
}

#[test]
fn the_caret_still_points_at_something_after_a_table_is_removed() {
    let mut document = document_with_table(2, 2);
    assert!(document.delete_table());
    let caret = document.caret();
    assert!(caret.paragraph < document.paragraph_count(), "the caret is off the end");
    let length = document.paragraph_text(caret.paragraph).map_or(0, |text| text.len());
    assert!(caret.offset <= length, "the caret is past the end of its paragraph");
}

#[test]
fn the_lines_a_table_draws_can_be_changed() {
    let mut document = document_with_table(2, 2);
    assert!(document.set_table_borders(&TableBorders::default()));
    let table = table_of(&round_trip(&document));
    assert!(table.borders.top.is_none(), "the borders should have been taken off");

    assert!(document.set_table_borders(&TableBorders::grid()));
    let table = table_of(&round_trip(&document));
    assert!(table.borders.top.is_some(), "and put back");
}

#[test]
fn editing_a_table_can_be_undone() {
    let mut document = document_with_table(2, 2);
    assert!(document.insert_table_row(true));
    assert_eq!(shape(&document).0, 3);
    assert!(document.undo());
    assert_eq!(shape(&document).0, 2);
}

#[test]
fn nothing_happens_when_the_caret_is_not_in_a_table() {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("plain")));
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    let mut document = Document::open(&bytes).expect("reopening");

    assert!(!document.insert_table_row(true));
    assert!(!document.insert_table_column(true));
    assert!(!document.delete_table_row());
    assert!(!document.delete_table());
}
