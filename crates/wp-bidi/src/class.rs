//! Which bidirectional class each character belongs to.
//!
//! # Why this is a table and not a lookup in the character database
//!
//! Because there is no character database here yet. The classes come from
//! `DerivedBidiClass.txt`, which is a hundred thousand lines long, and almost
//! all of it says the same thing: left-to-right. What actually matters for
//! ordering text is the small set of ranges that are *not* left-to-right — the
//! right-to-left scripts, the numbers, the separators and the marks — and those
//! are listed here, from the standard, with the range each covers.
//!
//! Anything not named is left-to-right, which is the default the standard
//! itself gives for unassigned characters in most planes. A character in a
//! script this table does not know is therefore laid out left to right, which
//! is right for every script that reads that way and wrong only for a
//! right-to-left script nobody has added — a fault that shows itself plainly
//! rather than hiding.

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
#[must_use]
pub fn class_of(character: char) -> Class {
    let code = character as u32;

    // The explicit codes first: they are single characters and they change
    // everything about what follows them.
    match code {
        0x202A => return Class::LRE,
        0x202B => return Class::RLE,
        0x202D => return Class::LRO,
        0x202E => return Class::RLO,
        0x202C => return Class::PDF,
        0x2066 => return Class::LRI,
        0x2067 => return Class::RLI,
        0x2068 => return Class::FSI,
        0x2069 => return Class::PDI,
        _ => {}
    }

    for (first, last, class) in RANGES {
        if code >= *first && code <= *last {
            return *class;
        }
    }
    Class::L
}

/// The ranges that are not left-to-right, in order.
///
/// Read as: everything from `first` to `last` has this class. Taken from the
/// standard's derived classes, keeping the ranges a document is likely to
/// hold — every right-to-left script, the numbers and the punctuation that
/// behaves differently between them.
const RANGES: &[(u32, u32, Class)] = &[
    // Control characters that are ignored altogether.
    (0x0000, 0x0008, Class::BN),
    (0x000E, 0x001B, Class::BN),
    (0x007F, 0x0084, Class::BN),
    (0x0086, 0x009F, Class::BN),
    (0x00AD, 0x00AD, Class::BN),
    (0x200B, 0x200D, Class::BN),
    (0x2060, 0x2064, Class::BN),
    (0xFEFF, 0xFEFF, Class::BN),
    // Tab is a segment separator; the line and paragraph ends are paragraph
    // separators.
    (0x0009, 0x0009, Class::S),
    (0x000B, 0x000B, Class::S),
    (0x001F, 0x001F, Class::S),
    (0x000A, 0x000A, Class::B),
    (0x000D, 0x000D, Class::B),
    (0x001C, 0x001E, Class::B),
    (0x0085, 0x0085, Class::B),
    (0x2029, 0x2029, Class::B),
    // Whitespace.
    (0x000C, 0x000C, Class::WS),
    (0x0020, 0x0020, Class::WS),
    (0x1680, 0x1680, Class::WS),
    (0x2000, 0x200A, Class::WS),
    (0x2028, 0x2028, Class::WS),
    (0x205F, 0x205F, Class::WS),
    (0x3000, 0x3000, Class::WS),
    // The marks that hang off the letter before them.
    (0x0300, 0x036F, Class::NSM),
    (0x0483, 0x0489, Class::NSM),
    (0x0591, 0x05BD, Class::NSM),
    (0x05BF, 0x05BF, Class::NSM),
    (0x05C1, 0x05C2, Class::NSM),
    (0x05C4, 0x05C5, Class::NSM),
    (0x05C7, 0x05C7, Class::NSM),
    (0x0610, 0x061A, Class::NSM),
    (0x064B, 0x065F, Class::NSM),
    (0x0670, 0x0670, Class::NSM),
    (0x06D6, 0x06DC, Class::NSM),
    (0x06DF, 0x06E4, Class::NSM),
    (0x06E7, 0x06E8, Class::NSM),
    (0x06EA, 0x06ED, Class::NSM),
    (0x0711, 0x0711, Class::NSM),
    (0x0730, 0x074A, Class::NSM),
    (0x07A6, 0x07B0, Class::NSM),
    (0x07EB, 0x07F3, Class::NSM),
    (0x0816, 0x0819, Class::NSM),
    (0x081B, 0x0823, Class::NSM),
    (0x0825, 0x0827, Class::NSM),
    (0x0829, 0x082D, Class::NSM),
    (0x0859, 0x085B, Class::NSM),
    (0x1AB0, 0x1AFF, Class::NSM),
    (0x1DC0, 0x1DFF, Class::NSM),
    (0x20D0, 0x20F0, Class::NSM),
    (0xFE00, 0xFE0F, Class::NSM),
    (0xFE20, 0xFE2F, Class::NSM),
    // European numbers and what goes with them.
    (0x0030, 0x0039, Class::EN),
    (0x00B2, 0x00B3, Class::EN),
    (0x00B9, 0x00B9, Class::EN),
    (0x06F0, 0x06F9, Class::EN),
    (0x2070, 0x2070, Class::EN),
    (0x2074, 0x2079, Class::EN),
    (0x2080, 0x2089, Class::EN),
    (0xFF10, 0xFF19, Class::EN),
    (0x002B, 0x002B, Class::ES),
    (0x002D, 0x002D, Class::ES),
    (0x207A, 0x207B, Class::ES),
    (0x208A, 0x208B, Class::ES),
    (0xFB29, 0xFB29, Class::ES),
    (0xFE62, 0xFE63, Class::ES),
    (0xFF0B, 0xFF0B, Class::ES),
    (0xFF0D, 0xFF0D, Class::ES),
    (0x0023, 0x0025, Class::ET),
    (0x00A2, 0x00A5, Class::ET),
    (0x00B0, 0x00B1, Class::ET),
    (0x058F, 0x058F, Class::ET),
    (0x0609, 0x060A, Class::ET),
    (0x066A, 0x066A, Class::ET),
    (0x09F2, 0x09F3, Class::ET),
    (0x20A0, 0x20BF, Class::ET),
    (0x2030, 0x2034, Class::ET),
    (0x212E, 0x212E, Class::ET),
    (0xFE5F, 0xFE5F, Class::ET),
    (0xFE69, 0xFE6A, Class::ET),
    (0xFF03, 0xFF05, Class::ET),
    (0xFFE0, 0xFFE1, Class::ET),
    (0xFFE5, 0xFFE6, Class::ET),
    // Arabic numbers, which are numbers written in a right-to-left script and
    // are ordered differently from European ones.
    (0x0600, 0x0605, Class::AN),
    (0x0660, 0x0669, Class::AN),
    (0x066B, 0x066C, Class::AN),
    (0x06DD, 0x06DD, Class::AN),
    (0x0890, 0x0891, Class::AN),
    (0x08E2, 0x08E2, Class::AN),
    (0x10D30, 0x10D39, Class::AN),
    // Common separators: the comma, the full stop, the colon and the slash,
    // which join two numbers together.
    (0x002C, 0x002C, Class::CS),
    (0x002E, 0x002F, Class::CS),
    (0x003A, 0x003A, Class::CS),
    (0x00A0, 0x00A0, Class::CS),
    (0x060C, 0x060C, Class::CS),
    (0x202F, 0x202F, Class::CS),
    (0x2044, 0x2044, Class::CS),
    (0xFE50, 0xFE50, Class::CS),
    (0xFE52, 0xFE52, Class::CS),
    (0xFE55, 0xFE55, Class::CS),
    (0xFF0C, 0xFF0C, Class::CS),
    (0xFF0E, 0xFF0F, Class::CS),
    (0xFF1A, 0xFF1A, Class::CS),
    // Hebrew and the other right-to-left scripts that are not Arabic.
    (0x0590, 0x05FF, Class::R),
    (0x07C0, 0x085F, Class::R),
    (0xFB1D, 0xFB4F, Class::R),
    (0x10800, 0x10CFF, Class::R),
    (0x10D40, 0x10FFF, Class::R),
    (0x1E800, 0x1EC6F, Class::R),
    (0x1ECC0, 0x1ECFF, Class::R),
    (0x1ED50, 0x1EDFF, Class::R),
    (0x1EF00, 0x1EFFF, Class::R),
    (0x200F, 0x200F, Class::R),
    (0xFB1D, 0xFB1D, Class::R),
    // Arabic letters, which take their own class because a number beside them
    // is ordered differently.
    (0x0608, 0x0608, Class::AL),
    (0x060B, 0x060B, Class::AL),
    (0x060D, 0x061A, Class::AL),
    (0x061C, 0x064A, Class::AL),
    (0x066D, 0x066F, Class::AL),
    (0x0671, 0x06D5, Class::AL),
    (0x06E5, 0x06E6, Class::AL),
    (0x06EE, 0x06EF, Class::AL),
    (0x06FA, 0x0710, Class::AL),
    (0x0712, 0x072F, Class::AL),
    (0x074D, 0x07A5, Class::AL),
    (0x07B1, 0x07BF, Class::AL),
    (0x0860, 0x08E1, Class::AL),
    (0x08E3, 0x08FF, Class::AL),
    (0xFB50, 0xFDFF, Class::AL),
    (0xFE70, 0xFEFE, Class::AL),
    (0x1EC70, 0x1ECBF, Class::AL),
    (0x1ED00, 0x1ED4F, Class::AL),
    (0x1EE00, 0x1EEFF, Class::AL),
    // The punctuation and symbols that take their direction from what is
    // around them.
    (0x0021, 0x0022, Class::ON),
    (0x0026, 0x002A, Class::ON),
    (0x003B, 0x0040, Class::ON),
    (0x005B, 0x0060, Class::ON),
    (0x007B, 0x007E, Class::ON),
    (0x00A1, 0x00A1, Class::ON),
    (0x00A6, 0x00A9, Class::ON),
    (0x00AB, 0x00AC, Class::ON),
    (0x00AE, 0x00AF, Class::ON),
    (0x00B4, 0x00B8, Class::ON),
    (0x00BB, 0x00BF, Class::ON),
    (0x00D7, 0x00D7, Class::ON),
    (0x00F7, 0x00F7, Class::ON),
    (0x2010, 0x2027, Class::ON),
    (0x2035, 0x2043, Class::ON),
    (0x2045, 0x205E, Class::ON),
    (0x2100, 0x2101, Class::ON),
    (0x2190, 0x2BFF, Class::ON),
    (0x3001, 0x3020, Class::ON),
    (0xFF01, 0xFF02, Class::ON),
    (0xFF06, 0xFF0A, Class::ON),
    (0xFF1B, 0xFF20, Class::ON),
    (0xFF3B, 0xFF40, Class::ON),
    (0xFF5B, 0xFF65, Class::ON),
];

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
}
