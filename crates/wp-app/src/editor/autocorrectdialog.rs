//! Word's AutoCorrect dialog: which corrections are made, and what the list of
//! replacements holds.
//!
//! # Two dialogs, not one
//!
//! Word's has a second behind it — Exceptions — because two of its rules need
//! a list of the words they must leave alone: the abbreviations a sentence does
//! not begin after, and the words whose two capitals are meant. Both are here,
//! reached the way Word reaches them.
//!
//! # Where the working copy lives
//!
//! In [`Editor::editing_rules`], and not in the dialog. Add and Delete change a
//! list and leave the dialog standing, which means building it again; the
//! Exceptions button takes the dialog away altogether and puts it back. Neither
//! can keep what has been changed inside a dialog that is about to be thrown
//! away, so what has been changed is read out of it first.
//!
//! Nothing reaches [`Editor::autocorrect`] until OK is pressed, which is what
//! makes Cancel mean what it says.
//!
//! # What is missing
//!
//! Word's two "Automatically add words to list" boxes, which put a word on an
//! exception list when a correction is undone straight after it is made. They
//! are not drawn, because they are not done — see the roadmap. Word's Math
//! AutoCorrect, AutoFormat and Actions tabs are missing for the same reason
//! the switches they hold are: there is nothing behind them here.

use std::collections::BTreeSet;

use wp_shell::Response;

use crate::chrome::dialog::{Answer, Button, Dialog, Field};

use super::dialogs::Asking;
use super::Editor;

// The AutoCorrect tab.
const TAB_CORRECT: usize = 0;
const AS_YOU_TYPE: usize = 1;
const TWO_INITIALS: usize = 2;
const SENTENCE_CASE: usize = 3;
const DAY_NAMES: usize = 4;
const CAPS_LOCK: usize = 5;
const REPLACING: usize = 6;
const REPLACE_TEXT: usize = 7;
const PAIR_ROW: usize = 8;
const WHAT: usize = 9;
const WITH: usize = 10;
const LIST: usize = 11;

// The AutoFormat As You Type tab.
const TAB_FORMAT: usize = 12;
const REPLACE_AS_YOU_TYPE: usize = 13;
const CURLY_QUOTES: usize = 14;
const ORDINALS: usize = 15;
const FRACTIONS: usize = 16;
const DASHES: usize = 17;
const APPLY_AS_YOU_TYPE: usize = 18;
const AUTOMATIC_LISTS: usize = 19;

// The Exceptions dialog: two tabs, each a box and a list.
const TAB_FIRST_LETTER: usize = 0;
const FIRST_WORD: usize = 1;
const FIRST_LIST: usize = 2;
const TAB_INITIAL_CAPS: usize = 3;
const CAPS_WORD: usize = 4;
const CAPS_LIST: usize = 5;

/// Word's buttons past OK and Cancel.
pub(super) const ADD: &str = "Add";
pub(super) const DELETE: &str = "Delete";
pub(super) const EXCEPTIONS: &str = "Exceptions...";

impl Editor {
    /// Opens the dialog, taking a working copy of the corrections to edit.
    pub(super) fn open_autocorrect(&mut self) -> Response {
        self.open_autocorrect_with("")
    }

    /// The same, with something already in the "With" box.
    ///
    /// Word's Symbol dialog opens it this way: the character is chosen, and all
    /// that is left to say is what to type to get it.
    pub(super) fn open_autocorrect_with(&mut self, with: &str) -> Response {
        self.editing_rules = self.autocorrect.clone();
        let mut dialog = self.autocorrect_dialog(0);
        if let Some(Field::Text { value, .. }) = dialog.fields.get_mut(WITH) {
            *value = with.to_owned();
        }
        self.ask(Asking::AutoCorrect, dialog)
    }

    /// The dialog, filled in from the working copy.
    ///
    /// `chosen` is the row of the replacement list the keyboard is on, which
    /// Add and Delete both move: a list that jumped back to the top every time
    /// a word was deleted would make deleting three of them a hunt.
    fn autocorrect_dialog(&self, chosen: usize) -> Dialog {
        let rules = &self.editing_rules;
        let check = |label: &str, on: bool| Field::Check { label: label.to_owned(), on };
        let rows: Vec<(String, String)> =
            rules.replacements.iter().map(|(what, with)| (what.clone(), with.clone())).collect();
        let chosen = chosen.min(rows.len().saturating_sub(1));

        let fields = vec![
            // --- AutoCorrect ------------------------------------------------
            Field::Tab("AutoCorrect".to_owned()),
            Field::Group("As you type".to_owned()),
            check("Correct TWo INitial CApitals", rules.two_initials),
            check("Capitalize first letter of sentences", rules.sentence_case),
            check("Capitalize names of days", rules.day_names),
            check("Correct accidental use of cAPS LOCK key", rules.caps_lock),
            Field::Group("Replace text as you type".to_owned()),
            check("Replace text as you type", rules.replace_text),
            Field::Columns(2),
            Field::Text { label: "Replace".to_owned(), value: String::new() },
            Field::Text { label: "With".to_owned(), value: String::new() },
            Field::Pairs {
                label: "Replace".to_owned(),
                second: "With".to_owned(),
                rows,
                current: chosen,
                scroll: chosen.saturating_sub(crate::chrome::dialog::PAIR_ROWS - 1),
            },
            // --- AutoFormat As You Type -------------------------------------
            Field::Tab("AutoFormat As You Type".to_owned()),
            Field::Group("Replace as you type".to_owned()),
            check(
                "\u{201C}Straight quotes\u{201D} with \u{201C}smart quotes\u{201D}",
                rules.curly_quotes,
            ),
            check("Ordinals (1st) with superscript", rules.ordinals),
            check("Fractions (1/2) with fraction character", rules.fractions),
            check("Hyphens (--) with dash (\u{2014})", rules.dashes),
            Field::Group("Apply as you type".to_owned()),
            check("Automatic bulleted and numbered lists", rules.automatic_lists),
        ];

        crate::chrome::dialog::check_rows(
            "AutoCorrect",
            &fields,
            &[
                (TAB_CORRECT, "a tab"),
                (AS_YOU_TYPE, "a group"),
                (TWO_INITIALS, "a tick box"),
                (SENTENCE_CASE, "a tick box"),
                (DAY_NAMES, "a tick box"),
                (CAPS_LOCK, "a tick box"),
                (REPLACING, "a group"),
                (REPLACE_TEXT, "a tick box"),
                (PAIR_ROW, "a row"),
                (WHAT, "a box"),
                (WITH, "a box"),
                (LIST, "a list of pairs"),
                (TAB_FORMAT, "a tab"),
                (REPLACE_AS_YOU_TYPE, "a group"),
                (CURLY_QUOTES, "a tick box"),
                (ORDINALS, "a tick box"),
                (FRACTIONS, "a tick box"),
                (DASHES, "a tick box"),
                (APPLY_AS_YOU_TYPE, "a group"),
                (AUTOMATIC_LISTS, "a tick box"),
            ],
        );

        Dialog::with_buttons(
            "AutoCorrect",
            fields,
            vec![
                Button { label: "OK".to_owned(), answer: Answer::Accept, default: true },
                Button { label: ADD.to_owned(), answer: Answer::Named(ADD), default: false },
                Button { label: DELETE.to_owned(), answer: Answer::Named(DELETE), default: false },
                Button {
                    label: EXCEPTIONS.to_owned(),
                    answer: Answer::Named(EXCEPTIONS),
                    default: false,
                },
                Button { label: "Cancel".to_owned(), answer: Answer::Cancel, default: false },
            ],
        )
        .wide(600.0)
        // The three that work on the replacement list, which is on the first
        // tab: Word keeps them beside it, and so does this.
        .button_on_tab(Answer::Named(ADD), 0)
        .button_on_tab(Answer::Named(DELETE), 0)
        .button_on_tab(Answer::Named(EXCEPTIONS), 0)
    }

    /// Reads the switches out of the dialog and onto the working copy.
    ///
    /// The lists are not read back here: they are only ever changed by the
    /// buttons, which change the working copy themselves.
    fn read_autocorrect_dialog(&mut self, dialog: &Dialog) {
        let rules = &mut self.editing_rules;
        rules.two_initials = dialog.ticked(TWO_INITIALS);
        rules.sentence_case = dialog.ticked(SENTENCE_CASE);
        rules.day_names = dialog.ticked(DAY_NAMES);
        rules.caps_lock = dialog.ticked(CAPS_LOCK);
        rules.replace_text = dialog.ticked(REPLACE_TEXT);
        rules.curly_quotes = dialog.ticked(CURLY_QUOTES);
        rules.ordinals = dialog.ticked(ORDINALS);
        rules.fractions = dialog.ticked(FRACTIONS);
        rules.dashes = dialog.ticked(DASHES);
        rules.automatic_lists = dialog.ticked(AUTOMATIC_LISTS);
    }

    /// Add, Delete and Exceptions: none of them answers the dialog.
    pub(super) fn autocorrect_dialog_button(&mut self, button: &str) -> Response {
        let Some(dialog) = self.dialog.clone() else { return Response::Ignored };
        self.read_autocorrect_dialog(&dialog);

        let chosen = match button {
            ADD => {
                // What is typed in the two boxes, which is what Word adds.
                // Nothing typed adds nothing: a blank row on the list would be
                // a replacement of nothing by nothing.
                let what = dialog.said(WHAT).trim().to_lowercase();
                let with = dialog.said(WITH).trim().to_owned();
                if what.is_empty() || with.is_empty() {
                    self.editing_rules.replacements.len()
                } else {
                    self.editing_rules.replacements.insert(what.clone(), with);
                    self.editing_rules.replacements.keys().position(|key| *key == what).unwrap_or(0)
                }
            }
            DELETE => {
                let at = dialog.chose_pair(LIST);
                if let Some((what, _)) = dialog.pair(LIST) {
                    let what = what.to_owned();
                    self.editing_rules.replacements.remove(&what);
                }
                at
            }
            EXCEPTIONS => {
                // What the lists hold now, kept so that Cancel over there can
                // put them back. Add and Delete on that dialog change the
                // working copy as they go, which is what lets it be built
                // again after each of them.
                self.exceptions_stash = Some((
                    self.editing_rules.first_letter.clone(),
                    self.editing_rules.initial_caps.clone(),
                ));
                let dialog = self.exceptions_dialog(0, 0);
                return self.ask(Asking::Exceptions, dialog);
            }
            _ => return Response::Ignored,
        };

        let mut rebuilt = self.autocorrect_dialog(chosen);
        // Add empties the two boxes, as Word's does: the pair is on the list
        // now, and leaving it in the boxes invites it being added twice.
        if button != ADD {
            rebuilt.carry_typing_from(&dialog);
        } else {
            rebuilt.show_tab(dialog.showing_tab());
        }
        self.dialog = Some(rebuilt);
        self.needs_redraw = true;
        Response::Redraw
    }

    /// What OK does: the working copy becomes the real one, and is written
    /// down.
    pub(super) fn apply_autocorrect_dialog(&mut self, dialog: &Dialog) -> Response {
        self.read_autocorrect_dialog(dialog);
        self.autocorrect = self.editing_rules.clone();
        self.settings.autocorrect = Some(self.autocorrect.clone());
        self.settings.save();
        self.needs_redraw = true;
        Response::Redraw
    }

    // --- Exceptions ---------------------------------------------------------

    /// Word's Exceptions dialog, built from the working copy.
    fn exceptions_dialog(&self, first: usize, caps: usize) -> Dialog {
        // No heading over the list: the box above it is already labelled with
        // what the list holds, and Word puts nothing there either.
        let list = |words: &BTreeSet<String>, chosen: usize| {
            let rows: Vec<(String, String)> =
                words.iter().map(|word| (word.clone(), String::new())).collect();
            let chosen = chosen.min(rows.len().saturating_sub(1));
            Field::Pairs {
                label: String::new(),
                second: String::new(),
                rows,
                current: chosen,
                scroll: chosen.saturating_sub(crate::chrome::dialog::PAIR_ROWS - 1),
            }
        };
        let rules = &self.editing_rules;

        let fields = vec![
            Field::Tab("First Letter".to_owned()),
            Field::Text { label: "Don't capitalize after".to_owned(), value: String::new() },
            list(&rules.first_letter, first),
            Field::Tab("INitial CAps".to_owned()),
            Field::Text { label: "Don't correct".to_owned(), value: String::new() },
            list(&rules.initial_caps, caps),
        ];

        crate::chrome::dialog::check_rows(
            "AutoCorrect Exceptions",
            &fields,
            &[
                (TAB_FIRST_LETTER, "a tab"),
                (FIRST_WORD, "a box"),
                (FIRST_LIST, "a list of pairs"),
                (TAB_INITIAL_CAPS, "a tab"),
                (CAPS_WORD, "a box"),
                (CAPS_LIST, "a list of pairs"),
            ],
        );

        Dialog::with_buttons(
            "AutoCorrect Exceptions",
            fields,
            vec![
                Button { label: "OK".to_owned(), answer: Answer::Accept, default: true },
                Button { label: ADD.to_owned(), answer: Answer::Named(ADD), default: false },
                Button { label: DELETE.to_owned(), answer: Answer::Named(DELETE), default: false },
                Button { label: "Cancel".to_owned(), answer: Answer::Cancel, default: false },
            ],
        )
        .wide(460.0)
    }

    /// Add and Delete on the Exceptions dialog.
    ///
    /// Which of the two lists they work on is whichever tab is showing, which
    /// is how Word's does it: the buttons are the same two buttons on both.
    pub(super) fn exceptions_dialog_button(&mut self, button: &str) -> Response {
        let Some(dialog) = self.dialog.clone() else { return Response::Ignored };
        let on_first = dialog.showing_tab() == 0;
        let (box_row, list_row) =
            if on_first { (FIRST_WORD, FIRST_LIST) } else { (CAPS_WORD, CAPS_LIST) };

        let words = if on_first {
            &mut self.editing_rules.first_letter
        } else {
            &mut self.editing_rules.initial_caps
        };
        let mut chosen = dialog.chose_pair(list_row);
        match button {
            ADD => {
                // The first-letter list is matched against lower case, so it is
                // kept in lower case; the capitals list is matched as typed.
                let word = dialog.said(box_row).trim().to_owned();
                let word = if on_first { word.to_lowercase() } else { word };
                if !word.is_empty() {
                    words.insert(word.clone());
                    chosen = words.iter().position(|held| *held == word).unwrap_or(0);
                }
            }
            DELETE => {
                if let Some((word, _)) = dialog.pair(list_row) {
                    let word = word.to_owned();
                    words.remove(&word);
                }
            }
            _ => return Response::Ignored,
        }

        let (first_at, caps_at) = if on_first {
            (chosen, dialog.chose_pair(CAPS_LIST))
        } else {
            (dialog.chose_pair(FIRST_LIST), chosen)
        };
        let mut rebuilt = self.exceptions_dialog(first_at, caps_at);
        if button == ADD {
            // Emptied, as on the dialog behind: the word is on the list now.
            rebuilt.show_tab(dialog.showing_tab());
        } else {
            rebuilt.carry_typing_from(&dialog);
        }
        self.dialog = Some(rebuilt);
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Leaving the Exceptions dialog, one way or the other, and going back to
    /// the one it was opened from.
    ///
    /// The lists have been changed on the working copy as the buttons were
    /// pressed, so OK has nothing left to do and Cancel has everything: it puts
    /// back what was there when the dialog opened.
    pub(super) fn close_exceptions(&mut self, keeping: bool) -> Response {
        if let Some((first, caps)) = self.exceptions_stash.take() {
            if !keeping {
                self.editing_rules.first_letter = first;
                self.editing_rules.initial_caps = caps;
            }
        }

        let dialog = self.autocorrect_dialog(0);
        self.ask(Asking::AutoCorrect, dialog)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chrome::dialog::Field;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::Document;
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event};

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    fn editor() -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("Text")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor
    }

    fn tick(editor: &mut Editor, row: usize, on: bool) {
        if let Some(dialog) = &mut editor.dialog {
            if let Some(Field::Check { on: state, .. }) = dialog.fields.get_mut(row) {
                *state = on;
            }
        }
    }

    fn type_into(editor: &mut Editor, row: usize, text: &str) {
        if let Some(dialog) = &mut editor.dialog {
            if let Some(Field::Text { value, .. }) = dialog.fields.get_mut(row) {
                *value = text.to_owned();
            }
        }
    }

    /// Answers the dialog without letting it write to the settings file.
    fn accept(editor: &mut Editor) {
        let dialog = editor.dialog.take().expect("a dialog");
        editor.asking = None;
        editor.read_autocorrect_dialog(&dialog);
        editor.autocorrect = editor.editing_rules.clone();
    }

    #[test]
    fn the_dialog_opens_showing_which_corrections_are_made() {
        let mut editor = editor();
        editor.autocorrect.curly_quotes = false;
        editor.open_autocorrect();

        let dialog = editor.dialog.as_ref().expect("a dialog");
        assert!(dialog.ticked(SENTENCE_CASE));
        assert!(!dialog.ticked(CURLY_QUOTES), "the quotes are off and the box says on");
    }

    #[test]
    fn a_switch_turned_off_stops_the_correction() {
        let mut editor = editor();
        editor.open_autocorrect();
        tick(&mut editor, REPLACE_TEXT, false);
        accept(&mut editor);

        assert!(!editor.autocorrect.replace_text);
        assert_eq!(editor.autocorrect.on_word("say teh"), None);
    }

    #[test]
    fn a_replacement_added_is_on_the_list_and_is_made() {
        let mut editor = editor();
        editor.open_autocorrect();
        type_into(&mut editor, WHAT, "hte");
        type_into(&mut editor, WITH, "the");
        editor.autocorrect_dialog_button(ADD);
        accept(&mut editor);

        assert_eq!(editor.autocorrect.replacements.get("hte").map(String::as_str), Some("the"));
        assert_eq!(editor.autocorrect.on_word("hte").expect("a correction").putting, "the");
    }

    #[test]
    fn the_boxes_are_emptied_once_the_pair_is_on_the_list() {
        // Otherwise the next press of Add puts the same pair on again.
        let mut editor = editor();
        editor.open_autocorrect();
        type_into(&mut editor, WHAT, "hte");
        type_into(&mut editor, WITH, "the");
        editor.autocorrect_dialog_button(ADD);

        let dialog = editor.dialog.as_ref().expect("a dialog");
        assert_eq!(dialog.said(WHAT), "");
        assert_eq!(dialog.said(WITH), "");
    }

    #[test]
    fn a_replacement_deleted_is_off_the_list() {
        let mut editor = editor();
        editor.open_autocorrect();
        let first = editor.autocorrect.replacements.keys().next().expect("a pair").clone();
        editor.autocorrect_dialog_button(DELETE);
        accept(&mut editor);

        assert!(!editor.autocorrect.replacements.contains_key(&first), "{first} is still there");
    }

    #[test]
    fn cancelling_changes_nothing() {
        let mut editor = editor();
        let before = editor.autocorrect.clone();
        editor.open_autocorrect();
        tick(&mut editor, SENTENCE_CASE, false);
        editor.autocorrect_dialog_button(DELETE);
        editor.dialog = None;
        editor.asking = None;

        assert_eq!(editor.autocorrect, before);
    }

    #[test]
    fn an_exception_added_stops_the_correction() {
        let mut editor = editor();
        editor.open_autocorrect();
        editor.autocorrect_dialog_button(EXCEPTIONS);

        type_into(&mut editor, FIRST_WORD, "ib.");
        editor.exceptions_dialog_button(ADD);
        editor.close_exceptions(true);
        accept(&mut editor);

        assert!(editor.autocorrect.first_letter.contains("ib."));
        assert_eq!(editor.autocorrect.on_word("see ib. the"), None);
    }

    #[test]
    fn cancelling_the_exceptions_leaves_the_lists_alone() {
        let mut editor = editor();
        let before = editor.autocorrect.first_letter.clone();
        editor.open_autocorrect();
        editor.autocorrect_dialog_button(EXCEPTIONS);

        type_into(&mut editor, FIRST_WORD, "ib.");
        editor.exceptions_dialog_button(ADD);
        editor.close_exceptions(false);
        accept(&mut editor);

        assert_eq!(editor.autocorrect.first_letter, before);
    }

    #[test]
    fn a_symbol_arrives_in_the_box_that_says_what_to_put_in() {
        // Word's Symbol dialog hands a character over this way, and the button
        // that does it was waiting on there being an AutoCorrect list at all.
        let mut editor = editor();
        editor.open_autocorrect_with("\u{00A9}");
        let dialog = editor.dialog.as_ref().expect("a dialog");
        assert_eq!(dialog.said(WITH), "\u{00A9}");
    }

    #[test]
    fn the_exceptions_come_back_to_the_dialog_they_were_opened_from() {
        let mut editor = editor();
        editor.open_autocorrect();
        editor.autocorrect_dialog_button(EXCEPTIONS);
        assert_eq!(editor.asking, Some(Asking::Exceptions));

        editor.close_exceptions(true);
        assert_eq!(editor.asking, Some(Asking::AutoCorrect));
    }
}
