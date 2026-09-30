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

use crate::messages::t;

use crate::chrome::dialog::{Answer, Button, Dialog, Field};
use crate::chrome::theme::{Mode, Theme};
use crate::measure::Unit;

use super::dialogs::Asking;
use super::ribbondialog;
use super::Editor;

// General.
const TAB_GENERAL: usize = 0;
const APPEARANCE: usize = 1;
const DARK: usize = 2;
const ROW_START: usize = 3;
const ZOOM: usize = 4;
const UNIT: usize = 5;
const FILE_TYPES: usize = 6;
const FILE_TYPES_SAID: usize = 7;

// Display.
const TAB_DISPLAY: usize = 8;
const ALWAYS_SHOW: usize = 9;
const MARKS: usize = 10;
const GRIDLINES: usize = 11;
const PAGE_DISPLAY: usize = 12;
const RULERS: usize = 13;
const NAVIGATION: usize = 14;
const WHITE_SPACE: usize = 15;

// Language.
const TAB_LANGUAGE: usize = 16;
const DISPLAY_LANGUAGE: usize = 17;
const LANGUAGE_CHOICE: usize = 18;
const LANGUAGE_SAID: usize = 19;

// Save.
const TAB_SAVE: usize = 20;
const SAVING: usize = 21;
const AUTOSAVE: usize = 22;
const AUTOSAVE_MINUTES: usize = 23;
const KEEP_AUTOSAVED: usize = 24;
const RECOVERY_FOLDER: usize = 25;

// Proofing.
const TAB_PROOFING: usize = 26;
const AUTOCORRECT: usize = 27;
const AUTOCORRECT_SAID: usize = 28;
const CORRECTING: usize = 29;
const PROOFING: usize = 30;
const HIDE_SPELLING: usize = 31;

// Advanced.
const TAB_ADVANCED: usize = 32;
const ADVANCED_GENERAL: usize = 33;
const CONFIRM_CONVERSION: usize = 34;

// Trust Centre. Word gives it a dialog of its own behind a button; here it
// is a page of this one, because the two are the same question — what this
// program is allowed to do without asking — and a dialog that opens a dialog
// to answer it would be one window too many.
// It is the last page, after the two pages of lists, so its numbers follow
// theirs: see [`ribbondialog::FIRST`] for where those begin.
const TAB_TRUST: usize = ribbondialog::FIRST + 8;
const MACRO_SETTINGS: usize = TAB_TRUST + 1;
const MACRO_TRUST: usize = TAB_TRUST + 2;
const TRUSTED_LOCATIONS: usize = TAB_TRUST + 3;
pub(super) const TRUSTED_PLACES: usize = TAB_TRUST + 4;
const TRUSTED_PUBLISHERS_GROUP: usize = TAB_TRUST + 5;
pub(super) const TRUSTED_PUBLISHERS: usize = TAB_TRUST + 6;

/// Which tab of the dialog Proofing is, so its button is drawn on that one.
const TAB_PROOFING_PAGE: usize = 4;

/// And which the Trust Centre is, for the buttons that belong to it:
/// after General, Display, Language, Save, Proofing, Advanced, and the two
/// pages of lists.
pub(super) const TAB_TRUST_PAGE: usize = 8;

/// The button that trusts the folder the document is in.
pub(super) const TRUST_FOLDER: &str = "Trust This Folder";

/// And the one that forgets a folder that was trusted.
pub(super) const FORGET_PLACE: &str = "Remove Location";

/// The button that trusts whoever signed the open document, by the
/// certificate their signature carries.
pub(super) const TRUST_PUBLISHER: &str = "Trust This Publisher";

/// And the one that forgets a publisher that was trusted.
pub(super) const FORGET_PUBLISHER: &str = "Remove Publisher";
/// And which General is, for the button that registers the file types.
const TAB_GENERAL_PAGE: usize = 0;

/// Word's button for the dialog behind this one.
pub(super) const AUTOCORRECT_OPTIONS: &str = "AutoCorrect Options...";

/// The button that tells the desktop this program opens Word documents.
/// Word's own wording for it.
pub(super) const MAKE_DEFAULT: &str = "Make Default";

/// Which language in the list is the one being read now.
fn language_index() -> usize {
    let language = crate::messages::language();
    crate::messages::languages().iter().position(|(code, _)| *code == language).unwrap_or(0)
}

impl Editor {
    /// Opens Word's Options.
    pub(super) fn open_options(&mut self) -> Response {
        // A working copy of what a person may change about the ribbon, which
        // nothing but OK puts back. See [`super::ribbondialog`].
        self.editing_chrome = self.ribbon.custom.clone();
        let dialog = self.options_dialog();
        self.ask(Asking::Options, dialog)
    }

    /// What the General page says about which program opens documents.
    ///
    /// Three answers, and each is true of some machine: it already does, it
    /// does not, or this build has no desktop to ask.
    pub(super) fn default_program_line(&self) -> String {
        let kinds = super::files::DESKTOP_KINDS;
        let Some(word) = kinds.iter().find(|kind| kind.extension == ".docx") else {
            return t("This program can open Word documents.").to_owned();
        };
        if wp_shell::files::opens(word) {
            t("Word documents open in this program.").to_owned()
        } else if wp_shell::files::defaults_are_chosen_by_hand() {
            t("Word documents do not open in this program. Make Default registers it and opens the system's own page for choosing.")
                .to_owned()
        } else {
            t("Word documents do not open in this program.").to_owned()
        }
    }

    /// Tells the desktop this program opens Word documents.
    ///
    /// On a desktop where a program may say it is the one to open a kind,
    /// that is the end of it. On Windows the kinds are registered — which is
    /// what puts the program in Open With and in Default Apps — and then the
    /// page where the person chooses is opened, because the choice is kept
    /// where a program cannot write it and pretending otherwise would be a
    /// button that lies.
    pub(super) fn make_default_program(&mut self) -> Response {
        let kinds = super::files::DESKTOP_KINDS;
        let registered = wp_shell::files::associate(kinds, "Word Processor");
        let word = kinds.iter().find(|kind| kind.extension == ".docx");
        let now_opens = word.is_some_and(wp_shell::files::opens);
        self.status = if !registered {
            t("The file types could not be registered").to_owned()
        } else if now_opens {
            t("Word documents now open in this program").to_owned()
        } else if wp_shell::files::defaults_are_chosen_by_hand() {
            wp_shell::files::choose_defaults();
            t("Registered. Choose this program in the page that opened").to_owned()
        } else {
            t("The file types were registered").to_owned()
        };
        // The dialog is standing, and the line it shows is now out of date.
        let line = self.default_program_line();
        if let Some(dialog) = &mut self.dialog {
            dialog.set_said(FILE_TYPES_SAID, &line);
        }
        self.needs_redraw = true;
        Response::Redraw
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
            // Which program the desktop opens a document with. Word says the
            // same thing in the same place, and for the same reason: a person
            // who wants their documents to open here has nowhere else to say
            // so.
            Field::Group("File types".to_owned()),
            Field::Said { label: self.default_program_line(), value: String::new() },
            // --- Display ---------------------------------------------------
            Field::Tab("Display".to_owned()),
            Field::Group("Always show these on screen".to_owned()),
            check("Formatting marks", self.show_marks),
            check("Gridlines", self.show_gridlines),
            Field::Group("Page display".to_owned()),
            check("Rulers", self.rulers_setting()),
            check("Navigation pane", self.show_navigation),
            check("White space between pages", !self.joined_pages),
            // --- Language --------------------------------------------------
            // Word's own page for this, and Word's own wording. What it
            // offers is English, whatever came with the program, and
            // whatever the person has put in the folder named below.
            Field::Tab("Language".to_owned()),
            Field::Group("Office display language".to_owned()),
            Field::Choice {
                label: "Interface language".to_owned(),
                items: crate::messages::languages().into_iter().map(|(_, name)| name).collect(),
                current: language_index(),
            },
            Field::Said {
                label: "Catalogues can be added in".to_owned(),
                value: crate::messages::folder()
                    .map(|folder| folder.display().to_string())
                    .unwrap_or_else(|| "nowhere this program can read".to_owned()),
            },
            // --- Save ------------------------------------------------------
            Field::Tab("Save".to_owned()),
            Field::Group("Save documents".to_owned()),
            check("Save AutoRecover information", self.autosave),
            Field::Number {
                label: "every".to_owned(),
                value: self.autosave_minutes.to_string(),
                unit: "minutes",
            },
            check(
                "Keep the last AutoRecovered version if I close without saving",
                self.keep_autosaved,
            ),
            Field::Said {
                label: "AutoRecover file location".to_owned(),
                value: super::autorecover::folder()
                    .map(|folder| folder.display().to_string())
                    .unwrap_or_else(|| "nowhere this program can write".to_owned()),
            },
            // --- Proofing --------------------------------------------------
            Field::Tab("Proofing".to_owned()),
            Field::Group("AutoCorrect options".to_owned()),
            Field::Said {
                label: "Change how the text is corrected as you type".to_owned(),
                value: String::new(),
            },
            Field::Group("When correcting spelling".to_owned()),
            check("Mark spelling mistakes as you type", self.show_proofing),
            // Word's wording, and Word's place for it: a setting of the document
            // rather than of the program, kept in the file and honoured by
            // whoever opens it next.
            check(
                "Hide spelling errors in this document only",
                self.document.setting_is_on("hideSpellingErrors"),
            ),
            // --- Advanced --------------------------------------------------
            Field::Tab("Advanced".to_owned()),
            Field::Group("General".to_owned()),
            // Word's wording: every file not a Word document is asked about
            // before it is converted, even when its kind is plain.
            check("Confirm file format conversion on open", self.confirm_conversion),
        ];
        // --- Quick Access Toolbar, and Customize Ribbon --------------------
        // Built elsewhere because they are two pages of lists rather than a
        // column of switches. See [`super::ribbondialog`].
        let mut fields = fields;
        fields.extend(self.customise_fields());
        // --- Trust Centre ---------------------------------------------------
        // Last, as it is last in Word's own list of pages.
        fields.extend([
            Field::Tab("Trust Center".to_owned()),
            Field::Group("Macro settings".to_owned()),
            Field::Choice {
                label: "What a document's own macros may do".to_owned(),
                items: super::trust::Trusting::ALL
                    .iter()
                    .map(|one| crate::messages::t(one.label()).to_owned())
                    .collect(),
                current: super::trust::Trusting::ALL
                    .iter()
                    .position(|one| *one == self.trusting())
                    .unwrap_or(1),
            },
            Field::Group("Trusted locations".to_owned()),
            Field::Pairs {
                label: "Folder".to_owned(),
                second: "What is trusted".to_owned(),
                rows: self
                    .settings
                    .trusted_places
                    .iter()
                    .map(|place| {
                        (
                            place.clone(),
                            crate::messages::t("Macros run without being asked about").to_owned(),
                        )
                    })
                    .collect(),
                current: 0,
                scroll: 0,
            },
            // Word's Trusted Publishers page, in its own columns: who the
            // certificate is about, and who issued it until when. Shown and
            // never matched by — what is trusted is the certificate.
            Field::Group("Trusted publishers".to_owned()),
            Field::Pairs {
                label: "Issued To".to_owned(),
                second: "Issued By".to_owned(),
                rows: self
                    .settings
                    .trusted_publishers
                    .iter()
                    .map(|publisher| (publisher.subject.clone(), publisher.said()))
                    .collect(),
                current: 0,
                scroll: 0,
            },
        ]);

        let mut kinds = vec![
            (TAB_GENERAL, "a tab"),
            (APPEARANCE, "a group"),
            (DARK, "a tick box"),
            (ROW_START, "a row"),
            (ZOOM, "a number"),
            (UNIT, "a list"),
            (FILE_TYPES, "a group"),
            (FILE_TYPES_SAID, "a line"),
            (TAB_DISPLAY, "a tab"),
            (ALWAYS_SHOW, "a group"),
            (MARKS, "a tick box"),
            (GRIDLINES, "a tick box"),
            (PAGE_DISPLAY, "a group"),
            (RULERS, "a tick box"),
            (NAVIGATION, "a tick box"),
            (WHITE_SPACE, "a tick box"),
            (TAB_LANGUAGE, "a tab"),
            (DISPLAY_LANGUAGE, "a group"),
            (LANGUAGE_CHOICE, "a list"),
            (LANGUAGE_SAID, "a line"),
            (TAB_SAVE, "a tab"),
            (SAVING, "a group"),
            (AUTOSAVE, "a tick box"),
            (AUTOSAVE_MINUTES, "a number"),
            (KEEP_AUTOSAVED, "a tick box"),
            (RECOVERY_FOLDER, "a line"),
            (TAB_PROOFING, "a tab"),
            (AUTOCORRECT, "a group"),
            (AUTOCORRECT_SAID, "a line"),
            (CORRECTING, "a group"),
            (PROOFING, "a tick box"),
            (HIDE_SPELLING, "a tick box"),
            (TAB_ADVANCED, "a tab"),
            (ADVANCED_GENERAL, "a group"),
            (CONFIRM_CONVERSION, "a tick box"),
        ];
        kinds.extend(Self::customise_kinds());
        kinds.extend([
            (TAB_TRUST, "a tab"),
            (MACRO_SETTINGS, "a group"),
            (MACRO_TRUST, "a list"),
            (TRUSTED_LOCATIONS, "a group"),
            (TRUSTED_PLACES, "a list of pairs"),
            (TRUSTED_PUBLISHERS_GROUP, "a group"),
            (TRUSTED_PUBLISHERS, "a list of pairs"),
        ]);
        crate::chrome::dialog::check_rows("Options", &fields, &kinds);

        let named = |label: &'static str| Button {
            label: label.to_owned(),
            answer: Answer::Named(label),
            default: false,
        };
        let mut dialog = Dialog::with_buttons(
            "Options",
            fields,
            vec![
                Button { label: "OK".to_owned(), answer: Answer::Accept, default: true },
                named(AUTOCORRECT_OPTIONS),
                named(MAKE_DEFAULT),
                named(TRUST_FOLDER),
                named(FORGET_PLACE),
                named(TRUST_PUBLISHER),
                named(FORGET_PUBLISHER),
                named(ribbondialog::ADD),
                named(ribbondialog::REMOVE),
                named(ribbondialog::MOVE_UP),
                named(ribbondialog::MOVE_DOWN),
                named(ribbondialog::RESET),
                Button { label: "Cancel".to_owned(), answer: Answer::Cancel, default: false },
            ],
        )
        // Word's dialog is this wide because it holds two lists side by side,
        // and a dialog that changed size when a tab was pressed would jump
        // about under the pointer. The Trust Center's row of buttons is wider
        // than this in English and wider still in German; the dialog makes
        // itself as wide as that row, on every tab alike.
        .wide(760.0)
        .button_on_tab(Answer::Named(AUTOCORRECT_OPTIONS), TAB_PROOFING_PAGE)
        .button_on_tab(Answer::Named(MAKE_DEFAULT), TAB_GENERAL_PAGE)
        .button_on_tab(Answer::Named(TRUST_FOLDER), TAB_TRUST_PAGE)
        .button_on_tab(Answer::Named(FORGET_PLACE), TAB_TRUST_PAGE)
        .button_on_tab(Answer::Named(TRUST_PUBLISHER), TAB_TRUST_PAGE)
        .button_on_tab(Answer::Named(FORGET_PUBLISHER), TAB_TRUST_PAGE);
        for label in [
            ribbondialog::ADD,
            ribbondialog::REMOVE,
            ribbondialog::MOVE_UP,
            ribbondialog::MOVE_DOWN,
            ribbondialog::RESET,
        ] {
            dialog = dialog
                .button_on_tab(Answer::Named(label), ribbondialog::QUICK_PAGE)
                .button_on_tab(Answer::Named(label), ribbondialog::RIBBON_PAGE);
        }
        dialog
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
        self.set_rulers_setting(dialog.ticked(RULERS));
        self.show_navigation = dialog.ticked(NAVIGATION);
        // Word's wording is the white space; the editor keeps whether the
        // pages are joined, which is the other way round.
        self.joined_pages = !dialog.ticked(WHITE_SPACE);
        // The language before anything else is read out of the dialog: the
        // dialog is about to be drawn again, and in the new language.
        let languages = crate::messages::languages();
        let chosen = dialog.chose(LANGUAGE_CHOICE);
        if let Some((code, _)) = languages.get(chosen) {
            crate::messages::set_language(code);
            self.settings.language = Some(code.clone());
        }
        self.autosave = dialog.ticked(AUTOSAVE);
        if let Ok(minutes) = dialog.said(AUTOSAVE_MINUTES).trim().parse::<u32>() {
            self.autosave_minutes = minutes.clamp(1, 120);
        }
        self.keep_autosaved = dialog.ticked(KEEP_AUTOSAVED);
        self.confirm_conversion = dialog.ticked(CONFIRM_CONVERSION);
        // What the Trust Centre was told, which is about the person and not
        // about the document: it follows them to the next one.
        if let Some(chosen) = super::trust::Trusting::ALL.get(dialog.chose(MACRO_TRUST)) {
            self.settings.macro_trust = Some(chosen.name().to_owned());
        }
        self.show_proofing = dialog.ticked(PROOFING);
        self.document.set_setting_flag("hideSpellingErrors", dialog.ticked(HIDE_SPELLING));

        // The ribbon and the toolbar: the ticks are read out of the tree here,
        // and everything else was already put on the working copy by the
        // buttons that did it.
        self.read_customise_dialog(dialog);
        self.ribbon.custom = self.editing_chrome.clone();
        self.settings.chrome = self.editing_chrome.clone();

        // Remembered as well as done: these follow the person from one document
        // to the next, which is the whole difference between a setting and a
        // command.
        self.settings.autosave = Some(self.autosave);
        self.settings.autosave_minutes = Some(self.autosave_minutes);
        self.settings.keep_autosaved = Some(self.keep_autosaved);
        self.settings.confirm_conversion = Some(self.confirm_conversion);
        self.settings.dark = Some(dark);
        self.settings.rulers = Some(self.rulers_setting());
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
