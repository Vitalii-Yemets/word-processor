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
//! Spelling proper needs a dictionary of the language, and none is shipped with
//! this program: they are data with their own licences, as typefaces are. Word
//! ships one for each language it supports and it is the larger part of what
//! proofing costs. So spelling is checked against whatever dictionary the
//! machine has for the language each run is in, and left alone where there is
//! none — rather than guessed at, which would underline correct words and teach
//! people to ignore the underlining.
//!
//! # What grammar means here
//!
//! The mistakes that show in a handful of words side by side — "could of",
//! "a apple", "he don't", a double negative — found by rules over the words,
//! which is what [`wp_grammar`] holds. Not the grammar of the sentence: "which
//! of these two verbs agrees with that noun" needs a parser of the language
//! and a part of speech for every word, and is wrong often enough even then
//! that a check which fires on half of what it should is worse than one that
//! says what it cannot do.

use std::collections::{HashMap, HashSet};

use crate::{Document, TextPosition};

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
    /// A mistake the grammar rules found, named as Word names it: "Article
    /// use", "Verb form", "Double negation".
    Grammar(&'static str),
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
            Self::Grammar(category) => category,
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

/// The words a language is known to have, and the rules for making their
/// forms.
///
/// Empty until somebody gives it a dictionary, and an empty one checks no
/// spelling at all — which is the honest thing for a program with none to do.
/// See [`wp_dict`] for what a real one is and why a word list alone is not it.
#[derive(Clone, Debug, Default)]
pub struct Dictionary {
    words: wp_dict::Dictionary,
    /// What the reader has added by hand, which outlives a change of
    /// dictionary because it is theirs and not the language's.
    own: HashSet<String>,
}

impl Dictionary {
    /// Reads a plain list of words, one to a line.
    ///
    /// What somebody's own list of names and jargon looks like. A real
    /// dictionary is two files and is read by [`Dictionary::read`].
    #[must_use]
    pub fn parse(text: &str) -> Self {
        Self { words: wp_dict::Dictionary::from_list(text), own: HashSet::new() }
    }

    /// Reads a real dictionary: the word list and the affix rules that say
    /// what forms its words take.
    pub fn read(affix: &[u8], words: &[u8]) -> Result<Self, wp_dict::Error> {
        Ok(Self::from_words(wp_dict::Dictionary::read(affix, words)?))
    }

    /// The same from a dictionary already read, which is what a program that
    /// found one on the machine has.
    #[must_use]
    pub fn from_words(words: wp_dict::Dictionary) -> Self {
        Self { words, own: HashSet::new() }
    }

    /// Whether anything is known at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.words.is_empty() && self.own.is_empty()
    }

    /// How many stems are held. Not how many words are known, which is a far
    /// larger number and is what the rules are for.
    #[must_use]
    pub fn len(&self) -> usize {
        self.words.stems() + self.own.len()
    }

    /// Whether a word is one the language has.
    #[must_use]
    pub fn knows(&self, word: &str) -> bool {
        self.own.contains(&word.to_lowercase()) || self.words.spelled(word)
    }

    /// Adds a word, as "Add to Dictionary" does.
    pub fn add(&mut self, word: &str) {
        self.own.insert(word.to_lowercase());
    }

    /// What the writer probably meant by a word this does not know.
    #[must_use]
    pub fn suggest(&self, word: &str) -> Vec<String> {
        self.words.suggest(word)
    }
}

/// The dictionaries a document is checked against: one for each language it
/// is written in, and what the reader has told the checker to leave alone.
///
/// # Why more than one
///
/// Because a document is not written in one language. A French quotation in
/// an English essay is French, and the document says so — every run carries
/// its language — and checking it against English underlines every word of
/// it. Word keeps a dictionary per language and picks by the run; so does
/// this.
#[derive(Clone, Debug, Default)]
pub struct Dictionaries {
    /// By language tag, lower-cased and with the hyphen the document uses.
    by_language: HashMap<String, Dictionary>,
    /// Words the reader said to ignore everywhere, which lasts as long as the
    /// document is open: "Ignore All" is about this document, and adding to
    /// the dictionary is about every document.
    ignored: HashSet<String>,
}

impl Dictionaries {
    /// One dictionary for everything, whatever language the text claims.
    #[must_use]
    pub fn single(dictionary: Dictionary) -> Self {
        let mut out = Self::default();
        out.insert("", dictionary);
        out
    }

    /// Adds the dictionary for a language.
    pub fn insert(&mut self, tag: &str, dictionary: Dictionary) {
        self.by_language.insert(normalise(tag), dictionary);
    }

    /// Whether there is a dictionary for a language, or one near enough: the
    /// one for the language without the country will do, and so will any
    /// country's where the document names none.
    #[must_use]
    pub fn has_language(&self, tag: &str) -> bool {
        self.for_language(tag).is_some()
    }

    /// The dictionary a run in a language is checked against.
    #[must_use]
    pub fn for_language(&self, tag: &str) -> Option<&Dictionary> {
        let wanted = normalise(tag);
        if let Some(found) = self.by_language.get(&wanted) {
            return Some(found);
        }
        let base = wanted.split('-').next().unwrap_or_default();
        if let Some(found) = self.by_language.get(base) {
            return Some(found);
        }
        // Any dictionary of the same language: en-US for text marked en-GB
        // where there is nothing better, which is what Word does too.
        self.by_language
            .iter()
            .find(|(held, _)| held.split('-').next() == Some(base))
            .map(|(_, dictionary)| dictionary)
            // The one for no language at all, which is what a plain word list
            // loaded by hand is.
            .or_else(|| self.by_language.get(""))
    }

    /// The languages there are dictionaries for.
    #[must_use]
    pub fn languages(&self) -> Vec<String> {
        let mut out: Vec<String> = self.by_language.keys().cloned().collect();
        out.sort();
        out
    }

    /// Whether nothing at all can be checked.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_language.values().all(Dictionary::is_empty)
    }

    /// Leaves a word alone for the rest of this session, wherever it appears.
    pub fn ignore(&mut self, word: &str) {
        self.ignored.insert(word.to_lowercase());
    }

    #[must_use]
    pub fn is_ignored(&self, word: &str) -> bool {
        self.ignored.contains(&word.to_lowercase())
    }

    /// Adds a word to every dictionary, as "Add to Dictionary" does: the word
    /// is the reader's and belongs to no language in particular.
    pub fn add(&mut self, word: &str) {
        if self.by_language.is_empty() {
            self.by_language.insert(String::new(), Dictionary::default());
        }
        for dictionary in self.by_language.values_mut() {
            dictionary.add(word);
        }
    }

    /// What the writer probably meant by a word in a language.
    #[must_use]
    pub fn suggest(&self, word: &str, tag: &str) -> Vec<String> {
        self.for_language(tag).map(|dictionary| dictionary.suggest(word)).unwrap_or_default()
    }
}

/// A language tag the way the map keys it: `en-US` and `en_us` are one.
fn normalise(tag: &str) -> String {
    tag.trim().replace('_', "-").to_lowercase()
}

/// What was found in each paragraph last time, kept against what the
/// paragraph was.
#[derive(Clone, Debug, Default)]
pub struct ProofingCache {
    /// By a hash of the text and its languages; the issues inside are numbered
    /// against the paragraph they were found in, not the document.
    held: HashMap<u64, Vec<Issue>>,
}

impl ProofingCache {
    fn key(text: &str, stretches: &[Stretch]) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        text.hash(&mut hasher);
        for stretch in stretches {
            stretch.start.hash(&mut hasher);
            stretch.end.hash(&mut hasher);
            stretch.language.hash(&mut hasher);
            stretch.no_proof.hash(&mut hasher);
        }
        hasher.finish()
    }

    /// Forgets everything, which is what a change of dictionary calls for.
    pub fn clear(&mut self) {
        self.held.clear();
    }
}

/// One stretch of a paragraph and how its text is to be checked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stretch {
    pub start: usize,
    pub end: usize,
    /// The language the run says it is in, if it says.
    pub language: Option<String>,
    /// Whether the run asked to be left alone.
    pub no_proof: bool,
}

impl Document {
    /// Every mistake in the document, in reading order.
    ///
    /// Each run is checked against the dictionary for the language it says it
    /// is in, and a run that asked to be left alone is left alone. The
    /// document itself may ask for the spelling marks or the others to be
    /// hidden, which Word keeps in its settings and this honours.
    #[must_use]
    pub fn proofing_issues(&self, dictionaries: &Dictionaries) -> Vec<Issue> {
        self.proofing_issues_cached(dictionaries, &mut ProofingCache::default())
    }

    /// The same, remembering each paragraph's answer for as long as the
    /// paragraph is unchanged.
    ///
    /// Checking is done again after every keystroke, because that is what "as
    /// you type" means; but a keystroke changes one paragraph, and fifty
    /// thousand words of the other paragraphs asked of the dictionary again
    /// is what makes typing lag. So each paragraph's mistakes are kept against
    /// its text and its languages, and only a paragraph that is not what it
    /// was is checked afresh.
    #[must_use]
    pub fn proofing_issues_cached(
        &self,
        dictionaries: &Dictionaries,
        cache: &mut ProofingCache,
    ) -> Vec<Issue> {
        let spelling = !self.setting_is_on("hideSpellingErrors");
        let grammar = !self.setting_is_on("hideGrammaticalErrors");
        if !spelling && !grammar {
            return Vec::new();
        }

        let mut fresh = HashMap::new();
        let mut out = Vec::new();
        for index in 0..self.paragraph_count() {
            let Some(text) = self.paragraph_text(index) else { continue };
            let stretches = self.stretches_of(index, text.len());
            let key = ProofingCache::key(&text, &stretches);

            let found = match cache.held.remove(&key) {
                Some(found) => found,
                None => {
                    let mut found = Vec::new();
                    check_paragraph(0, &text, dictionaries, &stretches, &mut found);
                    found
                }
            };
            out.extend(
                found
                    .iter()
                    .filter(|issue| if issue.kind.is_spelling() { spelling } else { grammar })
                    .map(|issue| Issue { paragraph: index, ..issue.clone() }),
            );
            fresh.insert(key, found);
        }
        // What was not seen this time belongs to a paragraph that is gone.
        cache.held = fresh;
        out
    }

    /// How each stretch of a paragraph asks to be checked: which language it
    /// is in, and whether it is to be checked at all.
    fn stretches_of(&self, paragraph: usize, length: usize) -> Vec<Stretch> {
        let Some(element) = self.paragraph_element(paragraph) else { return Vec::new() };
        crate::format::runs_in_range(element, 0, length, &self.styles)
            .into_iter()
            .map(|(start, (end, resolved))| Stretch {
                start,
                end,
                language: resolved.language,
                no_proof: resolved.no_proof,
            })
            .collect()
    }

    /// Every language the document says any of its text is in, for finding a
    /// dictionary for each.
    #[must_use]
    pub fn languages_used(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for index in 0..self.paragraph_count() {
            let Some(text) = self.paragraph_text(index) else { continue };
            for stretch in self.stretches_of(index, text.len()) {
                if let Some(language) = stretch.language {
                    if !out.contains(&language) {
                        out.push(language);
                    }
                }
            }
        }
        out
    }

    /// The language a word at a position is in, for asking what was meant by
    /// it.
    #[must_use]
    pub fn language_at(&self, at: TextPosition) -> String {
        let Some(text) = self.paragraph_text(at.paragraph) else {
            return self.language_here();
        };
        self.stretches_of(at.paragraph, text.len())
            .into_iter()
            .find(|stretch| at.offset >= stretch.start && at.offset < stretch.end.max(1))
            .and_then(|stretch| stretch.language)
            .unwrap_or_else(|| self.language_here())
    }
}

/// Finds every mistake in one paragraph.
///
/// `stretches` says which language each part of the text is in and which
/// parts are not to be checked; empty means all of it, in whatever language
/// the dictionaries hold for none.
pub fn check_paragraph(
    paragraph: usize,
    text: &str,
    dictionaries: &Dictionaries,
    stretches: &[Stretch],
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

    // The grammar rules, which are for English: applied where the text says it
    // is English or says nothing, and left alone where it says otherwise or
    // asks not to be checked.
    for finding in wp_grammar::check(text) {
        let stretch = stretches
            .iter()
            .find(|stretch| finding.start >= stretch.start && finding.start < stretch.end);
        if stretch.is_some_and(|stretch| stretch.no_proof) {
            continue;
        }
        let english = stretch
            .and_then(|stretch| stretch.language.as_deref())
            .is_none_or(|tag| tag.len() < 2 || tag[..2].eq_ignore_ascii_case("en"));
        if !english {
            continue;
        }
        out.push(Issue {
            paragraph,
            start: finding.start,
            end: finding.end,
            kind: Kind::Grammar(finding.category),
            text: text[finding.start..finding.end].to_owned(),
            suggestion: finding.replacement,
        });
    }

    // And the spelling, when there is a dictionary to check it against — the
    // one for the language each word is in.
    if dictionaries.is_empty() {
        return;
    }
    for (start, end, word) in &words {
        if !is_wordy(word) || dictionaries.is_ignored(word) {
            continue;
        }
        let stretch =
            stretches.iter().find(|stretch| *start >= stretch.start && *start < stretch.end);
        if stretch.is_some_and(|stretch| stretch.no_proof) {
            continue;
        }
        let language = stretch.and_then(|stretch| stretch.language.as_deref()).unwrap_or("");
        let Some(dictionary) = dictionaries.for_language(language) else {
            // A language nobody has a dictionary for is not a language full of
            // mistakes.
            continue;
        };
        if dictionary.knows(word) {
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
        check_paragraph(0, text, &Dictionaries::default(), &[], &mut out);
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
        check_paragraph(
            0,
            "The qwertyuiop is not a word.",
            &Dictionaries::single(dictionary),
            &[],
            &mut out,
        );

        let unknown: Vec<&Issue> =
            out.iter().filter(|issue| issue.kind == Kind::UnknownWord).collect();
        assert_eq!(unknown.len(), 1);
        assert_eq!(unknown[0].text, "qwertyuiop");
    }

    #[test]
    fn a_word_in_capitals_is_left_alone() {
        let dictionary = Dictionary::parse("the\nis\nan\n");
        let mut out = Vec::new();
        check_paragraph(
            0,
            "The BBC is an abbreviation.",
            &Dictionaries::single(dictionary),
            &[],
            &mut out,
        );
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

/// What a real dictionary changes about the checking.
///
/// The rest of this file's tests are held to a word list written for them.
/// These are held to the dictionary the machine has, because the whole point
/// of reading the affix rules is what happens to words nobody listed.
#[cfg(test)]
mod against_a_real_dictionary {
    use super::{check_paragraph, Dictionaries, Dictionary, Kind};

    fn english() -> Dictionary {
        let path = std::path::Path::new("/usr/share/hunspell/en_US.dic");
        let words = wp_dict::read_pair(path).unwrap_or_else(|error| {
            panic!(
                "cannot read {}: {error}\n\
                 the build image should install hunspell-en-us",
                path.display()
            )
        });
        Dictionary::from_words(words)
    }

    /// The words a paragraph is marked for, in order.
    fn marked(text: &str, dictionary: &Dictionary) -> Vec<String> {
        let mut out = Vec::new();
        check_paragraph(0, text, &Dictionaries::single(dictionary.clone()), &[], &mut out);
        out.iter()
            .filter(|issue| issue.kind == Kind::UnknownWord)
            .map(|issue| issue.text.clone())
            .collect()
    }

    #[test]
    fn the_forms_of_a_word_are_not_underlined() {
        // Every one of these is a form no word list holds, and a checker
        // without the rules underlines all of them — which teaches the reader
        // to ignore the underlining, and that is worse than no checker.
        let dictionary = english();
        let text = "She walked quickly to the biggest houses and tried opening them.";
        assert_eq!(marked(text, &dictionary), Vec::<String>::new());
    }

    #[test]
    fn what_is_really_wrong_is_still_underlined() {
        let dictionary = english();
        let text = "I definately recieve teh letter.";
        assert_eq!(marked(text, &dictionary), vec!["definately", "recieve", "teh"]);
    }

    #[test]
    fn a_word_added_by_hand_outlives_the_dictionary_it_was_added_to() {
        // "Add to Dictionary" is the reader's, not the language's: it must
        // still hold when another language's dictionary is loaded.
        let mut dictionary = english();
        assert_eq!(marked("Grzegorz wrote it.", &dictionary), vec!["Grzegorz"]);
        dictionary.add("Grzegorz");
        assert_eq!(marked("Grzegorz wrote it.", &dictionary), Vec::<String>::new());
    }
}

/// What the settings and the languages of a document change about the
/// checking, held to word lists written here so that the reading of the
/// document — not of a dictionary — is what is tested.
#[cfg(test)]
mod by_language {
    use super::{Dictionaries, Dictionary, Kind};
    use crate::model::{Block, Body, Paragraph, Run};
    use crate::Document;

    /// A document of one paragraph made of the runs given, each in a language.
    fn made_of(runs: &[(&str, Option<&str>)]) -> Document {
        let mut body = Body::default();
        let runs: Vec<Run> = runs
            .iter()
            .map(|(text, language)| match language {
                Some(tag) => Run::text(text).in_language(tag),
                None => Run::text(text),
            })
            .collect();
        body.blocks.push(Block::Paragraph(Paragraph::from_runs(runs)));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        Document::open(&bytes).expect("reopening")
    }

    fn english_and_french() -> Dictionaries {
        let mut dictionaries = Dictionaries::default();
        dictionaries.insert("en-US", Dictionary::parse("the\nword\nis\nhere\nand\n"));
        dictionaries.insert("fr-FR", Dictionary::parse("le\nmot\nest\nici\n"));
        dictionaries
    }

    fn misspelt(document: &Document, dictionaries: &Dictionaries) -> Vec<String> {
        document
            .proofing_issues(dictionaries)
            .into_iter()
            .filter(|issue| issue.kind == Kind::UnknownWord)
            .map(|issue| issue.text)
            .collect()
    }

    #[test]
    fn each_run_is_checked_against_the_dictionary_of_its_own_language() {
        // A French quotation in an English document is French, and checking
        // it against English underlines every word of it.
        let document = made_of(&[("the word ", Some("en-US")), ("le mot", Some("fr-FR"))]);
        assert_eq!(misspelt(&document, &english_and_french()), Vec::<String>::new());

        let wrong = made_of(&[("the mot ", Some("en-US")), ("le word", Some("fr-FR"))]);
        assert_eq!(misspelt(&wrong, &english_and_french()), vec!["mot", "word"]);
    }

    #[test]
    fn a_language_with_no_dictionary_is_not_a_language_full_of_mistakes() {
        let document = made_of(&[("the word ", Some("en-US")), ("das Wort", Some("de-DE"))]);
        assert_eq!(misspelt(&document, &english_and_french()), Vec::<String>::new());
    }

    #[test]
    fn a_dictionary_for_the_language_serves_every_country_of_it() {
        // en-GB text with only an en-US dictionary is checked against it, as
        // Word does, rather than left unchecked.
        let document = made_of(&[("the wrod", Some("en-GB"))]);
        assert_eq!(misspelt(&document, &english_and_french()), vec!["wrod"]);
    }

    #[test]
    fn a_run_that_asked_to_be_left_alone_is_left_alone() {
        let mut document = made_of(&[("the wrod", Some("en-US"))]);
        assert_eq!(misspelt(&document, &english_and_french()), vec!["wrod"]);

        document.select_all();
        assert!(document.set_no_proof(true));
        assert_eq!(misspelt(&document, &english_and_french()), Vec::<String>::new());
    }

    #[test]
    fn the_document_may_ask_for_its_spelling_marks_to_be_hidden() {
        // Word keeps this in the file, so whoever opens it next sees the same.
        let mut document = made_of(&[("the wrod  here", Some("en-US"))]);
        let issues = document.proofing_issues(&english_and_french());
        assert!(issues.iter().any(|issue| issue.kind == Kind::UnknownWord));
        assert!(issues.iter().any(|issue| issue.kind == Kind::DoubleSpace));

        assert!(document.set_setting_flag("hideSpellingErrors", true));
        let issues = document.proofing_issues(&english_and_french());
        assert!(!issues.iter().any(|issue| issue.kind == Kind::UnknownWord), "still marked");
        assert!(issues.iter().any(|issue| issue.kind == Kind::DoubleSpace), "the rest went too");

        // And it survives being saved, because it is the document's.
        let bytes = document.save().expect("saving");
        let again = Document::open(&bytes).expect("reopening");
        assert!(again.setting_is_on("hideSpellingErrors"));
    }

    #[test]
    fn a_word_ignored_is_ignored_everywhere_and_a_word_added_is_known() {
        let document = made_of(&[("the wrod and the wrod", Some("en-US"))]);
        let mut dictionaries = english_and_french();
        assert_eq!(misspelt(&document, &dictionaries), vec!["wrod", "wrod"]);

        dictionaries.ignore("wrod");
        assert_eq!(misspelt(&document, &dictionaries), Vec::<String>::new());

        let mut fresh = english_and_french();
        fresh.add("Wrod");
        assert_eq!(misspelt(&document, &fresh), Vec::<String>::new());
    }

    #[test]
    fn the_document_says_which_languages_it_is_written_in() {
        let document = made_of(&[("the ", Some("en-US")), ("le ", Some("fr-FR")), ("das", None)]);
        let mut used = document.languages_used();
        used.sort();
        assert!(used.contains(&"en-US".to_owned()), "{used:?}");
        assert!(used.contains(&"fr-FR".to_owned()), "{used:?}");
    }

    #[test]
    fn the_spellings_offered_are_in_the_language_of_the_word() {
        let dictionaries = english_and_french();
        assert_eq!(dictionaries.suggest("wrod", "en-US"), vec!["word"]);
        assert_eq!(dictionaries.suggest("mto", "fr-FR"), vec!["mot"]);
    }
}

#[cfg(test)]
mod remembering {
    use super::{Dictionaries, Dictionary, Kind, ProofingCache};
    use crate::model::{Block, Body, Paragraph};
    use crate::Document;

    fn document(lines: &[&str]) -> Document {
        let mut body = Body::default();
        for line in lines {
            body.blocks.push(Block::Paragraph(Paragraph::text(line)));
        }
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        Document::open(&bytes).expect("reopening")
    }

    #[test]
    fn a_paragraph_checked_once_is_remembered_by_what_it_says() {
        let dictionaries = Dictionaries::single(Dictionary::parse("one\ntwo\n"));
        let document = document(&["one twwo", "two"]);
        let mut cache = ProofingCache::default();

        let first = document.proofing_issues_cached(&dictionaries, &mut cache);
        let spelling: Vec<_> = first.iter().filter(|issue| issue.kind.is_spelling()).collect();
        assert_eq!(spelling.len(), 1);
        assert_eq!(spelling[0].text, "twwo");
        assert_eq!(cache.held.len(), 2, "one answer per paragraph");

        // Asked again, the same answers come back — with the paragraph
        // numbers they belong to, which is not what the cache holds.
        let again = document.proofing_issues_cached(&dictionaries, &mut cache);
        assert_eq!(again, first);
    }

    #[test]
    fn a_paragraph_that_moved_keeps_its_answer_and_gets_its_new_number() {
        let dictionaries = Dictionaries::single(Dictionary::parse("one\ntwo\n"));
        let mut cache = ProofingCache::default();
        let _ = document(&["two", "one twwo"]).proofing_issues_cached(&dictionaries, &mut cache);

        // The same paragraphs the other way round: nothing is checked afresh,
        // and the mistake is now in the first.
        let swapped = document(&["one twwo", "two"]);
        let issues = swapped.proofing_issues_cached(&dictionaries, &mut cache);
        let spelling: Vec<_> = issues.iter().filter(|issue| issue.kind.is_spelling()).collect();
        assert_eq!(spelling.len(), 1);
        assert_eq!(spelling[0].paragraph, 0);
        assert_eq!(spelling[0].kind, Kind::UnknownWord);
    }

    #[test]
    fn a_paragraph_that_is_gone_is_forgotten() {
        let dictionaries = Dictionaries::single(Dictionary::parse("one\ntwo\n"));
        let mut cache = ProofingCache::default();
        let _ =
            document(&["one", "two", "three"]).proofing_issues_cached(&dictionaries, &mut cache);
        assert_eq!(cache.held.len(), 3);
        let _ = document(&["one"]).proofing_issues_cached(&dictionaries, &mut cache);
        assert_eq!(cache.held.len(), 1);
    }
}

#[cfg(test)]
mod grammar {
    use super::{Dictionaries, Dictionary, Kind};
    use crate::model::{Block, Body, Paragraph, Run};
    use crate::Document;

    fn made_of(runs: &[(&str, Option<&str>)]) -> Document {
        let mut body = Body::default();
        let runs: Vec<Run> = runs
            .iter()
            .map(|(text, language)| match language {
                Some(tag) => Run::text(text).in_language(tag),
                None => Run::text(text),
            })
            .collect();
        body.blocks.push(Block::Paragraph(Paragraph::from_runs(runs)));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        Document::open(&bytes).expect("reopening")
    }

    fn grammar(document: &Document) -> Vec<(String, &'static str, Option<String>)> {
        document
            .proofing_issues(&Dictionaries::default())
            .into_iter()
            .filter_map(|issue| match issue.kind {
                Kind::Grammar(category) => Some((issue.text, category, issue.suggestion)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_mistake_every_word_of_which_is_spelled_right_is_found() {
        // Which is the whole point: a spelling checker sees nothing wrong with
        // "could of", and a reader sees it at once.
        let document = made_of(&[("I could of gone to a university.", Some("en-US"))]);
        assert_eq!(
            grammar(&document),
            vec![("could of".to_owned(), "Verb form", Some("could have".to_owned()))]
        );
    }

    #[test]
    fn the_rules_are_for_english_and_leave_other_languages_alone() {
        let french = made_of(&[("Je could of gone.", Some("fr-FR"))]);
        assert_eq!(grammar(&french), vec![]);
        // Text that names no language is taken to be the default, which is
        // English.
        let unmarked = made_of(&[("I could of gone.", None)]);
        assert_eq!(grammar(&unmarked).len(), 1);
    }

    #[test]
    fn a_run_asked_to_be_left_alone_is_left_alone() {
        let mut document = made_of(&[("I could of gone.", Some("en-US"))]);
        document.select_all();
        assert!(document.set_no_proof(true));
        assert_eq!(grammar(&document), vec![]);
    }

    #[test]
    fn the_document_may_hide_the_grammar_marks_and_keep_the_spelling_ones() {
        let mut document = made_of(&[("I could of gone hme.", Some("en-US"))]);
        let mut dictionaries = Dictionaries::default();
        dictionaries.insert("en-US", Dictionary::parse("i\ncould\nof\ngone\nhome\n"));

        let before = document.proofing_issues(&dictionaries);
        assert!(before.iter().any(|issue| matches!(issue.kind, Kind::Grammar(_))));
        assert!(before.iter().any(|issue| issue.kind == Kind::UnknownWord));

        assert!(document.set_setting_flag("hideGrammaticalErrors", true));
        let after = document.proofing_issues(&dictionaries);
        assert!(!after.iter().any(|issue| matches!(issue.kind, Kind::Grammar(_))));
        assert!(after.iter().any(|issue| issue.kind == Kind::UnknownWord));
    }

    #[test]
    fn a_grammar_mistake_is_not_a_spelling_one() {
        // Drawn in the other colour, and offered no dictionary to be added to.
        assert!(!Kind::Grammar("Article use").is_spelling());
        assert_eq!(Kind::Grammar("Article use").message(), "Article use");
    }
}
