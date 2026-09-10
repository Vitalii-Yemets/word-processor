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
//! Arabic and Syriac joining, and ligatures wherever a font offers them. The
//! Indic scripts need reordering within a syllable as well as substitution, and
//! are not shaped yet — they are drawn as they are stored, which is wrong in a
//! different way and is the next thing to do here.

#![forbid(unsafe_code)]

pub mod gsub;
pub mod joining;

use wp_font::{Font, GlyphId};

pub use gsub::Substitutions;
pub use joining::{forms, is_joining_script, joining_of, Form, Joining};

/// One glyph, and which character of the original text it came from.
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
            0x0E00..=0x0E7F => return *b"thai",
            _ => {}
        }
    }
    *b"latn"
}

/// Turns text into glyphs, applying whatever the font offers for its script.
///
/// The characters are mapped to glyphs first and the font's rules applied
/// afterwards, which is the order the format is built around: a substitution
/// rule names glyphs, not characters.
#[must_use]
pub fn shape(font: &Font<'_>, text: &str) -> Vec<Shaped> {
    // What a script written joined needs, applied whether or not anybody asked:
    // in Arabic these are not an embellishment, they are the writing.
    shape_with(font, text, &[*b"calt", *b"liga"])
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
        return zip(glyphs, clusters);
    };
    let script = script_of(text);

    // The joining forms first: which shape each letter takes is decided by the
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

    zip(glyphs, clusters)
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
    glyphs.into_iter().zip(clusters).map(|(glyph, cluster)| Shaped { glyph, cluster }).collect()
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
    fn hebrew_and_thai_are_told_apart_from_arabic() {
        assert_eq!(script_of("\u{05D0}"), *b"hebr");
        assert_eq!(script_of("\u{0E01}"), *b"thai");
        assert_eq!(script_of("\u{0915}"), *b"deva");
    }
}
