//! Tests against a real dictionary rather than one written for the test.
//!
//! A hand-built pair of files proves the reader handles the shapes it was
//! written for. Only a dictionary somebody else published proves it handles
//! the shapes that actually occur: a hundred rules, conditions on every one,
//! flags aliased to numbers, words that are forbidden, words that exist only
//! inside a longer word. The Hunspell dictionaries for English and German are
//! installed in the build container for this.

use std::path::Path;

const ENGLISH: &str = "/usr/share/hunspell/en_US.dic";
const GERMAN: &str = "/usr/share/hunspell/de_DE.dic";

fn read(path: &str) -> wp_dict::Dictionary {
    wp_dict::read_pair(Path::new(path)).unwrap_or_else(|error| {
        panic!(
            "cannot read {path}: {error}\n\
             the build image should install hunspell-en-us and hunspell-de-de"
        )
    })
}

#[test]
fn a_real_dictionary_reads() {
    let dictionary = read(ENGLISH);
    assert!(dictionary.stems() > 10_000, "an English dictionary has tens of thousands of stems");
    assert!(!dictionary.is_empty());
    assert!(!dictionary.try_letters.is_empty(), "the letters a suggester would try");
}

#[test]
fn the_words_of_the_language_are_words() {
    let dictionary = read(ENGLISH);
    for word in
        ["the", "quick", "brown", "fox", "jumps", "over", "lazy", "dog", "word", "processor"]
    {
        assert!(dictionary.spelled(word), "{word} is a word");
    }
}

#[test]
fn what_is_not_a_word_is_not_a_word() {
    let dictionary = read(ENGLISH);
    // The point of a checker: these must be underlined, and nothing correct
    // may be.
    for wrong in ["teh", "recieve", "definately", "occured", "seperate", "wierd"] {
        assert!(!dictionary.spelled(wrong), "{wrong} should not be a word");
    }
}

#[test]
fn the_forms_a_rule_makes_are_words() {
    // What the whole item is about. None of these is in the word list: each is
    // a stem with a rule applied, and a checker that reads only the list
    // underlines every one of them.
    let dictionary = read(ENGLISH);
    for word in [
        "walked",
        "walking",
        "walks",
        "cats",
        "boxes",
        "tried",
        "tries",
        "bigger",
        "biggest",
        "happily",
        "unhappy",
        "children's",
        "lovely",
        "quickly",
    ] {
        assert!(dictionary.spelled(word), "{word} is a form of a word in the list");
    }
}

#[test]
fn a_form_no_rule_makes_is_not_a_word() {
    let dictionary = read(ENGLISH);
    // Each of these is the right stem with the wrong ending, which is exactly
    // what a reader that ignores the conditions would let through.
    for wrong in ["walkeded", "catss", "triess", "bigest", "happilyly"] {
        assert!(!dictionary.spelled(wrong), "{wrong} should not be a word");
    }
}

#[test]
fn a_word_is_a_word_however_it_is_capitalised() {
    let dictionary = read(ENGLISH);
    assert!(dictionary.spelled("Walking"), "at the start of a sentence");
    assert!(dictionary.spelled("WALKING"), "shouted");
    assert!(dictionary.spelled("London"));
    assert!(dictionary.spelled("LONDON"));
}

#[test]
fn german_writes_several_words_as_one() {
    // The other half of what a real dictionary is for. German makes a noun out
    // of two nouns by writing them together, and no word list can hold the
    // result: the rules have to say which words may join.
    let dictionary = read(GERMAN);
    assert!(dictionary.stems() > 10_000);

    for word in ["Haus", "Tür", "Bahn", "Wagen"] {
        assert!(dictionary.spelled(word), "{word} is a word");
    }
    assert!(!dictionary.spelled("Hausxyz"));
}

#[test]
fn the_whole_word_list_can_be_looked_up() {
    // Every stem the file holds, asked for. A reader that mishandles one line
    // of the format fails on whichever words happen to use it, and the only
    // way to know is to ask for all of them.
    let dictionary = read(ENGLISH);
    let text = std::fs::read_to_string(ENGLISH).expect("the word list");
    let mut asked = 0;
    let mut known = 0;

    for line in text.lines().skip(1) {
        let entry = line.split(['\t', ' ']).next().unwrap_or_default();
        let word = entry.split('/').next().unwrap_or_default();
        if word.is_empty() || word.chars().any(char::is_numeric) {
            continue;
        }
        asked += 1;
        if dictionary.spelled(word) {
            known += 1;
        }
    }

    assert!(asked > 10_000);
    // Not all of them: a dictionary holds stems that are not words on their
    // own — the ones flagged as needing an affix or as only ever part of a
    // longer word — and those are supposed to fail.
    assert!(
        known * 100 / asked > 90,
        "only {known} of {asked} words in the list are words according to the reader"
    );
}

#[test]
fn the_apostrophe_a_word_processor_types_is_the_one_the_list_holds() {
    // A word processor turns a typed apostrophe into a curly one, and no word
    // list holds curly apostrophes. The affix file says to read one as the
    // other, and without that every "don't" anybody writes is underlined.
    let dictionary = read(ENGLISH);
    assert!(dictionary.spelled("don't"));
    assert!(dictionary.spelled("don\u{2019}t"), "the curly one is the same word");
    assert!(dictionary.spelled("it\u{2019}s"));
}

#[test]
fn a_misspelling_is_offered_what_was_meant() {
    // Each of these is one slip from a word, and the word has to be among the
    // first few offered — not somewhere in a list of forty.
    let dictionary = read(ENGLISH);
    let first_few = |word: &str| dictionary.suggest(word).into_iter().take(3).collect::<Vec<_>>();

    assert!(first_few("teh").contains(&"the".to_owned()), "{:?}", first_few("teh"));
    assert!(first_few("recieve").contains(&"receive".to_owned()), "{:?}", first_few("recieve"));
    assert!(first_few("walkd").contains(&"walked".to_owned()), "{:?}", first_few("walkd"));
    assert!(first_few("housse").contains(&"house".to_owned()), "{:?}", first_few("housse"));
}

#[test]
fn a_suggestion_keeps_the_case_of_what_was_typed() {
    let dictionary = read(ENGLISH);
    assert!(dictionary.suggest("Teh").contains(&"The".to_owned()));
    assert!(dictionary.suggest("TEH").contains(&"THE".to_owned()));
}

#[test]
fn two_words_run_together_are_offered_apart() {
    let dictionary = read(ENGLISH);
    let offered = dictionary.suggest("thequick");
    assert!(offered.contains(&"the quick".to_owned()), "{offered:?}");
}

#[test]
fn a_word_that_is_right_is_offered_nothing_of_itself() {
    let dictionary = read(ENGLISH);
    assert!(!dictionary.suggest("house").contains(&"house".to_owned()));
}

#[test]
fn a_real_thesaurus_says_what_else_a_word_could_have_been() {
    // Held to the thesaurus LibreOffice ships, because a reader of this kind
    // cannot be believed against a file written for the test: a hundred and
    // forty thousand entries, some of them a hundred synonyms long.
    let path = Path::new("/usr/share/mythes/th_en_US_v2.dat");
    let thesaurus = wp_dict::thesaurus::Thesaurus::open(path).unwrap_or_else(|error| {
        panic!(
            "cannot open {}: {error}\nthe build image should install mythes-en-us",
            path.display()
        )
    });
    assert!(thesaurus.words() > 100_000);

    let senses = thesaurus.senses("happy");
    assert!(!senses.is_empty());
    let all: Vec<&str> =
        senses.iter().flat_map(|sense| sense.synonyms.iter().map(String::as_str)).collect();
    assert!(all.contains(&"glad"), "{all:?}");
    assert!(
        senses.iter().any(|sense| sense.antonyms.iter().any(|word| word == "unhappy")),
        "the antonym is marked as one"
    );
    // The meanings are told apart: "bright" the light and "bright" the mind.
    let bright = thesaurus.senses("bright");
    assert!(bright.len() > 1, "{bright:?}");
    assert!(thesaurus.senses("xqzv").is_empty());
}

#[test]
fn the_thesaurus_on_this_machine_is_found() {
    let installed = wp_dict::thesaurus::installed();
    assert!(installed.iter().any(|(language, _)| language == "en-US"), "{installed:?}");
}
