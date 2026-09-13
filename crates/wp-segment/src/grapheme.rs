//! Where one character the reader sees ends and the next begins.
//!
//! A `char` in Rust is a Unicode code point, which is not what a person means
//! by a character. `é` may be one code point or two — `e` and an accent that
//! draws on top of it. A flag is two. A family emoji is seven, joined by an
//! invisible character whose whole job is to say "these draw as one".
//!
//! The caret must step over the thing a person sees, or Left arrow leaves it
//! between a letter and its accent and Backspace takes the skin tone off a
//! hand. What that thing is has a name — a *grapheme cluster* — and rules, in
//! [UAX #29], and the rules are here.
//!
//! [UAX #29]: https://www.unicode.org/reports/tr29/

/// What a character does to the boundary beside it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Class {
    /// Anything that stands on its own: a letter, a digit, a full stop.
    Other,
    CarriageReturn,
    LineFeed,
    /// A character that draws nothing and joins nothing.
    Control,
    /// A mark that draws on the character before it: an accent, a vowel sign, a
    /// skin tone, a variation selector.
    Extend,
    /// The zero width joiner, which welds two pictures into one.
    Joiner,
    /// Half of a flag: flags are written as two letters from a private
    /// alphabet, and it is the pair that draws as a flag.
    Regional,
    /// A character that attaches to whatever follows it.
    Prepend,
    /// A vowel sign that takes room of its own but still belongs to its
    /// consonant, as the Indic scripts are full of.
    SpacingMark,
    /// Hangul: a leading consonant, a vowel, a trailing consonant, and the
    /// composed syllables of the two shapes.
    Leading,
    Vowel,
    Trailing,
    LeadingVowel,
    LeadingVowelTrailing,
    /// An emoji, which may be joined to another one.
    Pictographic,
}

/// Every place in the text where one cluster ends and the next begins.
///
/// The ends count: the first offset is always 0 and the last is always the
/// length, so the boundaries chop the text into clusters with nothing left
/// over.
#[must_use]
pub fn boundaries(text: &str) -> Vec<usize> {
    let mut out = vec![0];
    if text.is_empty() {
        return out;
    }

    let mut previous: Option<Class> = None;
    // How many regional indicators run up to here without a break, which is
    // what says whether the next one starts a flag or finishes one.
    let mut flag_run = 0usize;
    // Whether what has been read so far is an emoji followed by any number of
    // marks — the shape a joiner may hang another emoji from.
    let mut picture_chain = false;
    // Whether the character just read was that joiner.
    let mut joined_picture = false;

    for (offset, character) in text.char_indices() {
        let class = class_of(character);
        if let Some(before) = previous {
            if breaks(before, class, flag_run, joined_picture) {
                out.push(offset);
                flag_run = 0;
            }
        }

        flag_run = if class == Class::Regional { flag_run + 1 } else { 0 };
        joined_picture = class == Class::Joiner && picture_chain;
        picture_chain = match class {
            Class::Pictographic => true,
            Class::Extend => picture_chain,
            _ => false,
        };
        previous = Some(class);
    }

    out.push(text.len());
    out
}

/// Where the cluster after an offset begins.
#[must_use]
pub fn next(text: &str, offset: usize) -> usize {
    boundaries(text).into_iter().find(|at| *at > offset).unwrap_or(text.len())
}

/// Where the cluster before an offset begins.
#[must_use]
pub fn previous(text: &str, offset: usize) -> usize {
    boundaries(text).into_iter().rev().find(|at| *at < offset).unwrap_or(0)
}

/// Whether a cluster ends between two characters.
///
/// The rules are the standard's, in its order, and the first that speaks
/// decides; the last one says that anything not spoken for is a boundary,
/// which is why unknown characters stand alone rather than sticking together.
fn breaks(before: Class, after: Class, flag_run: usize, joined_picture: bool) -> bool {
    use Class::{
        CarriageReturn, Control, Extend, Joiner, Leading, LeadingVowel, LeadingVowelTrailing,
        LineFeed, Pictographic, Prepend, Regional, SpacingMark, Trailing, Vowel,
    };

    match (before, after) {
        // GB3 to GB5: the line ending is one cluster, and nothing joins a
        // control character.
        (CarriageReturn, LineFeed) => false,
        (CarriageReturn | LineFeed | Control, _) | (_, CarriageReturn | LineFeed | Control) => true,
        // GB6 to GB8: a Hangul syllable spelled out in its parts is one
        // cluster, in the order the parts may appear.
        (Leading, Leading | Vowel | LeadingVowel | LeadingVowelTrailing) => false,
        (LeadingVowel | Vowel, Vowel | Trailing) => false,
        (LeadingVowelTrailing | Trailing, Trailing) => false,
        // GB9, GB9a and GB9b: a mark goes with what it draws on, and a prepend
        // with what follows it.
        (_, Extend | Joiner | SpacingMark) | (Prepend, _) => false,
        // GB11: emoji, marks, a joiner, and another emoji: one picture.
        (Joiner, Pictographic) => !joined_picture,
        // GB12 and GB13: two regional indicators make a flag, and a third
        // starts a new one rather than joining the first.
        (Regional, Regional) => flag_run % 2 == 0,
        // GB999.
        _ => true,
    }
}

/// The class of one character.
///
/// The table behind this is generated from the character database, so there is
/// no character it does not know; the emoji are laid over it, because the rule
/// that joins two of them together asks a question the property does not
/// answer.
pub(crate) fn class_of(character: char) -> Class {
    crate::tables::grapheme_class_of(character)
}

#[cfg(test)]
mod tests {
    use super::{boundaries, next, previous};

    /// The text chopped into the clusters a reader would count.
    fn clusters(text: &str) -> Vec<&str> {
        boundaries(text).windows(2).map(|pair| &text[pair[0]..pair[1]]).collect()
    }

    #[test]
    fn plain_text_is_one_cluster_per_letter() {
        assert_eq!(clusters("abc"), ["a", "b", "c"]);
    }

    #[test]
    fn a_letter_and_its_accent_are_one_character() {
        // "e" and a combining acute: two code points, one character.
        assert_eq!(clusters("e\u{0301}f"), ["e\u{0301}", "f"]);
    }

    #[test]
    fn a_flag_is_one_character_and_two_flags_are_two() {
        let flags = "\u{1F1FA}\u{1F1E6}\u{1F1EC}\u{1F1E7}";
        assert_eq!(clusters(flags), ["\u{1F1FA}\u{1F1E6}", "\u{1F1EC}\u{1F1E7}"]);
    }

    #[test]
    fn an_odd_regional_indicator_stands_alone() {
        let text = "\u{1F1FA}\u{1F1E6}\u{1F1EC}";
        assert_eq!(clusters(text), ["\u{1F1FA}\u{1F1E6}", "\u{1F1EC}"]);
    }

    #[test]
    fn a_family_joined_by_zero_width_joiners_is_one_character() {
        let family = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}";
        assert_eq!(clusters(family).len(), 1, "the family came apart");
    }

    #[test]
    fn a_skin_tone_belongs_to_the_hand_it_colours() {
        let wave = "\u{1F44B}\u{1F3FD}";
        assert_eq!(clusters(wave).len(), 1);
    }

    #[test]
    fn a_line_ending_of_two_characters_is_one() {
        assert_eq!(clusters("a\r\nb"), ["a", "\r\n", "b"]);
    }

    #[test]
    fn a_hangul_syllable_spelled_in_parts_is_one_character() {
        // Leading consonant, vowel, trailing consonant.
        assert_eq!(clusters("\u{1112}\u{1161}\u{11AB}").len(), 1);
    }

    #[test]
    fn the_caret_steps_over_a_whole_character() {
        let text = "e\u{0301}f";
        assert_eq!(next(text, 0), 3, "the caret stopped between the letter and its accent");
        assert_eq!(previous(text, 3), 0);
        assert_eq!(next(text, 3), 4);
        assert_eq!(previous(text, 4), 3);
    }

    #[test]
    fn the_ends_are_boundaries_and_nothing_runs_past_them() {
        assert_eq!(boundaries(""), vec![0]);
        assert_eq!(next("abc", 3), 3);
        assert_eq!(previous("abc", 0), 0);
    }
}
