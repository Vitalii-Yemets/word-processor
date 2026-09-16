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

use crate::chrome::dialog::{Dialog, Field};
use crate::chrome::Command;

use super::dialogs::Asking;
use super::Editor;

/// Where each answer sits in the Restrict Editing dialog.
///
/// Named rather than counted twice, for the reason every other dialog in this
/// program names its rows: the headings take places in the list too.
const MODE: usize = 1;
const PASSWORD: usize = 5;
const AGAIN: usize = 6;

/// And the one box of the dialog that asks for it back.
const ANSWER: usize = 1;

/// How many bytes of salt, asked of the format rather than decided here.
const SALT: usize = wp_docx::protection::SALT_BYTES;

impl Editor {
    /// Word's Restrict Editing: the restriction, or the way out of one.
    pub(super) fn open_protection(&mut self) -> Response {
        match self.document.protection_rules() {
            None => {
                let dialog = Self::protect_dialog(0);
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

    /// The dialog that puts a restriction on, with one of the modes already
    /// chosen.
    fn protect_dialog(chosen: usize) -> Dialog {
        let modes = EditMode::ALL.iter().map(|mode| mode.label().to_owned()).collect();
        Dialog::new(
            "Restrict Editing",
            vec![
                Field::Heading("Editing restrictions".to_owned()),
                Field::Choice {
                    label: "Allow only this kind of editing".to_owned(),
                    items: modes,
                    current: chosen,
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
        Dialog::new(
            "Unprotect Document",
            vec![
                Field::Said {
                    label: "Restricted to".to_owned(),
                    value: rules.mode.label().to_owned(),
                },
                Field::Secret { label: "Password".to_owned(), value: String::new() },
            ],
        )
    }

    /// Puts the restriction on, if the two passwords agree.
    pub(super) fn apply_protection(&mut self, dialog: &Dialog) -> Response {
        let chosen = dialog.chose(MODE);
        let Some(mode) = EditMode::ALL.get(chosen).copied() else { return Response::Ignored };
        let word = dialog.said(PASSWORD);

        if word != dialog.said(AGAIN) {
            // Asked again rather than given up on, with the mode kept: the
            // person mistyped one box of four and should not lose the rest.
            let again = Self::protect_dialog(chosen);
            self.status = "The two passwords are not the same".to_owned();
            return self.ask(Asking::Protect, again);
        }

        let mut wanted = Protection::new(mode);
        if !word.is_empty() {
            let Some(salt) = wp_shell::random::bytes::<SALT>() else {
                return self
                    .report("This machine would not give the random bytes a password needs");
            };
            wanted = wanted.behind(&word, &salt);
        }

        let changed = self.document.set_protection(Some(&wanted));
        self.needs_redraw = true;
        self.edited(changed, &format!("Restricted to: {}", mode.label()))
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

    /// Says why nothing happened.
    pub(super) fn refuse_locked(&mut self) -> Response {
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

    /// Whether the restriction stands in the way of a command, and what to
    /// say if it does.
    ///
    /// The rule itself is [`Command::is_allowed_under`], where the ribbon
    /// reads it too so that a button it greys out and a button this refuses
    /// are the same button.
    pub(super) fn refuse_restricted(&mut self, command: Command) -> Option<Response> {
        let restriction = self.document.protection();
        let here = !self.is_locked();
        (!command.is_allowed_under(restriction, here)).then(|| self.refuse_locked())
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
}
