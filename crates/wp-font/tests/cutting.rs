//! PostScript fonts cut down to the glyphs a document uses, held against the
//! whole fonts they were cut from.
//!
//! What matters about a cut font is one thing: every glyph that was kept is
//! drawn exactly as the whole font draws it. A subroutine renumbered wrongly
//! does not fail loudly — it draws a different stroke — so every glyph kept
//! is drawn from both and the two compared command for command, across every
//! PostScript font in the build image and across the glyphs of a font for
//! Japanese, whose glyphs are split between dictionaries each with its own
//! subroutines.

use std::collections::BTreeSet;

use wp_font::{Font, GlyphId, Subroutines};

const URW: &str = "/usr/share/fonts/opentype/urw-base35";
const LATIN: &str = "/usr/share/fonts/opentype/urw-base35/NimbusRoman-Regular.otf";
const JAPANESE: &str = "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc";

fn read(path: &str) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|error| {
        panic!(
            "cannot read {path}: {error}\n\
             the build image should install fonts-urw-base35 and fonts-noto-cjk"
        )
    })
}

/// The glyphs a piece of text is drawn with.
fn glyphs_of(font: &Font<'_>, text: &str) -> BTreeSet<u16> {
    text.chars().filter_map(|character| font.glyph_for(character)).map(|glyph| glyph.0).collect()
}

/// The glyph of the cut font a page's number finds: the same number in a
/// font whose glyphs are named, and whatever the cut font's table of CIDs
/// says in one whose glyphs are named by CID — which is how a PDF reader
/// looks it up.
fn found(cut: &Font<'_>, glyph: u16) -> Option<GlyphId> {
    if cut.glyph_for_cid(0).is_some() {
        cut.glyph_for_cid(glyph)
    } else {
        Some(GlyphId(glyph))
    }
}

/// Whether every glyph kept draws in the cut font what it draws in the whole
/// one, found the way a page finds it; the first that does not, if one does
/// not.
fn first_difference(whole: &Font<'_>, cut: &Font<'_>, kept: &BTreeSet<u16>) -> Option<u16> {
    kept.iter().copied().find(|glyph| {
        let before = whole.outline(GlyphId(*glyph)).ok().flatten();
        let after = found(cut, *glyph).and_then(|at| cut.outline(at).ok().flatten());
        before != after || found(cut, *glyph).is_none()
    })
}

#[test]
fn a_latin_font_is_cut_to_the_letters_asked_for() {
    let data = read(LATIN);
    let font = Font::parse(&data).expect("the font");
    let asked = glyphs_of(&font, "Hello, world — naïve café");

    let cut = font.cut_postscript(&asked).expect("a font that can be cut");
    assert_eq!(cut.subroutines, Subroutines::Renumbered);
    assert!(asked.is_subset(&cut.kept));
    // Every glyph up to the last kept, each where it was.
    assert_eq!(cut.glyphs, (0..=*cut.kept.last().unwrap()).collect::<Vec<_>>());
    assert!(
        cut.font.len() * 4 < data.len(),
        "cut to {} bytes from {}: not much of a cut",
        cut.font.len(),
        data.len()
    );

    assert!(!cut.cid_keyed);
    let small = Font::parse(&cut.font).expect("the cut font opens");
    assert!(small.has_postscript_outlines());
    assert_eq!(small.table(b"CFF "), Some(&cut.table[..]), "the bare table is the one inside");
    assert_eq!(usize::from(small.glyph_count()), cut.glyphs.len());
    assert!(small.glyph_for_cid(0).is_none(), "a font of named glyphs stays one");
    assert_eq!(first_difference(&font, &small, &cut.kept), None);

    // A glyph nobody asked for is still there, and draws nothing.
    let unasked = font.glyph_for('Z').expect("a Z").0;
    if usize::from(unasked) < cut.glyphs.len() {
        assert!(small.outline(GlyphId(unasked)).unwrap().is_none());
    }
    // And the widths are the whole font's.
    for glyph in &cut.kept {
        assert_eq!(small.advance(GlyphId(*glyph)), font.advance(GlyphId(*glyph)));
    }
}

#[test]
fn every_postscript_font_in_the_image_keeps_what_it_draws_whatever_is_cut() {
    // Every font of the URW set, cut four ways: a stride through all its
    // glyphs, and three pieces of text. Each set of glyphs reaches a
    // different set of subroutines, and so renumbers them differently.
    let mut fonts = std::fs::read_dir(URW)
        .expect("the URW set")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "otf"))
        .collect::<Vec<_>>();
    fonts.sort();
    assert!(fonts.len() >= 30, "{fonts:?}");

    for path in fonts {
        let data = std::fs::read(&path).unwrap();
        let font = Font::parse(&data).unwrap();
        let count = font.glyph_count();
        let mut sets: Vec<BTreeSet<u16>> = [3u16, 7, 29]
            .iter()
            .map(|stride| (0..count).step_by(usize::from(*stride)).collect())
            .collect();
        sets.push(glyphs_of(&font, "The quick brown fox jumps over the lazy dog 0123456789"));
        sets.push(glyphs_of(&font, "Ærøskøbing Łódź Ångström Überraschung façade"));

        for asked in sets {
            let cut = font
                .cut_postscript(&asked)
                .unwrap_or_else(|| panic!("{} could not be cut", path.display()));
            let small = Font::parse(&cut.font)
                .unwrap_or_else(|error| panic!("{}: the cut font: {error}", path.display()));
            assert_eq!(
                first_difference(&font, &small, &cut.kept),
                None,
                "{} draws a kept glyph differently once cut ({:?})",
                path.display(),
                cut.subroutines
            );
        }
    }
}

#[test]
fn a_font_for_japanese_is_cut_dictionary_by_dictionary() {
    let data = read(JAPANESE);
    let font = Font::parse(&data).expect("the first face of the collection");
    let asked = glyphs_of(&font, "日本語の文章を縦に書く。「括弧」とカタカナ、ひらがな。ABC 123");
    assert!(asked.len() > 20);

    let cut = font.cut_postscript(&asked).expect("a font that can be cut");
    assert_eq!(cut.subroutines, Subroutines::Renumbered);
    // Only the glyphs kept, one after another, and the CID of each the place
    // it had in the whole font.
    assert_eq!(cut.glyphs, cut.kept.iter().copied().collect::<Vec<_>>());
    // Nineteen megabytes of collection behind a line of text.
    assert!(cut.font.len() < 60_000, "cut to {} bytes", cut.font.len());

    assert!(cut.cid_keyed);
    let small = Font::parse(&cut.font).expect("the cut font opens");
    assert_eq!(small.table(b"CFF "), Some(&cut.table[..]), "the bare table is the one inside");
    assert_eq!(usize::from(small.glyph_count()), cut.glyphs.len());
    assert_eq!(first_difference(&font, &small, &cut.kept), None);
    for glyph in &cut.kept {
        let at = found(&small, *glyph).expect("found by its CID");
        assert_eq!(small.advance(at), font.advance(GlyphId(*glyph)));
    }
    // A glyph nobody asked for has no CID in the cut font at all.
    let unasked = font.glyph_for('猫').expect("a cat").0;
    assert!(!cut.kept.contains(&unasked));
    assert_eq!(small.glyph_for_cid(unasked), None);
}

#[test]
fn glyphs_from_every_dictionary_of_a_font_for_japanese_are_kept_as_drawn() {
    // A stride through all sixty-five thousand glyphs, which lands in every
    // one of the font's dictionaries and calls on each one's subroutines.
    let data = read(JAPANESE);
    let font = Font::parse(&data).unwrap();
    let asked: BTreeSet<u16> = (0..font.glyph_count()).step_by(331).collect();
    let cut = font.cut_postscript(&asked).expect("a font that can be cut");
    let small = Font::parse(&cut.font).expect("the cut font opens");
    assert_eq!(first_difference(&font, &small, &cut.kept), None);
}

#[test]
fn a_font_of_another_kind_is_not_cut_this_way() {
    // The TrueType fonts are cut by the PDF writer, point for point; this is
    // for the programs.
    let data = read("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf");
    let font = Font::parse(&data).unwrap();
    assert!(font.cut_postscript(&[36u16].into_iter().collect()).is_none());
}
