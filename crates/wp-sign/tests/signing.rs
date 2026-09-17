//! Signing a package, checking one, and handing what was written to a
//! program that had no part in writing it.
//!
//! `xmlsec1` is in the build image: it implements the XML signature standard
//! and knows nothing about this program. What it checks is the part that can
//! be checked without a package — the canonicalisation of the signed
//! information, the digests of the two objects, and the RSA arithmetic — and
//! that is the part where a careful reading of the specification can still be
//! wrong.

use std::path::PathBuf;
use std::process::Command;

use wp_opc::Package;
use wp_sign::Standing;

/// A key and a certificate from OpenSSL, in a folder of their own.
struct Keys {
    folder: PathBuf,
    certificate: Vec<u8>,
    key: Vec<u8>,
}

impl Drop for Keys {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.folder);
    }
}

fn run(command: &mut Command) -> std::process::Output {
    let output = command.output().expect("the tool is in the build image");
    assert!(output.status.success(), "{:?}: {}", command, String::from_utf8_lossy(&output.stderr));
    output
}

fn keys(name: &str) -> Keys {
    let folder = std::env::temp_dir().join(format!("wp-sign-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).expect("a folder");
    let at = |file: &str| folder.join(file).to_str().expect("a path").to_owned();

    run(Command::new("openssl").args([
        "req",
        "-x509",
        "-newkey",
        "rsa:2048",
        "-keyout",
        &at("key.pem"),
        "-out",
        &at("cert.pem"),
        "-days",
        "3650",
        "-nodes",
        "-sha256",
        "-subj",
        "/C=GB/O=Nobody/CN=A Signer",
    ]));
    run(Command::new("openssl").args([
        "x509",
        "-in",
        &at("cert.pem"),
        "-outform",
        "DER",
        "-out",
        &at("cert.der"),
    ]));
    run(Command::new("openssl").args([
        "pkey",
        "-in",
        &at("key.pem"),
        "-outform",
        "DER",
        "-out",
        &at("key.der"),
    ]));

    Keys {
        certificate: std::fs::read(folder.join("cert.der")).expect("the certificate"),
        key: std::fs::read(folder.join("key.der")).expect("the key"),
        folder,
    }
}

fn signer(keys: &Keys) -> wp_sign::Signer {
    let private = wp_asn1::private_key(&keys.key).expect("a private key");
    wp_sign::Signer {
        certificate: keys.certificate.clone(),
        chain: Vec::new(),
        key: Box::new(wp_rsa::PrivateKey::new(&private.modulus, &private.exponent)),
        reason: String::from("Because it is mine"),
        at: String::from("2026-09-16T12:00:00Z"),
    }
}

/// A package of a few parts, with relationships, which is the shape a
/// document has.
fn package() -> Package {
    let mut package = Package::empty();
    package.add_part(
        "word/document.xml",
        wp_opc::MAIN_DOCUMENT_CONTENT_TYPE,
        br#"<?xml version="1.0"?><document>The quick brown fox</document>"#.to_vec(),
    );
    package.add_part(
        "word/settings.xml",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml",
        br#"<?xml version="1.0"?><settings/>"#.to_vec(),
    );
    let mut root = wp_opc::Relationships::new("");
    root.add(
        wp_opc::OFFICE_DOCUMENT_RELATIONSHIP,
        "word/document.xml",
        wp_opc::TargetMode::Internal,
    );
    package.set_relationships(&root).expect("the root relationships");
    package
}

#[test]
fn a_package_signed_here_checks_out_here() {
    let keys = keys("round");
    let mut package = package();
    assert!(!wp_sign::is_signed(&package));

    let part = wp_sign::sign(&mut package, &signer(&keys)).expect("signing");
    assert_eq!(part, "_xmlsignatures/sig1.xml");
    assert!(wp_sign::is_signed(&package));

    // Written out and opened again, because that is what happens to a
    // document and because the zip is written fresh each time.
    let bytes = package.save().expect("saving");
    let package = Package::open(&bytes).expect("opening");
    let signatures = wp_sign::signatures(&package);
    assert_eq!(signatures.len(), 1);
    let signature = &signatures[0];
    assert_eq!(signature.standing, Standing::Good, "{}", signature.standing.label());
    assert_eq!(signature.certificate.subject, "CN=A Signer, O=Nobody, C=GB");
    assert_eq!(signature.signed_at, "2026-09-16T12:00:00Z");
    assert_eq!(signature.reason, "Because it is mine");
    assert!(signature.parts.contains(&String::from("word/document.xml")));
    assert!(signature.parts.contains(&String::from("_rels/.rels")));
}

#[test]
fn a_part_changed_after_signing_is_noticed_and_named() {
    let keys = keys("changed");
    let mut package = package();
    wp_sign::sign(&mut package, &signer(&keys)).expect("signing");

    package.set_part(
        "word/document.xml",
        br#"<?xml version="1.0"?><document>The slow brown fox</document>"#.to_vec(),
    );
    let signatures = wp_sign::signatures(&package);
    assert_eq!(
        signatures[0].standing,
        Standing::Changed(String::from("word/document.xml")),
        "a changed part was not noticed"
    );
}

#[test]
fn a_part_taken_away_after_signing_is_noticed_and_named() {
    let keys = keys("missing");
    let mut package = package();
    wp_sign::sign(&mut package, &signer(&keys)).expect("signing");
    package.remove_part("word/settings.xml");

    assert_eq!(
        wp_sign::signatures(&package)[0].standing,
        Standing::Missing(String::from("word/settings.xml"))
    );
}

#[test]
fn a_signature_changed_after_it_was_made_does_not_hold() {
    let keys = keys("tampered");
    let mut package = package();
    let part = wp_sign::sign(&mut package, &signer(&keys)).expect("signing");

    // The time it says it was signed, changed by somebody who could not sign.
    let text = package.xml_part(&part).expect("the signature").expect("text");
    let changed = text.replace("2026-09-16T12:00:00Z", "2020-01-01T00:00:00Z");
    assert_ne!(changed, text);
    package.set_part(&part, changed.into_bytes());

    let standing = &wp_sign::signatures(&package)[0].standing;
    assert!(!standing.is_good(), "a changed signature held: {}", standing.label());
}

#[test]
fn taking_the_signatures_off_leaves_an_ordinary_package() {
    let keys = keys("unsigning");
    let mut package = package();
    wp_sign::sign(&mut package, &signer(&keys)).expect("signing");
    assert!(wp_sign::unsign(&mut package));
    assert!(!wp_sign::is_signed(&package));
    assert!(!wp_sign::unsign(&mut package), "nothing left to take off");

    let bytes = package.save().expect("saving");
    let reopened = Package::open(&bytes).expect("opening");
    assert!(reopened.part("word/document.xml").is_some(), "the document went with it");
    assert!(wp_sign::signatures(&reopened).is_empty());
}

/// What xmlsec1 makes of a signature this program wrote.
///
/// It is given the signature on its own, with the certificate to check
/// against. The references it can resolve are the two into the signature
/// itself — the manifest of package parts it cannot, because the parts are
/// not files it can reach — so what it checks is the canonicalisation, those
/// two digests, and the arithmetic. That is the part a careful but wrong
/// reading of the standard would fail.
#[test]
fn another_program_accepts_what_this_one_signed() {
    let keys = keys("xmlsec");
    let mut package = package();
    let part = wp_sign::sign(&mut package, &signer(&keys)).expect("signing");
    let text = package.xml_part(&part).expect("the signature").expect("text");

    let path = keys.folder.join("sig.xml");
    std::fs::write(&path, &text).expect("writing");
    let output = Command::new("xmlsec1")
        .args(["--verify", "--ignore-manifests", "--id-attr:Id", "Object", "--pubkey-cert-der"])
        .arg(keys.folder.join("cert.der"))
        .arg("--enabled-reference-uris")
        .arg("empty,same-doc")
        .arg(&path)
        .output()
        .expect("xmlsec1 is in the build image");
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(said.contains("OK"), "xmlsec1 refused the signature:\n{said}");
}

/// And a signature it should refuse.
#[test]
fn another_program_refuses_one_that_has_been_changed() {
    let keys = keys("xmlsecbad");
    let mut package = package();
    let part = wp_sign::sign(&mut package, &signer(&keys)).expect("signing");
    let text = package
        .xml_part(&part)
        .expect("the signature")
        .expect("text")
        .replace("Because it is mine", "Because it is yours");

    let path = keys.folder.join("sig.xml");
    std::fs::write(&path, &text).expect("writing");
    let output = Command::new("xmlsec1")
        .args(["--verify", "--ignore-manifests", "--id-attr:Id", "Object", "--pubkey-cert-der"])
        .arg(keys.folder.join("cert.der"))
        .arg("--enabled-reference-uris")
        .arg("empty,same-doc")
        .arg(&path)
        .output()
        .expect("xmlsec1 is in the build image");
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(said.contains("FAIL"), "xmlsec1 took a changed signature:\n{said}");
}

#[test]
fn a_relationship_part_is_signed_for_what_it_says_and_not_how_it_was_written() {
    // The same relationships, written in a different order and with the
    // target mode left out of one of them.
    let one = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="urn:a" Target="a.xml"/><Relationship Id="rId2" Type="urn:b" Target="b.xml" TargetMode="Internal"/></Relationships>"#;
    let other = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId2" Type="urn:b" Target="b.xml"/><Relationship Id="rId1" Type="urn:a" Target="a.xml"/></Relationships>"#;

    let signed_one =
        wp_sign::package::relationships_as_signed("_rels/.rels", one, &[]).expect("one");
    let signed_other =
        wp_sign::package::relationships_as_signed("_rels/.rels", other, &[]).expect("the other");
    assert_eq!(signed_one, signed_other, "the same relationships signed differently");

    // And a different set is not the same.
    let fewer =
        wp_sign::package::relationships_as_signed("_rels/.rels", one, &[String::from("rId1")])
            .expect("fewer");
    assert_ne!(signed_one, fewer);
}
