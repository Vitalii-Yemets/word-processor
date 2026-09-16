//! The document that was opened read-only, and the password that opens it for
//! writing.
//!
//! # What this is, and which of the three passwords it is not
//!
//! A `.docx` can ask to be opened read-only, and can put a password behind
//! that asking — Word's Save As ▸ Tools ▸ General Options, and the Always Open
//! Read-Only on its Info page. See [`wp_docx::readonly`], where the three
//! passwords are told apart.
//!
//! The question it answers is not "what may be done to this text" — that is
//! [`super::protection`] — but "does this file open for writing at all". So it
//! is answered once, when the file is opened, and the answer is a property of
//! this window rather than of the document: a document opened read-only is
//! read-only until it is opened again.
//!
//! # What it is worth
//!
//! Nothing at all against a program that ignores it, and this program could
//! ignore it. What it is worth is the master copy that does not quietly become
//! somebody's draft: a person who was sent a document and told not to change
//! it is a person who will change it by habit, and one who is asked at the
//! door will not.

use std::path::Path;

use wp_docx::readonly::WriteProtection;
use wp_shell::Response;

use crate::chrome::dialog::{Dialog, Field};

use super::dialogs::Asking;
use super::Editor;

/// Where each answer sits in the dialog that asks at the door.
const ANSWER: usize = 2;
/// And in the one that sets it.
const RECOMMEND: usize = 3;
const PASSWORD: usize = 4;
const AGAIN: usize = 5;

/// How many bytes of salt, asked of the format rather than decided here.
const SALT: usize = wp_docx::protection::SALT_BYTES;

impl Editor {
    /// What to do about a document that has just been opened and asks not to
    /// be written.
    ///
    /// Says whether it asked anything.
    pub(super) fn asked_at_the_door(&mut self, path: &Path) -> Option<Response> {
        let asked = self.document.write_protection()?;
        let name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default().to_owned();
        self.opened_read_only = true;

        let mut fields = vec![
            Field::note(&crate::messages::with("{0} is reserved.", &[&name])),
            Field::note("The author asked for it to be opened read-only."),
        ];
        if asked.password.is_some() {
            fields.push(Field::Secret {
                label: "Password to modify".to_owned(),
                value: String::new(),
            });
        } else {
            // No password: the recommendation is a request, and a request is
            // one a person can decline.
            fields.push(Field::Check { label: "Open read-only".to_owned(), on: true });
        }
        Some(self.ask(Asking::OpenReadOnly, Dialog::new("Read-Only Recommended", fields)))
    }

    /// What the answer at the door means.
    pub(super) fn apply_open_read_only(&mut self, dialog: &Dialog) -> Response {
        let Some(asked) = self.document.write_protection() else { return Response::Redraw };
        if asked.password.is_none() {
            // The tick box: ticked means the request was granted.
            self.opened_read_only = dialog.ticked(ANSWER);
            self.update_title();
            return self.report(if self.opened_read_only {
                "Opened read-only"
            } else {
                "Opened for writing"
            });
        }

        if asked.opens_with(&dialog.said(ANSWER)) {
            return self.open_for_writing();
        }
        // Not asked again: the way past a wrong password here is to go on
        // reading, which is what the document offered in the first place.
        // Word's dialog has Read Only beside its OK for the same reason.
        self.opened_read_only = true;
        self.update_title();
        self.report("That is not the password: opened read-only")
    }

    /// Cancelled at the door, which means the document opens read-only.
    pub(super) fn cancel_open_read_only(&mut self) {
        self.opened_read_only = true;
        self.update_title();
        self.status = crate::messages::t("Opened read-only").to_owned();
    }

    /// Whether the document open now was opened read-only.
    #[must_use]
    pub(super) fn is_read_only(&self) -> bool {
        self.opened_read_only
    }

    /// Takes the read-only off, which needs the password if there is one.
    fn open_for_writing(&mut self) -> Response {
        self.opened_read_only = false;
        self.update_title();
        self.report("Opened for writing")
    }

    /// Word's Always Open Read-Only, and the password behind it: the line on
    /// the File page that sets what the other half of this module asks about.
    pub(super) fn open_read_only_settings(&mut self) -> Response {
        // A document already read-only in this window offers the way out
        // instead, which is the thing a person pressing that line wants.
        if self.opened_read_only {
            let asked = self.document.write_protection().unwrap_or_default();
            if asked.password.is_none() {
                return self.open_for_writing();
            }
            let dialog = Dialog::new(
                "Read-Only Recommended",
                vec![
                    Field::note("This document is open read-only."),
                    Field::note("The author asked for it to be opened read-only."),
                    Field::Secret { label: "Password to modify".to_owned(), value: String::new() },
                ],
            );
            return self.ask(Asking::OpenReadOnly, dialog);
        }

        let asked = self.document.write_protection().unwrap_or_default();
        let dialog = Dialog::new(
            "General Options",
            vec![
                Field::note("The document is not encrypted."),
                Field::note("Anybody who can open the file can take this off."),
                Field::Heading("File sharing".to_owned()),
                Field::Check { label: "Always open read-only".to_owned(), on: asked.recommended },
                Field::Secret {
                    label: "Password to modify (optional)".to_owned(),
                    value: String::new(),
                },
                Field::Secret {
                    label: "Reenter password to confirm".to_owned(),
                    value: String::new(),
                },
            ],
        );
        self.ask(Asking::ReadOnlySettings, dialog)
    }

    /// Writes what that dialog said into the document.
    pub(super) fn apply_read_only_settings(&mut self, dialog: &Dialog) -> Response {
        let word = dialog.said(PASSWORD);
        if word != dialog.said(AGAIN) {
            let mut again = dialog.clone();
            for row in [PASSWORD, AGAIN] {
                if let Some(Field::Secret { value, .. }) = again.fields.get_mut(row) {
                    value.clear();
                }
            }
            self.status = crate::messages::t("The two passwords are not the same").to_owned();
            return self.ask(Asking::ReadOnlySettings, again);
        }

        let recommended = dialog.ticked(RECOMMEND);
        let had = self.document.write_protection().unwrap_or_default();
        let wanted = if word.is_empty() {
            // An empty box takes a password off, which is how every password
            // box in this program says "none"; the recommendation stands or
            // falls by its own tick.
            WriteProtection { recommended, password: None }
        } else {
            let Some(salt) = wp_shell::random::bytes::<SALT>() else {
                return self
                    .report("This machine would not give the random bytes a password needs");
            };
            WriteProtection { recommended, ..WriteProtection::behind(&word, &salt) }
        };

        let changed = self.document.set_write_protection(Some(&wanted));
        let said = match (&wanted.password, wanted.recommended) {
            (Some(_), _) => "A password is needed to open this for writing",
            (None, true) => "This document will ask to be opened read-only",
            (None, false) if had.asks_anything() => "This document opens for writing",
            (None, false) => "Nothing was asked for",
        };
        self.needs_redraw = true;
        self.edited(changed, said)
    }

    /// What the File page says on that line.
    pub(super) fn read_only_note(&self) -> &'static str {
        if self.opened_read_only {
            return "This document is open read-only. Open it for writing";
        }
        match self.document.write_protection() {
            Some(asked) if asked.password.is_some() => {
                "A password is needed to open this for writing. Change it, or take it off"
            }
            Some(_) => "This document asks to be opened read-only",
            None => "Ask for this document to be opened read-only",
        }
    }

    /// Says why nothing happened to a document that was opened read-only.
    pub(super) fn refuse_read_only(&mut self) -> Response {
        self.report(
            "This document was opened read-only — File ▸ Always Open Read-Only lets it be written",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chrome::dialog::Answer;
    use crate::chrome::Command;
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

    /// An editor holding a document that asks not to be written, standing at
    /// the door with the question up.
    fn at_the_door(asked: WriteProtection) -> Editor {
        let mut editor = editor();
        editor.document.set_write_protection(Some(&asked));
        let opened = editor.asked_at_the_door(Path::new("reserved.docx"));
        assert!(opened.is_some(), "the document asked nothing");
        assert!(editor.dialog.is_some(), "nothing was asked at the door");
        editor
    }

    /// Types one letter and says whether it arrived.
    fn typing_arrives(editor: &mut Editor) -> bool {
        let before = editor.document.plain_text();
        editor.handle(Event::Char('x'));
        editor.document.plain_text() != before
    }

    #[test]
    fn a_document_that_asks_nothing_is_not_asked_about() {
        let mut editor = editor();
        assert!(editor.asked_at_the_door(Path::new("plain.docx")).is_none());
        assert!(!editor.is_read_only());
        assert!(editor.dialog.is_none());
    }

    #[test]
    fn a_recommendation_is_honoured_and_can_be_declined() {
        let mut editor = at_the_door(WriteProtection::recommended());
        editor.finish_dialog(Answer::Accept);
        assert!(editor.is_read_only(), "the recommendation was not honoured");
        assert!(!typing_arrives(&mut editor), "a read-only document took typing");
        assert!(editor.status.contains("read-only"), "{}", editor.status);

        // And declined, which is what makes it a request.
        let mut editor = at_the_door(WriteProtection::recommended());
        if let Some(Field::Check { on, .. }) =
            editor.dialog.as_mut().expect("the dialog").fields.get_mut(ANSWER)
        {
            *on = false;
        }
        editor.finish_dialog(Answer::Accept);
        assert!(!editor.is_read_only());
        assert!(typing_arrives(&mut editor), "a document opened for writing refused typing");
    }

    #[test]
    fn cancelling_at_the_door_opens_it_read_only() {
        let mut editor = at_the_door(WriteProtection::recommended());
        editor.finish_dialog(Answer::Cancel);
        assert!(editor.is_read_only(), "cancelling let it be written");
    }

    #[test]
    fn the_password_opens_it_for_writing_and_a_wrong_one_does_not() {
        let salt = b"0123456789abcdef";
        let mut editor = at_the_door(WriteProtection::behind("Fenchurch", salt));
        if let Some(Field::Secret { value, .. }) =
            editor.dialog.as_mut().expect("the dialog").fields.get_mut(ANSWER)
        {
            *value = "open sesame".to_owned();
        }
        editor.finish_dialog(Answer::Accept);
        assert!(editor.is_read_only(), "a wrong password opened it for writing");
        assert!(editor.status.contains("not the password"), "{}", editor.status);

        let mut editor = at_the_door(WriteProtection::behind("Fenchurch", salt));
        if let Some(Field::Secret { value, .. }) =
            editor.dialog.as_mut().expect("the dialog").fields.get_mut(ANSWER)
        {
            *value = "Fenchurch".to_owned();
        }
        editor.finish_dialog(Answer::Accept);
        assert!(!editor.is_read_only(), "the right password did not open it");
        assert!(typing_arrives(&mut editor));
    }

    #[test]
    fn the_ribbon_and_the_caption_both_say_so() {
        let mut editor = at_the_door(WriteProtection::recommended());
        editor.finish_dialog(Answer::Accept);

        assert!(editor.title.contains("(Read-Only)"), "the caption does not say: {}", editor.title);
        let state = editor.toolbar_state();
        assert!(
            !crate::chrome::is_enabled(Command::Format(wp_docx::CharacterFormat::Bold), &state),
            "the ribbon still offers to change a document that cannot be changed"
        );
        // And the reason given is this one rather than the restriction's.
        editor.run(Command::Format(wp_docx::CharacterFormat::Bold));
        assert!(editor.status.contains("opened read-only"), "{}", editor.status);
    }

    #[test]
    fn the_file_page_sets_it_and_the_two_passwords_must_agree() {
        let mut editor = editor();
        editor.open_read_only_settings();
        let dialog = editor.dialog.as_mut().expect("the dialog");
        if let Some(Field::Check { on, .. }) = dialog.fields.get_mut(RECOMMEND) {
            *on = true;
        }
        if let Some(Field::Secret { value, .. }) = dialog.fields.get_mut(PASSWORD) {
            *value = "one".to_owned();
        }
        if let Some(Field::Secret { value, .. }) = dialog.fields.get_mut(AGAIN) {
            *value = "another".to_owned();
        }
        editor.finish_dialog(Answer::Accept);
        assert_eq!(editor.document.write_protection(), None, "a mistyped password was written");
        assert!(editor.status.contains("not the same"), "{}", editor.status);
        assert!(editor.dialog.is_some(), "the dialog was not asked again");

        // The same answer typed the same way twice.
        let dialog = editor.dialog.as_mut().expect("asked again");
        assert!(
            matches!(dialog.fields.get(RECOMMEND), Some(Field::Check { on: true, .. })),
            "the tick was lost"
        );
        for row in [PASSWORD, AGAIN] {
            if let Some(Field::Secret { value, .. }) = dialog.fields.get_mut(row) {
                *value = "Fenchurch".to_owned();
            }
        }
        editor.finish_dialog(Answer::Accept);

        let asked = editor.document.write_protection().expect("nothing was written");
        assert!(asked.recommended);
        assert!(asked.opens_with("Fenchurch"));
    }

    #[test]
    fn the_file_page_is_the_way_back_out_as_well() {
        let mut editor = at_the_door(WriteProtection::recommended());
        editor.finish_dialog(Answer::Accept);
        assert!(editor.is_read_only());

        // With no password, the line itself opens it for writing.
        editor.open_read_only_settings();
        assert!(!editor.is_read_only(), "the way out did not open it");
        assert!(typing_arrives(&mut editor));
    }

    #[test]
    fn the_line_on_the_file_page_says_which_of_the_four_things_is_true() {
        let mut editor = editor();
        assert!(editor.read_only_note().starts_with("Ask for"));

        editor.document.set_write_protection(Some(&WriteProtection::recommended()));
        assert!(editor.read_only_note().contains("asks to be opened read-only"));

        editor
            .document
            .set_write_protection(Some(&WriteProtection::behind("word", b"0123456789abcdef")));
        assert!(editor.read_only_note().starts_with("A password is needed"));

        editor.opened_read_only = true;
        assert!(editor.read_only_note().contains("is open read-only"));
    }
}
