//! Moving and deleting by the word rather than by the character.
//!
//! # What counts as a word
//!
//! Three kinds of character, and a word is a run of one kind: letters and
//! digits are one, whitespace is another, and everything else — punctuation,
//! brackets, dashes — is the third. `Ctrl+Right` in `hello, world` steps over
//! `hello`, then over `, `, and lands on `world`, which is what every editor
//! that has ever bound that key does.
//!
//! Letters means letters in any alphabet, not the twenty-six: `is_alphanumeric`
//! is true of Cyrillic, Greek, Arabic and Han alike, so this works in a document
//! written in any of them.
//!
//! # Where it is not right, and why it is still this
//!
//! Chinese and Japanese are written without spaces, so a run of them is one
//! word by this rule where a reader sees several. Splitting them properly needs
//! a dictionary of the language — which Word has and this does not, yet. Until
//! it does, a rule that is simple and predictable beats one that is subtly
//! wrong in a way nobody can guess.

/// What kind of character something is, for the purpose of finding words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Space,
    Word,
    Symbol,
}

impl Kind {
    fn of(character: char) -> Self {
        if character.is_whitespace() {
            Self::Space
        } else if character.is_alphanumeric() || character == '_' {
            Self::Word
        } else {
            Self::Symbol
        }
    }
}

/// Where the next word begins, going forwards from an offset.
///
/// The rest of whatever is under the caret is stepped over, then the whitespace
/// after it — so the caret lands on the first character of the next word rather
/// than in the gap before it.
#[must_use]
pub fn next_word(text: &str, offset: usize) -> usize {
    let offset = offset.min(text.len());
    let Some(rest) = text.get(offset..) else { return text.len() };

    let mut at = offset;
    let mut characters = rest.chars();
    let Some(first) = characters.next() else { return text.len() };

    // Whitespace at the caret is only skipped — there is no word under it to
    // step over first.
    if Kind::of(first) != Kind::Space {
        let kind = Kind::of(first);
        at += first.len_utf8();
        for character in characters.by_ref() {
            if Kind::of(character) != kind {
                break;
            }
            at += character.len_utf8();
        }
    }

    for character in text[at..].chars() {
        if Kind::of(character) != Kind::Space {
            break;
        }
        at += character.len_utf8();
    }
    at
}

/// Where the word under the caret begins, going backwards from an offset.
///
/// The whitespace behind the caret is stepped over first, then the word behind
/// that — so pressing it at the end of `one two ` lands at the `t` of `two`,
/// not in the gap after it.
#[must_use]
pub fn previous_word(text: &str, offset: usize) -> usize {
    let offset = offset.min(text.len());
    let Some(before) = text.get(..offset) else { return 0 };

    let mut at = offset;
    for character in before.chars().rev() {
        if Kind::of(character) != Kind::Space {
            break;
        }
        at -= character.len_utf8();
    }

    let Some(before) = text.get(..at) else { return 0 };
    let Some(last) = before.chars().next_back() else { return at };
    let kind = Kind::of(last);
    for character in before.chars().rev() {
        if Kind::of(character) != kind {
            break;
        }
        at -= character.len_utf8();
    }
    at
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
