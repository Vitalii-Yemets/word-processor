//! What happens to somebody else's macros.
//!
//! This program does not run them — see the roadmap, where that is a decision
//! and not an oversight — and the thing it must therefore never do is lose
//! them. A document whose macros were quietly dropped by a word processor
//! that could not read them would be a document somebody has to write again.

use wp_docx::kinds::Kind;
use wp_docx::model::{Block, Body, Paragraph};
use wp_docx::Document;

/// Bytes that stand in for a Visual Basic project.
///
/// A real one is a compound file, and what this test is about is that the
/// bytes come out as they went in — so bytes that could not be mistaken for
/// anything this program understands are the right ones to use.
const PROJECT: &[u8] = &[
    0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1, 0x00, 0x01, 0x02, 0x03, 0xFF, 0xFE, 0xFD, 0xFC,
    0x2A, 0x2A, 0x2A, 0x2A,
];

/// A macro-enabled document with a project in it.
fn with_macros() -> Document {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("A document with macros in it")));
    let mut document = Document::create(&body).expect("a document");
    document.set_kind(Kind::MacroEnabledDocument);

    let bytes = document.save().expect("saving");
    let mut package = wp_opc::Package::open(&bytes).expect("a package");
    package.add_part(
        "word/vbaProject.bin",
        "application/vnd.ms-office.vbaProject",
        PROJECT.to_vec(),
    );
    let mut relationships = package.relationships("word/document.xml").expect("relationships");
    relationships.add(
        "http://schemas.microsoft.com/office/2006/relationships/vbaProject",
        "vbaProject.bin",
        wp_opc::TargetMode::Internal,
    );
    package.set_relationships(&relationships).expect("writing the relationships");

    let bytes = package.save().expect("saving the package");
    Document::open(&bytes).expect("reopening")
}

#[test]
fn a_document_says_whether_it_carries_macros() {
    assert!(with_macros().has_macros());

    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("plain")));
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    assert!(!Document::open(&bytes).expect("reopening").has_macros());
}

#[test]
fn somebody_elses_macros_come_out_of_this_program_as_they_went_in() {
    let document = with_macros();
    let bytes = document.save().expect("saving");
    let package = wp_opc::Package::open(&bytes).expect("a package");
    assert_eq!(
        package.part("word/vbaProject.bin"),
        Some(PROJECT),
        "the macros did not survive being saved"
    );
    assert_eq!(
        package.content_type("word/vbaProject.bin"),
        Some("application/vnd.ms-office.vbaProject")
    );
}

#[test]
fn they_survive_the_document_being_edited() {
    let mut document = with_macros();
    document.set_caret(wp_docx::TextPosition::new(0, 0));
    assert!(document.type_text("Edited. "));

    let bytes = document.save().expect("saving");
    let reopened = Document::open(&bytes).expect("reopening");
    assert!(reopened.plain_text().starts_with("Edited. "), "the edit did not happen");
    assert!(reopened.has_macros(), "an edit lost the macros");
    assert_eq!(
        wp_opc::Package::open(&bytes).expect("a package").part("word/vbaProject.bin"),
        Some(PROJECT)
    );
}

#[test]
fn a_macro_free_kind_takes_them_out_rather_than_writing_a_file_word_would_refuse() {
    let mut document = with_macros();
    assert_eq!(document.kind(), Kind::MacroEnabledDocument);
    assert!(document.set_kind(Kind::Document));

    assert!(!document.has_macros());
    let bytes = document.save().expect("saving");
    let package = wp_opc::Package::open(&bytes).expect("a package");
    assert!(package.part("word/vbaProject.bin").is_none());
    assert!(package.content_type("word/vbaProject.bin").is_none());
    let relationships = package.relationships("word/document.xml").expect("relationships");
    assert!(
        !relationships.all().iter().any(|relationship| relationship.target.contains("vbaProject")),
        "the relationship is still pointing at a part that is gone"
    );
}

#[test]
fn taking_them_out_twice_takes_nothing_out_the_second_time() {
    let mut document = with_macros();
    assert!(document.remove_macros());
    assert!(!document.remove_macros());
}

#[test]
fn the_four_kinds_know_which_of_them_may_hold_macros() {
    assert!(!Kind::Document.allows_macros());
    assert!(!Kind::Template.allows_macros());
    assert!(Kind::MacroEnabledDocument.allows_macros());
    assert!(Kind::MacroEnabledTemplate.allows_macros());
}
