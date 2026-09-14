//! Tests against a real bilingual dictionary rather than one written for the
//! test.
//!
//! A hand-built dictzip of one piece proves the reader handles the shape it
//! was written for. Only a dictionary somebody else published proves it
//! handles the real thing: fifteen megabytes in a thousand pieces, an entry
//! that runs across the boundary between two of them, an index of four hundred
//! thousand lines. FreeDict's English-German dictionary is installed in the
//! build container for this.

use std::path::Path;

const ENGLISH_GERMAN: &str = "/usr/share/dictd/freedict-eng-deu.index";

fn open() -> wp_dict::bilingual::Bilingual {
    wp_dict::bilingual::Bilingual::open(Path::new(ENGLISH_GERMAN)).unwrap_or_else(|error| {
        panic!(
            "cannot read {ENGLISH_GERMAN}: {error:?}\n\
             the build image should install dict-freedict-eng-deu"
        )
    })
}

#[test]
fn a_real_dictionary_reads() {
    let dictionary = open();
    assert!(
        dictionary.words() > 100_000,
        "an English-German dictionary has hundreds of thousands of words"
    );
}

#[test]
fn a_word_has_its_senses() {
    let dictionary = open();
    let meanings = dictionary.meanings("house");
    assert!(!meanings.is_empty(), "no entry for house");
    let words: Vec<&str> = meanings
        .iter()
        .flat_map(|meaning| meaning.translations.iter().map(String::as_str))
        .collect();
    assert!(words.contains(&"Haus"), "the senses of house: {words:?}");
    let kinds: Vec<&str> =
        meanings.iter().filter_map(|meaning| meaning.part_of_speech.as_deref()).collect();
    assert!(kinds.contains(&"neut"), "Haus is a neuter noun: {kinds:?}");
}

#[test]
fn an_entry_is_read_wherever_in_the_file_it_falls() {
    // Words from all over the alphabet, so that entries in early pieces, late
    // pieces, and across the boundary between two pieces are all read.
    let dictionary = open();
    for word in ["abandon", "happy", "quickly", "the", "walked", "zebra", "yes", "monday"] {
        let meanings = dictionary.meanings(word);
        assert!(!meanings.is_empty(), "no entry for {word}");
        assert!(
            meanings.iter().all(|meaning| !meaning.translations.is_empty()),
            "an empty sense of {word}: {meanings:?}"
        );
    }
}

#[test]
fn a_word_it_does_not_know_has_no_entry() {
    let dictionary = open();
    // Not "Fenster": the Ding dictionary indexes its German side too, so a
    // German word is found from the English side. A word of no language is
    // the only sure absence.
    assert!(dictionary.meanings("xqzv").is_empty());
}

#[test]
fn the_dictionary_is_found_among_the_installed() {
    let installed = wp_dict::bilingual::installed();
    let found = installed.iter().find(|pair| pair.from == "en" && pair.to == "de");
    assert!(found.is_some(), "English-German is not among {installed:?}");
}
