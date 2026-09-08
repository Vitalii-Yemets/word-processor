//! Line numbers, hyphenation, the page colour and document protection.

use wp_docx::appearance::{EditMode, LineNumbers, Restart};
use wp_docx::model::{Block, Body, Paragraph};
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

// --- Line numbers -------------------------------------------------------------

#[test]
fn a_document_does_not_number_its_lines() {
    assert_eq!(document().line_numbers(), None);
}

#[test]
fn lines_can_be_numbered_and_the_setting_survives_saving() {
    let mut document = document();
    let wanted =
        LineNumbers { count_by: 5, start: 1, restart: Restart::NewPage, distance: Some(360) };
    assert!(document.set_line_numbers(Some(wanted)));
    assert_eq!(round_trip(&document).line_numbers(), Some(wanted));
}

#[test]
fn numbering_can_be_turned_off_again() {
    let mut document = document();
    document.set_line_numbers(Some(LineNumbers::default()));
    assert!(document.set_line_numbers(None));
    assert_eq!(round_trip(&document).line_numbers(), None);
}

#[test]
fn setting_the_same_numbering_twice_changes_nothing() {
    let mut document = document();
    let wanted = LineNumbers::default();
    assert!(document.set_line_numbers(Some(wanted)));
    assert!(!document.set_line_numbers(Some(wanted)));
}

#[test]
fn numbering_can_be_undone() {
    let mut document = document();
    document.set_line_numbers(Some(LineNumbers::default()));
    assert!(document.undo());
    assert_eq!(document.line_numbers(), None);
}

// --- Hyphenation --------------------------------------------------------------

#[test]
fn a_document_does_not_hyphenate() {
    assert!(!document().automatic_hyphenation());
}

#[test]
fn hyphenation_can_be_turned_on_and_survives_saving() {
    let mut document = document();
    assert!(document.set_automatic_hyphenation(true));
    assert!(round_trip(&document).automatic_hyphenation());

    assert!(document.set_automatic_hyphenation(false));
    assert!(!round_trip(&document).automatic_hyphenation());
}

#[test]
fn words_in_capitals_are_hyphenated_until_they_are_not() {
    let mut document = document();
    assert!(document.hyphenate_capitals());
    assert!(document.set_hyphenate_capitals(false));
    assert!(!round_trip(&document).hyphenate_capitals());
}

#[test]
fn the_hyphenation_zone_is_a_measurement_and_reads_back() {
    let mut document = document();
    assert_eq!(document.hyphenation_zone(), None);
    assert!(document.set_hyphenation_zone(Some(360)));
    assert_eq!(round_trip(&document).hyphenation_zone(), Some(360));
}

// --- The colour of the page ---------------------------------------------------

#[test]
fn a_page_has_no_colour_of_its_own() {
    assert_eq!(document().page_color(), None);
}

#[test]
fn a_page_can_be_coloured_and_the_colour_survives_saving() {
    let mut document = document();
    assert!(document.set_page_color(Some("1F3864")));
    assert_eq!(round_trip(&document).page_color().as_deref(), Some("1F3864"));
}

#[test]
fn colouring_the_page_also_says_to_show_it() {
    let mut document = document();
    document.set_page_color(Some("1F3864"));

    let reopened = round_trip(&document);
    let settings = reopened
        .package()
        .xml_part("word/settings.xml")
        .expect("the settings part")
        .expect("readable settings");
    assert!(settings.contains("displayBackgroundShape"), "Word would show a white page");
}

#[test]
fn the_colour_can_be_taken_off_again() {
    let mut document = document();
    document.set_page_color(Some("1F3864"));
    assert!(document.set_page_color(None));
    assert_eq!(round_trip(&document).page_color(), None);
}

#[test]
fn colouring_the_page_can_be_undone() {
    let mut document = document();
    document.set_page_color(Some("1F3864"));
    assert!(document.undo());
    assert_eq!(document.page_color(), None);
}

#[test]
fn colouring_the_page_changes_not_one_character_of_the_text() {
    let mut document = document();
    document.set_page_color(Some("1F3864"));
    assert_eq!(round_trip(&document).plain_text(), "plain");
}

// --- Who may edit it ----------------------------------------------------------

#[test]
fn a_document_is_not_protected() {
    assert_eq!(document().protection(), None);
}

#[test]
fn every_kind_of_protection_survives_saving() {
    for mode in EditMode::ALL {
        let mut document = document();
        assert!(document.set_protection(Some(*mode)));
        assert_eq!(round_trip(&document).protection(), Some(*mode), "{}", mode.label());
    }
}

#[test]
fn protection_can_be_lifted() {
    let mut document = document();
    document.set_protection(Some(EditMode::ReadOnly));
    assert!(document.set_protection(None));
    assert_eq!(round_trip(&document).protection(), None);
}

#[test]
fn setting_the_same_protection_twice_changes_nothing() {
    let mut document = document();
    assert!(document.set_protection(Some(EditMode::Comments)));
    assert!(!document.set_protection(Some(EditMode::Comments)));
}

#[test]
fn protection_that_is_not_enforced_is_no_protection() {
    let mut document = document();
    document.set_protection(Some(EditMode::ReadOnly));

    // The same element with the enforcement turned off, which is what Word
    // writes when somebody sets a restriction and then does not apply it.
    let bytes = document.save().expect("saving");
    let mut package = wp_opc::Package::open(&bytes).expect("a package");
    let settings = package
        .xml_part("word/settings.xml")
        .expect("the settings")
        .expect("readable settings")
        .replace(r#"w:enforcement="1""#, r#"w:enforcement="0""#);
    package.set_part("word/settings.xml", settings.into_bytes());

    let bytes = package.save().expect("saving the package");
    assert_eq!(Document::open(&bytes).expect("reopening").protection(), None);
}
