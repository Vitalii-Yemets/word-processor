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
#[must_use]
pub fn word_boundaries(text: &str) -> Vec<usize> {
    word::boundaries(text)
}
