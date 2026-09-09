//! The characters that are drawn the other way round when the text reads
//! right to left.
//!
//! A parenthesis is not a shape, it is a role: the one that opens and the one
//! that closes. Hebrew and Arabic read from the right, so the bracket that
//! opens a phrase there is the one an English reader calls a closing bracket —
//! and a document holds the opening one, because that is what was typed. Rule
//! L4 of [UAX #9] says the drawing swaps them; this module is the table it
//! swaps them by.
//!
//! The same table says which brackets pair with which, which rule N0 needs to
//! work out which way a bracketed phrase reads.
//!
//! [UAX #9]: https://www.unicode.org/reports/tr9/

/// Which end of a pair a bracket is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Side {
    Opening,
    Closing,
}

/// The character drawn in place of this one where the text reads right to left.
///
/// `None` for everything that looks the same either way, which is nearly
/// everything: a letter has no mirror image and is never drawn as one.
#[must_use]
pub fn mirrored(character: char) -> Option<char> {
    let code = character as u32;
    for (left, right) in BRACKETS.iter().chain(MIRRORED) {
        if code == *left {
            return char::from_u32(*right);
        }
        if code == *right {
            return char::from_u32(*left);
        }
    }
    None
}

/// Which end of which pair a bracket is, for the rule that works out which way
/// a bracketed phrase reads.
///
/// Only the brackets: `<` is drawn mirrored but does not open anything.
pub(crate) fn bracket(character: char) -> Option<(Side, char)> {
    let code = character as u32;
    for (open, close) in BRACKETS {
        if code == *open {
            return char::from_u32(*close).map(|closing| (Side::Opening, closing));
        }
        if code == *close {
            return char::from_u32(*close).map(|closing| (Side::Closing, closing));
        }
    }
    None
}

/// Whether two closing brackets are the same bracket.
///
/// Unicode has two of a few of them — an angle bracket that came from a maths
/// standard and one that came from a Japanese one — and a phrase opened with
/// either may be closed with either.
pub(crate) fn same_bracket(first: char, second: char) -> bool {
    if first == second {
        return true;
    }
    let pair = |a: char, b: char| (first == a && second == b) || (first == b && second == a);
    pair('\u{2329}', '\u{3008}') || pair('\u{232A}', '\u{3009}')
}

/// The brackets, each written as the pair it belongs to.
const BRACKETS: &[(u32, u32)] = &[
    (0x0028, 0x0029),
    (0x005B, 0x005D),
    (0x007B, 0x007D),
    (0x0F3A, 0x0F3B),
    (0x0F3C, 0x0F3D),
    (0x169B, 0x169C),
    (0x2045, 0x2046),
    (0x207D, 0x207E),
    (0x208D, 0x208E),
    (0x2308, 0x2309),
    (0x230A, 0x230B),
    (0x2329, 0x232A),
    (0x2768, 0x2769),
    (0x276A, 0x276B),
    (0x276C, 0x276D),
    (0x276E, 0x276F),
    (0x2770, 0x2771),
    (0x2772, 0x2773),
    (0x2774, 0x2775),
    (0x27C5, 0x27C6),
    (0x27E6, 0x27E7),
    (0x27E8, 0x27E9),
    (0x27EA, 0x27EB),
    (0x27EC, 0x27ED),
    (0x27EE, 0x27EF),
    (0x2983, 0x2984),
    (0x2985, 0x2986),
    (0x2987, 0x2988),
    (0x2989, 0x298A),
    (0x298B, 0x298C),
    // These two cross over, and the table is the only reason anyone would know.
    (0x298D, 0x2990),
    (0x298F, 0x298E),
    (0x2991, 0x2992),
    (0x2993, 0x2994),
    (0x2995, 0x2996),
    (0x2997, 0x2998),
    (0x29D8, 0x29D9),
    (0x29DA, 0x29DB),
    (0x29FC, 0x29FD),
    (0x2E22, 0x2E23),
    (0x2E24, 0x2E25),
    (0x2E26, 0x2E27),
    (0x2E28, 0x2E29),
    (0x3008, 0x3009),
    (0x300A, 0x300B),
    (0x300C, 0x300D),
    (0x300E, 0x300F),
    (0x3010, 0x3011),
    (0x3014, 0x3015),
    (0x3016, 0x3017),
    (0x3018, 0x3019),
    (0x301A, 0x301B),
    (0xFE59, 0xFE5A),
    (0xFE5B, 0xFE5C),
    (0xFE5D, 0xFE5E),
    (0xFF08, 0xFF09),
    (0xFF3B, 0xFF3D),
    (0xFF5B, 0xFF5D),
    (0xFF5F, 0xFF60),
    (0xFF62, 0xFF63),
];

/// The rest of what is drawn mirrored: the comparisons, the quotation marks
/// that point, an arrow.
///
/// These have a mirror image but open and close nothing, so no rule pairs them
/// up — they are only swapped when they are drawn.
const MIRRORED: &[(u32, u32)] = &[
    (0x003C, 0x003E),
    (0x00AB, 0x00BB),
    (0x2039, 0x203A),
    (0x2190, 0x2192),
    (0x219A, 0x219B),
    (0x21A4, 0x21A6),
    (0x21A9, 0x21AA),
    (0x21BC, 0x21C0),
    (0x21BD, 0x21C1),
    (0x21C7, 0x21C9),
    (0x2208, 0x220B),
    (0x2209, 0x220C),
    (0x220A, 0x220D),
    (0x2243, 0x22CD),
    (0x2252, 0x2253),
    (0x2254, 0x2255),
    (0x2264, 0x2265),
    (0x2266, 0x2267),
    (0x2268, 0x2269),
    (0x226A, 0x226B),
    (0x226E, 0x226F),
    (0x2270, 0x2271),
    (0x2272, 0x2273),
    (0x227A, 0x227B),
    (0x227C, 0x227D),
    (0x2280, 0x2281),
    (0x2282, 0x2283),
    (0x2284, 0x2285),
    (0x2286, 0x2287),
    (0x2288, 0x2289),
    (0x22A2, 0x22A3),
    (0x22B0, 0x22B1),
    (0x22D0, 0x22D1),
    (0x22D6, 0x22D7),
    (0x22F2, 0x22FA),
    (0x2A7D, 0x2A7E),
    (0x2A95, 0x2A96),
];

#[cfg(test)]
mod tests {
    use super::{bracket, mirrored, same_bracket, Side};

    #[test]
    fn a_bracket_is_drawn_as_the_other_end_of_its_pair() {
        assert_eq!(mirrored('('), Some(')'));
        assert_eq!(mirrored(')'), Some('('));
        assert_eq!(mirrored('['), Some(']'));
        assert_eq!(mirrored('{'), Some('}'));
    }

    #[test]
    fn a_comparison_is_drawn_the_other_way_round_too() {
        assert_eq!(mirrored('<'), Some('>'));
        assert_eq!(mirrored('\u{00AB}'), Some('\u{00BB}'));
        assert_eq!(mirrored('\u{2264}'), Some('\u{2265}'));
    }

    #[test]
    fn a_letter_has_no_mirror_image() {
        assert_eq!(mirrored('a'), None);
        assert_eq!(mirrored('\u{05D0}'), None);
        // Nor has a quotation mark: it is the same shape whichever way the
        // text runs.
        assert_eq!(mirrored('"'), None);
    }

    #[test]
    fn a_bracket_knows_which_end_it_is() {
        assert_eq!(bracket('('), Some((Side::Opening, ')')));
        assert_eq!(bracket(')'), Some((Side::Closing, ')')));
        assert_eq!(bracket('<'), None, "a comparison opens nothing");
    }

    #[test]
    fn the_two_angle_brackets_are_the_same_bracket() {
        assert!(same_bracket('\u{232A}', '\u{3009}'));
        assert!(!same_bracket(')', ']'));
    }
}
