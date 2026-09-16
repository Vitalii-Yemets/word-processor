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
//! **The person's own certificates** come from a folder beside the one their
//! templates live in. That is not where Word keeps them — Windows holds a
//! person's certificates and their keys in a store of its own, and hands out
//! signatures without ever handing out the key — and it is what can be done
//! here honestly on both systems at once: a certificate and its key are two
//! files, and signing is arithmetic this program does itself. Signing with a
//! key Windows holds and will not part with is in the roadmap; it cannot be
//! written against a system this is not built on.
//!
//! # What "trusted" is allowed to mean
//!
//! That the chain reaches a root the machine trusts, that every link of it
//! holds, and that nothing in it had expired at the moment asked about. Not
//! that the certificate has not been revoked since — that means asking its
//! issuer over a network — and the program says which of the two it checked.

use std::path::PathBuf;

use wp_shell::Response;

use crate::chrome::dialog::{Dialog, Field};

use super::dialogs::Asking;

use wp_asn1::Certificate;
use wp_sign::trust::Trust;

use super::Editor;

/// What one of the person's own certificates is: the certificate, the key
/// beside it, and what to call it in a list.
#[derive(Clone, Debug)]
pub(super) struct Own {
    pub certificate: Certificate,
    pub key: wp_rsa::PrivateKey,
    /// The file it came from, which is what tells two certificates of the
    /// same name apart.
    pub file: String,
}

impl Own {
    /// The line a person picks from.
    pub fn label(&self) -> String {
        format!("{} ({})", self.certificate.subject, self.file)
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
    let Some(folder) = own_folder() else { return Vec::new() };
    own_certificates_in(&folder)
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
        out.push(Own { certificate, key, file: name.to_owned() });
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

/// Where each answer sits in the dialog that signs.
///
/// The rows before them are what the document already carries, and there may
/// be any number of those, so the two that are asked for are counted from the
/// end rather than from the start.
pub(super) const FROM_THE_END: usize = 2;

impl Editor {
    /// Word's Signatures: what the document carries, and the offer to add
    /// one.
    pub(super) fn open_signatures(&mut self) -> Response {
        let mut fields = Vec::new();
        let signatures = self.document.signatures();
        if signatures.is_empty() {
            fields.push(Field::note("This document is not signed."));
        } else {
            for signature in &signatures {
                fields.push(Field::Said {
                    label: signature.certificate.subject.clone(),
                    value: self.said_of(signature),
                });
            }
        }

        fields.push(Field::Heading("Sign this document".to_owned()));
        let mine = own_certificates();
        if mine.is_empty() {
            let folder = own_folder()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| String::from("the certificates folder"));
            // Two rows either way, so that the dialog is the same shape
            // whether or not there is anything to sign with. The folder is a
            // row of its own because a path and a sentence on one line is a
            // path with its end cut off.
            fields.push(Field::note("To sign, put a certificate and its key in this folder:"));
            fields.push(Field::Said { label: "Folder".to_owned(), value: folder });
        } else {
            fields.push(Field::Choice {
                label: "Certificate".to_owned(),
                items: mine.iter().map(Own::label).collect(),
                current: 0,
            });
            fields.push(Field::Text { label: "Purpose".to_owned(), value: String::new() });
        }

        // Wider than most: what it shows is a certificate's subject and a
        // folder's path, both of which are long, and a path with its end cut
        // off is a path nobody can follow.
        self.ask(Asking::Signatures, Dialog::new("Signatures", fields).wide(620.0))
    }

    /// Signs with whichever certificate was chosen.
    pub(super) fn apply_signature(&mut self, dialog: &Dialog) -> Response {
        let mine = own_certificates();
        if mine.is_empty() {
            return Response::Redraw;
        }
        let rows = dialog.fields.len();
        let chosen = dialog.chose(rows - FROM_THE_END);
        let why = dialog.said(rows - FROM_THE_END + 1);
        let Some(own) = mine.get(chosen) else { return Response::Ignored };

        // A document with unsaved changes signed as it stands would be a
        // signature over something nobody has seen. Word saves first, and so
        // does this.
        let Some(path) = self.file.clone() else {
            return self.report("Save the document before signing it");
        };

        let signer = wp_sign::Signer {
            certificate: own.certificate.der.clone(),
            chain: chain_for(&own.certificate, &mine),
            key: own.key.clone(),
            reason: why,
            at: super::files::timestamp(),
        };
        let bytes = match self.document.save_signed(&signer) {
            Ok(bytes) => bytes,
            Err(error) => return self.report(&format!("Cannot sign: {error}")),
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
        self.report(&crate::messages::with("Signed by {0}", &[&own.certificate.subject]))
    }
}

/// The certificates between one of the person's own and a root.
///
/// What is put into the signature so that whoever opens the document can
/// follow the chain: they have the leaf, and the roots their own machine
/// trusts, and everything in between has to travel with the document.
fn chain_for(leaf: &Certificate, mine: &[Own]) -> Vec<Vec<u8>> {
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
mod tests {
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
    struct Folder(PathBuf);

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Makes one certificate and key pair, in whichever form is asked for.
    fn folder(name: &str, form: &str) -> Folder {
        let path = std::env::temp_dir().join(format!("wp-certs-{}-{name}", std::process::id()));
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
        let dialog = editor.dialog.as_ref().expect("the dialog");
        assert_eq!(dialog.title, "Signatures");
        // Nothing signed, and the last two rows are the offer: either a list
        // to pick from or the note saying where to put a certificate.
        let rows = dialog.fields.len();
        assert!(rows >= 4, "{:?}", dialog.fields);
        assert!(matches!(dialog.fields.first(), Some(Field::Said { .. } | Field::Text { .. })));
        assert_eq!(rows - FROM_THE_END + 1, rows - 1, "the two asked-for rows are at the end");
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
            key: own.key.clone(),
            reason: String::from("Because it is mine"),
            // A few months after OpenSSL made the certificate: it dates one
            // from the minute it runs, and a moment earlier the same day is
            // outside it.
            at: String::from("2027-01-01T00:00:00Z"),
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
