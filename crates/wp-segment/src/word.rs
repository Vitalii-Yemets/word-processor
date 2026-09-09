//! Where one word ends and the next begins.
//!
//! Splitting on spaces gets `don't` wrong, and `3.14`, and `1,000`: the
//! apostrophe, the full stop and the comma are inside those words, not between
//! them. Double-clicking any of them in Word selects the whole thing, and so it
//! must here.
//!
//! The rules are [UAX #29]'s: each character has a class, and a pair of classes
//! — sometimes with a look at the one beyond — says whether a word ends there.
//!
//! # Where this parts company with the standard
//!
//! Chinese and Japanese are written without spaces, and the standard leaves
//! every ideograph a word of its own, which is not what a reader sees and not
//! what Word does: Word has a dictionary of the language and finds the words in
//! it. Until there is one here, a run of ideographs is kept together — too
//! much, where the standard gives too little, but predictable either way.
//! Katakana, which is used for whole words at a time, does separate.
//!
//! [UAX #29]: https://www.unicode.org/reports/tr29/

use crate::grapheme;

/// What a character does to the boundary beside it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
    /// Punctuation, symbols, and everything else that is a word of its own.
    Other,
    CarriageReturn,
    LineFeed,
    /// A line separator other than those two.
    Newline,
    /// A mark that belongs to the character before it, and a formatting
    /// character that draws nothing. Both are passed over as if they were not
    /// there.
    Ignored,
    /// The zero width joiner, which is passed over too but may hold two emoji
    /// together.
    Joiner,
    /// Half of a flag.
    Regional,
    /// A space that separates words and is a word of its own.
    Space,
    /// A letter, in any alphabet.
    Letter,
    /// Hebrew, which has rules of its own about quotation marks.
    Hebrew,
    Katakana,
    Numeric,
    /// `'`, which may be inside a word — `don't` — or around one.
    SingleQuote,
    /// `"`, which is inside a word only in Hebrew.
    DoubleQuote,
    /// A character that joins two letters: the colon of a Swedish abbreviation,
    /// the middle dot of Catalan.
    MidLetter,
    /// One that joins two numbers: the comma of `1,000`.
    MidNumber,
    /// One that does either: the full stop of `3.14` and of `e.g.`.
    MidBoth,
    /// The underscore, which joins whatever is either side of it.
    Connector,
    /// An emoji.
    Pictographic,
}

/// A character that counts, and the ignorable characters hanging from it.
struct Piece {
    offset: usize,
    class: Class,
    /// Whether the ignorable characters after it end in a joiner, which is what
    /// lets the next emoji join on.
    joins: bool,
}

/// Every place in the text where one word ends and the next begins.
///
/// The ends count, so the boundaries chop the text into words, punctuation and
/// gaps with nothing left over.
#[must_use]
pub fn boundaries(text: &str) -> Vec<usize> {
    let pieces = pieces(text);
    let mut out = vec![0];
    for index in 1..pieces.len() {
        if breaks(&pieces, index) {
            out.push(pieces[index].offset);
        }
    }
    if !text.is_empty() {
        out.push(text.len());
    }
    out
}

/// The stretch of text the word at an offset covers.
///
/// An offset inside a word gives that word; one between two gives the word
/// after it, because that is where a caret sits when it is at the start of
/// something.
#[must_use]
pub fn at(text: &str, offset: usize) -> core::ops::Range<usize> {
    let offset = offset.min(text.len());
    let bounds = boundaries(text);
    // The end first, so that the caret at the very end of the text belongs to
    // the last word rather than to nothing.
    let end = bounds.iter().find(|at| **at > offset).copied().unwrap_or(text.len());
    let start = bounds.iter().rev().find(|at| **at < end).copied().unwrap_or(0);
    start..end
}

/// Whether the word ends before the piece at an index.
///
/// The standard's rules, in its order; the first that speaks decides, and the
/// last says that anything else is a boundary.
#[allow(clippy::match_same_arms)]
fn breaks(pieces: &[Piece], index: usize) -> bool {
    use Class::{
        CarriageReturn, Connector, DoubleQuote, Hebrew, Katakana, LineFeed, MidBoth, MidLetter,
        MidNumber, Newline, Numeric, Pictographic, Regional, SingleQuote, Space,
    };

    let before = pieces[index - 1].class;
    let after = pieces[index].class;
    let two_back = if index >= 2 { Some(pieces[index - 2].class) } else { None };
    let beyond = pieces.get(index + 1).map(|piece| piece.class);

    // A letter for the purpose of the rules below: Hebrew is a letter that has
    // extra rules, not a different thing.
    let letter = |class: Class| matches!(class, Class::Letter | Hebrew);
    // The punctuation that may sit inside a word, and inside a number.
    let in_word = |class: Class| matches!(class, MidLetter | MidBoth | SingleQuote);
    let in_number = |class: Class| matches!(class, MidNumber | MidBoth | SingleQuote);

    match (before, after) {
        // WB3 to WB3b: a line ending holds together and breaks from everything.
        (CarriageReturn, LineFeed) => false,
        (CarriageReturn | LineFeed | Newline, _) | (_, CarriageReturn | LineFeed | Newline) => true,
        // WB3c: a joiner holds an emoji to what it was joined to.
        (_, Pictographic) if pieces[index - 1].joins => false,
        // WB3d: a run of spaces is one gap, not several.
        (Space, Space) => false,
        // WB5: two letters are the inside of a word.
        _ if letter(before) && letter(after) => false,
        // WB6 and WB7: punctuation between two letters belongs to the word —
        // the apostrophe of "don't", the colon of an abbreviation.
        _ if letter(before) && in_word(after) && beyond.is_some_and(letter) => false,
        _ if in_word(before) && letter(after) && two_back.is_some_and(letter) => false,
        // WB7a to WB7c: the Hebrew quotation marks.
        (Hebrew, SingleQuote) => false,
        (Hebrew, DoubleQuote) if beyond == Some(Hebrew) => false,
        (DoubleQuote, Hebrew) if two_back == Some(Hebrew) => false,
        // WB8 to WB10: a number is a word, and a word may hold digits.
        (Numeric, Numeric) => false,
        _ if letter(before) && after == Numeric => false,
        _ if before == Numeric && letter(after) => false,
        // WB11 and WB12: punctuation between two digits belongs to the number
        // — the point of "3.14", the comma of "1,000".
        _ if in_number(before) && after == Numeric && two_back == Some(Numeric) => false,
        _ if before == Numeric && in_number(after) && beyond == Some(Numeric) => false,
        // WB13: katakana holds together, which is how Japanese marks the words
        // it borrows.
        (Katakana, Katakana) => false,
        // WB13a and WB13b: the underscore joins whatever is either side of it.
        _ if before == Connector && (letter(after) || matches!(after, Numeric | Katakana)) => false,
        _ if after == Connector
            && (letter(before) || matches!(before, Numeric | Katakana | Connector)) =>
        {
            false
        }
        // WB15 and WB16: two regional indicators make a flag.
        (Regional, Regional) => flag_run(pieces, index) % 2 == 0,
        // WB999.
        _ => true,
    }
}

/// How many regional indicators run up to a piece without a break.
fn flag_run(pieces: &[Piece], index: usize) -> usize {
    pieces[..index].iter().rev().take_while(|piece| piece.class == Class::Regional).count()
}

/// The text as the characters that count, each carrying the ignorable ones
/// that follow it.
///
/// This is WB4: a mark or a formatting character never ends a word, and never
/// changes what the character it hangs from does. Taking them out here means
/// every rule below can be written as though they did not exist.
fn pieces(text: &str) -> Vec<Piece> {
    let mut out: Vec<Piece> = Vec::new();
    for (offset, character) in text.char_indices() {
        let class = class_of(character);
        let ignorable = matches!(class, Class::Ignored | Class::Joiner);
        let after_line_ending = out.last().is_some_and(|piece| {
            matches!(piece.class, Class::CarriageReturn | Class::LineFeed | Class::Newline)
        });

        if ignorable && !out.is_empty() && !after_line_ending {
            if let Some(piece) = out.last_mut() {
                piece.joins = class == Class::Joiner;
            }
            continue;
        }
        out.push(Piece { offset, class, joins: false });
    }
    out
}

/// The class of one character.
fn class_of(character: char) -> Class {
    let code = character as u32;
    match code {
        0x000D => return Class::CarriageReturn,
        0x000A => return Class::LineFeed,
        0x000B | 0x000C | 0x0085 | 0x2028 | 0x2029 => return Class::Newline,
        0x200D => return Class::Joiner,
        0x0022 => return Class::DoubleQuote,
        0x0027 => return Class::SingleQuote,
        0x1F1E6..=0x1F1FF => return Class::Regional,
        _ => {}
    }

    for (first, last, class) in RANGES {
        if code >= *first && code <= *last {
            return *class;
        }
    }

    if grapheme::is_mark(character) {
        Class::Ignored
    } else if character.is_whitespace() {
        Class::Space
    } else if character.is_numeric() {
        Class::Numeric
    } else if character.is_alphabetic() {
        Class::Letter
    } else if grapheme::is_pictographic(character) {
        Class::Pictographic
    } else {
        Class::Other
    }
}

/// The characters whose class is not the one their category would give.
const RANGES: &[(u32, u32, Class)] = &[
    // The characters that draw nothing and are read over as if they were not
    // there: the soft hyphen, the direction marks, the byte order mark.
    (0x00AD, 0x00AD, Class::Ignored),
    (0x061C, 0x061C, Class::Ignored),
    (0x180E, 0x180E, Class::Ignored),
    (0x200B, 0x200C, Class::Ignored),
    (0x200E, 0x200F, Class::Ignored),
    (0x202A, 0x202E, Class::Ignored),
    (0x2060, 0x2064, Class::Ignored),
    (0x2066, 0x206F, Class::Ignored),
    (0xFEFF, 0xFEFF, Class::Ignored),
    (0xFFF9, 0xFFFB, Class::Ignored),
    // The punctuation that can sit inside a word or a number.
    (0x002C, 0x002C, Class::MidNumber),
    (0x002E, 0x002E, Class::MidBoth),
    (0x003A, 0x003A, Class::MidLetter),
    (0x003B, 0x003B, Class::MidNumber),
    (0x005F, 0x005F, Class::Connector),
    (0x00B7, 0x00B7, Class::MidLetter),
    (0x037E, 0x037E, Class::MidNumber),
    (0x0387, 0x0387, Class::MidLetter),
    (0x0589, 0x0589, Class::MidNumber),
    (0x055F, 0x055F, Class::MidLetter),
    (0x05F4, 0x05F4, Class::MidLetter),
    (0x060C, 0x060D, Class::MidNumber),
    (0x066C, 0x066C, Class::MidNumber),
    (0x07F8, 0x07F8, Class::MidNumber),
    (0x2018, 0x2019, Class::MidBoth),
    (0x2024, 0x2024, Class::MidBoth),
    (0x2027, 0x2027, Class::MidLetter),
    (0x203F, 0x2040, Class::Connector),
    (0x2044, 0x2044, Class::MidNumber),
    (0x2054, 0x2054, Class::Connector),
    (0xFE10, 0xFE10, Class::MidNumber),
    (0xFE13, 0xFE13, Class::MidLetter),
    (0xFE14, 0xFE14, Class::MidNumber),
    (0xFE33, 0xFE34, Class::Connector),
    (0xFE4D, 0xFE4F, Class::Connector),
    (0xFE50, 0xFE50, Class::MidNumber),
    (0xFE52, 0xFE52, Class::MidBoth),
    (0xFE54, 0xFE54, Class::MidNumber),
    (0xFE55, 0xFE55, Class::MidLetter),
    (0xFF07, 0xFF07, Class::MidBoth),
    (0xFF0C, 0xFF0C, Class::MidNumber),
    (0xFF0E, 0xFF0E, Class::MidBoth),
    (0xFF1A, 0xFF1A, Class::MidLetter),
    (0xFF1B, 0xFF1B, Class::MidNumber),
    (0xFF3F, 0xFF3F, Class::Connector),
    // Hebrew, which the quotation mark rules are about.
    (0x05D0, 0x05F2, Class::Hebrew),
    (0xFB1D, 0xFB4F, Class::Hebrew),
    // Katakana, which is written in words even where the rest is not.
    (0x30A1, 0x30FA, Class::Katakana),
    (0x30FC, 0x30FF, Class::Katakana),
    (0x31F0, 0x31FF, Class::Katakana),
    (0x32D0, 0x32FE, Class::Katakana),
    (0x3300, 0x3357, Class::Katakana),
    (0xFF66, 0xFF9D, Class::Katakana),
    // A space that separates words. A non-breaking space is not one: it is
    // there precisely to hold two words together.
    (0x0020, 0x0020, Class::Space),
    (0x00A0, 0x00A0, Class::Other),
    (0x2007, 0x2007, Class::Other),
    (0x202F, 0x202F, Class::Other),
];

#[cfg(test)]
mod tests {
    use super::{at, boundaries};

    /// The text chopped into the words the rules find in it.
    fn words(text: &str) -> Vec<&str> {
        boundaries(text).windows(2).map(|pair| &text[pair[0]..pair[1]]).collect()
    }

    #[test]
    fn a_sentence_comes_apart_into_words_spaces_and_punctuation() {
        assert_eq!(words("one two"), ["one", " ", "two"]);
        assert_eq!(words("hello, world"), ["hello", ",", " ", "world"]);
    }

    #[test]
    fn an_apostrophe_inside_a_word_belongs_to_it() {
        assert_eq!(words("don't"), ["don't"]);
        assert_eq!(words("don\u{2019}t"), ["don\u{2019}t"]);
    }

    #[test]
    fn a_quotation_mark_around_a_word_does_not() {
        assert_eq!(words("'yes'"), ["'", "yes", "'"]);
    }

    #[test]
    fn a_number_keeps_its_point_and_its_comma() {
        assert_eq!(words("3.14"), ["3.14"]);
        assert_eq!(words("1,000"), ["1,000"]);
        // But a full stop that ends a sentence is not part of the number.
        assert_eq!(words("It is 3."), ["It", " ", "is", " ", "3", "."]);
    }

    #[test]
    fn a_word_may_hold_digits() {
        assert_eq!(words("mp3"), ["mp3"]);
        assert_eq!(words("3d"), ["3d"]);
    }

    #[test]
    fn an_underscore_joins_what_it_sits_between() {
        assert_eq!(words("some_name"), ["some_name"]);
    }

    #[test]
    fn a_hyphen_does_not() {
        // Two words with a hyphen between them, as Word treats them.
        assert_eq!(words("well-known"), ["well", "-", "known"]);
    }

    #[test]
    fn a_run_of_spaces_is_one_gap() {
        assert_eq!(words("a   b"), ["a", "   ", "b"]);
    }

    #[test]
    fn words_are_words_in_every_alphabet() {
        assert_eq!(words("одно два"), ["одно", " ", "два"]);
        assert_eq!(words("ένα δύο"), ["ένα", " ", "δύο"]);
    }

    #[test]
    fn an_emoji_joined_to_another_is_one_word() {
        let family = "\u{1F468}\u{200D}\u{1F469}";
        assert_eq!(words(family).len(), 1);
    }

    #[test]
    fn a_flag_is_one_word() {
        assert_eq!(words("\u{1F1FA}\u{1F1E6}").len(), 1);
    }

    #[test]
    fn the_word_at_a_point_is_the_one_the_caret_is_in() {
        let text = "one two three";
        assert_eq!(at(text, 0), 0..3);
        assert_eq!(at(text, 2), 0..3);
        assert_eq!(at(text, 4), 4..7);
        assert_eq!(at(text, 6), 4..7);
        assert_eq!(at(text, 13), 8..13, "the end of the text is inside the last word");
    }

    #[test]
    fn an_empty_paragraph_has_no_words_and_does_not_panic() {
        assert!(words("").is_empty());
        assert_eq!(at("", 0), 0..0);
    }
}
