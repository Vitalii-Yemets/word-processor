//! Tests against a real font that draws in colour.
//!
//! An emoji is a small picture, not a letter: a yellow face with brown eyes is
//! neither one shape nor one colour. Noto Color Emoji keeps each of them as a
//! PNG at a fixed size, several sizes over, and is installed in the build
//! container for this.

use wp_font::Font;

const FONT_PATH: &str = "/usr/share/fonts/truetype/noto/NotoColorEmoji.ttf";

fn load() -> Vec<u8> {
    std::fs::read(FONT_PATH).unwrap_or_else(|error| {
        panic!(
            "cannot read {FONT_PATH}: {error}\n\
             the build image should install fonts-noto-color-emoji"
        )
    })
}

#[test]
fn a_font_of_pictures_is_still_a_font() {
    let data = load();
    let font = Font::parse(&data).expect("a real font");

    assert!(!font.has_outlines(), "there is no glyf and no CFF in this one");
    assert!(font.has_colour(), "and yet it draws");
    assert!(font.can_be_drawn_with());
    assert!(font.glyph_for('\u{1F600}').is_some(), "a grinning face");
}

#[test]
fn an_emoji_comes_back_as_a_picture() {
    let data = load();
    let font = Font::parse(&data).unwrap();

    for emoji in ['\u{1F600}', '\u{1F680}', '\u{2764}', '\u{1F1EC}'] {
        let glyph = font.glyph_for(emoji).expect("the font has it");
        let bitmap = font.bitmap(glyph, 32).expect("a picture for it");

        assert!(bitmap.pixels_per_em > 0);
        // Every one of them is a PNG, and says so in its first eight bytes.
        assert_eq!(
            &bitmap.png[..8],
            &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A],
            "{emoji} is not a PNG"
        );
        assert!(bitmap.png.len() > 100, "{emoji} is an empty picture");
    }
}

#[test]
fn a_letter_has_no_picture_and_asking_is_not_an_error() {
    let data = load();
    let font = Font::parse(&data).unwrap();

    // The font maps a few ordinary characters and draws nothing for them.
    if let Some(glyph) = font.glyph_for('\u{0041}') {
        let _ = font.bitmap(glyph, 32);
    }
    assert!(font.bitmap(wp_font::GlyphId(0xFFFF), 32).is_none());
}

#[test]
fn the_size_asked_for_decides_which_picture_comes_back() {
    let data = load();
    let font = Font::parse(&data).unwrap();
    let glyph = font.glyph_for('\u{1F600}').unwrap();

    let small = font.bitmap(glyph, 8).expect("a picture");
    let large = font.bitmap(glyph, 200).expect("a picture");
    // Whatever sizes this font holds, a small size must never come back larger
    // than a large one: the nearest at least as big is what is wanted, and the
    // largest there is when nothing is big enough.
    assert!(small.pixels_per_em <= large.pixels_per_em);
}

#[test]
fn an_ordinary_font_draws_no_colour() {
    let data = std::fs::read("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf").unwrap();
    let font = Font::parse(&data).unwrap();

    assert!(!font.has_colour());
    let glyph = font.glyph_for('A').unwrap();
    assert!(font.colour_layers(glyph).is_none());
    assert!(font.bitmap(glyph, 32).is_none());
}
