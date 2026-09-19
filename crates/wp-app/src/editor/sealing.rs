//! The password that makes a document unreadable rather than uneditable.
//!
//! # Two passwords, and why they are not the same feature
//!
//! [`super::protection`] has one: it stops a person lifting a restriction,
//! and the text of the document is in the file for anybody who cares to
//! unzip it. This one is the other kind. Without it there is no text in the
//! file at all — only a compound file holding one enciphered stream — and no
//! program can read it, including this one. See [`wp_crypt`].
//!
//! # What this module is
//!
//! Three moments. Opening a file that turns out to be encrypted, which means
//! stopping and asking rather than showing an error about a broken zip.
//! Saving a document that was opened that way, which means writing it back
//! encrypted without being asked. And putting a password on a document that
//! has none, or taking one off, which is Word's Encrypt with Password on the
//! File page.
//!
//! # Where the unguessable bytes come from
//!
//! The machine, through `wp_shell::random`, fresh on every save. Two saves
//! of the same document under the same password come out as different bytes,
//! which is what a salt is for. A machine that will not give them is a
//! machine that cannot save an encrypted document, and the program says so
//! rather than writing one with a salt it made up.

use std::path::{Path, PathBuf};

use wp_docx::Document;
use wp_shell::Response;

use crate::chrome::dialog::{Dialog, Field};

use super::dialogs::Asking;
use super::Editor;

/// The two names the encryption dialog goes under, named rather than
/// written where they are used so that the catalogue of what this program
/// can say finds them.
const ENCRYPT: &str = "Encrypt Document";
const CHANGE: &str = "Change Password";

/// Where each answer sits in the two dialogs.
const PASSWORD: usize = 2;
const AGAIN: usize = 3;
/// And in the one that asks for the password of a file being opened.
const ANSWER: usize = 1;

/// How many unguessable bytes sealing a document takes.
const FRESH: usize = 144;

/// A file that turned out to be encrypted, waiting for its password.
#[derive(Clone, Debug)]
pub(super) struct Waiting {
    pub path: PathBuf,
    pub bytes: Vec<u8>,
}

impl Editor {
    /// Asks for the password of a file that cannot be opened without one.
    pub(super) fn ask_to_unseal(&mut self, path: &Path, bytes: Vec<u8>) -> Response {
        let name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default().to_owned();
        self.waiting_to_unseal = Some(Waiting { path: path.to_path_buf(), bytes });
        let dialog = Dialog::new(
            "Password",
            vec![
                Field::note(&crate::messages::with("{0} is encrypted.", &[&name])),
                Field::Secret { label: "Password".to_owned(), value: String::new() },
            ],
        );
        self.ask(Asking::Unseal, dialog)
    }

    /// Opens it, if that was the password.
    pub(super) fn apply_unseal(&mut self, dialog: &Dialog) -> Response {
        let Some(waiting) = self.waiting_to_unseal.clone() else { return Response::Ignored };
        match Document::open_sealed(&waiting.bytes, &dialog.said(ANSWER)) {
            Ok(document) => {
                self.waiting_to_unseal = None;
                self.set_document(document, Some(waiting.path.clone()));
                self.remember_recent(&waiting.path);
                self.raise(super::autoevents::Moment::Opened);
                self.status =
                    crate::messages::with("Opened {0}", &[&waiting.path.display().to_string()]);
                Response::Redraw
            }
            Err(error) => {
                // Asked again rather than given up on: a mistyped password is
                // the commonest thing that happens at this dialog, and Word
                // asks again too.
                self.status = error.to_string();
                let again = self.unseal_dialog_again(&waiting.path);
                self.ask(Asking::Unseal, again)
            }
        }
    }

    fn unseal_dialog_again(&self, path: &Path) -> Dialog {
        let name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default().to_owned();
        Dialog::new(
            "Password",
            vec![
                Field::note(&crate::messages::with(
                    "{0} did not open. Try the password again.",
                    &[&name],
                )),
                Field::Secret { label: "Password".to_owned(), value: String::new() },
            ],
        )
    }

    /// Gives up on a file whose password was not given.
    pub(super) fn cancel_unseal(&mut self) {
        self.waiting_to_unseal = None;
        self.status = String::from("Not opened");
    }

    /// Word's Encrypt with Password, off the File page.
    pub(super) fn open_encryption(&mut self) -> Response {
        let already = self.document.password().is_some();
        let dialog = Dialog::new(
            if already { CHANGE } else { ENCRYPT },
            vec![
                if already {
                    Field::note("An empty box takes the encryption off.")
                } else {
                    Field::note("The document is unreadable without it.")
                },
                // Word's own warning, and the thing about this feature that a
                // person most needs to be told before they use it.
                Field::note("A password that is lost cannot be recovered."),
                Field::Secret { label: "Password".to_owned(), value: String::new() },
                Field::Secret {
                    label: "Reenter password to confirm".to_owned(),
                    value: String::new(),
                },
            ],
        );
        self.ask(Asking::Encrypt, dialog)
    }

    /// Puts the password on, if the two boxes agree.
    pub(super) fn apply_encryption(&mut self, dialog: &Dialog) -> Response {
        let word = dialog.said(PASSWORD);
        if word != dialog.said(AGAIN) {
            self.status = "The two passwords are not the same".to_owned();
            return self.open_encryption();
        }
        let wanted = (!word.is_empty()).then_some(word);
        let changed = self.document.set_password(wanted.as_deref());
        self.update_title();
        self.needs_redraw = true;
        self.edited(
            changed,
            if wanted.is_some() {
                "The document will be encrypted when it is saved"
            } else {
                "The encryption will come off when it is saved"
            },
        )
    }

    /// The bytes to write to a file: the package, encrypted if the document
    /// has a password.
    ///
    /// `None` where the document wants encrypting and the machine would not
    /// give the bytes that takes. Writing it unencrypted instead would be the
    /// worst thing this program could do with a document somebody had asked
    /// to be unreadable.
    pub(super) fn bytes_to_write(&self) -> Option<Result<Vec<u8>, wp_docx::Error>> {
        if self.document.password().is_none() {
            return Some(self.document.save());
        }
        let bytes = wp_shell::random::bytes::<FRESH>()?;
        Some(self.document.save_sealed(&wp_crypt::Fresh::from_bytes(&bytes)))
    }
}

/// What the Info page says about the signatures a document carries.
///
/// A line that is pressed to be told more, because that is what every other
/// line on that page is. Word's own page says the same thing in the same
/// place: a document with a signature on it says so before it says anything
/// else about itself.
pub(super) fn signature_row(document: &Document) -> crate::chrome::backstage::Row {
    use crate::chrome::backstage::Row;
    let signatures = document.signatures();
    if signatures.is_empty() {
        return Row::new("Digital Signatures", "This document is not signed");
    }
    let bad = signatures.iter().filter(|signature| !signature.standing.is_good()).count();
    let note = if bad == 0 {
        let names: Vec<&str> =
            signatures.iter().map(|signature| signature.certificate.subject.as_str()).collect();
        format!("Signed by {}", names.join(", "))
    } else {
        format!("{bad} of {} signatures do not hold", signatures.len())
    };
    Row::new("Digital Signatures", note)
}

impl Editor {
    /// Word's Signatures: what the document carries and what it can be
    /// signed with.
    ///
    /// A dialog now rather than a line in the strip along the bottom, because
    /// there is something to answer: the certificates a person has are a list
    /// to pick from. See [`super::certificates`].
    pub(super) fn report_signatures(&mut self) -> Response {
        self.open_signatures()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chrome::dialog::Answer;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event};

    const SECRET: &str = "The quick brown fox";

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    fn editor() -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text(SECRET)));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor
    }

    fn typed(editor: &mut Editor, rows: &[usize], word: &str) {
        let dialog = editor.dialog.as_mut().expect("a dialog");
        for row in rows {
            if let Some(Field::Secret { value, .. }) = dialog.fields.get_mut(*row) {
                *value = word.to_owned();
            }
        }
    }

    #[test]
    fn a_password_put_on_makes_the_saved_bytes_unreadable() {
        let mut editor = editor();
        editor.open_encryption();
        typed(&mut editor, &[PASSWORD, AGAIN], "Fenchurch");
        editor.finish_dialog(Answer::Accept);

        assert_eq!(editor.document.password(), Some("Fenchurch"));
        let bytes =
            editor.bytes_to_write().expect("the machine gives random bytes").expect("saving");
        assert!(wp_docx::sealing::is_sealed(&bytes));
        assert!(
            !bytes.windows(SECRET.len()).any(|window| window == SECRET.as_bytes()),
            "the text is in the file in plain sight"
        );
        assert_eq!(
            Document::open_sealed(&bytes, "Fenchurch").expect("opening").plain_text(),
            SECRET
        );
    }

    #[test]
    fn two_passwords_that_differ_encrypt_nothing_and_ask_again() {
        let mut editor = editor();
        editor.open_encryption();
        typed(&mut editor, &[PASSWORD], "one");
        typed(&mut editor, &[AGAIN], "another");
        editor.finish_dialog(Answer::Accept);

        assert_eq!(editor.document.password(), None);
        assert!(editor.status.contains("not the same"), "{}", editor.status);
        assert!(editor.dialog.is_some(), "the dialog was not asked again");
    }

    #[test]
    fn an_encrypted_file_asks_for_its_password_and_opens_on_the_right_one() {
        let sealed = {
            let mut editor = editor();
            editor.open_encryption();
            typed(&mut editor, &[PASSWORD, AGAIN], "Fenchurch");
            editor.finish_dialog(Answer::Accept);
            editor.bytes_to_write().expect("random bytes").expect("saving")
        };

        let mut editor = editor();
        editor.ask_to_unseal(Path::new("/tmp/sealed.docx"), sealed);
        assert!(editor.dialog.is_some(), "it did not ask");

        typed(&mut editor, &[ANSWER], "fenchurch");
        editor.finish_dialog(Answer::Accept);
        assert!(editor.status.contains("not the password"), "{}", editor.status);
        assert!(editor.dialog.is_some(), "it did not ask again");

        typed(&mut editor, &[ANSWER], "Fenchurch");
        editor.finish_dialog(Answer::Accept);
        assert!(editor.dialog.is_none(), "it is still asking");
        assert_eq!(editor.document.plain_text(), SECRET);
        assert_eq!(editor.document.password(), Some("Fenchurch"), "it did not stay encrypted");
    }

    #[test]
    fn a_document_with_no_password_is_written_as_a_plain_package() {
        let editor = editor();
        let bytes = editor.bytes_to_write().expect("no random bytes needed").expect("saving");
        assert!(!wp_docx::sealing::is_sealed(&bytes));
    }
}
