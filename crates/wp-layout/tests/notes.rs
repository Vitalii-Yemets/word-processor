//! Footnotes: the number in the text, and the note at the foot of the page.

use wp_docx::model::{Block, Body, Paragraph};
use wp_docx::notes::Kind;
use wp_docx::{Document, TextPosition};
use wp_layout::{FontLibrary, LayoutEngine};

fn document(paragraphs: usize) -> Document {
    let mut body = Body::default();
    for index in 0..paragraphs {
        body.blocks.push(Block::Paragraph(Paragraph::text(&format!("Paragraph {index} here."))));
    }
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

fn library() -> &'static FontLibrary {
    Box::leak(Box::new(FontLibrary::scan_system()))
}

#[test]
fn a_footnote_is_drawn_at_the_foot_of_the_page_its_mark_is_on() {
    if library().is_empty() {
        return;
    }
    let mut document = document(6);
    document.set_caret(TextPosition::new(0, 5));
    document.add_note(Kind::Footnote, "The note itself.").expect("adding");

    let mut engine = LayoutEngine::new(library());
    let pages = engine.layout_document(&document);
    let page = &pages[0];

    let body_bottom = page.lines.iter().map(|line| line.baseline).fold(f32::MIN, f32::max);
    let lowest = page.glyphs.iter().map(|glyph| glyph.baseline).fold(f32::MIN, f32::max);
    assert!(lowest > body_bottom, "the note was not drawn below the text");
    assert!(lowest < page.height, "the note fell off the bottom of the page");
}

#[test]
fn the_note_is_not_text_the_caret_can_reach() {
    if library().is_empty() {
        return;
    }
    let mut document = document(4);
    let mut engine = LayoutEngine::new(library());
    let before = engine.layout_document(&document)[0].lines.len();

    document.set_caret(TextPosition::new(0, 5));
    document.add_note(Kind::Footnote, "A note").expect("adding");
    let after = engine.layout_document(&document)[0].lines.len();

    assert_eq!(before, after, "the note added lines a click could land in");
}

#[test]
fn a_rule_is_drawn_above_the_notes() {
    if library().is_empty() {
        return;
    }
    let mut document = document(4);
    let mut engine = LayoutEngine::new(library());
    let before = engine.layout_document(&document)[0].decorations.len();

    document.set_caret(TextPosition::new(0, 5));
    document.add_note(Kind::Footnote, "A note").expect("adding");
    let after = engine.layout_document(&document)[0].decorations.len();

    assert!(after > before, "no rule was drawn above the note");
}

#[test]
fn footnotes_take_room_away_from_the_text() {
    if library().is_empty() {
        return;
    }
    // A page full of text, then a footnote on it: the text has to give way,
    // so the document grows.
    let mut document = document(120);
    let mut engine = LayoutEngine::new(library());
    let before = engine.layout_document(&document).len();

    document.set_caret(TextPosition::new(0, 5));
    for number in 0..8 {
        document.set_caret(TextPosition::new(number, 5));
        document.add_note(Kind::Footnote, "A note that takes up a line of its own.").expect("ok");
    }
    let after = engine.layout_document(&document).len();

    assert!(after >= before, "the document should not have got shorter");
    let page = &engine.layout_document(&document)[0];
    let lowest_line = page.lines.iter().map(|line| line.baseline).fold(f32::MIN, f32::max);
    assert!(
        lowest_line < page.height * 0.85,
        "the text ran into the notes: it reached {lowest_line} of {}",
        page.height
    );
}

#[test]
fn the_mark_shows_a_number_rather_than_nothing() {
    if library().is_empty() {
        return;
    }
    let mut document = document(2);
    let mut engine = LayoutEngine::new(library());
    let before = engine.layout_document(&document)[0].glyphs.len();

    document.set_caret(TextPosition::new(0, 5));
    document.add_note(Kind::Footnote, "A note").expect("adding");
    let after = engine.layout_document(&document)[0].glyphs.len();

    assert!(after > before + 1, "the mark and the note should both have drawn something");
}
