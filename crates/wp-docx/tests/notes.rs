//! Footnotes and endnotes: the part, the mark, and the numbering.

use wp_docx::model::{Block, Body, Paragraph};
use wp_docx::notes::Kind;
use wp_docx::{Document, TextPosition};

fn document(text: &str) -> Document {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text(text)));
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

fn round_trip(document: &Document) -> Document {
    let bytes = document.save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

#[test]
fn a_document_starts_with_no_notes() {
    let document = document("plain");
    assert!(document.notes(Kind::Footnote).is_empty());
    assert!(document.notes(Kind::Endnote).is_empty());
}

#[test]
fn a_footnote_can_be_added_and_reads_back() {
    let mut document = document("A claim.");
    document.set_caret(TextPosition::new(0, 8));
    document.add_note(Kind::Footnote, "Source: somewhere.").expect("adding");

    let notes = round_trip(&document).notes(Kind::Footnote);
    assert_eq!(notes.len(), 1);
    assert!(notes[0].text.ends_with("Source: somewhere."), "got {:?}", notes[0].text);
}

#[test]
fn the_mark_takes_one_place_in_the_text() {
    let mut document = document("ab");
    document.set_caret(TextPosition::new(0, 1));
    document.add_note(Kind::Footnote, "note").expect("adding");

    // One character between the two letters, and the caret after it.
    assert_eq!(document.caret(), TextPosition::new(0, 2));
    assert_eq!(round_trip(&document).paragraph_text(0).as_deref(), Some("a\u{2}b"));
}

#[test]
fn the_mark_is_not_part_of_what_the_document_says() {
    let mut document = document("A claim.");
    document.set_caret(TextPosition::new(0, 8));
    document.add_note(Kind::Footnote, "note").expect("adding");
    assert_eq!(round_trip(&document).plain_text(), "A claim.");
}

#[test]
fn a_note_remembers_where_its_mark_is() {
    let mut document = document("A claim here.");
    document.set_caret(TextPosition::new(0, 7));
    document.add_note(Kind::Footnote, "note").expect("adding");

    let notes = round_trip(&document).notes(Kind::Footnote);
    let mark = notes[0].mark.expect("a mark");
    assert_eq!((mark.paragraph, mark.offset), (0, 7));
}

#[test]
fn notes_are_numbered_in_reading_order_not_by_when_they_were_written() {
    let mut document = document("one two three");
    // The second note is added earlier in the text than the first.
    document.set_caret(TextPosition::new(0, 13));
    document.add_note(Kind::Footnote, "later").expect("adding");
    document.set_caret(TextPosition::new(0, 3));
    document.add_note(Kind::Footnote, "earlier").expect("adding");

    let notes = round_trip(&document).notes(Kind::Footnote);
    assert_eq!(notes.len(), 2);
    assert!(notes[0].text.contains("earlier"), "reading order should come first");
    assert_eq!(notes[0].number, 1);
    assert_eq!(notes[1].number, 2);
}

#[test]
fn footnotes_and_endnotes_are_kept_apart() {
    let mut document = document("text");
    document.set_caret(TextPosition::new(0, 4));
    document.add_note(Kind::Footnote, "a footnote").expect("adding");
    document.add_note(Kind::Endnote, "an endnote").expect("adding");

    let reopened = round_trip(&document);
    assert_eq!(reopened.notes(Kind::Footnote).len(), 1);
    assert_eq!(reopened.notes(Kind::Endnote).len(), 1);
    assert!(reopened.notes(Kind::Footnote)[0].text.contains("a footnote"));
    assert!(reopened.notes(Kind::Endnote)[0].text.contains("an endnote"));
    assert_ne!(
        reopened.notes_part(Kind::Footnote),
        reopened.notes_part(Kind::Endnote),
        "they should be in different parts"
    );
}

#[test]
fn the_part_starts_with_the_two_entries_word_expects() {
    // Number -1 is the separator rule and number 0 the continuation one.
    // Neither is a note, and neither should be listed as one.
    let mut document = document("text");
    document.set_caret(TextPosition::new(0, 4));
    document.add_note(Kind::Footnote, "only note").expect("adding");
    assert_eq!(round_trip(&document).notes(Kind::Footnote).len(), 1);
}

#[test]
fn a_note_can_be_taken_off_again() {
    let mut document = document("A claim.");
    document.set_caret(TextPosition::new(0, 8));
    let id = document.add_note(Kind::Footnote, "note").expect("adding");

    assert!(document.delete_note(Kind::Footnote, id));
    let reopened = round_trip(&document);
    assert!(reopened.notes(Kind::Footnote).is_empty());
    assert_eq!(reopened.paragraph_text(0).as_deref(), Some("A claim."), "the mark should be gone");
}

#[test]
fn deleting_one_note_leaves_the_others_alone() {
    let mut document = document("one two");
    document.set_caret(TextPosition::new(0, 3));
    let first = document.add_note(Kind::Footnote, "first").expect("adding");
    document.set_caret(TextPosition::new(0, 8));
    document.add_note(Kind::Footnote, "second").expect("adding");

    assert!(document.delete_note(Kind::Footnote, first));
    let notes = round_trip(&document).notes(Kind::Footnote);
    assert_eq!(notes.len(), 1);
    assert!(notes[0].text.contains("second"));
}

#[test]
fn deleting_a_note_that_is_not_there_changes_nothing() {
    let mut document = document("plain");
    assert!(!document.delete_note(Kind::Footnote, 9));
}

#[test]
fn adding_a_note_can_be_undone() {
    let mut document = document("A claim.");
    document.set_caret(TextPosition::new(0, 8));
    document.add_note(Kind::Footnote, "note").expect("adding");
    assert!(document.undo());
    assert_eq!(document.paragraph_text(0).as_deref(), Some("A claim."));
}

#[test]
fn typing_after_a_mark_still_lands_where_it_should() {
    let mut document = document("ab");
    document.set_caret(TextPosition::new(0, 1));
    document.add_note(Kind::Footnote, "note").expect("adding");

    // The caret is after the mark; typing goes between the mark and "b".
    assert!(document.type_text("X"));
    assert_eq!(round_trip(&document).plain_text(), "aXb");
}

#[test]
fn a_note_of_several_lines_keeps_them() {
    let mut document = document("text");
    document.set_caret(TextPosition::new(0, 4));
    document.add_note(Kind::Footnote, "first line\nsecond line").expect("adding");
    let text = round_trip(&document).notes(Kind::Footnote)[0].text.clone();
    assert!(text.contains("first line"), "got {text:?}");
    assert!(text.contains("second line"));
}
