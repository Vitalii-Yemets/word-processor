//! Mail merge: the letter, the list, and what happens when the two meet.

use wp_docx::merge::{merge_instruction, Kind, Recipients};
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

fn list() -> Recipients {
    Recipients::parse(b"Name,Town\nAnn,Leeds\nJohn,Hull\n")
}

/// A letter saying "Dear «Name» of «Town»".
fn letter() -> Document {
    let mut document = document(&["Dear "]);
    document.set_caret(TextPosition::new(0, 5));
    document.insert_field(&merge_instruction("Name"), "«Name»");
    document.type_text(" of ");
    document.insert_field(&merge_instruction("Town"), "«Town»");
    document
}

// --- The letter ---------------------------------------------------------------

#[test]
fn a_document_is_not_a_merge_letter_until_it_says_so() {
    assert_eq!(document(&["plain"]).merge_source(), None);
}

#[test]
fn the_kind_and_the_list_are_written_into_the_settings() {
    let mut document = document(&["plain"]);
    assert!(document.set_merge_source(Kind::Letters, "C:/lists/people.csv"));

    let reopened = round_trip(&document);
    let (kind, path) = reopened.merge_source().expect("a merge source");
    assert_eq!(kind, Kind::Letters);
    assert_eq!(path, "C:/lists/people.csv");
}

#[test]
fn every_kind_survives_being_written_and_read_back() {
    for kind in Kind::ALL {
        let mut document = document(&["plain"]);
        document.set_merge_source(*kind, "people.csv");
        assert_eq!(round_trip(&document).merge_source().map(|(kind, _)| kind), Some(*kind));
    }
}

#[test]
fn a_document_can_stop_being_a_merge_letter() {
    let mut document = document(&["plain"]);
    document.set_merge_source(Kind::Letters, "people.csv");
    assert!(document.clear_merge_source());
    assert_eq!(round_trip(&document).merge_source(), None);
}

#[test]
fn the_letter_says_which_columns_it_wants() {
    let reopened = round_trip(&letter());
    assert_eq!(reopened.merge_fields(), vec!["Name".to_owned(), "Town".to_owned()]);
}

#[test]
fn a_column_asked_for_twice_is_named_once() {
    let mut document = letter();
    document.insert_field(&merge_instruction("Name"), "«Name»");
    assert_eq!(round_trip(&document).merge_fields().len(), 2);
}

#[test]
fn a_letter_that_asks_for_what_the_list_has_not_got_says_so() {
    let mut document = letter();
    document.insert_field(&merge_instruction("Postcode"), "«Postcode»");

    let reopened = round_trip(&document);
    assert_eq!(reopened.missing_merge_fields(&list()), vec!["Postcode".to_owned()]);
}

#[test]
fn a_letter_that_asks_for_nothing_missing_says_nothing() {
    assert!(round_trip(&letter()).missing_merge_fields(&list()).is_empty());
}

// --- Merging ------------------------------------------------------------------

#[test]
fn merging_puts_one_persons_values_into_the_letter() {
    let mut document = round_trip(&letter());
    assert_eq!(document.apply_merge_record(&list().record(0)), 2);
    assert_eq!(document.paragraph_text(0).as_deref(), Some("Dear Ann of Leeds"));
}

#[test]
fn merging_the_second_person_gives_the_second_letter() {
    let mut document = round_trip(&letter());
    document.apply_merge_record(&list().record(1));
    assert_eq!(document.paragraph_text(0).as_deref(), Some("Dear John of Hull"));
}

#[test]
fn a_merged_letter_has_no_merge_fields_left_in_it() {
    let mut document = round_trip(&letter());
    document.apply_merge_record(&list().record(0));
    assert!(round_trip(&document).merge_fields().is_empty());
}

#[test]
fn a_column_the_list_has_not_got_merges_to_nothing() {
    let mut document = document(&["Dear "]);
    document.set_caret(TextPosition::new(0, 5));
    document.insert_field(&merge_instruction("Missing"), "«Missing»");

    let mut document = round_trip(&document);
    document.apply_merge_record(&list().record(0));
    assert_eq!(document.paragraph_text(0).as_deref(), Some("Dear "));
}

#[test]
fn merging_can_be_undone() {
    let mut document = round_trip(&letter());
    document.apply_merge_record(&list().record(0));
    assert!(document.undo());
    assert_eq!(document.paragraph_text(0).as_deref(), Some("Dear «Name» of «Town»"));
}

#[test]
fn a_merged_letter_saves_and_reopens() {
    let mut document = round_trip(&letter());
    document.apply_merge_record(&list().record(0));
    assert_eq!(round_trip(&document).plain_text(), "Dear Ann of Leeds");
}

#[test]
fn merging_a_document_with_no_fields_changes_nothing() {
    let mut document = document(&["plain"]);
    assert_eq!(document.apply_merge_record(&list().record(0)), 0);
    assert_eq!(document.plain_text(), "plain");
}
