//! The hyphens a document carries, and the settings that govern them.

use wp_docx::model::{Block, Body, Paragraph, Run};
use wp_docx::Document;

fn document(text: &str) -> Document {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::from_runs(vec![Run::text(text)])));
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

fn round_trip(document: &Document) -> Document {
    let bytes = document.save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

/// A document part written the way Word writes the two hyphens: as elements
/// rather than as the characters they stand for.
fn word_wrote(inside: &str) -> Document {
    let plain = document("placeholder");
    let bytes = plain.save().expect("saving");
    let mut package = wp_opc::Package::open(&bytes).expect("the package");
    let main = package.main_document_part().expect("the main part");
    let xml = package.xml_part(&main).expect("the part").expect("text");
    let replaced = xml.replace("placeholder", &format!("hy</w:t>{inside}<w:t>phen"));
    package.set_part(&main, replaced.into_bytes());
    let bytes = package.save().expect("saving the package");
    Document::open(&bytes).expect("reopening")
}

#[test]
fn an_optional_hyphen_written_as_an_element_is_read_as_one() {
    let document = word_wrote("<w:softHyphen/>");
    assert_eq!(document.plain_text().trim_end(), "hy\u{00AD}phen");
}

#[test]
fn a_non_breaking_hyphen_written_as_an_element_is_read_as_one() {
    let document = word_wrote("<w:noBreakHyphen/>");
    assert_eq!(document.plain_text().trim_end(), "hy\u{2011}phen");
}

#[test]
fn the_element_takes_one_character_of_the_text_like_the_character_it_stands_for() {
    // Which is what keeps every offset after it right: the caret counts what
    // the document says, and the document says one hyphen either way.
    let document = word_wrote("<w:softHyphen/>");
    assert_eq!(document.paragraph_text(0).unwrap_or_default(), "hy\u{00AD}phen");
}

#[test]
fn an_optional_hyphen_typed_here_survives_being_saved() {
    let document = document("hy\u{00AD}phen\u{00AD}ation");
    assert_eq!(round_trip(&document).plain_text().trim_end(), "hy\u{00AD}phen\u{00AD}ation");
}

#[test]
fn every_hyphenation_setting_is_written_and_read_back() {
    let mut document = document("text");
    assert!(document.set_automatic_hyphenation(true));
    assert!(document.set_hyphenate_capitals(false));
    assert!(document.set_hyphenation_zone(Some(360)));
    assert!(document.set_consecutive_hyphen_limit(Some(2)));

    let read = round_trip(&document);
    assert!(read.automatic_hyphenation());
    assert!(!read.hyphenate_capitals());
    assert_eq!(read.hyphenation_zone(), Some(360));
    assert_eq!(read.consecutive_hyphen_limit(), Some(2));
}

#[test]
fn no_limit_is_what_nought_means() {
    // Word writes nought for "as many as it takes", which is the same thing
    // said the other way about.
    let mut document = document("text");
    assert!(document.set_consecutive_hyphen_limit(Some(2)));
    assert!(document.set_consecutive_hyphen_limit(Some(0)), "taking the limit off changed nothing");
    assert_eq!(round_trip(&document).consecutive_hyphen_limit(), None);
}

#[test]
fn a_paragraph_may_say_it_is_never_broken() {
    let mut body = Body::default();
    let mut paragraph = Paragraph::from_runs(vec![Run::text("unbreakable")]);
    paragraph.properties.no_hyphenation = Some(true);
    body.blocks.push(Block::Paragraph(paragraph));

    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    let document = Document::open(&bytes).expect("reopening");
    let Block::Paragraph(read) = &document.body().blocks[0] else { panic!("a paragraph") };
    assert_eq!(read.properties.no_hyphenation, Some(true));
}
