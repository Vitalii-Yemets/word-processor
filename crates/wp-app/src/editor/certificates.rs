//! Where a certificate comes from, and what a signature on a document is
//! worth.
//!
//! # Two lists, from two places
//!
//! **The roots the machine trusts** come from the machine — see
//! [`wp_shell::certificates`], which knows where each system keeps them. They
//! are what makes a signature worth anything: a certificate is a name and a
//! key until somebody the machine already trusts vouches for it.
//!
//! **The person's own certificates** come from two places, and the difference
//! between them is who holds the key.
//!
//! From **a folder** beside the one their templates live in: a certificate and
//! its key are two files, this program reads both, and the signing is
//! arithmetic it does itself. That works on every system alike, and it is the
//! only thing that works on one with no certificate store.
//!
//! From **the system's own store**, where Windows keeps a person's
//! certificates. The key there is not something this program can have, and
//! that is the point of keeping it there: the system signs on the person's
//! behalf and hands back a signature, never the key. It is also the only way
//! a key on a smart card or in a TPM can be used at all, and it is where the
//! certificate a person already signs their mail with lives. See
//! [`wp_shell::certificates::sign_with_held`].
//!
//! Both are offered in one list, because a person picking a certificate is
//! picking who they are and not which of two mechanisms will do the sums.
//!
//! # What "trusted" is allowed to mean
//!
//! That the chain reaches a root the machine trusts, that every link of it
//! holds, and that nothing in it had expired at the moment asked about. Not
//! that the certificate has not been revoked since — that means asking its
//! issuer over a network — and the program says which of the two it checked.

use std::path::PathBuf;

use wp_asn1::Certificate;
use wp_sign::trust::Trust;

use super::Editor;

/// What one of the person's own certificates is: the certificate, and where
/// whatever will sign with it is to be found.
#[derive(Clone, Debug)]
pub(super) struct Own {
    pub certificate: Certificate,
    pub from: From,
}

/// Which of the two places a certificate came from, and what that means for
/// signing with it.
#[derive(Clone, Debug)]
pub(super) enum From {
    /// A folder this program reads: the key is two files away and the signing
    /// is arithmetic done here.
    Folder {
        key: wp_rsa::PrivateKey,
        /// The file it came from, which is what tells two certificates of the
        /// same name apart.
        file: String,
    },
    /// The system's own store: the key stays there and the system signs.
    System,
}

impl Own {
    /// The line a person picks from.
    ///
    /// The subject and then where it came from, because a person may well
    /// have the same certificate in both places and the two sign by different
    /// routes: one of them can ask for a smart card and the other cannot.
    pub fn label(&self) -> String {
        match &self.from {
            From::Folder { file, .. } => format!("{} ({file})", self.certificate.subject),
            From::System => format!(
                "{} ({})",
                self.certificate.subject,
                crate::messages::t("in this machine's store")
            ),
        }
    }

    /// Whether the system will be doing the signing rather than this program.
    ///
    /// Worth telling apart when one fails: a key in a folder that will not
    /// sign is this program's fault, and a store that will not is something
    /// the person can do something about.
    #[must_use]
    pub fn is_from_the_system(&self) -> bool {
        matches!(self.from, From::System)
    }

    /// Whatever will sign with it.
    pub fn signs(&self) -> Box<dyn wp_sign::Signs> {
        match &self.from {
            From::Folder { key, .. } => Box::new(key.clone()),
            From::System => Box::new(Held { certificate: self.certificate.der.clone() }),
        }
    }
}

/// Signing through the system, which never hands the key over.
///
/// What crosses the wall is a hash one way and a signature the other. The
/// hashing is done here because the system will not be handed a document, and
/// the signing is done there because this program will not be handed a key.
struct Held {
    certificate: Vec<u8>,
}

impl wp_sign::Signs for Held {
    fn sign(&self, algorithm: wp_rsa::Algorithm, message: &[u8]) -> Option<Vec<u8>> {
        let hash = algorithm.of(message);
        wp_shell::certificates::sign_with_held(&self.certificate, algorithm.name(), &hash)
    }
}

/// Where a person's own certificates live.
///
/// Beside their templates, because that is the folder this program already
/// keeps a person's own things in and a second convention would be one to
/// remember.
#[must_use]
pub(super) fn own_folder() -> Option<PathBuf> {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok()
        .filter(|value| !value.is_empty())?;
    Some(PathBuf::from(home).join("Documents").join("Word Processor Certificates"))
}

/// Reads whatever is in that folder.
///
/// A certificate and its key are two files with the same stem: `mine.der` or
/// `mine.pem` beside `mine.key` — `.key.der` and `.key.pem` as well, since
/// that is how the tools that make them name things. A certificate with no
/// key beside it is not offered: it could not sign, and a list of things that
/// cannot be used is a list nobody can use.
#[must_use]
pub(super) fn own_certificates() -> Vec<Own> {
    // The system's first, because on a machine that has a store that is where
    // a person's real certificate is and the folder is the fallback.
    let mut out = held_certificates();
    if let Some(folder) = own_folder() {
        out.extend(own_certificates_in(&folder));
    }
    out
}

/// The certificates the system holds, as this program's own list has them.
///
/// A certificate the system cannot read back as a certificate is left out
/// rather than shown as a name nobody can make sense of: whatever is in a
/// store is the store's business, and what this program lists is what it
/// could sign with.
#[must_use]
pub(super) fn held_certificates() -> Vec<Own> {
    wp_shell::certificates::held_certificates()
        .into_iter()
        .filter_map(|held| {
            Certificate::read(&held.certificate)
                .map(|certificate| Own { certificate, from: From::System })
        })
        .collect()
}

/// The same, out of a folder that is named rather than found.
///
/// Named so that a test can put certificates somewhere and read them back
/// without moving the home directory out from under the rest of the program.
#[must_use]
pub(super) fn own_certificates_in(folder: &std::path::Path) -> Vec<Own> {
    let Ok(entries) = std::fs::read_dir(folder) else { return Vec::new() };

    let mut out = Vec::new();
    let mut found: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
    found.sort();

    for path in &found {
        let name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
        if name.contains(".key") {
            continue;
        }
        let Some(der) = read_der(path) else { continue };
        let Some(certificate) = Certificate::read(&der) else { continue };
        let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
        let Some(key) = key_beside(folder, stem) else { continue };
        out.push(Own { certificate, from: From::Folder { key, file: name.to_owned() } });
    }
    out
}

/// The key that goes with a certificate, whatever it is called.
fn key_beside(folder: &std::path::Path, stem: &str) -> Option<wp_rsa::PrivateKey> {
    for name in [format!("{stem}.key"), format!("{stem}.key.der"), format!("{stem}.key.pem")] {
        let Some(der) = read_der(&folder.join(name)) else { continue };
        if let Some(key) = wp_asn1::private_key(&der) {
            return Some(wp_rsa::PrivateKey::new(&key.modulus, &key.exponent));
        }
    }
    None
}

/// Reads a file that may be written either way round.
///
/// DER is the bytes themselves; PEM is those bytes in base 64 between two
/// marker lines. Both are met, and telling them apart is looking for the
/// marker.
fn read_der(path: &std::path::Path) -> Option<Vec<u8>> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.starts_with(b"-----BEGIN") {
        let text = String::from_utf8_lossy(&bytes);
        return wp_shell::certificates::from_pem(&text).into_iter().next().or_else(|| {
            // A key file, which is the same shape with another marker in it.
            let start = text.find("-----\n")? + 6;
            let end = text.rfind("-----END")?;
            Some(wp_text::base64::decode(
                text[start..end]
                    .chars()
                    .filter(|c| !c.is_whitespace())
                    .collect::<String>()
                    .as_bytes(),
            ))
        });
    }
    Some(bytes)
}

/// Every certificate the machine trusts, read.
#[must_use]
pub(super) fn trusted_roots() -> Vec<Certificate> {
    wp_shell::certificates::trusted_roots()
        .iter()
        .filter_map(|der| Certificate::read(der))
        .collect()
}

/// The certificates between one of the person's own and a root.
///
/// What is put into the signature so that whoever opens the document can
/// follow the chain: they have the leaf, and the roots their own machine
/// trusts, and everything in between has to travel with the document.
pub(super) fn chain_for(leaf: &Certificate, mine: &[Own]) -> Vec<Vec<u8>> {
    let roots = trusted_roots();
    let mut out = Vec::new();
    let mut current = leaf.clone();
    // The same limit the chain walk uses, and for the same reason.
    for _ in 0..16 {
        if roots.iter().any(|root| root.der == current.der) {
            break;
        }
        let Some(parent) = mine
            .iter()
            .map(|own| &own.certificate)
            .chain(roots.iter())
            .find(|other| other.subject_der == current.issuer_der && other.der != current.der)
        else {
            break;
        };
        // A root is on the reader's machine already; sending it is sending
        // them something they have and would have ignored.
        if !roots.iter().any(|root| root.der == parent.der) {
            out.push(parent.der.clone());
        }
        current = parent.clone();
    }
    out
}

impl Editor {
    /// What the signature on the document is worth, as this machine sees it.
    ///
    /// `None` where the machine's own list could not be read at all, which is
    /// not the same answer as "not trusted" and must not be shown as one.
    pub(super) fn trust_of(&self, signature: &wp_sign::Signature) -> Option<Trust> {
        let roots = trusted_roots();
        if roots.is_empty() {
            return None;
        }
        let moment = if signature.signed_at.is_empty() {
            super::files::timestamp()
        } else {
            signature.signed_at.clone()
        };
        Some(wp_sign::trust::chain(&signature.certificate, &signature.chain, &roots, &moment))
    }

    /// One line about a signature: whose it is, whether it holds, and what
    /// this machine thinks of the certificate behind it.
    pub(super) fn said_of(&self, signature: &wp_sign::Signature) -> String {
        let standing = signature.standing.label();
        let trust = match self.trust_of(signature) {
            Some(trust) => trust.said(),
            None => crate::messages::t("This machine's list of trusted issuers could not be read")
                .to_owned(),
        };
        format!("{} — {standing}. {trust}", signature.certificate.subject)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::process::Command;
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

    fn run(command: &mut Command) {
        let output = command.output().expect("the tool is in the build image");
        assert!(
            output.status.success(),
            "{command:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// A folder with a certificate and its key in it, made by OpenSSL, which
    /// goes when the test does.
    pub(crate) struct Folder(pub PathBuf);

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Makes one certificate and key pair, in whichever form is asked for.
    pub(crate) fn folder(name: &str, form: &str) -> Folder {
        // Inside the project rather than in the machine's temp: a test
        // that scatters files outside the tree it was run from is one
        // nobody can clean up by deleting the tree.
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("certificates")
            .join(format!("{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("a folder");
        let at = |file: &str| path.join(file).to_str().expect("a path").to_owned();

        run(Command::new("openssl").args([
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-keyout",
            &at("mine.key.pem"),
            "-out",
            &at("mine.pem"),
            "-days",
            "3650",
            "-nodes",
            "-sha256",
            "-subj",
            "/C=GB/O=Nobody/CN=A Signer",
        ]));
        if form == "der" {
            run(Command::new("openssl").args([
                "x509",
                "-in",
                &at("mine.pem"),
                "-outform",
                "DER",
                "-out",
                &at("mine.der"),
            ]));
            run(Command::new("openssl").args([
                "pkey",
                "-in",
                &at("mine.key.pem"),
                "-outform",
                "DER",
                "-out",
                &at("mine.key"),
            ]));
            let _ = std::fs::remove_file(path.join("mine.pem"));
            let _ = std::fs::remove_file(path.join("mine.key.pem"));
        }
        Folder(path)
    }

    #[test]
    fn a_certificate_and_its_key_are_found_however_they_are_written() {
        for form in ["der", "pem"] {
            let folder = folder(&format!("found-{form}"), form);
            let mine = own_certificates_in(&folder.0);
            assert_eq!(mine.len(), 1, "{form}: {mine:?}");
            assert!(mine[0].certificate.subject.contains("A Signer"), "{form}");
            assert!(mine[0].label().contains("A Signer"), "{form}");
        }
    }

    #[test]
    fn where_a_certificate_came_from_is_on_the_line_a_person_picks() {
        // A person may well have the same certificate in both places, and the
        // two sign by different routes: one of them can want a smart card and
        // the other cannot. A list that showed one name twice would be asking
        // them to guess.
        let folder = folder("labelled", "der");
        let mine = own_certificates_in(&folder.0);
        let own = mine.first().expect("a certificate");
        assert!(own.label().ends_with(".der)"), "{}", own.label());

        let held = Own { certificate: own.certificate.clone(), from: From::System };
        assert!(held.label().ends_with("(in this machine's store)"), "{}", held.label());
        assert_ne!(own.label(), held.label());
    }

    #[test]
    fn a_key_in_a_folder_signs_through_the_same_door_the_system_would() {
        // The signing went behind a trait so that a key this program can read
        // and one it cannot look the same to whatever is signing. What that
        // has to not change is the signature: the same key over the same
        // bytes, through the trait, is the signature the key makes.
        let folder = folder("through-the-door", "der");
        let mine = own_certificates_in(&folder.0);
        let own = mine.first().expect("a certificate");
        let From::Folder { key, .. } = &own.from else { panic!("it came from a folder") };

        let message = b"what a SignedInfo comes to";
        let straight = key.sign(wp_rsa::Algorithm::Sha256, message).expect("signing");
        let through = own.signs().sign(wp_rsa::Algorithm::Sha256, message).expect("signing");
        assert_eq!(straight, through);
    }

    #[test]
    fn a_certificate_the_system_holds_cannot_be_signed_with_where_there_is_no_system() {
        // On a machine with no store there is nothing to ask, and the honest
        // answer to "sign this" is that it was not signed. The build
        // container is such a machine, which is what makes this testable at
        // all: what is being checked is the refusal, not the store.
        let folder = folder("no-store", "der");
        let mine = own_certificates_in(&folder.0);
        let own = mine.first().expect("a certificate");
        let held = Own { certificate: own.certificate.clone(), from: From::System };

        let made = held.signs().sign(wp_rsa::Algorithm::Sha256, b"anything");
        #[cfg(not(windows))]
        assert_eq!(made, None, "a machine with no store signed something");
        // On Windows it may well sign, and whether it does is the store's
        // business: what this asserts there is that asking does not panic.
        #[cfg(windows)]
        let _ = made;
    }

    #[test]
    fn a_certificate_with_no_key_beside_it_is_not_offered() {
        // It could not sign, and a list of things that cannot be used is a
        // list nobody can use.
        let folder = folder("no-key", "der");
        std::fs::remove_file(folder.0.join("mine.key")).expect("the key");
        assert!(own_certificates_in(&folder.0).is_empty());
    }

    #[test]
    fn a_folder_that_is_not_there_is_no_certificates_and_not_a_crash() {
        assert!(own_certificates_in(std::path::Path::new("/no/such/folder")).is_empty());
    }

    #[test]
    fn the_dialog_says_what_the_document_carries_and_what_it_can_be_signed_with() {
        let mut editor = editor();
        editor.open_signatures();
        assert!(editor.show_signatures, "the pane did not open");
        assert!(editor.dialog.is_none(), "it opened a dialog over the document");

        // Nothing signed and nobody asked, which is what an ordinary document
        // is, and the pane says both rather than either.
        let shown = editor.signature_pane_shown();
        assert!(shown.made.is_empty());
        assert!(shown.wanted.is_empty());
    }

    #[test]
    fn a_signature_is_read_back_with_what_this_machine_makes_of_it() {
        // Signed with a certificate nobody vouches for, which is the honest
        // answer for one made here: it holds, and the machine has never heard
        // of whoever issued it.
        let folder = folder("read-back", "der");
        let mine = own_certificates_in(&folder.0);
        let own = mine.first().expect("a certificate");

        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("Signed")));
        let document = Document::create(&body).expect("a document");
        let signer = wp_sign::Signer {
            certificate: own.certificate.der.clone(),
            chain: chain_for(&own.certificate, &mine),
            key: own.signs(),
            reason: String::from("Because it is mine"),
            // A few months after OpenSSL made the certificate: it dates one
            // from the minute it runs, and a moment earlier the same day is
            // outside it.
            at: String::from("2027-01-01T00:00:00Z"),
            line: String::new(),
        };
        let bytes = document.save_signed(&signer).expect("signing");
        let signed = Document::open(&bytes).expect("reopening");

        let mut editor = editor();
        editor.set_document(signed, None);
        let signatures = editor.document.signatures();
        assert_eq!(signatures.len(), 1, "{signatures:?}");
        assert!(signatures[0].standing.is_good(), "{:?}", signatures[0].standing);

        let said = editor.said_of(&signatures[0]);
        assert!(said.contains("A Signer"), "{said}");
        // Either the machine has a list and does not know this issuer, or it
        // has no list at all; both are said plainly and neither says trusted.
        assert!(
            said.contains("vouches for") || said.contains("could not be read"),
            "a certificate nobody issued was called trusted: {said}"
        );
        assert!(editor.trust_of(&signatures[0]).is_none_or(|trust| !trust.is_trusted()));
    }

    #[test]
    fn a_chain_is_only_what_the_reader_would_not_have() {
        // One self-signed certificate has nothing between it and a root, so
        // nothing travels with the document.
        let folder = folder("chain", "der");
        let mine = own_certificates_in(&folder.0);
        let own = mine.first().expect("a certificate");
        assert!(chain_for(&own.certificate, &mine).is_empty());
    }
}
