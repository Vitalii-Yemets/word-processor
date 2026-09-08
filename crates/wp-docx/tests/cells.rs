//! Joining and splitting table cells.

use wp_docx::model::{Block, Body, Paragraph};
use wp_docx::{Document, TextPosition};

/// A document with one paragraph and then a table of the given size.
fn document(rows: usize, columns: usize) -> Document {
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

/// The first paragraph index inside the table, which is its first cell.
fn first_cell(document: &Document) -> usize {
    // The paragraph before the table is index 0; the table's cells follow.
    let _ = document;
    1
}

/// Selects from one cell to another by their paragraph indices.
fn select_cells(document: &mut Document, from: usize, to: usize) {
    document.move_caret(TextPosition::new(from, 0), false);
    document.move_caret(TextPosition::new(to, 0), true);
}

#[test]
fn merging_across_a_row_leaves_one_cell_covering_the_others() {
    let mut document = document(2, 3);
    let first = first_cell(&document);
    select_cells(&mut document, first, first + 2);
    assert!(document.merge_cells());

    let table = table_of(&round_trip(&document));
    assert_eq!(table.rows[0].cells.len(), 1, "the row should hold one cell now");
    assert_eq!(table.rows[0].cells[0].span, 3, "and it should cover all three columns");
    assert_eq!(table.rows[1].cells.len(), 3, "the row below is untouched");
}

#[test]
fn merging_down_a_column_leaves_the_lower_cells_continuing_the_first() {
    let mut document = document(3, 2);
    let first = first_cell(&document);
    // Down the first column: cells are numbered along each row, so the cell
    // below the first is two paragraphs on.
    select_cells(&mut document, first, first + 4);
    assert!(document.merge_cells());

    let table = table_of(&round_trip(&document));
    assert_eq!(table.rows.len(), 3);
    assert!(!table.rows[0].cells[0].merged_upwards, "the first starts the merge");
    assert!(table.rows[1].cells[0].merged_upwards, "the second continues it");
    assert!(table.rows[2].cells[0].merged_upwards, "and so does the third");
}

#[test]
fn merging_one_cell_with_itself_does_nothing() {
    let mut document = document(2, 2);
    assert!(!document.merge_cells(), "there is nothing to merge");
}

#[test]
fn a_merged_cell_can_be_split_again() {
    let mut document = document(2, 3);
    let first = first_cell(&document);
    select_cells(&mut document, first, first + 2);
    document.merge_cells();

    document.set_caret(TextPosition::new(first, 0));
    assert!(document.split_cell());

    let table = table_of(&round_trip(&document));
    assert_eq!(table.rows[0].cells.len(), 3);
    assert!(table.rows[0].cells.iter().all(|cell| cell.span == 1));
}

#[test]
fn splitting_a_cell_that_was_never_merged_does_nothing() {
    let mut document = document(2, 2);
    assert!(!document.split_cell());
}

#[test]
fn merging_keeps_the_table_the_width_it_was() {
    let mut document = document(2, 3);
    let before: i32 = table_of(&document).grid.iter().sum();
    let first = first_cell(&document);
    select_cells(&mut document, first, first + 2);
    document.merge_cells();

    let after: i32 = table_of(&round_trip(&document)).grid.iter().sum();
    assert_eq!(before, after, "the grid should not have changed at all");
}

#[test]
fn merging_can_be_undone() {
    let mut document = document(2, 3);
    let first = first_cell(&document);
    select_cells(&mut document, first, first + 2);
    document.merge_cells();
    assert!(document.undo());
    assert_eq!(table_of(&document).rows[0].cells.len(), 3);
}

#[test]
fn columns_can_be_given_equal_widths() {
    let mut document = document(2, 3);
    // Adding a column halves one of them, so they start out uneven.
    document.set_caret(TextPosition::new(first_cell(&document), 0));
    document.insert_table_column(true);

    let uneven = table_of(&document).grid;
    assert!(uneven.iter().any(|width| *width != uneven[0]), "they should start uneven");

    assert!(document.distribute_columns());
    let even = table_of(&round_trip(&document)).grid;
    assert!(even.iter().all(|width| (*width - even[0]).abs() <= 1), "got {even:?}");
}

#[test]
fn nothing_happens_when_the_caret_is_not_in_a_table() {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("plain")));
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    let mut document = Document::open(&bytes).expect("reopening");

    assert!(!document.merge_cells());
    assert!(!document.split_cell());
    assert!(!document.distribute_columns());
    assert!(document.selected_cells().is_none());
}
