//! Page setup: paper size, orientation, margins and columns.
//!
//! All four live in `w:sectPr`, and all four are read back through a save and
//! reopen — because the point of them is what Word sees, not what is in memory.

use wp_docx::model::{Block, Body, Paragraph};
use wp_docx::page::{MARGIN_PRESETS, PAGE_SIZES};
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
fn a_created_document_is_a4_portrait_with_inch_margins() {
    let document = document();
    assert_eq!(document.page_size(), (11906, 16838));
    assert!(!document.is_landscape());
    assert_eq!(document.page_margins(), (1440, 1440, 1440, 1440));
}

#[test]
fn the_paper_size_can_be_changed() {
    let mut document = document();
    assert!(document.set_page_size(12240, 15840));
    assert_eq!(round_trip(&document).page_size(), (12240, 15840));
}

#[test]
fn turning_the_page_on_its_side_swaps_its_measurements() {
    let mut document = document();
    assert!(document.set_landscape(true));

    let reopened = round_trip(&document);
    assert_eq!(reopened.page_size(), (16838, 11906));
    assert!(reopened.is_landscape());
}

#[test]
fn turning_it_to_the_side_it_is_already_on_changes_nothing() {
    let mut document = document();
    assert!(!document.set_landscape(false), "already portrait");
}

#[test]
fn a_paper_size_set_while_landscape_stays_landscape() {
    // The two numbers are always quoted portrait-way-round; which way the page
    // faces is a separate fact and must survive the change.
    let mut document = document();
    document.set_landscape(true);
    assert!(document.set_page_size(12240, 15840));

    let reopened = round_trip(&document);
    assert_eq!(reopened.page_size(), (15840, 12240));
    assert!(reopened.is_landscape());
}

#[test]
fn margins_can_be_set_from_the_presets() {
    let mut document = document();
    let (_, top, right, bottom, left) = MARGIN_PRESETS[1];
    assert!(document.set_page_margins(top, right, bottom, left));
    assert_eq!(round_trip(&document).page_margins(), (top, right, bottom, left));
}

#[test]
fn every_named_paper_size_reads_back_as_itself() {
    for (name, width, height) in PAGE_SIZES {
        let mut document = document();
        assert!(document.set_page_size(*width, *height), "{name}");
        assert_eq!(round_trip(&document).page_size(), (*width, *height), "{name}");
    }
}

#[test]
fn a_document_flows_down_one_column_until_it_is_told_otherwise() {
    assert_eq!(document().columns(), (1, 720));
}

#[test]
fn the_column_count_can_be_changed() {
    let mut document = document();
    assert!(document.set_columns(3, 360));
    assert_eq!(round_trip(&document).columns(), (3, 360));
}

#[test]
fn an_absurd_column_count_is_brought_back_to_something_a_page_can_hold() {
    let mut document = document();
    document.set_columns(500, 720);
    assert_eq!(round_trip(&document).columns().0, 12);
}

#[test]
fn page_setup_can_be_undone() {
    let mut document = document();
    document.set_landscape(true);
    assert!(document.undo());
    assert!(!document.is_landscape());
}

#[test]
fn setting_the_page_up_does_not_disturb_the_text() {
    let mut document = document();
    document.set_landscape(true);
    document.set_columns(2, 720);
    document.set_page_margins(720, 720, 720, 720);
    assert_eq!(round_trip(&document).plain_text(), "text");
}
