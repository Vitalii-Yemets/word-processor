//! Which way up each character stands when the line runs down the page.
//!
//! # Why a line set vertically is not a line turned on its side
//!
//! Because the letters of the scripts written that way — Chinese, Japanese,
//! Korean — stand upright in it, one under the other, while everything else
//! in the same line lies on its side reading downwards: a Latin word, a
//! number, a run of punctuation from the Latin repertoire. Which is which is a
//! property of the character, and Unicode says it in the Vertical Orientation
//! property (UAX #50). What is here is that property, reduced to the ranges
//! that decide it, and the two refinements it makes: a character may stand
//! upright *if the font has a vertical form of it*, and lie down otherwise.
//!
//! A word processor draws the whole line into a box as long as the column
//! and turns the box; what this module answers is which glyphs then have to
//! be turned back.

/// Which way a character stands in a line that runs down the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orientation {
    /// Stands upright, as every ideograph and every kana does.
    Upright,
    /// Lies on its side, reading downwards: Latin, Cyrillic, Greek, digits
    /// and their punctuation.
    Rotated,
    /// Stands upright, and the font is asked for its vertical form first: a
    /// full stop that moves to the top right corner of its square, a bracket
    /// that turns to open downwards.
    TransformedUpright,
    /// Lies on its side unless the font has a vertical form of it, in which
    /// case that form stands upright: the long vowel mark of katakana, the
    /// dashes and the ellipsis.
    TransformedRotated,
}

impl Orientation {
    /// The orientation of a character, from the ranges Unicode assigns.
    #[must_use]
    pub fn of(character: char) -> Self {
        use Orientation::{Rotated, TransformedRotated, TransformedUpright, Upright};
        match character as u32 {
            // The marks of the East Asian scripts that change their corner or
            // their opening: the ideographic full stop and comma, the corner
            // and lenticular brackets, the wave dash, and the fullwidth forms
            // of the ASCII punctuation.
            0x3001..=0x3002
            | 0x3008..=0x3011
            | 0x3014..=0x301F
            | 0x3030
            | 0x30A0
            | 0x3041
            | 0x3043
            | 0x3045
            | 0x3047
            | 0x3049
            | 0x3063
            | 0x3083
            | 0x3085
            | 0x3087
            | 0x308E
            | 0x3095..=0x3096
            | 0x30A1
            | 0x30A3
            | 0x30A5
            | 0x30A7
            | 0x30A9
            | 0x30C3
            | 0x30E3
            | 0x30E5
            | 0x30E7
            | 0x30EE
            | 0x30F5..=0x30F6
            | 0x31F0..=0x31FF
            | 0xFF08..=0xFF09
            | 0xFF0C
            | 0xFF0E
            | 0xFF1A..=0xFF1B
            | 0xFF3B
            | 0xFF3D
            | 0xFF5B..=0xFF5D
            | 0xFF5F
            | 0xFF60
            | 0xFF61..=0xFF64 => TransformedUpright,
            // Upright only where the font has a vertical form: the dashes,
            // the ellipses, the katakana-hiragana prolonged sound mark and
            // the fullwidth tilde and equals.
            0x2014
            | 0x2015
            | 0x2025
            | 0x2026
            | 0x3033..=0x3035
            | 0x30FC
            | 0xFE31..=0xFE32
            | 0xFF0D
            | 0xFF1C..=0xFF1E
            | 0xFF3F
            | 0xFF5E => TransformedRotated,
            // The ideographs and the syllabaries.
            0x1100..=0x11FF
            | 0x2E80..=0x2FDF
            | 0x2FF0..=0x2FFF
            | 0x3000
            | 0x3003..=0x3007
            | 0x3012..=0x3013
            | 0x3020..=0x302F
            | 0x3031..=0x3032
            | 0x3036..=0x303F
            | 0x3040..=0x309F
            | 0x30A0..=0x30FF
            | 0x3100..=0x312F
            | 0x3130..=0x318F
            | 0x3190..=0x31EF
            | 0x3200..=0x33FF
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xA000..=0xA4CF
            | 0xA960..=0xA97F
            | 0xAC00..=0xD7FF
            | 0xF900..=0xFAFF
            | 0xFE10..=0xFE1F
            | 0xFE30..=0xFE6F
            | 0xFF01..=0xFF60
            | 0xFFE0..=0xFFE6
            | 0x1B000..=0x1B2FF
            | 0x1F000..=0x1FAFF
            | 0x20000..=0x3FFFF => Upright,
            // The enclosed numbers and letters, the arrows and the symbols
            // stand upright too: a circled digit in a vertical line does not
            // lie down.
            0x2460..=0x24FF | 0x2600..=0x27BF | 0x2E3A..=0x2E3B => Upright,
            // Halfwidth katakana are the exception among the Japanese
            // characters: they were made for a screen that could only write
            // across, and they lie down.
            0xFF65..=0xFF9F => Rotated,
            _ => Rotated,
        }
    }

    /// Whether the character stands upright when the font is asked for
    /// nothing: the two kinds that do not depend on the font.
    #[must_use]
    pub fn is_upright(self) -> bool {
        matches!(self, Self::Upright | Self::TransformedUpright)
    }

    /// Whether the font's vertical form should be asked for.
    #[must_use]
    pub fn wants_vertical_form(self) -> bool {
        matches!(self, Self::TransformedUpright | Self::TransformedRotated)
    }
}

/// Whether any character of a run stands upright in a vertical line, so a
/// caller can leave a run of Latin alone without asking about each letter.
#[must_use]
pub fn has_upright(text: &str) -> bool {
    text.chars().any(|character| Orientation::of(character) != Orientation::Rotated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ideographs_and_kana_stand_and_latin_lies_down() {
        assert_eq!(Orientation::of('漢'), Orientation::Upright);
        assert_eq!(Orientation::of('か'), Orientation::Upright);
        assert_eq!(Orientation::of('カ'), Orientation::Upright);
        assert_eq!(Orientation::of('한'), Orientation::Upright);
        assert_eq!(Orientation::of('\u{3000}'), Orientation::Upright);
        assert_eq!(Orientation::of('A'), Orientation::Rotated);
        assert_eq!(Orientation::of('1'), Orientation::Rotated);
        assert_eq!(Orientation::of(' '), Orientation::Rotated);
        assert_eq!(Orientation::of('я'), Orientation::Rotated);
    }

    #[test]
    fn the_marks_that_change_their_corner_are_transformed() {
        assert_eq!(Orientation::of('。'), Orientation::TransformedUpright);
        assert_eq!(Orientation::of('、'), Orientation::TransformedUpright);
        assert_eq!(Orientation::of('「'), Orientation::TransformedUpright);
        assert_eq!(Orientation::of('」'), Orientation::TransformedUpright);
        assert_eq!(Orientation::of('っ'), Orientation::TransformedUpright);
        assert_eq!(Orientation::of('ー'), Orientation::TransformedRotated);
        assert_eq!(Orientation::of('…'), Orientation::TransformedRotated);
        assert_eq!(Orientation::of('—'), Orientation::TransformedRotated);
    }

    #[test]
    fn halfwidth_katakana_lie_down_and_fullwidth_letters_stand() {
        assert_eq!(Orientation::of('\u{FF76}'), Orientation::Rotated);
        assert_eq!(Orientation::of('\u{FF21}'), Orientation::Upright);
        assert_eq!(Orientation::of('\u{FF11}'), Orientation::Upright);
        assert!(has_upright("abc漢"));
        assert!(!has_upright("abc 123"));
    }
}
