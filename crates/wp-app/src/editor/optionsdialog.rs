//! Word's Options: what the program does, rather than what the document says.
//!
//! # Only what is honoured
//!
//! Word's Options has ten categories and several hundred settings, most of them
//! about things this program does not have. A dialog that offered them all
//! would be a dialog where most of the switches do nothing, which is worse than
//! a short one — a switch that does nothing is a lie told once per person who
//! tries it.
//!
//! So every setting here is one the program obeys, and the categories are
//! Word's own for the ones that are: General, Display, Proofing, Advanced. What
//! is missing is named in the roadmap rather than drawn as a dead switch.
//!
//! # Where they are kept
//!
//! In the settings file beside the program, not in the document: they are about
//! this person's copy of the program and follow them from one document to the
//! next. See [`crate::settings`].

use wp_shell::Response;

use crate::chrome::dialog::{Answer, Button, Dialog, Field};
use crate::chrome::theme::{Mode, Theme};
use crate::measure::Unit;

use super::dialogs::Asking;
use super::Editor;

// General.
const TAB_GENERAL: usize = 0;
const APPEARANCE: usize = 1;
const DARK: usize = 2;
const ROW_START: usize = 3;
const ZOOM: usize = 4;
const UNIT: usize = 5;

// Display.
const TAB_DISPLAY: usize = 6;
const ALWAYS_SHOW: usize = 7;
const MARKS: usize = 8;
const GRIDLINES: usize = 9;
const PAGE_DISPLAY: usize = 10;
const RULERS: usize = 11;
const NAVIGATION: usize = 12;
const WHITE_SPACE: usize = 13;

// Proofing.
const TAB_PROOFING: usize = 14;
const AUTOCORRECT: usize = 15;
const AUTOCORRECT_SAID: usize = 16;
const CORRECTING: usize = 17;
const PROOFING: usize = 18;

/// Word's button for the dialog behind this one.
pub(super) const AUTOCORRECT_OPTIONS: &str = "AutoCorrect Options...";

impl Editor {
    /// Opens Word's Options.
    pub(super) fn open_options(&mut self) -> Response {
        let dialog = self.options_dialog();
        self.ask(Asking::Options, dialog)
    }

    /// The dialog itself, filled in from how the window is now.
    pub(super) fn options_dialog(&self) -> Dialog {
        let check = |label: &str, on: bool| Field::Check { label: label.to_owned(), on };

        let fields = vec![
            // --- General ---------------------------------------------------
            Field::Tab("General".to_owned()),
            Field::Group("Appearance".to_owned()),
            check("Use a dark window", self.theme.mode == Mode::Dark),
            Field::Columns(2),
            Field::Number {
                label: "Open documents at".to_owned(),
                value: format!("{:.0}", self.zoom),
                unit: "%",
            },
            Field::Choice {
                label: "Show measurements in".to_owned(),
                items: Unit::ALL.iter().map(|unit| unit.label().to_owned()).collect(),
                current: Unit::ALL.iter().position(|unit| *unit == self.unit).unwrap_or(0),
            },
            // --- Display ---------------------------------------------------
            Field::Tab("Display".to_owned()),
            Field::Group("Always show these on screen".to_owned()),
            check("Formatting marks", self.show_marks),
            check("Gridlines", self.show_gridlines),
            Field::Group("Page display".to_owned()),
            check("Rulers", self.show_rulers),
            check("Navigation pane", self.show_navigation),
            check("White space between pages", !self.joined_pages),
            // --- Proofing --------------------------------------------------
            Field::Tab("Proofing".to_owned()),
            Field::Group("AutoCorrect options".to_owned()),
            Field::Said {
                label: "Change how the text is corrected as you type".to_owned(),
                value: String::new(),
            },
            Field::Group("When correcting spelling".to_owned()),
            check("Mark spelling mistakes as you type", self.show_proofing),
        ];

        crate::chrome::dialog::check_rows(
            "Options",
            &fields,
            &[
                (TAB_GENERAL, "a tab"),
                (APPEARANCE, "a group"),
                (DARK, "a tick box"),
                (ROW_START, "a row"),
                (ZOOM, "a number"),
                (UNIT, "a list"),
                (TAB_DISPLAY, "a tab"),
                (ALWAYS_SHOW, "a group"),
                (MARKS, "a tick box"),
                (GRIDLINES, "a tick box"),
                (PAGE_DISPLAY, "a group"),
                (RULERS, "a tick box"),
                (NAVIGATION, "a tick box"),
                (WHITE_SPACE, "a tick box"),
                (TAB_PROOFING, "a tab"),
                (AUTOCORRECT, "a group"),
                (AUTOCORRECT_SAID, "a line"),
                (CORRECTING, "a group"),
                (PROOFING, "a tick box"),
            ],
        );

        Dialog::with_buttons(
            "Options",
            fields,
            vec![
                Button { label: "OK".to_owned(), answer: Answer::Accept, default: true },
                Button {
                    label: AUTOCORRECT_OPTIONS.to_owned(),
                    answer: Answer::Named(AUTOCORRECT_OPTIONS),
                    default: false,
                },
                Button { label: "Cancel".to_owned(), answer: Answer::Cancel, default: false },
            ],
        )
        .wide(560.0)
    }

    /// Takes what the dialog says and does it, then writes it down.
    pub(super) fn apply_options(&mut self, dialog: &Dialog) -> Response {
        let dark = dialog.ticked(DARK);
        self.theme = Theme::of(if dark { Mode::Dark } else { Mode::Light });

        if let Ok(zoom) = dialog.said(ZOOM).trim().parse::<f32>() {
            self.zoom =
                zoom.clamp(crate::chrome::status::MIN_ZOOM, crate::chrome::status::MAX_ZOOM);
        }
        self.unit = Unit::ALL.get(dialog.chose(UNIT)).copied().unwrap_or_default();

        self.show_marks = dialog.ticked(MARKS);
        self.show_gridlines = dialog.ticked(GRIDLINES);
        self.show_rulers = dialog.ticked(RULERS);
        self.show_navigation = dialog.ticked(NAVIGATION);
        // Word's wording is the white space; the editor keeps whether the
        // pages are joined, which is the other way round.
        self.joined_pages = !dialog.ticked(WHITE_SPACE);
        self.show_proofing = dialog.ticked(PROOFING);

        // Remembered as well as done: these follow the person from one document
        // to the next, which is the whole difference between a setting and a
        // command.
        self.settings.dark = Some(dark);
        self.settings.rulers = Some(self.show_rulers);
        self.settings.navigation = Some(self.show_navigation);
        self.settings.zoom = Some(self.zoom);
        self.settings.marks = Some(self.show_marks);
        self.settings.gridlines = Some(self.show_gridlines);
        self.settings.white_space = Some(!self.joined_pages);
        self.settings.proofing = Some(self.show_proofing);
        self.settings.unit = Some(self.unit.to_name().to_owned());
        self.settings.save();

        self.relayout();
        self.needs_redraw = true;
        Response::Redraw
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chrome::dialog::Field;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::Document;
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event, Key, Modifiers};

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

    /// Answers the dialog without letting it write to the settings file: a
    /// test must not leave anything behind on the machine it ran on.
    fn accept(editor: &mut Editor) {
        let dialog = editor.dialog.take().expect("a dialog");
        editor.asking = None;
        editor.apply_options(&dialog);
    }

    #[test]
    fn the_dialog_opens_showing_how_the_window_is() {
        let mut editor = editor();
        editor.show_marks = true;
        editor.show_rulers = false;
        editor.open_options();

        let dialog = editor.dialog.as_ref().expect("a dialog");
        assert!(dialog.ticked(MARKS), "the marks are showing and the box says not");
        assert!(!dialog.ticked(RULERS), "the rulers are off and the box says on");
    }

    #[test]
    fn what_is_ticked_reaches_the_window() {
        let mut editor = editor();
        editor.open_options();
        tick(&mut editor, MARKS, true);
        tick(&mut editor, GRIDLINES, true);
        tick(&mut editor, NAVIGATION, false);
        accept(&mut editor);

        assert!(editor.show_marks);
        assert!(editor.show_gridlines);
        assert!(!editor.show_navigation);
    }

    #[test]
    fn the_white_space_box_is_the_other_way_round_from_what_is_kept() {
        // Word says "white space between pages"; the editor keeps whether the
        // pages are joined. Getting this backwards would hide the white space
        // whenever somebody asked for it.
        let mut editor = editor();
        editor.open_options();
        tick(&mut editor, WHITE_SPACE, false);
        accept(&mut editor);
        assert!(editor.joined_pages, "the pages were not joined");

        editor.open_options();
        tick(&mut editor, WHITE_SPACE, true);
        accept(&mut editor);
        assert!(!editor.joined_pages, "the white space did not come back");
    }

    #[test]
    fn the_unit_reaches_every_box_in_the_program() {
        // The point of the setting: a person who asked for centimetres expects
        // the margins to be in centimetres too.
        let mut editor = editor();
        editor.open_options();
        if let Some(dialog) = &mut editor.dialog {
            if let Some(Field::Choice { current, .. }) = dialog.fields.get_mut(UNIT) {
                *current = 1;
            }
        }
        accept(&mut editor);
        assert_eq!(editor.unit, Unit::Centimetres);

        editor.open_page_setup();
        let dialog = editor.dialog.as_ref().expect("a dialog");
        // An inch is 2.54 centimetres, and the box has to say so.
        assert_eq!(dialog.said(super::super::dialogs::MARGIN_TOP), "2.54");
    }

    #[test]
    fn the_theme_follows_the_tick_box() {
        let mut editor = editor();
        editor.open_options();
        tick(&mut editor, DARK, true);
        accept(&mut editor);
        assert_eq!(editor.theme.mode, Mode::Dark);
    }

    #[test]
    fn cancelling_changes_nothing() {
        let mut editor = editor();
        let before = editor.show_marks;
        editor.open_options();
        tick(&mut editor, MARKS, !before);
        editor.handle(Event::KeyDown { key: Key::Escape, modifiers: Modifiers::default() });

        assert!(!editor.in_dialog());
        assert_eq!(editor.show_marks, before);
    }
}
