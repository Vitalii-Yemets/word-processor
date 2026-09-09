//! Finding a piece of text in a paragraph, the way a person means it.
//!
//! # Not a substring search
//!
//! Looking for `café` with `str::find` misses four things a reader would not:
//!
//! - **Capitals.** Word's Find does not mind them unless it is told to, so
//!   `word` finds `Word`. Lowering both sides is the obvious fix and it has a
//!   trap in it: `İ` lowers to two characters, so every offset after it in the
//!   lowered text is wrong, and the match is reported in the wrong place. What
//!   is kept here is not the lowered text alone but where each piece of it came
//!   from.
//! - **Accents written apart.** `é` is one character on Windows and two on a
//!   Mac — an `e` and a mark drawn on it. They are the same word. See
//!   [`wp_normal`].
//! - **Whole words.** Word can be asked to find `cat` only where it is a word
//!   and not inside `catalogue`. Where a word begins and ends is
//!   [`wp_segment`]'s business.
//! - **Where the match really is.** A match found in the folded text has to be
//!   given back as a stretch of the original, or the selection lands somewhere
//!   else than the text that was found.

use core::ops::Range;

/// How a search treats the text it looks through.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Matching {
    /// Whether a capital has to be matched by a capital. Off by default, as it
    /// is in Word.
    pub match_case: bool,
    /// Whether a match has to be a whole word: `cat` and not the `cat` in
    /// `catalogue`.
    pub whole_word: bool,
}

impl Matching {
    /// A search that minds about capitals, which is what a program comparing
    /// text rather than a person reading it wants.
    #[must_use]
    pub fn exact() -> Self {
        Self { match_case: true, whole_word: false }
    }
}

/// Every place the needle appears in the text, in order, as stretches of the
/// text itself.
#[must_use]
pub fn matches(text: &str, needle: &str, how: Matching) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    if needle.is_empty() || text.is_empty() {
        return out;
    }

    let haystack = fold(text, how.match_case);
    let wanted = fold(needle, how.match_case).text;
    if wanted.is_empty() {
        return out;
    }

    // Where the words are, but only when the answer depends on it: finding
    // them costs a walk of the paragraph.
    let bounds = how.whole_word.then(|| wp_segment::word_boundaries(text));

    let mut from = 0;
    while let Some(found) = haystack.text.get(from..).and_then(|rest| rest.find(&wanted)) {
        let at = from + found;
        let range = haystack.starts[at]..haystack.ends[at + wanted.len()];
        let whole = bounds
            .as_ref()
            .is_none_or(|bounds| bounds.contains(&range.start) && bounds.contains(&range.end));
        if whole {
            out.push(range);
        }
        from = at + wanted.len();
    }
    out
}

/// The text as it is compared, and where every byte of it came from.
struct Folded {
    /// The text with the accents joined on and, unless capitals matter, in
    /// lower case.
    text: String,
    /// For each byte of it, where the character it belongs to starts in the
    /// original. One longer than the text, so a match that ends at the end has
    /// somewhere to point.
    starts: Vec<usize>,
    /// For each byte of it, where the character before it ends in the original.
    ends: Vec<usize>,
}

/// Puts text into the form it is compared in, keeping track of where every
/// piece of it came from.
fn fold(text: &str, match_case: bool) -> Folded {
    let mut folded =
        Folded { text: String::with_capacity(text.len()), starts: Vec::new(), ends: vec![0] };

    for (start, end, character) in joined(text) {
        let before = folded.text.len();
        if match_case {
            folded.text.push(character);
        } else {
            folded.text.extend(character.to_lowercase());
        }
        for _ in before..folded.text.len() {
            folded.starts.push(start);
            folded.ends.push(end);
        }
    }

    folded.starts.push(text.len());
    folded
}

/// The text as characters, with every mark joined onto the letter it is drawn
/// on where Unicode has a single character for the pair.
///
/// Each comes back with where it starts and ends in the original, because that
/// is what a match has to be reported in terms of.
fn joined(text: &str) -> Vec<(usize, usize, char)> {
    let mut out: Vec<(usize, usize, char)> = Vec::with_capacity(text.len());

    for (at, character) in text.char_indices() {
        let end = at + character.len_utf8();
        if wp_normal::combining_class(character) != 0 {
            if let Some(last) = out.last_mut() {
                if let Some(joined) = wp_normal::composed(last.2, character) {
                    last.1 = end;
                    last.2 = joined;
                    continue;
                }
            }
        }
        out.push((at, end, character));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{matches, Matching};

    /// The pieces of text a search finds, as text.
    fn found<'a>(text: &'a str, needle: &str, how: Matching) -> Vec<&'a str> {
        matches(text, needle, how).into_iter().map(|range| &text[range]).collect()
    }

    #[test]
    fn a_search_finds_what_is_there() {
        assert_eq!(found("one two one", "one", Matching::exact()), ["one", "one"]);
        assert_eq!(found("one two", "three", Matching::exact()), Vec::<&str>::new());
    }

    #[test]
    fn capitals_do_not_matter_unless_they_are_asked_to() {
        assert_eq!(found("Word and word", "word", Matching::default()), ["Word", "word"]);
        assert_eq!(found("Word and word", "word", Matching::exact()), ["word"]);
    }

    #[test]
    fn a_match_is_reported_where_it_really_is() {
        // The Turkish capital I lowers to two characters, so everything after
        // it sits at a different offset in the lowered text than in the real
        // one. The match must still be given back in the real one.
        let text = "\u{0130}stanbul is a city";
        let ranges = matches(text, "city", Matching::default());
        assert_eq!(ranges.len(), 1);
        assert_eq!(&text[ranges[0].clone()], "city");
    }

    #[test]
    fn an_accent_written_either_way_is_the_same_word() {
        let apart = "a cafe\u{0301} in town";
        assert_eq!(found(apart, "café", Matching::default()), ["cafe\u{0301}"]);

        let together = "a café in town";
        assert_eq!(found(together, "cafe\u{0301}", Matching::default()), ["café"]);
    }

    #[test]
    fn a_whole_word_search_does_not_match_inside_one() {
        let text = "a cat in a catalogue";
        let whole = Matching { whole_word: true, ..Matching::default() };
        assert_eq!(found(text, "cat", whole), ["cat"]);
        assert_eq!(found(text, "cat", Matching::default()), ["cat", "cat"]);
    }

    #[test]
    fn a_whole_word_search_still_finds_a_word_against_punctuation() {
        let text = "(cat), cat.";
        let whole = Matching { whole_word: true, ..Matching::default() };
        assert_eq!(found(text, "cat", whole).len(), 2);
    }

    #[test]
    fn matches_do_not_overlap() {
        assert_eq!(found("aaaa", "aa", Matching::exact()), ["aa", "aa"]);
    }

    #[test]
    fn an_empty_needle_finds_nothing_rather_than_everything() {
        assert!(matches("some text", "", Matching::default()).is_empty());
        assert!(matches("", "text", Matching::default()).is_empty());
    }

    #[test]
    fn the_search_works_in_every_alphabet() {
        assert_eq!(found("Привет мир", "привет", Matching::default()), ["Привет"]);
        assert_eq!(found("ΑΘΗΝΑ", "αθηνα", Matching::default()), ["ΑΘΗΝΑ"]);
    }
}
