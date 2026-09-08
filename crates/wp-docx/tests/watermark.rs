//! The watermark behind the text.

use wp_docx::furniture::Furniture;
use wp_docx::model::{Block, Body, Paragraph};
use wp_docx::watermark::Watermark;
use wp_docx::Document;

fn document() -> Document {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("plain")));
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

fn round_trip(document: &Document) -> Document {
    let bytes = document.save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

#[test]
fn a_document_has_no_watermark() {
    assert_eq!(document().watermark(), None);
}

#[test]
fn a_watermark_can_be_put_on_and_reads_back_after_saving() {
    let mut document = document();
    let wanted = Watermark::saying("DRAFT");
    assert!(document.set_watermark(Some(&wanted)));
    assert_eq!(round_trip(&document).watermark(), Some(wanted));
}

#[test]
fn putting_one_on_makes_a_header_for_it_to_live_in() {
    let mut document = document();
    assert_eq!(document.furniture_part(Furniture::Header), None);
    document.set_watermark(Some(&Watermark::saying("DRAFT")));

    let reopened = round_trip(&document);
    assert!(reopened.furniture_part(Furniture::Header).is_some(), "no header was made");
}

#[test]
fn a_watermark_is_not_part_of_the_text() {
    let mut document = document();
    document.set_watermark(Some(&Watermark::saying("DRAFT")));
    assert_eq!(round_trip(&document).plain_text(), "plain");
}

#[test]
fn a_flat_watermark_stays_flat() {
    let mut document = document();
    let wanted = Watermark { diagonal: false, ..Watermark::saying("SAMPLE") };
    document.set_watermark(Some(&wanted));
    assert_eq!(round_trip(&document).watermark(), Some(wanted));
}

#[test]
fn a_colour_of_its_own_survives() {
    let mut document = document();
    let wanted = Watermark { color: "FF0000".to_owned(), ..Watermark::saying("URGENT") };
    document.set_watermark(Some(&wanted));
    assert_eq!(
        round_trip(&document).watermark().map(|found| found.color).as_deref(),
        Some("FF0000")
    );
}

#[test]
fn changing_it_replaces_it_rather_than_adding_a_second() {
    let mut document = document();
    document.set_watermark(Some(&Watermark::saying("DRAFT")));
    document.set_watermark(Some(&Watermark::saying("FINAL")));

    let reopened = round_trip(&document);
    assert_eq!(reopened.watermark().map(|found| found.text).as_deref(), Some("FINAL"));

    let part = reopened.furniture_part(Furniture::Header).expect("a header");
    let header = reopened.package().xml_part(&part).expect("the part").expect("readable");
    assert_eq!(header.matches("PowerPlusWaterMarkObject").count(), 1, "two shapes were left");
}

#[test]
fn it_can_be_taken_off_again() {
    let mut document = document();
    document.set_watermark(Some(&Watermark::saying("DRAFT")));
    assert!(document.set_watermark(None));
    assert_eq!(round_trip(&document).watermark(), None);
}

#[test]
fn taking_off_one_that_is_not_there_changes_nothing() {
    let mut document = document();
    assert!(!document.set_watermark(None));
}

#[test]
fn setting_the_same_one_twice_changes_nothing() {
    let mut document = document();
    let wanted = Watermark::saying("DRAFT");
    assert!(document.set_watermark(Some(&wanted)));
    assert!(!document.set_watermark(Some(&wanted)));
}

#[test]
fn a_watermark_leaves_the_rest_of_the_header_alone() {
    use wp_docx::furniture::Preset;
    use wp_docx::model::Alignment;

    let mut document = document();
    document
        .set_furniture(Furniture::Header, Preset::Text, Alignment::Center, "Chapter One")
        .expect("a header");
    document.set_watermark(Some(&Watermark::saying("DRAFT")));

    let reopened = round_trip(&document);
    let header = reopened.furniture(Furniture::Header).expect("a header");
    assert!(
        header.plain_text().contains("Chapter One"),
        "the header text was lost: {:?}",
        header.plain_text()
    );
    assert!(reopened.watermark().is_some());
}

#[test]
fn putting_one_on_can_be_undone() {
    let mut document = document();
    document.set_watermark(Some(&Watermark::saying("DRAFT")));
    assert!(document.undo());
    assert_eq!(document.watermark(), None);
}
