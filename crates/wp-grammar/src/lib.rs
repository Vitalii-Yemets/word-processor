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
//! that begins with a vowel sound, a word one of the language's own tests says
//! yes to. A rule matches a run of words with nothing but spaces between them
//! — punctuation ends the run, because "I don't. No." is not a double
//! negative. Each rule carries what to say about the match, which is the name
//! Word gives the mistake, and what to put in its place where there is one
//! right answer. And each carries an example of the mistake it is for, which
//! is how a reader of the list sees what it does and how the tests hold every
//! rule to finding it.
//!
//! # Which languages
//!
//! English, Russian, German and French, each a list of its own in a module of
//! its own: the engine is the same for every language and the mistakes are
//! not. A rule is only ever a mistake in the language it was written for —
//! "seid Jahren" is German and wrong, and means nothing in English — so a text
//! is checked by the rules of the language it says it is in, and a language
//! with no list is not checked at all.

#![forbid(unsafe_code)]

mod english;
mod french;
mod german;
mod russian;

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
    /// This word, in any case. Written in small letters, with a straight
    /// apostrophe standing for either.
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
    /// The first word of one of these pairs, and `%1` in the replacement the
    /// second word of the pair the first word of the match was: how "ложил"
    /// becomes "клал" and "einzigste" becomes "einzige" without a rule for
    /// each.
    Swap(&'static [(&'static str, &'static str)]),
    /// A word this test says yes to, given the word as it is written.
    Test(fn(&str) -> bool),
}

/// One rule: what to look for, what to call it, what to put instead.
#[derive(Clone, Copy, Debug)]
struct Rule {
    pattern: &'static [Piece],
    category: &'static str,
    /// A template for the replacement, where `$1` is the first word matched
    /// as it was written, and so on, and `%1` is it as the rule has it: what
    /// a [`Piece::Swap`] puts in its place, or the word in small letters —
    /// which is what a word joined onto another wants, "Das Selbe" being
    /// "Dasselbe". `None` where the fix is the writer's to choose.
    replacement: Option<&'static str>,
    /// A sentence with the mistake in it: for a reader of the list, and for
    /// the tests, which hold every rule to finding it.
    #[cfg_attr(not(test), allow(dead_code))]
    example: &'static str,
}

/// The rules for a language, by its tag — `en-GB`, `ru`, `de-AT` — of which
/// only the language counts: the mistakes are the same in every country that
/// writes it.
fn rules_for(language: &str) -> &'static [Rule] {
    let primary = language.split(['-', '_']).next().unwrap_or("").to_ascii_lowercase();
    match primary.as_str() {
        "en" => english::RULES,
        "ru" => russian::RULES,
        "de" => german::RULES,
        "fr" => french::RULES,
        _ => &[],
    }
}

/// Whether there are rules for a language at all.
#[must_use]
pub fn has_rules(language: &str) -> bool {
    !rules_for(language).is_empty()
}

/// Every name a finding can be given, in every language: what a translator
/// of the interface has to say in theirs.
#[must_use]
pub fn categories() -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for rules in [english::RULES, russian::RULES, german::RULES, french::RULES] {
        for rule in rules {
            if !out.contains(&rule.category) {
                out.push(rule.category);
            }
        }
    }
    out
}

/// One word of the text, and where it is.
#[derive(Clone, Debug)]
struct Token<'a> {
    text: &'a str,
    /// The word in small letters and with a straight apostrophe, which is
    /// what the patterns are written in: a word processor types the curly one
    /// and a person may type either.
    folded: String,
    start: usize,
    end: usize,
    /// Whether punctuation stands between this word and the one before, which
    /// is where a run of words ends as far as any rule is concerned.
    after_punctuation: bool,
}

/// Every mistake the rules for a language find in a text.
#[must_use]
pub fn check(text: &str, language: &str) -> Vec<Finding> {
    let rules = rules_for(language);
    if rules.is_empty() {
        return Vec::new();
    }
    let tokens = tokens_of(text);
    let mut out: Vec<Finding> = Vec::new();

    for at in 0..tokens.len() {
        for rule in rules {
            let Some(matched) = matches_at(rule, &tokens, at) else { continue };
            let start = tokens[at].start;
            let end = tokens[at + matched - 1].end;
            // One finding per place: the first rule to speak decides, which
            // keeps a double negative from also being a verb form.
            if out.iter().any(|found| found.start < end && start < found.end) {
                continue;
            }
            let replacement = rule
                .replacement
                .map(|template| fill(template, rule, &tokens[at..at + matched], &text[start..end]));
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
        let word = token.folded.as_str();
        let fits = match piece {
            Piece::Word(wanted) => word == *wanted,
            Piece::AnyOf(wanted) => wanted.contains(&word),
            Piece::Any => true,
            Piece::VowelSound => vowel_sound(token.text) == Some(true),
            Piece::ConsonantSound => vowel_sound(token.text) == Some(false),
            Piece::Swap(pairs) => pairs.iter().any(|(from, _)| *from == word),
            Piece::Test(test) => test(token.text),
        };
        if !fits {
            return None;
        }
    }
    Some(rule.pattern.len())
}

/// Fills a replacement template in, keeping the case the writer used at the
/// front of what is replaced and the apostrophe they typed.
fn fill(template: &str, rule: &Rule, tokens: &[Token<'_>], original: &str) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(at) = rest.find(['$', '%']) {
        out.push_str(&rest[..at]);
        let swapped = rest[at..].starts_with('%');
        let digits: String = rest[at + 1..].chars().take_while(char::is_ascii_digit).collect();
        let word = digits
            .parse::<usize>()
            .ok()
            .filter(|number| (1..=tokens.len()).contains(number))
            .map(|number| number - 1)
            .and_then(|index| {
                if !swapped {
                    return Some(tokens[index].text.to_owned());
                }
                let Some(Piece::Swap(pairs)) = rule.pattern.get(index) else {
                    return Some(tokens[index].folded.clone());
                };
                pairs
                    .iter()
                    .find(|(from, _)| *from == tokens[index].folded)
                    .map(|(_, to)| (*to).to_owned())
            });
        match word {
            Some(word) => {
                out.push_str(&word);
                rest = &rest[at + 1 + digits.len()..];
            }
            None => {
                out.push_str(&rest[at..=at]);
                rest = &rest[at + 1..];
            }
        }
    }
    out.push_str(rest);

    // The apostrophe the writer typed. A correction that brings one of its
    // own — French's "d’abord", English's "doesn't" — brings it in the
    // writer's shape where the words replaced had one to go by.
    if original.contains('\u{2019}') {
        out = out.replace('\'', "\u{2019}");
    } else if original.contains('\'') {
        out = out.replace('\u{2019}', "'");
    }

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
    let token = |from: usize, to: usize, after_punctuation: bool| Token {
        text: &text[from..to],
        folded: text[from..to].to_lowercase().replace('\u{2019}', "'"),
        start: from,
        end: to,
        after_punctuation,
    };
    for (at, character) in text.char_indices() {
        match (inside(character), start) {
            (true, None) => start = Some(at),
            (false, Some(from)) => {
                out.push(token(from, at, punctuation_since_last));
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
        out.push(token(from, text.len(), punctuation_since_last));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(text: &str) -> Vec<(String, &'static str, Option<String>)> {
        found_in(text, "en")
    }

    fn found_in(text: &str, language: &str) -> Vec<(String, &'static str, Option<String>)> {
        check(text, language)
            .into_iter()
            .map(|finding| {
                (text[finding.start..finding.end].to_owned(), finding.category, finding.replacement)
            })
            .collect()
    }

    /// Every rule of every language, with the tag it is checked under.
    fn every_rule() -> Vec<(&'static str, &'static Rule)> {
        [
            ("en", english::RULES),
            ("ru", russian::RULES),
            ("de", german::RULES),
            ("fr", french::RULES),
        ]
        .into_iter()
        .flat_map(|(language, rules)| rules.iter().map(move |rule| (language, rule)))
        .collect()
    }

    #[test]
    fn every_rule_finds_the_mistake_it_is_for() {
        // Each rule's own example, checked with the whole list: a rule that
        // matches its example only for another rule to have spoken first is a
        // rule that never fires, and this is where that shows.
        for (language, rule) in every_rule() {
            let tokens = tokens_of(rule.example);
            let place = (0..tokens.len())
                .find_map(|at| matches_at(rule, &tokens, at).map(|length| (at, length)));
            let Some((at, length)) = place else {
                panic!("{language}: the rule for {:?} does not match its own example", rule.example)
            };
            let (start, end) = (tokens[at].start, tokens[at + length - 1].end);
            let findings = check(rule.example, language);
            assert!(
                findings.iter().any(|finding| finding.start == start
                    && finding.end == end
                    && finding.category == rule.category),
                "{language}: {:?} is not found as {:?} — another rule spoke first: {findings:?}",
                rule.example,
                rule.category,
            );
        }
    }

    #[test]
    fn a_correction_is_not_itself_a_mistake() {
        // The example with every correction made has nothing left to find.
        // Where a rule offers none — a double negative, which half to keep
        // being the writer's to say — there is nothing to hold it to.
        for (language, rule) in
            every_rule().into_iter().filter(|(_, rule)| rule.replacement.is_some())
        {
            let corrected = corrected(rule.example, language);
            assert_eq!(
                check(&corrected, language),
                vec![],
                "{language}: {:?} corrected to {corrected:?} is still found wanting",
                rule.example
            );
        }
    }

    #[test]
    fn a_correction_is_spelled_as_the_language_spells_it() {
        // A rule that puts a misspelling where a mistake was has made the
        // text worse, and a list written by somebody who does not write the
        // language every day is exactly where that happens. So every example,
        // corrected, is looked up word by word in the dictionary for its
        // language that the build image carries.
        let mut wrong = Vec::new();
        for (language, path) in [
            ("en", "/usr/share/hunspell/en_US.dic"),
            ("ru", "/usr/share/hunspell/ru_RU.dic"),
            ("de", "/usr/share/hunspell/de_DE.dic"),
            ("fr", "/usr/share/hunspell/fr_FR.dic"),
        ] {
            let dictionary = wp_dict::read_pair(std::path::Path::new(path))
                .unwrap_or_else(|error| panic!("cannot read {path}: {error}"));
            for (_, rule) in every_rule().into_iter().filter(|(tag, _)| *tag == language) {
                let corrected = corrected(rule.example, language);
                for token in tokens_of(&corrected) {
                    if !dictionary.spelled(token.text) {
                        wrong.push(format!("{language}: {:?} in {corrected:?}", token.text));
                    }
                }
            }
        }
        assert!(wrong.is_empty(), "not words of the language: {wrong:#?}");
    }

    /// A text with every correction the rules offer made.
    fn corrected(text: &str, language: &str) -> String {
        let mut out = text.to_owned();
        for finding in check(text, language).into_iter().rev() {
            if let Some(replacement) = finding.replacement {
                out.replace_range(finding.start..finding.end, &replacement);
            }
        }
        out
    }

    #[test]
    fn a_language_is_checked_by_its_own_rules_and_no_other() {
        assert_eq!(found_in("I could of gone.", "en-GB").len(), 1);
        assert_eq!(found_in("I could of gone.", "de-DE"), vec![]);
        assert_eq!(found_in("Wir warten seid Jahren.", "en"), vec![]);
        assert_eq!(found_in("Wir warten seid Jahren.", "de-CH").len(), 1);
        assert_eq!(found_in("I could of gone.", "nl"), vec![], "a language with no list");
        assert!(has_rules("ru-RU") && has_rules("fr-CA") && !has_rules("pl"));
    }

    #[test]
    fn every_category_is_listed_for_translation() {
        let listed = categories();
        for (_, rule) in every_rule() {
            assert!(listed.contains(&rule.category));
        }
        assert!(listed.contains(&"Elision") && listed.contains(&"Verb form"));
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
        assert_eq!(found("I should of went.")[0].2, Some("should have gone".to_owned()));
    }

    #[test]
    fn a_pronoun_takes_the_verb_that_goes_with_it() {
        assert_eq!(found("He don't care.")[0].2, Some("He doesn't".to_owned()));
        assert_eq!(found("they was there")[0].2, Some("they were".to_owned()));
        assert_eq!(found("She has left."), vec![]);
    }

    #[test]
    fn the_apostrophe_a_word_processor_types_is_the_one_a_rule_knows() {
        // Word turns the apostrophe curly as it is typed, and a rule written
        // with a straight one must still see "don’t" — and give back the
        // apostrophe the writer has.
        assert_eq!(found("He don\u{2019}t care.")[0].2, Some("He doesn\u{2019}t".to_owned()));
        assert_eq!(found("I don\u{2019}t know nothing.").len(), 1);
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

    #[test]
    fn a_capital_is_matched_in_every_alphabet() {
        // The patterns are in small letters, and "Более" is "более" at the
        // start of a sentence.
        assert_eq!(found_in("Более лучше так.", "ru")[0].2, Some("Лучше".to_owned()));
        assert_eq!(found_in("Das Selbe gilt hier.", "de")[0].2, Some("Dasselbe".to_owned()));
    }

    #[test]
    fn a_russian_rule_is_as_narrow_as_the_mistake() {
        // "В течении реки" is the river's current, and right; "в течении
        // года" is a year's time, and wrong.
        assert_eq!(found_in("Лодку сносило в течении реки.", "ru"), vec![]);
        assert_eq!(
            found_in("Он работал в течении года.", "ru"),
            vec![(
                "в течении года".to_owned(),
                "Commonly confused words",
                Some("в течение года".to_owned())
            )]
        );
        assert_eq!(found_in("Он надел пальто.", "ru"), vec![]);
        assert_eq!(found_in("Он одел пальто.", "ru")[0].2, Some("надел пальто".to_owned()));
        assert_eq!(found_in("Она одела ребёнка.", "ru"), vec![], "одеть кого-то is right");
    }

    #[test]
    fn a_german_rule_is_as_narrow_as_the_mistake() {
        assert_eq!(found_in("Ihr seid dem Ziel nahe.", "de"), vec![], "seid is a verb here");
        assert_eq!(found_in("Wir warten seid Jahren.", "de")[0].2, Some("seit Jahren".to_owned()));
        assert_eq!(found_in("Ein Paar Schuhe.", "de"), vec![], "a pair, and a Paar");
        assert_eq!(found_in("Ein Paar Tage.", "de")[0].2, Some("Ein paar Tage".to_owned()));
        assert_eq!(found_in("Das ist besser als nichts.", "de"), vec![]);
    }

    #[test]
    fn french_drops_the_vowel_before_a_vowel() {
        assert_eq!(
            found_in("Il parle de abord.", "fr"),
            vec![("de abord".to_owned(), "Elision", Some("d\u{2019}abord".to_owned()))]
        );
        assert_eq!(found_in("Je ai faim.", "fr")[0].2, Some("J\u{2019}ai".to_owned()));
        // The writer's apostrophe, where they typed one nearby to go by: none
        // here, so the typographer's.
        assert_eq!(found_in("Il le aime.", "fr")[0].2, Some("l\u{2019}aime".to_owned()));
        // Not before an aspirated h, which the rule leaves alone, nor before
        // the words that take no elision, nor before a capital letter named.
        assert_eq!(found_in("Le héros de onze ans.", "fr"), vec![]);
        assert_eq!(found_in("De A à Z.", "fr"), vec![]);
        assert_eq!(found_in("Il est à la une.", "fr"), vec![]);
        assert_eq!(
            found_in("Je viendrai si il fait beau.", "fr")[0].2,
            Some("s\u{2019}il".to_owned())
        );
        assert_eq!(found_in("Je viendrai si elle vient.", "fr"), vec![]);
    }
}
