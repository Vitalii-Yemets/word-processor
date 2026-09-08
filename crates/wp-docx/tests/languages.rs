//! The proofing language a stretch of text is marked as.

use wp_docx::languages::DEFAULT_TAG;
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

#[test]
fn text_that_says_nothing_falls_back_to_the_default() {
    let mut document = document(&["plain"]);
    document.set_caret(TextPosition::new(0, 0));
    assert_eq!(document.language_here(), DEFAULT_TAG);
}

#[test]
fn a_language_can_be_set_and_reads_back_after_saving() {
    let mut document = document(&["plain"]);
    select(&mut document, 0, 0, 5);
    assert!(document.set_language("ru-RU"));

    let mut reopened = round_trip(&document);
    reopened.set_caret(TextPosition::new(0, 2));
    assert_eq!(reopened.language_here(), "ru-RU");
}

#[test]
fn one_document_can_hold_two_languages() {
    let mut document = document(&["one two"]);
    select(&mut document, 0, 0, 3);
    document.set_language("en-GB");
    select(&mut document, 0, 4, 7);
    document.set_language("fr-FR");

    let mut reopened = round_trip(&document);
    reopened.set_caret(TextPosition::new(0, 1));
    assert_eq!(reopened.language_here(), "en-GB");
    reopened.set_caret(TextPosition::new(0, 5));
    assert_eq!(reopened.language_here(), "fr-FR", "the second half is French");
}

#[test]
fn the_language_is_written_where_word_looks_for_it() {
    let mut document = document(&["plain"]);
    select(&mut document, 0, 0, 5);
    document.set_language("de-DE");

    let reopened = round_trip(&document);
    let part =
        reopened.package().xml_part("word/document.xml").expect("the document").expect("readable");
    assert!(part.contains(r#"<w:lang "#), "there is no language element at all: {part}");
    assert!(part.contains(r#"w:val="de-DE""#), "got {part}");
}

#[test]
fn setting_the_language_changes_not_one_character_of_the_text() {
    let mut document = document(&["plain"]);
    select(&mut document, 0, 0, 5);
    document.set_language("ru-RU");
    assert_eq!(round_trip(&document).plain_text(), "plain");
}

#[test]
fn setting_the_language_can_be_undone() {
    let mut document = document(&["plain"]);
    select(&mut document, 0, 0, 5);
    document.set_language("ru-RU");
    assert!(document.undo());

    document.set_caret(TextPosition::new(0, 2));
    assert_eq!(document.language_here(), DEFAULT_TAG);
}

#[test]
fn a_language_chosen_with_nothing_selected_applies_to_what_is_typed_next() {
    let mut document = document(&["one "]);
    document.set_caret(TextPosition::new(0, 4));
    document.set_language("ru-RU");
    assert!(document.type_text("два"));

    let mut reopened = round_trip(&document);
    reopened.set_caret(TextPosition::new(0, 6));
    assert_eq!(reopened.language_here(), "ru-RU");
}
