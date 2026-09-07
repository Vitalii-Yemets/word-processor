//! Tests against a real font file rather than a hand-built one.
//!
//! Synthetic tables prove the reader handles the shapes it was written for. Only
//! a font somebody else produced proves it handles the shapes that actually
//! occur — repeat flags, composite glyphs, a `cmap` covering thousands of
//! characters. DejaVu Sans is installed in the build container for this.

use wp_font::{Error, Font, PathCommand};

/// Where the container keeps the font. If it is missing, the tests say so rather
/// than passing quietly, since a silently skipped test proves nothing.
const FONT_PATH: &str = "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf";

fn load() -> Vec<u8> {
    std::fs::read(FONT_PATH).unwrap_or_else(|error| {
        panic!(
            "cannot read {FONT_PATH}: {error}\n\
             the build image should install fonts-dejavu-core; rebuild it with .\\x.ps1 image"
        )
    })
}

#[test]
fn reads_the_header_of_a_real_font() {
    let data = load();
    let font = Font::parse(&data).expect("a real font should parse");

    // 2048 is the usual grid for a TrueType font, and it must never be zero.
    assert!(font.units_per_em() > 0);
    assert!(font.glyph_count() > 100, "a text font has far more than 100 glyphs");
    assert!(font.has_outlines(), "DejaVu Sans stores quadratic outlines");

    let metrics = font.vertical_metrics();
    assert!(metrics.ascender > 0, "the ascender rises above the baseline");
    assert!(metrics.descender < 0, "the descender falls below it");
    assert!(metrics.line_height() > i32::from(font.units_per_em()) / 2);
}

#[test]
fn reads_the_name_of_a_real_font() {
    let data = load();
    let font = Font::parse(&data).unwrap();

    let family = font.family_name().expect("a font declares its family");
    assert!(family.contains("DejaVu"), "unexpected family name: {family}");
    assert!(!font.is_bold(), "the regular weight should not claim to be bold");
    assert!(!font.is_italic());
}

#[test]
fn maps_characters_from_several_scripts() {
    let data = load();
    let font = Font::parse(&data).unwrap();

    // DejaVu Sans covers Latin, Cyrillic and Greek, which is what makes it a
    // useful test: a mapping that only worked for ASCII would pass with one of
    // these and fail with the others.
    for character in ['A', 'z', '0', ' ', 'Ä', 'Ж', 'Ω', '—', '€'] {
        assert!(
            font.glyph_for(character).is_some(),
            "no glyph for {character:?}, which the font should cover"
        );
    }

    // Different characters must not all collapse onto one glyph.
    assert_ne!(font.glyph_for('A'), font.glyph_for('B'));
    assert_ne!(font.glyph_for('A'), font.glyph_for('Ж'));
}

#[test]
fn advances_are_sensible() {
    let data = load();
    let font = Font::parse(&data).unwrap();
    let units = f32::from(font.units_per_em());

    let space = font.glyph_for(' ').unwrap();
    let wide = font.glyph_for('W').unwrap();
    let narrow = font.glyph_for('i').unwrap();

    assert!(font.advance(space) > 0, "a space still moves the pen");
    assert!(
        font.advance(wide) > font.advance(narrow),
        "W should be wider than i in a proportional font"
    );
    // Nothing in a text font is wider than the design grid itself.
    assert!(f32::from(font.advance(wide)) < units * 1.5);
}

#[test]
fn reads_a_glyph_outline() {
    let data = load();
    let font = Font::parse(&data).unwrap();

    let glyph = font.glyph_for('A').unwrap();
    let outline = font.outline(glyph).unwrap().expect("A has an outline");

    assert!(matches!(outline.commands.first(), Some(PathCommand::MoveTo(_))));
    assert!(matches!(outline.commands.last(), Some(PathCommand::Close)));
    assert!(outline.commands.len() > 4, "a letter is more than a few segments");

    assert!(outline.bounds.max_x > outline.bounds.min_x);
    assert!(outline.bounds.max_y > outline.bounds.min_y);

    // Every point must sit inside the bounds the glyph declares, or the
    // rasterizer will draw outside the area it reserved.
    for command in &outline.commands {
        let points = match command {
            PathCommand::MoveTo(point) | PathCommand::LineTo(point) => vec![*point],
            PathCommand::QuadTo(control, point) => vec![*control, *point],
            PathCommand::Close => vec![],
        };
        for point in points {
            // Control points may sit slightly outside; on-curve points may not,
            // so the tolerance is generous but finite.
            let slack = f32::from(font.units_per_em()) * 0.5;
            assert!(
                point.x >= f32::from(outline.bounds.min_x) - slack
                    && point.x <= f32::from(outline.bounds.max_x) + slack,
                "a point at x={} is far outside the declared bounds",
                point.x
            );
        }
    }
}

#[test]
fn a_space_has_no_outline() {
    let data = load();
    let font = Font::parse(&data).unwrap();

    let space = font.glyph_for(' ').unwrap();
    assert_eq!(font.outline(space).unwrap(), None, "a space is blank by definition");
}

#[test]
fn composite_glyphs_are_assembled() {
    // An accented letter is stored as references to other glyphs with offsets.
    // A reader that stops at simple glyphs draws it as nothing at all.
    let data = load();
    let font = Font::parse(&data).unwrap();

    for character in ['Ä', 'é', 'ñ', 'Ż'] {
        let Some(glyph) = font.glyph_for(character) else { continue };
        let outline = font
            .outline(glyph)
            .unwrap_or_else(|error| panic!("{character:?}: {error}"))
            .unwrap_or_else(|| panic!("{character:?} should have an outline"));

        assert!(outline.commands.len() > 4, "{character:?} came out nearly empty");
        // An accent sits above the letter, so the shape is taller than the plain
        // letter it is built from.
        let plain = font.glyph_for('n').and_then(|id| font.outline(id).ok().flatten());
        if let Some(plain) = plain {
            assert!(
                outline.bounds.max_y >= plain.bounds.max_y,
                "{character:?} should be at least as tall as an unaccented letter"
            );
        }
    }
}

#[test]
fn every_glyph_can_be_read_without_panicking() {
    // The real check on the outline reader: run it over the whole font rather
    // than the handful of characters a test would think to name.
    let data = load();
    let font = Font::parse(&data).unwrap();

    let mut with_outlines = 0usize;
    for index in 0..font.glyph_count() {
        match font.outline(wp_font::GlyphId(index)) {
            Ok(Some(outline)) => {
                assert!(!outline.is_empty());
                with_outlines += 1;
            }
            Ok(None) => {}
            Err(error) => panic!("glyph {index} failed: {error}"),
        }
    }

    assert!(with_outlines > 100, "only {with_outlines} glyphs had outlines");
}

#[test]
fn a_damaged_font_is_refused_without_panicking() {
    let original = load();

    // Truncation at many points, and bit flips through the header and table
    // directory, which is where a corrupt offset does the most damage.
    for cut in (0..original.len()).step_by(4093) {
        let _ = Font::parse(&original[..cut]);
    }
    for index in (0..2048.min(original.len())).step_by(7) {
        let mut damaged = original.clone();
        damaged[index] ^= 0xFF;
        if let Ok(font) = Font::parse(&damaged) {
            for glyph in 0..font.glyph_count().min(500) {
                let _ = font.outline(wp_font::GlyphId(glyph));
            }
        }
    }
}

#[test]
fn a_file_that_is_not_a_font_is_refused() {
    assert_eq!(Font::parse(b"").unwrap_err(), Error::OutOfBounds);
    assert_eq!(Font::parse(b"not a font at all").unwrap_err(), Error::NotAFont);
    assert_eq!(Font::parse(&[0u8; 1000]).unwrap_err(), Error::NotAFont);
}
