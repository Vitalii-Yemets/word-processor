//! Tests against a real bilingual dictionary rather than one written for the
//! test.
//!
//! A hand-built dictzip of one piece proves the reader handles the shape it
//! was written for. Only a dictionary somebody else published proves it
//! handles the real thing: fifteen megabytes in a thousand pieces, an entry
//! that runs across the boundary between two of them, an index of four hundred
//! thousand lines. FreeDict's English-German dictionary is installed in the
//! build container for this, and four more beside it.

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

/// The other dictionaries on the build image: German into English, the same
/// Ding list the other way round and as large; and FreeDict's English-French,
/// French-English and English-Russian, which are small and written from
/// Wiktionary, the other way FreeDict writes its entries.
struct Other {
    path: &'static str,
    from: &'static str,
    to: &'static str,
    /// Words it must have, and a word each must be translated as.
    words: &'static [(&'static str, &'static str)],
}

const OTHERS: &[Other] = &[
    Other {
        path: "/usr/share/dictd/freedict-deu-eng.index",
        from: "de",
        to: "en",
        words: &[("Haus", "house"), ("Hund", "dog"), ("schnell", "fast"), ("Zeitung", "newspaper")],
    },
    Other {
        path: "/usr/share/dictd/freedict-eng-fra.index",
        from: "en",
        to: "fr",
        words: &[("cat", "chat"), ("house", "maison")],
    },
    Other {
        path: "/usr/share/dictd/freedict-fra-eng.index",
        from: "fr",
        to: "en",
        words: &[("chat", "cat"), ("maison", "house")],
    },
    Other {
        path: "/usr/share/dictd/freedict-eng-rus.index",
        from: "en",
        to: "ru",
        words: &[("water", "\u{432}\u{43E}\u{434}")],
    },
];

#[test]
fn every_dictionary_on_the_machine_is_read_and_says_what_it_should() {
    for Other { path, words, .. } in OTHERS {
        let dictionary = wp_dict::bilingual::Bilingual::open(Path::new(path))
            .unwrap_or_else(|error| panic!("cannot read {path}: {error:?}"));
        assert!(dictionary.words() > 1000, "{path} has only {} words", dictionary.words());
        for (word, wanted) in *words {
            let meanings = dictionary.meanings(word);
            let said: Vec<&str> = meanings
                .iter()
                .flat_map(|meaning| meaning.translations.iter().map(String::as_str))
                .collect();
            assert!(
                said.iter().any(|translation| translation.contains(wanted)),
                "{path}: {word} is not {wanted} but {said:?}"
            );
            assert!(
                meanings.iter().all(|meaning| !meaning.translations.is_empty()),
                "{path}: an empty sense of {word}: {meanings:?}"
            );
        }
        assert!(dictionary.meanings("xqzv").is_empty(), "{path}");
    }
}

#[test]
fn every_pair_on_the_machine_is_found_the_way_round_it_goes() {
    let installed = wp_dict::bilingual::installed();
    for Other { from, to, .. } in OTHERS {
        assert!(
            installed.iter().any(|pair| pair.from == *from && pair.to == *to),
            "{from} into {to} is not among {installed:?}"
        );
    }
}
