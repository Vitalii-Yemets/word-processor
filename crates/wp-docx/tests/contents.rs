//! The table of contents: gathering it, writing it and updating it.

use wp_docx::model::{Block, Body, Paragraph, ParagraphProperties};
use wp_docx::{Document, TextPosition};

/// A document with a heading, some text, another heading and more text.
fn document() -> Document {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("")));
    body.blocks.push(Block::Paragraph(heading("First chapter", "Heading1")));
    body.blocks.push(Block::Paragraph(Paragraph::text("Some words.")));
    body.blocks.push(Block::Paragraph(heading("A section", "Heading2")));
    body.blocks.push(Block::Paragraph(Paragraph::text("More words.")));
    body.blocks.push(Block::Paragraph(heading("Second chapter", "Heading1")));

    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

fn heading(text: &str, style: &str) -> Paragraph {
    Paragraph {
        properties: ParagraphProperties {
            style: Some(style.to_owned()),
            ..ParagraphProperties::default()
        },
        runs: vec![wp_docx::model::Run::text(text)],
    }
}

fn round_trip(document: &Document) -> Document {
    let bytes = document.save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

#[test]
fn the_headings_are_gathered_in_reading_order() {
    let entries = document().contents_entries(3, &[]);
    let texts: Vec<&str> = entries.iter().map(|entry| entry.text.as_str()).collect();
    assert_eq!(texts, ["First chapter", "A section", "Second chapter"]);
}

#[test]
fn each_entry_remembers_how_deep_it_is() {
    let entries = document().contents_entries(3, &[]);
    assert_eq!(entries[0].level, 0);
    assert_eq!(entries[1].level, 1);
    assert_eq!(entries[2].level, 0);
}

#[test]
fn asking_for_fewer_levels_leaves_the_deeper_ones_out() {
    let entries = document().contents_entries(1, &[]);
    let texts: Vec<&str> = entries.iter().map(|entry| entry.text.as_str()).collect();
    assert_eq!(texts, ["First chapter", "Second chapter"]);
}

#[test]
fn ordinary_paragraphs_are_not_gathered() {
    let entries = document().contents_entries(3, &[]);
    assert!(entries.iter().all(|entry| !entry.text.contains("words")));
}

#[test]
fn a_document_has_no_table_of_contents_until_one_is_put_in() {
    assert!(!document().has_contents());
}

#[test]
fn a_table_of_contents_can_be_inserted_and_reads_back() {
    let mut document = document();
    document.set_caret(TextPosition::new(0, 0));
    assert_eq!(document.insert_contents(3, &[]), 3);

    let reopened = round_trip(&document);
    assert!(reopened.has_contents());
    let text = reopened.plain_text();
    assert!(text.contains("Contents"), "the heading over the table is missing");
    assert!(text.contains("First chapter"));
    assert!(text.contains("A section"));
}

#[test]
fn every_line_of_it_is_a_field_so_it_can_be_found_again() {
    let mut document = document();
    document.set_caret(TextPosition::new(0, 0));
    document.insert_contents(3, &[]);

    let reopened = round_trip(&document);
    let Block::Paragraph(first) = &reopened.body().blocks[0] else { panic!("a paragraph") };
    let instruction = first.runs[0].field.as_deref().expect("a field");
    assert!(instruction.starts_with("TOC"), "got {instruction:?}");
}

#[test]
fn page_numbers_are_used_when_they_are_known() {
    let mut document = document();
    document.set_caret(TextPosition::new(0, 0));
    // Paragraph 1 is on page 1, paragraph 3 on page 2, paragraph 5 on page 3.
    document.insert_contents(3, &[1, 1, 1, 2, 2, 3]);

    let text = round_trip(&document).plain_text();
    assert!(text.contains("First chapter\t1"), "got {text:?}");
    assert!(text.contains("A section\t2"));
    assert!(text.contains("Second chapter\t3"));
}

#[test]
fn updating_replaces_the_table_rather_than_adding_a_second() {
    let mut document = document();
    document.set_caret(TextPosition::new(0, 0));
    document.insert_contents(3, &[]);
    let after_first = round_trip(&document).paragraph_count();

    // Updating from anywhere finds the existing table and replaces it.
    document.set_caret(TextPosition::new(0, 0));
    document.insert_contents(3, &[]);
    assert_eq!(round_trip(&document).paragraph_count(), after_first);

    let text = round_trip(&document).plain_text();
    assert_eq!(text.matches("Contents").count(), 1, "there are two tables now");
}

#[test]
fn a_heading_added_later_turns_up_when_the_table_is_updated() {
    let mut document = document();
    document.set_caret(TextPosition::new(0, 0));
    document.insert_contents(3, &[]);
    assert!(!round_trip(&document).plain_text().contains("Third chapter"));

    // Add a heading at the end, then update.
    let last = document.paragraph_count() - 1;
    let end = document.paragraph_text(last).map_or(0, |text| text.len());
    document.set_caret(TextPosition::new(last, end));
    document.split_paragraph(TextPosition::new(last, end));
    document.type_text("Third chapter");
    document.set_paragraph_style_here(Some("Heading1"));

    document.insert_contents(3, &[]);
    assert!(round_trip(&document).plain_text().contains("Third chapter"));
}

#[test]
fn a_table_of_contents_can_be_taken_out_again() {
    let mut document = document();
    document.set_caret(TextPosition::new(0, 0));
    document.insert_contents(3, &[]);
    assert!(document.remove_contents());

    let reopened = round_trip(&document);
    assert!(!reopened.has_contents());
    assert!(!reopened.plain_text().contains("Contents"));
    // The headings themselves are untouched.
    assert!(reopened.plain_text().contains("First chapter"));
}

#[test]
fn removing_one_that_is_not_there_changes_nothing() {
    let mut document = document();
    assert!(!document.remove_contents());
}

#[test]
fn inserting_a_table_of_contents_can_be_undone() {
    let mut document = document();
    let before = document.paragraph_count();
    document.set_caret(TextPosition::new(0, 0));
    document.insert_contents(3, &[]);
    assert!(document.undo());
    assert_eq!(document.paragraph_count(), before);
}

#[test]
fn the_document_itself_is_not_disturbed() {
    let mut document = document();
    document.set_caret(TextPosition::new(0, 0));
    document.insert_contents(3, &[]);
    document.remove_contents();
    assert_eq!(round_trip(&document).plain_text(), document.plain_text());
}
