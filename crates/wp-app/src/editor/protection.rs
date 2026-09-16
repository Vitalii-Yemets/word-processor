//! Who may change the document, and the password that says so.
//!
//! # What the four restrictions mean here
//!
//! A document may say that a reader is allowed to do nothing, to leave
//! comments, to edit as long as every change is recorded, or to fill in the
//! fields of a form and nothing else. Each of those has to be true of the
//! program in front of the person, or the document is saying something the
//! program is not doing:
//!
//! * **No changes** — nothing that would change the text runs, and the ribbon
//!   button that would have done it says why.
//! * **Comments** — the same, except that a comment may be made and taken
//!   away again, which is the whole point of the restriction.
//! * **Tracked changes** — everything may be done, and the recording of
//!   changes is switched on and cannot be switched off. See
//!   [`wp_docx::revisions`], which is where that is refused.
//! * **Filling in forms** — typing is allowed inside the answer of a form
//!   field and nowhere else. See [`wp_docx::forms`].
//!
//! # Limiting the formatting
//!
//! The other half of Word's dialog, and the one that has nothing to do with
//! who may type: a document may say that only some of its styles may be
//! applied and that no formatting may be written directly on to the text at
//! all. See [`wp_docx::locking`] for what is written down; what is here is the
//! list of tick boxes and the refusing of every command that would write
//! formatting while the restriction stands.
//!
//! The two halves are independent, as they are in Word. A document may limit
//! its formatting and let anybody type, or restrict the typing and leave the
//! formatting alone, or both.
//!
//! # The password
//!
//! A restriction may have a password behind it, and then lifting it means
//! typing that password. The arithmetic is [`wp_docx::protection`]; what is
//! here is the asking. Two things are worth saying plainly, and the dialog
//! says both: the salt comes from the machine and not from this program, and
//! the document is not encrypted — the password stops the restriction being
//! lifted, not the file being read.

use wp_docx::forms::FormKind;
use wp_docx::protection::{EditMode, Protection};
use wp_shell::Response;

use crate::chrome::dialog::{Dialog, Field, TreeRow};
use crate::chrome::Command;

use super::dialogs::Asking;
use super::Editor;

/// Where each answer sits in the Restrict Editing dialog.
///
/// Named rather than counted twice, for the reason every other dialog in this
/// program names its rows: the headings take places in the list too.
const LIMIT: usize = 1;
const STYLES: usize = 2;
const THEME: usize = 3;
const RESTRICT: usize = 5;
const MODE: usize = 6;
const PASSWORD: usize = 10;
const AGAIN: usize = 11;

/// And the one box of the dialog that asks for it back.
const ANSWER: usize = 1;

/// How many bytes of salt, asked of the format rather than decided here.
const SALT: usize = wp_docx::protection::SALT_BYTES;

impl Editor {
    /// Word's Restrict Editing: the restriction, or the way out of one.
    pub(super) fn open_protection(&mut self) -> Response {
        match self.document.protection_rules() {
            None => {
                let dialog = self.protect_dialog();
                self.ask(Asking::Protect, dialog)
            }
            // Nothing to ask. Word's Stop Protection button lifts a
            // restriction with no password the moment it is pressed.
            Some(rules) if rules.password.is_none() => self.stop_protecting(),
            Some(rules) => {
                let dialog = Self::unprotect_dialog(&rules);
                self.ask(Asking::Unprotect, dialog)
            }
        }
    }

    /// Every style the restriction can allow or forbid, as identifier and
    /// name.
    ///
    /// The document's own, in the order it defines them. The hundreds a
    /// `styles.xml` merely mentions are not listed — a list of three hundred
    /// styles nobody has used is not a list anybody can read — and they are
    /// locked together with everything unticked here. See
    /// [`wp_docx::locking`].
    fn lockable_styles(&self) -> Vec<(String, String)> {
        self.document
            .styles()
            .all()
            .iter()
            .map(|style| {
                let name = style.name.clone().unwrap_or_else(|| style.id.clone());
                (style.id.clone(), super::insert::title_case(&name))
            })
            .collect()
    }

    /// The dialog that puts a restriction on.
    fn protect_dialog(&self) -> Dialog {
        let modes = EditMode::ALL.iter().map(|mode| mode.label().to_owned()).collect();
        let rows = self
            .lockable_styles()
            .into_iter()
            .map(|(id, name)| TreeRow::ticked(0, &name, !self.document.style_is_locked(&id)))
            .collect();
        Dialog::new(
            "Restrict Editing",
            vec![
                Field::Heading("Formatting restrictions".to_owned()),
                Field::Check {
                    label: "Limit formatting to a selection of styles".to_owned(),
                    on: false,
                },
                Field::Tree {
                    label: "Checked styles are currently allowed".to_owned(),
                    rows,
                    current: 0,
                    scroll: 0,
                },
                Field::Check { label: "Block Theme or Scheme switching".to_owned(), on: false },
                Field::Heading("Editing restrictions".to_owned()),
                Field::Check {
                    label: "Allow only this kind of editing in the document".to_owned(),
                    on: true,
                },
                Field::Choice {
                    label: "Allow only this kind of editing".to_owned(),
                    items: modes,
                    current: 0,
                },
                Field::Heading("Start enforcing protection".to_owned()),
                // Word's dialog says this, and it is the truest sentence in
                // it: a password on a document that is not encrypted stops a
                // person, not a program.
                Field::note("The document is not encrypted."),
                Field::note("Anybody who can open the file can take this off."),
                Field::Secret {
                    label: "Enter new password (optional)".to_owned(),
                    value: String::new(),
                },
                Field::Secret {
                    label: "Reenter password to confirm".to_owned(),
                    value: String::new(),
                },
            ],
        )
    }

    /// The dialog that asks for the password back.
    fn unprotect_dialog(rules: &Protection) -> Dialog {
        let said = match rules.mode {
            Some(mode) => mode.label().to_owned(),
            // A restriction on the formatting alone leaves the words open,
            // and saying "restricted to nothing" would be worse than saying
            // what it does restrict.
            None => "Formatting only".to_owned(),
        };
        Dialog::new(
            "Unprotect Document",
            vec![
                Field::Said { label: "Restricted to".to_owned(), value: said },
                Field::Secret { label: "Password".to_owned(), value: String::new() },
            ],
        )
    }

    /// Asks the same dialog again, with what was typed into it kept and the
    /// passwords cleared.
    ///
    /// Kept rather than rebuilt: a person who mistyped one box of twelve
    /// should not lose the other eleven, and the ticks against thirty styles
    /// least of all.
    fn ask_again(&mut self, dialog: &Dialog, why: &str) -> Response {
        let mut again = dialog.clone();
        for row in [PASSWORD, AGAIN] {
            if let Some(Field::Secret { value, .. }) = again.fields.get_mut(row) {
                value.clear();
            }
        }
        self.status = why.to_owned();
        self.ask(Asking::Protect, again)
    }

    /// Puts the restriction on, if the two passwords agree.
    pub(super) fn apply_protection(&mut self, dialog: &Dialog) -> Response {
        let limit = dialog.ticked(LIMIT);
        let mode = dialog
            .ticked(RESTRICT)
            .then(|| EditMode::ALL.get(dialog.chose(MODE)).copied())
            .flatten();
        let word = dialog.said(PASSWORD);

        if !limit && mode.is_none() {
            // Both halves empty. Word greys its Start Enforcing button out
            // until one of them is answered; this says why instead, because a
            // dialog that will not close and will not say why is worse.
            return self.ask_again(dialog, "Nothing is restricted: tick one of the two boxes");
        }
        if word != dialog.said(AGAIN) {
            return self.ask_again(dialog, "The two passwords are not the same");
        }

        let mut wanted = Protection {
            mode,
            formatting: limit,
            theme_locked: limit && dialog.ticked(THEME),
            password: None,
        };
        if !word.is_empty() {
            let Some(salt) = wp_shell::random::bytes::<SALT>() else {
                return self
                    .report("This machine would not give the random bytes a password needs");
            };
            wanted = wanted.behind(&word, &salt);
        }

        // Which styles are allowed is written into the styles themselves and
        // stands whether or not anything is being enforced, so it is written
        // whichever way the box was ticked.
        let allowed = self.allowed_by(dialog);
        if limit {
            self.document.allow_only_styles(&allowed);
        } else {
            self.document.allow_every_style();
        }

        let changed = self.document.set_protection(Some(&wanted));
        self.needs_redraw = true;
        let said = match mode {
            Some(mode) if limit => {
                format!("Restricted to: {}, formatting limited", mode.label())
            }
            Some(mode) => format!("Restricted to: {}", mode.label()),
            None => format!("Formatting limited to {} styles", allowed.len()),
        };
        self.edited(changed, &said)
    }

    /// Which styles the ticks in the dialog say are allowed.
    fn allowed_by(&self, dialog: &Dialog) -> Vec<String> {
        let rows = dialog.tree_rows(STYLES);
        self.lockable_styles()
            .into_iter()
            .zip(rows)
            .filter(|(_, row)| row.tick != Some(false))
            .map(|((id, _), _)| id)
            .collect()
    }

    /// Lifts the restriction, if what was typed is the password.
    pub(super) fn apply_unprotection(&mut self, dialog: &Dialog) -> Response {
        let Some(rules) = self.document.protection_rules() else { return Response::Redraw };
        if rules.opens_with(&dialog.said(ANSWER)) {
            return self.stop_protecting();
        }

        // Saying which hash was wanted matters: "that is not the password" is
        // wrong and maddening when the password was right and this program is
        // the one that cannot check it.
        self.status = match &rules.password {
            Some(password) if !password.understood() => format!(
                "This document's password was hashed with {}, which this program has not got",
                password.algorithm_name()
            ),
            _ => "That is not the password".to_owned(),
        };
        let again = Self::unprotect_dialog(&rules);
        self.ask(Asking::Unprotect, again)
    }

    /// Takes the restriction off.
    fn stop_protecting(&mut self) -> Response {
        let changed = self.document.set_protection(None);
        self.needs_redraw = true;
        self.edited(changed, "Protection lifted")
    }
}

impl Editor {
    /// Whether the document says the text may not be changed where the caret
    /// is.
    ///
    /// Where the caret is, and not merely whether the document is protected:
    /// a form is protected everywhere except inside its fields, and a program
    /// that could not tell the difference could not let a form be filled in.
    #[must_use]
    pub(super) fn is_locked(&self) -> bool {
        // A document opened read-only is locked everywhere, whatever else it
        // says: the question was answered at the door. See
        // [`super::readonly`].
        if self.is_read_only() {
            return true;
        }
        let Some(mode) = self.document.protection() else { return false };
        match mode {
            EditMode::ReadOnly | EditMode::Comments => true,
            // Allowed, and recorded. What stops the recording being switched
            // off is in the document itself.
            EditMode::TrackedChanges => false,
            EditMode::Forms => !self.inside_a_form_field(),
        }
    }

    /// Whether everything that would change is inside one field of a form
    /// that a person is allowed to fill in.
    fn inside_a_form_field(&self) -> bool {
        // A section may be let out of the restriction, and then it is
        // ordinary text and may be edited like any other.
        if !self.document.section_is_form_protected(self.document.section_here()) {
            return true;
        }
        let caret = self.document.caret();
        let (start, end) = self.document.selection().unwrap_or((caret, caret));
        self.document.form_field_at(start).is_some_and(|field| {
            // A tick box and a drop-down hold no text to type into, and a
            // field with filling in turned off is shown and not answered.
            field.kind == FormKind::Text && field.enabled && field.covers(end)
        })
    }

    /// Says why a formatting command did nothing.
    ///
    /// Told apart from the editing restriction because they are different
    /// restrictions with different ways out, and a person told "this document
    /// is protected" when the document is not would look for the wrong thing.
    pub(super) fn refuse_limited(&mut self, command: Command) -> Response {
        let note = if command.is_theme_switching() {
            "This document fixes its theme — Review ▸ Restrict Editing lifts it"
        } else {
            "This document limits formatting to its styles — Review ▸ Restrict Editing lifts it"
        };
        self.report(note)
    }

    /// Says why nothing happened.
    pub(super) fn refuse_locked(&mut self) -> Response {
        if self.is_read_only() {
            return self.refuse_read_only();
        }
        let note = match self.document.protection() {
            Some(EditMode::Comments) => {
                "This document is restricted to comments — Review ▸ Restrict Editing lifts it"
            }
            Some(EditMode::Forms) => {
                "This document is a form: only the fields in it may be filled in"
            }
            _ => "This document is protected — Review ▸ Restrict Editing lifts it",
        };
        self.report(note)
    }

    /// What restriction stands over the document as things are.
    ///
    /// The document's own, or the read-only it asked for at the door and was
    /// given - see [`super::readonly`]. One rule, because the ribbon and the
    /// command both ask it and a button that looks pressable and does nothing
    /// is what that is for.
    #[must_use]
    pub(super) fn restriction_now(&self) -> Option<EditMode> {
        self.document.protection().or_else(|| self.is_read_only().then_some(EditMode::ReadOnly))
    }

    /// Whether the restriction stands in the way of a command, and what to
    /// say if it does.
    ///
    /// The rule itself is [`Command::is_allowed_under`], where the ribbon
    /// reads it too so that a button it greys out and a button this refuses
    /// are the same button.
    pub(super) fn refuse_restricted(&mut self, command: Command) -> Option<Response> {
        let restriction = self.restriction_now();
        let limits = self.document.formatting_limits();
        let here = !self.is_locked();
        // The formatting limit is asked about first, so that a command it
        // forbids is refused with the reason that is true of it.
        if !command.is_allowed_by(limits) {
            return Some(self.refuse_limited(command));
        }
        (!command.is_allowed_under(restriction, limits, here)).then(|| self.refuse_locked())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chrome::dialog::Answer;
    use crate::editor::Editor;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::Document;
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event};

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    fn editor() -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("One")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor
    }

    /// Fills in the Restrict Editing dialog and answers it.
    fn protect(editor: &mut Editor, mode: usize, word: &str) {
        editor.run(Command::RestrictEditing);
        let dialog = editor.dialog.as_mut().expect("the dialog");
        if let Some(Field::Choice { current, .. }) = dialog.fields.get_mut(MODE) {
            *current = mode;
        }
        for row in [PASSWORD, AGAIN] {
            if let Some(Field::Secret { value, .. }) = dialog.fields.get_mut(row) {
                *value = word.to_owned();
            }
        }
        editor.finish_dialog(Answer::Accept);
    }

    #[test]
    fn a_restriction_with_a_password_is_not_lifted_without_it() {
        let mut editor = editor();
        protect(&mut editor, 0, "shibboleth");
        assert_eq!(editor.document.protection(), Some(EditMode::ReadOnly));

        // Stop Protection asks rather than stopping.
        editor.run(Command::RestrictEditing);
        let dialog = editor.dialog.as_mut().expect("the dialog asking for the password");
        if let Some(Field::Secret { value, .. }) = dialog.fields.get_mut(ANSWER) {
            *value = "open sesame".to_owned();
        }
        editor.finish_dialog(Answer::Accept);
        assert_eq!(
            editor.document.protection(),
            Some(EditMode::ReadOnly),
            "a wrong password lifted it"
        );
        assert!(editor.status.contains("not the password"), "{}", editor.status);

        // And the right one does.
        let dialog = editor.dialog.as_mut().expect("asked again");
        if let Some(Field::Secret { value, .. }) = dialog.fields.get_mut(ANSWER) {
            *value = "shibboleth".to_owned();
        }
        editor.finish_dialog(Answer::Accept);
        assert_eq!(editor.document.protection(), None);
    }

    #[test]
    fn two_passwords_that_differ_protect_nothing_and_ask_again() {
        let mut editor = editor();
        editor.run(Command::RestrictEditing);
        let dialog = editor.dialog.as_mut().expect("the dialog");
        if let Some(Field::Secret { value, .. }) = dialog.fields.get_mut(PASSWORD) {
            *value = "one".to_owned();
        }
        if let Some(Field::Secret { value, .. }) = dialog.fields.get_mut(AGAIN) {
            *value = "another".to_owned();
        }
        editor.finish_dialog(Answer::Accept);

        assert_eq!(editor.document.protection(), None, "protected on a mistyped password");
        assert!(editor.status.contains("not the same"), "{}", editor.status);
        assert!(editor.dialog.is_some(), "the dialog was not asked again");
    }

    #[test]
    fn a_password_is_not_shown_while_it_is_typed() {
        let mut editor = editor();
        editor.run(Command::RestrictEditing);
        for character in "secret".chars() {
            editor.handle(Event::Char(character));
        }
        // Whatever it lands on first, nothing that was typed into a password
        // box is in a field anybody can read off the screen.
        let dialog = editor.dialog.as_ref().expect("the dialog");
        for field in &dialog.fields {
            if let Field::Text { value, .. } | Field::Said { value, .. } = field {
                assert!(!value.contains("secret"), "the password is showing: {value}");
            }
        }
    }

    #[test]
    fn a_document_restricted_to_comments_refuses_a_ribbon_button_and_takes_a_comment() {
        let mut editor = editor();
        protect(&mut editor, 1, "");
        assert_eq!(editor.document.protection(), Some(EditMode::Comments));

        editor.run(Command::Format(wp_docx::CharacterFormat::Bold));
        assert!(editor.status.contains("restricted to comments"), "{}", editor.status);

        editor.status.clear();
        editor.run(Command::NewComment);
        assert!(!editor.status.contains("restricted"), "a comment was refused: {}", editor.status);
    }

    #[test]
    fn restricting_to_tracked_changes_starts_recording_them_and_will_not_stop() {
        let mut editor = editor();
        protect(&mut editor, 2, "");
        assert!(editor.document.tracking_changes(), "the restriction did not start the recording");

        editor.run(Command::TrackChanges);
        assert!(editor.document.tracking_changes(), "the recording was switched off");
    }

    /// Ticks "Limit formatting to a selection of styles", leaves the named
    /// styles ticked and unticks every other, and answers the dialog.
    fn limit_formatting(editor: &mut Editor, allowed: &[&str], theme: bool) {
        editor.run(Command::RestrictEditing);
        let styles = editor.lockable_styles();
        let dialog = editor.dialog.as_mut().expect("the dialog");
        if let Some(Field::Check { on, .. }) = dialog.fields.get_mut(LIMIT) {
            *on = true;
        }
        if let Some(Field::Check { on, .. }) = dialog.fields.get_mut(THEME) {
            *on = theme;
        }
        // The editing half left alone, which is the case worth testing: a
        // document anybody may type in and nobody may format.
        if let Some(Field::Check { on, .. }) = dialog.fields.get_mut(RESTRICT) {
            *on = false;
        }
        if let Some(Field::Tree { rows, .. }) = dialog.fields.get_mut(STYLES) {
            for (row, (id, _)) in rows.iter_mut().zip(&styles) {
                row.tick = Some(allowed.iter().any(|wanted| wanted.eq_ignore_ascii_case(id)));
            }
        }
        editor.finish_dialog(Answer::Accept);
    }

    #[test]
    fn the_styles_that_were_not_ticked_are_locked_and_leave_the_gallery() {
        let mut editor = editor();
        assert!(
            editor.style_gallery().iter().any(|sample| sample.id.as_deref() == Some("Heading1")),
            "the gallery had no Heading1 to lose"
        );

        limit_formatting(&mut editor, &["Title"], false);
        assert!(editor.document.formatting_is_limited());
        assert!(editor.document.style_is_locked("Heading1"));
        assert!(!editor.document.style_is_locked("Title"), "a ticked style was locked");

        let gallery = editor.style_gallery();
        assert!(
            !gallery.iter().any(|sample| sample.id.as_deref() == Some("Heading1")),
            "a style that cannot be applied is still being offered"
        );
        assert!(gallery.iter().any(|sample| sample.id.as_deref() == Some("Title")));
    }

    #[test]
    fn a_locked_style_is_refused_when_it_is_asked_for_by_name() {
        let mut editor = editor();
        limit_formatting(&mut editor, &["Title"], false);

        editor.apply_style(Some("Heading1"));
        assert!(editor.document.style_here().is_none(), "a locked style was applied");
        assert!(editor.status.contains("limits formatting"), "{}", editor.status);

        editor.apply_style(Some("Title"));
        assert_eq!(editor.document.style_here().as_deref(), Some("Title"));
    }

    #[test]
    fn formatting_is_refused_and_typing_is_not() {
        let mut editor = editor();
        limit_formatting(&mut editor, &["Title"], false);
        assert_eq!(editor.document.protection(), None, "the words were restricted too");

        editor.run(Command::Format(wp_docx::CharacterFormat::Bold));
        assert!(editor.status.contains("limits formatting"), "{}", editor.status);
        assert!(!editor.document.format_is_on(wp_docx::CharacterFormat::Bold), "it went bold");

        // And the words themselves are nobody's business but the typist's.
        let before = editor.document.plain_text();
        editor.handle(Event::Char('x'));
        assert_ne!(editor.document.plain_text(), before, "a formatting limit stopped the typing");
    }

    #[test]
    fn the_ribbon_greys_out_what_the_limit_refuses() {
        // The same question asked twice, and the answers must agree: a button
        // that looks pressable and does nothing is what this is for.
        let mut editor = editor();
        limit_formatting(&mut editor, &["Title"], true);
        let state = editor.toolbar_state();

        for command in [
            Command::Format(wp_docx::CharacterFormat::Bold),
            Command::FontDialog,
            Command::Align(wp_docx::model::Alignment::Center),
            Command::FormatPainter,
            Command::Themes,
        ] {
            assert!(!crate::chrome::is_enabled(command, &state), "{command:?} is still offered",);
        }
        for command in [Command::Find, Command::NewComment, Command::InsertTable] {
            assert!(crate::chrome::is_enabled(command, &state), "{command:?} was grey");
        }
    }

    #[test]
    fn the_theme_is_left_alone_unless_that_box_is_ticked() {
        let mut free = editor();
        limit_formatting(&mut free, &["Title"], false);
        assert!(!free.document.theme_is_locked());
        assert!(crate::chrome::is_enabled(Command::Themes, &free.toolbar_state()));

        let mut fixed = editor();
        limit_formatting(&mut fixed, &["Title"], true);
        assert!(fixed.document.theme_is_locked());
        fixed.run(Command::Themes);
        assert!(fixed.status.contains("fixes its theme"), "{}", fixed.status);
    }

    #[test]
    fn a_dialog_that_restricts_nothing_says_so_and_asks_again() {
        let mut editor = editor();
        editor.run(Command::RestrictEditing);
        let dialog = editor.dialog.as_mut().expect("the dialog");
        for row in [LIMIT, RESTRICT] {
            if let Some(Field::Check { on, .. }) = dialog.fields.get_mut(row) {
                *on = false;
            }
        }
        editor.finish_dialog(Answer::Accept);

        assert!(editor.document.protection_rules().is_none(), "nothing was asked and it was set");
        assert!(editor.status.contains("Nothing is restricted"), "{}", editor.status);
        assert!(editor.dialog.is_some(), "the dialog was not asked again");
    }

    #[test]
    fn the_ticks_are_kept_when_the_dialog_is_asked_again() {
        let mut editor = editor();
        editor.run(Command::RestrictEditing);
        let dialog = editor.dialog.as_mut().expect("the dialog");
        if let Some(Field::Tree { rows, .. }) = dialog.fields.get_mut(STYLES) {
            for row in rows.iter_mut() {
                row.tick = Some(false);
            }
        }
        if let Some(Field::Check { on, .. }) = dialog.fields.get_mut(LIMIT) {
            *on = true;
        }
        if let Some(Field::Secret { value, .. }) = dialog.fields.get_mut(PASSWORD) {
            *value = "one".to_owned();
        }
        if let Some(Field::Secret { value, .. }) = dialog.fields.get_mut(AGAIN) {
            *value = "another".to_owned();
        }
        editor.finish_dialog(Answer::Accept);

        let dialog = editor.dialog.as_ref().expect("asked again");
        assert!(dialog.ticked(LIMIT), "the tick was lost");
        assert!(
            dialog.tree_rows(STYLES).iter().all(|row| row.tick == Some(false)),
            "thirty ticks were thrown away over a mistyped password"
        );
        assert!(dialog.said(PASSWORD).is_empty(), "the password was left in the box");
    }
}
