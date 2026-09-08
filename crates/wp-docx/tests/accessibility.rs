//! What in a document would stop somebody reading it.

use wp_docx::accessibility::Severity;
use wp_docx::model::{Block, Body, Paragraph};
use wp_docx::properties::Properties;
use wp_docx::shapes::Shape;
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

/// A document that says nothing wrong beyond having no title.
fn titled(lines: &[&str]) -> Document {
    let mut document = document(lines);
    document
        .set_properties(&Properties { title: "A Report".to_owned(), ..Properties::default() })
        .expect("writing the title");
    round_trip(&document)
}

fn problems(document: &Document) -> Vec<String> {
    document.accessibility_findings().into_iter().map(|finding| finding.problem).collect()
}

#[test]
fn a_document_with_no_title_is_reported() {
    let found = problems(&document(&["plain"]));
    assert!(found.iter().any(|problem| problem.contains("no title")), "got {found:?}");
}

#[test]
fn a_document_with_a_title_is_not() {
    let found = problems(&titled(&["plain"]));
    assert!(!found.iter().any(|problem| problem.contains("no title")), "got {found:?}");
}

#[test]
fn a_picture_with_nothing_said_about_it_is_an_error() {
    let mut document = titled(&["plain"]);
    document.set_caret(TextPosition::new(0, 5));
    document.insert_shape(&Shape::preset("rect", 100.0, 100.0));

    let reopened = round_trip(&document);
    let findings = reopened.accessibility_findings();
    let described = findings
        .iter()
        .find(|finding| finding.problem.contains("no description"))
        .expect("a finding");
    assert_eq!(described.severity, Severity::Error);
}

#[test]
fn a_picture_that_is_described_is_not_reported() {
    let mut shape = Shape::preset("rect", 100.0, 100.0);
    shape.description = "A red square".to_owned();

    let mut document = titled(&["plain"]);
    document.set_caret(TextPosition::new(0, 5));
    document.insert_shape(&shape);

    let reopened = round_trip(&document);
    assert!(
        !problems(&reopened).iter().any(|problem| problem.contains("no description")),
        "a described shape should not be reported"
    );
}

#[test]
fn a_description_survives_being_saved_and_read_back() {
    let mut shape = Shape::preset("rect", 100.0, 100.0);
    shape.description = "A red square".to_owned();

    let mut document = titled(&["plain"]);
    document.set_caret(TextPosition::new(0, 5));
    document.insert_shape(&shape);

    let shapes = round_trip(&document).shapes();
    assert_eq!(shapes[0].description, "A red square");
}

#[test]
fn a_text_box_needs_no_description_because_it_says_what_it_says() {
    let mut document = titled(&["plain"]);
    document.set_caret(TextPosition::new(0, 5));
    document.insert_shape(&Shape::text_box(200.0, 80.0, "The words inside"));

    assert!(
        !problems(&round_trip(&document)).iter().any(|problem| problem.contains("no description")),
        "a text box describes itself"
    );
}

#[test]
fn text_too_near_the_colour_of_the_page_is_an_error() {
    let mut document = titled(&["Some text on the page"]);
    document.move_caret(TextPosition::new(0, 0), false);
    document.move_caret(TextPosition::new(0, 21), true);
    document.set_color(Some("EEEEEE"));

    let reopened = round_trip(&document);
    let found = reopened
        .accessibility_findings()
        .into_iter()
        .find(|finding| finding.problem.contains("against the page"))
        .expect("a contrast finding");
    assert_eq!(found.severity, Severity::Error);
    assert_eq!(found.paragraph, Some(0));
}

#[test]
fn text_that_reads_easily_is_not_reported() {
    let mut document = titled(&["Some text on the page"]);
    document.move_caret(TextPosition::new(0, 0), false);
    document.move_caret(TextPosition::new(0, 21), true);
    document.set_color(Some("1F3864"));

    assert!(
        !problems(&round_trip(&document))
            .iter()
            .any(|problem| problem.contains("against the page")),
        "dark blue on white reads perfectly well"
    );
}

#[test]
fn a_heading_level_skipped_is_reported() {
    let mut document = titled(&["Title", "Section"]);
    document.set_caret(TextPosition::new(0, 0));
    document.set_paragraph_style_here(Some("Heading1"));
    document.set_caret(TextPosition::new(1, 0));
    document.set_paragraph_style_here(Some("Heading3"));

    let reopened = round_trip(&document);
    let found = problems(&reopened);
    assert!(
        found.iter().any(|problem| problem.contains("follows heading")),
        "a skipped level should be reported: {found:?}"
    );
}

#[test]
fn heading_levels_in_order_are_not_reported() {
    let mut document = titled(&["Title", "Section"]);
    document.set_caret(TextPosition::new(0, 0));
    document.set_paragraph_style_here(Some("Heading1"));
    document.set_caret(TextPosition::new(1, 0));
    document.set_paragraph_style_here(Some("Heading2"));

    assert!(
        !problems(&round_trip(&document)).iter().any(|problem| problem.contains("follows heading")),
        "one level to the next is what an outline is"
    );
}

#[test]
fn a_link_that_reads_as_its_own_address_is_a_tip() {
    let mut document = titled(&["see https://example.org here"]);
    document.move_caret(TextPosition::new(0, 4), false);
    document.move_caret(TextPosition::new(0, 23), true);
    document.add_hyperlink("https://example.org", "");

    let reopened = round_trip(&document);
    let found = reopened
        .accessibility_findings()
        .into_iter()
        .find(|finding| finding.problem.contains("its own address"))
        .expect("a link finding");
    assert_eq!(found.severity, Severity::Tip);
}

#[test]
fn a_link_with_words_of_its_own_is_not_reported() {
    let mut document = titled(&["see the manual here"]);
    document.move_caret(TextPosition::new(0, 8), false);
    document.move_caret(TextPosition::new(0, 14), true);
    document.add_hyperlink("https://example.org", "");

    assert!(!problems(&round_trip(&document))
        .iter()
        .any(|problem| problem.contains("its own address")));
}

#[test]
fn the_worst_problems_come_first() {
    let mut document = document(&["plain"]);
    document.set_caret(TextPosition::new(0, 5));
    document.insert_shape(&Shape::preset("rect", 100.0, 100.0));

    let findings = round_trip(&document).accessibility_findings();
    assert!(findings.len() >= 2, "there should be a title warning and a description error");
    assert_eq!(findings[0].severity, Severity::Error);
}
