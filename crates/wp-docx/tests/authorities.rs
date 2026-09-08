//! The table of authorities.

use wp_docx::authorities::Category;
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

#[test]
fn a_document_starts_with_nothing_marked() {
    assert!(document(&["plain"]).authorities(&[]).is_empty());
}

#[test]
fn a_citation_can_be_marked_and_reads_back() {
    let mut document = document(&["As held in Smith."]);
    document.set_caret(TextPosition::new(0, 11));
    assert!(document.mark_authority("Smith v Jones, 1 F.2d 1", "Smith", Category::Cases));

    let marks = round_trip(&document).authorities(&[]);
    assert_eq!(marks.len(), 1);
    assert_eq!(marks[0].long, "Smith v Jones, 1 F.2d 1");
    assert_eq!(marks[0].short, "Smith");
    assert_eq!(marks[0].category, Category::Cases);
}

#[test]
fn a_mark_changes_not_one_character_of_the_text() {
    let mut document = document(&["As held in Smith."]);
    document.set_caret(TextPosition::new(0, 11));
    document.mark_authority("Smith v Jones", "Smith", Category::Cases);

    let reopened = round_trip(&document);
    assert_eq!(reopened.paragraph_text(0).as_deref(), Some("As held in Smith."));
    assert_eq!(reopened.plain_text(), "As held in Smith.");
}

#[test]
fn a_citation_with_no_text_is_refused() {
    let mut document = document(&["plain"]);
    document.set_caret(TextPosition::new(0, 0));
    assert!(!document.mark_authority("   ", "", Category::Cases));
}

#[test]
fn a_document_has_no_table_until_one_is_put_in() {
    assert!(!document(&["plain"]).has_authorities(Category::Cases));
}

#[test]
fn a_table_gathers_the_citations_of_its_category() {
    let mut document = document(&["", "one", "two"]);
    document.set_caret(TextPosition::new(1, 0));
    document.mark_authority("Smith v Jones", "Smith", Category::Cases);
    document.set_caret(TextPosition::new(2, 0));
    document.mark_authority("The Companies Act", "", Category::Statutes);

    document.set_caret(TextPosition::new(0, 0));
    assert_eq!(document.insert_authorities(Category::Cases, &[]), 1);

    let text = round_trip(&document).plain_text();
    assert!(text.contains("Cases"), "the heading is missing");
    assert!(text.contains("Smith v Jones"));
    // The statute belongs to another table and is not gathered into this one.
    let heading_at = text.find("Cases").expect("a heading");
    let before_body = text[heading_at..].find("one").unwrap_or(text.len() - heading_at);
    assert!(!text[heading_at..heading_at + before_body].contains("Companies Act"));
}

#[test]
fn each_category_has_a_table_of_its_own() {
    let mut document = document(&["", "one", "two"]);
    document.set_caret(TextPosition::new(1, 0));
    document.mark_authority("Smith v Jones", "", Category::Cases);
    document.set_caret(TextPosition::new(2, 0));
    document.mark_authority("The Companies Act", "", Category::Statutes);

    document.set_caret(TextPosition::new(0, 0));
    document.insert_authorities(Category::Cases, &[]);
    document.set_caret(TextPosition::new(0, 0));
    document.insert_authorities(Category::Statutes, &[]);

    let reopened = round_trip(&document);
    assert!(reopened.has_authorities(Category::Cases));
    assert!(reopened.has_authorities(Category::Statutes));
    let text = reopened.plain_text();
    assert!(text.contains("Smith v Jones"));
    assert!(text.contains("The Companies Act"));
}

#[test]
fn the_long_and_the_short_form_are_one_line_with_both_pages() {
    let mut document = document(&["", "first mention", "later mention"]);
    document.set_caret(TextPosition::new(1, 0));
    document.mark_authority("Smith v Jones", "Smith", Category::Cases);
    document.set_caret(TextPosition::new(2, 0));
    document.mark_authority("Smith v Jones", "Smith", Category::Cases);

    document.set_caret(TextPosition::new(0, 0));
    // Paragraph 1 is on page 2 and paragraph 2 on page 6.
    assert_eq!(document.insert_authorities(Category::Cases, &[0, 2, 6]), 1, "one line, not two");

    let text = round_trip(&document).plain_text();
    assert!(text.contains("2, 6"), "both pages should be listed: got {text:?}");
}

#[test]
fn the_lines_are_in_alphabetical_order() {
    let mut document = document(&["", "one", "two"]);
    document.set_caret(TextPosition::new(1, 0));
    document.mark_authority("Zebra v Aardvark", "", Category::Cases);
    document.set_caret(TextPosition::new(2, 0));
    document.mark_authority("Aardvark v Zebra", "", Category::Cases);

    document.set_caret(TextPosition::new(0, 0));
    document.insert_authorities(Category::Cases, &[]);

    let text = round_trip(&document).plain_text();
    let first = text.find("Aardvark v Zebra").expect("Aardvark");
    let second = text.find("Zebra v Aardvark").expect("Zebra");
    assert!(first < second, "the table is not in order");
}

#[test]
fn updating_replaces_the_table_rather_than_adding_a_second() {
    let mut document = document(&["", "one"]);
    document.set_caret(TextPosition::new(1, 0));
    document.mark_authority("Smith v Jones", "", Category::Cases);

    document.set_caret(TextPosition::new(0, 0));
    document.insert_authorities(Category::Cases, &[]);
    let after_first = round_trip(&document).paragraph_count();

    document.set_caret(TextPosition::new(0, 0));
    document.insert_authorities(Category::Cases, &[]);
    assert_eq!(round_trip(&document).paragraph_count(), after_first);
}

#[test]
fn a_table_with_nothing_marked_says_so() {
    let mut document = document(&["", "plain"]);
    document.set_caret(TextPosition::new(0, 0));
    assert_eq!(document.insert_authorities(Category::Cases, &[]), 0);
    assert!(round_trip(&document).plain_text().contains("No cases are marked"));
}

#[test]
fn a_table_of_authorities_and_a_table_of_contents_do_not_replace_each_other() {
    let mut document = document(&["", "one"]);
    document.set_caret(TextPosition::new(1, 0));
    document.mark_authority("Smith v Jones", "", Category::Cases);

    document.set_caret(TextPosition::new(0, 0));
    document.insert_authorities(Category::Cases, &[]);
    document.set_caret(TextPosition::new(0, 0));
    document.insert_contents(3, &[]);

    let text = round_trip(&document).plain_text();
    assert!(text.contains("Smith v Jones"), "the table of authorities was replaced");
    assert!(text.contains("Contents"), "the table of contents is missing");
}

#[test]
fn marking_can_be_undone() {
    let mut document = document(&["plain"]);
    document.set_caret(TextPosition::new(0, 0));
    document.mark_authority("Smith v Jones", "", Category::Cases);
    assert!(document.undo());
    assert!(document.authorities(&[]).is_empty());
}
