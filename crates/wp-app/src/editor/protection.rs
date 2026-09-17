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
//! # The stretches a restriction lets through
//!
//! A restriction may carry exceptions: stretches of the document that stay
//! editable while the rest is shut. They are written as the same pair of
//! markers Block Authors writes — see [`wp_docx::permissions`] — and the rule
//! is one rule read both ways. Inside a pair of markers only the people they
//! name may edit; outside them, whatever the document says. So a stretch
//! marked for everybody is a way in to a protected document, and a stretch
//! marked for one person is a way out of an unprotected one, and the program
//! does not have to know which sort of document it is looking at.
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
const STYLE_SET: usize = 4;
const AUTO_FORMAT: usize = 5;
const RESTRICT: usize = 7;
const MODE: usize = 8;
const PASSWORD: usize = 12;
const AGAIN: usize = 13;

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
    /// The document's own first, in the order it defines them, and then the
    /// ones it mentions without defining — Word's latent styles, which are as
    /// real to somebody applying a style as the defined ones and would
    /// otherwise be three hundred doors left open. See [`wp_docx::locking`].
    fn lockable_styles(&self) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = self
            .document
            .styles()
            .all()
            .iter()
            .map(|style| {
                let name = style.name.clone().unwrap_or_else(|| style.id.clone());
                (style.id.clone(), super::insert::title_case(&name))
            })
            .collect();
        out.extend(
            self.document
                .latent_styles()
                .into_iter()
                .map(|latent| (latent.name.clone(), super::insert::title_case(&latent.name))),
        );
        out
    }

    /// How many of that list are styles the document actually defines.
    ///
    /// The two halves are ticked the same way and written down differently:
    /// a defined style carries `w:locked`, a latent one is an exception
    /// inside `w:latentStyles`.
    fn defined_style_count(&self) -> usize {
        self.document.styles().all().len()
    }

    /// The dialog that puts a restriction on.
    fn protect_dialog(&self) -> Dialog {
        let modes = EditMode::ALL.iter().map(|mode| mode.label().to_owned()).collect();
        let defined = self.defined_style_count();
        let rows = self
            .lockable_styles()
            .into_iter()
            .enumerate()
            .map(|(at, (id, name))| {
                let allowed = if at < defined {
                    !self.document.style_is_locked(&id)
                } else {
                    self.document.latent_style_is_available(&id)
                };
                TreeRow::ticked(0, &name, allowed)
            })
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
                Field::Check { label: "Block Quick Style Set switching".to_owned(), on: false },
                // The one box under this heading that lets something through
                // rather than shutting it: a `*word*` turned bold as it is
                // typed is a correction the person asked for by typing the
                // marks, and Word offers to let it through.
                Field::Check {
                    label: "Allow AutoFormat to override formatting restrictions".to_owned(),
                    on: false,
                },
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
            style_set_locked: limit && dialog.ticked(STYLE_SET),
            auto_format_override: dialog.ticked(AUTO_FORMAT),
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
            let defined = self.defined_style_count();
            let names = self.lockable_styles();
            self.document.allow_only_styles(&allowed);
            // And the ones the document only mentions, each by name: they
            // have no definition to carry a mark, so the mark goes on the
            // exception instead.
            for (id, _) in names.into_iter().skip(defined) {
                let locked = !allowed.iter().any(|allowed| allowed.eq_ignore_ascii_case(&id));
                self.document.set_latent_style_locked(&id, locked);
            }
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
        let Some(mode) = self.restriction_now() else { return false };
        match mode {
            EditMode::ReadOnly | EditMode::Comments => true,
            // Allowed, and recorded. What stops the recording being switched
            // off is in the document itself.
            EditMode::TrackedChanges => false,
            EditMode::Forms => !self.inside_a_form_field(),
        }
    }

    /// Whether everything that would change is inside one stretch with its
    /// own rule about who may edit it, and whether that rule admits the
    /// person at the keyboard.
    ///
    /// `None` where the selection is not in one at all, which leaves the
    /// answer to the rest of the document.
    fn inside_a_marked_stretch(&self) -> Option<bool> {
        let caret = self.document.caret();
        let (start, end) = self.document.selection().unwrap_or((caret, caret));
        let marked = self.document.locked_at(start)?;
        // Half in and half out is out: an edit that ran off the end of the
        // stretch would be an edit to the part that is shut.
        if !marked.covers(end) {
            return Some(false);
        }
        Some(marked.admits(&super::files::user_name()))
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
        } else if command.is_style_set_switching() {
            "This document fixes its style set — Review ▸ Restrict Editing lifts it"
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
        if self.inside_a_locked_control() {
            return self.report("The contents of this control cannot be edited");
        }
        // A stretch that names somebody else says so by name: "this document
        // is protected" would send a person to a dialog that would not help.
        if let Some(marked) = self.document.locked_at(self.document.caret()) {
            if !marked.admits(&super::files::user_name()) {
                let named = marked.named().to_owned();
                let note = if named.is_empty() {
                    crate::messages::t("This stretch is locked").to_owned()
                } else {
                    crate::messages::with("This stretch may only be edited by {0}", &[&named])
                };
                self.status = note;
                self.needs_redraw = true;
                return Response::Redraw;
            }
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

    /// Whether a correction made as somebody types may format text the
    /// restriction forbids them to format by hand.
    ///
    /// Word's "Allow AutoFormat to override formatting restrictions", which
    /// is the only box on that half of its pane that lets something through.
    /// A document that does not say so gets the plain answer: a restriction
    /// on formatting is a restriction on formatting, whoever asked for it.
    #[must_use]
    pub(super) fn autoformat_may_override(&self) -> bool {
        let limits = self.document.formatting_limits();
        !limits.formatting || limits.auto_format
    }

    /// What restriction stands over the place the caret is in, as things are.
    ///
    /// Three things can make one, and they are asked in this order because
    /// each overrules the next:
    ///
    /// 1. The document was opened read-only, which was settled at the door
    ///    and holds everywhere - see [`super::readonly`].
    /// 2. The caret is inside a stretch with its own rule about who may edit
    ///    it. If it admits the person at the keyboard nothing else applies
    ///    here, which is what makes an exception an exception; if it does
    ///    not, this place is shut whatever the rest of the document says.
    /// 3. What the document itself says.
    ///
    /// One answer, because the ribbon asks it to know what to grey out and
    /// the command asks it to know whether to run, and a button that looks
    /// pressable and does nothing is what that is for.
    #[must_use]
    pub(super) fn restriction_now(&self) -> Option<EditMode> {
        if self.is_read_only() {
            return Some(EditMode::ReadOnly);
        }
        match self.inside_a_marked_stretch() {
            Some(true) => return None,
            Some(false) => return Some(EditMode::ReadOnly),
            None => {}
        }
        // A control whose contents are locked is shut wherever it is, and by
        // the document rather than by a restriction: see
        // [`super::controls`].
        if self.inside_a_locked_control() {
            return Some(EditMode::ReadOnly);
        }
        self.document.protection()
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
        body.blocks.push(Block::Paragraph(Paragraph::text("One two three four five")));
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
    /// Restricts the document to nothing at all being changed, the short way.
    fn read_only(editor: &mut Editor) {
        editor
            .document
            .set_protection(Some(&wp_docx::protection::Protection::new(EditMode::ReadOnly)));
    }

    /// Marks a stretch of the first paragraph as one everybody may edit.
    fn everyone_may_edit(editor: &mut Editor, from: usize, to: usize) {
        editor.document.set_caret(wp_docx::TextPosition::new(0, from));
        editor.document.extend_selection_to(wp_docx::TextPosition::new(0, to));
        editor.run(Command::AllowEveryone);
        editor.document.set_caret(wp_docx::TextPosition::new(0, from));
    }

    /// Types one letter and says whether it arrived.
    fn typing_arrives(editor: &mut Editor) -> bool {
        let before = editor.document.plain_text();
        editor.handle(Event::Char('x'));
        editor.document.plain_text() != before
    }

    #[test]
    fn an_exception_is_editable_while_the_rest_of_the_document_is_not() {
        let mut editor = editor();
        editor.document.set_caret(wp_docx::TextPosition::new(0, 0));
        // "One" of "One two three" - a stretch in the middle of the paragraph.
        everyone_may_edit(&mut editor, 4, 7);
        read_only(&mut editor);

        // Inside it, typing arrives.
        editor.document.set_caret(wp_docx::TextPosition::new(0, 5));
        assert!(!editor.is_locked(), "the exception is shut");
        assert!(typing_arrives(&mut editor), "nothing could be typed into the exception");

        // Outside it, nothing does.
        editor.document.set_caret(wp_docx::TextPosition::new(0, 0));
        assert!(editor.is_locked(), "the rest of the document is open");
        assert!(!typing_arrives(&mut editor), "the restriction let something through");
    }

    #[test]
    fn a_selection_that_runs_off_the_end_of_an_exception_is_refused() {
        // Half in and half out is out: the edit would reach the part that is
        // shut.
        let mut editor = editor();
        everyone_may_edit(&mut editor, 4, 7);
        read_only(&mut editor);

        editor.document.set_caret(wp_docx::TextPosition::new(0, 5));
        editor.document.extend_selection_to(wp_docx::TextPosition::new(0, 12));
        assert!(editor.is_locked(), "a selection running out of the exception was let through");
    }

    #[test]
    fn a_stretch_blocked_for_somebody_else_is_shut_even_with_no_restriction() {
        let mut editor = editor();
        editor.document.set_caret(wp_docx::TextPosition::new(0, 4));
        editor.document.extend_selection_to(wp_docx::TextPosition::new(0, 7));
        editor.document.block_authors("Somebody Else");
        editor.document.set_caret(wp_docx::TextPosition::new(0, 5));

        assert_eq!(editor.document.protection(), None, "the document is not restricted");
        assert!(editor.is_locked(), "a stretch belonging to somebody else took typing");
        assert!(!typing_arrives(&mut editor));

        // And it says whose it is rather than talking about a restriction.
        editor.run(Command::Format(wp_docx::CharacterFormat::Bold));
        assert!(editor.status.contains("Somebody Else"), "{}", editor.status);
    }

    #[test]
    fn the_stretch_this_person_blocked_is_still_theirs_to_edit() {
        let mut editor = editor();
        editor.document.set_caret(wp_docx::TextPosition::new(0, 4));
        editor.document.extend_selection_to(wp_docx::TextPosition::new(0, 7));
        editor.run(Command::BlockAuthors);
        editor.document.set_caret(wp_docx::TextPosition::new(0, 5));

        assert!(!editor.is_locked(), "a person was locked out of their own stretch");
        assert!(typing_arrives(&mut editor));
    }

    #[test]
    fn the_button_takes_the_exception_off_again() {
        let mut editor = editor();
        everyone_may_edit(&mut editor, 4, 7);
        assert_eq!(editor.document.locked_regions().len(), 1);

        editor.document.set_caret(wp_docx::TextPosition::new(0, 5));
        editor.run(Command::AllowEveryone);
        assert!(editor.document.locked_regions().is_empty(), "it would not come off");
    }

    #[test]
    fn the_button_says_what_it_needs_when_nothing_is_selected() {
        let mut editor = editor();
        editor.document.set_caret(wp_docx::TextPosition::new(0, 4));
        editor.run(Command::AllowEveryone);
        assert!(editor.document.locked_regions().is_empty());
        assert!(editor.status.contains("Select the text"), "{}", editor.status);
    }

    #[test]
    fn a_stretch_that_belongs_to_somebody_is_not_handed_to_everybody() {
        // Blocked for the person at the keyboard, so they may edit it and the
        // command runs - and finds that the stretch is already spoken for.
        let mut editor = editor();
        editor.document.set_caret(wp_docx::TextPosition::new(0, 4));
        editor.document.extend_selection_to(wp_docx::TextPosition::new(0, 7));
        editor.run(Command::BlockAuthors);
        editor.document.set_caret(wp_docx::TextPosition::new(0, 5));

        editor.run(Command::AllowEveryone);
        assert!(editor.status.contains("already belongs"), "{}", editor.status);
        assert!(!editor.document.locked_regions()[0].for_everyone(), "it was taken over");
    }

    #[test]
    fn somebody_elses_stretch_cannot_be_handed_to_everybody_either() {
        let mut editor = editor();
        editor.document.set_caret(wp_docx::TextPosition::new(0, 4));
        editor.document.extend_selection_to(wp_docx::TextPosition::new(0, 7));
        editor.document.block_authors("Somebody Else");
        editor.document.set_caret(wp_docx::TextPosition::new(0, 5));

        editor.run(Command::AllowEveryone);
        assert!(editor.status.contains("Somebody Else"), "{}", editor.status);
        assert!(!editor.document.locked_regions()[0].for_everyone(), "it was taken over");
    }
    #[test]
    fn a_correction_that_formats_is_refused_while_formatting_is_limited() {
        // Typing `*word*` turns it bold, which is formatting by hand under
        // another name. Word's Restrict Editing has a box that lets it
        // through; without it the marks stay marks.
        let mut editor = editor();
        limit_formatting(&mut editor, &["Title"], false);
        assert!(!editor.autoformat_may_override());

        editor.document.set_caret(wp_docx::TextPosition::new(0, 0));
        for character in "*word* ".chars() {
            editor.handle(Event::Char(character));
        }
        assert!(
            editor.document.plain_text().contains("*word*"),
            "the marks went: {}",
            editor.document.plain_text()
        );
    }

    #[test]
    fn the_box_that_lets_it_through_lets_it_through() {
        let mut editor = editor();
        editor.run(Command::RestrictEditing);
        let dialog = editor.dialog.as_mut().expect("the dialog");
        for (row, on) in [(LIMIT, true), (AUTO_FORMAT, true), (RESTRICT, false)] {
            if let Some(Field::Check { on: value, .. }) = dialog.fields.get_mut(row) {
                *value = on;
            }
        }
        editor.finish_dialog(Answer::Accept);
        assert!(editor.document.formatting_is_limited());
        assert!(editor.autoformat_may_override(), "the box was ticked and changed nothing");

        editor.document.set_caret(wp_docx::TextPosition::new(0, 0));
        for character in "*word* ".chars() {
            editor.handle(Event::Char(character));
        }
        assert!(
            !editor.document.plain_text().contains("*word*"),
            "the correction was refused anyway: {}",
            editor.document.plain_text()
        );
    }

    #[test]
    fn a_fixed_style_set_cannot_be_switched() {
        let mut editor = editor();
        editor.run(Command::RestrictEditing);
        let dialog = editor.dialog.as_mut().expect("the dialog");
        for (row, on) in [(LIMIT, true), (STYLE_SET, true), (RESTRICT, false)] {
            if let Some(Field::Check { on: value, .. }) = dialog.fields.get_mut(row) {
                *value = on;
            }
        }
        editor.finish_dialog(Answer::Accept);

        let state = editor.toolbar_state();
        assert!(!crate::chrome::is_enabled(Command::StyleSet, &state), "the button is still lit");
        editor.run(Command::StyleSet);
        assert!(editor.status.contains("style set"), "{}", editor.status);
        assert!(editor.popup.is_none(), "the gallery opened anyway");
    }

    #[test]
    fn a_style_set_that_is_not_fixed_can_be() {
        let mut editor = editor();
        assert!(crate::chrome::is_enabled(Command::StyleSet, &editor.toolbar_state()));
        // The gallery hangs under its button, so the tab it is on has to be
        // the one showing - as it is when a person presses it.
        editor.ribbon.tab = crate::chrome::ribbon::Tab::Design;
        let (width, height) = (editor.view_width, editor.view_height);
        editor.draw(width, height);
        editor.run(Command::StyleSet);
        assert!(editor.popup.is_some(), "the gallery did not open");
    }
}
