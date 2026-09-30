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
//! Run button on the macro dialog, F5 and F8 in the Visual Basic editor, a
//! line typed into the Immediate window, and the moments a document runs its
//! own — `AutoOpen`, `Document_Open` and the rest, see [`super::autoevents`]
//! — which ask the same question, because there is only one to ask. There
//! is no other route in.
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
//! documents are trusted, and the people whose signatures are — each of them
//! a certificate, never a name, see [`Publisher`].
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

/// Where the macros a document runs are read from: the part is taken by this
/// name, see [`super::macros`].
const MACRO_PROJECT: &str = "word/vbaProject.bin";

/// And what the format reaches them by, from the main part.
const MACRO_PROJECT_RELATIONSHIP: &str =
    "http://schemas.microsoft.com/office/2006/relationships/vbaProject";

/// Whether a signature covers the macros a document would run.
///
/// A signature that holds says that what it covers is as it was, and
/// nothing about anything beside it. A macro project added to a signed
/// document — the part, the relationship that reaches it and the content
/// type that makes the document a macro-enabled one — changes nothing the
/// signature covers, so the signature still holds and says nothing about
/// the project. So the question asked is the one Word asks of a project's
/// own signature: whether it covers the project. The part the program runs,
/// by its name, and whatever the main part's macro-project relationships
/// reach, with those relationships.
fn covers_the_macros(signature: &wp_sign::Signature, document: &wp_docx::Document) -> bool {
    let package = document.package();
    let Ok(main) = package.main_document_part() else { return false };
    (package.part(MACRO_PROJECT).is_none() || signature.covers_part(MACRO_PROJECT))
        && signature.covers_reached(package, &main, MACRO_PROJECT_RELATIONSHIP)
}

/// Somebody whose signature lets a document's macros run: Word's trusted
/// publisher.
///
/// # What decides, and what is only shown
///
/// Their certificate, by its fingerprint — see [`wp_sign::fingerprint`] —
/// and nothing else. A signature that holds proves that it was made with the
/// key of the certificate beside it, and the name on that certificate is
/// whatever its maker chose to write: anybody can make a key and a
/// certificate reading `CN=Somebody Trusted`. So the name, the issuer and the
/// date are kept only so that the Trust Center can say who this is, in the
/// three columns Word's Trusted Publishers page has, and never to decide.
/// Word keeps the certificate itself, in the system's Trusted Publishers
/// store, and finds it there by its thumbprint; the fingerprint is the same
/// idea with a stronger hash.
///
/// # A name alone
///
/// An earlier version remembered a publisher by the name on the certificate
/// and nothing more. Such a line is still read, because it is the person's
/// record of whom they meant to trust, and it stays on the list marked as not
/// verified, where it can be seen and removed — but it trusts nobody.
/// Dropping it quietly would leave a person wondering why a document that ran
/// yesterday is refused today; trusting the publisher again from a document
/// they signed puts the certificate in its place.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Publisher {
    /// The SHA-256 of their certificate, in lower-case hex. Empty for a name
    /// kept by an earlier version.
    pub fingerprint: String,
    /// Who the certificate says it is about: Word's Issued To.
    pub subject: String,
    /// Who says so: Word's Issued By.
    pub issuer: String,
    /// When it stops being good, as `YYYY-MM-DDTHH:MM:SSZ`: Word's
    /// Expiration Date.
    pub expires: String,
}

impl Publisher {
    /// The publisher a certificate is, taken from the certificate in hand.
    #[must_use]
    pub fn of(certificate: &wp_asn1::Certificate) -> Self {
        // The words are the certificate maker's, and they are going into a
        // file of one setting per line: a line break written on a certificate
        // would otherwise be a setting of its maker's choosing.
        let shown = |text: &str| -> String {
            text.chars().map(|at| if at.is_control() { ' ' } else { at }).collect()
        };
        Self {
            fingerprint: wp_sign::fingerprint(certificate),
            subject: shown(&certificate.subject),
            issuer: shown(&certificate.issuer),
            expires: shown(&certificate.not_after),
        }
    }

    /// A name kept by an earlier version, with nothing to match it by.
    #[must_use]
    pub fn named_only(subject: &str) -> Self {
        Self { subject: subject.to_owned(), ..Self::default() }
    }

    /// Whether there is a certificate behind it at all.
    #[must_use]
    pub fn is_verified(&self) -> bool {
        self.fingerprint.len() == 64
            && self.fingerprint.chars().all(|digit| digit.is_ascii_hexdigit())
    }

    /// Whether a certificate is this publisher's: the same certificate, byte
    /// for byte, whatever name either of them bears.
    #[must_use]
    pub fn is(&self, certificate: &wp_asn1::Certificate) -> bool {
        self.is_verified() && self.fingerprint == wp_sign::fingerprint(certificate)
    }

    /// What the Trust Center says beside the name: who issued it and until
    /// when, as Word's list does — or that it is a name alone.
    ///
    /// The issuer by its common name, which is what Word's Issued By column
    /// shows: the whole of an issuer's name is a line of its own, and the
    /// date after it would be cut off the end of the column.
    #[must_use]
    pub fn said(&self) -> String {
        if !self.is_verified() {
            return t("Not verified: a name alone, which trusts nobody").to_owned();
        }
        let until = self.expires.get(..10).unwrap_or(&self.expires);
        with("{0} (expires {1})", &[common_name(&self.issuer), until])
    }
}

/// The common name in a name as a certificate prints it, or the whole name
/// where it has none.
fn common_name(name: &str) -> &str {
    name.split(", ").find_map(|part| part.strip_prefix("CN=")).unwrap_or(name)
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
        self.allowed_for(self.file.as_deref(), &self.document)
    }

    /// The same question about a file that is not the one open: the
    /// template a new document was made from, whose macros are its own.
    ///
    /// Enable Content does not reach here, because it was pressed for the
    /// document and not for the template.
    #[must_use]
    pub(super) fn allowed_for(
        &self,
        file: Option<&std::path::Path>,
        document: &wp_docx::Document,
    ) -> Allowed {
        if self.in_a_trusted_place(file) {
            return Allowed::Yes;
        }
        match self.trusting() {
            Trusting::Everything => Allowed::Yes,
            Trusting::Signed => {
                if self.signed_by_somebody_trusted(document) {
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

    /// Whether a file is in a folder somebody trusted.
    #[must_use]
    fn in_a_trusted_place(&self, file: Option<&std::path::Path>) -> bool {
        let Some(path) = file.and_then(|path| path.parent()) else { return false };
        self.settings.trusted_places.iter().any(|place| {
            let place = std::path::Path::new(place);
            // A folder trusts what is under it as well as what is in it,
            // which is what Word's own "subfolders" tick does.
            path.starts_with(place)
        })
    }

    /// Whether a document is signed by somebody in the trusted list, whether
    /// that signature still holds, and whether it covers the macros.
    ///
    /// By the certificate that made the signature, never by the name on it:
    /// that the signature holds says the certificate's key made it, and only
    /// the certificate being the one that was trusted says whose key that is.
    #[must_use]
    fn signed_by_somebody_trusted(&self, document: &wp_docx::Document) -> bool {
        document.signatures().iter().any(|signature| {
            signature.standing.is_good()
                && covers_the_macros(signature, document)
                && self
                    .settings
                    .trusted_publishers
                    .iter()
                    .any(|publisher| publisher.is(&signature.certificate))
        })
    }

    /// Trusts whoever holds the key of a certificate, and says whether they
    /// were not trusted already: Word's "Trust all documents from this
    /// publisher".
    ///
    /// Taken from the certificate in hand and from nothing a person could
    /// type, since what is trusted is that certificate.
    pub(super) fn trust_publisher(&mut self, certificate: &wp_asn1::Certificate) -> bool {
        let publishers = &mut self.settings.trusted_publishers;
        if publishers.iter().any(|publisher| publisher.is(certificate)) {
            return false;
        }
        let publisher = Publisher::of(certificate);
        // A name an earlier version kept alone is the person's note of whom
        // they meant, and the certificate they have now chosen takes its row
        // rather than sitting beside it. Which row it lands in is all the
        // name decides; the trust is in the certificate either way.
        match publishers
            .iter()
            .position(|one| !one.is_verified() && one.subject == publisher.subject)
        {
            Some(at) => publishers[at] = publisher,
            None => publishers.push(publisher),
        }
        true
    }

    /// Trusts whoever signed the open document, from the Trust Centre.
    ///
    /// Only a signature that holds offers its certificate: a certificate is
    /// public, and one pasted beside a signature it did not make is not a
    /// publisher of this document.
    pub(super) fn trust_this_publisher(&mut self, dialog: &Dialog) -> Response {
        let mut names: Vec<String> = Vec::new();
        let mut any = false;
        for signature in self.document.signatures() {
            if !signature.standing.is_good() {
                continue;
            }
            any = true;
            self.trust_publisher(&signature.certificate);
            if !names.contains(&signature.certificate.subject) {
                names.push(signature.certificate.subject.clone());
            }
        }
        if !any {
            return self.report(t(
                "This document carries no signature that holds, so there is nobody to trust",
            ));
        }
        self.settings.save();
        let again = self.options_dialog();
        let _ = dialog;
        self.ask(super::dialogs::Asking::Options, again);
        self.report(&with("{0} is trusted", &[&names.join("; ")]))
    }

    /// And forgets the publisher chosen on the list.
    pub(super) fn forget_trusted_publisher(&mut self, dialog: &Dialog) -> Response {
        let chosen = dialog.chose_pair(super::optionsdialog::TRUSTED_PUBLISHERS);
        if chosen < self.settings.trusted_publishers.len() {
            let gone = self.settings.trusted_publishers.remove(chosen);
            self.settings.save();
            let again = self.options_dialog();
            self.ask(super::dialogs::Asking::Options, again);
            return self.report(&with("{0} is not trusted any more", &[&gone.subject]));
        }
        Response::Ignored
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
        let response =
            self.report(t("Macros are enabled for this document, for as long as it is open"));
        // Which is when the document's opening macros run, as in Word: they
        // were held back at the door, and this is the door opening.
        self.raise(super::autoevents::Moment::Opened);
        response
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
        // The list is one of pairs, and which row of it is chosen is asked as
        // one: asked as a tree, it answered the first row whatever was chosen.
        let chosen = dialog.chose_pair(super::optionsdialog::TRUSTED_PLACES);
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
    use crate::editor::certificates::Own;
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
        // signature that holds, by a certificate somebody put on the list.
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

        let certificate = editor.document.signatures()[0].certificate.clone();
        assert!(editor.trust_publisher(&certificate));
        assert!(!editor.trust_publisher(&certificate), "the same certificate was trusted twice");
        assert_eq!(editor.macros_allowed(), Allowed::Yes);

        // And an unsigned document is still refused, however trusted the
        // signer is.
        editor.set_document(with_auto_open(), None);
        assert!(matches!(editor.macros_allowed(), Allowed::No(_)));
    }

    /// A certificate of the person's own, made afresh: each one has the
    /// same name and a key nobody else has.
    fn a_certificate(name: &str) -> (crate::editor::certificates::tests::Folder, Own) {
        let folder = crate::editor::certificates::tests::folder(name, "der");
        let own = crate::editor::certificates::own_certificates_in(&folder.0)
            .into_iter()
            .next()
            .expect("a certificate");
        (folder, own)
    }

    /// Somebody signing with one of them.
    fn signer_of(own: &Own) -> wp_sign::Signer {
        wp_sign::Signer {
            certificate: own.certificate.der.clone(),
            chain: Vec::new(),
            key: own.signs(),
            reason: String::from("Approved"),
            at: String::from("2027-01-01T00:00:00Z"),
            line: String::new(),
        }
    }

    /// The document carrying `AutoOpen`, signed with one of them.
    fn signed_by(own: &Own) -> Vec<u8> {
        with_auto_open().save_signed(&signer_of(own)).expect("signing")
    }

    #[test]
    fn a_macro_project_added_beside_a_trusted_signature_does_not_run() {
        // A trusted publisher's signed document with no macros in it, and a
        // project put in afterwards: the part, the relationship from the main
        // part that reaches it, and the content type that makes the document
        // a macro-enabled one. Nothing the signature covers has changed, so
        // it holds — and it says nothing whatever about the project.
        let (_folder, own) = a_certificate("trust-added-project");
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("untouched")));
        let plain = Document::create(&body).expect("a document");
        let signed = plain.save_signed(&signer_of(&own)).expect("signing");

        let mut package = wp_opc::Package::open(&signed).expect("a package");
        package.add_part(
            "word/vbaProject.bin",
            "application/vnd.ms-office.vbaProject",
            wp_vba::example(&[(
                "Module1",
                "Public Sub AutoOpen()\r\n    Selection.TypeText \"added \"\r\nEnd Sub\r\n",
            )]),
        );
        let mut relationships = package.relationships("word/document.xml").expect("relationships");
        relationships.add(
            "http://schemas.microsoft.com/office/2006/relationships/vbaProject",
            "vbaProject.bin",
            wp_opc::TargetMode::Internal,
        );
        package.set_relationships(&relationships).expect("writing them");
        let mut types = package.content_types().clone();
        types.set_override("word/document.xml", Kind::MacroEnabledDocument.content_type());
        package.set_content_types(types);
        let added = Document::open(&package.save().expect("saving")).expect("reopening");
        assert_eq!(added.kind(), Kind::MacroEnabledDocument);
        assert!(added.signatures()[0].standing.is_good(), "it holds for what it covers");

        let mut editor = opened(None);
        editor.settings.macro_trust = Some(Trusting::Signed.name().to_owned());
        editor.trust_publisher(&own.certificate);
        editor.set_document(added, None);
        assert!(
            matches!(editor.macros_allowed(), Allowed::No(_)),
            "a project nobody signed ran under a trusted publisher's signature"
        );

        // And the same publisher's macro-enabled document, signed with its
        // project in it, runs.
        editor.set_document(Document::open(&signed_by(&own)).expect("reopening"), None);
        assert_eq!(editor.macros_allowed(), Allowed::Yes);
    }

    #[test]
    fn the_trust_centers_buttons_fit_inside_the_dialog_in_english_and_in_german() {
        // Four buttons of the page's own, with OK and Cancel, is the widest
        // row any page of Options puts along the bottom, and German says each
        // of them at greater length. None may hang outside the frame, and the
        // page with its two lists has to fit the window.
        use crate::chrome::dialog::Part;
        for language in [crate::messages::ENGLISH, "de"] {
            crate::messages::tests::in_language(language, || {
                let mut editor = opened(None);
                editor.open_options();
                if let Some(dialog) = &mut editor.dialog {
                    dialog.show_tab(super::super::optionsdialog::TAB_TRUST_PAGE);
                }
                editor.draw(1400, 900);
                let dialog = editor.dialog.as_ref().expect("the dialog");
                let (left, top, width, height) = dialog.frame();
                assert!(top >= 0.0 && top + height <= 900.0, "{language}: taller than the window");
                let buttons: Vec<(f32, f32, f32, f32)> = dialog
                    .parts()
                    .into_iter()
                    .filter(|(part, _)| matches!(part, Part::Button(_)))
                    .map(|(_, place)| place)
                    .collect();
                assert_eq!(buttons.len(), 6, "{language}: {buttons:?}");
                for (x, y, across, down) in buttons {
                    assert!(
                        x >= left && x + across <= left + width,
                        "{language}: a button from {x} to {} is outside {left} to {}",
                        x + across,
                        left + width
                    );
                    assert!(y >= top && y + down <= top + height, "{language}: below the frame");
                }
            });
        }
    }

    #[test]
    fn remove_location_removes_the_folder_that_is_chosen() {
        use crate::chrome::dialog::{Answer, Field};
        let mut editor = opened(None);
        editor.settings.trusted_places =
            vec!["/one".to_owned(), "/two".to_owned(), "/three".to_owned()];
        editor.open_options();
        let list = editor
            .dialog
            .as_mut()
            .and_then(|dialog| dialog.fields.get_mut(super::super::optionsdialog::TRUSTED_PLACES));
        let Some(Field::Pairs { current, .. }) = list else { panic!("no list of folders") };
        *current = 1;
        editor.finish_dialog(Answer::Named(super::super::optionsdialog::FORGET_PLACE));
        assert_eq!(
            editor.settings.trusted_places,
            vec!["/one".to_owned(), "/three".to_owned()],
            "the folder removed was not the one chosen"
        );
        assert!(editor.status.contains("/two"), "{}", editor.status);
    }

    #[test]
    fn a_certificate_with_a_trusted_name_and_another_key_is_not_trusted() {
        // Anybody can make a key and write any name they like on a
        // certificate for it. What was trusted is one certificate, and a
        // stranger's with the same words in it is a stranger's.
        let (_genuine_folder, genuine) = a_certificate("trust-genuine");
        let (_impostor_folder, impostor) = a_certificate("trust-impostor");
        assert_eq!(genuine.certificate.subject, impostor.certificate.subject);
        assert_ne!(genuine.certificate.der, impostor.certificate.der);

        let mut editor = opened(None);
        editor.settings.macro_trust = Some(Trusting::Signed.name().to_owned());
        editor.trust_publisher(&genuine.certificate);

        let forged = Document::open(&signed_by(&impostor)).expect("reopening");
        assert!(forged.signatures()[0].standing.is_good(), "the impostor's own signature holds");
        editor.set_document(forged, None);
        assert!(
            matches!(editor.macros_allowed(), Allowed::No(_)),
            "a certificate was trusted for the name written on it"
        );

        editor.set_document(Document::open(&signed_by(&genuine)).expect("reopening"), None);
        assert_eq!(editor.macros_allowed(), Allowed::Yes, "the one that was trusted");
    }

    #[test]
    fn a_changed_macro_project_does_not_run_behind_an_unsigned_manifest() {
        // The whole of the attack on the gate: a trusted publisher's signed
        // document, its macros swapped for others, and an empty manifest
        // nobody signed put first in the signature to cover for them.
        let (_folder, own) = a_certificate("trust-decoy");
        let signed = signed_by(&own);
        let mut editor = opened(None);
        editor.settings.macro_trust = Some(Trusting::Signed.name().to_owned());
        editor.trust_publisher(&own.certificate);
        editor.set_document(Document::open(&signed).expect("reopening"), None);
        assert_eq!(editor.macros_allowed(), Allowed::Yes, "the untouched document");

        let mut package = wp_opc::Package::open(&signed).expect("a package");
        package.set_part(
            "word/vbaProject.bin",
            wp_vba::example(&[(
                "Module1",
                "Public Sub AutoOpen()\r\n    Selection.TypeText \"swapped \"\r\nEnd Sub\r\n",
            )]),
        );
        let part = "_xmlsignatures/sig1.xml";
        let text = package.xml_part(part).expect("the signature").expect("text");
        let decoy = text.replacen(
            r#"<Object Id="idPackageObject">"#,
            r#"<Object><Manifest></Manifest></Object><Object Id="idPackageObject">"#,
            1,
        );
        assert_ne!(decoy, text, "the decoy did not go in");
        package.set_part(part, decoy.into_bytes());
        let tampered = Document::open(&package.save().expect("saving")).expect("reopening");

        editor.set_document(tampered, None);
        assert!(
            matches!(editor.macros_allowed(), Allowed::No(_)),
            "a changed macro project ran under a signature that no longer covers it"
        );
    }

    /// The rows of the Trust Center's list of publishers, as the dialog
    /// standing now shows them.
    fn publishers_shown(editor: &Editor) -> Vec<(String, String)> {
        let dialog = editor.dialog.as_ref().expect("the Options dialog is up");
        match &dialog.fields[super::super::optionsdialog::TRUSTED_PUBLISHERS] {
            crate::chrome::dialog::Field::Pairs { rows, .. } => rows.clone(),
            other => panic!("not the list of publishers: {other:?}"),
        }
    }

    #[test]
    fn the_trust_center_trusts_the_certificate_in_hand_and_shows_only_its_name() {
        use super::super::optionsdialog::{FORGET_PUBLISHER, TRUST_PUBLISHER};
        use crate::chrome::dialog::Answer;

        let (_folder, own) = a_certificate("trust-centre");
        let mut editor = opened(None);
        editor.settings.macro_trust = Some(Trusting::Signed.name().to_owned());

        // A document nobody signed has nobody to trust, and says so.
        editor.open_options();
        editor.finish_dialog(Answer::Named(TRUST_PUBLISHER));
        assert!(editor.settings.trusted_publishers.is_empty());
        assert!(editor.status.contains("nobody to trust"), "{}", editor.status);

        editor.set_document(Document::open(&signed_by(&own)).expect("reopening"), None);
        assert!(matches!(editor.macros_allowed(), Allowed::No(_)));
        editor.open_options();
        editor.finish_dialog(Answer::Named(TRUST_PUBLISHER));
        assert!(editor.status.contains("is trusted"), "{}", editor.status);

        // What was written down is the certificate's fingerprint, with the
        // words on it beside it.
        let publishers = &editor.settings.trusted_publishers;
        assert_eq!(publishers.len(), 1, "{publishers:?}");
        assert_eq!(publishers[0].fingerprint, wp_sign::fingerprint(&own.certificate));
        assert_eq!(publishers[0].subject, own.certificate.subject);
        assert_eq!(editor.macros_allowed(), Allowed::Yes);

        // The page shows Word's three columns: who it is, who issued it, and
        // until when.
        let rows = publishers_shown(&editor);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, own.certificate.subject);
        // The issuer by its common name, as Word's column has it, and the
        // date the certificate stops being good.
        assert!(rows[0].1.starts_with("A Signer ("), "{}", rows[0].1);
        assert!(rows[0].1.contains(&own.certificate.not_after[..10]), "{}", rows[0].1);

        // And removing it is removing the trust.
        editor.finish_dialog(Answer::Named(FORGET_PUBLISHER));
        assert!(editor.settings.trusted_publishers.is_empty());
        assert!(editor.status.contains("not trusted any more"), "{}", editor.status);
        assert!(publishers_shown(&editor).is_empty());
        assert!(matches!(editor.macros_allowed(), Allowed::No(_)));
    }

    #[test]
    fn a_name_an_earlier_version_kept_is_shown_as_not_verified_and_trusts_nobody() {
        // What the settings file held before: the name on the certificate and
        // nothing else. It is still the person's record of whom they meant,
        // so it stays on the list — but a name is not a certificate.
        let (_folder, own) = a_certificate("trust-earlier");
        let mut editor = opened(None);
        editor.settings = crate::settings::Settings::parse(&format!(
            "macro-trust = signed\ntrusted-publisher = {}\n",
            own.certificate.subject
        ));
        editor.set_document(Document::open(&signed_by(&own)).expect("reopening"), None);
        assert!(
            matches!(editor.macros_allowed(), Allowed::No(_)),
            "a name written down alone was trusted"
        );

        editor.open_options();
        let rows = publishers_shown(&editor);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, own.certificate.subject);
        assert!(rows[0].1.contains("Not verified"), "{}", rows[0].1);

        // Trusting the publisher again from a document they signed puts the
        // certificate in the name's place, rather than beside it.
        editor.finish_dialog(crate::chrome::dialog::Answer::Named(
            super::super::optionsdialog::TRUST_PUBLISHER,
        ));
        let publishers = &editor.settings.trusted_publishers;
        assert_eq!(publishers.len(), 1, "{publishers:?}");
        assert!(publishers[0].is(&own.certificate));
        assert_eq!(editor.macros_allowed(), Allowed::Yes);
    }

    #[test]
    fn the_words_on_a_certificate_cannot_write_a_setting() {
        // The name is the certificate maker's, and the settings file is one
        // setting to a line. A line break in the name would otherwise be a
        // line of the maker's choosing in the person's settings.
        let (_folder, own) = a_certificate("trust-lines");
        let mut certificate = own.certificate.clone();
        certificate.subject = String::from("CN=Somebody\nmacro-trust = all");
        let mut editor = opened(None);
        editor.trust_publisher(&certificate);

        let read = crate::settings::Settings::parse(&editor.settings.to_text());
        assert_eq!(read.macro_trust, None, "{}", editor.settings.to_text());
        assert_eq!(read.trusted_publishers, editor.settings.trusted_publishers);
        assert!(read.trusted_publishers[0].is(&certificate));
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
