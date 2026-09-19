//! Turning characters into the glyphs that actually draw them.
//!
//! # Why this is not simply a lookup per character
//!
//! For Latin it very nearly is: one character, one glyph, and the only thing
//! lost by stopping there is a ligature or two. For Arabic it is not even
//! close. Every letter has four shapes and the text says which one only by
//! implication — through what stands beside it. A program that draws one glyph
//! per character produces a row of disconnected letters that a reader of Arabic
//! sees immediately as wrong, in the way a reader of English would see "t h e".
//!
//! So the work here is: decide what form each letter takes from its
//! neighbours, then ask the font for the glyph of that form, through the
//! substitution tables the font carries for exactly this purpose.
//!
//! # What is covered
//!
//! Arabic and Syriac joining, and ligatures wherever a font offers them; the
//! composing a font asks for before anything else; and the rules that say
//! *when* a rule applies — a glyph in this company becomes that — which most
//! of what a large font knows is written in. See [`gsub`].
//!
//! The Indic scripts need reordering within a syllable as well as
//! substitution, and are not shaped yet — they are drawn as they are stored,
//! which is wrong in a different way and is the next thing to do here.

#![forbid(unsafe_code)]

mod common;
pub mod gdef;
pub mod gpos;
pub mod gsub;
pub mod indic;
pub mod joining;
pub mod vertical;

use wp_font::{Font, GlyphId};

pub use gdef::{Definitions, Kind};
pub use gpos::{Placement, Positions};
pub use gsub::Substitutions;
pub use joining::{forms, is_joining_script, is_mark, joining_of, Form, Joining};

/// One glyph, which character of the original text it came from, and where it
/// goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shaped {
    pub glyph: GlyphId,
    /// The byte offset within the text this glyph belongs to.
    ///
    /// Several glyphs can share one, when a letter becomes a letter and a mark;
    /// several characters can share one glyph, when they become a ligature. A
    /// caret has to land between characters either way, so what is carried is
    /// where the glyph came from rather than a count of anything.
    pub cluster: usize,
    /// Where it is drawn from where the advances would have put it, in font
    /// units: right and up are positive.
    ///
    /// Nothing for nearly every glyph. An accent is the case it exists for: it
    /// is drawn without moving the pen, so left alone it lands at the edge of
    /// the letter before it instead of on it.
    pub x_offset: i32,
    pub y_offset: i32,
    /// What to add to the glyph's own width before the pen moves on, in font
    /// units. This is where kerning arrives.
    pub x_advance: i32,
}

/// Which script a run of text is in, as an OpenType tag.
///
/// Only what changes the shaping: a font's Latin features and its Arabic ones
/// live under different scripts, and asking under the wrong one finds nothing.
#[must_use]
pub fn script_of(text: &str) -> [u8; 4] {
    for character in text.chars() {
        match character as u32 {
            0x0600..=0x06FF | 0x0750..=0x077F | 0x08A0..=0x08FF | 0xFB50..=0xFEFF => {
                return *b"arab";
            }
            0x0700..=0x074F => return *b"syrc",
            0x0590..=0x05FF => return *b"hebr",
            0x0900..=0x097F => return *b"deva",
            0x0980..=0x09FF => return *b"beng",
            0x0A00..=0x0A7F => return *b"guru",
            0x0A80..=0x0AFF => return *b"gujr",
            0x0B00..=0x0B7F => return *b"orya",
            0x0B80..=0x0BFF => return *b"taml",
            0x0C00..=0x0C7F => return *b"telu",
            0x0C80..=0x0CFF => return *b"knda",
            0x0D00..=0x0D7F => return *b"mlym",
            0x0D80..=0x0DFF => return *b"sinh",
            0x0E00..=0x0E7F => return *b"thai",
            // The East Asian scripts, whose fonts keep the vertical forms
            // under the script's own name: the kana, the ideographs with the
            // marks written alongside them, and hangul.
            0x3040..=0x30FF | 0x31F0..=0x31FF | 0xFF66..=0xFF9F | 0x1B000..=0x1B16F => {
                return *b"kana";
            }
            0x2E80..=0x2FDF
            | 0x3000..=0x303F
            | 0x3100..=0x312F
            | 0x3190..=0x31EF
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xF900..=0xFAFF
            | 0xFF01..=0xFF60
            | 0x20000..=0x3FFFF => return *b"hani",
            0x1100..=0x11FF | 0x3130..=0x318F | 0xA960..=0xA97F | 0xAC00..=0xD7FF => {
                return *b"hang";
            }
            _ => {}
        }
    }
    *b"latn"
}

/// Whether a run has to be shaped whole rather than a character at a time.
///
/// Three things ask for it, and each of them is invisible one character at a
/// time: a script written joined, where the shape of a letter is decided by
/// its neighbours; a mark, whose place is decided by the letter it sits on;
/// and a script that is not drawn in the order it is written, where the
/// letters have to be rearranged before the font is asked anything.
#[must_use]
pub fn needs_shaping(text: &str) -> bool {
    text.chars()
        .any(|character| is_joining_script(character) || is_mark(character) || reorders(character))
}

/// Whether a character belongs to a script drawn in an order of its own.
#[must_use]
pub fn reorders(character: char) -> bool {
    indic::Script::of(character).is_some()
}

/// Turns text into glyphs, applying whatever the font offers for its script.
///
/// The characters are mapped to glyphs first and the font's rules applied
/// afterwards, which is the order the format is built around: a substitution
/// rule names glyphs, not characters.
#[must_use]
pub fn shape(font: &Font<'_>, text: &str) -> Vec<Shaped> {
    // What a script written joined needs, applied whether or not anybody asked:
    // in Arabic these are not an embellishment, they are the writing. And the
    // kerning, which a font of a joined script uses to set the letters at the
    // distances its designer meant.
    shape_with(font, text, &[*b"calt", *b"liga", *b"kern"])
}

/// The same, asking the font for exactly the features named.
///
/// This is what Word's Advanced tab reaches: ligatures, old-style figures,
/// tabular figures, a stylistic set. Nothing is applied that was not asked for,
/// with one exception — `rlig`, the required ligatures, which a font marks as
/// required because the writing is wrong without them.
#[must_use]
pub fn shape_with(font: &Font<'_>, text: &str, features: &[[u8; 4]]) -> Vec<Shaped> {
    let characters: Vec<char> = text.chars().collect();
    let mut glyphs = Vec::with_capacity(characters.len());
    let mut clusters = Vec::with_capacity(characters.len());

    let mut offset = 0usize;
    for character in &characters {
        // A character the font has no glyph for still takes a place, so that
        // the fallback machinery above can see which one is missing.
        glyphs.push(font.glyph_for(*character).unwrap_or(GlyphId(0)));
        clusters.push(offset);
        offset += character.len_utf8();
    }

    let Some(table) = font.substitution_table().and_then(Substitutions::parse) else {
        // No rules for which glyph; there may still be rules for where it
        // goes, and a mark in the wrong place is as wrong either way.
        let mut out = zip(glyphs, clusters);
        position(font, &script_of(text), &mut out, features.contains(b"kern"));
        return out;
    };
    let mut script = script_of(text);

    // The scripts of India are not drawn in the order they are written, and
    // no substitution table can say so: the text has to be rearranged before
    // the font is asked anything. See [`indic`].
    if let Some(which) = indic::Script::of_tag(&script) {
        script = which.tag_in(&table);
        let (glyphs, clusters) = indic::shape(font, &table, &script, text);
        let mut out = zip(glyphs, clusters);
        position(font, &script, &mut out, features.contains(b"kern"));
        return out;
    }

    // Composing and decomposing first, which is what the format says: a font
    // uses it to say that a letter and the mark under it are written as one
    // glyph, or that one character is drawn as two pieces. Every shaper
    // applies it whether or not anybody asked, because what it says is not a
    // refinement of the writing, it is the writing.
    for lookup in table.lookups_for(&script, b"ccmp") {
        table.apply(lookup, &mut glyphs, &mut clusters);
    }

    // The joining forms next: which shape each letter takes is decided by the
    // text, and the font is then asked for that shape.
    if characters.iter().any(|character| is_joining_script(*character)) {
        let wanted = forms(&characters);
        for (index, form) in wanted.iter().enumerate() {
            apply_to_one(&table, &script, form.feature(), index, &mut glyphs, &mut clusters);
        }
    }

    // Then the features that work on a whole run: required ligatures first,
    // because a font may spell a required form as one, and because a font that
    // marks a ligature required means the writing is wrong without it.
    for feature in core::iter::once(*b"rlig").chain(features.iter().copied()) {
        for lookup in table.lookups_for(&script, &feature) {
            table.apply(lookup, &mut glyphs, &mut clusters);
        }
    }

    let mut out = zip(glyphs, clusters);
    // And then where each of them goes, which is the other table.
    position(font, &script, &mut out, features.contains(b"kern"));
    out
}

/// Works out where each glyph goes, once it is known which glyphs they are.
///
/// Two things come out of this and neither is decoration: the kerning, which
/// every font written this century keeps in the positioning table rather than
/// in the old one; and where a mark belongs, which is the difference between
/// an accent on a letter and an accent beside it.
fn position(font: &Font<'_>, script: &[u8; 4], shaped: &mut [Shaped], kern: bool) {
    let glyphs: Vec<GlyphId> = shaped.iter().map(|entry| entry.glyph).collect();
    let advances: Vec<i32> = glyphs.iter().map(|glyph| i32::from(font.advance(*glyph))).collect();

    let table = font.positioning_table().and_then(gpos::Positions::parse);
    let mut kerned = false;

    if let Some(table) = table {
        let definitions = font.definitions_table().and_then(gdef::Definitions::parse);
        let mut placements = vec![gpos::Placement::default(); glyphs.len()];
        let mut run =
            gpos::Run { glyphs: &glyphs, advances: &advances, placements: &mut placements };

        // The kerning first, because where a mark goes is worked out from how
        // far the pen has travelled and the kerning is part of that travel.
        // Then the marks onto their letters, then the marks onto each other,
        // which is the order the format lists them in and the order they
        // depend on each other in.
        // The marks above and below, and the spacing between the pieces of a
        // syllable, are the same question as `mark` and `mkmk` asked by the
        // scripts that reorder. A font that does not use them says nothing
        // under them and nothing happens.
        for feature in [b"kern", b"mark", b"mkmk", b"abvm", b"blwm", b"dist"] {
            if feature == b"kern" && !kern {
                continue;
            }
            let lookups = table.lookups_for(script, feature);
            if feature == b"kern" && !lookups.is_empty() {
                kerned = true;
            }
            for lookup in lookups {
                table.apply(lookup, &mut run, definitions.as_ref());
            }
        }

        for (entry, placement) in shaped.iter_mut().zip(placements) {
            entry.x_offset = placement.x_offset;
            entry.y_offset = placement.y_offset;
            entry.x_advance = placement.x_advance;
        }
    }

    // A font that says nothing about kerning in the new table may still say it
    // in the old one, which is where every font said it before 1997 and where
    // a good many still do.
    if kern && !kerned {
        for index in 1..shaped.len() {
            let by = font.kerning(glyphs[index - 1], glyphs[index]);
            shaped[index - 1].x_advance += i32::from(by);
        }
    }
}

/// What the font would put between two glyphs, for a caller that shapes one
/// character at a time.
///
/// The positioning table first and the old one only if it says nothing: a font
/// that carries both means the same thing twice, and counting both would kern
/// twice as hard as the designer asked.
#[must_use]
pub fn kerning_between(font: &Font<'_>, script: &[u8; 4], left: GlyphId, right: GlyphId) -> i32 {
    if let Some(table) = font.positioning_table().and_then(gpos::Positions::parse) {
        let lookups = table.lookups_for(script, b"kern");
        if !lookups.is_empty() {
            let glyphs = [left, right];
            let advances = [i32::from(font.advance(left)), i32::from(font.advance(right))];
            let mut placements = [gpos::Placement::default(); 2];
            let definitions = font.definitions_table().and_then(gdef::Definitions::parse);
            let mut run =
                gpos::Run { glyphs: &glyphs, advances: &advances, placements: &mut placements };
            for lookup in lookups {
                table.apply(lookup, &mut run, definitions.as_ref());
            }
            return placements[0].x_advance;
        }
    }
    i32::from(font.kerning(left, right))
}

/// The form a glyph takes when it stands in a line that runs down the page,
/// if the font has one: the `vert` feature, which is where a full stop moves
/// to the top right of its square and a bracket turns to open downwards.
///
/// One glyph at a time, because which glyphs stand upright in such a line is
/// decided character by character — see [`vertical`] — and a Latin letter in
/// the same run is not asked. `None` when the font offers nothing for it,
/// which for a letter is the ordinary answer.
#[must_use]
pub fn vertical_form(font: &Font<'_>, script: &[u8; 4], glyph: GlyphId) -> Option<GlyphId> {
    let table = font.substitution_table().and_then(Substitutions::parse)?;
    let lookups = table.lookups_for(script, b"vert");
    if lookups.is_empty() {
        return None;
    }
    let mut one = vec![glyph];
    let mut cluster = vec![0];
    for lookup in lookups {
        table.apply(lookup, &mut one, &mut cluster);
    }
    match one.as_slice() {
        [form] if *form != glyph => Some(*form),
        _ => None,
    }
}

/// Applies a feature to one glyph, leaving the rest of the run alone.
///
/// A joining form is decided per letter, so the lookup has to be run against
/// that letter only — running it over the whole run would give every covered
/// glyph the same form.
fn apply_to_one(
    table: &Substitutions<'_>,
    script: &[u8; 4],
    feature: &[u8; 4],
    index: usize,
    glyphs: &mut [GlyphId],
    clusters: &mut [usize],
) {
    let lookups = table.lookups_for(script, feature);
    if lookups.is_empty() || index >= glyphs.len() {
        return;
    }

    let mut one = vec![glyphs[index]];
    let mut cluster = vec![clusters[index]];
    for lookup in lookups {
        table.apply(lookup, &mut one, &mut cluster);
    }
    // Only a substitution that kept the run one glyph long can be written back
    // in place; anything else would shift everything after it.
    if one.len() == 1 {
        glyphs[index] = one[0];
    }
}

fn zip(glyphs: Vec<GlyphId>, clusters: Vec<usize>) -> Vec<Shaped> {
    glyphs
        .into_iter()
        .zip(clusters)
        .map(|(glyph, cluster)| Shaped { glyph, cluster, x_offset: 0, y_offset: 0, x_advance: 0 })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arabic_text_is_recognised_as_arabic() {
        assert_eq!(script_of("\u{0628}\u{0633}\u{0645}"), *b"arab");
    }

    #[test]
    fn plain_text_is_latin() {
        assert_eq!(script_of("hello"), *b"latn");
        assert_eq!(script_of(""), *b"latn");
    }

    #[test]
    fn the_script_is_taken_from_the_first_character_that_names_one() {
        // A line beginning in English and continuing in Arabic is shaped as
        // Arabic: the Latin part needs nothing the Arabic rules would break.
        assert_eq!(script_of("page \u{0628}"), *b"arab");
    }

    #[test]
    fn the_east_asian_scripts_are_named() {
        assert_eq!(script_of("漢字"), *b"hani");
        assert_eq!(script_of("かな"), *b"kana");
        assert_eq!(script_of("한글"), *b"hang");
        assert_eq!(script_of("。"), *b"hani");
    }

    #[test]
    fn hebrew_and_thai_are_told_apart_from_arabic() {
        assert_eq!(script_of("\u{05D0}"), *b"hebr");
        assert_eq!(script_of("\u{0E01}"), *b"thai");
        assert_eq!(script_of("\u{0915}"), *b"deva");
    }
}
