//! What a page number prints: the section's own numbering, not the page's
//! place in the document.

use wp_docx::model::{Block, Body, Paragraph};
use wp_docx::sections::{NumberFormat, PageNumbering};
use wp_docx::{Document, TextPosition};
use wp_layout::{FontLibrary, LayoutEngine, PageMetrics};

fn library() -> &'static FontLibrary {
    Box::leak(Box::new(FontLibrary::scan_system()))
}

/// A document of several pages with a footer that prints the page number.
fn document() -> Document {
    let mut body = Body::default();
    for index in 0..90 {
        body.blocks.push(Block::Paragraph(Paragraph::text(&format!("Paragraph {index}"))));
    }
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    let mut document = Document::open(&bytes).expect("reopening");
    document
        .set_furniture(
            wp_docx::furniture::Furniture::Footer,
            wp_docx::furniture::Preset::PageNumber,
            wp_docx::model::Alignment::Center,
            "",
        )
        .expect("a footer");
    let bytes = document.save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

/// What each page's footer says, as the glyphs it was drawn with.
///
/// A glyph carries no character, so the footers are read as glyph numbers and
/// compared against the same words laid out through the same fonts. Two runs of
/// the same text give the same glyphs; anything else means the page said
/// something else.
fn footers(document: &Document) -> Vec<Vec<u16>> {
    let mut engine = LayoutEngine::new(library());
    let pages = engine.layout_document_with(document, PageMetrics::default());
    pages
        .iter()
        .map(|page| {
            // The footer is what was drawn below the last line of text.
            let lowest = page.lines.iter().map(|line| line.baseline).fold(0.0f32, f32::max);
            let mut found: Vec<(f32, u16)> = page
                .glyphs
                .iter()
                .filter(|glyph| glyph.baseline > lowest && !glyph.invisible)
                .map(|glyph| (glyph.x, glyph.glyph.0))
                .collect();
            found.sort_by(|(one, _), (other, _)| one.total_cmp(other));
            found.into_iter().map(|(_, glyph)| glyph).collect()
        })
        .collect()
}

/// The same words, laid out on their own, to compare a footer against.
fn glyphs_of(text: &str) -> Vec<u16> {
    let mut engine = LayoutEngine::new(library());
    let line = engine.simple_line(text, 0.0, 0.0, 11.0, wp_raster::Color::BLACK);
    line.glyphs.iter().map(|glyph| glyph.glyph.0).collect()
}

#[test]
fn the_pages_are_numbered_from_one() {
    let laid = footers(&document());
    assert!(laid.len() > 2, "the document did not run to three pages");
    assert_eq!(laid[0], glyphs_of("1"));
    assert_eq!(laid[1], glyphs_of("2"));
}

#[test]
fn a_section_can_ask_for_roman_figures() {
    let mut document = document();
    document.set_page_numbering(PageNumbering { start: None, format: NumberFormat::LowerRoman });
    let laid = footers(&document);
    assert_eq!(laid[0], glyphs_of("i"));
    assert_eq!(laid[1], glyphs_of("ii"));
    assert_eq!(laid[2], glyphs_of("iii"));
}

#[test]
fn a_section_can_start_at_a_number_of_its_own() {
    let mut document = document();
    document.set_page_numbering(PageNumbering { start: Some(10), format: NumberFormat::Decimal });
    let laid = footers(&document);
    assert_eq!(laid[0], glyphs_of("10"));
    assert_eq!(laid[1], glyphs_of("11"));
}

#[test]
fn the_numbering_starts_again_where_a_section_says_so() {
    // Front matter in roman figures, then a body that starts again at one.
    let mut document = document();
    document.set_page_numbering(PageNumbering { start: None, format: NumberFormat::LowerRoman });

    document.set_caret(TextPosition::new(45, 0));
    document.insert_section_break(wp_docx::sections::Start::NextPage);
    document.set_page_numbering(PageNumbering { start: Some(1), format: NumberFormat::Decimal });

    let bytes = document.save().expect("saving");
    let document = Document::open(&bytes).expect("reopening");
    let laid = footers(&document);

    assert_eq!(laid[0], glyphs_of("i"), "the front matter is not in roman figures");
    let restarted = laid
        .iter()
        .position(|footer| *footer == glyphs_of("1"))
        .expect("the body starts its numbering again");
    assert!(restarted > 0, "the body did not come after the front matter");
    assert_eq!(laid.get(restarted + 1), Some(&glyphs_of("2")));
}
