//! Blocking other authors out of part of a document.

use wp_docx::model::{Block, Body, Paragraph};
use wp_docx::{Document, TextPosition};

/// Two paragraphs, with the first four characters of the first selected.
fn document() -> Document {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("First paragraph")));
    body.blocks.push(Block::Paragraph(Paragraph::text("Second paragraph")));

    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

fn round_trip(document: &Document) -> Document {
    let bytes = document.save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

fn document_xml(document: &Document) -> String {
    document.package().xml_part("word/document.xml").expect("the document").expect("readable")
}

#[test]
fn nothing_is_locked_to_begin_with() {
    assert!(document().locked_regions().is_empty());
}

#[test]
fn a_selection_can_be_locked_and_reads_back_after_saving() {
    let mut document = document();
    document.set_caret(TextPosition::new(0, 0));
    document.extend_selection_to(TextPosition::new(0, 5));
    assert!(document.block_authors("Ann Roe"));

    let reopened = round_trip(&document);
    let locked = reopened.locked_regions();
    assert_eq!(locked.len(), 1, "{locked:?}");
    assert_eq!(locked[0].editor, "Ann Roe");
    assert_eq!(locked[0].start, TextPosition::new(0, 0));
    assert_eq!(locked[0].end, TextPosition::new(0, 5));
}

#[test]
fn a_lock_is_written_as_the_pair_of_markers_the_format_uses() {
    let mut document = document();
    document.set_caret(TextPosition::new(0, 0));
    document.extend_selection_to(TextPosition::new(0, 5));
    document.block_authors("Ann");

    let part = document_xml(&round_trip(&document));
    assert!(part.contains("permStart"), "no start marker: {part}");
    assert!(part.contains("permEnd"), "no end marker: {part}");
    assert!(part.contains(r#"w:ed="Ann""#), "the marker does not say who: {part}");
}

#[test]
fn a_lock_can_run_from_one_paragraph_into_the_next() {
    let mut document = document();
    document.set_caret(TextPosition::new(0, 6));
    document.extend_selection_to(TextPosition::new(1, 6));
    assert!(document.block_authors("Ann"));

    let reopened = round_trip(&document);
    let locked = reopened.locked_regions();
    assert_eq!(locked.len(), 1, "{locked:?}");
    assert_eq!(locked[0].start, TextPosition::new(0, 6));
    assert_eq!(locked[0].end, TextPosition::new(1, 6));
}

#[test]
fn locking_nothing_locks_nothing() {
    let mut document = document();
    document.set_caret(TextPosition::new(0, 3));
    assert!(!document.block_authors("Ann"), "a caret is not a selection");
    assert!(document.locked_regions().is_empty());
}

#[test]
fn a_lock_needs_somebody_to_lock_it_for() {
    let mut document = document();
    document.set_caret(TextPosition::new(0, 0));
    document.extend_selection_to(TextPosition::new(0, 5));
    assert!(!document.block_authors("   "));
}

#[test]
fn the_caret_knows_whether_it_is_in_a_locked_stretch() {
    let mut document = document();
    document.set_caret(TextPosition::new(0, 0));
    document.extend_selection_to(TextPosition::new(0, 5));
    document.block_authors("Ann");

    let mut reopened = round_trip(&document);
    reopened.set_caret(TextPosition::new(0, 3));
    assert_eq!(reopened.locked_here().map(|locked| locked.editor), Some("Ann".to_owned()));

    reopened.set_caret(TextPosition::new(0, 12));
    assert_eq!(reopened.locked_here(), None);
}

#[test]
fn a_lock_can_be_taken_off_again() {
    let mut document = document();
    document.set_caret(TextPosition::new(0, 0));
    document.extend_selection_to(TextPosition::new(0, 5));
    document.block_authors("Ann");

    let mut reopened = round_trip(&document);
    reopened.set_caret(TextPosition::new(0, 2));
    assert!(reopened.unblock_authors());

    let plain = round_trip(&reopened);
    assert!(plain.locked_regions().is_empty());
    assert!(!document_xml(&plain).contains("permStart"), "the marker is still there");
}

#[test]
fn unlocking_where_nothing_is_locked_does_nothing() {
    let mut document = document();
    document.set_caret(TextPosition::new(0, 2));
    assert!(!document.unblock_authors());
}

#[test]
fn two_stretches_can_be_locked_separately() {
    let mut document = document();
    document.set_caret(TextPosition::new(0, 0));
    document.extend_selection_to(TextPosition::new(0, 5));
    document.block_authors("Ann");

    let mut reopened = round_trip(&document);
    reopened.set_caret(TextPosition::new(1, 0));
    reopened.extend_selection_to(TextPosition::new(1, 6));
    assert!(reopened.block_authors("Ben"));

    let both = round_trip(&reopened);
    let locked = both.locked_regions();
    assert_eq!(locked.len(), 2, "{locked:?}");
    assert_eq!(locked[0].editor, "Ann");
    assert_eq!(locked[1].editor, "Ben");
    // And they carry different numbers, or Word would read them as one.
    assert_ne!(locked[0].id, locked[1].id);
}

#[test]
fn locking_does_not_touch_a_character_of_the_text() {
    let mut document = document();
    document.set_caret(TextPosition::new(0, 3));
    document.extend_selection_to(TextPosition::new(1, 3));
    document.block_authors("Ann");
    assert_eq!(round_trip(&document).plain_text(), "First paragraph\nSecond paragraph");
}

#[test]
fn locking_can_be_undone() {
    let mut document = document();
    document.set_caret(TextPosition::new(0, 0));
    document.extend_selection_to(TextPosition::new(0, 5));
    document.block_authors("Ann");
    assert!(document.undo());
    assert!(document.locked_regions().is_empty());
}
