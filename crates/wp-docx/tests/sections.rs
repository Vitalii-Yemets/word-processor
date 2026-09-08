//! Section breaks: cutting a document into stretches with their own page setup.

use wp_docx::model::{Block, Body, Paragraph};
use wp_docx::sections::Start;
use wp_docx::{Document, TextPosition};

/// Two paragraphs, "first" and "second".
fn document() -> Document {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("first")));
    body.blocks.push(Block::Paragraph(Paragraph::text("second")));
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

fn round_trip(document: &Document) -> Document {
    let bytes = document.save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

/// The same, cut in two between the paragraphs.
fn broken(start: Start) -> Document {
    let mut document = document();
    document.set_caret(TextPosition::new(0, 5));
    assert!(document.insert_section_break(start), "the break was not made");
    round_trip(&document)
}

#[test]
fn a_document_starts_as_one_section() {
    assert_eq!(document().sections().len(), 1);
}

#[test]
fn a_break_cuts_it_in_two() {
    assert_eq!(broken(Start::NextPage).sections().len(), 2);
}

#[test]
fn the_caret_goes_into_the_new_section() {
    let mut document = document();
    document.set_caret(TextPosition::new(0, 5));
    document.insert_section_break(Start::NextPage);
    assert_eq!(document.caret(), TextPosition::new(1, 0));
}

#[test]
fn the_new_section_begins_the_way_it_was_asked_to() {
    for start in [Start::Continuous, Start::EvenPage, Start::OddPage] {
        let sections = broken(start).sections();
        assert_eq!(sections[1].setup.start, start, "{start:?}");
    }
}

#[test]
fn a_next_page_break_is_the_one_that_needs_no_saying() {
    // nextPage is what the format assumes, so Word writes nothing for it.
    let sections = broken(Start::NextPage).sections();
    assert_eq!(sections[1].setup.start, Start::NextPage);
}

#[test]
fn the_first_section_keeps_the_page_it_had() {
    // A break moves text; it does not change the paper until somebody does.
    let before = document().sections()[0].setup;
    let sections = broken(Start::NextPage).sections();
    assert_eq!(sections[0].setup, before);
    assert_eq!(sections[1].setup, before);
}

#[test]
fn a_break_keeps_a_page_setup_that_was_already_there() {
    let mut document = document();
    document.set_page_margins(720, 720, 720, 720);
    document.set_caret(TextPosition::new(0, 5));
    document.insert_section_break(Start::Continuous);
    let sections = round_trip(&document).sections();
    assert_eq!(sections[0].setup.margin_top, 720);
    assert_eq!(sections[1].setup.margin_top, 720);
}

#[test]
fn a_break_in_the_middle_of_a_paragraph_splits_it() {
    let mut document = document();
    document.set_caret(TextPosition::new(0, 2));
    document.insert_section_break(Start::NextPage);
    assert_eq!(round_trip(&document).plain_text(), "fi\nrst\nsecond");
}

#[test]
fn a_break_can_be_undone() {
    let mut document = document();
    document.set_caret(TextPosition::new(0, 5));
    document.insert_section_break(Start::NextPage);
    assert!(document.undo());
    assert_eq!(document.sections().len(), 1);
    assert_eq!(document.plain_text(), "first\nsecond");
}

#[test]
fn a_second_break_makes_a_third_section() {
    let mut document = broken(Start::NextPage);
    document.set_caret(TextPosition::new(2, 6));
    assert!(document.insert_section_break(Start::Continuous));
    let sections = round_trip(&document).sections();
    assert_eq!(sections.len(), 3, "{sections:?}");
    assert_eq!(sections[1].setup.start, Start::NextPage);
    assert_eq!(sections[2].setup.start, Start::Continuous);
}

#[test]
fn page_setup_reaches_the_section_the_caret_is_in() {
    let mut document = broken(Start::NextPage);
    document.set_caret(TextPosition::new(0, 0));
    assert!(document.set_landscape(true));

    let reopened = round_trip(&document);
    let sections = reopened.sections();
    assert!(sections[0].setup.is_landscape(), "the caret's section was not turned");
    assert!(!sections[1].setup.is_landscape(), "the other section was turned as well");
}

#[test]
fn page_setup_reaches_the_last_section_too() {
    let mut document = broken(Start::NextPage);
    document.set_caret(TextPosition::new(2, 0));
    assert!(document.set_landscape(true));

    let sections = round_trip(&document).sections();
    assert!(!sections[0].setup.is_landscape());
    assert!(sections[1].setup.is_landscape());
}

#[test]
fn what_is_read_back_is_the_caret_sections_setup() {
    let mut document = broken(Start::NextPage);
    document.set_caret(TextPosition::new(2, 0));
    document.set_page_margins(360, 360, 360, 360);
    document.set_columns(2, 400);

    let mut reopened = round_trip(&document);
    reopened.set_caret(TextPosition::new(2, 0));
    assert_eq!(reopened.page_margins(), (360, 360, 360, 360));
    assert_eq!(reopened.columns(), (2, 400));

    reopened.set_caret(TextPosition::new(0, 0));
    assert_eq!(reopened.page_margins(), (1440, 1440, 1440, 1440), "the first section was changed");
    assert_eq!(reopened.columns(), (1, 720));
}

#[test]
fn the_columns_of_one_section_do_not_disturb_the_other() {
    let mut document = broken(Start::Continuous);
    document.set_caret(TextPosition::new(0, 0));
    document.set_columns(3, 360);

    let sections = round_trip(&document).sections();
    assert_eq!(sections[0].setup.columns, 3);
    assert_eq!(sections[1].setup.columns, 1);
}

/// The text of a header, whichever section's it is.
fn header_text(document: &Document, section: usize) -> String {
    document
        .furniture_of(wp_docx::furniture::Furniture::Header, section)
        .map(|body| body.blocks.iter().map(Block::plain_text).collect::<Vec<_>>().join("\n"))
        .unwrap_or_default()
}

/// Puts a header on the section the caret is in.
fn set_header(document: &mut Document, text: &str) {
    document
        .set_furniture(
            wp_docx::furniture::Furniture::Header,
            wp_docx::furniture::Preset::Text,
            wp_docx::model::Alignment::Start,
            text,
        )
        .expect("a header");
}

#[test]
fn a_section_without_a_header_of_its_own_follows_the_one_before() {
    // Which is what "Link to Previous" means, and what the format assumes when
    // a section writes no reference at all.
    let mut document = broken(Start::NextPage);
    document.set_caret(TextPosition::new(0, 0));
    set_header(&mut document, "Everywhere");

    let reopened = round_trip(&document);
    assert!(header_text(&reopened, 0).contains("Everywhere"));
    assert!(header_text(&reopened, 1).contains("Everywhere"), "the second section has no header");
}

#[test]
fn each_section_can_have_a_header_of_its_own() {
    let mut document = broken(Start::NextPage);
    document.set_caret(TextPosition::new(0, 0));
    set_header(&mut document, "First");
    document.set_caret(TextPosition::new(2, 0));
    set_header(&mut document, "Second");

    let reopened = round_trip(&document);
    let first = header_text(&reopened, 0);
    let second = header_text(&reopened, 1);
    assert!(first.contains("First"), "{first:?}");
    assert!(second.contains("Second"), "{second:?}");
    assert!(!first.contains("Second"), "the second header was written over the first: {first:?}");
}

#[test]
fn the_header_the_caret_sees_is_its_own_sections() {
    let mut document = broken(Start::NextPage);
    document.set_caret(TextPosition::new(0, 0));
    set_header(&mut document, "First");
    document.set_caret(TextPosition::new(2, 0));
    set_header(&mut document, "Second");

    let mut reopened = round_trip(&document);
    reopened.set_caret(TextPosition::new(0, 0));
    let here = reopened
        .furniture(wp_docx::furniture::Furniture::Header)
        .expect("a header")
        .blocks
        .iter()
        .map(Block::plain_text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(here.contains("First"), "{here:?}");
}
