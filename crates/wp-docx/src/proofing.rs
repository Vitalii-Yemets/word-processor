//! Checking the writing: the mistakes a program can find without a dictionary,
//! and the spelling it can check when it is given one.
//!
//! # Why the two are separated
//!
//! Most of what a word processor underlines is not spelling. A word typed
//! twice, a sentence begun in lower case, a space before a comma — none of
//! these needs to know a single word of the language, only what a sentence
//! looks like. They are found here for any language, and they are the mistakes
//! that most often survive proofreading, because the eye reads what was meant.
//!
//! Spelling proper needs a list of every word in the language, and there is no
//! such list in this program. Word ships one for each language it supports and
//! it is the larger part of what proofing costs. So spelling is checked when a
//! list is given and left alone when it is not — rather than guessed at, which
//! would underline correct words and teach people to ignore the underlining.
//!
//! # What is deliberately not checked
//!
//! Grammar. "Which of these two verbs agrees with that noun" is a different
//! kind of question and needs the grammar of the language, not its words.
//! Saying so is better than a check that fires on half of what it should.

use std::collections::HashSet;

use crate::Document;

/// What kind of mistake was found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The same word twice in a row.
    RepeatedWord,
    /// A sentence begun in lower case.
    MissingCapital,
    /// A space before a comma, a full stop or the like.
    SpaceBeforePunctuation,
    /// Two or more spaces where one belongs.
    DoubleSpace,
    /// A word the dictionary does not have.
    UnknownWord,
}

impl Kind {
    /// Whether this is spelling rather than the way the words are set out.
    ///
    /// Word draws the two differently — red for spelling and blue for the rest
    /// — and so does this.
    #[must_use]
    pub fn is_spelling(self) -> bool {
        self == Self::UnknownWord
    }

    /// What to say about it.
    #[must_use]
    pub fn message(self) -> &'static str {
        match self {
            Self::RepeatedWord => "Repeated word",
            Self::MissingCapital => "Sentence should begin with a capital",
            Self::SpaceBeforePunctuation => "Space before punctuation",
            Self::DoubleSpace => "More than one space",
            Self::UnknownWord => "Not in the dictionary",
        }
    }
}

/// One mistake, and where it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Issue {
    pub paragraph: usize,
    /// Byte offsets within the paragraph's text.
    pub start: usize,
    pub end: usize,
    pub kind: Kind,
    /// The text that is wrong.
    pub text: String,
    /// What to put in its place, when there is an obvious answer.
    pub suggestion: Option<String>,
}

/// The words a language is known to have.
///
/// Empty until somebody gives it a list, and an empty one checks no spelling
/// at all — which is the honest thing for a program with no dictionary to do.
#[derive(Clone, Debug, Default)]
pub struct Dictionary {
    words: HashSet<String>,
}

impl Dictionary {
    /// Reads a list of words, one to a line.
    ///
    /// Anything after a slash on a line is ignored, so a Hunspell dictionary's
    /// word list can be used as it stands — the affix rules after the slash are
    /// not applied, which means some forms of a word will be missed, and that
    /// is written here rather than found out.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let words = text
            .lines()
            .map(|line| line.split('/').next().unwrap_or_default().trim())
            .filter(|word| !word.is_empty() && !word.chars().all(|c| c.is_ascii_digit()))
            .map(str::to_lowercase)
            .collect();
        Self { words }
    }

    /// Whether the list has anything in it.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.words.len()
    }

    /// Whether a word is in the list.
    #[must_use]
    pub fn knows(&self, word: &str) -> bool {
        self.words.contains(&word.to_lowercase())
    }

    /// Adds a word, as "Add to Dictionary" does.
    pub fn add(&mut self, word: &str) {
        self.words.insert(word.to_lowercase());
    }
}

impl Document {
    /// Every mistake in the document, in reading order.
    #[must_use]
    pub fn proofing_issues(&self, dictionary: &Dictionary) -> Vec<Issue> {
        let mut out = Vec::new();
        for index in 0..self.paragraph_count() {
            let Some(text) = self.paragraph_text(index) else { continue };
            check_paragraph(index, &text, dictionary, &mut out);
        }
        out
    }
}

/// Finds every mistake in one paragraph.
pub fn check_paragraph(
    paragraph: usize,
    text: &str,
    dictionary: &Dictionary,
    out: &mut Vec<Issue>,
) {
    let words = words_of(text);

    // The same word twice in a row, ignoring how it was capitalised: "The the"
    // is the mistake, and so is "the The".
    for pair in words.windows(2) {
        let (first, second) = (&pair[0], &pair[1]);
        // Compared in lower case rather than by ignoring ASCII case: a document
        // is not written in English, and "Это это" is as much a repeat as
        // "The the".
        if first.2.to_lowercase() != second.2.to_lowercase() || !is_wordy(&first.2) {
            continue;
        }
        // Only when nothing but spaces lies between them: "so, so" is not a
        // repeat and neither is a word ending one sentence and beginning the
        // next.
        let between = &text[first.1..second.0];
        if !between.chars().all(char::is_whitespace) || between.is_empty() {
            continue;
        }
        out.push(Issue {
            paragraph,
            start: first.0,
            end: second.1,
            kind: Kind::RepeatedWord,
            text: text[first.0..second.1].to_owned(),
            suggestion: Some(first.2.clone()),
        });
    }

    // A sentence begun in lower case. The first word of the paragraph counts as
    // the beginning of one.
    for (at, (start, end, word)) in words.iter().enumerate() {
        if !is_wordy(word) {
            continue;
        }
        let begins_sentence = match at {
            0 => true,
            _ => {
                let before = &text[words[at - 1].1..*start];
                before.contains(['.', '!', '?', '…'])
            }
        };
        let first = word.chars().next().unwrap_or(' ');
        if begins_sentence && first.is_lowercase() {
            out.push(Issue {
                paragraph,
                start: *start,
                end: *end,
                kind: Kind::MissingCapital,
                text: word.clone(),
                suggestion: Some(capitalised(word)),
            });
        }
    }

    // A space before punctuation that closes rather than opens.
    let bytes: Vec<(usize, char)> = text.char_indices().collect();
    for at in 1..bytes.len() {
        let (start, space) = bytes[at - 1];
        let (_, mark) = bytes[at];
        if space == ' ' && matches!(mark, ',' | '.' | ';' | ':' | '!' | '?' | ')') {
            out.push(Issue {
                paragraph,
                start,
                end: start + space.len_utf8(),
                kind: Kind::SpaceBeforePunctuation,
                text: " ".to_owned(),
                suggestion: Some(String::new()),
            });
        }
    }

    // Two spaces or more. Counted as one mistake however many there are.
    let mut run: Option<usize> = None;
    for (at, character) in text.char_indices().chain(core::iter::once((text.len(), 'x'))) {
        if character == ' ' {
            run.get_or_insert(at);
            continue;
        }
        if let Some(start) = run.take() {
            if at - start > 1 {
                out.push(Issue {
                    paragraph,
                    start,
                    end: at,
                    kind: Kind::DoubleSpace,
                    text: text[start..at].to_owned(),
                    suggestion: Some(" ".to_owned()),
                });
            }
        }
    }

    // And the spelling, when there is a dictionary to check it against.
    if dictionary.is_empty() {
        return;
    }
    for (start, end, word) in &words {
        if !is_wordy(word) || dictionary.knows(word) {
            continue;
        }
        // A word in capitals is usually a name or an abbreviation, and Word
        // leaves those alone unless it is told not to.
        if word.chars().all(|c| !c.is_lowercase()) {
            continue;
        }
        out.push(Issue {
            paragraph,
            start: *start,
            end: *end,
            kind: Kind::UnknownWord,
            text: word.clone(),
            suggestion: None,
        });
    }
}

/// Every word of a text, with where it starts and ends.
///
/// A word is a run of letters, digits and the marks that live inside words —
/// an apostrophe and a hyphen — because "don't" and "well-known" are one word
/// each and underlining half of either would be wrong.
fn words_of(text: &str) -> Vec<(usize, usize, String)> {
    let mut out = Vec::new();
    let mut start = None;

    for (at, character) in text.char_indices() {
        let inside = character.is_alphanumeric() || character == '\'' || character == '\u{2019}';
        match (inside, start) {
            (true, None) => start = Some(at),
            (false, Some(from)) => {
                out.push((from, at, text[from..at].to_owned()));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(from) = start {
        out.push((from, text.len(), text[from..].to_owned()));
    }
    out
}

/// Whether a word is made of letters rather than of digits.
fn is_wordy(word: &str) -> bool {
    word.chars().any(char::is_alphabetic)
}

/// The same word with its first letter in upper case.
fn capitalised(word: &str) -> String {
    let mut characters = word.chars();
    match characters.next() {
        Some(first) => first.to_uppercase().collect::<String>() + characters.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issues(text: &str) -> Vec<Issue> {
        let mut out = Vec::new();
        check_paragraph(0, text, &Dictionary::default(), &mut out);
        out
    }

    fn kinds(text: &str) -> Vec<Kind> {
        issues(text).into_iter().map(|issue| issue.kind).collect()
    }

    #[test]
    fn a_sentence_written_properly_has_nothing_wrong_with_it() {
        assert!(issues("The quick brown fox jumps over the lazy dog.").is_empty());
    }

    #[test]
    fn a_word_typed_twice_is_found() {
        let found = issues("The the quick fox.");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, Kind::RepeatedWord);
        assert_eq!(found[0].text, "The the");
        assert_eq!(found[0].suggestion.as_deref(), Some("The"));
    }

    #[test]
    fn a_word_repeated_across_a_full_stop_is_not_a_repeat() {
        assert!(!kinds("I know. Know that.").contains(&Kind::RepeatedWord));
    }

    #[test]
    fn a_word_repeated_across_a_comma_is_not_a_repeat() {
        assert!(!kinds("So, so it goes.").contains(&Kind::RepeatedWord));
    }

    #[test]
    fn a_sentence_begun_in_lower_case_is_found() {
        let found = issues("the fox jumps.");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, Kind::MissingCapital);
        assert_eq!(found[0].suggestion.as_deref(), Some("The"));
    }

    #[test]
    fn a_second_sentence_is_checked_too() {
        let found = issues("A fox. the dog.");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].text, "the");
    }

    #[test]
    fn a_space_before_a_comma_is_found() {
        let found = issues("A fox , a dog.");
        assert_eq!(
            found.iter().filter(|issue| issue.kind == Kind::SpaceBeforePunctuation).count(),
            1
        );
    }

    #[test]
    fn a_space_after_a_comma_is_not() {
        assert!(!kinds("A fox, a dog.").contains(&Kind::SpaceBeforePunctuation));
    }

    #[test]
    fn two_spaces_are_found_and_counted_once() {
        let all = issues("A  fox.");
        let found: Vec<&Issue> =
            all.iter().filter(|issue| issue.kind == Kind::DoubleSpace).collect();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].suggestion.as_deref(), Some(" "));
    }

    #[test]
    fn four_spaces_are_still_one_mistake() {
        let found: Vec<Kind> =
            kinds("A    fox.").into_iter().filter(|kind| *kind == Kind::DoubleSpace).collect();
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn one_space_is_not_a_mistake() {
        assert!(!kinds("A fox.").contains(&Kind::DoubleSpace));
    }

    #[test]
    fn a_word_with_an_apostrophe_is_one_word() {
        assert!(issues("Don't do that.").is_empty(), "an apostrophe should not split a word");
    }

    #[test]
    fn nothing_is_checked_against_a_dictionary_that_is_empty() {
        assert!(!kinds("Qwertyuiop is not a word.").contains(&Kind::UnknownWord));
    }

    #[test]
    fn a_word_the_dictionary_does_not_have_is_found() {
        let dictionary = Dictionary::parse("the\nfox\nis\na\nword\nnot\n");
        let mut out = Vec::new();
        check_paragraph(0, "The qwertyuiop is not a word.", &dictionary, &mut out);

        let unknown: Vec<&Issue> =
            out.iter().filter(|issue| issue.kind == Kind::UnknownWord).collect();
        assert_eq!(unknown.len(), 1);
        assert_eq!(unknown[0].text, "qwertyuiop");
    }

    #[test]
    fn a_word_in_capitals_is_left_alone() {
        let dictionary = Dictionary::parse("the\nis\nan\n");
        let mut out = Vec::new();
        check_paragraph(0, "The BBC is an abbreviation.", &dictionary, &mut out);
        assert!(
            !out.iter().any(|issue| issue.text == "BBC"),
            "a name in capitals should not be underlined"
        );
    }

    #[test]
    fn a_dictionary_ignores_what_follows_a_slash() {
        let dictionary = Dictionary::parse("walk/DGS\nrun\n");
        assert!(dictionary.knows("walk"));
        assert!(!dictionary.knows("DGS"));
        assert_eq!(dictionary.len(), 2);
    }

    #[test]
    fn a_dictionary_does_not_mind_how_a_word_is_capitalised() {
        let dictionary = Dictionary::parse("London\n");
        assert!(dictionary.knows("london"));
        assert!(dictionary.knows("LONDON"));
    }

    #[test]
    fn a_word_added_to_the_dictionary_stops_being_a_mistake() {
        let mut dictionary = Dictionary::parse("the\n");
        assert!(!dictionary.knows("qwertyuiop"));
        dictionary.add("Qwertyuiop");
        assert!(dictionary.knows("qwertyuiop"));
    }

    #[test]
    fn the_checks_work_in_a_language_written_in_another_alphabet() {
        let found = issues("Это это предложение.");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, Kind::RepeatedWord);
        assert_eq!(found[0].text, "Это это");
    }

    #[test]
    fn only_spelling_is_spelling() {
        assert!(Kind::UnknownWord.is_spelling());
        for kind in [Kind::RepeatedWord, Kind::MissingCapital, Kind::DoubleSpace] {
            assert!(!kind.is_spelling(), "{}", kind.message());
        }
    }
}
