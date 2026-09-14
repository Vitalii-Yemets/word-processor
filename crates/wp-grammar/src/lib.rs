//! Grammar: the mistakes that are not spelling, found by rules.
//!
//! # What a grammar checker can honestly do
//!
//! Not grammar. Deciding whether a verb agrees with its subject needs to know
//! which word is the subject, and that needs a parser of the language and a
//! dictionary of every word's part of speech, and even then it is wrong often
//! enough that Word's own is turned off by half the people who write for a
//! living.
//!
//! What can be done, and what Word mostly does, is narrower and worth having:
//! the mistakes that show in a handful of words side by side. "Could of" is
//! never right. "A apple" is never right. "He don't" is never right in the
//! register a document is written in. "Don't know nothing" is a double
//! negative. These need no parser — a few words in a row and a rule about them
//! — and they are the mistakes a spelling checker cannot see because every
//! word in them is spelled correctly.
//!
//! # How a rule is written
//!
//! As a pattern over words: literal words, a choice of words, any word, a word
//! that begins with a vowel sound. A rule matches a run of words with nothing
//! but spaces between them — punctuation ends the run, because "I don't. No."
//! is not a double negative. Each rule carries what to say about the match,
//! which is the explanation Word gives, and what to put in its place where
//! there is one right answer.
//!
//! The rules are for English. A rule engine is the same for every language and
//! the rules are not, and the rules for another language are another item.

#![forbid(unsafe_code)]

/// One thing wrong, and where.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    /// Byte offsets into the text checked.
    pub start: usize,
    pub end: usize,
    /// What kind of mistake it is, as Word names it: "Article use",
    /// "Verb form", "Double negation".
    pub category: &'static str,
    /// What to put in its place, where there is one right answer.
    pub replacement: Option<String>,
}

/// One word of a pattern.
#[derive(Clone, Copy, Debug)]
enum Piece {
    /// This word, in any case.
    Word(&'static str),
    /// One of these words, in any case.
    AnyOf(&'static [&'static str]),
    /// Any word at all.
    Any,
    /// A word that begins with a vowel sound, which is what "an" goes before.
    VowelSound,
    /// A word that begins with a consonant sound, which is what "a" goes
    /// before.
    ConsonantSound,
}

/// One rule: what to look for, what to call it, what to put instead.
#[derive(Clone, Copy, Debug)]
struct Rule {
    pattern: &'static [Piece],
    category: &'static str,
    /// A template for the replacement, where `$1` is the first word matched
    /// and so on. `None` where the fix is the writer's to choose.
    replacement: Option<&'static str>,
}

/// The pronouns that take a verb in the singular.
const HE_SHE_IT: &[&str] = &["he", "she", "it"];
/// And those that take it in the plural.
const THEY_WE_YOU: &[&str] = &["they", "we", "you"];
/// A verb with "not" folded into it.
const NEGATIVE_VERBS: &[&str] = &[
    "don't",
    "doesn't",
    "didn't",
    "can't",
    "cannot",
    "couldn't",
    "won't",
    "wouldn't",
    "shouldn't",
    "isn't",
    "aren't",
    "wasn't",
    "weren't",
    "haven't",
    "hasn't",
    "hadn't",
    "ain't",
];
/// The words that make a negative a double one.
const NEGATIVE_WORDS: &[&str] = &["no", "nothing", "nobody", "nowhere", "never", "none", "neither"];
/// Verbs a "have" gets misheard after.
const MODALS: &[&str] = &["could", "should", "would", "might", "must", "may"];

/// The rules for English.
const ENGLISH: &[Rule] = &[
    // --- Article use -------------------------------------------------------
    Rule {
        pattern: &[Piece::Word("a"), Piece::VowelSound],
        category: "Article use",
        replacement: Some("an $2"),
    },
    Rule {
        pattern: &[Piece::Word("an"), Piece::ConsonantSound],
        category: "Article use",
        replacement: Some("a $2"),
    },
    // --- Verb form ---------------------------------------------------------
    Rule {
        pattern: &[Piece::AnyOf(MODALS), Piece::Word("of")],
        category: "Verb form",
        replacement: Some("$1 have"),
    },
    Rule {
        pattern: &[Piece::AnyOf(HE_SHE_IT), Piece::Word("don't")],
        category: "Subject-verb agreement",
        replacement: Some("$1 doesn't"),
    },
    Rule {
        pattern: &[Piece::AnyOf(HE_SHE_IT), Piece::Word("have")],
        category: "Subject-verb agreement",
        replacement: Some("$1 has"),
    },
    Rule {
        pattern: &[Piece::AnyOf(HE_SHE_IT), Piece::Word("were")],
        category: "Subject-verb agreement",
        replacement: Some("$1 was"),
    },
    Rule {
        pattern: &[Piece::AnyOf(THEY_WE_YOU), Piece::Word("was")],
        category: "Subject-verb agreement",
        replacement: Some("$1 were"),
    },
    Rule {
        pattern: &[Piece::AnyOf(THEY_WE_YOU), Piece::Word("is")],
        category: "Subject-verb agreement",
        replacement: Some("$1 are"),
    },
    Rule {
        pattern: &[Piece::AnyOf(THEY_WE_YOU), Piece::Word("has")],
        category: "Subject-verb agreement",
        replacement: Some("$1 have"),
    },
    Rule {
        pattern: &[Piece::AnyOf(THEY_WE_YOU), Piece::Word("doesn't")],
        category: "Subject-verb agreement",
        replacement: Some("$1 don't"),
    },
    Rule {
        pattern: &[Piece::Word("i"), Piece::Word("is")],
        category: "Subject-verb agreement",
        replacement: Some("I am"),
    },
    // --- Double negation ---------------------------------------------------
    Rule {
        pattern: &[Piece::AnyOf(NEGATIVE_VERBS), Piece::AnyOf(NEGATIVE_WORDS)],
        category: "Double negation",
        replacement: None,
    },
    Rule {
        pattern: &[Piece::AnyOf(NEGATIVE_VERBS), Piece::Any, Piece::AnyOf(NEGATIVE_WORDS)],
        category: "Double negation",
        replacement: None,
    },
    // --- Commonly confused words -------------------------------------------
    Rule {
        pattern: &[Piece::Word("alot")],
        category: "Commonly confused words",
        replacement: Some("a lot"),
    },
    Rule {
        pattern: &[Piece::Word("their"), Piece::AnyOf(&["is", "are", "was", "were"])],
        category: "Commonly confused words",
        replacement: Some("there $2"),
    },
    Rule {
        pattern: &[Piece::Word("your"), Piece::Word("welcome")],
        category: "Commonly confused words",
        replacement: Some("you're welcome"),
    },
    Rule {
        pattern: &[
            Piece::AnyOf(&[
                "better", "worse", "more", "less", "rather", "other", "bigger", "smaller", "fewer",
                "greater", "higher", "lower", "later", "earlier", "faster", "slower", "longer",
                "shorter", "larger",
            ]),
            Piece::Word("then"),
        ],
        category: "Commonly confused words",
        replacement: Some("$1 than"),
    },
    Rule {
        pattern: &[Piece::Word("irregardless")],
        category: "Word choice",
        replacement: Some("regardless"),
    },
    Rule {
        pattern: &[
            Piece::Word("for"),
            Piece::Word("all"),
            Piece::Word("intensive"),
            Piece::Word("purposes"),
        ],
        category: "Commonly confused words",
        replacement: Some("for all intents and purposes"),
    },
    Rule {
        pattern: &[Piece::Word("could"), Piece::Word("care"), Piece::Word("less")],
        category: "Commonly confused words",
        replacement: Some("couldn't care less"),
    },
    Rule {
        pattern: &[Piece::Word("should"), Piece::Word("of"), Piece::Word("went")],
        category: "Verb form",
        replacement: Some("should have gone"),
    },
    // --- Capitalization ----------------------------------------------------
    Rule { pattern: &[Piece::Word("i")], category: "Capitalization", replacement: Some("I") },
];

/// One word of the text, and where it is.
#[derive(Clone, Copy, Debug)]
struct Token<'a> {
    text: &'a str,
    start: usize,
    end: usize,
    /// Whether punctuation stands between this word and the one before, which
    /// is where a run of words ends as far as any rule is concerned.
    after_punctuation: bool,
}

/// Every mistake the rules find in a text.
#[must_use]
pub fn check(text: &str) -> Vec<Finding> {
    let tokens = tokens_of(text);
    let mut out: Vec<Finding> = Vec::new();

    for at in 0..tokens.len() {
        for rule in ENGLISH {
            let Some(matched) = matches_at(rule, &tokens, at) else { continue };
            let start = tokens[at].start;
            let end = tokens[at + matched - 1].end;
            // One finding per place: the first rule to speak decides, which
            // keeps a double negative from also being a verb form.
            if out.iter().any(|found| found.start < end && start < found.end) {
                continue;
            }
            let replacement = rule.replacement.map(|template| {
                let words: Vec<&str> =
                    tokens[at..at + matched].iter().map(|token| token.text).collect();
                fill(template, &words, &text[start..end])
            });
            // A replacement that is what is there already is no finding: the
            // rule for "i" fires on "I", and "I" is right.
            if replacement.as_deref() == Some(&text[start..end]) {
                continue;
            }
            out.push(Finding { start, end, category: rule.category, replacement });
        }
    }
    out
}

/// Whether a rule matches the run of words beginning at a token, and how many
/// words it took.
fn matches_at(rule: &Rule, tokens: &[Token<'_>], at: usize) -> Option<usize> {
    for (offset, piece) in rule.pattern.iter().enumerate() {
        let token = tokens.get(at + offset)?;
        if offset > 0 && token.after_punctuation {
            return None;
        }
        let word = token.text;
        let fits = match piece {
            Piece::Word(wanted) => word.eq_ignore_ascii_case(wanted),
            Piece::AnyOf(wanted) => wanted.iter().any(|one| word.eq_ignore_ascii_case(one)),
            Piece::Any => true,
            Piece::VowelSound => vowel_sound(word) == Some(true),
            Piece::ConsonantSound => vowel_sound(word) == Some(false),
        };
        if !fits {
            return None;
        }
    }
    Some(rule.pattern.len())
}

/// Fills a replacement template in, keeping the case the writer used at the
/// front of what is replaced.
fn fill(template: &str, words: &[&str], original: &str) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(at) = rest.find('$') {
        out.push_str(&rest[..at]);
        let digits: String = rest[at + 1..].chars().take_while(char::is_ascii_digit).collect();
        match digits.parse::<usize>() {
            Ok(number) if number >= 1 && number <= words.len() => {
                out.push_str(words[number - 1]);
                rest = &rest[at + 1 + digits.len()..];
            }
            _ => {
                out.push('$');
                rest = &rest[at + 1..];
            }
        }
    }
    out.push_str(rest);

    // A sentence that began "A apple" begins "An apple", not "an apple".
    let capital = original.chars().next().is_some_and(char::is_uppercase);
    if capital && !out.starts_with(|character: char| character.is_uppercase()) {
        let mut letters = out.chars();
        if let Some(first) = letters.next() {
            out = first.to_uppercase().collect::<String>() + letters.as_str();
        }
    }
    out
}

/// Whether a word begins with a vowel sound: `Some(true)` for "apple" and
/// "hour", `Some(false)` for "house" and "university", `None` where it cannot
/// be told and no rule should fire.
fn vowel_sound(word: &str) -> Option<bool> {
    let lower = word.to_lowercase();
    let first = lower.chars().next()?;

    // A letter read out by its name: "an F", "a U". Only where the whole
    // word is capitals, which is what an initialism looks like.
    if word.len() <= 4 && word.chars().all(|c| c.is_ascii_uppercase()) {
        return Some(matches!(
            first,
            'a' | 'e' | 'f' | 'h' | 'i' | 'l' | 'm' | 'n' | 'o' | 'r' | 's' | 'x'
        ));
    }
    // A number: "an 8", "an 11", "a 12".
    if first.is_ascii_digit() {
        return Some(lower.starts_with('8') || lower.starts_with("11") || lower.starts_with("18"));
    }
    if !first.is_ascii_alphabetic() {
        return None;
    }

    // The h that is not sounded.
    if first == 'h' {
        const SILENT: &[&str] = &["hour", "honest", "honor", "honour", "heir", "herb", "homage"];
        return Some(SILENT.iter().any(|silent| lower.starts_with(silent)));
    }
    // The u and the e and the o that begin with a y or a w.
    if first == 'u' {
        const YOU: &[&str] = &[
            "uni", "use", "usu", "usa", "ute", "uti", "ura", "uri", "uro", "ubi", "uku", "una",
            "ure", "url",
        ];
        return Some(!YOU.iter().any(|you| lower.starts_with(you)));
    }
    if first == 'e' {
        return Some(!lower.starts_with("eu") && !lower.starts_with("ewe"));
    }
    if first == 'o' {
        return Some(!lower.starts_with("one") && !lower.starts_with("once"));
    }
    Some(matches!(first, 'a' | 'i'))
}

/// The words of a text, with where each is and whether punctuation came
/// before it.
fn tokens_of(text: &str) -> Vec<Token<'_>> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    let mut punctuation_since_last = false;

    let inside = |character: char| {
        character.is_alphanumeric() || character == '\'' || character == '\u{2019}'
    };
    for (at, character) in text.char_indices() {
        match (inside(character), start) {
            (true, None) => start = Some(at),
            (false, Some(from)) => {
                out.push(Token {
                    text: &text[from..at],
                    start: from,
                    end: at,
                    after_punctuation: punctuation_since_last,
                });
                start = None;
                punctuation_since_last = !character.is_whitespace();
            }
            (false, None) => {
                if !character.is_whitespace() {
                    punctuation_since_last = true;
                }
            }
            (true, Some(_)) => {}
        }
    }
    if let Some(from) = start {
        out.push(Token {
            text: &text[from..],
            start: from,
            end: text.len(),
            after_punctuation: punctuation_since_last,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(text: &str) -> Vec<(String, &'static str, Option<String>)> {
        check(text)
            .into_iter()
            .map(|finding| {
                (text[finding.start..finding.end].to_owned(), finding.category, finding.replacement)
            })
            .collect()
    }

    #[test]
    fn a_before_a_vowel_sound_is_an() {
        assert_eq!(
            found("I ate a apple."),
            vec![("a apple".to_owned(), "Article use", Some("an apple".to_owned()))]
        );
        assert_eq!(found("an hour"), vec![]);
        assert_eq!(
            found("a hour"),
            vec![("a hour".to_owned(), "Article use", Some("an hour".to_owned()))]
        );
    }

    #[test]
    fn an_before_a_consonant_sound_is_a() {
        assert_eq!(
            found("an house"),
            vec![("an house".to_owned(), "Article use", Some("a house".to_owned()))]
        );
        // The u that begins with a y, and the e that begins with a y, and the
        // o that begins with a w: a university, a European, a one-off.
        assert_eq!(found("a university"), vec![]);
        assert_eq!(found("a European"), vec![]);
        assert_eq!(found("a one-off"), vec![]);
        assert_eq!(
            found("an university"),
            vec![("an university".to_owned(), "Article use", Some("a university".to_owned()))]
        );
    }

    #[test]
    fn a_letter_read_by_its_name_takes_the_article_its_name_takes() {
        assert_eq!(found("an FBI agent"), vec![]);
        assert_eq!(found("a FBI agent").len(), 1);
        assert_eq!(found("a UN resolution"), vec![]);
    }

    #[test]
    fn could_of_is_could_have() {
        assert_eq!(
            found("I could of gone."),
            vec![("could of".to_owned(), "Verb form", Some("could have".to_owned()))]
        );
        assert_eq!(
            found("We should of known"),
            vec![("should of".to_owned(), "Verb form", Some("should have".to_owned()))]
        );
    }

    #[test]
    fn a_pronoun_takes_the_verb_that_goes_with_it() {
        assert_eq!(found("He don't care.")[0].2, Some("He doesn't".to_owned()));
        assert_eq!(found("they was there")[0].2, Some("they were".to_owned()));
        assert_eq!(found("She has left."), vec![]);
    }

    #[test]
    fn a_double_negative_is_pointed_out_but_not_rewritten() {
        let findings = found("I don't know nothing about it.");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].1, "Double negation");
        assert_eq!(findings[0].2, None);
        assert_eq!(found("I can't get no satisfaction").len(), 1);
    }

    #[test]
    fn punctuation_ends_a_run_of_words() {
        // "I don't. No." is two sentences and not a double negative.
        assert_eq!(found("I don't. No."), vec![]);
        assert_eq!(found("Could, of course, be."), vec![]);
    }

    #[test]
    fn the_replacement_keeps_the_case_of_what_it_replaces() {
        assert_eq!(found("A apple fell.")[0].2, Some("An apple".to_owned()));
        assert_eq!(found("Their is a way")[0].2, Some("There is".to_owned()));
    }

    #[test]
    fn the_pronoun_i_is_a_capital() {
        assert_eq!(found("and then i left")[0].2, Some("I".to_owned()));
        assert_eq!(found("and then I left"), vec![], "a capital I is right already");
    }

    #[test]
    fn a_word_that_is_a_mistake_on_its_own() {
        assert_eq!(found("alot of them")[0].2, Some("a lot".to_owned()));
        assert_eq!(found("Irregardless of that")[0].2, Some("Regardless".to_owned()));
    }

    #[test]
    fn one_finding_per_place() {
        // "he don't know nothing" is a verb form and a double negative in the
        // same three words; the first rule to speak decides.
        let findings = found("he don't know nothing");
        assert_eq!(findings.len(), 1);
    }
}
