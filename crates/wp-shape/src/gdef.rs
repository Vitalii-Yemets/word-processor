//! What kind of thing a glyph is.
//!
//! `GDEF` answers one question the rest of shaping keeps asking: is this glyph
//! a letter, or a mark that sits on one? Nothing in the glyph itself says so —
//! a mark is an outline like any other — and every rule about marks needs the
//! answer. Where a combining accent belongs is decided by finding the letter
//! before it, and finding it means passing over whatever marks are in between.
//!
//! A font without the table is read as saying nothing, and then the only
//! evidence left is the width: a mark is drawn without moving the pen, so a
//! glyph of no width and some ink in it is a mark. That is a guess, and it is
//! marked as one where it is made.

use wp_font::GlyphId;

use crate::common::{class_in, u16_at};

/// What a glyph is, as the font says.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    /// The font says nothing about it.
    #[default]
    Unsaid,
    /// A letter, a digit, anything that stands on the line by itself.
    Base,
    /// Several of those written as one.
    Ligature,
    /// Something that sits on one of the above.
    Mark,
    /// A piece of a ligature, which never appears on its own.
    Component,
}

/// The definitions a font carries about its own glyphs.
#[derive(Clone, Copy, Debug)]
pub struct Definitions<'a> {
    data: &'a [u8],
    classes: Option<usize>,
    mark_classes: Option<usize>,
}

impl<'a> Definitions<'a> {
    /// Reads the header of a `GDEF` table.
    #[must_use]
    pub fn parse(data: &'a [u8]) -> Option<Self> {
        if u16_at(data, 0)? != 1 {
            return None;
        }
        let at = |offset: usize| -> Option<usize> {
            let value = u16_at(data, offset)?;
            (value != 0).then(|| usize::from(value))
        };
        Some(Self { data, classes: at(4), mark_classes: at(10) })
    }

    /// What kind of thing a glyph is.
    #[must_use]
    pub fn kind(&self, glyph: GlyphId) -> Kind {
        let Some(at) = self.classes else { return Kind::Unsaid };
        match class_in(self.data, at, glyph) {
            1 => Kind::Base,
            2 => Kind::Ligature,
            3 => Kind::Mark,
            4 => Kind::Component,
            _ => Kind::Unsaid,
        }
    }

    /// Which group of marks a glyph belongs to.
    ///
    /// A lookup may say it passes over every mark but one group — the flag
    /// carries the group's number — which is how a font keeps the accents
    /// above a letter from being confused with the marks below it.
    #[must_use]
    pub fn mark_class(&self, glyph: GlyphId) -> u16 {
        let Some(at) = self.mark_classes else { return 0 };
        class_in(self.data, at, glyph)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A table saying that glyphs 10 to 19 are letters and 20 to 29 are marks.
    fn table() -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&1u16.to_be_bytes()); // major version
        data.extend_from_slice(&0u16.to_be_bytes()); // minor version
        data.extend_from_slice(&12u16.to_be_bytes()); // the class definitions
        data.extend_from_slice(&0u16.to_be_bytes()); // no attachment points
        data.extend_from_slice(&0u16.to_be_bytes()); // no ligature carets
        data.extend_from_slice(&0u16.to_be_bytes()); // no mark groups

        // The class definitions themselves, in ranges.
        data.extend_from_slice(&2u16.to_be_bytes());
        data.extend_from_slice(&2u16.to_be_bytes());
        for (first, last, class) in [(10u16, 19u16, 1u16), (20, 29, 3)] {
            data.extend_from_slice(&first.to_be_bytes());
            data.extend_from_slice(&last.to_be_bytes());
            data.extend_from_slice(&class.to_be_bytes());
        }
        data
    }

    #[test]
    fn a_letter_and_a_mark_are_told_apart() {
        let data = table();
        let definitions = Definitions::parse(&data).expect("the table should parse");
        assert_eq!(definitions.kind(GlyphId(12)), Kind::Base);
        assert_eq!(definitions.kind(GlyphId(22)), Kind::Mark);
    }

    #[test]
    fn a_glyph_the_font_says_nothing_about_is_left_unsaid() {
        let data = table();
        let definitions = Definitions::parse(&data).expect("the table should parse");
        assert_eq!(definitions.kind(GlyphId(5)), Kind::Unsaid);
        assert_eq!(definitions.mark_class(GlyphId(22)), 0, "there are no groups in this font");
    }

    #[test]
    fn a_table_of_a_version_this_does_not_know_is_not_read() {
        let mut data = table();
        data[0..2].copy_from_slice(&2u16.to_be_bytes());
        assert!(Definitions::parse(&data).is_none());
    }
}
