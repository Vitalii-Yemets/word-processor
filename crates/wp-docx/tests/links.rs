//! Hyperlinks.

use wp_docx::links::Destination;
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
fn a_document_starts_with_no_links() {
    assert!(document(&["plain"]).hyperlinks().is_empty());
}

#[test]
fn the_selection_becomes_a_link_without_a_character_changing() {
    let mut document = document(&["see the manual here"]);
    select(&mut document, 0, 8, 14);
    assert!(document.add_hyperlink("example.org", ""));

    let reopened = round_trip(&document);
    assert_eq!(reopened.paragraph_text(0).as_deref(), Some("see the manual here"));

    let links = reopened.hyperlinks();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].text, "manual");
    assert_eq!(links[0].range, (8, 14));
    assert_eq!(links[0].destination, Destination::Address("https://example.org".to_owned()));
}

#[test]
fn the_address_goes_in_the_relationships_and_not_in_the_text() {
    let mut document = document(&["see the manual here"]);
    select(&mut document, 0, 8, 14);
    document.add_hyperlink("https://example.org/a", "");

    let reopened = round_trip(&document);
    assert!(!reopened.plain_text().contains("example.org"));
    assert_eq!(
        reopened.hyperlinks()[0].destination,
        Destination::Address("https://example.org/a".to_owned())
    );
}

#[test]
fn with_nothing_selected_the_words_are_typed_in() {
    let mut document = document(&["go to "]);
    document.set_caret(TextPosition::new(0, 6));
    assert!(document.add_hyperlink("example.org", "the manual"));

    let reopened = round_trip(&document);
    assert_eq!(reopened.paragraph_text(0).as_deref(), Some("go to the manual"));
    assert_eq!(reopened.hyperlinks()[0].text, "the manual");
}

#[test]
fn with_no_words_either_the_address_is_the_text() {
    let mut document = document(&["go to "]);
    document.set_caret(TextPosition::new(0, 6));
    document.add_hyperlink("https://example.org", "");

    assert_eq!(
        round_trip(&document).paragraph_text(0).as_deref(),
        Some("go to https://example.org")
    );
}

#[test]
fn a_link_to_a_bookmark_carries_the_name_rather_than_a_relationship() {
    let mut document = document(&["A heading here", "back to the top"]);
    select(&mut document, 0, 0, 14);
    document.add_bookmark("Top");

    select(&mut document, 1, 8, 15);
    assert!(document.add_hyperlink("Top", ""));

    let links = round_trip(&document).hyperlinks();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].destination, Destination::Place("Top".to_owned()));
}

#[test]
fn a_link_is_blue_and_underlined() {
    let mut document = document(&["see the manual here"]);
    select(&mut document, 0, 8, 14);
    document.add_hyperlink("example.org", "");

    let reopened = round_trip(&document);
    let Block::Paragraph(paragraph) = &reopened.body().blocks[0] else { panic!("a paragraph") };
    let run = paragraph.runs.iter().find(|run| run.plain_text() == "manual").expect("the run");
    assert_eq!(run.properties.color.as_deref(), Some("0563C1"));
    assert!(run.properties.underline.as_ref().is_some_and(wp_docx::model::Underline::is_visible));
    assert_eq!(run.properties.style.as_deref(), Some("Hyperlink"));
}

#[test]
fn a_link_at_the_very_start_of_a_paragraph_keeps_the_paragraph_properties() {
    let mut document = document(&["manual and more"]);
    document.set_caret(TextPosition::new(0, 0));
    document.set_alignment_here(wp_docx::model::Alignment::Center);
    select(&mut document, 0, 0, 6);
    document.add_hyperlink("example.org", "");

    let reopened = round_trip(&document);
    assert_eq!(reopened.paragraph_text(0).as_deref(), Some("manual and more"));
    assert_eq!(reopened.alignment_here(), wp_docx::model::Alignment::Center);
    assert_eq!(reopened.hyperlinks().len(), 1);
}

#[test]
fn the_caret_knows_which_link_it_is_in() {
    let mut document = document(&["see the manual here"]);
    select(&mut document, 0, 8, 14);
    document.add_hyperlink("example.org", "");

    let mut reopened = round_trip(&document);
    reopened.set_caret(TextPosition::new(0, 10));
    assert!(reopened.hyperlink_here().is_some());
    reopened.set_caret(TextPosition::new(0, 2));
    assert!(reopened.hyperlink_here().is_none());
}

#[test]
fn a_link_can_be_taken_off_again() {
    let mut document = document(&["see the manual here"]);
    select(&mut document, 0, 8, 14);
    document.add_hyperlink("example.org", "");

    document.set_caret(TextPosition::new(0, 10));
    assert!(document.remove_hyperlink());

    let reopened = round_trip(&document);
    assert!(reopened.hyperlinks().is_empty());
    assert_eq!(reopened.paragraph_text(0).as_deref(), Some("see the manual here"));
}

#[test]
fn the_same_address_twice_is_the_same_relationship() {
    let mut document = document(&["one two"]);
    select(&mut document, 0, 0, 3);
    document.add_hyperlink("example.org", "");
    select(&mut document, 0, 4, 7);
    document.add_hyperlink("example.org", "");

    let reopened = round_trip(&document);
    assert_eq!(reopened.hyperlinks().len(), 2);
    // Both point at the same place, and the package says so only once.
    let relationships = reopened
        .package()
        .relationships("word/document.xml")
        .expect("the document's relationships");
    let links: Vec<_> = relationships
        .all()
        .iter()
        .filter(|relationship| relationship.target == "https://example.org")
        .collect();
    assert_eq!(links.len(), 1);
}

#[test]
fn a_link_across_paragraphs_becomes_one_link_in_each() {
    let mut document = document(&["first line", "second line"]);
    document.move_caret(TextPosition::new(0, 6), false);
    document.move_caret(TextPosition::new(1, 6), true);
    assert!(document.add_hyperlink("example.org", ""));

    let reopened = round_trip(&document);
    assert_eq!(reopened.hyperlinks().len(), 2);
    assert_eq!(reopened.plain_text(), "first line\nsecond line");
}

#[test]
fn making_a_link_can_be_undone() {
    let mut document = document(&["see the manual here"]);
    select(&mut document, 0, 8, 14);
    document.add_hyperlink("example.org", "");
    assert!(document.undo());
    assert!(document.hyperlinks().is_empty());
    assert_eq!(document.paragraph_text(0).as_deref(), Some("see the manual here"));
}

#[test]
fn an_empty_address_is_refused() {
    let mut document = document(&["plain"]);
    select(&mut document, 0, 0, 5);
    assert!(!document.add_hyperlink("   ", ""));
}
