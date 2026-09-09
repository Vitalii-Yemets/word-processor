//! Moving and deleting by the word rather than by the character.
//!
//! # What counts as a word
//!
//! [`wp_segment`] answers that: it holds the Unicode rules, which know that the
//! apostrophe of `don't` and the point of `3.14` are inside the word and the
//! hyphen of `well-known` is between two.
//!
//! What is left here is what an editor does with the answer. `Ctrl+Right` in
//! `hello, world` steps over `hello`, then over `, `, and lands on `world`: a
//! run of whitespace is crossed rather than stopped in, because nobody wants
//! the caret to stop in a gap. `Ctrl+Left` does the same going backwards, so
//! pressing it at the end of `one two ` lands at the `t` of `two`, not in the
//! space after it.

/// Where the next word begins, going forwards from an offset.
///
/// The rest of whatever is under the caret is stepped over, then the whitespace
/// after it — so the caret lands on the first character of the next word rather
/// than in the gap before it.
#[must_use]
pub fn next_word(text: &str, offset: usize) -> usize {
    let offset = offset.min(text.len());
    let bounds = wp_segment::word_boundaries(text);
    for (index, start) in bounds.iter().enumerate() {
        if *start <= offset {
            continue;
        }
        match bounds.get(index + 1) {
            // A gap is crossed rather than stopped in.
            Some(end) if is_gap(&text[*start..*end]) => continue,
            _ => return *start,
        }
    }
    text.len()
}

/// Where the word under the caret begins, going backwards from an offset.
#[must_use]
pub fn previous_word(text: &str, offset: usize) -> usize {
    let offset = offset.min(text.len());
    let bounds = wp_segment::word_boundaries(text);
    for (index, start) in bounds.iter().enumerate().rev() {
        if *start >= offset {
            continue;
        }
        match bounds.get(index + 1) {
            Some(end) if is_gap(&text[*start..*end]) => continue,
            _ => return *start,
        }
    }
    0
}

/// Whether a piece of text is a gap between words rather than one of them.
fn is_gap(piece: &str) -> bool {
    piece.chars().all(char::is_whitespace)
}

#[cfg(test)]
mod tests {
    use super::{next_word, previous_word};

    #[test]
    fn forwards_over_a_word_and_the_space_after_it() {
        assert_eq!(next_word("one two three", 0), 4);
        assert_eq!(next_word("one two three", 4), 8);
    }

    #[test]
    fn forwards_from_the_middle_of_a_word_finishes_it_first() {
        assert_eq!(next_word("one two", 1), 4);
    }

    #[test]
    fn forwards_stops_at_the_end() {
        assert_eq!(next_word("one two", 4), 7);
        assert_eq!(next_word("one two", 7), 7);
        assert_eq!(next_word("one two   ", 4), 10);
    }

    #[test]
    fn punctuation_is_a_word_of_its_own() {
        // "hello, world": over "hello", then over ", ", then at "world".
        assert_eq!(next_word("hello, world", 0), 5);
        assert_eq!(next_word("hello, world", 5), 7);
    }

    #[test]
    fn forwards_from_inside_a_gap_only_crosses_the_gap() {
        assert_eq!(next_word("one   two", 4), 6);
    }

    #[test]
    fn backwards_to_the_start_of_the_word_before() {
        assert_eq!(previous_word("one two three", 13), 8);
        assert_eq!(previous_word("one two three", 8), 4);
        assert_eq!(previous_word("one two three", 4), 0);
    }

    #[test]
    fn backwards_from_the_middle_of_a_word_reaches_its_start() {
        assert_eq!(previous_word("one two", 5), 4);
    }

    #[test]
    fn backwards_crosses_the_gap_before_the_word() {
        assert_eq!(previous_word("one   two", 6), 0);
    }

    #[test]
    fn backwards_stops_at_the_beginning() {
        assert_eq!(previous_word("one", 0), 0);
        assert_eq!(previous_word("   one", 3), 0);
    }

    #[test]
    fn backwards_treats_punctuation_as_its_own_word() {
        assert_eq!(previous_word("hello, world", 7), 5);
        assert_eq!(previous_word("hello, world", 5), 0);
    }

    #[test]
    fn words_are_words_in_every_alphabet() {
        // Cyrillic, and a letter that is two bytes long in each step.
        assert_eq!(next_word("одно два", 0), 9);
        assert_eq!(previous_word("одно два", 16), 9);
        // Greek, to be sure it is not one alphabet that was special-cased.
        assert_eq!(next_word("ένα δύο", 0), 7);
    }

    #[test]
    fn a_word_with_an_apostrophe_in_it_is_crossed_in_one_step() {
        // Word does this: Ctrl+Right over "don't" lands on "stop", not on the
        // apostrophe and then the t.
        assert_eq!(next_word("don't stop", 0), 6);
        assert_eq!(previous_word("don't stop", 6), 0);
    }

    #[test]
    fn a_number_is_crossed_in_one_step_however_it_is_written() {
        assert_eq!(next_word("3.14 is pi", 0), 5);
        assert_eq!(next_word("1,000 apples", 0), 6);
    }

    #[test]
    fn an_offset_past_the_end_is_clamped_rather_than_panicking() {
        assert_eq!(next_word("one", 99), 3);
        assert_eq!(previous_word("one", 99), 0);
    }

    #[test]
    fn an_offset_inside_a_character_gives_an_answer_rather_than_panicking() {
        // Byte 1 is the middle of "о", which no caret should ever be at — but
        // a wrong answer is better than a crash if one ever is.
        assert_eq!(next_word("одно", 1), 8);
        assert_eq!(previous_word("одно", 1), 0);
    }
}
