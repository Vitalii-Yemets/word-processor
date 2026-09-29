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

use crate::chrome::dialog::{Answer, Button, Dialog, Field, TreeRow};
use crate::chrome::Command;

use super::dialogs::Asking;
use super::Editor;

/// Where each answer sits in the Formatting Restrictions dialog.
///
/// Named rather than counted twice, for the reason every other dialog in this
/// program names its rows: the headings take places in the list too.
const LIMIT: usize = 0;
const STYLES: usize = 1;
const AUTO_FORMAT: usize = 3;
const THEME: usize = 4;
const STYLE_SET: usize = 5;

/// And the one box of the dialog that asks a password back.
const ANSWER: usize = 1;

/// Word's three buttons above the list of styles.
///
/// The difference between ticking three boxes and ticking three hundred. Two
/// of them are a loop; the third is the interesting one.
pub(super) const ALL_OF_THEM: &str = "All";
pub(super) const RECOMMENDED: &str = "Recommended Minimum";
pub(super) const NONE_OF_THEM: &str = "None";

impl Editor {
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
                (style.id.clone(), crate::names::shown(&name))
            })
            .collect();
        out.extend(
            self.document
                .latent_styles()
                .into_iter()
                .map(|latent| (latent.name.clone(), crate::names::shown(&latent.name))),
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

    /// Word's Formatting Restrictions: which styles may be used, and the three
    /// boxes that go with the question.
    ///
    /// A dialog and not part of the pane because it is a list of every style
    /// the document has — three hundred of them in a document made from a
    /// template — and a list that long down the side of the window would leave
    /// no window at all. Word makes the same split for the same reason.
    pub(super) fn open_formatting_limits(&mut self) -> Response {
        let dialog = self.formatting_dialog();
        self.ask(Asking::FormattingLimits, dialog)
    }

    /// What that dialog is made of.
    fn formatting_dialog(&self) -> Dialog {
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
        Dialog::with_buttons(
            "Formatting Restrictions",
            vec![
                Field::Check {
                    label: "Limit formatting to a selection of styles".to_owned(),
                    on: self.restrict_limiting,
                },
                Field::Tree {
                    label: "Checked styles are currently allowed".to_owned(),
                    rows,
                    current: 0,
                    scroll: 0,
                },
                Field::Heading("Formatting".to_owned()),
                // The one box under this heading that lets something through
                // rather than shutting it: a `*word*` turned bold as it is
                // typed is a correction the person asked for by typing the
                // marks, and Word offers to let it through.
                Field::Check {
                    label: "Allow AutoFormat to override formatting restrictions".to_owned(),
                    on: self.restrict_auto_format,
                },
                Field::Check {
                    label: "Block Theme or Scheme switching".to_owned(),
                    on: self.restrict_theme_locked,
                },
                Field::Check {
                    label: "Block Quick Style Set switching".to_owned(),
                    on: self.restrict_style_set_locked,
                },
            ],
            vec![
                Button {
                    label: ALL_OF_THEM.to_owned(),
                    answer: Answer::Named(ALL_OF_THEM),
                    default: false,
                },
                Button {
                    label: RECOMMENDED.to_owned(),
                    answer: Answer::Named(RECOMMENDED),
                    default: false,
                },
                Button {
                    label: NONE_OF_THEM.to_owned(),
                    answer: Answer::Named(NONE_OF_THEM),
                    default: false,
                },
                Button { label: "OK".to_owned(), answer: Answer::Accept, default: true },
                Button { label: "Cancel".to_owned(), answer: Answer::Cancel, default: false },
            ],
        )
    }

    /// Which styles the document puts forward as the ones to write with.
    ///
    /// The template's own answer, out of the `w:qFormat` each style carries,
    /// and not a list invented here: a document made from somebody's template
    /// is for whatever that template says it is for, and this program has
    /// never seen it.
    ///
    /// A document that says nothing — no style marked at all — gets the
    /// headings and the body style, because a set that recommends nothing
    /// would tick nothing and make the button useless. That is a guess, and
    /// it is only made where there is nothing to read.
    #[must_use]
    fn recommended_styles(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .document
            .styles()
            .all()
            .iter()
            .filter(|style| style.recommended)
            .map(|style| style.id.clone())
            .collect();
        out.extend(
            self.document
                .latent_styles()
                .into_iter()
                .filter(|latent| latent.recommended)
                .map(|latent| latent.name),
        );
        if !out.is_empty() {
            return out;
        }

        // Nothing said, so the ones every document is written with.
        self.lockable_styles()
            .into_iter()
            .map(|(id, _)| id)
            .filter(|id| {
                let lowered = id.to_lowercase();
                lowered == "normal" || lowered.starts_with("heading") || lowered == "title"
            })
            .collect()
    }

    /// One of the three buttons: ticks what it names and leaves the dialog up.
    pub(super) fn formatting_button(&mut self, dialog: &Dialog, button: &str) -> Response {
        let wanted: Vec<String> = match button {
            ALL_OF_THEM => self.lockable_styles().into_iter().map(|(id, _)| id).collect(),
            NONE_OF_THEM => Vec::new(),
            RECOMMENDED => self.recommended_styles(),
            _ => return Response::Ignored,
        };

        let mut again = dialog.clone();
        // Ticking any of them means the limit is on: a person who presses
        // Recommended Minimum has said which styles they want, and leaving
        // the box above unticked would throw the answer away.
        if let Some(Field::Check { on, .. }) = again.fields.get_mut(LIMIT) {
            *on = button != ALL_OF_THEM;
        }
        let styles = self.lockable_styles();
        if let Some(Field::Tree { rows, .. }) = again.fields.get_mut(STYLES) {
            for (row, (id, _)) in rows.iter_mut().zip(&styles) {
                row.tick = Some(wanted.iter().any(|one| one.eq_ignore_ascii_case(id)));
            }
        }
        self.status = crate::messages::with(
            "{0} styles allowed",
            &[&wanted.len().min(styles.len()).to_string()],
        );
        self.ask(Asking::FormattingLimits, again)
    }

    /// The dialog that asks for the password back.
    pub(super) fn unprotect_dialog(rules: &Protection) -> Dialog {
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

    /// Writes down which formatting is allowed.
    ///
    /// The styles go into the document then and there, because which styles a
    /// document allows is written into the styles themselves and stands
    /// whether or not anything is being enforced. The three boxes are about
    /// the restriction rather than about the styles, so they wait in the pane
    /// for the button that starts it.
    pub(super) fn apply_formatting_limits(&mut self, dialog: &Dialog) -> Response {
        let limit = dialog.ticked(LIMIT);
        self.restrict_limiting = limit;
        self.restrict_auto_format = dialog.ticked(AUTO_FORMAT);
        self.restrict_theme_locked = dialog.ticked(THEME);
        self.restrict_style_set_locked = dialog.ticked(STYLE_SET);

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

        self.relayout();
        self.needs_redraw = true;
        if !limit {
            return self.edited(true, "Every style may be used");
        }
        let said = crate::messages::with(
            "Formatting limited to {0} styles",
            &[&allowed.len().to_string()],
        );
        self.edited(true, &said)
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
    pub(super) fn stop_protecting(&mut self) -> Response {
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
        let covering = self.document.locked_all_at(start);
        if covering.is_empty() {
            return None;
        }

        // Half in and half out is out: an edit that ran off the end of a
        // stretch would be an edit to the part that is shut. So the question
        // is about the pairs that hold the whole of what would change.
        let whole: Vec<_> = covering.into_iter().filter(|marked| marked.covers(end)).collect();
        if whole.is_empty() {
            return Some(false);
        }
        // One pair naming somebody else does not shut a person out of a pair
        // that names them: the format gives a marker a single editor, so two
        // people sharing a stretch is two pairs round the same words, and a
        // rule that read only the first would let one of them in and not the
        // other.
        let me = super::files::user_name();
        Some(whole.iter().any(|marked| marked.admits(&me)))
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
        // Named where each stands: a sentence reached through a variable is
        // one the catalogue never sees.
        use crate::messages::t;
        let note = if command.is_theme_switching() {
            t("This document fixes its theme — Review ▸ Restrict Editing lifts it")
        } else if command.is_style_set_switching() {
            t("This document fixes its style set — Review ▸ Restrict Editing lifts it")
        } else {
            t("This document limits formatting to its styles — Review ▸ Restrict Editing lifts it")
        };
        self.report(note)
    }

    /// Says why nothing happened.
    pub(super) fn refuse_locked(&mut self) -> Response {
        if self.is_read_only() {
            return self.refuse_read_only();
        }
        if self.inside_a_locked_control() {
            return self
                .report(crate::messages::t("The contents of this control cannot be edited"));
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

    /// Restricts the document the way a person does: the pane, the kind of
    /// editing, the button, and the password under it.
    fn protect(editor: &mut Editor, mode: usize, word: &str) {
        editor.run(Command::RestrictEditing);
        assert!(editor.show_restrict, "the pane did not open");
        editor.choose_restrict_mode(mode);
        editor.start_enforcing();
        type_password(editor, word);
    }

    /// Ticks boxes of the Formatting Restrictions dialog and enforces it,
    /// leaving the editing half alone.
    fn formatting_boxes(editor: &mut Editor, boxes: &[(usize, bool)]) {
        editor.run(Command::RestrictEditing);
        editor.open_formatting_limits();
        let dialog = editor.dialog.as_mut().expect("the dialog");
        for (row, on) in boxes.iter().copied() {
            if let Some(Field::Check { on: value, .. }) = dialog.fields.get_mut(row) {
                *value = on;
            }
        }
        editor.finish_dialog(Answer::Accept);
        editor.start_enforcing();
        type_password(editor, "");
    }

    /// Fills both boxes of whatever password dialog is open and answers it.
    ///
    /// Both by shape rather than by number, so that a box moving does not
    /// quietly leave one of them empty and make the test about something else.
    fn type_password(editor: &mut Editor, word: &str) {
        let dialog = editor.dialog.as_mut().expect("the password dialog");
        for field in &mut dialog.fields {
            if let Field::Secret { value, .. } = field {
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
        editor.stop_enforcing();
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
        editor.choose_restrict_mode(0);
        editor.start_enforcing();

        let dialog = editor.dialog.as_mut().expect("the password dialog");
        let mut boxes = dialog.fields.iter_mut().filter_map(|field| match field {
            Field::Secret { value, .. } => Some(value),
            _ => None,
        });
        *boxes.next().expect("the first box") = "one".to_owned();
        *boxes.next().expect("the second box") = "another".to_owned();
        editor.finish_dialog(Answer::Accept);

        assert_eq!(editor.document.protection(), None, "protected on a mistyped password");
        assert!(editor.status.contains("not the same"), "{}", editor.status);

        // Asked again with both boxes empty, so that a person retypes rather
        // than correcting one of two things they cannot read.
        let dialog = editor.dialog.as_ref().expect("the dialog was not asked again");
        for field in &dialog.fields {
            if let Field::Secret { value, .. } = field {
                assert!(value.is_empty(), "a password was left in the box");
            }
        }
    }

    #[test]
    fn a_password_is_not_shown_while_it_is_typed() {
        let mut editor = editor();
        editor.run(Command::RestrictEditing);
        editor.choose_restrict_mode(0);
        editor.start_enforcing();
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
        editor.open_formatting_limits();
        let styles = editor.lockable_styles();
        let dialog = editor.dialog.as_mut().expect("the dialog");
        if let Some(Field::Check { on, .. }) = dialog.fields.get_mut(LIMIT) {
            *on = true;
        }
        if let Some(Field::Check { on, .. }) = dialog.fields.get_mut(THEME) {
            *on = theme;
        }
        if let Some(Field::Tree { rows, .. }) = dialog.fields.get_mut(STYLES) {
            for (row, (id, _)) in rows.iter_mut().zip(&styles) {
                row.tick = Some(allowed.iter().any(|wanted| wanted.eq_ignore_ascii_case(id)));
            }
        }
        editor.finish_dialog(Answer::Accept);

        // The editing half is left alone, which is the case worth testing: a
        // document anybody may type in and nobody may format. Enforcing it is
        // what writes the two boxes down.
        editor.start_enforcing();
        type_password(editor, "");
    }

    #[test]
    fn the_three_buttons_tick_what_they_name() {
        // The difference between ticking three boxes and ticking three
        // hundred. Two of them are a loop; the third is the interesting one.
        let mut editor = editor();
        editor.run(Command::RestrictEditing);
        editor.open_formatting_limits();

        let all = editor.lockable_styles().len();
        assert!(all > 3, "a document with three styles proves nothing");

        for (button, wanted) in
            [(NONE_OF_THEM, Some(0)), (ALL_OF_THEM, Some(all)), (RECOMMENDED, None)]
        {
            let dialog = editor.dialog.clone().expect("the dialog");
            editor.formatting_button(&dialog, button);
            let dialog = editor.dialog.as_ref().expect("it stayed up");
            let ticked =
                dialog.tree_rows(STYLES).iter().filter(|row| row.tick == Some(true)).count();
            match wanted {
                Some(exactly) => assert_eq!(ticked, exactly, "{button} ticked the wrong number"),
                // Recommended ticks what the document recommends: as many as
                // it marks, never more than it has, and never nothing —
                // a button that ticked nothing would be None under another
                // name. How many that is, is the document's business, and in
                // one whose every style is recommended it is all of them.
                None => {
                    assert_eq!(ticked, editor.recommended_styles().len());
                    assert!(ticked > 0, "{button} ticked nothing");
                    assert!(ticked <= all, "{button} ticked more styles than there are");
                }
            }
        }
    }

    #[test]
    fn the_recommended_set_is_the_documents_own_answer() {
        // Out of what each style carries, not a list invented here: a
        // document made from somebody's template is for whatever that
        // template says it is for.
        let editor = editor();
        let recommended = editor.recommended_styles();
        assert!(!recommended.is_empty(), "nothing was recommended at all");

        // Every one of them is a style the document has or mentions.
        let known = editor.lockable_styles();
        for id in &recommended {
            assert!(
                known.iter().any(|(had, _)| had.eq_ignore_ascii_case(id)),
                "{id} is recommended and is not a style this document has"
            );
        }

        // And the ones the document marks are the ones that come back.
        let marked: Vec<String> = editor
            .document
            .styles()
            .all()
            .iter()
            .filter(|style| style.recommended)
            .map(|style| style.id.clone())
            .collect();
        if !marked.is_empty() {
            for id in &marked {
                assert!(recommended.contains(id), "{id} is marked and was not recommended");
            }
        }
    }

    #[test]
    fn pressing_all_turns_the_limit_off_and_the_others_turn_it_on() {
        // Ticking every style is not a limit, and saying it is would leave a
        // document claiming a restriction that restricts nothing. Picking a
        // few is a limit, and leaving the box above unticked would throw the
        // answer away.
        let mut editor = editor();
        editor.run(Command::RestrictEditing);
        editor.open_formatting_limits();

        let dialog = editor.dialog.clone().expect("the dialog");
        editor.formatting_button(&dialog, ALL_OF_THEM);
        assert!(!editor.dialog.as_ref().expect("up").ticked(LIMIT), "every style is a limit");

        let dialog = editor.dialog.clone().expect("the dialog");
        editor.formatting_button(&dialog, RECOMMENDED);
        assert!(editor.dialog.as_ref().expect("up").ticked(LIMIT), "a few styles is not a limit");
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
    fn enforcing_nothing_says_so_rather_than_asking_for_a_password() {
        // Word greys its Start Enforcing button out until one of the two
        // boxes is answered; this says why instead, because a button that
        // does nothing and will not say why is worse.
        let mut editor = editor();
        editor.run(Command::RestrictEditing);
        editor.start_enforcing();

        assert!(editor.dialog.is_none(), "it asked for a password with nothing to protect");
        assert!(editor.document.protection_rules().is_none(), "nothing was asked and it was set");
        assert!(editor.status.contains("Nothing is restricted"), "{}", editor.status);
    }

    #[test]
    fn the_ticks_against_the_styles_are_not_lost_to_a_mistyped_password() {
        // They cannot be any more: which styles are allowed is written down
        // when its own dialog is answered, and the password is asked for
        // afterwards by a dialog that has no ticks in it to lose.
        let mut editor = editor();
        limit_formatting(&mut editor, &["Title"], false);
        assert!(editor.document.style_is_locked("Heading1"), "the ticks were not written");

        editor.start_enforcing();
        let dialog = editor.dialog.as_mut().expect("the password dialog");
        let mut boxes = dialog.fields.iter_mut().filter_map(|field| match field {
            Field::Secret { value, .. } => Some(value),
            _ => None,
        });
        *boxes.next().expect("the first box") = "one".to_owned();
        *boxes.next().expect("the second box") = "another".to_owned();
        editor.finish_dialog(Answer::Accept);

        assert!(editor.status.contains("not the same"), "{}", editor.status);
        assert!(editor.document.style_is_locked("Heading1"), "the ticks went with the password");
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
        formatting_boxes(&mut editor, &[(LIMIT, true), (AUTO_FORMAT, true)]);
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
        formatting_boxes(&mut editor, &[(LIMIT, true), (STYLE_SET, true)]);

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
