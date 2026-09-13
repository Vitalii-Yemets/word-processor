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
    crate::tables::mirror_of(character as u32).and_then(char::from_u32)
}

/// Which end of which pair a bracket is, for the rule that works out which way
/// a bracketed phrase reads.
///
/// Only the brackets: `<` is drawn mirrored but does not open anything.
pub(crate) fn bracket(character: char) -> Option<(Side, char)> {
    let code = character as u32;
    for (open, close) in crate::tables::BRACKETS {
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
