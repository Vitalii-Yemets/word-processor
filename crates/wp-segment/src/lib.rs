//! Where one thing ends and the next begins.
//!
//! Two questions the editor asks constantly and cannot answer by counting
//! bytes or even code points:
//!
//! - **Where does a character end?** Left arrow must step over `é` whether it
//!   was written as one code point or as `e` and an accent, and Backspace must
//!   take a whole emoji rather than the skin tone off a waving hand.
//! - **Where does a word end?** Double-clicking `don't` selects `don't`, and
//!   `3.14` selects `3.14`: the apostrophe and the point are inside those
//!   words. `Ctrl+Right` walks by the same rule.
//!
//! Both are [UAX #29], which calls the first grapheme cluster boundaries and
//! the second word boundaries.
//!
//! [UAX #29]: https://www.unicode.org/reports/tr29/
//!
//! # Example
//!
//! ```
//! // A letter and the accent drawn on it are one character to the caret.
//! assert_eq!(wp_segment::next_character("e\u{0301}f", 0), 3);
//! // And an apostrophe inside a word belongs to it.
//! assert_eq!(wp_segment::word_at("don't stop", 2), 0..5);
//! ```

#![forbid(unsafe_code)]

mod grapheme;
mod tables;
mod word;

/// Where the character after an offset begins.
///
/// A character means the one a reader counts, not a code point: the caret never
/// stops between a letter and the accent drawn on it.
#[must_use]
pub fn next_character(text: &str, offset: usize) -> usize {
    grapheme::next(text, offset)
}

/// Where the character before an offset begins.
#[must_use]
pub fn previous_character(text: &str, offset: usize) -> usize {
    grapheme::previous(text, offset)
}

/// Every place in the text where one character ends and the next begins,
/// including both ends of it.
#[must_use]
pub fn character_boundaries(text: &str) -> Vec<usize> {
    grapheme::boundaries(text)
}

/// The stretch of text the word at an offset covers.
#[must_use]
pub fn word_at(text: &str, offset: usize) -> core::ops::Range<usize> {
    word::at(text, offset)
}

/// Every place in the text where one word ends and the next begins, including
/// both ends of it.
///
/// The pieces between them are words, punctuation and gaps: everything is in
/// one of them, so the boundaries can be walked in either direction without
/// anything falling between.
/// How many words a piece of text holds.
///
/// # Why this is not "count the spaces"
///
/// Because half the world writes without them. A Japanese sentence has no
/// spaces in it at all, and counting its gaps gives one — which is the number
/// this program used to show for a page of Japanese. What is counted here is
/// what the segmentation says a word is, which for a script written without
/// spaces is every character and for a katakana word is the word.
///
/// That is the same rule Word counts by, and it is why a Japanese document
/// shows a far larger count than an English one of the same length: the
/// characters are being counted, because without a dictionary of the language
/// nothing else can be.
///
/// Punctuation and spaces are not words: a piece has to hold a letter or a
/// digit to be counted.
#[must_use]
pub fn count_words(text: &str) -> usize {
    let bounds = word_boundaries(text);
    bounds
        .windows(2)
        .filter(|pair| text[pair[0]..pair[1]].chars().any(char::is_alphanumeric))
        .count()
}

#[must_use]
pub fn word_boundaries(text: &str) -> Vec<usize> {
    word::boundaries(text)
}

#[cfg(test)]
mod counting {
    use super::count_words;

    #[test]
    fn a_sentence_of_five_words_is_five() {
        assert_eq!(count_words("A sentence of five words"), 5);
    }

    #[test]
    fn punctuation_and_gaps_are_not_words() {
        assert_eq!(count_words("Hello, world!  ---  "), 2);
        assert_eq!(count_words("   "), 0);
        assert_eq!(count_words(""), 0);
    }

    #[test]
    fn a_number_is_a_word_and_keeps_its_point() {
        assert_eq!(count_words("3.14 is pi"), 3);
        assert_eq!(count_words("1,000 and 2,000"), 3);
    }

    #[test]
    fn a_japanese_sentence_is_not_one_word() {
        // Which is what counting the gaps said, because there are none. Every
        // character is counted, and the katakana word is counted once.
        let glass = "\u{79C1}\u{306F}\u{30AC}\u{30E9}\u{30B9}\u{3092}\u{98DF}\u{3079}\u{3089}\u{308C}\u{307E}\u{3059}";
        assert_eq!(count_words(glass), 10);
    }

    #[test]
    fn a_line_of_chinese_counts_its_characters() {
        assert_eq!(count_words("\u{6211}\u{80FD}\u{541E}\u{4E0B}\u{73BB}\u{7483}"), 6);
    }

    #[test]
    fn english_and_japanese_in_one_line_each_count_their_own_way() {
        let mixed = "Rust \u{3067}\u{66F8}\u{304F}";
        assert_eq!(count_words(mixed), 4, "one Latin word and three characters");
    }

    #[test]
    fn a_hyphenated_word_is_two_the_way_word_counts_them() {
        // Word counts "well-known" as two, because the hyphen is a break
        // between words rather than a letter inside one.
        assert_eq!(count_words("well-known"), 2);
    }
}

/// What the generated tables added: the scripts nobody listed by hand, which
/// used to fall through to "an ordinary character standing on its own".
#[cfg(test)]
mod coverage {
    use crate::{character_boundaries, word_boundaries};

    /// The characters of a string, as the pieces a caret steps over.
    fn characters(text: &str) -> Vec<&str> {
        let bounds = character_boundaries(text);
        bounds.windows(2).map(|pair| &text[pair[0]..pair[1]]).collect()
    }

    fn words(text: &str) -> Vec<&str> {
        let bounds = word_boundaries(text);
        bounds.windows(2).map(|pair| &text[pair[0]..pair[1]]).collect()
    }

    #[test]
    fn every_character_there_is_has_both_classes() {
        for code in 0..=0x10FFFFu32 {
            if let Some(character) = char::from_u32(code) {
                let _ = crate::tables::grapheme_class_of(character);
                let _ = crate::tables::word_class_of(character);
            }
        }
        assert!(crate::tables::in_order(), "a table is out of order");
    }

    #[test]
    fn a_vowel_sign_belongs_to_its_consonant_in_every_script() {
        // Devanagari was written down by hand. Telugu, Khmer and Balinese were
        // not, and a caret that stepped between a consonant and its vowel sign
        // in any of them would land in the middle of a letter.
        assert_eq!(characters("\u{0C15}\u{0C3F}").len(), 1, "Telugu");
        assert_eq!(characters("\u{1780}\u{17B6}").len(), 1, "Khmer");
        assert_eq!(characters("\u{1B33}\u{1B35}").len(), 1, "Balinese");
    }

    #[test]
    fn a_letter_of_a_script_nobody_listed_is_still_a_letter() {
        // Cherokee, Tifinagh and Osage are alphabets; a word written in one is
        // a word, not a row of separate characters.
        assert_eq!(words("\u{13A0}\u{13A1}\u{13A2}"), vec!["\u{13A0}\u{13A1}\u{13A2}"]);
        assert_eq!(words("\u{2D30}\u{2D31}"), vec!["\u{2D30}\u{2D31}"]);
        assert_eq!(words("\u{104B0}\u{104B1}"), vec!["\u{104B0}\u{104B1}"]);
    }

    #[test]
    fn the_digits_of_every_script_are_numbers() {
        // Which matters because a number holds together across a full stop and
        // a comma, and a letter does not.
        assert_eq!(words("\u{0967}.\u{0968}"), vec!["\u{0967}.\u{0968}"], "Devanagari");
        assert_eq!(words("\u{0669}.\u{0660}"), vec!["\u{0669}.\u{0660}"], "Arabic-Indic");
    }
}
