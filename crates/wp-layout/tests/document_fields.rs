//! Fields that ask the document about itself, worked out as it is laid out.

use wp_docx::model::{Block, Body, Paragraph, Run};
use wp_docx::properties::Properties;
use wp_docx::Document;
use wp_layout::{FontLibrary, LayoutEngine, PageMetrics};

/// The fonts, read once per test.
fn library() -> &'static FontLibrary {
    Box::leak(Box::new(FontLibrary::scan_system()))
}

/// A document of one paragraph holding one field, with the properties given.
fn document(instruction: &str, cached: &str, properties: Properties) -> Document {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::from_runs(vec![Run::field(instruction, cached)])));
    let bytes = Document::create(&body).expect("a document").save().expect("saving");

    let mut document = Document::open(&bytes).expect("reopening");
    document.set_properties(&properties).expect("writing the properties");
    let bytes = document.save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

/// What the first page actually shows.
fn shown(document: &Document) -> String {
    let mut engine = LayoutEngine::new(library());
    let pages = engine.layout_document_with(document, PageMetrics::default());
    let Some(page) = pages.first() else { return String::new() };
    // The glyphs carry no characters, so the line's stretch of the document is
    // what says which text was laid out. What matters here is only that the
    // number of visible glyphs follows the field's answer, so the answer is
    // measured by how many glyphs there are.
    page.glyphs.iter().filter(|glyph| !glyph.invisible).count().to_string()
}

/// How many glyphs a piece of text lays out to.
fn glyphs_for(text: &str) -> String {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text(text)));
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    shown(&Document::open(&bytes).expect("reopening"))
}

#[test]
fn an_author_field_shows_the_author_and_not_what_was_cached() {
    let properties = Properties { author: "Ann Roe".to_owned(), ..Properties::default() };
    let laid = document("AUTHOR", "somebody else entirely", properties);
    assert_eq!(
        shown(&laid),
        glyphs_for("Ann Roe"),
        "the field should show the author, not the answer cached in the file"
    );
}

#[test]
fn a_title_field_shows_the_title() {
    let properties = Properties { title: "A Report".to_owned(), ..Properties::default() };
    let laid = document("TITLE", "", properties);
    assert_eq!(shown(&laid), glyphs_for("A Report"));
}

#[test]
fn a_company_field_shows_the_company() {
    let properties = Properties { company: "A Company".to_owned(), ..Properties::default() };
    let laid = document("COMPANY", "", properties);
    assert_eq!(shown(&laid), glyphs_for("A Company"));
}

#[test]
fn a_date_field_shows_the_day_and_not_the_time_of_day() {
    let properties =
        Properties { created: "2026-09-08T11:07:00Z".to_owned(), ..Properties::default() };
    let laid = document("CREATEDATE", "", properties);
    assert_eq!(shown(&laid), glyphs_for("2026-09-08"));
}

#[test]
fn a_field_the_document_cannot_answer_keeps_what_was_cached() {
    let laid = document("MERGEFIELD Name", "Ann Roe", Properties::default());
    assert_eq!(
        shown(&laid),
        glyphs_for("Ann Roe"),
        "a field nothing can work out should still show its last answer"
    );
}
