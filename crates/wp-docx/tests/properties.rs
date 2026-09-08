//! What the document says about itself.

use wp_docx::model::{Block, Body, Paragraph};
use wp_docx::properties::Properties;
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

fn filled() -> Properties {
    Properties {
        title: "A Report".to_owned(),
        subject: "Quarterly figures".to_owned(),
        author: "Ann Roe".to_owned(),
        keywords: "figures; quarter; report".to_owned(),
        description: "What the quarter looked like.".to_owned(),
        last_modified_by: "John Doe".to_owned(),
        category: "Reports".to_owned(),
        company: "A Company".to_owned(),
        created: "2026-09-08T09:00:00Z".to_owned(),
        modified: "2026-09-08T11:07:00Z".to_owned(),
    }
}

#[test]
fn a_new_document_says_nothing_about_itself() {
    assert!(document().properties().is_empty());
}

#[test]
fn everything_written_reads_back_after_saving() {
    let mut document = document();
    assert!(document.set_properties(&filled()).expect("writing"));
    assert_eq!(round_trip(&document).properties(), filled());
}

#[test]
fn the_properties_live_in_parts_of_their_own() {
    let mut document = document();
    document.set_properties(&filled()).expect("writing");

    let reopened = round_trip(&document);
    assert!(reopened.package().part("docProps/core.xml").is_some(), "no core part");
    assert!(reopened.package().part("docProps/app.xml").is_some(), "no app part");
    // And not one word of them is in the text.
    assert_eq!(reopened.plain_text(), "plain");
}

#[test]
fn the_package_points_at_them() {
    let mut document = document();
    document.set_properties(&filled()).expect("writing");

    let reopened = round_trip(&document);
    let relationships = reopened.package().relationships("").expect("the package relationships");
    assert!(
        relationships.all().iter().any(|entry| entry.target == "docProps/core.xml"),
        "nothing points at the core properties"
    );
    assert!(
        relationships.all().iter().any(|entry| entry.target == "docProps/app.xml"),
        "nothing points at the extended properties"
    );
}

#[test]
fn the_dates_carry_the_type_word_expects() {
    let mut document = document();
    document.set_properties(&filled()).expect("writing");

    let reopened = round_trip(&document);
    let core =
        reopened.package().xml_part("docProps/core.xml").expect("the core part").expect("readable");
    assert!(core.contains("dcterms:W3CDTF"), "a date without its type is no date: {core}");
}

#[test]
fn the_file_says_what_wrote_it() {
    let mut document = document();
    document.set_properties(&filled()).expect("writing");

    let reopened = round_trip(&document);
    let app =
        reopened.package().xml_part("docProps/app.xml").expect("the app part").expect("readable");
    assert!(app.contains(wp_docx::properties::APPLICATION), "got {app}");
}

#[test]
fn writing_the_same_properties_twice_changes_nothing() {
    let mut document = document();
    assert!(document.set_properties(&filled()).expect("writing"));
    assert!(!document.set_properties(&filled()).expect("writing"));
}

#[test]
fn a_property_cleared_is_gone_rather_than_left_empty() {
    let mut document = document();
    document.set_properties(&filled()).expect("writing");
    document.set_properties(&Properties { title: String::new(), ..filled() }).expect("writing");

    let reopened = round_trip(&document);
    assert!(reopened.properties().title.is_empty());
    let core =
        reopened.package().xml_part("docProps/core.xml").expect("the core part").expect("readable");
    assert!(!core.contains("<dc:title"), "an empty title should not be written at all");
}

#[test]
fn the_properties_survive_being_written_twice_over() {
    let mut document = document();
    document.set_properties(&filled()).expect("writing");
    let second = Properties { title: "A Better Report".to_owned(), ..filled() };
    document.set_properties(&second).expect("writing");

    let reopened = round_trip(&document);
    assert_eq!(reopened.properties(), second);
    let core =
        reopened.package().xml_part("docProps/core.xml").expect("the core part").expect("readable");
    assert_eq!(core.matches("<dc:title>").count(), 1, "the title was written twice");
}

#[test]
fn setting_properties_marks_the_document_as_changed() {
    let mut document = document();
    assert!(!document.is_modified());
    document.set_properties(&filled()).expect("writing");
    assert!(document.is_modified());
}
