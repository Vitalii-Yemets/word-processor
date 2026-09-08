//! Putting a table and a picture into a document that had neither.
//!
//! Both are written out, read back through a full save and reopen, and then
//! asked about through the ordinary reading interface — because a drawing that
//! only this program can find is not a picture, it is a private note.

use wp_docx::model::{Block, Body, Paragraph, RunContent};
use wp_docx::{Document, TextPosition, EMU_PER_INCH};

/// A document of one paragraph, saved and reopened so it is a real package.
fn document(text: &str) -> Document {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text(text)));
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

/// Saves and reopens, which is the only proof the XML written is XML that reads.
fn round_trip(document: &Document) -> Document {
    let bytes = document.save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

fn tables_of(document: &Document) -> Vec<wp_docx::model::Table> {
    document
        .body()
        .blocks
        .into_iter()
        .filter_map(|block| match block {
            Block::Table(table) => Some(*table),
            Block::Paragraph(_) => None,
        })
        .collect()
}

#[test]
fn a_table_is_inserted_with_the_rows_and_columns_asked_for() {
    let mut document = document("here");
    assert!(document.insert_table(3, 4));

    let reopened = round_trip(&document);
    let tables = tables_of(&reopened);
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0].rows.len(), 3);
    assert!(tables[0].rows.iter().all(|row| row.cells.len() == 4));
}

#[test]
fn an_inserted_table_carries_a_grid_and_borders() {
    let mut document = document("here");
    assert!(document.insert_table(2, 3));

    let reopened = round_trip(&document);
    let table = tables_of(&reopened).remove(0);
    assert_eq!(table.grid.len(), 3, "one width per column");
    assert!(table.grid.iter().all(|width| *width > 0));
    assert!(table.borders.top.is_some(), "a table with no lines is invisible");
    assert!(table.borders.inside_horizontal.is_some(), "the cells need lines too");
}

#[test]
fn a_table_goes_after_the_paragraph_the_caret_is_in() {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("first")));
    body.blocks.push(Block::Paragraph(Paragraph::text("second")));
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    let mut document = Document::open(&bytes).expect("reopening");

    document.set_caret(TextPosition::new(0, 2));
    assert!(document.insert_table(2, 2));

    let reopened = round_trip(&document);
    let kinds: Vec<&'static str> = reopened
        .body()
        .blocks
        .iter()
        .map(|block| match block {
            Block::Paragraph(_) => "p",
            Block::Table(_) => "table",
        })
        .collect();
    // The paragraph the caret was in, the table, the paragraph that follows a
    // table so there is somewhere to type, then the rest of the document.
    assert_eq!(kinds, ["p", "table", "p", "p"]);
}

#[test]
fn the_caret_lands_in_the_first_cell() {
    let mut document = document("here");
    document.set_caret(TextPosition::new(0, 4));
    assert!(document.insert_table(2, 2));

    // Typing goes into the table, not after it.
    assert!(document.type_text("in"));
    let reopened = round_trip(&document);
    let table = tables_of(&reopened).remove(0);
    let first = &table.rows[0].cells[0];
    let text: String = first
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph(paragraph) => Some(paragraph.plain_text()),
            Block::Table(_) => None,
        })
        .collect();
    assert_eq!(text, "in");
}

#[test]
fn a_picture_is_inserted_and_reads_back_as_one() {
    let png = std::fs::read("../wp-image/tests/fixtures/flat.png").expect("the fixture");
    let mut document = document("here");
    document.set_caret(TextPosition::new(0, 4));
    assert!(document
        .insert_picture(&png, "png", EMU_PER_INCH, EMU_PER_INCH / 2)
        .expect("inserting"));

    let reopened = round_trip(&document);
    let picture = reopened
        .body()
        .blocks
        .into_iter()
        .filter_map(|block| match block {
            Block::Paragraph(paragraph) => Some(paragraph),
            Block::Table(_) => None,
        })
        .flat_map(|paragraph| paragraph.runs)
        .flat_map(|run| run.content)
        .find_map(|content| match content {
            RunContent::Picture(picture) => Some(picture),
            _ => None,
        })
        .expect("a picture in the text");

    assert_eq!(picture.width_emu, EMU_PER_INCH);
    assert_eq!(picture.height_emu, EMU_PER_INCH / 2);
}

#[test]
fn the_bytes_of_an_inserted_picture_are_the_bytes_given() {
    let png = std::fs::read("../wp-image/tests/fixtures/flat.png").expect("the fixture");
    let mut document = document("here");
    document.set_caret(TextPosition::new(0, 4));
    document.insert_picture(&png, "png", EMU_PER_INCH, EMU_PER_INCH).expect("inserting");

    let reopened = round_trip(&document);
    let relationship = reopened
        .body()
        .blocks
        .into_iter()
        .filter_map(|block| match block {
            Block::Paragraph(paragraph) => Some(paragraph),
            Block::Table(_) => None,
        })
        .flat_map(|paragraph| paragraph.runs)
        .flat_map(|run| run.content)
        .find_map(|content| match content {
            RunContent::Picture(picture) => Some(picture.relationship),
            _ => None,
        })
        .expect("a picture that points somewhere");

    assert_eq!(reopened.embedded_part(&relationship), Some(png.as_slice()));
}

#[test]
fn a_picture_takes_one_place_in_the_text_it_was_put_into() {
    let png = std::fs::read("../wp-image/tests/fixtures/flat.png").expect("the fixture");
    let mut document = document("ab");
    document.set_caret(TextPosition::new(0, 1));
    document.insert_picture(&png, "png", EMU_PER_INCH, EMU_PER_INCH).expect("inserting");

    // One character between the two letters, and the caret after it.
    assert_eq!(document.caret(), TextPosition::new(0, 2));
    let reopened = round_trip(&document);
    assert_eq!(reopened.paragraph_text(0).as_deref(), Some("a\u{1}b"));
}

#[test]
fn inserting_a_table_can_be_undone() {
    let mut document = document("here");
    assert!(document.insert_table(2, 2));
    assert!(document.undo());
    assert!(tables_of(&round_trip(&document)).is_empty());
}

#[test]
fn inserting_a_picture_can_be_undone() {
    let png = std::fs::read("../wp-image/tests/fixtures/flat.png").expect("the fixture");
    let mut document = document("here");
    document.set_caret(TextPosition::new(0, 4));
    document.insert_picture(&png, "png", EMU_PER_INCH, EMU_PER_INCH).expect("inserting");
    assert!(document.undo());
    assert_eq!(document.plain_text(), "here");
}
