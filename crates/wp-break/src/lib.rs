//! Where a line of text may be broken.
//!
//! # Why this is not "break at the spaces"
//!
//! Because half the world writes without them. Chinese, Japanese and Korean put
//! no spaces between words, and a line may be broken between almost any two
//! characters — but not any two: a line may not begin with a full stop or a
//! closing bracket, and may not end with an opening one. Japanese typesetting
//! has a name for those rules, *kinsoku shori*, and a reader sees a broken one
//! immediately.
//!
//! And even in English, breaking only at spaces is wrong. A hyphenated word
//! breaks after its hyphen, a non-breaking space is a space that must not be
//! broken at, and a line may not begin with a comma however long the word
//! before it.
//!
//! # What is here
//!
//! The part of [UAX #14] that decides these cases: each character is given a
//! class, and the classes either side of a possible break say whether it is
//! allowed. The standard's full pair table is forty classes square and settles
//! a great many cases that never arise in a document; what is here is the
//! classes that do arise, and every rule is named where it is applied.
//!
//! Hyphenation — breaking *inside* a word, at a place the language allows — is
//! a different problem needing pattern data per language, and is not here.
//!
//! [UAX #14]: https://www.unicode.org/reports/tr14/
//!
//! # Example
//!
//! ```
//! // A line may be broken after a hyphen, but never before a full stop.
//! assert!(wp_break::may_break('-', 'k'));
//! assert!(!wp_break::may_break('d', '.'));
//! ```

#![forbid(unsafe_code)]

/// What a character does to a break beside it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    /// A space: a break is allowed after a run of them.
    Space,
    /// A line feed or a paragraph separator, which breaks whatever is beside
    /// it.
    Mandatory,
    /// An opening bracket or quote: nothing may be broken away from it.
    Open,
    /// A closing bracket, and the punctuation that clings to what precedes it —
    /// a full stop, a comma, an exclamation mark.
    Close,
    /// A quotation mark, which could be either and is treated as neither.
    Quote,
    /// Glue: a non-breaking space or hyphen, which forbids a break on both
    /// sides.
    Glue,
    /// A character that may not begin a line, though it may end one: the small
    /// kana, the sound marks, the ellipsis.
    NonStarter,
    /// A hyphen, which a line may be broken after.
    Hyphen,
    /// Something a break is allowed after: an en dash, a slash, an ideographic
    /// space.
    BreakAfter,
    /// Something a break is allowed before.
    BreakBefore,
    /// An ideograph or a kana, which may be broken between.
    Ideograph,
    /// A digit.
    Numeric,
    /// A sign that goes before a number, such as a currency sign.
    Prefix,
    /// A sign that goes after one, such as a per cent sign.
    Postfix,
    /// A letter.
    Alphabetic,
}

/// The class of one character.
#[must_use]
pub fn class_of(character: char) -> Class {
    let code = character as u32;
    match code {
        // The breaks the text itself asks for.
        0x000A | 0x000B | 0x000C | 0x000D | 0x0085 | 0x2028 | 0x2029 => return Class::Mandatory,
        // A space, and the ones that are not spaces at all.
        0x0020 => return Class::Space,
        0x00A0 | 0x2007 | 0x2011 | 0x202F | 0x2060 => return Class::Glue,
        0x0009 => return Class::BreakAfter,
        _ => {}
    }

    for (first, last, class) in RANGES {
        if code >= *first && code <= *last {
            return *class;
        }
    }

    if character.is_ascii_digit() {
        Class::Numeric
    } else if character.is_whitespace() {
        Class::Space
    } else {
        Class::Alphabetic
    }
}

/// Whether a line may be broken between two characters.
///
/// The rules are applied in the standard's order, and the first that speaks
/// decides. Anything the rules say nothing about is a break opportunity only
/// between two things that are not letters of the same word — which is what
/// the last two rules say.
#[must_use]
pub fn may_break(before: char, after: char) -> bool {
    let (left, right) = (class_of(before), class_of(after));

    // LB4 and LB5: a line feed breaks, and nothing may be broken away from it.
    if left == Class::Mandatory || right == Class::Mandatory {
        return left == Class::Mandatory;
    }

    // LB7: never before a space; a run of spaces goes with the line it ends.
    if right == Class::Space {
        return false;
    }

    // LB12 and LB12a: glue forbids a break on both sides. That is what makes a
    // non-breaking space non-breaking.
    if left == Class::Glue || right == Class::Glue {
        return false;
    }

    // LB18: after a space a break is always allowed.
    if left == Class::Space {
        return true;
    }

    // LB13 and LB16: never before closing punctuation or a non-starter, which
    // is the rule that keeps a full stop off the beginning of a line and a
    // small kana with the syllable it belongs to.
    if matches!(right, Class::Close | Class::NonStarter) {
        return false;
    }

    // LB14: never after an opening bracket.
    if left == Class::Open {
        return false;
    }

    // LB19: a quotation mark could open or close, so nothing is broken either
    // side of it.
    if left == Class::Quote || right == Class::Quote {
        return false;
    }

    // LB25: a number holds together with the signs around it. This is why
    // "$1,500" and "20%" are never broken up.
    if matches!(left, Class::Numeric | Class::Prefix)
        && matches!(right, Class::Numeric | Class::Postfix)
    {
        return false;
    }
    if left == Class::Numeric && right == Class::Alphabetic {
        return false;
    }

    // LB21: a break is allowed after a hyphen and before a break-before, and
    // never before a hyphen — "well-known" breaks after the hyphen, never in
    // front of it.
    if right == Class::Hyphen {
        return false;
    }
    if left == Class::Hyphen {
        // Except between two digits, where the hyphen is a minus sign or part
        // of a number and breaking would read as arithmetic.
        return right != Class::Numeric;
    }
    if left == Class::BreakAfter || right == Class::BreakBefore {
        return true;
    }

    // LB8a and LB23: an ideograph may be broken from anything, and anything
    // from an ideograph, which is what makes text without spaces wrap at all.
    if left == Class::Ideograph || right == Class::Ideograph {
        return true;
    }

    // LB28 and LB29: two letters, or a letter and a digit, are the inside of a
    // word and are never broken.
    false
}

/// Every place in a line where it may be broken, as byte offsets.
///
/// The offsets are the starts of the characters a break would put on the next
/// line. Neither end of the text is one: a break there would move nothing.
#[must_use]
pub fn opportunities(text: &str) -> Vec<usize> {
    let mut out = Vec::new();
    let mut previous: Option<(usize, char)> = None;
    for (offset, character) in text.char_indices() {
        if let Some((_, before)) = previous {
            if may_break(before, character) {
                out.push(offset);
            }
        }
        previous = Some((offset, character));
    }
    out
}

/// The ranges that are not ordinary letters.
///
/// Read as: everything from `first` to `last` behaves this way at a line break.
const RANGES: &[(u32, u32, Class)] = &[
    // The punctuation that clings to what comes before it.
    (0x0021, 0x0021, Class::Close),
    (0x002C, 0x002C, Class::Close),
    (0x002E, 0x002E, Class::Close),
    (0x003A, 0x003B, Class::Close),
    (0x003F, 0x003F, Class::Close),
    (0x0029, 0x0029, Class::Close),
    (0x005D, 0x005D, Class::Close),
    (0x007D, 0x007D, Class::Close),
    (0x00BB, 0x00BB, Class::Close),
    (0x2019, 0x2019, Class::Close),
    (0x201D, 0x201D, Class::Close),
    (0x203A, 0x203A, Class::Close),
    // And the openings, which nothing may be broken away from.
    (0x0028, 0x0028, Class::Open),
    (0x005B, 0x005B, Class::Open),
    (0x007B, 0x007B, Class::Open),
    (0x00AB, 0x00AB, Class::Open),
    (0x00BF, 0x00BF, Class::Open),
    (0x2018, 0x2018, Class::Open),
    (0x201C, 0x201C, Class::Open),
    (0x2039, 0x2039, Class::Open),
    // The quotation mark that could be either.
    (0x0022, 0x0022, Class::Quote),
    (0x0027, 0x0027, Class::Quote),
    // Hyphens, and the dashes a break is allowed after.
    (0x002D, 0x002D, Class::Hyphen),
    (0x058A, 0x058A, Class::Hyphen),
    (0x2010, 0x2010, Class::Hyphen),
    (0x2012, 0x2014, Class::BreakAfter),
    (0x002F, 0x002F, Class::BreakAfter),
    (0x2026, 0x2026, Class::NonStarter),
    // The signs that go before and after a number.
    (0x0024, 0x0024, Class::Prefix),
    (0x00A3, 0x00A5, Class::Prefix),
    (0x20A0, 0x20BF, Class::Prefix),
    (0x0025, 0x0025, Class::Postfix),
    (0x00B0, 0x00B0, Class::Postfix),
    (0x2030, 0x2030, Class::Postfix),
    (0x2103, 0x2103, Class::Postfix),
    // The Japanese and Chinese punctuation that may not begin a line.
    (0x3001, 0x3002, Class::Close),
    (0x30FB, 0x30FB, Class::NonStarter),
    (0xFF01, 0xFF01, Class::Close),
    (0xFF0C, 0xFF0C, Class::Close),
    (0xFF0E, 0xFF0E, Class::Close),
    (0xFF1A, 0xFF1B, Class::Close),
    (0xFF1F, 0xFF1F, Class::Close),
    (0xFF09, 0xFF09, Class::Close),
    (0xFF3D, 0xFF3D, Class::Close),
    (0xFF5D, 0xFF5D, Class::Close),
    (0x3009, 0x3009, Class::Close),
    (0x300B, 0x300B, Class::Close),
    (0x300D, 0x300D, Class::Close),
    (0x300F, 0x300F, Class::Close),
    (0x3011, 0x3011, Class::Close),
    (0x3015, 0x3015, Class::Close),
    (0x3019, 0x3019, Class::Close),
    (0x301B, 0x301B, Class::Close),
    // And the ones that may not end a line.
    (0x3008, 0x3008, Class::Open),
    (0x300A, 0x300A, Class::Open),
    (0x300C, 0x300C, Class::Open),
    (0x300E, 0x300E, Class::Open),
    (0x3010, 0x3010, Class::Open),
    (0x3014, 0x3014, Class::Open),
    (0x3018, 0x3018, Class::Open),
    (0x301A, 0x301A, Class::Open),
    (0xFF08, 0xFF08, Class::Open),
    (0xFF3B, 0xFF3B, Class::Open),
    (0xFF5B, 0xFF5B, Class::Open),
    // The small kana and the marks that belong to the syllable before them.
    (0x3041, 0x3041, Class::NonStarter),
    (0x3043, 0x3043, Class::NonStarter),
    (0x3045, 0x3045, Class::NonStarter),
    (0x3047, 0x3047, Class::NonStarter),
    (0x3049, 0x3049, Class::NonStarter),
    (0x3063, 0x3063, Class::NonStarter),
    (0x3083, 0x3083, Class::NonStarter),
    (0x3085, 0x3085, Class::NonStarter),
    (0x3087, 0x3087, Class::NonStarter),
    (0x308E, 0x308E, Class::NonStarter),
    (0x3095, 0x3096, Class::NonStarter),
    (0x309B, 0x309E, Class::NonStarter),
    (0x30A1, 0x30A1, Class::NonStarter),
    (0x30A3, 0x30A3, Class::NonStarter),
    (0x30A5, 0x30A5, Class::NonStarter),
    (0x30A7, 0x30A7, Class::NonStarter),
    (0x30A9, 0x30A9, Class::NonStarter),
    (0x30C3, 0x30C3, Class::NonStarter),
    (0x30E3, 0x30E3, Class::NonStarter),
    (0x30E5, 0x30E5, Class::NonStarter),
    (0x30E7, 0x30E7, Class::NonStarter),
    (0x30EE, 0x30EE, Class::NonStarter),
    (0x30F5, 0x30F6, Class::NonStarter),
    (0x30FC, 0x30FE, Class::NonStarter),
    // The ideographs and the syllabaries themselves, which break between.
    (0x1100, 0x11FF, Class::Ideograph),
    (0x2E80, 0x303F, Class::Ideograph),
    (0x3040, 0x30FF, Class::Ideograph),
    (0x3400, 0x4DBF, Class::Ideograph),
    (0x4E00, 0x9FFF, Class::Ideograph),
    (0xA960, 0xA97F, Class::Ideograph),
    (0xAC00, 0xD7FF, Class::Ideograph),
    (0xF900, 0xFAFF, Class::Ideograph),
    (0xFF00, 0xFF60, Class::Ideograph),
    (0x20000, 0x3FFFF, Class::Ideograph),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_word_is_never_broken_inside() {
        assert!(!may_break('e', 'l'));
        assert!(!may_break('a', '1'));
    }

    #[test]
    fn a_line_breaks_after_a_space_and_not_before_one() {
        assert!(may_break(' ', 'w'));
        assert!(!may_break('o', ' '));
        // Two spaces stay together with the line they end.
        assert!(!may_break(' ', ' '));
    }

    #[test]
    fn a_non_breaking_space_does_not_break() {
        assert!(!may_break('\u{00A0}', 'w'));
        assert!(!may_break('o', '\u{00A0}'));
    }

    #[test]
    fn a_hyphenated_word_breaks_after_the_hyphen() {
        assert!(may_break('-', 'k'));
        assert!(!may_break('l', '-'), "a line may not begin with a hyphen");
    }

    #[test]
    fn a_line_never_begins_with_the_punctuation_that_ends_a_sentence() {
        for mark in ['.', ',', ';', ':', '!', '?', ')', ']', '}'] {
            assert!(!may_break('d', mark), "a line could begin with {mark}");
        }
    }

    #[test]
    fn a_line_never_ends_with_an_opening_bracket() {
        for bracket in ['(', '[', '{'] {
            assert!(!may_break(bracket, 'a'), "a line could end with {bracket}");
        }
    }

    #[test]
    fn a_number_holds_together_with_its_signs() {
        assert!(!may_break('$', '1'));
        assert!(!may_break('5', '%'));
        assert!(!may_break('1', '5'));
        assert!(!may_break('2', '0'));
    }

    #[test]
    fn text_without_spaces_breaks_between_its_characters() {
        // Japanese: a line may be broken between two ideographs.
        assert!(may_break('日', '本'));
        assert!(may_break('あ', 'い'));
    }

    #[test]
    fn japanese_punctuation_never_begins_a_line() {
        assert!(!may_break('本', '。'), "a line could begin with a full stop");
        assert!(!may_break('本', '、'), "a line could begin with a comma");
        assert!(!may_break('本', '」'), "a line could begin with a closing bracket");
        assert!(!may_break('「', '本'), "a line could end with an opening bracket");
    }

    #[test]
    fn a_small_kana_stays_with_the_syllable_before_it() {
        assert!(!may_break('き', 'ょ'), "the small kana was left to begin a line");
        assert!(!may_break('ラ', 'ー'), "the sound mark was left to begin a line");
    }

    #[test]
    fn the_places_a_line_may_break_are_found_in_order() {
        let text = "one two three";
        assert_eq!(opportunities(text), vec![4, 8]);
    }

    #[test]
    fn a_line_break_in_the_text_is_a_break_wherever_it_falls() {
        assert!(may_break('\n', 'a'));
        assert!(!may_break('a', '\n'), "nothing is broken away from the break itself");
    }
}
