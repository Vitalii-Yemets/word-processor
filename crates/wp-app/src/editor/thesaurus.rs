//! The thesaurus: what else the word at the caret could have been.
//!
//! # What Word does
//!
//! Shift+F7, or the button on the Review tab, or Synonyms on the right-click
//! menu: a list of the words that mean what the word means, grouped by which
//! of its meanings they share, with the opposites marked. Choosing one puts it
//! in place of the word.
//!
//! # Where the words come from
//!
//! From whatever thesaurus the machine has, in the open format LibreOffice
//! reads, on the same terms as the dictionaries: none is shipped with this
//! program. The one for the language of the word is used, and where there is
//! none the thesaurus has nothing to say — which it says, rather than showing
//! an empty list and leaving the reader to wonder.

use wp_docx::TextPosition;
use wp_shell::Response;

use crate::chrome::popup::{Kind, Row};
use crate::chrome::{Choice, Command, Popup};

use super::Editor;

/// How wide the list is drawn.
const WIDTH: f32 = 260.0;

impl Editor {
    /// Opens the list of synonyms for the word at the caret.
    pub(super) fn open_thesaurus(&mut self) -> Response {
        if self.popup.as_ref().is_some_and(|popup| popup.choice == Choice::Synonym) {
            self.popup = None;
            self.needs_redraw = true;
            return Response::Redraw;
        }

        let Some((paragraph, range)) = self.word_at_caret() else {
            return self.report("Put the caret in a word first");
        };
        let Some(text) = self.document.paragraph_text(paragraph) else { return Response::Ignored };
        let word = text[range.clone()].to_owned();
        let language = self.document.language_at(TextPosition::new(paragraph, range.start));

        let Some(thesaurus) = self.thesaurus_for(&language) else {
            return self.report(&format!("No thesaurus on this machine for {language}"));
        };
        let senses = thesaurus.senses(&word);
        if senses.is_empty() {
            return self.report(&format!("No synonyms for \u{201C}{word}\u{201D}"));
        }

        // The list: each meaning as a heading that cannot be chosen, then the
        // words that share it, then its opposites marked as such — which is
        // how Word's pane is laid out, one column of it.
        let mut items: Vec<String> = Vec::new();
        let mut rows: Vec<Row> = Vec::new();
        for sense in &senses {
            items.push(format!("{} ({})", sense.meaning, sense.part_of_speech));
            rows.push(Row::new(Kind::Disabled, crate::chrome::icons::Icon::None));
            for synonym in &sense.synonyms {
                if synonym.eq_ignore_ascii_case(&word) {
                    continue;
                }
                items.push(format!("    {synonym}"));
                rows.push(Row::new(Kind::Choice, crate::chrome::icons::Icon::None));
            }
            for antonym in &sense.antonyms {
                items.push(format!("    {antonym} (antonym)"));
                rows.push(Row::new(Kind::Choice, crate::chrome::icons::Icon::None));
            }
        }

        // The word itself is selected, so that what the choice replaces is
        // plain to see before it is made.
        self.document.move_caret(TextPosition::new(paragraph, range.start), false);
        self.document.move_caret(TextPosition::new(paragraph, range.end), true);
        self.pending_word = Some((paragraph, range.clone()));
        self.pending_synonyms = items
            .iter()
            .zip(&rows)
            .map(|(item, row)| {
                if row.kind == Kind::Choice {
                    Some(item.trim().trim_end_matches(" (antonym)").to_owned())
                } else {
                    None
                }
            })
            .collect();
        let (left, top) = self
            .ribbon
            .command_rect(Command::Thesaurus)
            .map_or((self.view_width as f32 / 2.0 - WIDTH / 2.0, 120.0), |(left, top, _)| {
                (left, top)
            });
        self.popup =
            Some(Popup::new(Choice::Synonym, items, None, left, top, WIDTH).with_rows(rows));
        self.needs_redraw = true;
        self.report(&format!("Synonyms for \u{201C}{word}\u{201D}"))
    }

    /// Puts the chosen word in place of the one it was offered for.
    pub(super) fn take_synonym(&mut self, index: usize) -> Response {
        self.popup = None;
        let Some(Some(chosen)) = self.pending_synonyms.get(index).cloned() else {
            return Response::Ignored;
        };
        self.pending_synonyms.clear();

        // The word it was offered for is selected, if it is not already:
        // from the right-click menu nothing was selected, only pointed at.
        if let Some((paragraph, range)) = self.pending_word.take() {
            self.document.move_caret(TextPosition::new(paragraph, range.start), false);
            self.document.move_caret(TextPosition::new(paragraph, range.end), true);
        }

        // In the case the word had: a synonym for "Happy" is "Glad".
        let selected = self.document.selected_text();
        let replacement = match_case(&chosen, &selected);
        let changed = self.document.paste(&replacement);
        self.relayout();
        self.reveal_caret();
        self.edited(changed, &format!("Changed to \u{201C}{replacement}\u{201D}"))
    }

    /// The first few synonyms of the word at a position, for the right-click
    /// menu: Word shows a handful there and the rest behind "Thesaurus".
    ///
    /// Fills in what the menu's entries will put in place of the word, and
    /// which word that is, and gives back the words to show.
    pub(super) fn synonyms_at(&mut self, at: TextPosition) -> Vec<String> {
        const SHOWN: usize = 6;
        let Some(text) = self.document.paragraph_text(at.paragraph) else { return Vec::new() };
        let range = wp_segment::word_at(&text, at.offset.min(text.len()));
        let word = text[range.clone()].to_owned();
        if word.is_empty() || !word.chars().any(char::is_alphanumeric) {
            return Vec::new();
        }
        let language = self.document.language_at(TextPosition::new(at.paragraph, range.start));
        let Some(thesaurus) = self.thesaurus_for(&language) else { return Vec::new() };

        let mut shown: Vec<String> = Vec::new();
        for sense in thesaurus.senses(&word) {
            for synonym in sense.synonyms {
                if shown.len() >= SHOWN {
                    break;
                }
                if !synonym.eq_ignore_ascii_case(&word) && !shown.contains(&synonym) {
                    shown.push(synonym);
                }
            }
        }
        if !shown.is_empty() {
            self.pending_word = Some((at.paragraph, range));
            self.pending_synonyms = shown.iter().cloned().map(Some).collect();
        }
        shown
    }

    /// The word the caret is in, or the selection if it is one word.
    fn word_at_caret(&self) -> Option<(usize, core::ops::Range<usize>)> {
        let caret = self.document.caret();
        let text = self.document.paragraph_text(caret.paragraph)?;
        if let Some((start, end)) = self.document.selection() {
            if start.paragraph == end.paragraph
                && start.paragraph == caret.paragraph
                && !text[start.offset..end.offset].trim().is_empty()
                && !text[start.offset..end.offset].contains(char::is_whitespace)
            {
                return Some((caret.paragraph, start.offset..end.offset));
            }
        }

        let offset = caret.offset.min(text.len());
        let mut word = wp_segment::word_at(&text, offset);
        // A caret at the end of a word is in that word, not in the gap after.
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

    /// The thesaurus for a language, opened the first time it is asked for.
    fn thesaurus_for(&mut self, language: &str) -> Option<&wp_dict::thesaurus::Thesaurus> {
        let key = language.replace('_', "-").to_lowercase();
        let base = key.split('-').next().unwrap_or_default().to_owned();
        if !self.thesauri.contains_key(&key) {
            let installed = wp_dict::thesaurus::installed();
            let found =
                installed.iter().find(|(name, _)| name.to_lowercase() == key).or_else(|| {
                    installed.iter().find(|(name, _)| {
                        name.split('-').next().unwrap_or_default().eq_ignore_ascii_case(&base)
                    })
                });
            let opened = found.and_then(|(_, path)| wp_dict::thesaurus::Thesaurus::open(path).ok());
            self.thesauri.insert(key.clone(), opened);
        }
        self.thesauri.get(&key).and_then(Option::as_ref)
    }
}

/// A word given the case of another: capitalised where it was, shouted where
/// it was.
pub(super) fn match_case(word: &str, like: &str) -> String {
    let shouted = like.chars().count() > 1 && like.chars().all(|c| !c.is_lowercase());
    let capital = like.chars().next().is_some_and(char::is_uppercase);
    if shouted {
        word.to_uppercase()
    } else if capital {
        let mut letters = word.chars();
        match letters.next() {
            Some(first) => first.to_uppercase().collect::<String>() + letters.as_str(),
            None => String::new(),
        }
    } else {
        word.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::match_case;

    #[test]
    fn a_synonym_takes_the_case_of_the_word_it_replaces() {
        assert_eq!(match_case("glad", "Happy"), "Glad");
        assert_eq!(match_case("glad", "HAPPY"), "GLAD");
        assert_eq!(match_case("glad", "happy"), "glad");
    }

    /// Held to the thesaurus the build image has, because a list of synonyms
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
        fn a_word_is_replaced_by_the_synonym_chosen_for_it() {
            let mut editor = editor("The happy dog.");
            // The caret inside "happy".
            editor.document.set_caret(TextPosition::new(0, 6));
            editor.open_thesaurus();
            assert!(
                editor.popup.as_ref().is_some_and(|popup| popup.choice == Choice::Synonym),
                "the list did not open: {}",
                editor.status
            );

            // The first line is a heading and cannot be chosen; the second is
            // the first word that means the same.
            let first = editor
                .pending_synonyms
                .iter()
                .position(Option::is_some)
                .expect("something to choose");
            let chosen = editor.pending_synonyms[first].clone().unwrap();
            editor.take_synonym(first);

            let text = editor.document.plain_text();
            assert_eq!(text.trim_end(), format!("The {chosen} dog."));
        }

        #[test]
        fn the_synonym_takes_the_case_of_the_word() {
            let mut editor = editor("Happy days.");
            editor.document.set_caret(TextPosition::new(0, 2));
            editor.open_thesaurus();
            let first = editor.pending_synonyms.iter().position(Option::is_some).unwrap();
            editor.take_synonym(first);
            let text = editor.document.plain_text();
            assert!(
                text.chars().next().is_some_and(char::is_uppercase),
                "the sentence lost its capital: {text}"
            );
        }

        #[test]
        fn a_word_with_no_synonyms_says_so() {
            let mut editor = editor("xqzv here.");
            editor.document.set_caret(TextPosition::new(0, 1));
            editor.open_thesaurus();
            assert!(editor.popup.is_none());
            assert!(editor.status.contains("No synonyms"), "{}", editor.status);
        }
    }
}
