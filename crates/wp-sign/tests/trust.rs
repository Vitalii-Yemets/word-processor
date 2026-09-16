//! A chain of certificates, built and checked against one OpenSSL made.
//!
//! OpenSSL is in the build image and had no part in writing any of this. What
//! it provides here is a real chain — a root, an authority under it, and a
//! certificate under that — and its own verdict on the same chain, which is
//! what this program's verdict is held to.

use std::path::PathBuf;
use std::process::Command;

use wp_asn1::Certificate;
use wp_sign::trust::{chain, Fault, Trust};

/// The moment every chain here is judged at.
///
/// Fixed rather than taken from the clock, so that a test does not change its
/// answer overnight - and a few months after the certificates are made rather
/// than the same day, because OpenSSL dates them from the minute it runs and
/// a moment earlier that day is outside them.
const NOW: &str = "2027-01-01T00:00:00Z";

/// A chain in a folder of its own, which goes when the test does.
struct Chain {
    folder: PathBuf,
    root: Certificate,
    authority: Certificate,
    leaf: Certificate,
    /// A certificate under one that is not allowed to issue.
    under_a_leaf: Certificate,
}

impl Drop for Chain {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.folder);
    }
}

fn run(command: &mut Command) -> std::process::Output {
    let output = command.output().expect("the tool is in the build image");
    assert!(output.status.success(), "{:?}: {}", command, String::from_utf8_lossy(&output.stderr));
    output
}

fn read(path: &PathBuf) -> Certificate {
    let der = std::fs::read(path).expect("the certificate");
    Certificate::read(&der).expect("a readable certificate")
}

/// Makes a root, an authority under it, a certificate under that, and one
/// under a certificate that may not issue.
fn built(name: &str) -> Chain {
    // One folder per test: they run in threads of one process, and a shared
    // folder is one test clearing away another's certificates.
    let folder = std::env::temp_dir().join(format!("wp-trust-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).expect("a folder");
    let at = |file: &str| folder.join(file).to_str().expect("a path").to_owned();

    // A root that says it may issue, which is what `-x509` with these
    // extensions writes.
    run(Command::new("openssl").args([
        "req",
        "-x509",
        "-newkey",
        "rsa:2048",
        "-keyout",
        &at("root.key"),
        "-out",
        &at("root.pem"),
        "-days",
        "3650",
        "-nodes",
        "-sha256",
        "-subj",
        "/C=GB/O=Nobody/CN=A Root",
        "-addext",
        "basicConstraints=critical,CA:TRUE",
    ]));

    // An authority under it: a request, then the root signing it.
    for (name, subject, constraints) in [
        ("authority", "/C=GB/O=Nobody/CN=An Authority", "basicConstraints=critical,CA:TRUE"),
        ("leafish", "/C=GB/O=Nobody/CN=Not An Authority", "basicConstraints=critical,CA:FALSE"),
    ] {
        run(Command::new("openssl").args([
            "req",
            "-newkey",
            "rsa:2048",
            "-keyout",
            &at(&format!("{name}.key")),
            "-out",
            &at(&format!("{name}.csr")),
            "-nodes",
            "-sha256",
            "-subj",
            subject,
        ]));
        std::fs::write(folder.join(format!("{name}.ext")), constraints).expect("the extensions");
        run(Command::new("openssl").args([
            "x509",
            "-req",
            "-in",
            &at(&format!("{name}.csr")),
            "-CA",
            &at("root.pem"),
            "-CAkey",
            &at("root.key"),
            "-set_serial",
            "2",
            "-days",
            "3000",
            "-sha256",
            "-extfile",
            &at(&format!("{name}.ext")),
            "-out",
            &at(&format!("{name}.pem")),
        ]));
    }

    // And one under each of those two.
    for (name, parent) in [("leaf", "authority"), ("hopeful", "leafish")] {
        run(Command::new("openssl").args([
            "req",
            "-newkey",
            "rsa:2048",
            "-keyout",
            &at(&format!("{name}.key")),
            "-out",
            &at(&format!("{name}.csr")),
            "-nodes",
            "-sha256",
            "-subj",
            &format!("/C=GB/O=Nobody/CN=A {name}"),
        ]));
        run(Command::new("openssl").args([
            "x509",
            "-req",
            "-in",
            &at(&format!("{name}.csr")),
            "-CA",
            &at(&format!("{parent}.pem")),
            "-CAkey",
            &at(&format!("{parent}.key")),
            "-set_serial",
            "3",
            "-days",
            "2000",
            "-sha256",
            "-out",
            &at(&format!("{name}.pem")),
        ]));
    }

    for name in ["root", "authority", "leaf", "leafish", "hopeful"] {
        run(Command::new("openssl").args([
            "x509",
            "-in",
            &at(&format!("{name}.pem")),
            "-outform",
            "DER",
            "-out",
            &at(&format!("{name}.der")),
        ]));
    }

    Chain {
        root: read(&folder.join("root.der")),
        authority: read(&folder.join("authority.der")),
        leaf: read(&folder.join("leaf.der")),
        under_a_leaf: read(&folder.join("hopeful.der")),
        folder,
    }
}

#[test]
fn a_chain_that_reaches_a_trusted_root_is_trusted() {
    let built = built("a_chain_that_reaches_a_trusted_root_is_trusted");
    let found = chain(
        &built.leaf,
        std::slice::from_ref(&built.authority),
        std::slice::from_ref(&built.root),
        NOW,
    );
    assert_eq!(found, Trust::Trusted(built.root.subject.clone()), "{}", found.said());
    assert!(found.is_trusted());

    // And OpenSSL, which had no part in the reading, says the same.
    let at = |file: &str| built.folder.join(file).to_str().expect("a path").to_owned();
    run(Command::new("openssl").args([
        "verify",
        "-CAfile",
        &at("root.pem"),
        "-untrusted",
        &at("authority.pem"),
        &at("leaf.pem"),
    ]));
}

#[test]
fn a_chain_whose_root_the_machine_does_not_trust_is_not() {
    let built = built("a_chain_whose_root_the_machine_does_not_trust_is_not");
    let found = chain(&built.leaf, std::slice::from_ref(&built.authority), &[], NOW);
    assert_eq!(found, Trust::Unknown(built.authority.subject.clone()), "{}", found.said());
    assert!(!found.is_trusted());
    assert!(found.said().contains("An Authority"), "{}", found.said());
}

#[test]
fn a_chain_with_its_middle_missing_stops_there() {
    let built = built("a_chain_with_its_middle_missing_stops_there");
    let found = chain(&built.leaf, &[], std::slice::from_ref(&built.root), NOW);
    assert_eq!(found, Trust::Unknown(built.leaf.subject.clone()), "{}", found.said());
}

#[test]
fn the_root_itself_is_trusted_when_it_is_in_the_list() {
    let built = built("the_root_itself_is_trusted_when_it_is_in_the_list");
    let found = chain(&built.root, &[], std::slice::from_ref(&built.root), NOW);
    assert!(found.is_trusted(), "{}", found.said());
}

#[test]
fn a_certificate_issued_by_one_that_may_not_issue_is_refused() {
    // The heart of it: without this check anybody with any certificate could
    // issue any other, and the chain would be worth nothing.
    let built = built("issued_by_one_that_may_not_issue");
    let found = chain(
        &built.under_a_leaf,
        &[read(&built.folder.join("leafish.der"))],
        std::slice::from_ref(&built.root),
        NOW,
    );
    assert_eq!(
        found,
        Trust::Broken(built.under_a_leaf.subject.clone(), Fault::NotAnAuthority),
        "{}",
        found.said()
    );

    // OpenSSL refuses it too, which is how this is known to be the right
    // answer rather than merely a strict one.
    let at = |file: &str| built.folder.join(file).to_str().expect("a path").to_owned();
    let output = Command::new("openssl")
        .args([
            "verify",
            "-CAfile",
            &at("root.pem"),
            "-untrusted",
            &at("leafish.pem"),
            &at("hopeful.pem"),
        ])
        .output()
        .expect("openssl");
    assert!(!output.status.success(), "OpenSSL accepted a certificate issued by a leaf");
}

#[test]
fn a_certificate_that_has_been_meddled_with_breaks_the_chain() {
    let built = built("a_certificate_that_has_been_meddled_with_breaks_the_chain");
    let mut changed = built.leaf.clone();
    // One byte of the part the issuer signed: the same certificate in every
    // other respect, and not the one that was signed.
    let last = changed.signed_part.len() - 1;
    changed.signed_part[last] ^= 0xFF;

    let found = chain(
        &changed,
        std::slice::from_ref(&built.authority),
        std::slice::from_ref(&built.root),
        NOW,
    );
    assert_eq!(found, Trust::Broken(changed.subject.clone(), Fault::Signature), "{}", found.said());
}

#[test]
fn a_certificate_is_not_trusted_outside_its_dates() {
    let built = built("a_certificate_is_not_trusted_outside_its_dates");
    let found = chain(
        &built.leaf,
        std::slice::from_ref(&built.authority),
        std::slice::from_ref(&built.root),
        "1999-01-01T00:00:00Z",
    );
    assert!(matches!(found, Trust::Expired(..)), "{found:?}");
    assert!(found.said().contains("only valid from"), "{}", found.said());
}

#[test]
fn what_the_certificate_says_about_itself_is_read() {
    let built = built("what_the_certificate_says_about_itself_is_read");
    assert!(built.root.authority, "the root does not say it may issue");
    assert!(built.authority.authority, "the authority does not say it may issue");
    assert!(!built.leaf.authority, "a leaf says it may issue");
    assert_eq!(built.leaf.issuer_der, built.authority.subject_der, "the names do not join up");
    assert_eq!(built.leaf.signature_algorithm, "1.2.840.113549.1.1.11", "sha256WithRSAEncryption");
}

#[test]
fn a_ring_of_certificates_does_not_walk_for_ever() {
    // Two that name each other as issuer. Nothing real looks like this; a
    // program that followed it would not come back.
    let built = built("a_ring_of_certificates");
    let mut first = built.leaf.clone();
    let mut second = built.authority.clone();
    first.issuer_der = second.subject_der.clone();
    second.issuer_der = first.subject_der.clone();
    second.authority = true;
    first.authority = true;

    let found = chain(&first, &[second], &[], NOW);
    assert!(matches!(found, Trust::Unknown(_) | Trust::Broken(..)), "{found:?}");
}
