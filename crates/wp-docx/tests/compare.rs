//! Comparing two documents.

use wp_docx::model::{Block, Body, Paragraph};
use wp_docx::revisions::Decision;
use wp_docx::Document;

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

/// The comparison of two documents, saved and reopened.
fn compared(original: &[&str], revised: &[&str]) -> Document {
    let mut first = document(original);
    let second = document(revised);
    first.compare_with(&second, "Compare");
    round_trip(&first)
}

#[test]
fn two_documents_the_same_have_nothing_to_show() {
    let mut first = document(&["one", "two"]);
    let second = document(&["one", "two"]);
    assert_eq!(first.compare_with(&second, "Compare"), 0);
    assert_eq!(first.revision_count(), 0);
}

#[test]
fn a_word_added_is_marked_as_an_insertion() {
    let result = compared(&["the cat sat"], &["the cat sat down"]);
    assert!(result.revision_count() > 0, "nothing was marked");

    // Accepting gives the new version; rejecting gives the old.
    let mut accepted = round_trip(&result);
    accepted.resolve_all_revisions(Decision::Accept);
    assert_eq!(accepted.plain_text(), "the cat sat down");
}

#[test]
fn rejecting_a_comparison_gives_the_document_back_as_it_was() {
    let result = compared(&["the cat sat"], &["the cat sat down"]);
    let mut rejected = round_trip(&result);
    rejected.resolve_all_revisions(Decision::Reject);
    assert_eq!(rejected.plain_text(), "the cat sat");
}

#[test]
fn a_word_removed_is_marked_as_a_deletion() {
    let result = compared(&["the cat sat down"], &["the cat sat"]);
    let mut accepted = round_trip(&result);
    accepted.resolve_all_revisions(Decision::Accept);
    assert_eq!(accepted.plain_text(), "the cat sat");
}

#[test]
fn a_word_changed_is_marked_both_ways() {
    let result = compared(&["the cat sat"], &["the dog sat"]);

    let mut accepted = round_trip(&result);
    accepted.resolve_all_revisions(Decision::Accept);
    assert_eq!(accepted.plain_text(), "the dog sat");

    let mut rejected = round_trip(&result);
    rejected.resolve_all_revisions(Decision::Reject);
    assert_eq!(rejected.plain_text(), "the cat sat");
}

#[test]
fn a_paragraph_added_is_marked() {
    let result = compared(&["one", "two"], &["one", "two", "three"]);
    let mut accepted = round_trip(&result);
    accepted.resolve_all_revisions(Decision::Accept);
    assert!(accepted.plain_text().contains("three"), "got {:?}", accepted.plain_text());
}

#[test]
fn a_paragraph_removed_is_marked() {
    let result = compared(&["one", "two", "three"], &["one", "three"]);
    let mut accepted = round_trip(&result);
    accepted.resolve_all_revisions(Decision::Accept);
    assert!(!accepted.plain_text().contains("two"), "got {:?}", accepted.plain_text());
}

#[test]
fn the_comparison_is_marked_under_the_name_it_was_given() {
    let mut first = document(&["the cat sat"]);
    let second = document(&["the dog sat"]);
    first.compare_with(&second, "Compare");

    let part = round_trip(&first)
        .package()
        .xml_part("word/document.xml")
        .expect("the document")
        .expect("readable");
    assert!(part.contains(r#"w:author="Compare""#), "got {part}");
}

#[test]
fn comparing_does_not_leave_tracking_switched_on() {
    let mut first = document(&["the cat sat"]);
    let second = document(&["the dog sat"]);
    assert!(!first.tracking_changes());
    first.compare_with(&second, "Compare");
    assert!(
        !first.tracking_changes(),
        "a comparison should not leave the document recording every later edit"
    );
}

#[test]
fn a_whole_comparison_is_one_undo() {
    let mut first = document(&["the cat sat", "and looked"]);
    let second = document(&["the dog sat", "and waited"]);
    first.compare_with(&second, "Compare");
    assert!(first.revision_count() > 0);

    assert!(first.undo());
    assert_eq!(first.revision_count(), 0, "one undo should take the whole comparison back");
    assert_eq!(first.plain_text(), "the cat sat\nand looked");
}

#[test]
fn comparing_with_an_empty_document_marks_everything_as_removed() {
    let result = compared(&["one", "two"], &[""]);
    let mut accepted = round_trip(&result);
    accepted.resolve_all_revisions(Decision::Accept);
    assert!(accepted.plain_text().trim().is_empty(), "got {:?}", accepted.plain_text());
}
