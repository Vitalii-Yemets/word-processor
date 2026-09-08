//! Headers, footers and the page-number field.

use wp_docx::furniture::{Furniture, Preset};
use wp_docx::model::{Alignment, Block, Body, Paragraph};
use wp_docx::Document;

fn document() -> Document {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("text")));
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

fn round_trip(document: &Document) -> Document {
    let bytes = document.save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

#[test]
fn a_new_document_has_neither() {
    let document = document();
    assert!(document.furniture(Furniture::Header).is_none());
    assert!(document.furniture(Furniture::Footer).is_none());
}

#[test]
fn a_header_can_be_added_and_reads_back() {
    let mut document = document();
    assert!(document
        .set_furniture(Furniture::Header, Preset::Text, Alignment::Center, "Report")
        .expect("setting a header"));

    let reopened = round_trip(&document);
    let header = reopened.furniture(Furniture::Header).expect("a header");
    assert_eq!(header.plain_text(), "Report");
}

#[test]
fn a_footer_is_a_different_part_from_the_header() {
    let mut document = document();
    document
        .set_furniture(Furniture::Header, Preset::Text, Alignment::Center, "Top")
        .expect("a header");
    document
        .set_furniture(Furniture::Footer, Preset::Text, Alignment::Center, "Bottom")
        .expect("a footer");

    let reopened = round_trip(&document);
    assert_eq!(reopened.furniture(Furniture::Header).expect("a header").plain_text(), "Top");
    assert_eq!(reopened.furniture(Furniture::Footer).expect("a footer").plain_text(), "Bottom");
    assert_ne!(
        reopened.furniture_part(Furniture::Header),
        reopened.furniture_part(Furniture::Footer)
    );
}

#[test]
fn a_page_number_is_stored_as_a_field_and_survives_the_round_trip() {
    let mut document = document();
    document
        .set_furniture(Furniture::Footer, Preset::PageNumber, Alignment::Center, "")
        .expect("a footer");

    let reopened = round_trip(&document);
    let footer = reopened.furniture(Furniture::Footer).expect("a footer");
    let Block::Paragraph(paragraph) = &footer.blocks[0] else { panic!("a paragraph") };
    assert_eq!(
        paragraph.runs[0].field.as_deref(),
        Some("PAGE"),
        "the field instruction was lost, leaving only the cached number"
    );
}

#[test]
fn page_of_total_keeps_both_fields_and_the_words_between_them() {
    let mut document = document();
    document
        .set_furniture(Furniture::Footer, Preset::PageOfTotal, Alignment::Center, "")
        .expect("a footer");

    let footer = round_trip(&document).furniture(Furniture::Footer).expect("a footer");
    let Block::Paragraph(paragraph) = &footer.blocks[0] else { panic!("a paragraph") };
    let fields: Vec<Option<&str>> = paragraph.runs.iter().map(|run| run.field.as_deref()).collect();
    assert_eq!(fields, vec![None, Some("PAGE"), None, Some("NUMPAGES")]);
    assert_eq!(footer.plain_text(), "Page 1 of 1");
}

#[test]
fn setting_a_header_twice_does_not_leave_two_parts_behind() {
    let mut document = document();
    document
        .set_furniture(Furniture::Header, Preset::Text, Alignment::Start, "First")
        .expect("a header");
    let first = document.furniture_part(Furniture::Header).expect("a part");

    document
        .set_furniture(Furniture::Header, Preset::Text, Alignment::Start, "Second")
        .expect("a header");
    let second = document.furniture_part(Furniture::Header).expect("a part");

    assert_eq!(first, second, "the second header went into a new part");
    assert_eq!(
        round_trip(&document).furniture(Furniture::Header).expect("a header").plain_text(),
        "Second"
    );
}

#[test]
fn a_header_can_be_taken_off_again() {
    let mut document = document();
    document
        .set_furniture(Furniture::Header, Preset::Text, Alignment::Start, "Gone soon")
        .expect("a header");
    assert!(document
        .set_furniture(Furniture::Header, Preset::None, Alignment::Start, "")
        .expect("removing"));

    assert!(round_trip(&document).furniture(Furniture::Header).is_none());
}

#[test]
fn the_alignment_asked_for_is_the_one_stored() {
    let mut document = document();
    document
        .set_furniture(Furniture::Footer, Preset::PageNumber, Alignment::End, "")
        .expect("a footer");

    let footer = round_trip(&document).furniture(Furniture::Footer).expect("a footer");
    let Block::Paragraph(paragraph) = &footer.blocks[0] else { panic!("a paragraph") };
    assert_eq!(paragraph.properties.alignment, Some(Alignment::End));
}

#[test]
fn the_body_is_untouched_by_any_of_it() {
    let mut document = document();
    document
        .set_furniture(Furniture::Header, Preset::Text, Alignment::Center, "Header")
        .expect("a header");
    document
        .set_furniture(Furniture::Footer, Preset::PageOfTotal, Alignment::Center, "")
        .expect("a footer");
    assert_eq!(round_trip(&document).plain_text(), "text");
}

#[test]
fn how_far_the_furniture_sits_from_the_edge_has_a_default() {
    // 708 twips, which is what this program writes and what Word writes too.
    assert_eq!(document().furniture_distances(), (708, 708));
}
