//! Carrying a style from one document to another, and taking one away.

use wp_docx::model::{Block, Body, Paragraph};
use wp_docx::styles::StyleDefinition;
use wp_docx::Document;

fn document() -> Document {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("One")));
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

fn round_trip(document: &Document) -> Document {
    let bytes = document.save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

/// A document with a style of its own in it.
fn with_a_style(id: &str, name: &str) -> Document {
    let mut document = document();
    let mut wanted = StyleDefinition {
        id: id.to_owned(),
        name: name.to_owned(),
        based_on: None,
        next: None,
        paragraph: Default::default(),
        run: Default::default(),
    };
    wanted.run.bold = Some(true);
    assert!(document.set_style(&wanted));
    document
}

#[test]
fn a_style_copied_from_another_document_arrives_whole() {
    let source = with_a_style("HouseQuote", "House Quote");
    let mut into = document();
    assert!(into.styles().get("HouseQuote").is_none());

    assert!(into.copy_style_from(&source, "HouseQuote"));
    let into = round_trip(&into);
    let style = into.styles().get("HouseQuote").expect("the style did not arrive");
    assert_eq!(style.name.as_deref(), Some("House Quote"));
    assert_eq!(style.run.bold, Some(true), "what the style said was lost on the way");
}

#[test]
fn a_style_that_is_not_in_the_other_document_is_not_copied() {
    let source = document();
    let mut into = document();
    assert!(!into.copy_style_from(&source, "NoSuchStyle"));
}

#[test]
fn copying_onto_a_style_of_the_same_identifier_replaces_it() {
    let source = with_a_style("Quote", "The house one");
    let mut into = with_a_style("Quote", "The old one");

    assert!(into.copy_style_from(&source, "Quote"));
    let into = round_trip(&into);
    assert_eq!(
        into.styles().get("Quote").and_then(|style| style.name.clone()).as_deref(),
        Some("The house one")
    );
    // And there is one of it, not two.
    assert_eq!(into.styles().all().iter().filter(|style| style.id == "Quote").count(), 1);
}

#[test]
fn a_style_taken_away_is_gone_and_the_rest_are_not() {
    let mut document = with_a_style("Quote", "Quote");
    let before = document.styles().all().len();
    assert!(document.delete_style("Quote"));

    let document = round_trip(&document);
    assert!(document.styles().get("Quote").is_none());
    assert_eq!(document.styles().all().len(), before - 1, "something else went with it");
    assert!(document.styles().get("Heading1").is_some(), "the built-in styles went too");
}

#[test]
fn taking_away_one_that_is_not_there_changes_nothing() {
    let mut document = document();
    assert!(!document.delete_style("NoSuchStyle"));
}

#[test]
fn a_renamed_style_keeps_the_identifier_everything_refers_to_it_by() {
    let mut document = with_a_style("Quote", "Quote");
    document.set_paragraph_style(0, Some("Quote"));
    assert!(document.rename_style("Quote", "House Quote"));

    let document = round_trip(&document);
    let style = document.styles().get("Quote").expect("it is still there by identifier");
    assert_eq!(style.name.as_deref(), Some("House Quote"));
    // The paragraph still finds it, which is the whole reason the identifier
    // does not change.
    assert_eq!(document.style_here().as_deref(), Some("Quote"));
    assert!(document.styles().resolve_run(Some("Quote"), &Default::default()).bold);
}

#[test]
fn a_name_of_nothing_is_refused() {
    let mut document = with_a_style("Quote", "Quote");
    assert!(!document.rename_style("Quote", "  "));
    assert_eq!(
        document.styles().get("Quote").and_then(|style| style.name.clone()).as_deref(),
        Some("Quote")
    );
}

#[test]
fn the_name_is_written_where_the_schema_wants_it() {
    let mut document = with_a_style("Quote", "Quote");
    document.rename_style("Quote", "House Quote");
    let bytes = document.save().expect("saving");
    let package = wp_opc::Package::open(&bytes).expect("a package");
    let styles = package.xml_part("word/styles.xml").expect("styles").expect("readable");

    let at = styles.find("w:styleId=\"Quote\"").expect("the style");
    let rest = &styles[at..];
    let name = rest.find("<w:name").expect("its name");
    // Before anything else the style says, which is where `w:name` goes.
    let properties = rest.find("<w:rPr").unwrap_or(usize::MAX);
    assert!(name < properties, "the name was written after the formatting");
    assert_eq!(styles.matches("w:styleId=\"Quote\"").count(), 1, "the style was written twice");
}
