//! What the interface says, in the language the person reads.
//!
//! # Why the English text is the key
//!
//! A catalogue could be keyed by numbers, or by names like
//! `ribbon.home.font.bold`. Word's resources are numbered; most programs
//! that are translated more than once use the English sentence itself. So
//! does this, for three reasons. The code stays readable: `t("Align Left")`
//! says what will be on the screen, where `t(ID_ALIGN_LEFT)` says only that
//! somebody knows. A message that has no translation falls back to
//! something true rather than to a number. And a message whose English is
//! changed becomes, correctly, a new message that needs translating again,
//! instead of quietly keeping the old translation of a sentence that no
//! longer exists.
//!
//! # Why `t` gives back a `&'static str`
//!
//! Because everything that draws the interface already works in those, and
//! a catalogue that forced every label into a `String` would allocate a
//! hundred of them on every repaint. A translated message is leaked once,
//! the first time it is asked for, and lives as long as the program: there
//! are a few hundred messages and a person changes language once, so the
//! leak is bounded and known rather than a leak in the ordinary sense.
//!
//! # Where the catalogues come from
//!
//! Two places. Those that come with the program are in `messages/` beside
//! the source and are built into it, so a translated program is one file
//! and not an installation. Beside them, a person may put a catalogue of
//! their own in `messages/` in the settings folder, which is read first —
//! that is what makes this a catalogue rather than a table in the code:
//! somebody who speaks a language nobody has translated into can translate
//! it themselves, without a compiler.
//!
//! # The pseudo-language
//!
//! One catalogue is not a language: [`PSEUDO`] writes every message in
//! accented letters between brackets. It is how a picture of the window
//! shows which words did not come through here — anything still in plain
//! English was written in the code, and is a string the program cannot
//! translate. Windows and Word both have one for the same purpose.

use std::collections::BTreeMap;
use std::sync::Mutex;

/// The catalogues that come with the program.
///
/// English is not among them: English is the key, so the catalogue for it
/// would map every message to itself.
const BUILT_IN: &[(&str, &str, &str)] = &[
    ("de", "Deutsch", include_str!("../messages/de.txt")),
    // A language read right to left, whose catalogue says so of itself:
    // the window in it is drawn turned about. See [`is_mirrored`].
    ("he", "עברית", include_str!("../messages/he.txt")),
];

/// Every catalogue that travels with the program: its tag, its name, and
/// what is in it.
///
/// Public so that the test holding every catalogue to the list of what the
/// program says can walk them all rather than naming them one by one — a
/// language added and not added to that test would be a language nobody
/// checked.
#[must_use]
pub fn built_in() -> &'static [(&'static str, &'static str, &'static str)] {
    BUILT_IN
}

/// The language whose "translation" is the English message written in
/// accented letters, for finding messages that never reach this module.
pub const PSEUDO: &str = "qps";

/// The same, in a window that reads right to left: the words are the
/// pseudo-language's, and everything the window is made of is turned about.
/// It is how a mirrored interface is looked at without a translation into
/// Arabic or Hebrew to look at it with. Windows has the same pair, and for
/// the same reason.
pub const PSEUDO_MIRRORED: &str = "qpsm";

/// What English is called where a language is named by a code.
pub const ENGLISH: &str = "en";

/// The state: which language, and what has been looked up in it so far.
struct State {
    language: String,
    /// Whether the window is read right to left.
    mirrored: bool,
    catalogue: BTreeMap<String, String>,
    /// Messages already translated and leaked, by their English text.
    known: BTreeMap<&'static str, &'static str>,
}

impl State {
    fn english() -> Self {
        Self {
            language: ENGLISH.to_owned(),
            mirrored: false,
            catalogue: BTreeMap::new(),
            known: BTreeMap::new(),
        }
    }
}

#[cfg(not(test))]
fn state() -> &'static Mutex<State> {
    static STATE: std::sync::OnceLock<Mutex<State>> = std::sync::OnceLock::new();
    STATE.get_or_init(|| Mutex::new(State::english()))
}

/// In the tests, a language for each thread: the tests run side by side,
/// and one that reads the interface in German must not be the reason
/// another, looking for an English word on a button, does not find it.
#[cfg(test)]
fn state() -> &'static Mutex<State> {
    thread_local! {
        static STATE: &'static Mutex<State> = Box::leak(Box::new(Mutex::new(State::english())));
    }
    STATE.with(|state| *state)
}

/// The message as the person reads it.
///
/// The English text where there is no catalogue, no translation in it, or
/// nothing to translate.
#[must_use]
pub fn t(message: &'static str) -> &'static str {
    let Ok(mut state) = state().lock() else { return message };
    if state.language == ENGLISH {
        return message;
    }
    if let Some(known) = state.known.get(message) {
        return known;
    }
    let translated = if state.language == PSEUDO || state.language == PSEUDO_MIRRORED {
        pseudo(message)
    } else {
        match state.catalogue.get(message) {
            Some(found) if !found.is_empty() => found.clone(),
            // A message nobody has translated is shown in English, which is
            // the one thing better than showing its name.
            _ => return message,
        }
    };
    let leaked: &'static str = Box::leak(translated.into_boxed_str());
    state.known.insert(message, leaked);
    leaked
}

/// The message as the person reads it, for text the code is not holding as
/// a literal: a label that arrived from a table somewhere else, or one that
/// was built up before it could be looked up.
///
/// The same lookup as [`t`], and a `String` because there is nothing static
/// to give back. Used where the text is a label of the interface; never for
/// what the person typed or for what came out of the document, which are
/// not messages and have no translation.
#[must_use]
pub fn translated(message: &str) -> String {
    let Ok(state) = state().lock() else { return message.to_owned() };
    if state.language == ENGLISH {
        return message.to_owned();
    }
    if state.language == PSEUDO || state.language == PSEUDO_MIRRORED {
        return pseudo(message);
    }
    match state.catalogue.get(message) {
        Some(found) if !found.is_empty() => found.clone(),
        _ => message.to_owned(),
    }
}

/// A message with something filled into it: `{0}`, `{1}` and so on, in
/// whatever order the translated sentence needs them.
///
/// Numbered rather than in order, because a language that puts the parts of
/// a sentence the other way round has to be able to say so.
#[must_use]
pub fn with(message: &str, values: &[&str]) -> String {
    let mut out = translated(message);
    for (index, value) in values.iter().enumerate() {
        out = out.replace(&format!("{{{index}}}"), value);
    }
    out
}

/// Changes the language the interface is read in.
///
/// Takes effect at once: every label is asked for again on the next repaint,
/// so there is nothing to restart. (Word asks for a restart here. Word is
/// loading resources into a running interface; this is looking a string up.)
pub fn set_language(code: &str) {
    let Ok(mut state) = state().lock() else { return };
    if state.language == code {
        return;
    }
    state.language = code.to_owned();
    state.known.clear();
    state.catalogue = if code == ENGLISH || code == PSEUDO || code == PSEUDO_MIRRORED {
        BTreeMap::new()
    } else {
        read_catalogue(code).unwrap_or_default()
    };
    state.mirrored = code == PSEUDO_MIRRORED || reads_right_to_left(code);
}

/// Whether the interface is read right to left, and so drawn the other way
/// round: the ribbon from the right, the panes on the right, the scroll bar
/// on the left.
///
/// A catalogue says so of itself, with `direction = rtl` at the top —
/// because it is a fact about the language and not about the person, and a
/// person who chooses Arabic has already said everything there is to say
/// about which way their interface reads.
#[must_use]
pub fn is_mirrored() -> bool {
    state().lock().is_ok_and(|state| state.mirrored)
}

/// Whether a catalogue says its language is read right to left.
fn reads_right_to_left(code: &str) -> bool {
    let text = catalogue_text(code);
    text.is_some_and(|text| {
        text.lines()
            .find_map(|line| line.strip_prefix("direction ="))
            .is_some_and(|direction| direction.trim() == "rtl")
    })
}

/// Which language the interface is being read in.
#[must_use]
pub fn language() -> String {
    state().lock().map_or_else(|_| ENGLISH.to_owned(), |state| state.language.clone())
}

/// Every language the interface can be read in: the code, and the name of
/// the language written in that language, which is how a person finds their
/// own in a list.
#[must_use]
pub fn languages() -> Vec<(String, String)> {
    let mut out = vec![(ENGLISH.to_owned(), "English".to_owned())];
    for (code, name, _) in BUILT_IN {
        out.push(((*code).to_owned(), (*name).to_owned()));
    }
    // And whatever the person has put in the settings folder, which may add
    // a language or replace one that came with the program.
    if let Some(folder) = folder() {
        if let Ok(entries) = std::fs::read_dir(folder) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|extension| extension.to_str()) != Some("txt") {
                    continue;
                }
                let Some(code) = path.file_stem().and_then(|stem| stem.to_str()) else { continue };
                let name = std::fs::read_to_string(&path)
                    .ok()
                    .and_then(|text| name_in(&text))
                    .unwrap_or_else(|| code.to_owned());
                match out.iter_mut().find(|(known, _)| known == code) {
                    Some((_, known_name)) => *known_name = name,
                    None => out.push((code.to_owned(), name)),
                }
            }
        }
    }
    out.push((PSEUDO.to_owned(), "Pseudo (for finding untranslated text)".to_owned()));
    out.push((PSEUDO_MIRRORED.to_owned(), "Pseudo, right to left".to_owned()));
    out
}

/// Where a person's own catalogues go.
#[must_use]
pub fn folder() -> Option<std::path::PathBuf> {
    Some(crate::settings::Settings::path()?.parent()?.join("messages"))
}

/// The catalogue for a language as its text: the person's own if they have
/// written one, else the one that came with the program.
fn catalogue_text(code: &str) -> Option<String> {
    if let Some(folder) = folder() {
        if let Ok(text) = std::fs::read_to_string(folder.join(format!("{code}.txt"))) {
            return Some(text);
        }
    }
    BUILT_IN.iter().find(|(known, _, _)| *known == code).map(|(_, _, text)| (*text).to_owned())
}

/// The catalogue for a language, read.
fn read_catalogue(code: &str) -> Option<BTreeMap<String, String>> {
    catalogue_text(code).map(|text| parse(&text))
}

/// A catalogue file: what the language is called, and the messages.
///
/// ```text
/// language = Deutsch
///
/// = Align Left
/// > Links ausrichten
/// ```
///
/// A line beginning `=` is a message in English, and the `>` under it is
/// that message in the other language. Anything else — a blank line, a line
/// beginning `#` — is for whoever is reading the file. A `\n` in either
/// stands for a line break, because a message can have one in it and a file
/// of one message per line cannot.
#[must_use]
pub fn parse(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut source: Option<String> = None;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("= ") {
            source = Some(unescape(rest));
        } else if let Some(rest) = line.strip_prefix("> ") {
            if let Some(key) = source.take() {
                out.insert(key, unescape(rest));
            }
        }
    }
    out
}

/// Every message a catalogue file names, translated or not.
///
/// The list of messages is a catalogue with nothing under any of them, so
/// it is read by the same reader; what is wanted from it is the messages
/// themselves.
#[must_use]
pub fn sources(text: &str) -> std::collections::BTreeSet<String> {
    text.lines().filter_map(|line| line.strip_prefix("= ")).map(unescape).collect()
}

/// What the catalogue says its language is called.
#[must_use]
pub fn name_in(text: &str) -> Option<String> {
    text.lines()
        .find_map(|line| line.strip_prefix("language ="))
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
}

/// A catalogue as its file, in the order a translator wants: the messages
/// sorted, each under the English it stands for.
#[must_use]
pub fn write(name: &str, messages: &[(String, String)]) -> String {
    let mut out = format!("# The catalogue for {name}.\nlanguage = {name}\n\n");
    for (source, translated) in messages {
        out.push_str("= ");
        out.push_str(&escape(source));
        out.push_str("\n> ");
        out.push_str(&escape(translated));
        out.push_str("\n\n");
    }
    out
}

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('\n', "\\n")
}

fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut characters = text.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            out.push(character);
            continue;
        }
        match characters.next() {
            Some('n') => out.push('\n'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// The message in the pseudo-language: the same letters with accents on
/// them, between brackets.
///
/// Accented so that every word is still readable — somebody testing has to
/// be able to tell Bold from Border — and bracketed so that a message cut
/// off by a box too small for the longer words other languages use can be
/// seen to have been cut off. What is filled into a message is left alone:
/// `{0}` has to stay `{0}` or nothing will be filled in.
#[must_use]
pub fn pseudo(message: &str) -> String {
    // A message that has been through here already is left as it is: some
    // of what the window shows was put together out of messages, and
    // marking it twice would say the marking was wrong rather than the
    // message.
    if message.starts_with('[') && message.ends_with(']') {
        return message.to_owned();
    }
    let mut out = String::with_capacity(message.len() + 2);
    out.push('[');
    let mut inside_placeholder = false;
    for character in message.chars() {
        match character {
            '{' => {
                inside_placeholder = true;
                out.push(character);
            }
            '}' => {
                inside_placeholder = false;
                out.push(character);
            }
            _ if inside_placeholder => out.push(character),
            _ => out.push(accented(character)),
        }
    }
    out.push(']');
    out
}

/// The letter with an accent on it, where there is one to put.
fn accented(character: char) -> char {
    match character {
        'a' => 'å',
        'e' => 'ê',
        'i' => 'ì',
        'o' => 'ö',
        'u' => 'ü',
        'y' => 'ý',
        'c' => 'ç',
        'n' => 'ñ',
        's' => 'š',
        'A' => 'Å',
        'E' => 'Ê',
        'I' => 'Ì',
        'O' => 'Ö',
        'U' => 'Ü',
        'C' => 'Ç',
        'N' => 'Ñ',
        'S' => 'Š',
        other => other,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The language belongs to the whole program, so tests that change it
    /// go one at a time and put it back.
    pub(crate) fn in_language<R>(code: &str, work: impl FnOnce() -> R) -> R {
        static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());
        let _held = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let was = language();
        set_language(code);
        let out = work();
        set_language(&was);
        out
    }

    #[test]
    fn english_is_the_message_itself() {
        in_language(ENGLISH, || {
            assert_eq!(t("Align Left"), "Align Left");
        });
    }

    #[test]
    fn a_catalogue_is_read_back_as_it_was_written() {
        let messages = vec![
            ("Align Left".to_owned(), "Links ausrichten".to_owned()),
            ("Save the changes to {0}?".to_owned(), "Änderungen an {0} speichern?".to_owned()),
            ("One\nTwo".to_owned(), "Eins\nZwei".to_owned()),
        ];
        let written = write("Deutsch", &messages);
        assert_eq!(name_in(&written).as_deref(), Some("Deutsch"));
        let read = parse(&written);
        assert_eq!(read.get("Align Left").map(String::as_str), Some("Links ausrichten"));
        assert_eq!(read.get("One\nTwo").map(String::as_str), Some("Eins\nZwei"));
        assert_eq!(read.len(), 3);
    }

    #[test]
    fn a_message_nobody_has_translated_is_shown_in_english() {
        in_language("de", || {
            // Named through a constant rather than written into the call,
            // so that the list of messages does not gain this one.
            const ABSENT: &str = "A message that is certainly not in any catalogue";
            assert_eq!(t(ABSENT), ABSENT);
        });
    }

    #[test]
    fn the_built_in_catalogue_is_used_when_that_language_is_chosen() {
        in_language("de", || {
            assert_eq!(t("Bold"), "Fett", "a word every German copy of Word uses");
            assert_eq!(t("Home"), "Start");
        });
    }

    #[test]
    fn the_pseudo_language_marks_every_message_it_is_given() {
        in_language(PSEUDO, || {
            assert_eq!(t("Bold"), "[Böld]");
            assert_eq!(t("Align Left"), "[Ålìgñ Lêft]");
        });
        // And leaves what is filled in alone, or nothing could be filled in.
        assert_eq!(pseudo("Save {0} as"), "[Šåvê {0} åš]");
    }

    #[test]
    fn what_is_filled_in_goes_where_the_translation_puts_it() {
        in_language(ENGLISH, || {
            assert_eq!(with("{0} of {1}", &["3", "8"]), "3 of 8");
        });
    }

    #[test]
    fn a_language_read_right_to_left_says_so_and_a_window_in_it_is_turned() {
        in_language(ENGLISH, || assert!(!is_mirrored(), "English is read the other way"));
        in_language(PSEUDO, || assert!(!is_mirrored(), "and so is the pseudo-language"));
        in_language(PSEUDO_MIRRORED, || {
            assert!(is_mirrored(), "the mirrored one is not");
            assert_eq!(t("Bold"), "[Böld]", "and says the same words");
        });
        in_language("de", || assert!(!is_mirrored(), "German is read left to right"));
    }

    #[test]
    fn a_catalogue_says_which_way_its_language_reads() {
        let mut written = write("עברית", &[("Bold".to_owned(), "מודגש".to_owned())]);
        written.insert_str(0, "direction = rtl\n");
        assert!(
            written.lines().any(|line| line.trim() == "direction = rtl"),
            "which is a line at the top of the file, where a translator can see it"
        );
        assert_eq!(parse(&written).get("Bold").map(String::as_str), Some("מודגש"));
    }

    #[test]
    fn english_and_the_languages_that_come_with_the_program_are_offered() {
        let offered = languages();
        assert_eq!(offered.first().map(|(code, _)| code.as_str()), Some(ENGLISH));
        assert!(offered.iter().any(|(code, name)| code == "de" && name == "Deutsch"));
        assert!(
            offered.last().map(|(code, _)| code.as_str()) == Some(PSEUDO_MIRRORED),
            "and the pseudo-languages last, where they are out of the way"
        );
    }
}
