//! Translating, from the Review tab.
//!
//! # What Word does
//!
//! Translate is a menu of three: Translate Selection, which opens the
//! Translator pane with the selection in it and what it is in the other
//! language under it; Translate Document, which makes a translated copy of the
//! whole document; and Translator Preferences. All three send the text to
//! Microsoft's servers and show what comes back.
//!
//! # What this does instead
//!
//! This program talks to nobody, not because it would be hard but because a
//! word processor that quietly posts the document somebody is writing to a
//! third party is a different kind of program, and the choice belongs to
//! whoever is writing it. So each of the three does what can be done on the
//! machine:
//!
//! - Translate Selection asks the bilingual dictionary the machine has — the
//!   open format `dict` reads, see [`wp_dict::bilingual`] — what each word of
//!   the selection is in the other language, sense by sense, and lists it
//!   under the button; choosing a sense puts it in place of the word. That is
//!   what Word's pane shows for one word, and the part of translation that
//!   needs no network.
//! - Translate Document applies a glossary: a file of `source = target` lines
//!   the person supplies, every term of it replaced through the document.
//!   That is the part of translating a document a machine does reliably —
//!   terminology, the words that have to come out the same every time — and
//!   [`wp_docx::translate`] says the rest.
//! - Translator Preferences says which language to translate into, from
//!   among the dictionaries the machine has.

use crate::messages::t;
use wp_dict::bilingual::{self, Bilingual};
use wp_docx::translate::Glossary;
use wp_docx::TextPosition;
use wp_shell::dialog::FileFilter;
use wp_shell::Response;

use crate::chrome::dialog::{Answer, Button, Dialog, Field};
use crate::chrome::{Choice, Command, Popup};

use super::dialogs::Asking;
use super::Editor;

/// The three lines under the button, in Word's order and Word's words.
pub(super) const TRANSLATE_MENU: &[&str] =
    &["Translate Selection", "Translate Document\u{2026}", "Translator Preferences\u{2026}"];

/// The rows of the Translator Preferences dialog.
const FROM: usize = 0;
const TO: usize = 1;
const HAVE: usize = 2;

impl Editor {
    /// Drops the menu under the Translate button.
    pub(super) fn open_translate_menu(&mut self) -> Response {
        if self.close_popup_if(Choice::Translate) {
            return Response::Redraw;
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::Translate) else {
            return Response::Ignored;
        };
        let items = TRANSLATE_MENU.iter().map(|line| (*line).to_owned()).collect();
        self.popup = Some(Popup::new(Choice::Translate, items, None, left, top, 240.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// One of the three was picked.
    pub(super) fn choose_translate(&mut self, index: usize) -> Response {
        self.popup = None;
        match index {
            0 => self.translate_selection(),
            1 => self.translate_document(),
            2 => self.open_translator_preferences(),
            _ => Response::Ignored,
        }
    }

    /// Word's Translate Selection: the Translator pane, with the selection —
    /// or the word at the caret — in its box and what the dictionary says of
    /// it under that. Nothing selected and no word at the caret opens it
    /// empty, to be typed into.
    pub(super) fn translate_selection(&mut self) -> Response {
        let Some((paragraph, range)) = self.stretch_to_translate() else {
            self.open_translator(String::new(), None);
            return Response::Redraw;
        };
        let text = self.document.paragraph_text(paragraph).unwrap_or_default();
        let stretch = text.get(range.clone()).unwrap_or_default().to_owned();
        // The stretch itself is selected, so that what a choice replaces is
        // plain to see before it is made.
        self.document.move_caret(TextPosition::new(paragraph, range.start), false);
        self.document.move_caret(TextPosition::new(paragraph, range.end), true);
        self.open_translator(stretch, Some((paragraph, range.start)));
        Response::Redraw
    }

    /// The stretch to translate: the selection, if it is within one
    /// paragraph, or else the word at the caret.
    fn stretch_to_translate(&self) -> Option<(usize, core::ops::Range<usize>)> {
        let caret = self.document.caret();
        let text = self.document.paragraph_text(caret.paragraph)?;
        if let Some((start, end)) = self.document.selection() {
            if start.paragraph == end.paragraph && start.paragraph == caret.paragraph {
                let stretch = text.get(start.offset..end.offset)?;
                let trimmed = stretch.trim();
                if !trimmed.is_empty() {
                    let lead = stretch.len() - stretch.trim_start().len();
                    return Some((
                        caret.paragraph,
                        start.offset + lead..start.offset + lead + trimmed.len(),
                    ));
                }
            }
            return None;
        }

        let offset = caret.offset.min(text.len());
        let mut word = wp_segment::word_at(&text, offset);
        if word.start == offset && offset > 0 {
            let earlier = wp_segment::word_at(&text, offset - 1);
            if text[earlier.clone()].chars().any(char::is_alphanumeric) {
                word = earlier;
            }
        }
        if word.is_empty() || !text[word.clone()].chars().any(char::is_alphanumeric) {
            return None;
        }
        Some((caret.paragraph, word))
    }

    /// Which language to translate into: the one the preferences say, if a
    /// dictionary from this language into it is on the machine, or else the
    /// first language any dictionary from this one goes to.
    pub(super) fn language_to_translate_into(&self, from: &str) -> Option<String> {
        let installed = bilingual::installed();
        let goes_to = |to: &str| installed.iter().any(|pair| pair.from == from && pair.to == to);
        if let Some(wanted) = &self.settings.translate_to {
            let wanted = base_language(wanted);
            if goes_to(&wanted) {
                return Some(wanted);
            }
        }
        installed.iter().find(|pair| pair.from == from).map(|pair| pair.to.clone())
    }

    /// The dictionary from one language to another, opened the first time it
    /// is asked for. Reading the index of a large one takes a moment, and
    /// nothing changes on the machine while the program runs.
    pub(super) fn bilingual_for(&mut self, from: &str, to: &str) -> Option<&Bilingual> {
        let key = (from.to_owned(), to.to_owned());
        if !self.bilinguals.contains_key(&key) {
            let opened = bilingual::installed()
                .into_iter()
                .find(|pair| pair.from == from && pair.to == to)
                .and_then(|pair| Bilingual::open(&pair.index).ok());
            self.bilinguals.insert(key.clone(), opened);
        }
        self.bilinguals.get(&key).and_then(Option::as_ref)
    }

    /// Word's Translator Preferences, as far as there is anything to prefer:
    /// which language to translate into, from among the dictionaries the
    /// machine has.
    pub(super) fn open_translator_preferences(&mut self) -> Response {
        let caret = self.document.caret();
        let from = base_language(&self.document.language_at(caret));
        let installed = bilingual::installed();

        let mut targets: Vec<String> = Vec::new();
        for pair in &installed {
            if !targets.contains(&pair.to) {
                targets.push(pair.to.clone());
            }
        }
        let current = self
            .settings
            .translate_to
            .as_deref()
            .map(base_language)
            .and_then(|wanted| targets.iter().position(|to| *to == wanted))
            .unwrap_or(0);
        let items: Vec<String> = if targets.is_empty() {
            vec!["(no bilingual dictionary on this machine)".to_owned()]
        } else {
            targets.iter().map(|to| bilingual::language_name(to)).collect()
        };

        // What there is to translate with, spelled out: the answer to "why
        // can this not translate into French" is that nothing on the machine
        // does.
        let have = if installed.is_empty() {
            "none".to_owned()
        } else {
            installed
                .iter()
                .map(|pair| pair_name(&pair.from, &pair.to))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let fields = vec![
            Field::Said {
                label: "Translate from".to_owned(),
                value: format!("{} (the language of the text)", bilingual::language_name(&from)),
            },
            Field::Choice { label: "Translate to".to_owned(), items, current },
            Field::Said { label: "Dictionaries on this machine".to_owned(), value: have },
        ];
        crate::chrome::dialog::check_rows(
            "Translator Preferences",
            &fields,
            &[(FROM, "a line"), (TO, "a list"), (HAVE, "a line")],
        );
        let dialog = Dialog::with_buttons(
            "Translator Preferences",
            fields,
            vec![
                Button { label: "OK".to_owned(), answer: Answer::Accept, default: true },
                Button { label: "Cancel".to_owned(), answer: Answer::Cancel, default: false },
            ],
        )
        .wide(480.0);
        self.pending_targets = targets;
        self.ask(Asking::Translator, dialog)
    }

    /// What OK does: the language chosen is remembered.
    pub(super) fn apply_translator_preferences(&mut self, dialog: &Dialog) -> Response {
        let chosen = dialog.chose(TO);
        let Some(to) = self.pending_targets.get(chosen).cloned() else {
            return Response::Redraw;
        };
        self.pending_targets.clear();
        self.settings.translate_to = Some(to.clone());
        self.settings.save();
        self.report(&format!("Translating into {}", bilingual::language_name(&to)))
    }

    /// Translates the document with a glossary, asking for one the first
    /// time.
    pub(super) fn translate_document(&mut self) -> Response {
        if self.glossary.is_empty() {
            return self.load_glossary();
        }

        let selection = self.document.selection().is_some();
        let glossary = core::mem::take(&mut self.glossary);
        let replaced = self.document.translate(&glossary, selection);
        self.glossary = glossary;

        if replaced == 0 {
            return self.report("No term of the glossary was found");
        }
        self.relayout();
        self.reveal_caret();
        let where_ = if selection { "the selection" } else { "the document" };
        self.edited(true, &format!("{replaced} terms translated in {where_}"))
    }

    /// Asks for the glossary to translate with.
    pub(super) fn load_glossary(&mut self) -> Response {
        let filters = [
            FileFilter { label: "Glossaries", pattern: "*.txt" },
            FileFilter { label: "All files", pattern: "*.*" },
        ];
        let Some(path) = wp_shell::dialog::open_file(t("Glossary"), &filters) else {
            return Response::Ignored;
        };

        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) => return self.report(&format!("The glossary could not be read: {error}")),
        };

        let glossary = Glossary::parse(&bytes);
        if glossary.is_empty() {
            return self.report("That file holds no terms — one per line, as source = target");
        }
        let count = glossary.len();
        self.glossary = glossary;
        self.report(&format!("{count} terms — choose Translate Document again to use them"))
    }
}

/// The language without its region: "en" for "en-US".
pub(super) fn base_language(language: &str) -> String {
    language.split(['-', '_']).next().unwrap_or(language).to_lowercase()
}

/// "English → German".
pub(super) fn pair_name(from: &str, to: &str) -> String {
    format!("{} \u{2192} {}", bilingual::language_name(from), bilingual::language_name(to))
}

/// What to say when there is nothing to translate with.
pub(super) fn no_dictionary(from: &str) -> String {
    let installed = bilingual::installed();
    if installed.is_empty() {
        "No bilingual dictionary on this machine: this program translates with what the machine \
         has, and sends nothing anywhere"
            .to_owned()
    } else {
        format!(
            "No dictionary on this machine from {}; there is {}",
            bilingual::language_name(from),
            installed
                .iter()
                .map(|pair| pair_name(&pair.from, &pair.to))
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

/// The words of a stretch of text and where each begins in it.
pub(super) fn word_ranges(text: &str) -> Vec<(usize, &str)> {
    let mut words = Vec::new();
    let mut at = 0;
    while at < text.len() {
        let range = wp_segment::word_at(text, at);
        if range.is_empty() || range.end <= at {
            at += text[at..].chars().next().map_or(1, char::len_utf8);
            continue;
        }
        // A hyphenated word is one word to a dictionary, whatever the
        // segmentation rules say: "well-known" has an entry, "well" and
        // "known" have others.
        let mut end = range.end;
        while text[end..].starts_with('-') {
            let next = wp_segment::word_at(text, end + 1);
            if next.start != end + 1 || !text[next.clone()].chars().any(char::is_alphanumeric) {
                break;
            }
            end = next.end;
        }
        let word = &text[range.start..end];
        if word.chars().any(char::is_alphanumeric) {
            words.push((range.start, word));
        }
        at = end;
    }
    words
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stretch_is_cut_into_its_words() {
        assert_eq!(word_ranges("the happy dog"), vec![(0, "the"), (4, "happy"), (10, "dog")]);
        assert_eq!(word_ranges("well-known, yes"), vec![(0, "well-known"), (12, "yes")]);
        assert_eq!(word_ranges(""), Vec::<(usize, &str)>::new());
    }

    #[test]
    fn a_language_is_named_without_its_region() {
        assert_eq!(base_language("en-US"), "en");
        assert_eq!(base_language("de_DE"), "de");
        assert_eq!(pair_name("en", "de"), "English \u{2192} German");
    }

    /// Held to the dictionary the build image has, because a dictionary
    /// written for the test would prove the list and not the reading.
    mod against_the_machine {
        use wp_docx::model::{Block, Body, Paragraph};
        use wp_docx::Document;
        use wp_layout::FontLibrary;
        use wp_shell::{App, Event};

        use crate::editor::Editor;

        fn library() -> &'static FontLibrary {
            Box::leak(Box::new(FontLibrary::scan_system()))
        }

        fn editor(text: &str) -> Editor {
            let mut body = Body::default();
            body.blocks.push(Block::Paragraph(Paragraph::text(text)));
            let bytes = Document::create(&body).expect("a document").save().expect("saving");
            let document = Document::open(&bytes).expect("reopening");
            let mut editor = Editor::new(library(), document, None);
            editor.handle(Event::Resized { width: 1400, height: 900 });
            editor
        }

        #[test]
        fn the_preferences_offer_the_languages_the_machine_has() {
            let mut editor = editor("Hello");
            editor.open_translator_preferences();
            let dialog = editor.dialog.as_ref().expect("the dialog");
            assert_eq!(dialog.title, "Translator Preferences");
            for wanted in ["de", "fr", "ru"] {
                assert!(editor.pending_targets.iter().any(|to| to == wanted), "{wanted}");
            }
        }
    }
}
