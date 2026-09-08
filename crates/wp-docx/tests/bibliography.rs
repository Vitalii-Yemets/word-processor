//! Sources, citations and the bibliography.

use wp_docx::bibliography::{Source, SourceKind};
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

fn book() -> Source {
    Source {
        tag: "Doe01".to_owned(),
        kind: SourceKind::Book,
        author: "John Doe".to_owned(),
        title: "A Book".to_owned(),
        year: "2001".to_owned(),
        publisher: "A Press".to_owned(),
        city: "London".to_owned(),
    }
}

fn article() -> Source {
    Source {
        tag: "Roe99".to_owned(),
        kind: SourceKind::JournalArticle,
        author: "Ann Roe".to_owned(),
        title: "A Paper".to_owned(),
        year: "1999".to_owned(),
        publisher: "A Journal".to_owned(),
        city: String::new(),
    }
}

// --- Sources ------------------------------------------------------------------

#[test]
fn a_document_starts_with_no_sources() {
    assert!(document(&["plain"]).sources().is_empty());
}

#[test]
fn a_source_can_be_added_and_reads_back_after_saving() {
    let mut document = document(&["plain"]);
    assert_eq!(document.add_source(&book()).expect("adding"), "Doe01");

    let sources = round_trip(&document).sources();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0], book());
}

#[test]
fn the_sources_live_in_a_part_of_their_own() {
    let mut document = document(&["plain"]);
    document.add_source(&book()).expect("adding");

    let reopened = round_trip(&document);
    assert_eq!(reopened.bibliography_part().as_deref(), Some("word/bibliography.xml"));
    // Nothing of the source is in the text.
    assert_eq!(reopened.plain_text(), "plain");
}

#[test]
fn using_a_tag_twice_replaces_the_source_rather_than_adding_a_second() {
    let mut document = document(&["plain"]);
    document.add_source(&book()).expect("adding");
    document.add_source(&Source { title: "A Better Book".to_owned(), ..book() }).expect("adding");

    let sources = round_trip(&document).sources();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].title, "A Better Book");
}

#[test]
fn a_source_with_no_tag_is_given_one() {
    let mut document = document(&["plain"]);
    let tag = document.add_source(&Source { tag: String::new(), ..book() }).expect("adding");
    assert_eq!(tag, "Doe2001");
}

#[test]
fn two_sources_are_never_given_the_same_tag() {
    let mut document = document(&["plain"]);
    let first = document.add_source(&Source { tag: String::new(), ..book() }).expect("adding");
    let second = document
        .add_source(&Source { tag: String::new(), title: "Another".to_owned(), ..book() })
        .expect("adding");
    assert_ne!(first, second);
    assert_eq!(round_trip(&document).sources().len(), 2);
}

#[test]
fn a_source_can_be_taken_away() {
    let mut document = document(&["plain"]);
    document.add_source(&book()).expect("adding");
    assert!(document.remove_source("Doe01"));
    assert!(round_trip(&document).sources().is_empty());
}

#[test]
fn removing_one_that_is_not_there_changes_nothing() {
    let mut document = document(&["plain"]);
    assert!(!document.remove_source("Nowhere"));
}

// --- Citations ----------------------------------------------------------------

#[test]
fn a_citation_shows_the_name_in_brackets() {
    let mut document = document(&["As shown "]);
    document.add_source(&book()).expect("adding");
    document.set_caret(TextPosition::new(0, 9));
    assert!(document.insert_citation("Doe01"));

    let text = round_trip(&document).paragraph_text(0).unwrap_or_default();
    assert_eq!(text, "As shown (Doe, 2001)");
}

#[test]
fn a_citation_of_a_source_that_is_not_there_is_refused() {
    let mut document = document(&["plain"]);
    document.set_caret(TextPosition::new(0, 0));
    assert!(!document.insert_citation("Nowhere"));
}

#[test]
fn a_citation_is_a_field_so_it_can_be_worked_out_again() {
    let mut document = document(&["plain "]);
    document.add_source(&book()).expect("adding");
    document.set_caret(TextPosition::new(0, 6));
    document.insert_citation("Doe01");

    let reopened = round_trip(&document);
    let Block::Paragraph(paragraph) = &reopened.body().blocks[0] else { panic!("a paragraph") };
    let instruction = paragraph.runs.iter().find_map(|run| run.field.as_deref()).expect("a field");
    assert!(instruction.starts_with("CITATION Doe01"), "got {instruction:?}");
}

#[test]
fn the_document_knows_what_it_cites() {
    let mut document = document(&["one ", "two "]);
    document.add_source(&book()).expect("adding");
    document.add_source(&article()).expect("adding");
    document.set_caret(TextPosition::new(1, 4));
    document.insert_citation("Roe99");
    document.set_caret(TextPosition::new(0, 4));
    document.insert_citation("Doe01");

    assert_eq!(round_trip(&document).citations(), vec!["Doe01", "Roe99"]);
}

#[test]
fn the_same_source_cited_twice_is_named_once() {
    let mut document = document(&["one ", "two "]);
    document.add_source(&book()).expect("adding");
    document.set_caret(TextPosition::new(0, 4));
    document.insert_citation("Doe01");
    document.set_caret(TextPosition::new(1, 4));
    document.insert_citation("Doe01");

    assert_eq!(round_trip(&document).citations().len(), 1);
}

#[test]
fn a_citation_can_be_undone() {
    let mut document = document(&["plain "]);
    document.add_source(&book()).expect("adding");
    document.set_caret(TextPosition::new(0, 6));
    document.insert_citation("Doe01");
    assert!(document.undo());
    assert_eq!(document.paragraph_text(0).as_deref(), Some("plain "));
}

// --- The bibliography ---------------------------------------------------------

#[test]
fn a_document_has_no_bibliography_until_one_is_put_in() {
    assert!(!document(&["plain"]).has_bibliography());
}

#[test]
fn a_bibliography_lists_what_is_cited() {
    let mut document = document(&["", "one ", "two "]);
    document.add_source(&book()).expect("adding");
    document.add_source(&article()).expect("adding");
    document.set_caret(TextPosition::new(1, 4));
    document.insert_citation("Doe01");
    document.set_caret(TextPosition::new(2, 4));
    document.insert_citation("Roe99");

    document.set_caret(TextPosition::new(0, 0));
    assert_eq!(document.insert_bibliography(), 2);

    let text = round_trip(&document).plain_text();
    assert!(text.contains("Bibliography"), "the heading is missing");
    assert!(text.contains("Doe, J. (2001). A Book. London: A Press."), "got {text:?}");
    assert!(text.contains("Roe, A. (1999). A Paper. A Journal."), "got {text:?}");
}

#[test]
fn a_source_nobody_cites_is_not_listed() {
    let mut document = document(&["", "one "]);
    document.add_source(&book()).expect("adding");
    document.add_source(&article()).expect("adding");
    document.set_caret(TextPosition::new(1, 4));
    document.insert_citation("Doe01");

    document.set_caret(TextPosition::new(0, 0));
    assert_eq!(document.insert_bibliography(), 1);
    assert!(!round_trip(&document).plain_text().contains("A Paper"));
}

#[test]
fn the_lines_are_in_alphabetical_order() {
    let mut document = document(&["", "one ", "two "]);
    document.add_source(&book()).expect("adding");
    document.add_source(&article()).expect("adding");
    // Cited the other way round; the list still comes out in order.
    document.set_caret(TextPosition::new(1, 4));
    document.insert_citation("Roe99");
    document.set_caret(TextPosition::new(2, 4));
    document.insert_citation("Doe01");

    document.set_caret(TextPosition::new(0, 0));
    document.insert_bibliography();

    let text = round_trip(&document).plain_text();
    let doe = text.find("Doe, J.").expect("Doe");
    let roe = text.find("Roe, A.").expect("Roe");
    assert!(doe < roe, "the bibliography is not in order");
}

#[test]
fn updating_replaces_the_bibliography_rather_than_adding_a_second() {
    let mut document = document(&["", "one "]);
    document.add_source(&book()).expect("adding");
    document.set_caret(TextPosition::new(1, 4));
    document.insert_citation("Doe01");

    document.set_caret(TextPosition::new(0, 0));
    document.insert_bibliography();
    let after_first = round_trip(&document).paragraph_count();

    document.set_caret(TextPosition::new(0, 0));
    document.insert_bibliography();
    assert_eq!(round_trip(&document).paragraph_count(), after_first);
    assert_eq!(round_trip(&document).plain_text().matches("Bibliography").count(), 1);
}

#[test]
fn a_bibliography_with_nothing_cited_says_so() {
    let mut document = document(&["", "plain"]);
    document.set_caret(TextPosition::new(0, 0));
    assert_eq!(document.insert_bibliography(), 0);
    assert!(round_trip(&document).plain_text().contains("Nothing in this document is cited"));
}

#[test]
fn a_bibliography_and_a_table_of_contents_do_not_replace_each_other() {
    let mut document = document(&["", "one "]);
    document.add_source(&book()).expect("adding");
    document.set_caret(TextPosition::new(1, 4));
    document.insert_citation("Doe01");

    document.set_caret(TextPosition::new(0, 0));
    document.insert_bibliography();
    document.set_caret(TextPosition::new(0, 0));
    document.insert_contents(3, &[]);

    let text = round_trip(&document).plain_text();
    assert!(text.contains("Bibliography"), "the bibliography was replaced");
    assert!(text.contains("Contents"), "the table of contents is missing");
}
