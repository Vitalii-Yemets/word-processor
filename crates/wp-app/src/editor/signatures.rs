//! The Signatures pane, and what signing from it does.
//!
//! The pane itself is [`crate::chrome::signaturepane`]; where a certificate
//! comes from is [`super::certificates`]; the signature is [`wp_sign`]. What
//! is here is the three questions the pane can ask and what each of them
//! signs.
//!
//! # The three things a person can sign
//!
//! **The document**, which is Word's Add a Digital Signature: a signature
//! over every part of the package, about the whole of it.
//!
//! **A signature line**, which is somebody's request answered. The line
//! carries an identifier — see [`wp_docx::signature`] — and the signature
//! records it, so that the two can be paired afterwards by anybody reading
//! the file. A request with nothing against it is what is left to do, and
//! that pairing is the whole reason a signature line is worth having.
//!
//! **Somebody else's signature**, which is a countersignature: not a second
//! opinion about the document but a statement about that signature — a
//! witness, an approval. What it covers is the first signature's value, so it
//! cannot be lifted off and put on another.
//!
//! # Why signing saves first
//!
//! A signature is over what is in the file. Signing a document with unsaved
//! changes would put a signature over something nobody has seen, so the
//! document is written first and what is signed is what is on the disk. Word
//! does the same.

use wp_shell::Response;

use crate::chrome::dialog::{Dialog, Field};
use crate::chrome::signaturepane::{Hit, Made, Shown, Wanted, WIDTH};

use super::certificates::{own_certificates, Own};
use super::dialogs::Asking;
use super::Editor;

/// Where each answer sits in the dialog that asks what to sign with.
const CERTIFICATE: usize = 1;
const PURPOSE: usize = 2;

/// What a signature about to be made is about.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) enum SignFor {
    /// The document at large.
    #[default]
    Document,
    /// A signature line, by the identifier it carries.
    Line(String),
    /// And somebody else's signature, by the part that holds it.
    Counter(String),
}

impl Editor {
    /// How much room the pane takes, which is none when it is shut.
    pub(super) fn signature_pane_width(&self) -> f32 {
        if self.show_signatures {
            WIDTH
        } else {
            0.0
        }
    }

    /// Where its left edge is.
    pub(super) fn signature_pane_left(&self) -> f32 {
        self.view_width as f32 - WIDTH
    }

    /// Whether a point is inside it at all.
    pub(super) fn over_signature_pane(&self, x: i32) -> bool {
        let x = crate::chrome::mirror::flip(x);
        self.show_signatures && (x as f32) >= self.signature_pane_left()
    }

    /// Word's Signatures: opens the pane, or shuts it again.
    pub(super) fn open_signatures(&mut self) -> Response {
        if self.show_signatures {
            self.show_signatures = false;
            self.clamp_scroll();
            self.needs_redraw = true;
            return Response::Redraw;
        }
        // Three panes want the same strip of window, and two of them at once
        // is one of them hidden behind the other.
        self.show_styles = false;
        self.show_restrict = false;
        self.show_mapping = false;
        self.show_text_pane = false;
        self.show_translator = false;
        self.show_signatures = true;
        self.clamp_scroll();
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Everything the pane draws, worked out afresh from the document.
    pub(super) fn signature_pane_shown(&self) -> Shown {
        let signatures = self.document.signatures();
        let lines = self.document.signature_lines();

        let made: Vec<Made> = signatures
            .iter()
            .map(|signature| Made {
                who: signature.certificate.subject.clone(),
                standing: self.said_of(signature),
                good: signature.standing.is_good(),
                reason: signature.reason.clone(),
                at: signature.signed_at.clone(),
                // The line it was made for, named by whoever is on that line
                // rather than by the identifier: an identifier is for pairing
                // and a name is for reading.
                line: lines
                    .iter()
                    .find(|line| line.id == signature.line)
                    .map(|line| line.name.clone())
                    .unwrap_or_default(),
                counters: signature
                    .counters
                    .iter()
                    .map(|counter| {
                        if counter.role.trim().is_empty() {
                            counter.certificate.subject.clone()
                        } else {
                            format!("{} ({})", counter.certificate.subject, counter.role)
                        }
                    })
                    .collect(),
            })
            .collect();

        // A line with a signature against it is answered, and what the second
        // half of the pane is for is what is still outstanding.
        let wanted: Vec<Wanted> = lines
            .iter()
            .filter(|line| !signatures.iter().any(|signature| signature.line == line.id))
            .map(|line| Wanted { name: line.name.clone(), title: line.title.clone() })
            .collect();

        Shown {
            made,
            wanted,
            can_sign: !own_certificates().is_empty(),
            saved: self.file.is_some(),
            folder: super::certificates::own_folder()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
        }
    }

    /// A press inside the pane.
    pub(super) fn signature_pane_press(&mut self, x: i32, y: i32) -> Response {
        let Some(hit) = self.signature_pane.at(x, y) else { return Response::Ignored };
        self.signature_pane_do(hit)
    }

    /// And what landing on one of its parts means.
    pub(super) fn signature_pane_do(&mut self, hit: Hit) -> Response {
        match hit {
            Hit::Close => self.open_signatures(),
            Hit::SignDocument => self.ask_to_sign(SignFor::Document),
            Hit::Made(index) => {
                // To the line it was made for, where it was made for one:
                // a signature is about a place in the document, and taking a
                // person there is more use than telling them where it is.
                let shown = self.signature_pane_shown();
                let Some(made) = shown.made.get(index) else { return Response::Ignored };
                let name = made.line.clone();
                if name.is_empty() {
                    return Response::Ignored;
                }
                self.go_to_line_named(&name)
            }
            Hit::Countersign(index) => {
                let signatures = self.document.signatures();
                let Some(signature) = signatures.get(index) else { return Response::Ignored };
                let part = signature.part.clone();
                self.ask_to_sign(SignFor::Counter(part))
            }
            Hit::Wanted(index) => {
                let Some(line) = self.wanted_lines().get(index).cloned() else {
                    return Response::Ignored;
                };
                self.go_to_line_named(&line.name)
            }
            Hit::Sign(index) => {
                let Some(line) = self.wanted_lines().get(index).cloned() else {
                    return Response::Ignored;
                };
                self.ask_to_sign(SignFor::Line(line.id))
            }
        }
    }

    /// The lines still waiting, in the order the pane lists them.
    fn wanted_lines(&self) -> Vec<wp_docx::signature::Line> {
        let signatures = self.document.signatures();
        self.document
            .signature_lines()
            .into_iter()
            .filter(|line| !signatures.iter().any(|signature| signature.line == line.id))
            .collect()
    }

    /// Takes the person to the line somebody is named on.
    fn go_to_line_named(&mut self, name: &str) -> Response {
        let Some(line) = self.document.signature_lines().into_iter().find(|line| line.name == name)
        else {
            return self.report(crate::messages::t("That line is no longer in this document"));
        };
        self.document.set_caret(line.at);
        self.reveal_caret();
        self.needs_redraw = true;
        Response::Redraw
    }

    /// The pointer moving over the pane.
    pub(super) fn signature_pane_hover(&mut self, x: i32, y: i32) -> bool {
        self.signature_pane.hover(x, y)
    }

    /// Two clicks on a signature line ask to sign it, which is Word's own
    /// shortcut and the thing a person tries first.
    pub(super) fn sign_line_at(&mut self, x: i32, y: i32) -> Option<Response> {
        let at = self.position_at(x, y)?;
        let line = self.line_at(at)?;
        Some(self.ask_to_sign(SignFor::Line(line.id)))
    }

    /// Which signature line a place in the document is inside, if any.
    ///
    /// Apart from the press that asks, because where a line is is a question
    /// about the document and where a click landed is a question about the
    /// window, and only one of the two is worth a test.
    pub(super) fn line_at(&self, at: wp_docx::TextPosition) -> Option<wp_docx::signature::Line> {
        self.document.signature_lines().into_iter().find(|line| {
            // The mark covers the whole of the line: the space above it, the
            // cross, the name and the title.
            self.document
                .bookmark(&format!("{}{}", wp_docx::signature::LINE_MARK, line.id))
                .is_some_and(|mark| {
                    mark.range.0.paragraph <= at.paragraph && at.paragraph <= mark.range.1.paragraph
                })
        })
    }

    /// Asks what to sign with, and why.
    pub(super) fn ask_to_sign(&mut self, target: SignFor) -> Response {
        if self.file.is_none() {
            return self.report(crate::messages::t("Save the document before signing it."));
        }
        let mine = own_certificates();
        if mine.is_empty() {
            return self.report(crate::messages::t(
                "There is no certificate on this machine to sign with.",
            ));
        }

        let said = match &target {
            SignFor::Document => crate::messages::t("This signs the whole document.").to_owned(),
            SignFor::Line(id) => {
                let name =
                    self.document.signature_line(id).map(|line| line.name).unwrap_or_default();
                crate::messages::with("This signs the line of {0}.", &[&name])
            }
            SignFor::Counter(part) => {
                let who = self
                    .document
                    .signatures()
                    .into_iter()
                    .find(|signature| &signature.part == part)
                    .map(|signature| signature.certificate.subject)
                    .unwrap_or_default();
                crate::messages::with("This signs the signature of {0}.", &[&who])
            }
        };

        self.signing_for = target;
        // What a countersigner gives is the capacity they signed in, which is
        // a different question from why somebody signed their own document —
        // and the box asks the question it means.
        let purpose = match &self.signing_for {
            SignFor::Counter(_) => crate::messages::t("Signed as"),
            _ => crate::messages::t("Purpose"),
        };
        let dialog = Dialog::new(
            "Sign",
            vec![
                Field::note(&said),
                Field::Choice {
                    label: "Certificate".to_owned(),
                    items: mine.iter().map(Own::label).collect(),
                    current: 0,
                },
                Field::Text { label: purpose.to_owned(), value: String::new() },
            ],
        )
        .wide(620.0);
        self.ask(Asking::Signatures, dialog)
    }

    /// Signs with whichever certificate was chosen.
    pub(super) fn apply_signature(&mut self, dialog: &Dialog) -> Response {
        let mine = own_certificates();
        let Some(own) = mine.get(dialog.chose(CERTIFICATE)) else { return Response::Ignored };
        let why = dialog.said(PURPOSE);
        let Some(path) = self.file.clone() else {
            return self.report(crate::messages::t("Save the document before signing it."));
        };

        let signer = wp_sign::Signer {
            certificate: own.certificate.der.clone(),
            chain: super::certificates::chain_for(&own.certificate, &mine),
            key: own.signs(),
            reason: why,
            at: super::files::timestamp(),
            line: match &self.signing_for {
                SignFor::Line(id) => id.clone(),
                _ => String::new(),
            },
        };

        let through_the_system = own.is_from_the_system();
        let made = match self.signing_for.clone() {
            SignFor::Counter(part) => self.document.countersign_saved(&part, &signer),
            _ => self.document.save_signed(&signer).map_err(|error| error.to_string()),
        };
        let bytes = match made {
            Ok(bytes) => bytes,
            Err(error) => {
                if through_the_system {
                    return self.report(crate::messages::t(
                        "This machine would not sign with that certificate",
                    ));
                }
                return self.report(&format!("Cannot sign: {error}"));
            }
        };
        if let Err(error) = std::fs::write(&path, bytes) {
            return self.report(&format!("Cannot write {}: {error}", path.display()));
        }

        // What is on disk is signed; what is open has to be the same thing,
        // or the next save would take the signature off something the person
        // never edited.
        if let Ok(written) = std::fs::read(&path) {
            if let Ok(document) = wp_docx::Document::open(&written) {
                self.set_document(document, Some(path.clone()));
            }
        }
        self.needs_redraw = true;
        let who = own.certificate.subject.clone();
        let said = match &self.signing_for {
            SignFor::Counter(_) => crate::messages::with("Countersigned by {0}", &[&who]),
            _ => crate::messages::with("Signed by {0}", &[&who]),
        };
        self.report(&said)
    }

    /// Draws the pane down the right-hand side of the window.
    pub(super) fn draw_signature_pane(&mut self) {
        if !self.show_signatures {
            return;
        }
        let shown = self.signature_pane_shown();
        let left = self.signature_pane_left();
        let top = self.ribbon_bottom();
        let bottom = self.window_bottom();
        let theme = self.theme;

        let mut pane = core::mem::take(&mut self.signature_pane);
        pane.draw(
            &mut self.canvas,
            &mut self.chrome_engine,
            &mut self.renderer,
            &shown,
            left,
            top,
            bottom,
            &theme,
        );
        self.signature_pane = pane;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::certificates::tests::folder;
    use crate::editor::certificates::{own_certificates_in, From};
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::{Document, TextPosition};
    use wp_shell::{App, Event};

    fn editor() -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("Yours faithfully,")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let library: &'static wp_layout::FontLibrary =
            Box::leak(Box::new(wp_layout::FontLibrary::scan_system()));
        let mut editor = Editor::new(library, document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor
    }

    /// A document with a line for somebody to sign on.
    fn with_a_line(editor: &mut Editor, id: &str) {
        editor.document.set_caret(TextPosition::new(0, 0));
        editor.document.insert_signature_line(&wp_docx::signature::Signer {
            name: String::from("Ada Lovelace"),
            title: String::from("Director"),
            id: id.to_owned(),
        });
        editor.relayout();
    }

    #[test]
    fn the_button_opens_a_pane_and_not_a_dialog() {
        let mut editor = editor();
        editor.open_signatures();
        assert!(editor.show_signatures, "no pane");
        assert!(editor.dialog.is_none(), "it opened a dialog over the document");
        assert!(editor.signature_pane_width() > 0.0);

        editor.open_signatures();
        assert!(!editor.show_signatures, "it would not shut again");
    }

    #[test]
    fn a_line_nobody_has_signed_is_what_the_document_is_waiting_for() {
        let mut editor = editor();
        with_a_line(&mut editor, "9a3f");

        let shown = editor.signature_pane_shown();
        assert!(shown.made.is_empty(), "nothing has signed it");
        assert_eq!(shown.wanted.len(), 1, "{:?}", shown.wanted);
        assert_eq!(shown.wanted[0].name, "Ada Lovelace");
        assert_eq!(shown.wanted[0].title, "Director");
    }

    #[test]
    fn a_signature_made_for_a_line_answers_it_and_stops_it_being_wanted() {
        // The pairing is the whole reason a signature line is worth having:
        // a request with nothing against it is what is left to do, and one
        // that has been answered is not.
        let folder = folder("pane-pairing", "der");
        let mine = own_certificates_in(&folder.0);
        let own = mine.first().expect("a certificate");

        let mut editor = editor();
        with_a_line(&mut editor, "9a3f");

        let signer = wp_sign::Signer {
            certificate: own.certificate.der.clone(),
            chain: Vec::new(),
            key: own.signs(),
            reason: String::from("Approved"),
            at: String::from("2027-01-01T00:00:00Z"),
            line: String::from("9a3f"),
        };
        let bytes = editor.document.save_signed(&signer).expect("signing");
        let signed = Document::open(&bytes).expect("reopening");
        editor.set_document(signed, None);

        let shown = editor.signature_pane_shown();
        assert_eq!(shown.made.len(), 1, "the signature was not read back");
        assert_eq!(shown.made[0].line, "Ada Lovelace", "it does not say which line it answered");
        assert_eq!(shown.made[0].reason, "Approved");
        assert!(shown.wanted.is_empty(), "the line is still being asked for: {:?}", shown.wanted);
    }

    #[test]
    fn a_signature_about_the_document_answers_no_line() {
        let folder = folder("pane-plain", "der");
        let mine = own_certificates_in(&folder.0);
        let own = mine.first().expect("a certificate");

        let mut editor = editor();
        with_a_line(&mut editor, "9a3f");

        let signer = wp_sign::Signer {
            certificate: own.certificate.der.clone(),
            chain: Vec::new(),
            key: own.signs(),
            reason: String::from("Because it is mine"),
            at: String::from("2027-01-01T00:00:00Z"),
            line: String::new(),
        };
        let bytes = editor.document.save_signed(&signer).expect("signing");
        editor.set_document(Document::open(&bytes).expect("reopening"), None);

        let shown = editor.signature_pane_shown();
        assert_eq!(shown.made.len(), 1);
        assert!(shown.made[0].line.is_empty(), "it claimed a line it was not made for");
        assert_eq!(shown.wanted.len(), 1, "the line stopped being asked for anyway");
    }

    #[test]
    fn an_unsaved_document_is_not_signed_and_says_why() {
        // A signature over something nobody has seen is not one. The pane
        // says so rather than offering a button that cannot work.
        let mut editor = editor();
        let shown = editor.signature_pane_shown();
        assert!(!shown.saved);

        editor.signature_pane_do(crate::chrome::signaturepane::Hit::SignDocument);
        assert!(editor.dialog.is_none(), "it asked what to sign with");
        assert!(editor.status.contains("Save the document"), "{}", editor.status);
    }

    #[test]
    fn two_clicks_on_a_line_ask_to_sign_that_line() {
        // Word.s own shortcut, and the thing a person tries first. What is
        // held here is the finding: a place inside the marked stretch is
        // inside that line, and a place outside it is not.
        let mut editor = editor();
        with_a_line(&mut editor, "9a3f");

        let line = editor.document.signature_line("9a3f").expect("the line");
        let inside = wp_docx::TextPosition::new(line.at.paragraph + 2, 0);
        assert_eq!(editor.line_at(inside).map(|found| found.id), Some(String::from("9a3f")));
        assert_eq!(editor.line_at(wp_docx::TextPosition::new(0, 0)), None, "the text above it");

        // And with nothing to sign with, being asked says so rather than
        // opening a dialog whose list would be empty.
        editor.file = Some(std::path::PathBuf::from("signed.docx"));
        editor.ask_to_sign(SignFor::Line(String::from("9a3f")));
        if own_certificates().is_empty() {
            assert!(editor.dialog.is_none(), "an empty list was offered");
            assert!(editor.status.contains("no certificate"), "{}", editor.status);
        } else {
            assert_eq!(editor.signing_for, SignFor::Line(String::from("9a3f")));
        }
    }

    #[test]
    fn a_certificate_from_the_folder_is_not_one_from_the_system() {
        let folder = folder("pane-source", "der");
        let mine = own_certificates_in(&folder.0);
        let own = mine.first().expect("a certificate");
        assert!(!own.is_from_the_system());
        assert!(matches!(own.from, From::Folder { .. }));
    }
}
