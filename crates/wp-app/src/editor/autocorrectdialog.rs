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
//! Word's Actions tab, for the same reason the switches it holds are: there
//! is nothing behind them here; and the Math AutoCorrect tab's Recognized
//! Functions, which the linear format here has no functions for. And on the
//! AutoFormat
//! tab, "Other paragraph styles" and "Plain text e-mail documents", which are
//! about kinds of document this program does not tell apart.

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

// The Math AutoCorrect tab: a switch, the list's switch, and a list like the
// first tab's.
const TAB_MATH: usize = 12;
const MATH_OUTSIDE: usize = 13;
const MATH_REPLACE: usize = 14;
const MATH_PAIR_ROW: usize = 15;
const MATH_WHAT: usize = 16;
const MATH_WITH: usize = 17;
const MATH_LIST: usize = 18;

/// Which tab is the Math AutoCorrect one, which Add and Delete ask.
const MATH_TAB: usize = 1;

// The AutoFormat As You Type tab.
const TAB_FORMAT: usize = 19;
const REPLACE_AS_YOU_TYPE: usize = 20;
const CURLY_QUOTES: usize = 21;
const ORDINALS: usize = 22;
const FRACTIONS: usize = 23;
const DASHES: usize = 24;
const BOLD_ITALIC: usize = 25;
const HYPERLINKS: usize = 26;
const APPLY_AS_YOU_TYPE: usize = 27;
const AUTOMATIC_LISTS: usize = 28;
const BORDER_LINES: usize = 29;
const HEADINGS: usize = 30;
const TABLES: usize = 31;

// The AutoFormat tab: the same rules, for a whole document at once.
const TAB_WHOLE: usize = 32;
const WHOLE_APPLY: usize = 33;
const WHOLE_HEADINGS: usize = 34;
const WHOLE_NUMBERED: usize = 35;
const WHOLE_BULLETED: usize = 36;
const WHOLE_REPLACE: usize = 37;
const WHOLE_QUOTES: usize = 38;
const WHOLE_ORDINALS: usize = 39;
const WHOLE_FRACTIONS: usize = 40;
const WHOLE_DASHES: usize = 41;
const WHOLE_BOLD_ITALIC: usize = 42;
const WHOLE_HYPERLINKS: usize = 43;
const WHOLE_PRESERVE: usize = 44;
const WHOLE_STYLES: usize = 45;

// The Exceptions dialog: two tabs, each a box and a list.
const TAB_FIRST_LETTER: usize = 0;
const FIRST_WORD: usize = 1;
const FIRST_LIST: usize = 2;
const FIRST_AUTO: usize = 3;
const TAB_INITIAL_CAPS: usize = 4;
const CAPS_WORD: usize = 5;
const CAPS_LIST: usize = 6;
const CAPS_AUTO: usize = 7;

/// Word's buttons past OK and Cancel.
pub(super) const ADD: &str = "Add";
pub(super) const DELETE: &str = "Delete";
pub(super) const EXCEPTIONS: &str = "Exceptions...";

/// What the Exceptions dialog found when it opened, so that its Cancel can
/// put it back.
#[derive(Clone, Debug)]
pub(crate) struct Stashed {
    pub first_letter: BTreeSet<String>,
    pub initial_caps: BTreeSet<String>,
    pub add_first: bool,
    pub add_caps: bool,
}

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
        self.autocorrect_dialog_at(chosen, 0)
    }

    /// The same, with `math_chosen` the row the Math AutoCorrect list is on.
    fn autocorrect_dialog_at(&self, chosen: usize, math_chosen: usize) -> Dialog {
        let rules = &self.editing_rules;
        let check = |label: &str, on: bool| Field::Check { label: label.to_owned(), on };
        let rows: Vec<(String, String)> =
            rules.replacements.iter().map(|(what, with)| (what.clone(), with.clone())).collect();
        let chosen = chosen.min(rows.len().saturating_sub(1));
        let math_rows: Vec<(String, String)> =
            rules.math.iter().map(|(what, with)| (what.clone(), with.clone())).collect();
        let math_chosen = math_chosen.min(math_rows.len().saturating_sub(1));

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
            // --- Math AutoCorrect -------------------------------------------
            Field::Tab("Math AutoCorrect".to_owned()),
            check("Use Math AutoCorrect rules outside of math regions", rules.math_outside),
            check("Replace text as you type", rules.math_replace),
            Field::Columns(2),
            Field::Text { label: "Replace".to_owned(), value: String::new() },
            Field::Text { label: "With".to_owned(), value: String::new() },
            Field::Pairs {
                label: "Replace".to_owned(),
                second: "With".to_owned(),
                rows: math_rows,
                current: math_chosen,
                scroll: math_chosen.saturating_sub(crate::chrome::dialog::PAIR_ROWS - 1),
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
            check("*Bold* and _italic_ with real formatting", rules.bold_italic),
            check("Internet and network paths with hyperlinks", rules.hyperlinks),
            Field::Group("Apply as you type".to_owned()),
            check("Automatic bulleted and numbered lists", rules.automatic_lists),
            check("Border lines", rules.border_lines),
            check("Built-in Heading styles", rules.headings),
            check("Tables", rules.tables),
            // --- AutoFormat -------------------------------------------------
            Field::Tab("AutoFormat".to_owned()),
            Field::Group("Apply".to_owned()),
            check("Built-in Heading styles", rules.reformat.headings),
            check("List styles", rules.reformat.numbered_lists),
            check("Automatic bulleted lists", rules.reformat.bulleted_lists),
            Field::Group("Replace".to_owned()),
            check(
                "\u{201C}Straight quotes\u{201D} with \u{201C}smart quotes\u{201D}",
                rules.reformat.curly_quotes,
            ),
            check("Ordinals (1st) with superscript", rules.reformat.ordinals),
            check("Fractions (1/2) with fraction character", rules.reformat.fractions),
            check("Hyphens (--) with dash (\u{2014})", rules.reformat.dashes),
            check("*Bold* and _italic_ with real formatting", rules.reformat.bold_italic),
            check("Internet and network paths with hyperlinks", rules.reformat.hyperlinks),
            Field::Group("Preserve".to_owned()),
            check("Styles", rules.reformat.keep_styles),
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
                (TAB_MATH, "a tab"),
                (MATH_OUTSIDE, "a tick box"),
                (MATH_REPLACE, "a tick box"),
                (MATH_PAIR_ROW, "a row"),
                (MATH_WHAT, "a box"),
                (MATH_WITH, "a box"),
                (MATH_LIST, "a list of pairs"),
                (TAB_FORMAT, "a tab"),
                (REPLACE_AS_YOU_TYPE, "a group"),
                (CURLY_QUOTES, "a tick box"),
                (ORDINALS, "a tick box"),
                (FRACTIONS, "a tick box"),
                (DASHES, "a tick box"),
                (BOLD_ITALIC, "a tick box"),
                (HYPERLINKS, "a tick box"),
                (APPLY_AS_YOU_TYPE, "a group"),
                (AUTOMATIC_LISTS, "a tick box"),
                (BORDER_LINES, "a tick box"),
                (HEADINGS, "a tick box"),
                (TABLES, "a tick box"),
                (TAB_WHOLE, "a tab"),
                (WHOLE_APPLY, "a group"),
                (WHOLE_HEADINGS, "a tick box"),
                (WHOLE_NUMBERED, "a tick box"),
                (WHOLE_BULLETED, "a tick box"),
                (WHOLE_REPLACE, "a group"),
                (WHOLE_QUOTES, "a tick box"),
                (WHOLE_ORDINALS, "a tick box"),
                (WHOLE_FRACTIONS, "a tick box"),
                (WHOLE_DASHES, "a tick box"),
                (WHOLE_BOLD_ITALIC, "a tick box"),
                (WHOLE_HYPERLINKS, "a tick box"),
                (WHOLE_PRESERVE, "a group"),
                (WHOLE_STYLES, "a tick box"),
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
        // And Add and Delete beside the Math AutoCorrect list as well, where
        // they work on that list instead.
        .button_on_tab(Answer::Named(ADD), MATH_TAB)
        .button_on_tab(Answer::Named(DELETE), MATH_TAB)
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
        rules.math_outside = dialog.ticked(MATH_OUTSIDE);
        rules.math_replace = dialog.ticked(MATH_REPLACE);
        rules.curly_quotes = dialog.ticked(CURLY_QUOTES);
        rules.ordinals = dialog.ticked(ORDINALS);
        rules.fractions = dialog.ticked(FRACTIONS);
        rules.dashes = dialog.ticked(DASHES);
        rules.automatic_lists = dialog.ticked(AUTOMATIC_LISTS);
        rules.bold_italic = dialog.ticked(BOLD_ITALIC);
        rules.hyperlinks = dialog.ticked(HYPERLINKS);
        rules.border_lines = dialog.ticked(BORDER_LINES);
        rules.headings = dialog.ticked(HEADINGS);
        rules.tables = dialog.ticked(TABLES);
        let whole = &mut rules.reformat;
        whole.headings = dialog.ticked(WHOLE_HEADINGS);
        whole.numbered_lists = dialog.ticked(WHOLE_NUMBERED);
        whole.bulleted_lists = dialog.ticked(WHOLE_BULLETED);
        whole.curly_quotes = dialog.ticked(WHOLE_QUOTES);
        whole.ordinals = dialog.ticked(WHOLE_ORDINALS);
        whole.fractions = dialog.ticked(WHOLE_FRACTIONS);
        whole.dashes = dialog.ticked(WHOLE_DASHES);
        whole.bold_italic = dialog.ticked(WHOLE_BOLD_ITALIC);
        whole.hyperlinks = dialog.ticked(WHOLE_HYPERLINKS);
        whole.keep_styles = dialog.ticked(WHOLE_STYLES);
    }

    /// The two tick boxes of the Exceptions dialog, onto the working copy.
    fn read_exceptions_dialog(&mut self, dialog: &Dialog) {
        self.editing_rules.add_first_letter_exceptions = dialog.ticked(FIRST_AUTO);
        self.editing_rules.add_initial_caps_exceptions = dialog.ticked(CAPS_AUTO);
    }

    /// Add, Delete and Exceptions: none of them answers the dialog.
    pub(super) fn autocorrect_dialog_button(&mut self, button: &str) -> Response {
        let Some(dialog) = self.dialog.clone() else { return Response::Ignored };
        self.read_autocorrect_dialog(&dialog);
        if dialog.showing_tab() == MATH_TAB {
            return self.math_list_button(button, &dialog);
        }

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
                self.exceptions_stash = Some(Stashed {
                    first_letter: self.editing_rules.first_letter.clone(),
                    initial_caps: self.editing_rules.initial_caps.clone(),
                    add_first: self.editing_rules.add_first_letter_exceptions,
                    add_caps: self.editing_rules.add_initial_caps_exceptions,
                });
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

    /// Add and Delete beside the Math AutoCorrect list.
    ///
    /// As beside the other list, except that what is typed keeps its case:
    /// `\Delta` and `\delta` are two letters.
    fn math_list_button(&mut self, button: &str, dialog: &Dialog) -> Response {
        let math = &mut self.editing_rules.math;
        let chosen = match button {
            ADD => {
                let what = dialog.said(MATH_WHAT).trim().to_owned();
                let with = dialog.said(MATH_WITH).trim().to_owned();
                if what.is_empty() || with.is_empty() {
                    dialog.chose_pair(MATH_LIST)
                } else {
                    math.insert(what.clone(), with);
                    math.keys().position(|key| *key == what).unwrap_or(0)
                }
            }
            DELETE => {
                if let Some((what, _)) = dialog.pair(MATH_LIST) {
                    let what = what.to_owned();
                    math.remove(&what);
                }
                dialog.chose_pair(MATH_LIST)
            }
            _ => return Response::Ignored,
        };
        let mut rebuilt = self.autocorrect_dialog_at(dialog.chose_pair(LIST), chosen);
        if button != ADD {
            rebuilt.carry_typing_from(dialog);
        }
        rebuilt.show_tab(MATH_TAB);
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
            Field::Check {
                label: "Automatically add words to list".to_owned(),
                on: rules.add_first_letter_exceptions,
            },
            Field::Tab("INitial CAps".to_owned()),
            Field::Text { label: "Don't correct".to_owned(), value: String::new() },
            list(&rules.initial_caps, caps),
            Field::Check {
                label: "Automatically add words to list".to_owned(),
                on: rules.add_initial_caps_exceptions,
            },
        ];

        crate::chrome::dialog::check_rows(
            "AutoCorrect Exceptions",
            &fields,
            &[
                (TAB_FIRST_LETTER, "a tab"),
                (FIRST_WORD, "a box"),
                (FIRST_LIST, "a list of pairs"),
                (FIRST_AUTO, "a tick box"),
                (TAB_INITIAL_CAPS, "a tab"),
                (CAPS_WORD, "a box"),
                (CAPS_LIST, "a list of pairs"),
                (CAPS_AUTO, "a tick box"),
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
        self.read_exceptions_dialog(&dialog);
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
        if keeping {
            if let Some(dialog) = self.dialog.clone() {
                self.read_exceptions_dialog(&dialog);
            }
        }
        if let Some(stashed) = self.exceptions_stash.take() {
            if !keeping {
                self.editing_rules.first_letter = stashed.first_letter;
                self.editing_rules.initial_caps = stashed.initial_caps;
                self.editing_rules.add_first_letter_exceptions = stashed.add_first;
                self.editing_rules.add_initial_caps_exceptions = stashed.add_caps;
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
    fn the_math_tab_adds_and_deletes_on_its_own_list() {
        let mut editor = editor();
        editor.open_autocorrect();
        editor.dialog.as_mut().unwrap().show_tab(MATH_TAB);
        type_into(&mut editor, MATH_WHAT, "\\Ohm");
        type_into(&mut editor, MATH_WITH, "\u{2126}");
        editor.autocorrect_dialog_button(ADD);
        assert_eq!(editor.dialog.as_ref().unwrap().showing_tab(), MATH_TAB, "the tab moved");

        // Deleting the row that is chosen, which is the one just added.
        let row = editor.dialog.as_ref().unwrap().pair(MATH_LIST).map(|(what, _)| what.to_owned());
        assert_eq!(row.as_deref(), Some("\\Ohm"), "the new row is not the chosen one");
        if let Some(Field::Pairs { rows, current, .. }) =
            editor.dialog.as_mut().unwrap().fields.get_mut(MATH_LIST)
        {
            *current = rows.iter().position(|(what, _)| what == "\\alpha").expect("alpha");
        }
        editor.autocorrect_dialog_button(DELETE);
        accept(&mut editor);

        // Kept in its own case, and on its own list, not the other one.
        assert_eq!(editor.autocorrect.math.get("\\Ohm").map(String::as_str), Some("\u{2126}"));
        assert!(!editor.autocorrect.replacements.contains_key("\\ohm"));
        assert!(!editor.autocorrect.math.contains_key("\\alpha"));
        assert!(editor.autocorrect.replacements.contains_key("teh"), "the other list was touched");
    }

    #[test]
    fn the_math_switches_are_the_tab_boxes() {
        let mut editor = editor();
        editor.open_autocorrect();
        assert!(!editor.dialog.as_ref().unwrap().ticked(MATH_OUTSIDE), "Word ships it off");
        assert!(editor.dialog.as_ref().unwrap().ticked(MATH_REPLACE));
        tick(&mut editor, MATH_OUTSIDE, true);
        tick(&mut editor, MATH_REPLACE, false);
        accept(&mut editor);
        assert!(editor.autocorrect.math_outside);
        assert!(!editor.autocorrect.math_replace);
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
