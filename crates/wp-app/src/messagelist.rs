//! Every message the interface can show, gathered so that a translator has
//! a list and the program has something to hold itself to.
//!
//! # Why it is gathered twice over
//!
//! A message reaches the catalogue in one of two ways. Most are written in
//! the code as `t("Align Left")`, and those are found by reading the source
//! — which is what a `.pot` file is in the world of `gettext`, and is done
//! here for the same reason: nobody can be trusted to keep a list by hand.
//! The rest arrive as a label out of a table — every button on the ribbon,
//! every tab, every place on the File page — and are found by walking those
//! tables, which is the only way to reach text that never appears next to a
//! `t(` in the source.
//!
//! The two together are the whole of what the program can say. The test
//! below holds `messages/en.txt` to exactly that list: a message added to
//! the code and not to the list fails, and so does a message left on the
//! list after the code stopped saying it.

use std::collections::BTreeSet;

/// Every message, in the order a translator would want them: sorted, and
/// each exactly as it is written in the code.
#[must_use]
pub fn every_message() -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    out.extend(from_the_tables());
    out.extend(from_the_source());
    out
}

/// The labels that live in tables rather than in calls: the ribbon, the
/// tabs, the places on the File page, the panes.
fn from_the_tables() -> BTreeSet<String> {
    use crate::chrome::backstage::Place;
    use crate::chrome::navigation::Section;
    use crate::chrome::printpane::{Sides, Which};
    use crate::chrome::ribbon::{self, Item, Tab};
    use crate::chrome::stylespane::Showing;

    let mut out = BTreeSet::new();
    // The four things the Trust Centre may be told, in Word's own words.
    for trusting in crate::editor::trust::Trusting::ALL {
        out.insert(trusting.label().to_owned());
    }
    // The buttons a macro's message box may have, which are Word's.
    for buttons in 0..=5 {
        for (label, _) in crate::editor::debugger::message_buttons(buttons) {
            out.insert((*label).to_owned());
        }
    }
    for tab in Tab::ALL.iter().chain(Tab::CONTEXTUAL.iter()) {
        out.insert(tab.label().to_owned());
        for group in ribbon::groups_of(*tab) {
            out.insert(group.label.to_owned());
            for item in group.items {
                match item {
                    Item::Large(_, _, label)
                    | Item::Small(_, _, label)
                    | Item::Measure(_, label, _) => {
                        out.insert((*label).to_owned());
                    }
                    // A letter is a letter in every language: the B of Bold
                    // is drawn in the formatting it applies, not read.
                    Item::Letter(..)
                    | Item::Button(..)
                    | Item::Field(..)
                    | Item::StyleGallery
                    | Item::Break
                    | Item::NewColumn => {}
                }
            }
        }
    }
    // The name of every command, which is what a tip says and what the
    // Customize Ribbon page lists — including the commands that have a name
    // but no button, like the ones on the File page.
    for (command, name) in ribbon::all_commands() {
        // A command whose name is one letter is the letter itself — the B
        // that is drawn in bold — and there is nothing to translate in it.
        if name.chars().count() > 1 {
            out.insert(name.to_owned());
        }
        if let Some(label) = crate::chrome::tip::label_of(command) {
            out.insert(label.to_owned());
        }
    }
    for place in Place::ALL {
        out.insert(place.label().to_owned());
    }
    for section in Section::ALL {
        out.insert(section.label().to_owned());
    }
    for which in Which::ALL {
        out.insert(which.label().to_owned());
    }
    for sides in Sides::ALL {
        out.insert(sides.label().to_owned());
    }
    for showing in [Showing::All, Showing::InUse] {
        out.insert(showing.label().to_owned());
    }
    // What measurements are shown in, which is a list in Options.
    for unit in crate::measure::Unit::ALL {
        out.insert(unit.label().to_owned());
    }
    // The page-number gallery: the name of every design in it, and what the
    // program says once one has gone in.
    for design in
        crate::editor::designs::DESIGNS.iter().chain(crate::editor::designs::IN_THE_MARGIN)
    {
        out.insert(design.name.to_owned());
    }
    for place in crate::editor::designs::Place::ALL {
        out.insert(place.label().to_owned());
        out.insert(place.said().to_owned());
    }
    // And the name of every gallery a building block can be filed under,
    // which the organiser shows and the dialog that saves one names.
    for (_, shown) in crate::editor::ownblocks::GALLERIES {
        out.insert((*shown).to_owned());
    }
    // The arrangements a cover page comes in.
    for layout in wp_docx::cover::Layout::ALL {
        out.insert(layout.label().to_owned());
    }
    // The kinds of chart the list offers.
    for (_, _, label) in crate::editor::chart::PRESETS {
        out.insert((*label).to_owned());
    }
    // The arrangements a diagram can take, and the colours it can be drawn
    // in.
    for arrangement in wp_docx::diagram::Arrangement::ALL {
        out.insert(arrangement.label().to_owned());
    }
    for colouring in wp_docx::diagram::Colouring::ALL {
        out.insert(colouring.label().to_owned());
    }
    // The quick style sets: what each is called and what it says about
    // itself, both of which a person reads off the menu.
    for set in crate::editor::stylesets::SETS {
        out.insert(set.name.to_owned());
        out.insert(set.note.to_owned());
    }
    // The columns a list of people starts with, which are the headings of a
    // table somebody is about to type into.
    for column in crate::editor::mailings::NEW_LIST_COLUMNS {
        out.insert((*column).to_owned());
    }
    // What the bar across the top says, and what its button offers.
    for because in crate::chrome::infobar::Because::ALL {
        out.insert(because.said().to_owned());
        if let Some(button) = because.button() {
            out.insert(button.to_owned());
        }
    }
    out
}

/// The messages written into the code, found by reading it.
///
/// Every `t("…")` and every `with("…", …)`, which are the two ways a
/// message is named where it stands.
fn from_the_source() -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for file in source_files(std::path::Path::new("src")) {
        let Ok(text) = std::fs::read_to_string(&file) else { continue };
        // What the tests say is not what the interface says: a test may
        // pass anything at all to `t` to see what comes back, and those are
        // not messages to translate.
        let program = text
            .split(
                "
#[cfg(test)]",
            )
            .next()
            .unwrap_or(&text);
        out.extend(messages_in(program));
    }
    out
}

/// Every `.rs` file under a directory.
fn source_files(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else { return out };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(source_files(&path));
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("rs") {
            // What a macro is written against is not the interface. The
            // names of Word's own objects — `Selection`, `Range`,
            // `Paragraphs` — are part of the language a macro is written in,
            // the same in every country, and a catalogue offering them for
            // translation would be offering to break every macro there is.
            // What that module says when it refuses something goes to
            // whoever wrote the macro, in the same English as the rest of the
            // language's own errors. See [`crate::editor::objects`].
            if path.file_name().and_then(|name| name.to_str()) == Some("objects.rs") {
                continue;
            }
            out.push(path);
        }
    }
    out
}

/// The messages named in one file of source.
///
/// Reading the source rather than parsing it. Two shapes are looked for.
/// A message named where it stands — the string inside `t(` or `with(` —
/// and a label written into a dialog, which is the string after `label:`
/// or inside the handful of things a dialog is built of. The second is
/// there because a dialog's words are looked up when the dialog is drawn
/// rather than where it is built, so there is no `t(` beside them to find:
/// see [`crate::chrome::dialog::Field::in_the_readers_language`].
#[must_use]
pub fn messages_in(text: &str) -> BTreeSet<String> {
    let bytes = text.as_bytes();
    let mut out = BTreeSet::new();
    for call in [
        "t(",
        "with(",
        "label: ",
        "second: ",
        "Field::Tab(",
        "Field::Group(",
        "Field::Heading(",
        "Field::note(",
        "Dialog::new(",
        "Dialog::with_buttons(",
        "Row::new(",
        "TreeRow::plain(",
        "check(",
        "named(",
        "heading(",
        // A button whose label is named once and used twice — "Make
        // Default", "AutoCorrect Options..." — is written as a constant,
        // and the constant is where the words are.
        ": &str = ",
    ] {
        let mut from = 0usize;
        while let Some(at) = text[from..].find(call) {
            let start = from + at;
            from = start + call.len();
            // `t(` at the end of a longer name — `format!(`, `insert(` — is
            // not this call. The letter before it settles that.
            // Only a call can be the tail of a longer name; `label: "` and
            // `: &str = "` are preceded by a name by definition.
            if start > 0 && call.contains("(") {
                let before = bytes[start - 1];
                if before.is_ascii_alphanumeric() || before == b'_' {
                    continue;
                }
            }
            // The literal may be on the next line: a long call is broken
            // where it fits, and where it is broken is not the question.
            let Some(quote) = text[from..].find(|character: char| !character.is_whitespace())
            else {
                continue;
            };
            if bytes.get(from + quote) != Some(&b'"') {
                continue;
            }
            let Some(literal) = literal_at(text, from + quote) else { continue };
            // A message is something to read; a one-letter marker or an empty
            // string is not, and neither is a name the program uses to talk
            // to itself — a key in a file, an address, a marker.
            //
            // Except inside `t(` and `with(`, where whatever is written is a
            // message by construction: those two calls exist to look a
            // message up. The guess about names is for the looser shapes — a
            // label, a constant — where a program's own name and a person's
            // words look alike. Without the exception a message that happens
            // to be one lowercase word, like the "locked" beside a style
            // nobody may use, is dropped for looking like an identifier.
            let asked_for = call == "t(" || call == "with(";
            if literal.chars().count() > 1 && (asked_for || !is_a_name(&literal)) {
                out.insert(literal);
            }
        }
    }
    out
}

/// Whether a string is the program talking to itself rather than to the
/// person: a key written into a file, an address, the name of a clipboard
/// format. None of them has a translation, and a catalogue listing them
/// would be asking a translator to break the program.
fn is_a_name(text: &str) -> bool {
    if text.contains(' ') {
        return false;
    }
    // The folder this program keeps its own files in is its own name,
    // spelled without a space so that it can be one.
    if text == "WordProcessor" {
        return true;
    }
    // A file name is a file name in every language: `Normal.dotm` is what
    // the file on disk is called, and a translated one would point at
    // nothing.
    if is_a_file_name(text) {
        return true;
    }
    // And a colour is six hexadecimal digits, which is the same colour to
    // everybody.
    if is_a_colour(text) {
        return true;
    }
    text.starts_with("http")
        || text.contains('/')
        || text.contains('_')
        || (text.contains('-') && text == text.to_lowercase())
        || text.chars().all(|character| character.is_lowercase() || character == '.')
}

/// Whether a word is a colour: six hexadecimal digits, as the format writes
/// one.
///
/// A word of six letters that happen to be hexadecimal - `facade` - is all
/// lowercase and is already not a message by the rule above this one, so
/// what this has to catch is the ones with a digit or a capital in them.
fn is_a_colour(text: &str) -> bool {
    text.len() == 6 && text.chars().all(|character| character.is_ascii_hexdigit())
}

/// Whether a word is a file's name: something, a dot, and a short ending
/// that is all letters.
///
/// `Normal.dotm` and `settings.xml` are names; `Yours faithfully.` is a
/// sentence with a full stop, which is why the ending has to be short and
/// have no space before it.
fn is_a_file_name(text: &str) -> bool {
    let Some((stem, ending)) = text.rsplit_once('.') else { return false };
    !stem.is_empty()
        && (1..=5).contains(&ending.chars().count())
        && ending.chars().all(|character| character.is_ascii_alphanumeric())
}

/// The string literal beginning at the quotation mark, with its escapes
/// read, or nothing where it is not a plain literal.
fn literal_at(text: &str, quote: usize) -> Option<String> {
    let mut out = String::new();
    let mut characters = text[quote + 1..].chars();
    loop {
        match characters.next()? {
            '"' => return Some(out),
            '\\' => match characters.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                'u' => {
                    // `\u{2022}` and its like, which the messages do carry.
                    let mut digits = String::new();
                    for character in characters.by_ref() {
                        match character {
                            '{' => {}
                            '}' => break,
                            digit => digits.push(digit),
                        }
                    }
                    let code = u32::from_str_radix(&digits, 16).ok()?;
                    out.push(char::from_u32(code)?);
                }
                // Anything else is a literal this is not meant to read.
                _ => return None,
            },
            // A literal that runs over a line is written in the source with
            // a backslash at the end of the line, which is handled above.
            character => out.push(character),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The list every catalogue is written against.
    const LIST: &str = include_str!("../messages/en.txt");

    #[test]
    fn the_list_holds_every_message_the_interface_can_show() {
        let found = every_message();
        let listed = crate::messages::sources(LIST);

        let missing: Vec<&String> = found.difference(&listed).take(10).collect();
        assert!(
            missing.is_empty(),
            "messages the program says that messages/en.txt does not list \
             (run `cargo test -p wp-app -- --ignored write_the_message_list` to write it): {missing:#?}"
        );
        let stale: Vec<&String> = listed.difference(&found).take(10).collect();
        assert!(
            stale.is_empty(),
            "messages/en.txt lists what the program no longer says: {stale:#?}"
        );
    }

    /// The catalogue of a language lists what the program says, and all of it.
    ///
    /// The list is what a translator works from, and the two ways it can be
    /// wrong are both silent. A message the catalogue does not list is one
    /// nobody was asked to translate and one that comes out in English on a
    /// German machine. A message it lists that the program no longer says is
    /// work somebody did for nothing and a line that will puzzle the next
    /// person to read it.
    ///
    /// Neither shows up by using the program: English is the language the
    /// tests run in, and a missing translation looks exactly like a
    /// translation that was not needed. So it is checked here.
    #[test]
    fn every_language_lists_what_the_program_says_and_nothing_else() {
        let said = crate::messages::sources(LIST);
        for (tag, _, catalogue) in crate::messages::built_in() {
            let listed = crate::messages::sources(catalogue);

            let missing: Vec<&String> = said.difference(&listed).take(10).collect();
            assert!(
                missing.is_empty(),
                "messages/{tag}.txt does not list what the program says: {missing:#?}"
            );
            let stale: Vec<&String> = listed.difference(&said).take(10).collect();
            assert!(
                stale.is_empty(),
                "messages/{tag}.txt lists what the program no longer says: {stale:#?}"
            );

            // And every one of them is actually said in that language, since
            // a listed message with nothing under it is a message that comes
            // out in English.
            let untranslated: Vec<&str> = catalogue
                .split("\n\n")
                .filter_map(|block| {
                    let message = block.lines().find_map(|line| line.strip_prefix("= "))?;
                    block.lines().all(|line| !line.starts_with("> ")).then_some(message)
                })
                .take(10)
                .collect();
            assert!(
                untranslated.is_empty(),
                "messages/{tag}.txt lists these without saying them: {untranslated:#?}"
            );
        }
    }

    /// Writes the list, for when messages have been added or changed.
    ///
    /// A test rather than a program of its own because it needs the tables
    /// the program is built from, and ignored because it writes a file.
    #[test]
    #[ignore = "writes messages/en.txt"]
    fn write_the_message_list() {
        let found = every_message();
        let mut out = String::from(
            "# Every message the interface can show, in the order a translator\n\
             # reads them. Written by `cargo test -p wp-app -- --ignored\n\
             # write_the_message_list`, and held to by the test beside it.\n\
             #\n\
             # A catalogue for a language is this list with a line under each\n\
             # message saying it in that language. See messages/de.txt.\n\n",
        );
        for message in &found {
            out.push_str("= ");
            out.push_str(&message.replace('\\', "\\\\").replace('\n', "\\n"));
            out.push('\n');
        }
        std::fs::write("messages/en.txt", out).expect("writing the list");
        println!("{} messages", found.len());
    }

    #[test]
    fn a_message_is_found_where_it_stands_and_nowhere_else() {
        let source = r#"
            let a = t("Align Left");
            let b = messages::with("{0} of {1}", &[x, y]);
            let c = format!("not a message");
            let d = self.insert("not a message either");
            let e = t("One\nTwo");
        "#;
        let found = messages_in(source);
        assert!(found.contains("Align Left"));
        assert!(found.contains("{0} of {1}"));
        assert!(found.contains("One\nTwo"), "an escape is read as what it stands for");
        assert!(!found.contains("not a message"), "`format!(` is not `t(`");
        assert!(!found.contains("not a message either"), "and neither is `insert(`");
    }

    #[test]
    fn the_tables_give_up_the_labels_that_are_never_written_as_calls() {
        let found = from_the_tables();
        assert!(found.contains("Home"), "a tab");
        assert!(found.contains("Clipboard"), "a group");
        assert!(found.contains("Format Painter"), "a button");
        assert!(found.contains("Info"), "a place on the File page");
        assert!(found.contains("Headings"), "a section of the navigation pane");
    }
}
