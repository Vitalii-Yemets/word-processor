//! Tests against a real PostScript-flavoured font.
//!
//! The other kind of font: outlines in a `CFF` table, written as a program in a
//! stack language rather than as a list of points. Every `.otf` file whose
//! signature reads `OTTO` is one of these, which is most of the fonts a
//! designer buys and all of the ones Adobe made.
//!
//! A hand-built table would prove the reader handles the shapes it was written
//! for. Only a font somebody else produced proves it handles the shapes that
//! actually occur: subroutines called from subroutines, the flex operators, a
//! width hidden in front of the first operator's arguments. The URW set — the
//! thirty-five fonts every PostScript printer has — is installed in the build
//! container for this.

use wp_font::{Font, PathCommand};

const FONT_PATH: &str = "/usr/share/fonts/opentype/urw-base35/NimbusRoman-Regular.otf";

fn load() -> Vec<u8> {
    std::fs::read(FONT_PATH).unwrap_or_else(|error| {
        panic!(
            "cannot read {FONT_PATH}: {error}\n\
             the build image should install fonts-urw-base35; rebuild it with .\\x.ps1 image"
        )
    })
}

/// Every point a command passes through, control points included.
fn points(command: &PathCommand) -> Vec<wp_font::Point> {
    match command {
        PathCommand::MoveTo(point) | PathCommand::LineTo(point) => vec![*point],
        PathCommand::QuadTo(control, point) => vec![*control, *point],
        PathCommand::CubicTo(first, second, point) => vec![*first, *second, *point],
        PathCommand::Close => vec![],
    }
}

#[test]
fn a_postscript_font_is_read_like_any_other() {
    let data = load();
    let font = Font::parse(&data).expect("a real font should parse");

    assert!(font.units_per_em() > 0);
    assert!(font.glyph_count() > 100, "a text font has far more than 100 glyphs");
    assert!(font.has_outlines(), "the outlines are there, in the other table");
    assert!(font.has_postscript_outlines(), "and they are the PostScript kind");

    let metrics = font.vertical_metrics();
    assert!(metrics.ascender > 0);
    assert!(metrics.descender < 0);
}

#[test]
fn a_letter_has_an_outline_made_of_cubic_curves() {
    let data = load();
    let font = Font::parse(&data).unwrap();

    let glyph = font.glyph_for('B').unwrap();
    let outline = font.outline(glyph).unwrap().expect("B has an outline");

    assert!(matches!(outline.commands.first(), Some(PathCommand::MoveTo(_))));
    assert!(matches!(outline.commands.last(), Some(PathCommand::Close)));
    assert!(
        outline.commands.iter().any(|command| matches!(command, PathCommand::CubicTo(..))),
        "the bowls of a B are curves, and a PostScript curve is cubic"
    );
    assert!(
        !outline.commands.iter().any(|command| matches!(command, PathCommand::QuadTo(..))),
        "nothing in this kind of font is quadratic"
    );
}

#[test]
fn the_bounds_hold_what_is_drawn() {
    // Unlike the other kind of font, a CFF glyph does not say how big it is:
    // the reader has to find out by following the pen. So the answer is worth
    // holding to every point of every letter.
    let data = load();
    let font = Font::parse(&data).unwrap();
    let units = f32::from(font.units_per_em());

    for character in "AQfgjoyw0123@&".chars() {
        let glyph = font.glyph_for(character).unwrap();
        let outline = font.outline(glyph).unwrap().expect("a letter has an outline");
        assert!(outline.bounds.max_x > outline.bounds.min_x, "{character} is flat");
        assert!(outline.bounds.max_y > outline.bounds.min_y, "{character} is flat");

        for command in &outline.commands {
            for point in points(command) {
                assert!(
                    point.x >= f32::from(outline.bounds.min_x) - 1.0
                        && point.x <= f32::from(outline.bounds.max_x) + 1.0,
                    "{character} draws at x={} outside its own bounds",
                    point.x
                );
                assert!(
                    point.y >= f32::from(outline.bounds.min_y) - 1.0
                        && point.y <= f32::from(outline.bounds.max_y) + 1.0,
                    "{character} draws at y={} outside its own bounds",
                    point.y
                );
            }
        }
        assert!(f32::from(outline.bounds.max_y) <= units * 1.5, "{character} is absurdly tall");
    }
}

#[test]
fn a_letter_is_as_wide_as_it_says_it_is() {
    // The width of a glyph is in `hmtx` like any other font's, and the outline
    // has to agree with it: a charstring that swallowed the width where there
    // was none, or left one on the stack where there was, draws a letter that
    // does not fit the space reserved for it.
    let data = load();
    let font = Font::parse(&data).unwrap();

    for character in "AHMnoxz".chars() {
        let glyph = font.glyph_for(character).unwrap();
        let advance = f32::from(font.advance(glyph));
        let outline = font.outline(glyph).unwrap().unwrap();
        assert!(
            f32::from(outline.bounds.max_x) <= advance * 1.3,
            "{character} is drawn {} wide but claims {advance}",
            outline.bounds.max_x
        );
        assert!(f32::from(outline.bounds.min_x) >= -advance * 0.3, "{character} starts left of 0");
    }
}

#[test]
fn every_glyph_in_the_font_can_be_drawn() {
    // A charstring is a program, and a reader that mishandles one operator
    // fails on whichever letters happen to use it. The only way to know is to
    // run all of them.
    let data = load();
    let font = Font::parse(&data).unwrap();

    let mut drawn = 0;
    for id in 0..font.glyph_count() {
        let outline = font
            .outline(wp_font::GlyphId(id))
            .unwrap_or_else(|error| panic!("glyph {id} would not draw: {error}"));
        if outline.is_some() {
            drawn += 1;
        }
    }
    assert!(drawn > font.glyph_count() as usize / 2, "most glyphs in a text font draw something");
}

#[test]
fn an_accented_letter_is_the_letter_and_the_accent() {
    // The old way of writing one: the charstring names a letter and an accent
    // rather than drawing either, and says how far to move the accent. A
    // reader that ignores it draws nothing at all for half the alphabet of
    // half of Europe.
    let data = load();
    let font = Font::parse(&data).unwrap();

    let plain = font.outline(font.glyph_for('e').unwrap()).unwrap().unwrap();
    let accented = font.outline(font.glyph_for('é').unwrap()).unwrap().unwrap();

    assert!(
        accented.commands.len() > plain.commands.len(),
        "the accented letter draws no more than the plain one"
    );
    assert!(
        accented.bounds.max_y > plain.bounds.max_y,
        "the accent is drawn over the letter and so reaches higher"
    );
}
