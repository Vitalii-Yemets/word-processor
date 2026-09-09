//! An engine that remembers must give the same answer as one that does not.
//!
//! Keeping what each paragraph measured to is what makes typing into a long
//! document as cheap as typing into a short one. It is also the sort of thing
//! that goes wrong quietly: a stale measurement shows as text of the wrong
//! width, a line broken in the wrong place, or a page that ends too soon — and
//! only in the document somebody has been editing for an hour, never in a
//! fresh one.
//!
//! So every test here says the same thing in a different way: whatever the
//! engine has been through, the pages it produces are the pages a new engine
//! produces from the same document.

use wp_docx::model::{Block, Body, Paragraph};
use wp_docx::{Document, TextPosition};
use wp_layout::{FontLibrary, LayoutEngine, Page, PageMetrics};

fn library() -> &'static FontLibrary {
    Box::leak(Box::new(FontLibrary::scan_system()))
}

/// A document of several pages, with headings and a table of contents field so
/// that more than plain text is exercised.
fn document() -> Document {
    let mut body = Body::default();
    for number in 0..30 {
        body.blocks.push(Block::Paragraph(
            Paragraph::text(&format!("Heading {number}")).with_style("Heading1"),
        ));
        for _ in 0..3 {
            body.blocks.push(Block::Paragraph(Paragraph::text(
                "Some words that go on for long enough to fill several lines of a page, so that \
                 the line breaking has something to decide and a change to one paragraph can be \
                 seen in the ones after it.",
            )));
        }
    }
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

/// The pages a brand-new engine gives for a document.
fn fresh(document: &Document) -> Vec<Page> {
    LayoutEngine::new(library()).layout_document_with(document, PageMetrics::default())
}

#[test]
fn laying_the_same_document_out_twice_gives_the_same_pages() {
    let document = document();
    let mut engine = LayoutEngine::new(library());
    let first = engine.layout_document_with(&document, PageMetrics::default());
    let second = engine.layout_document_with(&document, PageMetrics::default());
    assert!(first.len() > 3, "the document should fill several pages");
    assert_eq!(first, second, "the second time round came out differently");
}

#[test]
fn a_paragraph_that_changed_is_measured_again() {
    let mut document = document();
    let mut engine = LayoutEngine::new(library());
    engine.layout_document_with(&document, PageMetrics::default());

    // A word typed into the middle of the document.
    let middle = document.paragraph_count() / 2;
    document.insert_text(TextPosition::new(middle, 0), "Inserted words here. ");

    let again = engine.layout_document_with(&document, PageMetrics::default());
    assert_eq!(again, fresh(&document), "the edited document came out stale");
}

#[test]
fn a_paragraph_taken_away_moves_the_ones_after_it() {
    let mut document = document();
    let mut engine = LayoutEngine::new(library());
    engine.layout_document_with(&document, PageMetrics::default());

    // Deleting a paragraph shifts every paragraph after it up by one, so every
    // kept measurement after that point is now under the wrong number.
    let middle = document.paragraph_count() / 2;
    document.set_caret(TextPosition::new(middle, 0));
    document.merge_with_previous(middle);

    let again = engine.layout_document_with(&document, PageMetrics::default());
    assert_eq!(again, fresh(&document), "the paragraphs after the deletion came out stale");
}

#[test]
fn changing_the_resolution_forgets_what_was_measured_at_the_old_one() {
    let document = document();
    let mut engine = LayoutEngine::new(library());
    engine.layout_document_with(&document, PageMetrics::default());

    engine.set_dpi(192.0);
    let bigger = engine.layout_document_with(&document, PageMetrics::default());
    let expected = LayoutEngine::new(library())
        .with_dpi(192.0)
        .layout_document_with(&document, PageMetrics::default());
    assert_eq!(bigger, expected, "the pages were measured at the old resolution");
}

#[test]
fn changing_the_colour_of_automatic_text_forgets_the_old_colour() {
    let document = document();
    let mut engine = LayoutEngine::new(library());
    let dark = engine.layout_document_with(&document, PageMetrics::default());

    let white = wp_raster::Color::rgb(255, 255, 255);
    engine.set_automatic_colors(white, white);
    let light = engine.layout_document_with(&document, PageMetrics::default());

    assert_ne!(dark, light, "the text is drawn in the same colour on both papers");
    let expected = LayoutEngine::new(library())
        .with_automatic_colors(white, white)
        .layout_document_with(&document, PageMetrics::default());
    assert_eq!(light, expected);
}

#[test]
fn a_document_of_fields_is_not_kept_because_its_answers_move() {
    // A `PAGE` field says a different thing on every page it lands on, so its
    // paragraph is measured afresh every time. Adding a page before it must
    // change what it says.
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::from_runs(vec![wp_docx::model::Run::field(
        "PAGE", "1",
    )])));
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    let mut document = Document::open(&bytes).expect("reopening");

    let mut engine = LayoutEngine::new(library());
    engine.layout_document_with(&document, PageMetrics::default());

    document.insert_text(TextPosition::new(0, 0), "text before the field");
    let again = engine.layout_document_with(&document, PageMetrics::default());
    assert_eq!(again, fresh(&document));
}
