//! `w:caps` drawn in the language the run is written in.
//!
//! The file half is proved in `wp-docx`: Change Case rewrites the text the way
//! the language says. This is the other half — capitals *drawn* over text that
//! is not changed — and it has to give the same answer, or a Turkish word
//! looks right until somebody turns the capitals off.

use wp_docx::model::{Block, Body, Paragraph, Run};
use wp_docx::Document;
use wp_layout::{FontLibrary, LayoutEngine, Page, PageMetrics};

fn library() -> &'static FontLibrary {
    Box::leak(Box::new(FontLibrary::scan_system()))
}

/// One run, drawn in capitals, in the given language.
fn drawn(text: &str, language: &str) -> Vec<Page> {
    let mut run = Run::text(text).in_language(language);
    run.properties.caps = Some(true);

    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::from_runs(vec![run])));
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    let document = Document::open(&bytes).expect("reopening");
    LayoutEngine::new(library()).layout_document_with(&document, PageMetrics::default())
}

/// Which glyphs were drawn, in order.
fn glyphs(pages: &[Page]) -> Vec<u16> {
    pages
        .iter()
        .flat_map(|page| page.glyphs.iter())
        .filter(|glyph| !glyph.invisible)
        .map(|glyph| glyph.glyph.0)
        .collect()
}

/// The glyph a font draws for one character, for comparing against.
fn glyph_of(character: char) -> u16 {
    let pages = drawn(&character.to_string(), "en-GB");
    // Drawn in capitals, so this is the capital's glyph.
    glyphs(&pages).first().copied().unwrap_or(0)
}

#[test]
fn a_turkish_i_is_drawn_with_its_dot() {
    // The capital of a Turkish i is İ, and drawing it as I is drawing a
    // different letter.
    let turkish = glyphs(&drawn("i", "tr-TR"));
    assert_eq!(turkish.len(), 1);
    assert_eq!(turkish[0], glyph_of('\u{0130}'), "the dot was lost");
}

#[test]
fn an_english_i_is_drawn_without_one() {
    let english = glyphs(&drawn("i", "en-GB"));
    assert_eq!(english[0], glyph_of('I'));
    assert_ne!(english[0], glyph_of('\u{0130}'));
}

#[test]
fn what_is_drawn_is_what_change_case_would_write() {
    // The two halves of the same rule: capitals drawn over text that has not
    // changed, and capitals written into text that has.
    use wp_docx::page::CaseChange;

    let written = CaseChange::Upper
        .applied_to("i", wp_docx::casing::Tailoring::of("tr-TR"))
        .chars()
        .map(glyph_of)
        .collect::<Vec<_>>();
    assert_eq!(glyphs(&drawn("i", "tr-TR")), written);
}

#[test]
fn a_sharp_s_is_drawn_as_two_letters() {
    // One character of the document, two glyphs on the page — which is what
    // makes this worth a test: the run is not one glyph per character.
    let drawn_glyphs = glyphs(&drawn("a\u{00DF}b", "de-DE"));
    assert_eq!(drawn_glyphs.len(), 4, "ß should be drawn as SS");
    assert_eq!(drawn_glyphs[1], glyph_of('S'));
    assert_eq!(drawn_glyphs[2], glyph_of('S'));
}

#[test]
fn a_greek_accent_is_not_drawn_in_capitals() {
    // ά is drawn Α: one glyph, and not the accented capital.
    let greek = glyphs(&drawn("\u{03AC}", "el-GR"));
    assert_eq!(greek.len(), 1);
    assert_eq!(greek[0], glyph_of('\u{0391}'));
}
