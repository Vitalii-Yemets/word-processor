//! Text flowing down columns rather than across the page.

use wp_docx::model::{Block, Body, Paragraph};
use wp_docx::Document;
use wp_layout::{FontLibrary, LayoutEngine, PageMetrics};

/// A document of many short paragraphs, which is enough to fill a page.
fn document(paragraphs: usize, columns: usize) -> Document {
    let mut body = Body::default();
    for index in 0..paragraphs {
        body.blocks.push(Block::Paragraph(Paragraph::text(&format!("Paragraph number {index}"))));
    }
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    let mut document = Document::open(&bytes).expect("reopening");
    if columns > 1 {
        assert!(document.set_columns(columns, 720));
    }
    document
}

/// The fonts, read once per test.
///
/// Leaked rather than kept: a `FontLibrary` caches lazily and so is not
/// shareable between threads, and the tests run on several. There is nothing
/// to free at the end of a test binary anyway.
fn library() -> &'static FontLibrary {
    Box::leak(Box::new(FontLibrary::scan_system()))
}

#[test]
fn one_column_is_the_full_width_of_the_text_area() {
    let metrics = PageMetrics::default();
    assert_eq!(metrics.column_width(), metrics.text_width());
}

#[test]
fn two_columns_are_each_a_little_less_than_half_the_width() {
    let metrics = PageMetrics { columns: 2, ..PageMetrics::default() };
    // Half, minus the gap that has to fit between them.
    assert!(metrics.column_width() < metrics.text_width() / 2.0);
    assert!(metrics.column_width() > metrics.text_width() / 2.0 - metrics.column_gap);
}

#[test]
fn text_in_two_columns_needs_fewer_pages_than_the_same_text_in_one() {
    if library().is_empty() {
        return;
    }
    let mut engine = LayoutEngine::new(library());
    let single = engine.layout_document(&document(120, 1)).len();
    let double = engine.layout_document(&document(120, 2)).len();

    assert!(single > 1, "the sample should run to more than one page");
    assert!(double < single, "two columns took {double} pages and one took {single}");
}

#[test]
fn the_second_column_starts_further_across_the_page_than_the_first() {
    if library().is_empty() {
        return;
    }
    let mut engine = LayoutEngine::new(library());
    let pages = engine.layout_document(&document(120, 2));
    let first = &pages[0];

    let leftmost = first.lines.iter().map(|line| line.left).fold(f32::MAX, f32::min);
    let rightmost = first.lines.iter().map(|line| line.left).fold(f32::MIN, f32::max);
    assert!(
        rightmost > leftmost + first.width / 4.0,
        "every line started at about the same place, so nothing flowed into a second column"
    );
}

#[test]
fn a_column_is_filled_to_the_bottom_before_the_next_one_is_started() {
    if library().is_empty() {
        return;
    }
    let mut engine = LayoutEngine::new(library());
    let pages = engine.layout_document(&document(120, 2));
    let first = &pages[0];

    let leftmost = first.lines.iter().map(|line| line.left).fold(f32::MAX, f32::min);
    let left_column_bottom = first
        .lines
        .iter()
        .filter(|line| (line.left - leftmost).abs() < 1.0)
        .map(|line| line.baseline)
        .fold(f32::MIN, f32::max);

    // The left column should reach most of the way down the page before
    // anything appears in the right one.
    assert!(
        left_column_bottom > first.height * 0.6,
        "the first column only reached {left_column_bottom} of {}",
        first.height
    );
}

#[test]
fn a_click_still_finds_the_text_it_landed_on_in_the_second_column() {
    if library().is_empty() {
        return;
    }
    let mut engine = LayoutEngine::new(library());
    let pages = engine.layout_document(&document(120, 2));
    let first = &pages[0];

    let rightmost = first.lines.iter().map(|line| line.left).fold(f32::MIN, f32::max);
    let Some(line) = first.lines.iter().find(|line| (line.left - rightmost).abs() < 1.0) else {
        panic!("nothing was placed in the second column");
    };

    // A point inside a line of the right-hand column has to map back to a
    // caret position, or clicking there would do nothing.
    let found = first.position_at(line.left + 4.0, line.baseline - 2.0);
    assert!(found.is_some(), "a click in the second column found no text");
}
