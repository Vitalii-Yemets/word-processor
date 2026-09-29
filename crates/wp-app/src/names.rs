//! The document's own words in the person's language: what a style is
//! called on the screen, and what the language the text is marked as is
//! called.
//!
//! # Why a document's style names are not simply shown
//!
//! Because Word writes the names of its built-in styles into every file in
//! English, whatever language it runs in — "heading 1", "Normal", "Title" —
//! and shows them in the language it runs in: a German sees "Überschrift 1"
//! and "Standard" in the same file an English person sees "Heading 1" and
//! "Normal" in. A style somebody made and named is theirs, and is shown as
//! they named it, in any language. So a built-in style is recognised by
//! the name the file gives it, and shown by the name Word shows it by — a
//! message like any other, translated by the catalogue.
//!
//! The names are Word's: what it writes in the file, lower case for many,
//! and what it shows, which differs for a few — "annotation text" is shown
//! as Comment Text.
//!
//! # And the languages
//!
//! A document marks its text with a tag — `en-GB` — which Word shows in the
//! status strip and the Language list by name, in its own language: "English
//! (United Kingdom)" to an English person, "Englisch (Vereinigtes
//! Königreich)" to a German. The name is the language and, where one
//! language is written in several places or ways, which: each half is a
//! message, so a translator translates "English" once and not once for
//! every country it is spoken in.

use crate::messages::{t, translated};

/// Word's built-in styles: the name written in the file, and the name
/// shown, which is what is translated.
pub const BUILT_IN: &[(&str, &str)] = &[
    ("Normal", "Normal"),
    ("heading 1", "Heading 1"),
    ("heading 2", "Heading 2"),
    ("heading 3", "Heading 3"),
    ("heading 4", "Heading 4"),
    ("heading 5", "Heading 5"),
    ("heading 6", "Heading 6"),
    ("heading 7", "Heading 7"),
    ("heading 8", "Heading 8"),
    ("heading 9", "Heading 9"),
    ("Title", "Title"),
    ("Subtitle", "Subtitle"),
    ("Quote", "Quote"),
    ("Intense Quote", "Intense Quote"),
    ("List Paragraph", "List Paragraph"),
    ("caption", "Caption"),
    ("No Spacing", "No Spacing"),
    ("Strong", "Strong"),
    ("Emphasis", "Emphasis"),
    ("Subtle Emphasis", "Subtle Emphasis"),
    ("Intense Emphasis", "Intense Emphasis"),
    ("Subtle Reference", "Subtle Reference"),
    ("Intense Reference", "Intense Reference"),
    ("Book Title", "Book Title"),
    ("TOC Heading", "TOC Heading"),
    ("toc 1", "TOC 1"),
    ("toc 2", "TOC 2"),
    ("toc 3", "TOC 3"),
    ("toc 4", "TOC 4"),
    ("toc 5", "TOC 5"),
    ("toc 6", "TOC 6"),
    ("toc 7", "TOC 7"),
    ("toc 8", "TOC 8"),
    ("toc 9", "TOC 9"),
    ("header", "Header"),
    ("footer", "Footer"),
    ("footnote text", "Footnote Text"),
    ("footnote reference", "Footnote Reference"),
    ("endnote text", "Endnote Text"),
    ("endnote reference", "Endnote Reference"),
    ("Hyperlink", "Hyperlink"),
    ("Table Grid", "Table Grid"),
    ("Balloon Text", "Balloon Text"),
    ("Default Paragraph Font", "Default Paragraph Font"),
    ("Normal Table", "Normal Table"),
    ("No List", "No List"),
    ("Placeholder Text", "Placeholder Text"),
    ("List Bullet", "List Bullet"),
    ("List Number", "List Number"),
    ("List", "List"),
    ("Body Text", "Body Text"),
    ("Plain Text", "Plain Text"),
    ("annotation text", "Comment Text"),
    ("annotation reference", "Comment Reference"),
    ("annotation subject", "Comment Subject"),
    ("page number", "Page Number"),
    ("line number", "Line Number"),
    ("Revision", "Revision"),
    ("Bibliography", "Bibliography"),
    ("table of figures", "Table of Figures"),
    ("index heading", "Index Heading"),
    ("Date", "Date"),
    ("Salutation", "Salutation"),
    ("Signature", "Signature"),
    ("Closing", "Closing"),
    ("Block Text", "Block Text"),
];

/// What a style is called on the screen: a built-in one — known by the name
/// the file gives it, however it is capitalised — by the name Word shows it
/// by, in the interface's language; any other as it was named.
#[must_use]
pub fn shown(name: &str) -> String {
    match BUILT_IN.iter().find(|(written, _)| written.eq_ignore_ascii_case(name.trim())) {
        Some((_, shown)) => t(shown).to_owned(),
        None => name.to_owned(),
    }
}

/// What a language is called, by the tag a document marks its text with:
/// Word's name for it, in the interface's language, a half at a time.
#[must_use]
pub fn language(tag: &str) -> String {
    let name = wp_docx::languages::name_of(tag);
    match halves(&name) {
        (language, Some(which)) => format!("{} ({})", translated(language), translated(which)),
        (language, None) => translated(language),
    }
}

/// The languages the Language list offers, in the order their names take in
/// the interface's language, as places in [`wp_docx::languages::LANGUAGES`]:
/// the list is read in the language it is shown in, and "Englisch" is not
/// where "English" was.
#[must_use]
pub fn languages_in_order() -> Vec<usize> {
    let all = wp_docx::languages::LANGUAGES;
    let mut order: Vec<usize> = (0..all.len()).collect();
    order.sort_by_cached_key(|at| order_key(&language(all[*at].tag)));
    order
}

/// A name as it is put in order: without its accents and in small letters,
/// as a dictionary orders words — "Österreich" among the O's, and not after
/// the Z where its first letter's number would put it.
#[must_use]
pub fn order_key(name: &str) -> String {
    wp_normal::decompose(name)
        .chars()
        .filter(|character| wp_normal::combining_class(*character) == 0)
        .flat_map(char::to_lowercase)
        .collect()
}

/// A language's name as its two halves: the language, and which of its
/// places or ways of writing, where the name says one.
#[must_use]
pub fn halves(name: &str) -> (&str, Option<&str>) {
    match name.split_once(" (") {
        Some((language, rest)) => (language, Some(rest.strip_suffix(')').unwrap_or(rest))),
        None => (name, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The German catalogue's names of the languages text is marked as, and
    /// of the countries after them, held to Debian's German for the ISO
    /// lists (iso-codes) — somebody else's translation of the same names.
    /// Where Word's German is not ISO's it is Word's that is kept, and each
    /// such name is said here: Word writes Belarussisch, Galicisch,
    /// Mazedonisch, Nepalesisch and Punjabi where ISO's translators write
    /// Weißrussisch, Galizisch, Makedonisch, Nepali and Panjabi.
    #[test]
    fn the_german_names_of_languages_and_countries_are_the_ones_iso_gives() {
        use std::io::Write;
        let iso = std::path::Path::new("/usr/share/iso-codes/json");
        if !iso.join("iso_639-3.json").exists() {
            eprintln!("skipped: no iso-codes on this machine");
            return;
        }
        let script = "import json, gettext, sys
p = '/usr/share/iso-codes/json/'
names = {'L': {x['alpha_2']: x['name'] for x in json.load(open(p + 'iso_639-3.json'))['639-3'] if 'alpha_2' in x},
         'C': {x['alpha_2']: x['name'] for x in json.load(open(p + 'iso_3166-1.json'))['3166-1']}}
german = {'L': gettext.translation('iso_639-3', '/usr/share/locale', ['de']),
          'C': gettext.translation('iso_3166-1', '/usr/share/locale', ['de'])}
for line in sys.stdin:
    kind, code = line.split()
    english = names[kind].get(code, '')
    print(kind, code, english, german[kind].gettext(english) if english else '', sep='\\t')";
        let Ok(mut python) = std::process::Command::new("python3")
            .args(["-c", script])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
        else {
            eprintln!("skipped: no Python on this machine");
            return;
        };
        let mut asked = String::new();
        for language in wp_docx::languages::LANGUAGES {
            let language_code = language.tag.split('-').next().unwrap_or_default();
            let region = language.tag.rsplit('-').next().unwrap_or_default();
            asked.push_str(&format!("L {language_code}\nC {region}\n"));
        }
        python.stdin.take().expect("its input").write_all(asked.as_bytes()).expect("asking");
        let answer = python.wait_with_output().expect("Python's answer");
        let answer = String::from_utf8_lossy(&answer.stdout).into_owned();
        let iso: Vec<Vec<&str>> = answer.lines().map(|line| line.split('\t').collect()).collect();
        let (_, _, text) = crate::messages::built_in()
            .iter()
            .find(|(tag, ..)| *tag == "de")
            .expect("the German catalogue");
        let catalogue = crate::messages::parse(text);
        let words_own = ["Belarussisch", "Galicisch", "Mazedonisch", "Nepalesisch", "Punjabi"];

        let mut wrong = Vec::new();
        for (at, language) in wp_docx::languages::LANGUAGES.iter().enumerate() {
            let (name, which) = halves(language.name);
            let ours = catalogue.get(name).cloned().unwrap_or_default();
            let theirs = iso.get(at * 2).and_then(|row| row.get(3)).copied().unwrap_or_default();
            // Or the second half is the language, as ISO has Nynorsk where
            // Word has Norwegian (Nynorsk).
            let second = which.and_then(|which| catalogue.get(which)).cloned().unwrap_or_default();
            let agree = theirs.to_lowercase().contains(&ours.to_lowercase())
                || ours.to_lowercase().contains(&theirs.to_lowercase())
                || (!second.is_empty() && theirs.contains(&second));
            if !agree && !words_own.contains(&ours.as_str()) && !theirs.is_empty() {
                wrong.push(format!("{name}: {ours} where ISO says {theirs}"));
            }
            // The half after it, where that is a country: ISO's name of it.
            let country = iso.get(at * 2 + 1).map(|row| (row[2], row[3]));
            if let (Some(which), Some((english, german))) = (which, country) {
                if which == english && catalogue.get(which).map(String::as_str) != Some(german) {
                    wrong.push(format!(
                        "{which}: {:?} where ISO says {german}",
                        catalogue.get(which)
                    ));
                }
            }
        }
        assert!(wrong.is_empty(), "{wrong:#?}");
    }

    /// To a German, Word's own styles by Word's German names and a style of
    /// one's own as it was named; the language in German, a half at a time;
    /// and the Language list in the order a German reads it in.
    #[test]
    fn a_german_sees_word_s_own_words_in_german_and_their_own_as_they_wrote_them() {
        crate::messages::tests::in_language("de", || {
            assert_eq!(shown("heading 1"), "Überschrift 1");
            assert_eq!(shown("Normal"), "Standard");
            assert_eq!(shown("annotation text"), "Kommentartext");
            assert_eq!(shown("Kapitelanfang"), "Kapitelanfang");
            assert_eq!(shown("Chapter Opening"), "Chapter Opening", "not Word's, not translated");
            assert_eq!(language("en-GB"), "Englisch (Vereinigtes Königreich)");
            assert_eq!(language("de-AT"), "Deutsch (Österreich)");
            let order: Vec<String> = languages_in_order()
                .into_iter()
                .map(|at| language(wp_docx::languages::LANGUAGES[at].tag))
                .collect();
            let keys: Vec<String> = order.iter().map(|name| order_key(name)).collect();
            assert!(keys.windows(2).all(|pair| pair[0] <= pair[1]), "{order:?}");
            let place = |name: &str| order.iter().position(|found| found == name).expect(name);
            assert!(place("Deutsch (Deutschland)") < place("Englisch (Vereinigte Staaten)"));
        });
    }

    #[test]
    fn a_language_s_name_is_its_two_halves() {
        assert_eq!(halves("English (United Kingdom)"), ("English", Some("United Kingdom")));
        assert_eq!(halves("Welsh"), ("Welsh", None));
        assert_eq!(language("en-GB"), "English (United Kingdom)");
        assert_eq!(language("cy-GB"), "Welsh");
    }

    #[test]
    fn a_built_in_style_is_known_by_its_name_in_the_file_however_it_is_written() {
        assert_eq!(shown("heading 1"), "Heading 1");
        assert_eq!(shown("Heading 1"), "Heading 1");
        assert_eq!(shown("annotation text"), "Comment Text", "shown as Word shows it");
        assert_eq!(shown("Kapitelanfang"), "Kapitelanfang", "a style of one's own as named");
    }

    #[test]
    fn every_built_in_style_is_written_once() {
        for (at, (written, _)) in BUILT_IN.iter().enumerate() {
            let again =
                BUILT_IN[at + 1..].iter().any(|(other, _)| other.eq_ignore_ascii_case(written));
            assert!(!again, "{written} twice");
        }
    }
}
