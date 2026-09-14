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

use wp_dict::bilingual::{self, Bilingual};
use wp_docx::translate::Glossary;
use wp_docx::TextPosition;
use wp_shell::dialog::FileFilter;
use wp_shell::Response;

use crate::chrome::dialog::{Answer, Button, Dialog, Field};
use crate::chrome::popup::{Kind, Row};
use crate::chrome::{Choice, Command, Popup};

use super::dialogs::Asking;
use super::Editor;

/// The three lines under the button, in Word's order and Word's words.
pub(super) const TRANSLATE_MENU: &[&str] =
    &["Translate Selection", "Translate Document\u{2026}", "Translator Preferences\u{2026}"];

/// How wide the list of meanings is drawn.
const WIDTH: f32 = 300.0;

/// How many senses of one word the list shows, when the selection is more
/// than one word: enough to choose from, few enough that three words fit.
const SENSES_EACH: usize = 4;

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

    /// What the selection is in the other language, word by word, listed
    /// under the button.
    pub(super) fn translate_selection(&mut self) -> Response {
        let Some((paragraph, range)) = self.stretch_to_translate() else {
            return self.report("Select a word or a few words to translate");
        };
        let Some(text) = self.document.paragraph_text(paragraph) else { return Response::Ignored };
        let from = self.document.language_at(TextPosition::new(paragraph, range.start));
        let from_base = base_language(&from);

        let Some(to) = self.language_to_translate_into(&from_base) else {
            return self.report(&no_dictionary(&from_base));
        };
        let Some(dictionary) = self.bilingual_for(&from_base, &to) else {
            return self.report(&no_dictionary(&from_base));
        };

        // The words of the stretch, each looked up on its own — and the
        // stretch as a whole first, because "give up" is in the dictionary and
        // is not the sum of its words.
        let stretch = &text[range.clone()];
        let mut words: Vec<(core::ops::Range<usize>, Vec<bilingual::Meaning>)> = Vec::new();
        let whole = dictionary.meanings(stretch);
        if !whole.is_empty() {
            words.push((range.clone(), whole));
        } else {
            for (start, word) in word_ranges(stretch) {
                let at = range.start + start;
                let meanings = dictionary.meanings(word);
                words.push((at..at + word.len(), meanings));
            }
        }
        let known = words.iter().filter(|(_, meanings)| !meanings.is_empty()).count();
        if known == 0 {
            return self.report(&format!(
                "No entry for \u{201C}{stretch}\u{201D} in the {} dictionary",
                pair_name(&from_base, &to)
            ));
        }

        // The list: which way it translates, then each word as a heading that
        // cannot be chosen, then its senses — the translation, the kind of
        // word, and the dictionary's remark — each of which can be.
        let each = if words.len() > 1 { SENSES_EACH } else { usize::MAX };
        let mut items: Vec<String> = vec![pair_name(&from_base, &to)];
        let mut rows: Vec<Row> = vec![Row::new(Kind::Disabled, crate::chrome::icons::Icon::None)];
        let mut pending: Vec<Option<(usize, core::ops::Range<usize>, String)>> = vec![None];
        for (word_range, meanings) in &words {
            let word = &text[word_range.clone()];
            if meanings.is_empty() {
                items.push(format!("{word} \u{2014} no entry"));
            } else {
                items.push(word.to_owned());
            }
            rows.push(Row::new(Kind::Disabled, crate::chrome::icons::Icon::None));
            pending.push(None);
            for meaning in meanings.iter().take(each) {
                let mut line = format!("    {}", meaning.translations.join(", "));
                if let Some(kind) = &meaning.part_of_speech {
                    line.push_str(&format!("  ({kind})"));
                }
                if let Some(note) = &meaning.note {
                    line.push_str(&format!("  \u{2014} {note}"));
                }
                items.push(line);
                rows.push(Row::new(Kind::Choice, crate::chrome::icons::Icon::None));
                let first = meaning.translations.first().cloned().unwrap_or_default();
                pending.push(Some((paragraph, word_range.clone(), first)));
            }
        }

        // The stretch itself is selected, so that what a choice replaces is
        // plain to see before it is made.
        self.document.move_caret(TextPosition::new(paragraph, range.start), false);
        self.document.move_caret(TextPosition::new(paragraph, range.end), true);
        self.pending_translations = pending;
        let (left, top) = self
            .ribbon
            .command_rect(Command::Translate)
            .map_or((self.view_width as f32 / 2.0 - WIDTH / 2.0, 120.0), |(left, top, _)| {
                (left, top)
            });
        self.popup =
            Some(Popup::new(Choice::Translation, items, None, left, top, WIDTH).with_rows(rows));
        self.needs_redraw = true;
        self.report(&format!("\u{201C}{stretch}\u{201D} in {}", bilingual::language_name(&to)))
    }

    /// Puts the chosen sense in place of the word it was offered for.
    pub(super) fn take_translation(&mut self, index: usize) -> Response {
        self.popup = None;
        let Some(Some((paragraph, range, chosen))) = self.pending_translations.get(index).cloned()
        else {
            return Response::Ignored;
        };
        self.pending_translations.clear();

        self.document.move_caret(TextPosition::new(paragraph, range.start), false);
        self.document.move_caret(TextPosition::new(paragraph, range.end), true);
        // In the case the word had: "House" for "Haus" where the sentence
        // began with it.
        let selected = self.document.selected_text();
        let replacement = super::thesaurus::match_case(&chosen, &selected);
        let changed = self.document.paste(&replacement);
        self.relayout();
        self.reveal_caret();
        self.edited(changed, &format!("Changed to \u{201C}{replacement}\u{201D}"))
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
    fn language_to_translate_into(&self, from: &str) -> Option<String> {
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
    fn bilingual_for(&mut self, from: &str, to: &str) -> Option<&Bilingual> {
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
        let Some(path) = wp_shell::dialog::open_file("Glossary", &filters) else {
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
fn base_language(language: &str) -> String {
    language.split(['-', '_']).next().unwrap_or(language).to_lowercase()
}

/// "English → German".
fn pair_name(from: &str, to: &str) -> String {
    format!("{} \u{2192} {}", bilingual::language_name(from), bilingual::language_name(to))
}

/// What to say when there is nothing to translate with.
fn no_dictionary(from: &str) -> String {
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
fn word_ranges(text: &str) -> Vec<(usize, &str)> {
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
        use wp_docx::{Document, TextPosition};
        use wp_layout::FontLibrary;
        use wp_shell::{App, Event};

        use crate::chrome::Choice;
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
        fn a_word_is_listed_in_the_other_language_and_the_choice_replaces_it() {
            let mut editor = editor("The house is big.");
            editor.document.set_caret(TextPosition::new(0, 6));
            editor.translate_selection();
            assert!(
                editor.popup.as_ref().is_some_and(|popup| popup.choice == Choice::Translation),
                "the list did not open: {}",
                editor.status
            );
            let popup = editor.popup.as_ref().unwrap();
            assert_eq!(popup.item(0), Some("English \u{2192} German"));
            assert_eq!(popup.item(1), Some("house"));

            let first = editor
                .pending_translations
                .iter()
                .position(Option::is_some)
                .expect("something to choose");
            let (_, _, chosen) = editor.pending_translations[first].clone().unwrap();
            editor.take_translation(first);
            let text = editor.document.plain_text();
            assert_eq!(text.trim_end(), format!("The {chosen} is big."));
            assert!(
                chosen.chars().next().is_some_and(char::is_uppercase),
                "{chosen}: a German noun"
            );
        }

        #[test]
        fn several_words_are_each_listed_with_a_few_senses() {
            let mut editor = editor("the happy dog");
            editor.document.set_caret(TextPosition::new(0, 0));
            editor.document.extend_selection_to(TextPosition::new(0, 13));
            editor.translate_selection();
            let popup = editor.popup.as_ref().expect("the list");
            let headings: Vec<&str> = (0..40)
                .filter_map(|index| popup.item(index))
                .filter(|item| !item.starts_with(' ') && !item.contains('\u{2192}'))
                .collect();
            assert_eq!(headings, vec!["the", "happy", "dog"]);
            // A sense of "happy" replaces "happy" and nothing else.
            let happy = editor
                .pending_translations
                .iter()
                .position(|entry| entry.as_ref().is_some_and(|(_, range, _)| range.start == 4))
                .expect("a sense of happy");
            editor.take_translation(happy);
            let text = editor.document.plain_text();
            assert!(text.starts_with("the "), "{text}");
            assert!(text.trim_end().ends_with(" dog"), "{text}");
            assert!(!text.contains("happy"), "{text}");
        }

        #[test]
        fn a_word_with_no_entry_says_so() {
            let mut editor = editor("xqzv here.");
            editor.document.set_caret(TextPosition::new(0, 1));
            editor.translate_selection();
            assert!(editor.popup.is_none());
            assert!(editor.status.contains("No entry"), "{}", editor.status);
        }

        #[test]
        fn a_language_with_no_dictionary_says_which_there_are() {
            let mut editor = editor("Bonjour");
            editor.document.set_caret(TextPosition::new(0, 0));
            editor.document.extend_selection_to(TextPosition::new(0, 7));
            editor.document.set_language("fr-FR");
            editor.document.set_caret(TextPosition::new(0, 2));
            editor.translate_selection();
            assert!(editor.popup.is_none());
            assert!(editor.status.contains("from French"), "{}", editor.status);
            assert!(editor.status.contains("English \u{2192} German"), "{}", editor.status);
        }

        #[test]
        fn the_preferences_offer_the_languages_the_machine_has() {
            let mut editor = editor("Hello");
            editor.open_translator_preferences();
            let dialog = editor.dialog.as_ref().expect("the dialog");
            assert_eq!(dialog.title, "Translator Preferences");
            assert_eq!(editor.pending_targets, vec!["de"]);
        }
    }
}
