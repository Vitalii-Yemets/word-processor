//! Comments: the part that holds them, and the anchors that place them.

use wp_docx::model::{Block, Body, Paragraph};
use wp_docx::{Document, TextPosition};

const WHEN: &str = "2026-09-07T23:00:00Z";

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

/// Selects a stretch of the first paragraph.
fn select(document: &mut Document, from: usize, to: usize) {
    document.move_caret(TextPosition::new(0, from), false);
    document.move_caret(TextPosition::new(0, to), true);
}

#[test]
fn a_document_starts_with_no_comments() {
    assert!(document("plain text").comments().is_empty());
}

#[test]
fn a_comment_can_be_added_and_reads_back() {
    let mut document = document("the quick brown fox");
    select(&mut document, 4, 9);
    document.add_comment("Is it though?", "Ada Lovelace", WHEN).expect("adding");

    let comments = round_trip(&document).comments();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].text, "Is it though?");
    assert_eq!(comments[0].author, "Ada Lovelace");
    assert_eq!(comments[0].initials, "AL");
}

#[test]
fn a_comment_remembers_the_stretch_it_is_about() {
    let mut document = document("the quick brown fox");
    select(&mut document, 4, 9);
    document.add_comment("note", "Ada", WHEN).expect("adding");

    let comments = round_trip(&document).comments();
    let (start, end) = comments[0].range.expect("a range");
    assert_eq!((start.paragraph, start.offset), (0, 4));
    assert_eq!((end.paragraph, end.offset), (0, 9));
}

#[test]
fn commenting_does_not_change_a_single_character_of_the_text() {
    // The anchors carry no characters, which is the whole reason a comment can
    // be attached without moving every caret position after it.
    let mut document = document("the quick brown fox");
    let before = document.plain_text();
    select(&mut document, 4, 9);
    document.add_comment("note", "Ada", WHEN).expect("adding");

    let reopened = round_trip(&document);
    assert_eq!(reopened.plain_text(), before);
    assert_eq!(reopened.paragraph_text(0).as_deref(), Some("the quick brown fox"));
}

#[test]
fn several_comments_each_get_their_own_number() {
    let mut document = document("one two three four five");
    select(&mut document, 0, 3);
    let first = document.add_comment("first", "Ada", WHEN).expect("adding");
    select(&mut document, 8, 13);
    let second = document.add_comment("second", "Ada", WHEN).expect("adding");

    assert_ne!(first, second);
    let comments = round_trip(&document).comments();
    assert_eq!(comments.len(), 2);
    assert_eq!(comments[0].text, "first", "they should be listed in reading order");
    assert_eq!(comments[1].text, "second");
}

#[test]
fn a_comment_can_be_taken_off_again() {
    let mut document = document("the quick brown fox");
    select(&mut document, 4, 9);
    let id = document.add_comment("note", "Ada", WHEN).expect("adding");

    assert!(document.delete_comment(id));
    let reopened = round_trip(&document);
    assert!(reopened.comments().is_empty());
    assert_eq!(reopened.paragraph_text(0).as_deref(), Some("the quick brown fox"));
}

#[test]
fn deleting_one_comment_leaves_the_others_alone() {
    let mut document = document("one two three four five");
    select(&mut document, 0, 3);
    let first = document.add_comment("first", "Ada", WHEN).expect("adding");
    select(&mut document, 8, 13);
    document.add_comment("second", "Ada", WHEN).expect("adding");

    assert!(document.delete_comment(first));
    let comments = round_trip(&document).comments();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].text, "second");
    assert_eq!(comments[0].range.expect("a range").0.offset, 8);
}

#[test]
fn deleting_a_comment_that_is_not_there_changes_nothing() {
    let mut document = document("plain");
    assert!(!document.delete_comment(7));
}

#[test]
fn a_comment_with_no_selection_is_attached_at_the_caret() {
    let mut document = document("plain text");
    document.set_caret(TextPosition::new(0, 5));
    document.add_comment("here", "Ada", WHEN).expect("adding");

    let comments = round_trip(&document).comments();
    let (start, end) = comments[0].range.expect("a range");
    assert_eq!(start, end);
    assert_eq!(start.offset, 5);
}

#[test]
fn a_comment_of_several_lines_keeps_them() {
    let mut document = document("plain text");
    select(&mut document, 0, 5);
    document.add_comment("first line\nsecond line", "Ada", WHEN).expect("adding");
    assert_eq!(round_trip(&document).comments()[0].text, "first line\nsecond line");
}

#[test]
fn adding_a_comment_can_be_undone() {
    let mut document = document("the quick brown fox");
    select(&mut document, 4, 9);
    document.add_comment("note", "Ada", WHEN).expect("adding");
    assert!(document.undo());
    // Undo puts the document's own tree back; the anchors go with it.
    assert!(document.comments().iter().all(|comment| comment.range.is_none()));
}

#[test]
fn text_before_and_after_the_anchors_still_reads_as_one_run_of_text() {
    let mut document = document("aaa bbb ccc");
    select(&mut document, 4, 7);
    document.add_comment("middle", "Ada", WHEN).expect("adding");

    // The run holding "aaa bbb ccc" is cut in three by the two anchors; the
    // text has to come back out identical anyway.
    let reopened = round_trip(&document);
    assert_eq!(reopened.paragraph_text(0).as_deref(), Some("aaa bbb ccc"));
    assert_eq!(reopened.plain_text(), "aaa bbb ccc");
}

#[test]
fn typing_after_a_comment_still_lands_where_it_should() {
    let mut document = document("aaa bbb ccc");
    select(&mut document, 4, 7);
    document.add_comment("middle", "Ada", WHEN).expect("adding");

    document.set_caret(TextPosition::new(0, 11));
    assert!(document.type_text("!"));
    assert_eq!(round_trip(&document).paragraph_text(0).as_deref(), Some("aaa bbb ccc!"));
}
