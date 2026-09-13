//! Which bidirectional class each character belongs to.
//!
//! The class is what the algorithm of [UAX #9] works on: whether a character
//! reads left to right or right to left, whether it is a digit, a separator, a
//! mark drawn on the letter before it, or one of the codes that change the
//! direction of everything after them.
//!
//! The table itself is generated from the character database and covers every
//! code point, unassigned ones included — those take the direction the
//! standard gives their block, which is how an unassigned character in the
//! Hebrew block still reads right to left.
//!
//! [UAX #9]: https://www.unicode.org/reports/tr9/

/// The bidirectional class of a character, as [UAX #9] names them.
///
/// [UAX #9]: https://www.unicode.org/reports/tr9/
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    /// Strong: left-to-right, right-to-left, and Arabic letter.
    L,
    R,
    AL,
    /// Weak: European number, separator and terminator, Arabic number, common
    /// separator, non-spacing mark, boundary neutral.
    EN,
    ES,
    ET,
    AN,
    CS,
    NSM,
    BN,
    /// Neutral: paragraph separator, segment separator, whitespace, other.
    B,
    S,
    WS,
    ON,
    /// Explicit: the embedding and override codes, and the isolates.
    LRE,
    RLE,
    LRO,
    RLO,
    PDF,
    LRI,
    RLI,
    FSI,
    PDI,
}

impl Class {
    /// Whether the class is one of the three strong ones.
    #[must_use]
    pub fn is_strong(self) -> bool {
        matches!(self, Self::L | Self::R | Self::AL)
    }

    /// Whether it is an isolate initiator.
    #[must_use]
    pub fn is_isolate_start(self) -> bool {
        matches!(self, Self::LRI | Self::RLI | Self::FSI)
    }

    /// Whether it removes an embedding or an override.
    #[must_use]
    pub fn is_explicit(self) -> bool {
        matches!(
            self,
            Self::LRE
                | Self::RLE
                | Self::LRO
                | Self::RLO
                | Self::PDF
                | Self::LRI
                | Self::RLI
                | Self::FSI
                | Self::PDI
        )
    }
}

/// The class of one character.
///
/// The table behind this covers every code point there is, so there is no such
/// thing as a character it does not know.
#[must_use]
pub fn class_of(character: char) -> Class {
    crate::tables::class_of(character)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latin_and_cyrillic_read_left_to_right() {
        assert_eq!(class_of('a'), Class::L);
        assert_eq!(class_of('Я'), Class::L);
        assert_eq!(class_of('漢'), Class::L);
    }

    #[test]
    fn hebrew_is_right_to_left_and_arabic_is_its_own_class() {
        assert_eq!(class_of('א'), Class::R);
        assert_eq!(class_of('ا'), Class::AL);
    }

    #[test]
    fn the_two_kinds_of_number_are_told_apart() {
        assert_eq!(class_of('5'), Class::EN);
        assert_eq!(class_of('٥'), Class::AN);
    }

    #[test]
    fn punctuation_takes_its_direction_from_its_neighbours() {
        assert_eq!(class_of('!'), Class::ON);
        assert_eq!(class_of('('), Class::ON);
        assert_eq!(class_of(' '), Class::WS);
        assert_eq!(class_of(','), Class::CS);
    }

    #[test]
    fn the_explicit_codes_are_recognised() {
        assert_eq!(class_of('\u{202B}'), Class::RLE);
        assert_eq!(class_of('\u{202C}'), Class::PDF);
        assert_eq!(class_of('\u{2068}'), Class::FSI);
        assert!(Class::RLI.is_isolate_start());
    }

    #[test]
    fn a_mark_belongs_to_the_letter_before_it() {
        assert_eq!(class_of('\u{05B0}'), Class::NSM);
        assert_eq!(class_of('\u{064E}'), Class::NSM);
    }

    #[test]
    fn a_script_nobody_listed_by_hand_still_reads_its_own_way() {
        // The whole point of generating the table: these are right-to-left
        // scripts, and not one of them is a script anybody had written down.
        assert_eq!(class_of('\u{07CA}'), Class::R, "N'Ko");
        assert_eq!(class_of('\u{0800}'), Class::R, "Samaritan");
        assert_eq!(class_of('\u{10800}'), Class::R, "Cypriot");
        assert_eq!(class_of('\u{1E900}'), Class::R, "Adlam");
        assert_eq!(class_of('\u{1EE00}'), Class::AL, "Arabic mathematics");
    }

    #[test]
    fn an_unassigned_character_takes_the_direction_of_its_block() {
        // Nothing is written at U+05EB, but it is in the Hebrew block, and the
        // standard says a character there reads right to left. A table that
        // fell back to left-to-right would turn a line round at it.
        assert_eq!(class_of('\u{05EB}'), Class::R);
        assert_eq!(class_of('\u{08B5}'), Class::AL);
    }

    #[test]
    fn every_character_there_is_has_a_class() {
        // Not a figure of speech: every code point is asked, and the table is
        // in order, which is what lets the search be a search and not a walk.
        for code in 0..=0x10FFFFu32 {
            if let Some(character) = char::from_u32(code) {
                let _ = class_of(character);
            }
        }
        assert!(crate::tables::in_order(), "the table is out of order");
    }
}
