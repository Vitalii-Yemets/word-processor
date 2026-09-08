//! Editing a part of the package other than the document: a header or a footer.

use wp_docx::furniture::Furniture;
use wp_docx::model::{Block, Body, Paragraph};
use wp_docx::{Document, TextPosition};

/// A document with a header on it.
fn document() -> Document {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("The body of the document")));

    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    let mut document = Document::open(&bytes).expect("reopening");
    document
        .set_furniture(
            Furniture::Header,
            wp_docx::furniture::Preset::Text,
            wp_docx::model::Alignment::Start,
            "Title",
        )
        .expect("a header");

    let bytes = document.save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

fn round_trip(document: &Document) -> Document {
    let bytes = document.save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

#[test]
fn a_document_is_editing_itself_to_begin_with() {
    assert_eq!(document().part_being_edited(), None);
}

#[test]
fn a_header_can_be_entered_and_says_so() {
    let mut document = document();
    let part = document.furniture_part(Furniture::Header).expect("a header part");
    assert!(document.enter_part(&part));
    assert_eq!(document.part_being_edited(), Some(part.as_str()));
}

#[test]
fn inside_a_header_the_text_is_the_headers() {
    let mut document = document();
    let part = document.furniture_part(Furniture::Header).expect("a header part");
    let body = document.plain_text();
    document.enter_part(&part);

    assert_ne!(document.plain_text(), body, "the body is still what is being edited");
    assert!(document.plain_text().contains("Title"), "{}", document.plain_text());
}

#[test]
fn typing_inside_a_header_changes_the_header_and_not_the_body() {
    let mut document = document();
    let part = document.furniture_part(Furniture::Header).expect("a header part");
    document.enter_part(&part);
    document.set_caret(TextPosition::new(0, 0));
    document.type_text("New ");
    document.leave_part();

    let reopened = round_trip(&document);
    assert_eq!(reopened.plain_text(), "The body of the document", "the body was changed");
    let header = reopened.furniture(Furniture::Header).expect("a header");
    let text: String = header.blocks.iter().map(Block::plain_text).collect::<Vec<_>>().join("\n");
    assert!(text.contains("New "), "the header was not changed: {text:?}");
}

#[test]
fn leaving_puts_the_document_back() {
    let mut document = document();
    let part = document.furniture_part(Furniture::Header).expect("a header part");
    document.enter_part(&part);
    assert!(document.leave_part());
    assert_eq!(document.part_being_edited(), None);
    assert_eq!(document.plain_text(), "The body of the document");
}

#[test]
fn leaving_when_nothing_was_entered_does_nothing() {
    assert!(!document().leave_part());
}

#[test]
fn a_part_that_is_not_there_is_not_entered() {
    assert!(!document().enter_part("word/nothing.xml"));
}

#[test]
fn undo_comes_back_to_the_part_the_change_was_made_in() {
    // The behaviour that matters: edit the header, come out, press undo, and
    // the header edit is taken back — which means going back into the header.
    let mut document = document();
    let part = document.furniture_part(Furniture::Header).expect("a header part");
    document.enter_part(&part);
    document.set_caret(TextPosition::new(0, 0));
    document.type_text("New ");
    document.leave_part();

    assert!(document.undo());
    assert_eq!(document.part_being_edited(), Some(part.as_str()), "undo did not go back");
    assert!(!document.plain_text().contains("New "), "the change was not taken back");
}

#[test]
fn a_change_in_the_body_undoes_in_the_body() {
    let mut document = document();
    document.set_caret(TextPosition::new(0, 0));
    document.type_text("A ");
    assert!(document.undo());
    assert_eq!(document.part_being_edited(), None);
    assert_eq!(document.plain_text(), "The body of the document");
}

#[test]
fn what_was_typed_in_a_header_survives_being_saved() {
    let mut document = document();
    let part = document.furniture_part(Furniture::Header).expect("a header part");
    document.enter_part(&part);
    document.set_caret(TextPosition::new(0, 0));
    document.type_text("Kept ");
    document.leave_part();

    // Saved and reopened twice, because the part has to reach the package on
    // the way out and stay there.
    let once = round_trip(&document);
    let twice = round_trip(&once);
    let header = twice.furniture(Furniture::Header).expect("a header");
    let text: String = header.blocks.iter().map(Block::plain_text).collect::<Vec<_>>().join("\n");
    assert!(text.contains("Kept "), "{text:?}");
}
