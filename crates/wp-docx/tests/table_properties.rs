//! The properties of a table, its rows and its cells.

use wp_docx::model::{Alignment, Block, Body, Paragraph, Table, TableCell, TableRow};
use wp_docx::table_properties::CellAlignment;
use wp_docx::{Document, TextPosition};

/// A document holding one table of two rows and two columns.
fn document() -> Document {
    let cell = |text: &str| TableCell {
        blocks: vec![Block::Paragraph(Paragraph::text(text))],
        ..TableCell::default()
    };
    let row = |first: &str, second: &str| TableRow {
        cells: vec![cell(first), cell(second)],
        ..TableRow::default()
    };

    let mut body = Body::default();
    body.blocks.push(Block::Table(Box::new(Table {
        rows: vec![row("Name", "Town"), row("Ann", "Leeds")],
        ..Table::default()
    })));
    body.blocks.push(Block::Paragraph(Paragraph::text("after")));

    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    let mut document = Document::open(&bytes).expect("reopening");
    // The caret goes into the first cell, which is what every table command
    // works from.
    document.set_caret(TextPosition::new(0, 0));
    document
}

fn round_trip(document: &Document) -> Document {
    let bytes = document.save().expect("saving");
    let mut reopened = Document::open(&bytes).expect("reopening");
    reopened.set_caret(TextPosition::new(0, 0));
    reopened
}

#[test]
fn the_caret_is_in_a_table_to_begin_with() {
    assert!(document().table_here().is_some(), "the test needs the caret in the table");
}

#[test]
fn a_table_has_no_alignment_of_its_own_until_one_is_set() {
    assert_eq!(document().table_alignment(), None);
}

#[test]
fn a_table_can_be_centred_and_the_setting_survives_saving() {
    let mut document = document();
    assert!(document.set_table_alignment(Alignment::Center));
    assert_eq!(round_trip(&document).table_alignment(), Some(Alignment::Center));
}

#[test]
fn a_first_row_is_not_a_header_until_it_is_made_one() {
    assert_eq!(document().table_header_row(), Some(false));
}

#[test]
fn a_first_row_can_be_made_a_header_and_reads_back() {
    let mut document = document();
    assert!(document.set_table_header_row(true));

    let reopened = round_trip(&document);
    assert_eq!(reopened.table_header_row(), Some(true));

    // And the model says so too, which is what the accessibility check reads.
    let Block::Table(table) = &reopened.body().blocks[0] else { panic!("a table") };
    assert!(table.rows[0].is_header);
}

#[test]
fn a_header_row_can_be_made_an_ordinary_row_again() {
    let mut document = document();
    document.set_table_header_row(true);
    let mut reopened = round_trip(&document);
    assert!(reopened.set_table_header_row(false));
    assert_eq!(round_trip(&reopened).table_header_row(), Some(false));
}

#[test]
fn a_row_has_no_height_of_its_own_until_one_is_set() {
    assert_eq!(document().table_row_height(), None);
}

#[test]
fn a_row_height_is_written_and_reads_back() {
    let mut document = document();
    assert!(document.set_table_row_height(Some(720), false));
    assert_eq!(round_trip(&document).table_row_height(), Some(720));
}

#[test]
fn a_row_height_is_a_least_rather_than_an_exact_measure() {
    let mut document = document();
    document.set_table_row_height(Some(720), false);

    let part = round_trip(&document)
        .package()
        .xml_part("word/document.xml")
        .expect("the document")
        .expect("readable");
    assert!(
        part.contains(r#"w:hRule="atLeast""#),
        "text that does not fit an exact height is cut off: {part}"
    );
}

#[test]
fn a_row_can_be_let_find_its_own_height_again() {
    let mut document = document();
    document.set_table_row_height(Some(720), false);
    let mut reopened = round_trip(&document);
    assert!(reopened.set_table_row_height(None, false));
    assert_eq!(round_trip(&reopened).table_row_height(), None);
}

#[test]
fn text_sits_at_the_top_of_a_cell_until_it_is_told_otherwise() {
    assert_eq!(document().cell_alignment(), Some(CellAlignment::Top));
}

#[test]
fn every_cell_alignment_is_written_and_reads_back() {
    for alignment in [CellAlignment::Middle, CellAlignment::Bottom] {
        let mut document = document();
        assert!(document.set_cell_alignment(alignment), "{}", alignment.label());
        assert_eq!(round_trip(&document).cell_alignment(), Some(alignment));
    }
}

#[test]
fn going_back_to_the_top_writes_nothing_rather_than_the_word_top() {
    let mut document = document();
    document.set_cell_alignment(CellAlignment::Middle);
    let mut reopened = round_trip(&document);
    reopened.set_cell_alignment(CellAlignment::Top);

    let part = round_trip(&reopened)
        .package()
        .xml_part("word/document.xml")
        .expect("the document")
        .expect("readable");
    assert!(!part.contains("vAlign"), "the top is what a cell does anyway: {part}");
}

#[test]
fn none_of_these_touch_a_single_character_of_the_text() {
    let mut document = document();
    document.set_table_alignment(Alignment::Center);
    document.set_table_header_row(true);
    document.set_table_row_height(Some(720), false);
    document.set_cell_alignment(CellAlignment::Middle);

    // Cells of a row are separated by a tab, which is how a table reads as
    // plain text.
    assert_eq!(round_trip(&document).plain_text(), "Name\tTown\nAnn\tLeeds\nafter");
}

#[test]
fn a_caret_outside_a_table_changes_nothing() {
    let mut document = document();
    // The paragraph after the table.
    document.set_caret(TextPosition::new(4, 0));
    assert!(!document.set_table_header_row(true));
    assert_eq!(document.table_alignment(), None);
}

#[test]
fn changing_a_table_property_can_be_undone() {
    let mut document = document();
    document.set_table_alignment(Alignment::Center);
    assert!(document.undo());
    assert_eq!(document.table_alignment(), None);
}
