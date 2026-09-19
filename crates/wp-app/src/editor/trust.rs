//! Whether a macro may run at all.
//!
//! # The one gate
//!
//! A document can carry a program. That is the whole difficulty: opening a
//! file somebody sent is not supposed to be a decision, and a file that
//! carries a program makes it one. Word's answer — and this one — is that
//! nothing a document carries runs until somebody says so, and that there is
//! exactly one place where "somebody said so" is decided.
//!
//! Every way of running a macro asks [`Editor::macros_allowed`] first: the
//! Run button on the macro dialog, F5 and F8 in the Visual Basic editor, and
//! a line typed into the Immediate window. Nothing runs when a document is
//! opened, and there is no other route in. When the events a document can
//! raise arrive — `AutoOpen`, `Document_Open` — they will ask the same
//! question, because there is only one to ask.
//!
//! # What the answer is made of
//!
//! Four settings, which are Word's four:
//!
//! - **Nothing**: disabled, and nothing said about it.
//! - **Asking**: disabled, with a bar across the top offering to enable them
//!   for this document. Word's own default, and this one.
//! - **Signed**: only macros in a document signed by somebody trusted.
//! - **Everything**: anything runs, which is the setting nobody should have.
//!
//! And two lists that say yes before the setting is asked: the folders whose
//! documents are trusted, and the people whose signatures are.
//!
//! # Where this differs from Word, and why it is said out loud
//!
//! Word checks the signature **on the project**: a macro project is signed
//! separately from the document, and a trusted publisher's project runs
//! inside a document nobody signed. This program checks the signature on the
//! **document** — see [`super::signature`] — because that is the signature it
//! can read and verify. The difference matters and is written down rather
//! than hidden: a project signed the way Word signs one is a project this
//! program does not yet know how to check, and it says so rather than
//! trusting it.

use wp_shell::Response;

use crate::chrome::dialog::Dialog;
use crate::messages::{t, with};

use super::Editor;

/// What the trust centre has been told about macros.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Trusting {
    /// Disabled, and nothing said.
    Nothing,
    /// Disabled, with the bar that offers to enable them. Word's default.
    #[default]
    Asking,
    /// Only the ones in a document somebody trusted has signed.
    Signed,
    /// Anything at all.
    Everything,
}

impl Trusting {
    /// All four, in the order Word lists them.
    pub const ALL: [Self; 4] = [Self::Nothing, Self::Asking, Self::Signed, Self::Everything];

    /// What the setting is called in the file this program remembers things
    /// in, which is not what it is called on the screen: one is written down
    /// and the other is read by a person, and a translated setting file would
    /// be a file the next version could not read.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Nothing => "none",
            Self::Asking => "ask",
            Self::Signed => "signed",
            Self::Everything => "all",
        }
    }

    /// And back again, falling back on asking — which is the safe answer and
    /// the one Word starts from.
    #[must_use]
    pub fn named(name: &str) -> Self {
        Self::ALL.into_iter().find(|one| one.name() == name).unwrap_or(Self::Asking)
    }

    /// What it is called on the screen, in Word's own words.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Nothing => "Disable all macros without notification",
            Self::Asking => "Disable all macros with notification",
            Self::Signed => "Disable all macros except digitally signed macros",
            Self::Everything => "Enable all macros (not recommended)",
        }
    }
}

/// Whether a macro may run, and why not when it may not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Allowed {
    /// It may.
    Yes,
    /// It may not, and this is what to say about it.
    No(String),
}

impl Editor {
    /// What the trust centre has been told.
    #[must_use]
    pub(super) fn trusting(&self) -> Trusting {
        self.settings.macro_trust.as_deref().map_or(Trusting::default(), Trusting::named)
    }

    /// Whether this document's macros may run, and why not if they may not.
    ///
    /// The one gate. Everything that runs a macro asks this first, and
    /// nothing runs without asking.
    #[must_use]
    pub(super) fn macros_allowed(&self) -> Allowed {
        if self.enabled_here {
            return Allowed::Yes;
        }
        if self.in_a_trusted_place() {
            return Allowed::Yes;
        }
        match self.trusting() {
            Trusting::Everything => Allowed::Yes,
            Trusting::Signed => {
                if self.signed_by_somebody_trusted() {
                    return Allowed::Yes;
                }
                Allowed::No(
                    t("Only macros signed by somebody trusted may run. The Trust Centre is where that is set.")
                        .to_owned(),
                )
            }
            Trusting::Asking => Allowed::No(
                t("Macros are disabled. The bar across the top offers to enable them for this document.")
                    .to_owned(),
            ),
            Trusting::Nothing => Allowed::No(
                t("Macros are disabled without notification. The Trust Centre is where that is changed.")
                    .to_owned(),
            ),
        }
    }

    /// Whether the bar offering to enable them belongs at the top.
    ///
    /// Only where there is something to offer: a document whose macros are
    /// already allowed has nothing to enable, and one whose macros are
    /// disabled without notification is not to be notified.
    #[must_use]
    pub(super) fn should_offer_macros(&self) -> bool {
        self.carries_macros
            && self.trusting() == Trusting::Asking
            && matches!(self.macros_allowed(), Allowed::No(_))
    }

    /// Whether the document is in a folder somebody trusted.
    #[must_use]
    fn in_a_trusted_place(&self) -> bool {
        let Some(path) = self.file.as_ref().and_then(|path| path.parent()) else { return false };
        self.settings.trusted_places.iter().any(|place| {
            let place = std::path::Path::new(place);
            // A folder trusts what is under it as well as what is in it,
            // which is what Word's own "subfolders" tick does.
            path.starts_with(place)
        })
    }

    /// Whether the document is signed by somebody in the trusted list, and
    /// whether that signature still holds.
    #[must_use]
    fn signed_by_somebody_trusted(&self) -> bool {
        self.document.signatures().iter().any(|signature| {
            signature.standing == wp_sign::Standing::Good
                && self
                    .settings
                    .trusted_publishers
                    .iter()
                    .any(|publisher| publisher == &signature.certificate.subject)
        })
    }

    /// Enables them for this document, which is what the bar's button does.
    ///
    /// For this document and for as long as it is open, which is what Word's
    /// Enable Content means: nothing is written down, and opening the same
    /// file tomorrow asks again.
    pub(super) fn enable_macros_here(&mut self) -> Response {
        self.enabled_here = true;
        self.info_bar = None;
        self.needs_redraw = true;
        self.report(t("Macros are enabled for this document, for as long as it is open"))
    }

    /// Trusts the folder the document is in, from the Trust Centre.
    pub(super) fn trust_this_folder(&mut self, dialog: &Dialog) -> Response {
        let Some(folder) = self.file.as_ref().and_then(|path| path.parent()) else {
            return self
                .report(t("This document has never been saved, so it is not in a folder yet"));
        };
        let written = folder.to_string_lossy().into_owned();
        if !self.settings.trusted_places.contains(&written) {
            self.settings.trusted_places.push(written.clone());
            self.settings.save();
        }
        let again = self.options_dialog();
        let _ = dialog;
        self.ask(super::dialogs::Asking::Options, again);
        self.report(&with("{0} is trusted", &[&written]))
    }

    /// And forgets the one that is chosen.
    pub(super) fn forget_trusted_place(&mut self, dialog: &Dialog) -> Response {
        let chosen = dialog.chose_row(super::optionsdialog::TRUSTED_PLACES);
        if chosen < self.settings.trusted_places.len() {
            let gone = self.settings.trusted_places.remove(chosen);
            self.settings.save();
            let again = self.options_dialog();
            self.ask(super::dialogs::Asking::Options, again);
            return self.report(&with("{0} is not trusted any more", &[&gone]));
        }
        Response::Ignored
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::chrome::infobar::{Because, Hit};
    use wp_docx::kinds::Kind;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::Document;
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event};

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    /// A document carrying the one macro that would run itself if anything
    /// did: Word's `AutoOpen`.
    fn with_auto_open() -> Document {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("untouched")));
        let mut document = Document::create(&body).expect("a document");
        document.set_kind(Kind::MacroEnabledDocument);
        let bytes = document.save().expect("saving");

        let mut package = wp_opc::Package::open(&bytes).expect("a package");
        package.add_part(
            "word/vbaProject.bin",
            "application/vnd.ms-office.vbaProject",
            wp_vba::example(&[(
                "Module1",
                "Public Sub AutoOpen()\r\n    Selection.TypeText \"ran \"\r\nEnd Sub\r\n",
            )]),
        );
        let mut relationships = package.relationships("word/document.xml").expect("relationships");
        relationships.add(
            "http://schemas.microsoft.com/office/2006/relationships/vbaProject",
            "vbaProject.bin",
            wp_opc::TargetMode::Internal,
        );
        package.set_relationships(&relationships).expect("writing them");
        Document::open(&package.save().expect("saving the package")).expect("reopening")
    }

    /// An editor that opened that document, the way a person opens one.
    fn opened(file: Option<std::path::PathBuf>) -> Editor {
        let blank = Document::create(&Body::default()).expect("a blank document");
        let mut editor = Editor::new(library(), blank, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor.set_document(with_auto_open(), file);
        editor.draw(1400, 900);
        editor
    }

    /// Runs the document's first macro from the list, as a person would, and
    /// gives back what the status line said.
    fn tries_to_run(editor: &mut Editor) -> String {
        editor.set_view_option("tab=view").expect("the View tab");
        editor.draw(1400, 900);
        editor.run(crate::chrome::Command::Macros);
        let popup = editor.popup.as_ref().expect("the list");
        let at = (0..8)
            .find(|at| popup.item(*at).is_some_and(|line| line.contains("AutoOpen")))
            .expect("the macro on the list");
        editor.choose_macro(at);
        editor.finish_dialog(crate::chrome::dialog::Answer::Named(super::super::macros::RUN));
        editor.status.clone()
    }

    #[test]
    fn nothing_runs_when_a_document_is_opened() {
        // The whole of Word's answer to a document that carries a program,
        // and the test this item is named after: an AutoOpen does nothing at
        // all until somebody says so.
        let editor = opened(None);
        assert_eq!(editor.document.plain_text().trim(), "untouched");
        assert!(matches!(editor.macros_allowed(), Allowed::No(_)));
    }

    #[test]
    fn by_default_the_bar_offers_to_enable_them_and_only_the_bar_does() {
        let mut editor = opened(None);
        assert_eq!(editor.trusting(), Trusting::Asking, "Word's default is to ask");
        assert_eq!(
            editor.info_bar.as_ref().map(|bar| bar.because),
            Some(Because::Macros),
            "the bar is not there to ask with"
        );

        // Trying to run one is refused, and the refusal says where to go.
        let said = tries_to_run(&mut editor);
        assert!(said.contains("disabled"), "{said}");
        assert_eq!(editor.document.plain_text().trim(), "untouched", "it ran anyway");

        // The bar's button is Enable Content, and pressing it is the one
        // thing that turns them on.
        editor.draw(1400, 900);
        let bar = editor.info_bar.as_ref().expect("the bar");
        let (x, y) = bar.button_middle().expect("the button was drawn");
        assert_eq!(bar.at(x, y), Some(Hit::Button));
        editor.press_info_bar(x, y);

        assert_eq!(editor.macros_allowed(), Allowed::Yes);
        assert!(editor.info_bar.is_none(), "the bar stayed up after it was answered");
        let said = tries_to_run(&mut editor);
        assert!(said.contains("ran"), "{said}");
        assert!(
            editor.document.plain_text().starts_with("ran "),
            "{}",
            editor.document.plain_text()
        );
    }

    #[test]
    fn enabling_is_for_this_document_and_for_as_long_as_it_is_open() {
        let mut editor = opened(None);
        editor.enable_macros_here();
        assert_eq!(editor.macros_allowed(), Allowed::Yes);

        // Another document is asked about again, which is what "for this
        // document" means.
        editor.set_document(with_auto_open(), None);
        assert!(matches!(editor.macros_allowed(), Allowed::No(_)));
        assert!(editor.info_bar.is_some(), "the bar did not come back for the next document");
    }

    #[test]
    fn a_document_in_a_trusted_folder_is_not_asked() {
        let folder = std::path::PathBuf::from("/home/somebody/Trusted");
        let mut editor = opened(Some(folder.join("deep").join("report.docm")));
        assert!(matches!(editor.macros_allowed(), Allowed::No(_)));

        // Trusting the folder trusts what is under it as well.
        editor.settings.trusted_places.push(folder.to_string_lossy().into_owned());
        assert_eq!(editor.macros_allowed(), Allowed::Yes);
        editor.set_document(with_auto_open(), Some(folder.join("other.docm")));
        assert!(editor.info_bar.is_none(), "a trusted document was asked about");
    }

    #[test]
    fn disabled_without_notification_shows_no_bar_and_says_why_when_asked() {
        let mut editor = opened(None);
        editor.settings.macro_trust = Some(Trusting::Nothing.name().to_owned());
        editor.set_document(with_auto_open(), None);
        assert!(editor.info_bar.is_none(), "it was notified");

        let said = tries_to_run(&mut editor);
        assert!(said.contains("without notification"), "{said}");
        assert_eq!(editor.document.plain_text().trim(), "untouched");
    }

    #[test]
    fn enabling_everything_asks_nothing_which_is_why_nobody_should() {
        let mut editor = opened(None);
        editor.settings.macro_trust = Some(Trusting::Everything.name().to_owned());
        editor.set_document(with_auto_open(), None);
        assert!(editor.info_bar.is_none());
        assert_eq!(editor.macros_allowed(), Allowed::Yes);
    }

    #[test]
    fn only_signed_means_signed_by_somebody_on_the_list_and_still_holding() {
        // The one rule that leans on the certificates J12 and J24 read: a
        // signature that holds, by a name somebody put on the list.
        let folder = crate::editor::certificates::tests::folder("trust-signed", "der");
        let mine = crate::editor::certificates::own_certificates_in(&folder.0);
        let own = mine.first().expect("a certificate");
        let signer = wp_sign::Signer {
            certificate: own.certificate.der.clone(),
            chain: Vec::new(),
            key: own.signs(),
            reason: String::from("Approved"),
            at: String::from("2027-01-01T00:00:00Z"),
            line: String::new(),
        };
        let signed = Document::open(&with_auto_open().save_signed(&signer).expect("signing"))
            .expect("reopening");

        let mut editor = opened(None);
        editor.settings.macro_trust = Some(Trusting::Signed.name().to_owned());
        editor.set_document(signed, None);
        assert!(matches!(editor.macros_allowed(), Allowed::No(_)), "nobody has trusted the signer");

        let subject = editor.document.signatures()[0].certificate.subject.clone();
        editor.settings.trusted_publishers.push(subject);
        assert_eq!(editor.macros_allowed(), Allowed::Yes);

        // And an unsigned document is still refused, however trusted the
        // signer is.
        editor.set_document(with_auto_open(), None);
        assert!(matches!(editor.macros_allowed(), Allowed::No(_)));
    }

    #[test]
    fn every_way_of_running_one_goes_through_the_same_gate() {
        // The Run button is one way; F5 in the editor and a line in the
        // Immediate window are the other two, and all three are refused by
        // the same sentence.
        let mut editor = opened(None);
        editor.open_basic();
        editor.handle(Event::KeyDown {
            key: wp_shell::Key::Function(5),
            modifiers: wp_shell::Modifiers::default(),
        });
        assert!(editor.status.contains("disabled"), "{}", editor.status);
        assert!(editor.debugger.is_none(), "F5 started a macro anyway");

        if let Some(pane) = &mut editor.basic {
            pane.in_immediate = true;
        }
        for letter in "?1 + 1".chars() {
            editor.handle(Event::Char(letter));
        }
        editor.handle(Event::KeyDown {
            key: wp_shell::Key::Enter,
            modifiers: wp_shell::Modifiers::default(),
        });
        let answers = editor.basic.as_ref().expect("the editor").answers.clone();
        assert!(answers.iter().any(|line| line.contains("disabled")), "{answers:?}");
        assert!(
            !answers.iter().any(|line| line == "2"),
            "the Immediate window ran it: {answers:?}"
        );
    }

    #[test]
    fn the_setting_is_remembered_by_a_name_that_is_not_the_one_on_the_screen() {
        for one in Trusting::ALL {
            assert_eq!(Trusting::named(one.name()), one);
        }
        // And a name nobody wrote falls back on asking, which is the safe one.
        assert_eq!(Trusting::named("something else"), Trusting::Asking);
    }
}
