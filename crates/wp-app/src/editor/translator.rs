//! The Translator pane: what the editor keeps for it and what it does.
//!
//! The pane is opened by Translate Selection with the selection in its box,
//! or empty, and whatever is in the box is looked up as it changes: the
//! stretch as a whole where the dictionary has it — "give up" is not the sum
//! of its words — and word by word where it has not. Choosing a sense puts it
//! into the document: in place of the word it was offered for, when the box
//! still holds the words that came from there, and at the caret when it holds
//! something typed.

use wp_dict::bilingual;
use wp_docx::TextPosition;
use wp_shell::{Key, Response};

use crate::chrome::popup::{Kind, Row};
use crate::chrome::translatorpane::{Hit, Line, Shown, WIDTH};
use crate::chrome::{Choice, Popup};
use crate::messages;

use super::translate::{base_language, no_dictionary, pair_name, word_ranges};
use super::Editor;

/// How many senses of one word the pane lists when the box holds more than
/// one word: enough to choose from, few enough that three words fit.
const SENSES_EACH: usize = 4;

/// What the pane is about.
#[derive(Clone, Debug, Default)]
pub(super) struct Translator {
    /// The two languages, as their two-letter codes.
    pub from: String,
    pub to: String,
    /// What the box holds.
    pub text: String,
    /// Where the caret stands in it, in characters.
    pub caret: usize,
    /// Whether the box has the keyboard.
    pub typing: bool,
    /// Where the box's words came from in the document: the paragraph, the
    /// offset they began at, and the words as they were — which the box
    /// still holds for as long as nobody has typed into it.
    pub source: Option<(usize, usize, String)>,
    /// What was last looked up, and what the dictionary said: kept so that
    /// drawing the pane does not look everything up again.
    looked_up: Option<Looked>,
    /// Which of the two lists is dropped open: the one the words are from,
    /// or the one they go into.
    choosing_from: bool,
    /// The languages that list offers.
    choosing: Vec<String>,
}

/// One lookup: of what, and its answer.
#[derive(Clone, Debug)]
struct Looked {
    key: (String, String, String),
    lines: Vec<Line>,
    /// For each line, the stretch of the box it is a sense for and the word
    /// it would put in.
    takes: Vec<Option<(core::ops::Range<usize>, String)>>,
    note: Option<String>,
}

impl Editor {
    /// How much of the window the pane takes, when it is open.
    #[must_use]
    pub(super) fn translator_pane_width(&self) -> f32 {
        if self.show_translator {
            WIDTH
        } else {
            0.0
        }
    }

    /// Where its left edge is.
    fn translator_left(&self) -> f32 {
        self.view_width as f32 - WIDTH
    }

    /// Whether a point is inside it.
    pub(super) fn over_translator(&self, x: i32) -> bool {
        let x = crate::chrome::mirror::flip(x);
        self.show_translator && (x as f32) >= self.translator_left()
    }

    /// Opens the pane with words in its box — the selection's, or none.
    pub(super) fn open_translator(&mut self, text: String, source: Option<(usize, usize)>) {
        // The panes down the right-hand side share one strip of window.
        self.show_styles = false;
        self.show_restrict = false;
        self.show_signatures = false;
        self.show_mapping = false;
        self.show_text_pane = false;

        // The language of the words, whether or not the machine has a
        // dictionary from it: where it has none the pane says so, and says
        // which there are, rather than quietly reading them as some other
        // language.
        let from = match source {
            Some((paragraph, start)) => {
                base_language(&self.document.language_at(TextPosition::new(paragraph, start)))
            }
            None if !self.translator.from.is_empty() => self.translator.from.clone(),
            None => base_language(&self.document.language_here()),
        };
        let to = self.language_to_translate_into(&from).unwrap_or_default();
        self.translator = Translator {
            caret: text.chars().count(),
            typing: source.is_none(),
            source: source.map(|(paragraph, start)| (paragraph, start, text.clone())),
            text,
            from,
            to,
            ..Translator::default()
        };
        self.show_translator = true;
        self.clamp_scroll();
        self.needs_redraw = true;
    }

    pub(super) fn close_translator(&mut self) -> Response {
        self.show_translator = false;
        self.translator.typing = false;
        self.clamp_scroll();
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Whether the box has the keyboard.
    pub(super) fn translator_has_keyboard(&self) -> bool {
        self.show_translator && self.translator.typing
    }

    /// What the pane draws, looked up if the box or the languages changed.
    pub(super) fn translator_shown(&mut self) -> Shown {
        let key = (
            self.translator.from.clone(),
            self.translator.to.clone(),
            self.translator.text.clone(),
        );
        if self.translator.looked_up.as_ref().is_none_or(|looked| looked.key != key) {
            let looked = self.look_up(key);
            self.translator.looked_up = Some(looked);
        }
        let looked = self.translator.looked_up.as_ref();
        Shown {
            from: bilingual::language_name(&self.translator.from),
            to: bilingual::language_name(&self.translator.to),
            text: self.translator.text.clone(),
            caret: self.translator.typing.then_some(self.translator.caret),
            lines: looked.map(|looked| looked.lines.clone()).unwrap_or_default(),
            note: looked.and_then(|looked| looked.note.clone()),
        }
    }

    /// What the dictionary says about what the box holds.
    fn look_up(&mut self, key: (String, String, String)) -> Looked {
        let (from, to, text) = key.clone();
        let nothing = |note: Option<String>| Looked {
            key: key.clone(),
            lines: Vec::new(),
            takes: Vec::new(),
            note,
        };
        let stretch = text.trim();
        if stretch.is_empty() {
            return nothing(None);
        }
        let Some(dictionary) = self.bilingual_for(&from, &to) else {
            return nothing(Some(no_dictionary(&from)));
        };

        // The stretch as a whole first, then its words.
        let lead = text.len() - text.trim_start().len();
        let mut words: Vec<(core::ops::Range<usize>, Vec<bilingual::Meaning>)> = Vec::new();
        let whole = dictionary.meanings(stretch);
        if whole.is_empty() {
            for (start, word) in word_ranges(stretch) {
                let at = lead + start;
                words.push((at..at + word.len(), dictionary.meanings(word)));
            }
        } else {
            words.push((lead..lead + stretch.len(), whole));
        }
        if words.iter().all(|(_, meanings)| meanings.is_empty()) {
            return nothing(Some(messages::with(
                "No entry for \u{201C}{0}\u{201D} in the {1} dictionary",
                &[stretch, &pair_name(&from, &to)],
            )));
        }

        let each = if words.len() > 1 { SENSES_EACH } else { usize::MAX };
        let mut lines = Vec::new();
        let mut takes = Vec::new();
        for (range, meanings) in words {
            let word = text[range.clone()].to_owned();
            if meanings.is_empty() {
                lines.push(Line::Missing(word));
                takes.push(None);
                continue;
            }
            lines.push(Line::Word(word));
            takes.push(None);
            for meaning in meanings.iter().take(each) {
                let mut line = meaning.translations.join(", ");
                if let Some(kind) = &meaning.part_of_speech {
                    line.push_str(&format!("  ({kind})"));
                }
                if let Some(note) = &meaning.note {
                    line.push_str(&format!("  \u{2014} {note}"));
                }
                lines.push(Line::Sense(line));
                let first = meaning.translations.first().cloned().unwrap_or_default();
                takes.push(Some((range.clone(), first)));
            }
        }
        Looked { key, lines, takes, note: None }
    }

    /// A press on the pane.
    pub(super) fn translator_press(&mut self, x: i32, y: i32) -> Response {
        let hit = self.translator_pane.at(x, y);
        // Anywhere else on the pane takes the keyboard away from the box; a
        // press in the box gives it back.
        self.translator.typing = hit == Some(Hit::Box);
        self.needs_redraw = true;
        match hit {
            Some(Hit::Close) => self.close_translator(),
            Some(Hit::Box) => {
                let along = self.translator_pane.along(x);
                self.translator.caret = self.characters_before(along);
                Response::Redraw
            }
            Some(Hit::Swap) => self.swap_languages(),
            Some(Hit::From) => self.drop_languages(true),
            Some(Hit::To) => self.drop_languages(false),
            Some(Hit::Sense(line)) => self.take_sense(line),
            None => Response::Redraw,
        }
    }

    /// How many characters of the box come before a point so far along it.
    fn characters_before(&mut self, along: f32) -> usize {
        let text = self.translator.text.clone();
        let mut count = 0;
        for (index, _) in text.char_indices().skip(1).chain([(text.len(), ' ')]) {
            let width = self
                .chrome_engine
                .simple_line(&text[..index], 0.0, 0.0, 8.5, self.theme.text)
                .width;
            if width > along {
                break;
            }
            count += 1;
        }
        count
    }

    /// The button between the two languages: from becomes to, where there is
    /// a dictionary that way round.
    fn swap_languages(&mut self) -> Response {
        let (from, to) = (self.translator.to.clone(), self.translator.from.clone());
        if !bilingual::installed().iter().any(|pair| pair.from == from && pair.to == to) {
            return self.report(&messages::with(
                "No dictionary on this machine from {0} into {1}",
                &[&bilingual::language_name(&from), &bilingual::language_name(&to)],
            ));
        }
        self.translator.from = from;
        self.translator.to = to;
        self.translator.source = None;
        Response::Redraw
    }

    /// Drops the list of languages under one of the two.
    fn drop_languages(&mut self, from: bool) -> Response {
        let installed = bilingual::installed();
        let mut offered: Vec<String> = Vec::new();
        for pair in &installed {
            let language = if from {
                &pair.from
            } else if pair.from == self.translator.from {
                &pair.to
            } else {
                continue;
            };
            if !offered.contains(language) {
                offered.push(language.clone());
            }
        }
        if offered.is_empty() {
            return self.report(&no_dictionary(&self.translator.from));
        }
        let hit = if from { Hit::From } else { Hit::To };
        let (left, top, width, height) = self.translator_pane.place_of(hit).unwrap_or((
            self.translator_left(),
            120.0,
            140.0,
            20.0,
        ));
        let items: Vec<String> =
            offered.iter().map(|code| bilingual::language_name(code)).collect();
        let current = offered.iter().position(|code| {
            code == if from { &self.translator.from } else { &self.translator.to }
        });
        let rows = offered
            .iter()
            .map(|_| Row::new(Kind::Choice, crate::chrome::icons::Icon::None))
            .collect();
        self.translator.choosing_from = from;
        self.translator.choosing = offered;
        self.popup = Some(
            Popup::new(
                Choice::TranslatorLanguage,
                items,
                current,
                left,
                top + height,
                width.max(160.0),
            )
            .with_rows(rows),
        );
        Response::Redraw
    }

    /// A language picked from the list.
    pub(super) fn choose_translator_language(&mut self, index: usize) -> Response {
        self.popup = None;
        let Some(language) = self.translator.choosing.get(index).cloned() else {
            return Response::Ignored;
        };
        if self.translator.choosing_from {
            if language != self.translator.from {
                self.translator.from = language.clone();
                self.translator.source = None;
                self.translator.to = self.language_to_translate_into(&language).unwrap_or_default();
            }
        } else {
            self.translator.to = language;
        }
        self.needs_redraw = true;
        Response::Redraw
    }

    /// A sense chosen: into the document, in place of its word or at the
    /// caret.
    fn take_sense(&mut self, line: usize) -> Response {
        let Some((range, chosen)) = self
            .translator
            .looked_up
            .as_ref()
            .and_then(|looked| looked.takes.get(line).cloned().flatten())
        else {
            return Response::Redraw;
        };
        // Where it goes: the word it was offered for, if the box still holds
        // what came from the document and the document still holds it there.
        let from_document = self.translator.source.as_ref().and_then(|(paragraph, start, came)| {
            let text = self.document.paragraph_text(*paragraph)?;
            let still_there = *came == self.translator.text
                && text.get(*start..start + came.len()) == Some(came.as_str());
            still_there.then_some((*paragraph, start + range.start, start + range.end))
        });
        let replaced = match from_document {
            Some((paragraph, start, end)) => {
                self.document.move_caret(TextPosition::new(paragraph, start), false);
                self.document.move_caret(TextPosition::new(paragraph, end), true);
                let selected = self.document.selected_text();
                super::thesaurus::match_case(&chosen, &selected)
            }
            None => chosen,
        };
        let changed = self.document.paste(&replaced);
        // What came from the document is not there any more as it was.
        if from_document.is_some() {
            self.translator.source = None;
        }
        self.relayout();
        self.reveal_caret();
        self.edited(changed, &messages::with("Changed to \u{201C}{0}\u{201D}", &[&replaced]))
    }

    /// A character typed into the box.
    pub(super) fn translator_character(&mut self, character: char) -> Response {
        let caret = self.translator.caret.min(self.translator.text.chars().count());
        let at = byte_at(&self.translator.text, caret);
        match character {
            '\u{8}' => {
                if caret > 0 {
                    let before = byte_at(&self.translator.text, caret - 1);
                    self.translator.text.replace_range(before..at, "");
                    self.translator.caret = caret - 1;
                }
            }
            '\u{1b}' => self.translator.typing = false,
            '\r' | '\n' | '\t' => {}
            character if character.is_control() => return Response::Ignored,
            character => {
                self.translator.text.insert(at, character);
                self.translator.caret = caret + 1;
            }
        }
        self.needs_redraw = true;
        Response::Redraw
    }

    /// A key pressed while the box has the keyboard.
    pub(super) fn translator_key(&mut self, key: Key) -> Response {
        let length = self.translator.text.chars().count();
        let caret = self.translator.caret.min(length);
        match key {
            Key::Left => self.translator.caret = caret.saturating_sub(1),
            Key::Right => self.translator.caret = (caret + 1).min(length),
            Key::Home => self.translator.caret = 0,
            Key::End => self.translator.caret = length,
            Key::Delete => {
                if caret < length {
                    let at = byte_at(&self.translator.text, caret);
                    let next = byte_at(&self.translator.text, caret + 1);
                    self.translator.text.replace_range(at..next, "");
                }
            }
            Key::Backspace => return self.translator_character('\u{8}'),
            Key::Escape => self.translator.typing = false,
            _ => return Response::Ignored,
        }
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Where the button between the two languages is, or the box, for
    /// something that has to press it without a pointer.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) fn translator_pane_place(&mut self, swap: bool) -> Option<(i32, i32)> {
        let (width, height) = (self.view_width, self.view_height);
        wp_shell::App::draw(self, width, height);
        let hit = if swap { Hit::Swap } else { Hit::Box };
        let (left, top, across, down) = self.translator_pane.place_of(hit)?;
        Some(((left + across / 2.0) as i32, (top + down / 2.0) as i32))
    }

    /// The pointer over the pane.
    pub(super) fn translator_hover(&mut self, x: i32, y: i32) -> bool {
        self.translator_pane.hover(x, y)
    }

    /// Draws it, over the document's right-hand edge.
    pub(super) fn draw_translator(&mut self) {
        if !self.show_translator {
            return;
        }
        let shown = self.translator_shown();
        let left = self.translator_left();
        let top = self.ribbon_bottom();
        let bottom = self.window_bottom();
        let theme = self.theme;
        let mut pane = core::mem::take(&mut self.translator_pane);
        pane.draw(
            &mut self.canvas,
            &mut self.chrome_engine,
            &mut self.renderer,
            &shown,
            left,
            top,
            bottom,
            &theme,
        );
        self.translator_pane = pane;
    }
}

/// Where a character of a text begins, in bytes.
fn byte_at(text: &str, characters: usize) -> usize {
    text.char_indices().nth(characters).map_or(text.len(), |(at, _)| at)
}

/// Held to the dictionaries the build image has, because a dictionary written
/// for the test would prove the pane and not the reading.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::chrome::translatorpane::Line;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::Document;
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event};

    fn editor(text: &str) -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text(text)));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let library: &'static FontLibrary = Box::leak(Box::new(FontLibrary::scan_system()));
        let mut editor = Editor::new(library, document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor
    }

    /// The first sense listed, and where it is.
    fn first_sense(shown: &Shown) -> (usize, String) {
        shown
            .lines
            .iter()
            .enumerate()
            .find_map(|(index, line)| match line {
                Line::Sense(sense) => Some((index, sense.clone())),
                _ => None,
            })
            .expect("a sense")
    }

    fn senses(shown: &Shown) -> Vec<String> {
        shown
            .lines
            .iter()
            .filter_map(|line| match line {
                Line::Sense(sense) => Some(sense.clone()),
                _ => None,
            })
            .collect()
    }

    fn type_into_box(editor: &mut Editor, text: &str) {
        for character in text.chars() {
            editor.handle(Event::Char(character));
        }
    }

    #[test]
    fn translate_selection_opens_the_pane_with_the_word_and_a_sense_replaces_it() {
        let mut editor = editor("The house is big.");
        editor.document.set_caret(TextPosition::new(0, 6));
        editor.translate_selection();
        assert!(editor.show_translator, "the pane did not open: {}", editor.status);

        let shown = editor.translator_shown();
        assert_eq!((shown.from.as_str(), shown.to.as_str()), ("English", "German"));
        assert_eq!(shown.text, "house");
        assert_eq!(shown.lines[0], Line::Word("house".to_owned()));
        assert_eq!(shown.caret, None, "the box took the keyboard from the page");

        let (line, _) = first_sense(&shown);
        editor.take_sense(line);
        let text = editor.document.plain_text();
        assert!(text.starts_with("The ") && text.trim_end().ends_with(" is big."), "{text}");
        assert!(!text.contains("house"), "{text}");
    }

    #[test]
    fn several_words_are_each_listed_and_a_sense_replaces_its_own_word() {
        let mut editor = editor("the happy dog");
        editor.document.set_caret(TextPosition::new(0, 0));
        editor.document.extend_selection_to(TextPosition::new(0, 13));
        editor.translate_selection();
        let shown = editor.translator_shown();
        let words: Vec<Line> =
            shown.lines.iter().filter(|line| matches!(line, Line::Word(_))).cloned().collect();
        assert_eq!(words, ["the", "happy", "dog"].map(|word| Line::Word(word.to_owned())));
        let happy = shown.lines.iter().position(|line| *line == Line::Word("happy".to_owned()));
        editor.take_sense(happy.expect("happy") + 1);
        let text = editor.document.plain_text();
        assert!(text.starts_with("the ") && text.trim_end().ends_with(" dog"), "{text}");
        assert!(!text.contains("happy"), "{text}");
    }

    #[test]
    fn a_word_typed_into_the_box_is_looked_up_and_put_in_at_the_caret() {
        let mut editor = editor("A  here.");
        editor.document.set_caret(TextPosition::new(0, 2));
        editor.open_translator(String::new(), None);
        assert!(editor.translator_has_keyboard(), "an empty pane is for typing into");
        type_into_box(&mut editor, "doh\u{8}g");
        assert_eq!(editor.document.plain_text().trim_end(), "A  here.", "typing reached the page");

        let shown = editor.translator_shown();
        assert_eq!(shown.text, "dog");
        assert_eq!(shown.caret, Some(3));
        let (line, sense) = first_sense(&shown);
        editor.take_sense(line);
        let wanted = sense.split([',', ' ']).next().unwrap_or_default().to_owned();
        assert!(
            editor.document.plain_text().starts_with(&format!("A {wanted} here")),
            "{} / {sense}",
            editor.document.plain_text()
        );
    }

    #[test]
    fn the_button_between_the_languages_turns_them_round() {
        let mut editor = editor("x");
        editor.open_translator(String::new(), None);
        editor.swap_languages();
        type_into_box(&mut editor, "Haus");
        let shown = editor.translator_shown();
        assert_eq!((shown.from.as_str(), shown.to.as_str()), ("German", "English"));
        assert!(senses(&shown).iter().any(|sense| sense.contains("house")), "{:?}", shown.lines);
    }

    #[test]
    fn the_lists_offer_every_pair_the_machine_has() {
        let mut editor = editor("x");
        editor.open_translator(String::new(), None);
        editor.drop_languages(true);
        let popup = editor.popup.as_ref().expect("the list of languages");
        assert_eq!(popup.choice, Choice::TranslatorLanguage);
        let offered: Vec<String> =
            (0..8).filter_map(|index| popup.item(index).map(str::to_owned)).collect();
        for wanted in ["English", "French", "German"] {
            assert!(offered.iter().any(|name| name == wanted), "{offered:?}");
        }

        // French, and French into English comes with it.
        let french = offered.iter().position(|name| name == "French").unwrap();
        editor.choose_translator_language(french);
        editor.translator.typing = true;
        type_into_box(&mut editor, "chat");
        let shown = editor.translator_shown();
        assert_eq!((shown.from.as_str(), shown.to.as_str()), ("French", "English"));
        assert!(senses(&shown).iter().any(|sense| sense.contains("cat")), "{:?}", shown.lines);

        // And English into Russian, the other way.
        editor.drop_languages(true);
        let english = (0..8)
            .position(|index| editor.popup.as_ref().unwrap().item(index) == Some("English"))
            .unwrap();
        editor.choose_translator_language(english);
        editor.drop_languages(false);
        let russian = (0..8)
            .position(|index| editor.popup.as_ref().unwrap().item(index) == Some("Russian"))
            .expect("English into Russian");
        editor.choose_translator_language(russian);
        editor.translator.text = "water".to_owned();
        let shown = editor.translator_shown();
        let found = senses(&shown);
        let cyrillic = |sense: &String| sense.chars().any(|c| ('\u{430}'..='\u{44F}').contains(&c));
        assert!(found.iter().any(cyrillic), "{found:?}");
    }

    #[test]
    fn a_word_with_no_entry_and_a_language_with_no_dictionary_say_so() {
        let mut editor = editor("xqzv here.");
        editor.document.set_caret(TextPosition::new(0, 1));
        editor.translate_selection();
        let shown = editor.translator_shown();
        assert!(shown.note.as_deref().is_some_and(|note| note.contains("No entry")), "{shown:?}");

        let mut editor = super::tests::editor("Ciao");
        editor.document.select_all();
        editor.document.set_language("it-IT");
        editor.document.set_caret(TextPosition::new(0, 1));
        editor.translate_selection();
        let shown = editor.translator_shown();
        let note = shown.note.unwrap_or_default();
        assert!(note.contains("from Italian"), "{note}");
        assert!(note.contains("English \u{2192} German"), "{note}");
    }

    #[test]
    fn the_pane_shares_the_right_of_the_window_with_the_others() {
        let mut editor = editor("x");
        let wide = editor.viewport_width();
        editor.open_translator(String::new(), None);
        assert!(editor.viewport_width() < wide, "the page did not make room");
        editor.open_text_pane();
        assert!(!editor.show_translator, "two panes at once");
        editor.open_translator(String::new(), None);
        assert!(!editor.show_text_pane);
        editor.close_translator();
        assert_eq!(editor.viewport_width(), wide);
    }
}
