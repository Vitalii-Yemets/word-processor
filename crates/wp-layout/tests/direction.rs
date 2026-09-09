//! Mixed-direction text on the page: what is stored one way and drawn another.

use wp_docx::model::{Block, Body, Paragraph, ParagraphProperties};
use wp_docx::Document;
use wp_layout::{FontLibrary, LayoutEngine, Page, PageMetrics};

fn library() -> &'static FontLibrary {
    Box::leak(Box::new(FontLibrary::scan_system()))
}

fn document(text: &str, right_to_left: bool) -> Document {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph {
        properties: ParagraphProperties {
            right_to_left: right_to_left.then_some(true),
            ..ParagraphProperties::default()
        },
        runs: Paragraph::text(text).runs,
    }));
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

fn page(document: &Document) -> Page {
    let mut engine = LayoutEngine::new(library());
    engine.layout_document_with(document, PageMetrics::default()).remove(0)
}

/// Where each byte of the text was drawn, by the offset the glyph came from.
fn placed(page: &Page) -> Vec<(usize, f32)> {
    let mut found: Vec<(usize, f32)> = page
        .glyphs
        .iter()
        .filter(|glyph| !glyph.invisible && glyph.source_length > 0)
        .map(|glyph| (glyph.source.offset, glyph.x))
        .collect();
    found.sort_by_key(|(offset, _)| *offset);
    found
}

/// Whether the characters between two offsets were drawn right to left.
fn drawn_backwards(placed: &[(usize, f32)], range: core::ops::Range<usize>) -> bool {
    let inside: Vec<f32> =
        placed.iter().filter(|(offset, _)| range.contains(offset)).map(|(_, x)| *x).collect();
    inside.len() > 1 && inside.windows(2).all(|pair| pair[1] < pair[0])
}

#[test]
fn english_is_drawn_in_the_order_it_is_stored() {
    let document = document("hello world", false);
    let placed = placed(&page(&document));
    assert!(placed.windows(2).all(|pair| pair[1].1 > pair[0].1), "{placed:?}");
}

#[test]
fn hebrew_in_an_english_sentence_is_turned_round() {
    // Stored in reading order; drawn from the right. The English around it is
    // untouched, which is the whole difficulty of mixed text.
    let text = "The word שלום means peace";
    let document = document(text, false);
    let placed = placed(&page(&document));

    let start = text.find('ש').expect("the Hebrew is there");
    let end = start + "שלום".len();
    assert!(drawn_backwards(&placed, start..end), "the Hebrew was not turned: {placed:?}");

    let english = text.find("means").expect("the English is there");
    let after: Vec<f32> =
        placed.iter().filter(|(offset, _)| *offset >= english).map(|(_, x)| *x).collect();
    assert!(after.windows(2).all(|pair| pair[1] > pair[0]), "the English was turned too");
}

#[test]
fn a_hebrew_paragraph_puts_its_english_the_right_way_round() {
    // A right-to-left paragraph: the Hebrew is drawn from the right, and the
    // English word inside it still reads left to right.
    let text = "שלום Word שלום";
    let document = document(text, true);
    let placed = placed(&page(&document));

    let start = text.find('W').expect("the English is there");
    let end = start + "Word".len();
    let inside: Vec<f32> = placed
        .iter()
        .filter(|(offset, _)| (start..end).contains(offset))
        .map(|(_, x)| *x)
        .collect();
    assert!(inside.windows(2).all(|pair| pair[1] > pair[0]), "the English came out backwards");
}

#[test]
fn a_number_in_a_hebrew_paragraph_is_not_reversed() {
    let text = "שלום 1994 שלום";
    let document = document(text, true);
    let placed = placed(&page(&document));

    let start = text.find('1').expect("the number is there");
    let inside: Vec<f32> = placed
        .iter()
        .filter(|(offset, _)| (start..start + 4).contains(offset))
        .map(|(_, x)| *x)
        .collect();
    assert_eq!(inside.len(), 4, "the number was not laid out");
    assert!(inside.windows(2).all(|pair| pair[1] > pair[0]), "the year was drawn backwards");
}

#[test]
fn a_right_to_left_paragraph_of_hebrew_reads_from_the_right() {
    let text = "שלום עולם";
    let document = document(text, true);
    let placed = placed(&page(&document));
    assert!(drawn_backwards(&placed, 0..text.len()), "{placed:?}");
}
