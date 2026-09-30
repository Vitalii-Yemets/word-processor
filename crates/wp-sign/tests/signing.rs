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
        line: String::new(),
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

/// Signing, then somebody else signing that signature.
#[test]
fn a_signature_can_be_signed_by_somebody_else() {
    let mine = keys("countersigned");
    let mut sealed = package();
    let part = wp_sign::sign(&mut sealed, &signer(&mine)).expect("signing");

    // The same key stands in for the second person: what is being tested is
    // that a countersignature covers the first signature's value and holds,
    // not that two certificates differ.
    let theirs = keys("countersigner");
    let mut second = signer(&theirs);
    second.reason = String::from("Witnessed");
    second.at = String::from("2026-09-17T09:00:00Z");
    wp_sign::countersign(&mut sealed, &part, &second).expect("countersigning");

    let signatures = wp_sign::signatures(&sealed);
    assert_eq!(signatures.len(), 1, "countersigning made a second signature");
    let signature = &signatures[0];
    assert_eq!(signature.standing, wp_sign::Standing::Good, "the first signature stopped holding");

    assert_eq!(signature.counters.len(), 1, "the countersignature was not read back");
    let counter = &signature.counters[0];
    assert_eq!(counter.standing, wp_sign::Standing::Good, "{:?}", counter.standing);
    assert_eq!(counter.role, "Witnessed");
    assert_eq!(counter.signed_at, "2026-09-17T09:00:00Z");
}

#[test]
fn a_second_countersignature_goes_beside_the_first() {
    let mine = keys("countersigned-twice");
    let mut sealed = package();
    let part = wp_sign::sign(&mut sealed, &signer(&mine)).expect("signing");

    for (why, when) in [("Witnessed", "2026-09-17T09:00:00Z"), ("Approved", "2026-09-18T09:00:00Z")]
    {
        let mut witness = signer(&mine);
        witness.reason = String::from(why);
        witness.at = String::from(when);
        wp_sign::countersign(&mut sealed, &part, &witness).expect("countersigning");
    }

    let signatures = wp_sign::signatures(&sealed);
    let counters = &signatures[0].counters;
    assert_eq!(counters.len(), 2, "one of them replaced the other");
    assert!(counters.iter().all(|counter| counter.standing == wp_sign::Standing::Good));
    // Each says what it was signed as, which is the whole of why there are two.
    let roles: Vec<&str> = counters.iter().map(|counter| counter.role.as_str()).collect();
    assert_eq!(roles, vec!["Witnessed", "Approved"]);
}

#[test]
fn a_countersignature_does_not_hold_over_a_signature_that_was_changed() {
    // What it covers is the first signature's value. A document where that
    // value has been swapped for another is one where the countersignature
    // was made over something that is no longer there, and saying it still
    // holds would be the whole point thrown away.
    let mine = keys("countersign-changed");
    let mut sealed = package();
    let part = wp_sign::sign(&mut sealed, &signer(&mine)).expect("signing");
    let mut witness = signer(&mine);
    witness.reason = String::from("Witnessed");
    wp_sign::countersign(&mut sealed, &part, &witness).expect("countersigning");

    // One letter of the signed value, changed.
    let text = sealed.xml_part(&part).expect("the part").expect("text");
    let at = text.find("<SignatureValue").expect("a value");
    let end = text[at..].find("</SignatureValue>").expect("the end of it") + at;
    let middle = at + (end - at) / 2;
    let swapped = if text.as_bytes()[middle] == b'A' { 'B' } else { 'A' };
    let mut changed = text.clone();
    changed.replace_range(middle..middle + 1, &swapped.to_string());
    sealed.add_part(&part, wp_sign::package::SIGNATURE_TYPE, changed.into_bytes());

    let signatures = wp_sign::signatures(&sealed);
    let counter = &signatures[0].counters[0];
    assert_ne!(counter.standing, wp_sign::Standing::Good, "it held over a changed value");
}

#[test]
fn a_signature_made_for_a_line_says_which_one() {
    let mine = keys("for-a-line");
    let mut sealed = package();
    let mut named = signer(&mine);
    named.line = String::from("5f2c1a9e");
    wp_sign::sign(&mut sealed, &named).expect("signing");

    let signatures = wp_sign::signatures(&sealed);
    assert_eq!(signatures[0].line, "5f2c1a9e");
    assert_eq!(signatures[0].standing, wp_sign::Standing::Good);

    // And one about the document at large names nothing, which is the
    // difference a reader pairs signatures with lines by.
    let mut plain = package();
    wp_sign::sign(&mut plain, &signer(&mine)).expect("signing");
    assert_eq!(wp_sign::signatures(&plain)[0].line, "");
}

#[test]
fn what_a_signature_says_about_itself_is_signed_with_everything_else() {
    // The signing time and the signer's own certificate are written into the
    // signature as XAdES properties, and those properties are referenced from
    // the signed information. Changing either has to break the signature, or
    // they are decoration.
    let mine = keys("xades");
    let mut sealed = package();
    let part = wp_sign::sign(&mut sealed, &signer(&mine)).expect("signing");

    let text = sealed.xml_part(&part).expect("the part").expect("text");
    assert!(text.contains("SigningTime"), "no signing time was written");
    assert!(text.contains("SigningCertificate"), "no signing certificate was written");
    assert!(text.contains("idSignedProperties"), "the properties are not named");

    let changed = text.replace("2026-09-16T12:00:00Z", "2020-01-01T00:00:00Z");
    assert_ne!(changed, text, "the time was not there to change");
    sealed.add_part(&part, wp_sign::package::SIGNATURE_TYPE, changed.into_bytes());

    let signatures = wp_sign::signatures(&sealed);
    assert_ne!(
        signatures[0].standing,
        wp_sign::Standing::Good,
        "the signing time could be changed without the signature noticing"
    );
}

/// The signature a test has changed, signed again by whoever holds the key.
///
/// What somebody who is also the signer could do, and so the case where the
/// arithmetic comes out and only what was signed can say whether the
/// signature is worth anything. `counter` picks the first countersignature
/// rather than the signature itself.
fn signed_again(text: &str, keys: &Keys, counter: bool) -> String {
    fn named<'a>(
        element: &'a wp_xml::tree::Element,
        local: &str,
    ) -> Option<&'a wp_xml::tree::Element> {
        if element.local_name() == local {
            return Some(element);
        }
        element.child_elements().find_map(|child| named(child, local))
    }

    let tree = wp_xml::tree::XmlTree::parse(text).expect("the signature");
    let root = &tree.root;
    let signature = if counter {
        named(root, "CounterSignature")
            .and_then(|wrapper| {
                wrapper.child_elements().find(|one| one.local_name() == "Signature")
            })
            .expect("a countersignature")
    } else {
        root
    };
    let child = |local: &str| {
        signature.child_elements().find(|child| child.local_name() == local).expect(local)
    };
    // Canonicalised the way the reader does it: in what the whole signature
    // declares.
    let canonical = wp_sign::c14n::canonical(child("SignedInfo"), &wp_sign::c14n::context(&[root]));
    let private = wp_asn1::private_key(&keys.key).expect("a private key");
    let key = wp_rsa::PrivateKey::new(&private.modulus, &private.exponent);
    let value = key.sign(wp_rsa::Algorithm::Sha256, canonical.as_bytes()).expect("signing");
    let old = child("SignatureValue").text_content();
    text.replacen(old.trim(), &wp_text::base64::encode(&value), 1)
}

/// The text with the first stretch that runs from `from` to the next `to`
/// taken out, both included.
fn without(text: &str, from: &str, to: &str) -> String {
    let start = text.find(from).unwrap_or_else(|| panic!("{from} is not there"));
    let end = text[start..].find(to).expect("the end of it") + start + to.len();
    format!("{}{}", &text[..start], &text[end..])
}

/// The one reference in the signed information that covers the manifest.
const PACKAGE_REFERENCE: &str =
    r##"<Reference Type="http://www.w3.org/2000/09/xmldsig#Object" URI="#idPackageObject">"##;

#[test]
fn an_unsigned_manifest_put_before_the_signed_one_does_not_hide_a_changed_part() {
    // The attack the review found. The signed information and its value are
    // left exactly as they were made; a changed part is covered for by an
    // empty manifest nobody signed, put where a reader that takes the first
    // manifest it meets would meet it first.
    let keys = keys("decoy-manifest");
    let mut package = package();
    let part = wp_sign::sign(&mut package, &signer(&keys)).expect("signing");
    assert_eq!(wp_sign::signatures(&package)[0].standing, Standing::Good, "the untouched one");

    package.set_part(
        "word/document.xml",
        br#"<?xml version="1.0"?><document>The slow brown fox</document>"#.to_vec(),
    );
    let changed = Standing::Changed(String::from("word/document.xml"));
    assert_eq!(wp_sign::signatures(&package)[0].standing, changed, "the ordinary case");

    let text = package.xml_part(&part).expect("the signature").expect("text");
    let decoy = text.replacen(
        r#"<Object Id="idPackageObject">"#,
        r#"<Object><Manifest></Manifest></Object><Object Id="idPackageObject">"#,
        1,
    );
    assert_ne!(decoy, text, "the decoy did not go in");
    package.set_part(&part, decoy.into_bytes());

    let signature = &wp_sign::signatures(&package)[0];
    assert_eq!(signature.standing, changed, "an unsigned manifest covered for a changed part");
    assert!(
        signature.parts.contains(&String::from("word/document.xml")),
        "what it covers was read from the decoy: {:?}",
        signature.parts
    );
}

#[test]
fn a_signature_whose_signed_objects_hold_no_manifest_covers_nothing() {
    // Made by the key it names, and holding: but what it signed is only what
    // it says about itself, and none of the document.
    let keys = keys("no-manifest");
    let mut package = package();
    let part = wp_sign::sign(&mut package, &signer(&keys)).expect("signing");
    let text = package.xml_part(&part).expect("the signature").expect("text");
    let unreferenced = without(&text, PACKAGE_REFERENCE, "</Reference>");

    // The manifest still in the file, but no longer among what is signed.
    package.set_part(&part, signed_again(&unreferenced, &keys, false).into_bytes());
    let signature = &wp_sign::signatures(&package)[0];
    assert_eq!(signature.standing, Standing::CoversNothing, "an unsigned manifest was taken");
    assert!(signature.parts.is_empty(), "{:?}", signature.parts);
    // What it does sign is still read, and what it no longer signs is not:
    // the reason is in the Office object, which is still covered, and the
    // time is in the package object beside the manifest, which is not.
    assert_eq!(signature.reason, "Because it is mine");
    assert_eq!(signature.signed_at, "", "the time was read from an object nobody signed");

    // And not in the file at all.
    let gone = without(&unreferenced, r#"<Object Id="idPackageObject">"#, "</Object>");
    package.set_part(&part, signed_again(&gone, &keys, false).into_bytes());
    let signature = &wp_sign::signatures(&package)[0];
    assert_eq!(signature.standing, Standing::CoversNothing, "a signature over nothing held");
}

#[test]
fn an_identifier_used_twice_in_a_signature_is_refused() {
    // Which of the two was signed would be a matter of which one a reader
    // happens to look at first, and a signature that depends on that is not
    // one. On either side of the signed one, the answer is the same.
    let keys = keys("twice");
    let mut package = package();
    let part = wp_sign::sign(&mut package, &signer(&keys)).expect("signing");
    let text = package.xml_part(&part).expect("the signature").expect("text");
    let copy = r#"<Object Id="idPackageObject"><Manifest></Manifest></Object>"#;

    let after = text.replacen(
        r#"<Object Id="idOfficeObject">"#,
        &format!(r#"{copy}<Object Id="idOfficeObject">"#),
        1,
    );
    let before = text.replacen(
        r#"<Object Id="idPackageObject">"#,
        &format!(r#"{copy}<Object Id="idPackageObject">"#),
        1,
    );
    for (side, changed) in [("after", after), ("before", before)] {
        assert_ne!(changed, text);
        package.set_part(&part, changed.into_bytes());
        let signature = &wp_sign::signatures(&package)[0];
        assert_eq!(
            signature.standing,
            Standing::Ambiguous(String::from("idPackageObject")),
            "{side}: a doubled identifier was resolved"
        );
        // Refused before anything was read from either of them.
        assert!(signature.parts.is_empty(), "{side}: {:?}", signature.parts);
    }
}

#[test]
fn what_a_signature_says_about_itself_is_read_from_what_it_signed() {
    // A time, a reason and a line, written in an object nobody signed and
    // put first. The signature still holds — an object it does not cover may
    // be there, as its unsigned properties are — but what it says is what it
    // signed.
    let keys = keys("said");
    let mut package = package();
    let part = wp_sign::sign(&mut package, &signer(&keys)).expect("signing");
    let text = package.xml_part(&part).expect("the signature").expect("text");
    let forged = concat!(
        r#"<Object><SignatureProperties><SignatureProperty>"#,
        r#"<mdssi:SignatureTime"#,
        r#" xmlns:mdssi="http://schemas.openxmlformats.org/package/2006/digital-signature">"#,
        r#"<mdssi:Value>2001-01-01T00:00:00Z</mdssi:Value></mdssi:SignatureTime>"#,
        r#"</SignatureProperty><SignatureProperty>"#,
        r#"<SignatureInfoV1 xmlns="http://schemas.microsoft.com/office/2006/digsig">"#,
        r#"<SetupID>forged</SetupID><SignatureComments>Forged</SignatureComments>"#,
        r#"</SignatureInfoV1></SignatureProperty></SignatureProperties></Object>"#,
    );
    let changed = text.replacen(
        r#"<Object Id="idPackageObject">"#,
        &format!(r#"{forged}<Object Id="idPackageObject">"#),
        1,
    );
    assert_ne!(changed, text);
    package.set_part(&part, changed.into_bytes());

    let signature = &wp_sign::signatures(&package)[0];
    assert_eq!(signature.standing, Standing::Good, "{}", signature.standing.label());
    assert_eq!(signature.signed_at, "2026-09-16T12:00:00Z");
    assert_eq!(signature.reason, "Because it is mine");
    assert_eq!(signature.line, "");
}

#[test]
fn what_a_countersignature_says_is_read_from_what_it_signed() {
    let mine = keys("counter-said");
    let mut sealed = package();
    let part = wp_sign::sign(&mut sealed, &signer(&mine)).expect("signing");
    let mut witness = signer(&mine);
    witness.reason = String::from("Witnessed");
    witness.at = String::from("2026-09-17T09:00:00Z");
    wp_sign::countersign(&mut sealed, &part, &witness).expect("countersigning");

    // A role and a time nobody signed, put inside the countersignature ahead
    // of the properties it did sign.
    let text = sealed.xml_part(&part).expect("the part").expect("text");
    let theirs = concat!(
        r#"<Object><xd:QualifyingProperties"#,
        r#" xmlns:xd="http://uri.etsi.org/01903/v1.3.2#" Target="">"#,
    );
    let forged = concat!(
        r#"<Object><xd:SigningTime xmlns:xd="http://uri.etsi.org/01903/v1.3.2#">"#,
        r#"2001-01-01T00:00:00Z</xd:SigningTime>"#,
        r#"<xd:ClaimedRole xmlns:xd="http://uri.etsi.org/01903/v1.3.2#">Forged</xd:ClaimedRole>"#,
        r#"</Object>"#,
    );
    let changed = text.replacen(theirs, &format!("{forged}{theirs}"), 1);
    assert_ne!(changed, text, "the countersignature was not where it was looked for");
    sealed.set_part(&part, changed.into_bytes());

    let signatures = wp_sign::signatures(&sealed);
    let counter = &signatures[0].counters[0];
    assert_eq!(counter.standing, Standing::Good, "{:?}", counter.standing);
    assert_eq!(counter.role, "Witnessed");
    assert_eq!(counter.signed_at, "2026-09-17T09:00:00Z");
}

#[test]
fn a_countersignature_that_does_not_cover_the_signature_is_not_one() {
    // Made by the witness's own key and holding, but over nothing except
    // what it says about itself: it could be lifted into any document.
    let mine = keys("counter-nothing");
    let mut sealed = package();
    let part = wp_sign::sign(&mut sealed, &signer(&mine)).expect("signing");
    let mut witness = signer(&mine);
    witness.reason = String::from("Witnessed");
    wp_sign::countersign(&mut sealed, &part, &witness).expect("countersigning");

    let text = sealed.xml_part(&part).expect("the part").expect("text");
    let cut = without(
        &text,
        r#"<Reference Type="http://uri.etsi.org/01903#CountersignedSignature""#,
        "</Reference>",
    );
    sealed.set_part(&part, signed_again(&cut, &mine, true).into_bytes());

    let signatures = wp_sign::signatures(&sealed);
    assert_eq!(signatures[0].standing, Standing::Good, "the signature itself was not touched");
    let counter = &signatures[0].counters[0];
    assert_eq!(counter.standing, Standing::CoversNothing, "it held over nothing");
}

#[test]
fn a_certificate_is_known_by_the_same_fingerprint_another_program_gives_it() {
    // What a trusted publisher is remembered by, so it had better be the
    // number everybody else means by the words: OpenSSL's SHA-256
    // fingerprint, which it writes in capitals with colons between.
    let keys = keys("fingerprint");
    let output = run(Command::new("openssl")
        .args(["x509", "-noout", "-fingerprint", "-sha256", "-in"])
        .arg(keys.folder.join("cert.pem")));
    let said = String::from_utf8_lossy(&output.stdout);
    let theirs: String = said
        .split('=')
        .nth(1)
        .expect("a fingerprint")
        .trim()
        .chars()
        .filter(|character| *character != ':')
        .collect::<String>()
        .to_ascii_lowercase();
    let certificate = wp_asn1::Certificate::read(&keys.certificate).expect("a certificate");
    assert_eq!(wp_sign::fingerprint(&certificate), theirs);
    assert_eq!(theirs.len(), 64);
}

#[test]
fn what_a_signature_covers_is_asked_part_by_part_and_relationship_by_relationship() {
    let keys = keys("covers");
    let mut package = package();
    wp_sign::sign(&mut package, &signer(&keys)).expect("signing");
    let signature = &wp_sign::signatures(&package)[0];
    // The relationship to the signatures, which signing adds to the
    // package's own after the manifest is made, is the one the format leaves
    // out; everything else is covered.
    assert!(signature.covers_whole(&package), "a signature over everything was partial");
    assert!(signature.covers_part("word/document.xml"));
    assert!(signature.covers_part("/WORD/Document.xml"), "part names are not case-sensitive");
    let office = wp_opc::OFFICE_DOCUMENT_RELATIONSHIP;
    let root = package.relationships("").expect("the package's relationships");
    let main = root.by_type(office).next().expect("the main part's").id.clone();
    assert!(signature.covers_relationship("", &main));
    assert!(signature.covers_reached(&package, "", office));

    // A part added beside the signature, with a relationship to it. Nothing
    // that was signed changed, so the signature holds — for what it covers.
    package.add_part("word/added.bin", "application/octet-stream", b"added".to_vec());
    let mut root = package.relationships("").expect("the package's relationships");
    let added = root.add("urn:added", "word/added.bin", wp_opc::TargetMode::Internal).id.clone();
    package.set_relationships(&root).expect("writing them");

    let signature = &wp_sign::signatures(&package)[0];
    assert_eq!(signature.standing, Standing::Good, "{}", signature.standing.label());
    assert!(!signature.covers_whole(&package), "an added part was taken as signed");
    assert!(!signature.covers_part("word/added.bin"));
    assert!(!signature.covers_relationship("", &added), "an added relationship was taken");
    assert!(!signature.covers_reached(&package, "", "urn:added"));
    assert!(signature.covers_relationship("", &main), "what was signed stopped being");
    assert!(signature.covers_reached(&package, "", office));
}

#[test]
fn a_certificate_other_than_the_one_the_signature_names_does_not_hold() {
    // Another certificate for the same key, put where the signer's was. The
    // arithmetic still comes out, since the key is the same; what says it is
    // not the signer's is the digest of the signer's certificate, written
    // into what was signed.
    let keys = keys("other-certificate");
    let mut package = package();
    let part = wp_sign::sign(&mut package, &signer(&keys)).expect("signing");
    let at = |file: &str| keys.folder.join(file).to_str().expect("a path").to_owned();
    run(Command::new("openssl").args([
        "req",
        "-x509",
        "-new",
        "-key",
        &at("key.pem"),
        "-out",
        &at("other.pem"),
        "-days",
        "30",
        "-sha256",
        "-subj",
        "/CN=Somebody Else",
    ]));
    run(Command::new("openssl").args([
        "x509",
        "-in",
        &at("other.pem"),
        "-outform",
        "DER",
        "-out",
        &at("other.der"),
    ]));
    let other = std::fs::read(keys.folder.join("other.der")).expect("the other certificate");

    let text = package.xml_part(&part).expect("the signature").expect("text");
    let swapped = text.replacen(
        &wp_text::base64::encode(&keys.certificate),
        &wp_text::base64::encode(&other),
        1,
    );
    assert_ne!(swapped, text, "the certificate was not where it was looked for");
    package.set_part(&part, swapped.into_bytes());

    let signature = &wp_sign::signatures(&package)[0];
    assert_eq!(signature.certificate.subject, "CN=Somebody Else");
    assert_eq!(signature.standing, Standing::Broken, "a certificate it never named was taken");
}
