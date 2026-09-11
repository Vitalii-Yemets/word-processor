//! Correcting what was typed, as it is typed.
//!
//! # One mechanism, several tables
//!
//! Almost everything Word's AutoCorrect does is the same thing: watch the word
//! that was just finished, and if it is one of a known set, put something else
//! in its place. The replacement list is the obvious case — "teh" becomes
//! "the" — and so are the capital letters: a word typed as "TWo" is a word
//! whose first two letters are capitals, and the rule replaces it with "Two".
//!
//! What is not a word replacement is a character replacement: a straight quote
//! becomes a curly one the instant it is typed, because which way it curls
//! depends on what is in front of it and that is known at once. A hyphen
//! between two words becomes a dash, and "1st" becomes "1ˢᵗ".
//!
//! # Why the list is ours and not Word's
//!
//! Word ships about nine hundred replacements in a file of its own. That list
//! is Microsoft's; this one is written here, and is the handful of misspellings
//! that are worth catching without being wrong about anybody's name. Anyone can
//! add to it, and what they add is kept in the settings file beside the program
//! — it is about the person, not about the document.

use std::collections::{BTreeMap, BTreeSet};

/// The days, which Word capitalises whatever they are typed as.
const DAYS: &[&str] = &[
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
    "january",
    "february",
    "march",
    "april",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
];

/// The replacements a new settings file starts with.
///
/// Common misspellings of common words, and nothing whose "correction" could be
/// somebody's name or a word in another language: a program that quietly
/// rewrote a surname would be worse than one that corrected nothing.
const USUAL: &[(&str, &str)] = &[
    ("teh", "the"),
    ("adn", "and"),
    ("taht", "that"),
    ("thier", "their"),
    ("recieve", "receive"),
    ("seperate", "separate"),
    ("definately", "definitely"),
    ("occured", "occurred"),
    ("untill", "until"),
    ("wich", "which"),
    ("becuase", "because"),
    ("tommorow", "tomorrow"),
    ("alot", "a lot"),
    ("dont", "don't"),
    ("cant", "can't"),
    ("wont", "won't"),
    ("isnt", "isn't"),
    ("wasnt", "wasn't"),
    ("didnt", "didn't"),
    ("youre", "you're"),
    ("theyre", "they're"),
    ("its a", "it's a"),
];

/// The abbreviations a full stop does not end a sentence after.
///
/// Without these, "see e.g. the table" becomes "see e.g. The table", because a
/// full stop and a space is what the end of a sentence looks like. Word keeps
/// the same list, under Exceptions ▸ First Letter, and lets it be added to.
const ABBREVIATIONS: &[&str] = &[
    "al.", "apr.", "assn.", "aug.", "co.", "corp.", "dec.", "dept.", "dr.", "e.g.", "eq.", "etc.",
    "feb.", "fig.", "figs.", "i.e.", "inc.", "jan.", "jr.", "jul.", "jun.", "ltd.", "mar.", "mr.",
    "mrs.", "ms.", "mt.", "no.", "nov.", "oct.", "p.", "pp.", "prof.", "sept.", "sr.", "st.",
    "vol.", "vs.",
];

/// The words whose first two capitals are meant.
///
/// "CDs" is not a finger left on the shift key. Word keeps this list too, under
/// Exceptions ▸ INitial CAps. A word of three capitals or more is left alone by
/// the rule itself and needs no exception.
const TWO_CAPITALS: &[&str] = &["CDs", "GBs", "IDs", "KBs", "MBs", "PCs", "TVs"];

/// The fractions that have a character of their own.
const FRACTIONS: &[(&str, char)] =
    &[("1/2", '½'), ("1/4", '¼'), ("3/4", '¾'), ("1/3", '⅓'), ("2/3", '⅔')];

/// Which corrections are made.
///
/// Word's four tabs, less the two that are about things this program does not
/// have: AutoFormat (which reformats a whole document at once) and Actions
/// (which offers to look a name up in an address book).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutoCorrect {
    /// What is typed, and what goes in its place.
    pub replacements: BTreeMap<String, String>,
    /// The abbreviations a sentence does not begin after. Word's Exceptions ▸
    /// First Letter.
    pub first_letter: BTreeSet<String>,
    /// The words allowed to begin with two capitals. Word's Exceptions ▸
    /// INitial CAps.
    pub initial_caps: BTreeSet<String>,

    // --- The AutoCorrect tab ------------------------------------------------
    /// TWo initial capitals become one.
    pub two_initials: bool,
    /// The first letter of a sentence is capitalised.
    pub sentence_case: bool,
    /// The days of the week and the months are capitalised.
    pub day_names: bool,
    /// A word typed with the shift key and the caps lock both on — `tHE` — is
    /// turned back the right way up.
    pub caps_lock: bool,
    /// Whether the replacement list is used at all.
    pub replace_text: bool,

    // --- The AutoFormat As You Type tab -------------------------------------
    /// Straight quotes become curly ones, curling the way the text around them
    /// asks for.
    pub curly_quotes: bool,
    /// `1st` becomes `1ˢᵗ`.
    pub ordinals: bool,
    /// `1/2` becomes `½`.
    pub fractions: bool,
    /// A hyphen between two words becomes a dash.
    pub dashes: bool,
    /// A line begun with `- ` or `1. ` becomes a list.
    pub automatic_lists: bool,
}

impl Default for AutoCorrect {
    /// Everything on, which is how Word arrives.
    fn default() -> Self {
        Self {
            replacements: USUAL
                .iter()
                .map(|(what, with)| ((*what).to_owned(), (*with).to_owned()))
                .collect(),
            first_letter: ABBREVIATIONS.iter().map(|word| (*word).to_owned()).collect(),
            initial_caps: TWO_CAPITALS.iter().map(|word| (*word).to_owned()).collect(),
            two_initials: true,
            sentence_case: true,
            day_names: true,
            caps_lock: true,
            replace_text: true,
            curly_quotes: true,
            ordinals: true,
            fractions: true,
            dashes: true,
            automatic_lists: true,
        }
    }
}

/// What a correction does to the text before the caret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Correction {
    /// How many characters before the caret to take away.
    pub taking: usize,
    /// What to put in their place.
    pub putting: String,
}

impl AutoCorrect {
    /// The correction for the word just finished, if there is one.
    ///
    /// `before` is the text of the paragraph up to the caret, and the word is
    /// whatever follows the last space in it. The boundary character — the
    /// space or the full stop that finished the word — is not part of it and is
    /// typed afterwards.
    #[must_use]
    pub fn on_word(&self, before: &str) -> Option<Correction> {
        let word = last_word(before);
        if word.is_empty() {
            return None;
        }

        // The list first, because a person who put a word in it meant it to
        // win over any rule.
        if self.replace_text {
            if let Some(with) = self.replacements.get(&word.to_lowercase()) {
                return Some(Correction {
                    taking: word.chars().count(),
                    putting: matched_case(word, with),
                });
            }
        }

        if self.day_names && DAYS.contains(&word.to_lowercase().as_str()) {
            let capitalised = capitalise(word);
            if capitalised != word {
                return Some(Correction { taking: word.chars().count(), putting: capitalised });
            }
        }

        // TWo initial capitals, which is what a finger left on the shift key
        // makes. Not applied to a word that is all capitals: that is an
        // abbreviation, and Word leaves those alone too.
        if self.two_initials && !self.initial_caps.contains(word) {
            let letters: Vec<char> = word.chars().collect();
            let two_capitals = letters.len() > 2
                && letters[0].is_uppercase()
                && letters[1].is_uppercase()
                && letters[2..].iter().any(|letter| letter.is_lowercase())
                && letters[2..].iter().all(|letter| !letter.is_uppercase());
            if two_capitals {
                let mut fixed = String::new();
                fixed.extend(letters[0].to_uppercase());
                fixed.extend(letters[1].to_lowercase());
                fixed.extend(letters[2..].iter());
                return Some(Correction { taking: letters.len(), putting: fixed });
            }
        }

        // The caps lock left on with the shift key held: `tHE` for `The`.
        if self.caps_lock {
            let letters: Vec<char> = word.chars().collect();
            let inverted = letters.len() > 1
                && letters[0].is_lowercase()
                && letters[1..].iter().all(|letter| letter.is_uppercase());
            if inverted {
                return Some(Correction {
                    taking: letters.len(),
                    putting: capitalise(&word.to_lowercase()),
                });
            }
        }

        if self.sentence_case {
            if let Some(fixed) = sentence_capital(before, word, &self.first_letter) {
                return Some(Correction { taking: word.chars().count(), putting: fixed });
            }
        }

        if self.ordinals {
            if let Some(putting) = ordinal(word) {
                return Some(Correction { taking: word.chars().count(), putting });
            }
        }

        if self.fractions {
            if let Some((_, mark)) = FRACTIONS.iter().find(|(text, _)| *text == word) {
                return Some(Correction {
                    taking: word.chars().count(),
                    putting: mark.to_string(),
                });
            }
        }
        None
    }

    /// What a typed character should be instead, if it should be something
    /// else.
    ///
    /// The quotes, which curl by what is in front of them, and the hyphen
    /// between two words, which becomes a dash. Both are decided the moment the
    /// character is typed rather than at the end of the word, because both are
    /// about the character and not about the word.
    #[must_use]
    pub fn on_character(&self, before: &str, typed: char) -> Option<char> {
        let previous = before.chars().last();
        match typed {
            '"' if self.curly_quotes => {
                Some(if opens_a_quote(previous) { '\u{201C}' } else { '\u{201D}' })
            }
            '\'' if self.curly_quotes => {
                Some(if opens_a_quote(previous) { '\u{2018}' } else { '\u{2019}' })
            }
            _ => None,
        }
    }

    /// Whether a hyphen surrounded by words should become a dash.
    ///
    /// Word turns `a - b` into `a – b` when the space after the hyphen is
    /// typed, which is the moment it can tell a dash from a hyphenated word.
    #[must_use]
    pub fn dash_before(&self, before: &str) -> Option<Correction> {
        if !self.dashes {
            return None;
        }
        // The shape being looked for is "word space hyphen", the space after
        // which has just been typed.
        let mut letters = before.chars().rev();
        if letters.next() != Some('-') {
            return None;
        }
        if letters.next() != Some(' ') {
            return None;
        }
        letters.next().filter(|letter| !letter.is_whitespace())?;
        Some(Correction { taking: 1, putting: "\u{2013}".to_owned() })
    }
}

/// The word before the caret: everything after the last space.
#[must_use]
fn last_word(before: &str) -> &str {
    let start = before
        .char_indices()
        .rev()
        .find(|(_, letter)| letter.is_whitespace() || *letter == '\u{a0}')
        .map_or(0, |(at, letter)| at + letter.len_utf8());
    &before[start..]
}

/// Whether a quote typed after this character opens rather than closes.
fn opens_a_quote(previous: Option<char>) -> bool {
    match previous {
        // Nothing before it, or a space, or an opening bracket: it opens.
        None => true,
        Some(letter) => letter.is_whitespace() || matches!(letter, '(' | '[' | '{' | '\u{201C}'),
    }
}

/// The same word with its first letter a capital.
fn capitalise(word: &str) -> String {
    let mut letters = word.chars();
    match letters.next() {
        Some(first) => first.to_uppercase().chain(letters).collect(),
        None => String::new(),
    }
}

/// A replacement written the way the word it replaces was.
///
/// A person who types "TEH" in a heading of capitals means "THE", and one who
/// starts a sentence with "Teh" means "The". Only those two cases: anything
/// else goes in as the list has it, because the list may be a person's own
/// shorthand for something that is written a particular way.
fn matched_case(word: &str, with: &str) -> String {
    let letters: Vec<char> = word.chars().collect();
    if letters.len() > 1 && letters.iter().all(|letter| letter.is_uppercase()) {
        return with.to_uppercase();
    }
    if letters.first().is_some_and(|first| first.is_uppercase()) {
        return capitalise(with);
    }
    with.to_owned()
}

/// The first letter of a sentence, capitalised.
///
/// A sentence starts at the beginning of a paragraph or after a full stop, a
/// question mark or an exclamation mark. Nothing is done to a word that is
/// already capitalised, nor to one that follows an abbreviation: `exceptions`
/// is the list of full stops that end a word rather than a sentence.
fn sentence_capital(before: &str, word: &str, exceptions: &BTreeSet<String>) -> Option<String> {
    let first = word.chars().next()?;
    if !first.is_lowercase() {
        return None;
    }

    // What comes before the word, less the space that separates them. A word is
    // whatever follows the last space, so this either ends in a space or is the
    // whole of an empty beginning.
    let ahead = before[..before.len() - word.len()].trim_end();
    if ahead.is_empty() {
        // The first word of the paragraph.
        return Some(capitalise(word));
    }
    if !ahead.ends_with(['.', '?', '!']) {
        return None;
    }
    // A full stop that ends an abbreviation does not end a sentence.
    if exceptions.contains(&last_word(ahead).to_lowercase()) {
        return None;
    }
    Some(capitalise(word))
}

/// `1st` and its kin, with the ending raised.
///
/// The raised letters are real characters rather than a superscript run,
/// because a correction that changed the formatting of what follows it would
/// keep raising everything typed after it.
fn ordinal(word: &str) -> Option<String> {
    let digits: String = word.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    let ending = &word[digits.len()..];
    let raised = match ending.to_lowercase().as_str() {
        "st" => "\u{02E2}\u{1D57}",
        "nd" => "\u{207F}\u{1D48}",
        "rd" => "\u{02B3}\u{1D48}",
        "th" => "\u{1D57}\u{02B0}",
        _ => return None,
    };
    // Only where the number agrees with the ending: "2st" is a typing mistake,
    // not an ordinal, and raising it would make the mistake look deliberate.
    let number: u32 = digits.parse().ok()?;
    let wanted = match (number % 100, number % 10) {
        (11..=13, _) => "th",
        (_, 1) => "st",
        (_, 2) => "nd",
        (_, 3) => "rd",
        _ => "th",
    };
    if ending.to_lowercase() != wanted {
        return None;
    }
    Some(format!("{digits}{raised}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules() -> AutoCorrect {
        AutoCorrect::default()
    }

    #[test]
    fn the_word_before_the_caret_is_what_is_looked_at() {
        assert_eq!(last_word("one two teh"), "teh");
        assert_eq!(last_word("teh"), "teh");
        assert_eq!(last_word("one "), "");
    }

    #[test]
    fn a_word_on_the_list_is_replaced() {
        let correction = rules().on_word("I think teh").expect("a correction");
        assert_eq!(correction.taking, 3);
        assert_eq!(correction.putting, "the");
    }

    #[test]
    fn a_replacement_is_written_the_way_the_word_was() {
        assert_eq!(rules().on_word("Teh").expect("a correction").putting, "The");
        assert_eq!(rules().on_word("say TEH").expect("a correction").putting, "THE");
    }

    #[test]
    fn nothing_on_the_list_is_left_alone() {
        // Mid-sentence, because the first word of one gets a capital whatever
        // else is or is not done to it.
        assert_eq!(rules().on_word("an ordinary"), None);
    }

    #[test]
    fn two_initial_capitals_become_one() {
        let correction = rules().on_word("TWo").expect("a correction");
        assert_eq!(correction.putting, "Two");
    }

    #[test]
    fn a_word_of_capitals_is_an_abbreviation_and_is_left_alone() {
        // "BBC" is not a finger left on the shift key.
        assert_eq!(rules().on_word("BBC"), None);
        assert_eq!(rules().on_word("PDF"), None);
    }

    #[test]
    fn a_day_is_capitalised() {
        assert_eq!(rules().on_word("monday").expect("a correction").putting, "Monday");
        // And one already capitalised is not corrected to itself.
        assert_eq!(rules().on_word("Monday"), None);
    }

    #[test]
    fn a_word_typed_with_the_caps_lock_on_is_turned_back_up() {
        assert_eq!(rules().on_word("tHE").expect("a correction").putting, "The");
    }

    #[test]
    fn the_first_word_of_a_sentence_gets_a_capital() {
        assert_eq!(rules().on_word("hello").expect("a correction").putting, "Hello");
        assert_eq!(rules().on_word("One. two").expect("a correction").putting, "Two");
    }

    #[test]
    fn a_decimal_point_does_not_start_a_sentence() {
        assert_eq!(rules().on_word("it is 3.14"), None);
        assert_eq!(rules().on_word("it is 3.14 and"), None);
    }

    #[test]
    fn an_abbreviation_does_not_start_a_sentence() {
        assert_eq!(rules().on_word("see e.g. the"), None);
        assert_eq!(rules().on_word("ask Mr. smith"), None);
        // But a full stop that is not on the list does end one.
        assert_eq!(rules().on_word("it ended. then").expect("a correction").putting, "Then");
    }

    #[test]
    fn a_word_allowed_two_capitals_keeps_them() {
        assert_eq!(rules().on_word("two CDs"), None);
        // And one that is not on the list is still corrected.
        assert_eq!(rules().on_word("TWo").expect("a correction").putting, "Two");
    }

    #[test]
    fn a_quote_curls_the_way_what_is_in_front_of_it_asks() {
        let rules = rules();
        assert_eq!(rules.on_character("", '"'), Some('\u{201C}'));
        assert_eq!(rules.on_character("he said ", '"'), Some('\u{201C}'));
        assert_eq!(rules.on_character("he said \u{201C}yes", '"'), Some('\u{201D}'));
        assert_eq!(rules.on_character("it", '\''), Some('\u{2019}'));
    }

    #[test]
    fn a_hyphen_between_words_becomes_a_dash() {
        let correction = rules().dash_before("one -").expect("a dash");
        assert_eq!(correction.taking, 1);
        assert_eq!(correction.putting, "\u{2013}");

        // A hyphenated word is not a dash, and neither is a hyphen at the
        // start of a line.
        assert_eq!(rules().dash_before("well-"), None);
        assert_eq!(rules().dash_before(" -"), None);
    }

    #[test]
    fn an_ordinal_is_raised_only_where_the_ending_fits_the_number() {
        assert!(rules().on_word("1st").is_some());
        assert!(rules().on_word("22nd").is_some());
        assert!(rules().on_word("11th").is_some());
        // The ones that are typing mistakes rather than ordinals.
        assert_eq!(rules().on_word("2st"), None);
        assert_eq!(rules().on_word("11st"), None);
    }

    #[test]
    fn a_fraction_becomes_the_character_for_it() {
        assert_eq!(rules().on_word("1/2").expect("a fraction").putting, "½");
    }

    #[test]
    fn every_rule_can_be_switched_off() {
        let off = AutoCorrect {
            two_initials: false,
            sentence_case: false,
            day_names: false,
            caps_lock: false,
            replace_text: false,
            curly_quotes: false,
            ordinals: false,
            fractions: false,
            dashes: false,
            automatic_lists: false,
            ..AutoCorrect::default()
        };
        assert_eq!(off.on_word("teh"), None);
        assert_eq!(off.on_word("TWo"), None);
        assert_eq!(off.on_word("monday"), None);
        assert_eq!(off.on_word("tHE"), None);
        assert_eq!(off.on_word("hello"), None);
        assert_eq!(off.on_word("1st"), None);
        assert_eq!(off.on_word("1/2"), None);
        assert_eq!(off.on_character("", '"'), None);
        assert_eq!(off.dash_before("one -"), None);
    }
}
