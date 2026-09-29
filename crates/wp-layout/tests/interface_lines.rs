//! A line of the interface on its own — a button's name, the strip's "page
//! 12 of 40", a message — drawn in the order its reader reads it.
//!
//! Such a line had been turned round whole whenever it began in a script
//! read from the right, which is right for a line of nothing but Hebrew and
//! wrong for one with a number in it, or a word of English, or a bracket.

use wp_bidi::Direction;
use wp_layout::{FontLibrary, LayoutEngine};
use wp_raster::Color;

fn library() -> &'static FontLibrary {
    Box::leak(Box::new(FontLibrary::scan_system()))
}

/// The line as drawn from the left, a character a glyph: each glyph named
/// by the character of the line, or the bracket, that is drawn with it.
fn drawn(engine: &mut LayoutEngine<'_>, text: &str) -> String {
    let mut known = Vec::new();
    for character in text.chars().chain(['(', ')']) {
        let alone = engine.simple_line(&character.to_string(), 0.0, 20.0, 10.0, Color::BLACK);
        if let Some(glyph) = alone.glyphs.first() {
            known.push(((glyph.face, glyph.glyph), character));
        }
    }
    let line = engine.simple_line(text, 0.0, 20.0, 10.0, Color::BLACK);
    line.glyphs
        .iter()
        .map(|glyph| {
            known
                .iter()
                .find(|(drawn, _)| *drawn == (glyph.face, glyph.glyph))
                .map_or('?', |(_, character)| *character)
        })
        .collect()
}

#[test]
fn a_number_in_a_line_of_hebrew_keeps_its_digits_in_order() {
    let mut engine = LayoutEngine::new(library());
    engine.set_interface_direction(Some(Direction::RightToLeft));
    // Page 12 of 40: from the left, the 40, then "of" and the 12, and the
    // word for page on the right — and not 04 and 21.
    assert_eq!(drawn(&mut engine, "עמוד 12 מתוך 40"), "40 ךותמ 12 דומע");
}

#[test]
fn a_bracket_read_from_the_right_is_drawn_as_the_other_end_of_its_pair() {
    let mut engine = LayoutEngine::new(library());
    // English (United Kingdom), in Hebrew: the brackets close round the
    // country, where turning the line round had them both facing out.
    assert_eq!(drawn(&mut engine, "אנגלית (בריטניה)"), "(הינטירב) תילגנא");
}

#[test]
fn a_message_of_a_window_read_from_the_right_reads_from_the_right() {
    // "{0} saved", with an English name for the {0}: read from the right,
    // the name and then the word. A line that took its direction from its
    // first letter would put the word first.
    let mut engine = LayoutEngine::new(library());
    engine.set_interface_direction(Some(Direction::RightToLeft));
    assert_eq!(drawn(&mut engine, "report נשמר"), "רמשנ report");

    engine.set_interface_direction(None);
    assert_eq!(drawn(&mut engine, "report נשמר"), "report רמשנ");
}

#[test]
fn english_in_a_window_read_from_the_right_is_still_english() {
    let mut engine = LayoutEngine::new(library());
    engine.set_interface_direction(Some(Direction::RightToLeft));
    assert_eq!(drawn(&mut engine, "Heading 1"), "Heading 1");
    assert_eq!(drawn(&mut engine, "100%"), "100%");
}
