//! The optional hyphen: a mark inside a word saying it may be broken there.
//!
//! Word calls it Ctrl+Hyphen, and what makes it awkward is that it is drawn
//! only sometimes — nothing at all in the middle of a line, a hyphen at the end
//! of one. So every test here lays the same text out twice, in a width that
//! breaks it and in a width that does not, and holds the two to what a reader
//! would see.

use wp_docx::model::{Block, Body, Paragraph, Run};
use wp_docx::Document;
use wp_layout::{FontLibrary, LayoutEngine, Page, PageMetrics};

const SOFT: char = '\u{00AD}';

fn library() -> &'static FontLibrary {
    Box::leak(Box::new(FontLibrary::scan_system()))
}

/// One paragraph laid out in a page of a given text width.
fn laid_out(text: &str, width: f32) -> Vec<Page> {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::from_runs(vec![Run::text(text)])));

    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    let document = Document::open(&bytes).expect("reopening");

    let metrics = PageMetrics {
        width: width + 144.0,
        margin_left: 72.0,
        margin_right: 72.0,
        ..PageMetrics::default()
    };
    LayoutEngine::new(library()).layout_document_with(&document, metrics)
}

/// What is actually drawn, line by line, as the glyphs that carry ink.
fn drawn(pages: &[Page]) -> Vec<String> {
    let Some(page) = pages.first() else { return Vec::new() };
    page.lines
        .iter()
        .map(|line| {
            page.glyphs[line.glyphs.clone()]
                .iter()
                .filter(|glyph| !glyph.invisible)
                .count()
                .to_string()
        })
        .collect()
}

/// How many glyphs a page draws in all.
fn visible(pages: &[Page]) -> usize {
    pages.iter().flat_map(|page| page.glyphs.iter()).filter(|glyph| !glyph.invisible).count()
}

#[test]
fn an_optional_hyphen_is_drawn_as_nothing_when_the_line_does_not_break() {
    // A wide page: the word fits, and what the reader sees is the word.
    let plain = laid_out("hyphenation", 400.0);
    let marked = laid_out(&format!("hy{SOFT}phen{SOFT}ation"), 400.0);

    assert_eq!(visible(&plain), visible(&marked), "the marks were drawn: {:?}", drawn(&marked));
    assert_eq!(plain.len(), 1);
    assert_eq!(marked[0].lines.len(), 1, "the word was broken though it fitted");
}

#[test]
fn it_takes_up_no_room_of_its_own() {
    // Which is what makes it safe to put one in every word: a document full of
    // them is set exactly as a document without them.
    let reach = |pages: &[Page]| -> f32 {
        pages[0].glyphs.iter().map(|glyph| glyph.x + glyph.advance).fold(0.0f32, f32::max)
    };
    let plain = laid_out("hyphenation", 400.0);
    let marked = laid_out(&format!("hy{SOFT}phen{SOFT}ation"), 400.0);
    assert!((reach(&plain) - reach(&marked)).abs() < 0.01);
}

#[test]
fn a_word_is_broken_at_the_mark_and_the_hyphen_appears() {
    // Narrow enough that the word cannot fit, wide enough that its first
    // piece can.
    let marked = laid_out(&format!("hy{SOFT}phen{SOFT}ation"), 60.0);
    assert!(marked[0].lines.len() > 1, "the word was not broken");

    // One more glyph is drawn than there are letters on the line: the hyphen.
    let plain = laid_out("hyphenation", 400.0);
    assert_eq!(visible(&marked), visible(&plain) + 1, "the hyphen was not drawn at the break");
}

#[test]
fn the_hyphen_is_drawn_at_the_end_of_the_line_that_broke() {
    let marked = laid_out(&format!("hy{SOFT}phen{SOFT}ation"), 60.0);
    let page = &marked[0];
    let first = &page.lines[0];

    let last = page.glyphs[first.glyphs.clone()]
        .iter()
        .rev()
        .find(|glyph| !glyph.invisible)
        .expect("the line draws something");
    // The hyphen points at the mark it was drawn for and covers none of the
    // text, which is what keeps a click on it landing beside the mark.
    assert_eq!(last.source_length, 0, "the hyphen claimed to be text");
    assert!(last.x > 0.0);
}

#[test]
fn the_hyphen_does_not_hang_past_the_margin() {
    // The room for it is made before the line is settled rather than after: a
    // hyphen measured afterwards is a hyphen in the margin.
    //
    // Where the margin is is asked of the engine rather than assumed: a page
    // of short words wraps at it, and the furthest any of those lines reaches
    // is where the text may reach.
    let width = 60.0;
    let filled = laid_out(&"no ".repeat(30), width);
    let edge = filled[0]
        .glyphs
        .iter()
        .filter(|glyph| !glyph.invisible)
        .map(|glyph| glyph.x + glyph.advance)
        .fold(0.0f32, f32::max);

    let marked = laid_out(&format!("hy{SOFT}phen{SOFT}ation"), width);
    for glyph in marked[0].glyphs.iter().filter(|glyph| !glyph.invisible) {
        assert!(
            glyph.x + glyph.advance <= edge + 0.01,
            "something was drawn at {} where the text stops at {edge}",
            glyph.x + glyph.advance
        );
    }
}

#[test]
fn the_last_line_of_a_paragraph_shows_no_hyphen() {
    // A word broken onto the last line ends the paragraph; the mark inside
    // the part that fits there is still a mark and nothing more.
    let marked = laid_out(&format!("hy{SOFT}phen{SOFT}ation"), 60.0);
    let page = &marked[0];
    let last = page.lines.last().expect("a line");

    let glyphs: Vec<&wp_layout::PositionedGlyph> =
        page.glyphs[last.glyphs.clone()].iter().filter(|glyph| !glyph.invisible).collect();
    assert!(
        glyphs.iter().all(|glyph| glyph.source_length > 0),
        "a hyphen was drawn at the end of the paragraph"
    );
}
