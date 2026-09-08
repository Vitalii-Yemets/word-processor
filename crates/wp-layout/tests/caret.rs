//! Where the caret goes, and in particular where a space puts it.

use wp_docx::model::{Block, Body, Paragraph};
use wp_docx::{Document, TextPosition};
use wp_layout::{FontLibrary, LayoutEngine, PageMetrics};

/// The fonts, read once per test.
fn library() -> &'static FontLibrary {
    Box::leak(Box::new(FontLibrary::scan_system()))
}

/// A document of one paragraph.
fn document(text: &str) -> Document {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text(text)));
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

/// Where the caret sits at an offset in the first paragraph.
fn caret_x(text: &str, offset: usize) -> f32 {
    let document = document(text);
    let mut engine = LayoutEngine::new(library());
    let pages = engine.layout_document_with(&document, PageMetrics::default());
    let page = pages.first().expect("a page");
    page.caret_at(TextPosition::new(0, offset)).expect("a caret").0
}

#[test]
fn the_caret_moves_along_as_letters_are_typed() {
    let first = caret_x("a", 1);
    let second = caret_x("ab", 2);
    assert!(second > first, "the caret should move on: {first} then {second}");
}

#[test]
fn a_space_at_the_end_moves_the_caret_as_a_letter_does() {
    // What pressing the space bar looks like: the text was "abc" and is now
    // "abc ". The caret has to move, or the space appears not to have gone in.
    let without = caret_x("abc", 3);
    let with = caret_x("abc ", 4);
    assert!(with > without, "the caret did not move for the space: {without} then {with}");
}

#[test]
fn several_spaces_move_the_caret_further_than_one() {
    let one = caret_x("abc ", 4);
    let three = caret_x("abc   ", 6);
    assert!(three > one, "three spaces should reach further than one: {one} then {three}");
}

#[test]
fn a_space_between_words_moves_the_caret_too() {
    let before = caret_x("abc def", 3);
    let after = caret_x("abc def", 4);
    assert!(after > before);
}

#[test]
fn the_caret_at_the_start_is_at_the_left_margin() {
    let start = caret_x("abc", 0);
    let after = caret_x("abc", 1);
    assert!(after > start);
}

#[test]
fn a_trailing_space_is_not_drawn_even_though_it_moves_the_caret() {
    let document = document("abc ");
    let mut engine = LayoutEngine::new(library());
    let pages = engine.layout_document_with(&document, PageMetrics::default());
    let page = pages.first().expect("a page");

    // Three letters are drawn; the space is recorded but not drawn.
    let drawn = page.glyphs.iter().filter(|glyph| !glyph.invisible).count();
    assert_eq!(drawn, 3, "the trailing space should not be drawn");
    assert!(
        page.glyphs.iter().any(|glyph| glyph.invisible),
        "the trailing space should still be recorded"
    );
}

#[test]
fn a_click_past_the_end_of_a_line_lands_after_the_trailing_space() {
    let document = document("abc ");
    let mut engine = LayoutEngine::new(library());
    let pages = engine.layout_document_with(&document, PageMetrics::default());
    let page = pages.first().expect("a page");

    let line = page.lines.first().expect("a line");
    let found = page.position_at(line.right + 20.0, line.baseline).expect("a position");
    assert_eq!(found.offset, 4, "clicking past the words should land after the space");
}
