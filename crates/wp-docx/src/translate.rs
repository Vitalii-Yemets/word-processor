//! Translating a document against a glossary.
//!
//! # Why this is a glossary and not a translation service
//!
//! Word's Translate sends the text to Microsoft's servers and shows what comes
//! back. This program does not talk to the network at all — not because it
//! would be hard, but because a word processor that quietly posts the document
//! somebody is writing to a third party is a different kind of program, and the
//! choice belongs to whoever is writing it.
//!
//! What it does instead is what translators actually do first: it applies a
//! glossary. A file of `source = target` lines is read in, and every term in it
//! is replaced through the document, keeping the capitals. That is the part of
//! translation a machine does reliably — terminology, product names, the words
//! that have to come out the same every time — and it works with no network,
//! for any pair of languages, using the person's own list.
//!
//! It is the same bargain the spelling check makes: bring your own words. See
//! [`crate::proofing`].
//!
//! # What it does not do
//!
//! Grammar, word order, agreement, or anything that needs to know what a
//! sentence means. A document put through this is a document with its terms
//! translated, and the status bar says how many.

use std::collections::BTreeMap;

use crate::history::EditKind;
use crate::{edit, Document, TextPosition};

/// The longest phrase looked for, in words.
///
/// Terminology is short: "user interface", "chief executive officer". Looking
/// further would cost time on every word for phrases nobody writes.
const LONGEST_PHRASE: usize = 4;

/// A list of terms and what each becomes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Glossary {
    /// Keyed by the source term folded to lower case, so a term is found
    /// however it was capitalised in the text.
    terms: BTreeMap<String, String>,
}

impl Glossary {
    /// Reads a glossary out of a file.
    ///
    /// One term per line, with the source and the target separated by `=`, a
    /// tab or a semicolon — whichever the person's list uses. A line with no
    /// separator, and a line starting with `#`, is a comment.
    #[must_use]
    pub fn parse(bytes: &[u8]) -> Self {
        // The same reading a list of recipients gets: whatever encoding the
        // file is in, or the bytes as characters when nothing says.
        let text = wp_xml::decode_to_utf8(bytes)
            .map(|text| text.into_owned())
            .unwrap_or_else(|_| bytes.iter().map(|byte| *byte as char).collect());
        let mut terms = BTreeMap::new();

        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((source, target)) = line
                .split_once('=')
                .or_else(|| line.split_once('\t'))
                .or_else(|| line.split_once(';'))
            else {
                continue;
            };
            let (source, target) = (source.trim(), target.trim());
            if source.is_empty() || target.is_empty() {
                continue;
            }
            terms.insert(source.to_lowercase(), target.to_owned());
        }
        Self { terms }
    }

    /// How many terms it holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.terms.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    /// What a term becomes, if it is in the list.
    #[must_use]
    pub fn lookup(&self, term: &str) -> Option<&str> {
        self.terms.get(&term.to_lowercase()).map(String::as_str)
    }

    /// Where every term of the glossary falls in a line of text, and what each
    /// becomes — with the capitals of the original kept.
    ///
    /// The ranges never overlap and come back in order, so they can be applied
    /// one after another.
    #[must_use]
    pub fn matches(&self, text: &str) -> Vec<(usize, usize, String)> {
        let words = words_of(text);
        let mut out: Vec<(usize, usize, String)> = Vec::new();
        let mut index = 0usize;

        while index < words.len() {
            // The longest phrase first, so "user interface" beats "user".
            let mut taken = 0usize;
            for length in (1..=LONGEST_PHRASE.min(words.len() - index)).rev() {
                let (start, _) = words[index];
                let (_, end) = words[index + length - 1];
                let phrase = &text[start..end];
                let Some(target) = self.lookup(phrase) else { continue };

                out.push((start, end, matching_case(phrase, target)));
                taken = length;
                break;
            }
            index += taken.max(1);
        }
        out
    }
}

/// Every word in a line, as byte ranges.
///
/// A word is a run of letters and digits. Apostrophes and hyphens inside one
/// are part of it, because "don't" and "long-term" are one term each.
fn words_of(text: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;

    for (at, character) in text.char_indices() {
        let joined = character.is_alphanumeric() || character == '\'' || character == '-';
        match (joined, start) {
            (true, None) => start = Some(at),
            (false, Some(from)) => {
                out.push((from, at));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(from) = start {
        out.push((from, text.len()));
    }
    out
}

/// The target term written with the capitals the source had.
///
/// A term in the middle of a sentence stays as the glossary wrote it; one that
/// began a sentence is capitalised; one that was shouted is shouted back.
#[must_use]
fn matching_case(source: &str, target: &str) -> String {
    let letters: Vec<char> = source.chars().filter(|character| character.is_alphabetic()).collect();
    if letters.is_empty() {
        return target.to_owned();
    }

    if letters.len() > 1 && letters.iter().all(|character| character.is_uppercase()) {
        return target.to_uppercase();
    }
    if letters[0].is_uppercase() {
        let mut characters = target.chars();
        return match characters.next() {
            Some(first) => first.to_uppercase().collect::<String>() + characters.as_str(),
            None => target.to_owned(),
        };
    }
    target.to_owned()
}

impl Document {
    /// Replaces every term of the glossary, through the whole document or
    /// through the selection.
    ///
    /// Returns how many terms were replaced.
    pub fn translate(&mut self, glossary: &Glossary, selection_only: bool) -> usize {
        if glossary.is_empty() {
            return 0;
        }
        let range = if selection_only { self.selection() } else { None };
        if selection_only && range.is_none() {
            return 0;
        }

        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);

        let mut replaced = 0usize;
        for index in 0..self.paragraph_count() {
            let Some(text) = self.paragraph_text(index) else { continue };
            let mut found = glossary.matches(&text);
            if let Some((start, end)) = range {
                found.retain(|(from, to, _)| within(index, *from, *to, start, end));
            }
            if found.is_empty() {
                continue;
            }

            let Some(path) = crate::position::paragraph_path(&self.tree().root, index) else {
                continue;
            };
            let Some(paragraph) = edit::element_at_path_mut(&mut self.tree_mut().root, &path)
            else {
                continue;
            };
            replaced += edit::replace_ranges(paragraph, &found);
        }

        if replaced > 0 {
            self.clamp_caret();
            self.mark_modified();
        }
        replaced
    }
}

/// Whether a stretch of one paragraph falls inside a selection.
fn within(
    paragraph: usize,
    from: usize,
    to: usize,
    start: TextPosition,
    end: TextPosition,
) -> bool {
    let after_start =
        paragraph > start.paragraph || (paragraph == start.paragraph && from >= start.offset);
    let before_end = paragraph < end.paragraph || (paragraph == end.paragraph && to <= end.offset);
    after_start && before_end
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glossary() -> Glossary {
        Glossary::parse(b"cat = chat\ndog = chien\nuser interface = interface utilisateur\n")
    }

    #[test]
    fn a_glossary_is_read_out_of_a_file() {
        let list = glossary();
        assert_eq!(list.len(), 3);
        assert_eq!(list.lookup("cat"), Some("chat"));
    }

    #[test]
    fn a_term_is_found_however_it_was_capitalised() {
        assert_eq!(glossary().lookup("CAT"), Some("chat"));
        assert_eq!(glossary().lookup("Cat"), Some("chat"));
    }

    #[test]
    fn every_separator_a_list_might_use_is_understood() {
        let list = Glossary::parse(b"a = one\nb\ttwo\nc;three\n");
        assert_eq!(list.lookup("a"), Some("one"));
        assert_eq!(list.lookup("b"), Some("two"));
        assert_eq!(list.lookup("c"), Some("three"));
    }

    #[test]
    fn comments_and_lines_that_say_nothing_are_skipped() {
        let list = Glossary::parse(b"# a note\n\nnonsense\na = one\n");
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn a_term_with_no_translation_is_not_a_term() {
        assert!(Glossary::parse(b"a =\n= one\n").is_empty());
    }

    #[test]
    fn a_word_in_the_list_is_found_where_it_falls() {
        let found = glossary().matches("the cat sat");
        assert_eq!(found, vec![(4, 7, "chat".to_owned())]);
    }

    #[test]
    fn a_word_that_is_not_in_the_list_is_left_alone() {
        assert!(glossary().matches("the mouse sat").is_empty());
    }

    #[test]
    fn the_longest_phrase_wins_over_its_first_word() {
        let list = Glossary::parse(b"user = utilisateur\nuser interface = interface utilisateur\n");
        let found = list.matches("the user interface here");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].2, "interface utilisateur");
    }

    #[test]
    fn capitals_are_kept() {
        assert_eq!(matching_case("cat", "chat"), "chat");
        assert_eq!(matching_case("Cat", "chat"), "Chat");
        assert_eq!(matching_case("CAT", "chat"), "CHAT");
        // One capital letter on its own is a word, not shouting.
        assert_eq!(matching_case("A", "un"), "Un");
    }

    #[test]
    fn a_term_inside_a_longer_word_is_not_that_term() {
        // "catalogue" is not "cat".
        assert!(glossary().matches("the catalogue").is_empty());
    }

    #[test]
    fn a_hyphenated_word_is_one_word() {
        let list = Glossary::parse(b"long-term = a long terme\n");
        assert_eq!(list.matches("a long-term plan").len(), 1);
    }

    #[test]
    fn several_terms_in_one_line_come_back_in_order() {
        let found = glossary().matches("the cat and the dog");
        assert_eq!(found.len(), 2);
        assert!(found[0].0 < found[1].0);
    }

    #[test]
    fn words_are_found_where_they_are() {
        assert_eq!(words_of("a bc"), vec![(0, 1), (2, 4)]);
        assert_eq!(words_of("  "), Vec::new());
        assert_eq!(words_of("end"), vec![(0, 3)]);
    }
}
