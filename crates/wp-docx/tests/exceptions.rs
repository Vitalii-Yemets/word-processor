//! The stretches of a document that carry their own rule about who may edit
//! them: Word's Block Authors, and the exceptions a restriction lets through.

use wp_docx::model::{Block, Body, Paragraph};
use wp_docx::permissions::EVERYONE;
use wp_docx::protection::{EditMode, Protection};
use wp_docx::{Document, TextPosition};

fn document() -> Document {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("One two three four five")));
    body.blocks.push(Block::Paragraph(Paragraph::text("Another paragraph entirely")));
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

fn round_trip(document: &Document) -> Document {
    let bytes = document.save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

/// Selects a stretch of the first paragraph.
fn select(document: &mut Document, from: usize, to: usize) {
    document.set_caret(TextPosition::new(0, from));
    document.extend_selection_to(TextPosition::new(0, to));
}

#[test]
fn a_document_marks_nothing_to_begin_with() {
    assert!(document().locked_regions().is_empty());
}

#[test]
fn a_stretch_everyone_may_edit_survives_being_written_and_read_back() {
    let mut document = document();
    select(&mut document, 4, 11);
    assert!(document.allow_everyone());

    let document = round_trip(&document);
    let marked = document.locked_regions();
    assert_eq!(marked.len(), 1, "{marked:?}");
    assert!(marked[0].for_everyone(), "it was not written for everybody: {:?}", marked[0]);
    assert!(marked[0].admits("anybody at all"));
    assert_eq!(marked[0].start, TextPosition::new(0, 4));
    assert_eq!(marked[0].end, TextPosition::new(0, 11));
}

#[test]
fn it_is_written_the_way_word_writes_it() {
    let mut document = document();
    select(&mut document, 4, 11);
    document.allow_everyone();
    let bytes = document.save().expect("saving");
    let package = wp_opc::Package::open(&bytes).expect("a package");
    let xml = package.xml_part("word/document.xml").expect("the document").expect("readable");

    assert!(xml.contains("w:permStart"), "{xml}");
    assert!(xml.contains("w:permEnd"));
    // The group, not a name: Word writes `w:edGrp` for an exception and
    // `w:ed` for one person.
    assert!(xml.contains("w:edGrp=\"everyone\""), "{xml}");
}

#[test]
fn a_stretch_blocked_for_one_person_admits_that_person_and_no_other() {
    let mut document = document();
    select(&mut document, 4, 11);
    assert!(document.block_authors("Agnes Nitt"));

    let marked = round_trip(&document).locked_regions();
    assert_eq!(marked.len(), 1);
    assert!(!marked[0].for_everyone());
    assert!(marked[0].admits("Agnes Nitt"));
    assert!(marked[0].admits("agnes nitt"), "a name is not case-sensitive");
    assert!(!marked[0].admits("Perdita X"));
    assert_eq!(marked[0].named(), "Agnes Nitt");
}

#[test]
fn a_group_this_program_cannot_enumerate_admits_nobody() {
    // `w:edGrp="administrators"` names people out of a directory, and a
    // program that guessed would be inventing a permission.
    let mut document = document();
    select(&mut document, 4, 11);
    document.allow_everyone();
    let bytes = document.save().expect("saving");
    let mut package = wp_opc::Package::open(&bytes).expect("a package");
    let xml = package.xml_part("word/document.xml").expect("the document").expect("readable");
    let xml = xml.replace("everyone", "administrators");
    package.add_part(
        "word/document.xml",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml",
        xml.into_bytes(),
    );

    let document = Document::open(&package.save().expect("saving")).expect("reopening");
    let marked = document.locked_regions();
    assert_eq!(marked.len(), 1);
    assert!(!marked[0].for_everyone());
    assert!(!marked[0].admits("anybody"), "a group nobody can check let somebody in");
    assert_eq!(marked[0].named(), "administrators", "and it says which group was wanted");
}

#[test]
fn the_stretch_a_position_is_in_is_found() {
    let mut document = document();
    select(&mut document, 4, 11);
    document.allow_everyone();
    let document = round_trip(&document);

    assert!(document.locked_at(TextPosition::new(0, 6)).is_some());
    assert!(document.locked_at(TextPosition::new(0, 4)).is_some(), "the first character");
    assert!(document.locked_at(TextPosition::new(0, 11)).is_some(), "the last");
    assert!(document.locked_at(TextPosition::new(0, 2)).is_none(), "before it");
    assert!(document.locked_at(TextPosition::new(0, 20)).is_none(), "after it");
    assert!(document.locked_at(TextPosition::new(1, 0)).is_none(), "the next paragraph");
}

#[test]
fn an_exception_stands_beside_the_restriction_it_is_an_exception_to() {
    let mut document = document();
    select(&mut document, 4, 11);
    document.allow_everyone();
    document.set_protection(Some(&Protection::new(EditMode::ReadOnly)));

    let document = round_trip(&document);
    assert_eq!(document.protection(), Some(EditMode::ReadOnly));
    assert_eq!(document.locked_regions().len(), 1, "the exception went with the restriction");
    assert!(document.locked_regions()[0].for_everyone());
}

#[test]
fn a_stretch_can_be_taken_off_again() {
    let mut document = document();
    select(&mut document, 4, 11);
    document.allow_everyone();

    document.set_caret(TextPosition::new(0, 6));
    assert!(document.unblock_authors());
    assert!(round_trip(&document).locked_regions().is_empty());
}

#[test]
fn two_stretches_are_told_apart_and_keep_their_own_rules() {
    let mut document = document();
    select(&mut document, 0, 3);
    assert!(document.allow_everyone());
    select(&mut document, 12, 17);
    assert!(document.block_authors("Agnes Nitt"));

    let marked = round_trip(&document).locked_regions();
    assert_eq!(marked.len(), 2, "{marked:?}");
    assert!(marked[0].for_everyone(), "the first lost its group");
    assert_eq!(marked[1].named(), "Agnes Nitt");
    assert_ne!(marked[0].id, marked[1].id, "both pairs carry the same number");
}

#[test]
fn nothing_selected_marks_nothing() {
    let mut document = document();
    document.set_caret(TextPosition::new(0, 4));
    assert!(!document.allow_everyone(), "a stretch of no characters was marked");
    assert!(document.locked_regions().is_empty());
}

#[test]
fn a_stretch_may_run_across_a_paragraph_break() {
    let mut document = document();
    document.set_caret(TextPosition::new(0, 18));
    document.extend_selection_to(TextPosition::new(1, 7));
    assert!(document.allow_everyone());

    let marked = round_trip(&document).locked_regions();
    assert_eq!(marked.len(), 1, "{marked:?}");
    assert_eq!(marked[0].start, TextPosition::new(0, 18));
    assert_eq!(marked[0].end, TextPosition::new(1, 7));
    assert!(marked[0].covers(TextPosition::new(1, 0)), "the start of the second paragraph");
}

#[test]
fn the_name_everyone_is_the_one_the_format_uses() {
    assert_eq!(EVERYONE, "everyone");
}
