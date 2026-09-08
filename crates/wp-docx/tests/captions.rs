//! Bookmarks, captions and the cross-references that point at them.

use wp_docx::captions::{Label, Reference};
use wp_docx::model::{Block, Body, Paragraph};
use wp_docx::{Document, TextPosition};

fn document(lines: &[&str]) -> Document {
    let mut body = Body::default();
    for line in lines {
        body.blocks.push(Block::Paragraph(Paragraph::text(line)));
    }
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

fn round_trip(document: &Document) -> Document {
    let bytes = document.save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

fn select(document: &mut Document, paragraph: usize, from: usize, to: usize) {
    document.move_caret(TextPosition::new(paragraph, from), false);
    document.move_caret(TextPosition::new(paragraph, to), true);
}

// --- Bookmarks ----------------------------------------------------------------

#[test]
fn a_document_starts_with_no_bookmarks() {
    assert!(document(&["plain"]).bookmarks().is_empty());
}

#[test]
fn a_bookmark_can_be_added_and_reads_back() {
    let mut document = document(&["the quick brown fox"]);
    select(&mut document, 0, 4, 9);
    assert!(document.add_bookmark("Speed"));

    let reopened = round_trip(&document);
    let marks = reopened.bookmarks();
    assert_eq!(marks.len(), 1);
    assert_eq!(marks[0].name, "Speed");
    assert_eq!(reopened.bookmark_text("Speed").as_deref(), Some("quick"));
}

#[test]
fn a_bookmark_changes_not_one_character_of_the_text() {
    let mut document = document(&["the quick brown fox"]);
    select(&mut document, 0, 4, 9);
    document.add_bookmark("Speed");

    let reopened = round_trip(&document);
    assert_eq!(reopened.paragraph_text(0).as_deref(), Some("the quick brown fox"));
    assert_eq!(reopened.plain_text(), "the quick brown fox");
}

#[test]
fn using_a_name_twice_moves_the_bookmark_rather_than_duplicating_it() {
    let mut document = document(&["one two three"]);
    select(&mut document, 0, 0, 3);
    document.add_bookmark("Here");
    select(&mut document, 0, 8, 13);
    document.add_bookmark("Here");

    let reopened = round_trip(&document);
    assert_eq!(reopened.bookmarks().len(), 1);
    assert_eq!(reopened.bookmark_text("Here").as_deref(), Some("three"));
}

#[test]
fn a_bookmark_can_be_taken_away() {
    let mut document = document(&["the quick brown fox"]);
    select(&mut document, 0, 4, 9);
    document.add_bookmark("Speed");
    assert!(document.remove_bookmark("Speed"));

    let reopened = round_trip(&document);
    assert!(reopened.bookmarks().is_empty());
    assert_eq!(reopened.paragraph_text(0).as_deref(), Some("the quick brown fox"));
}

#[test]
fn removing_one_that_is_not_there_changes_nothing() {
    let mut document = document(&["plain"]);
    assert!(!document.remove_bookmark("Nowhere"));
}

#[test]
fn a_bookmark_can_be_added_without_a_selection() {
    let mut document = document(&["plain text"]);
    document.set_caret(TextPosition::new(0, 5));
    assert!(document.add_bookmark("Point"));
    let mark = round_trip(&document).bookmark("Point").expect("a bookmark");
    assert_eq!(mark.range.0, mark.range.1, "it covers nothing");
}

// --- Captions -----------------------------------------------------------------

#[test]
fn a_caption_goes_under_the_paragraph_the_caret_is_in() {
    let mut document = document(&["A picture would go here.", "Next paragraph."]);
    document.set_caret(TextPosition::new(0, 0));
    assert!(document.add_caption(Label::Figure, "The map"));

    let reopened = round_trip(&document);
    assert_eq!(reopened.paragraph_count(), 3);
    let caption = reopened.paragraph_text(1).unwrap_or_default();
    assert!(caption.starts_with("Figure "), "got {caption:?}");
    assert!(caption.ends_with(": The map"), "got {caption:?}");
}

#[test]
fn the_number_in_a_caption_is_a_field_and_not_a_typed_digit() {
    let mut document = document(&["A picture."]);
    document.set_caret(TextPosition::new(0, 0));
    document.add_caption(Label::Figure, "The map");

    let reopened = round_trip(&document);
    let Block::Paragraph(caption) = &reopened.body().blocks[1] else { panic!("a paragraph") };
    let instruction = caption
        .runs
        .iter()
        .find_map(|run| run.field.as_deref())
        .expect("the number should be a field");
    assert!(instruction.starts_with("SEQ Figure"), "got {instruction:?}");
}

#[test]
fn captions_are_counted_in_reading_order() {
    let mut document = document(&["First.", "Second.", "Third."]);
    // Added back to front; the numbers still come out in reading order.
    document.set_caret(TextPosition::new(2, 0));
    document.add_caption(Label::Figure, "third");
    document.set_caret(TextPosition::new(0, 0));
    document.add_caption(Label::Figure, "first");

    let captions = round_trip(&document).captions();
    assert_eq!(captions.len(), 2);
    assert_eq!(captions[0].number, 1);
    assert!(captions[0].text.contains("first"));
    assert_eq!(captions[1].number, 2);
    assert!(captions[1].text.contains("third"));
}

#[test]
fn each_label_counts_its_own_sequence() {
    let mut document = document(&["One.", "Two."]);
    document.set_caret(TextPosition::new(0, 0));
    document.add_caption(Label::Figure, "a figure");
    document.set_caret(TextPosition::new(2, 0));
    document.add_caption(Label::Table, "a table");

    let captions = round_trip(&document).captions();
    let figure = captions.iter().find(|caption| caption.label == Label::Figure).expect("a figure");
    let table = captions.iter().find(|caption| caption.label == Label::Table).expect("a table");
    assert_eq!(figure.number, 1);
    assert_eq!(table.number, 1, "a table is not counted as a figure");
}

#[test]
fn a_caption_is_named_so_it_can_be_pointed_at() {
    let mut document = document(&["A picture."]);
    document.set_caret(TextPosition::new(0, 0));
    document.add_caption(Label::Figure, "The map");

    let caption = round_trip(&document).captions().remove(0);
    let name = caption.bookmark.expect("a caption should be named");
    assert!(round_trip(&document).bookmark(&name).is_some());
}

// --- Cross-references ---------------------------------------------------------

#[test]
fn a_cross_reference_shows_the_text_it_points_at() {
    let mut document = document(&["A heading here", "See "]);
    select(&mut document, 0, 0, 14);
    document.add_bookmark("Target");

    document.set_caret(TextPosition::new(1, 4));
    assert!(document.add_cross_reference("Target", Reference::Text));

    let reopened = round_trip(&document);
    assert!(
        reopened.paragraph_text(1).unwrap_or_default().contains("A heading here"),
        "got {:?}",
        reopened.paragraph_text(1)
    );
}

#[test]
fn a_cross_reference_is_a_field_so_it_can_be_worked_out_again() {
    let mut document = document(&["A heading here", "See "]);
    select(&mut document, 0, 0, 14);
    document.add_bookmark("Target");
    document.set_caret(TextPosition::new(1, 4));
    document.add_cross_reference("Target", Reference::Text);

    let reopened = round_trip(&document);
    let Block::Paragraph(paragraph) = &reopened.body().blocks[1] else { panic!("a paragraph") };
    let instruction = paragraph.runs.iter().find_map(|run| run.field.as_deref()).expect("a field");
    assert!(instruction.starts_with("REF Target"), "got {instruction:?}");
}

#[test]
fn a_page_reference_asks_for_a_page_rather_than_the_text() {
    let mut document = document(&["A heading here", "See page "]);
    select(&mut document, 0, 0, 14);
    document.add_bookmark("Target");
    document.set_caret(TextPosition::new(1, 9));
    document.add_cross_reference("Target", Reference::Page);

    let reopened = round_trip(&document);
    let Block::Paragraph(paragraph) = &reopened.body().blocks[1] else { panic!("a paragraph") };
    let instruction = paragraph.runs.iter().find_map(|run| run.field.as_deref()).expect("a field");
    assert!(instruction.starts_with("PAGEREF Target"), "got {instruction:?}");
}

#[test]
fn adding_a_caption_can_be_undone() {
    let mut document = document(&["A picture."]);
    let before = document.paragraph_count();
    document.set_caret(TextPosition::new(0, 0));
    document.add_caption(Label::Figure, "The map");
    assert!(document.undo());
    assert_eq!(document.paragraph_count(), before);
}
