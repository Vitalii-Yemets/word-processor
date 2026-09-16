//! Signatures, checked against a program that had no part in writing them.
//!
//! OpenSSL is in the build image. It makes a key and a certificate, it signs,
//! and it checks. Every one of those is done both ways round here: what this
//! program signs, OpenSSL must accept, and what OpenSSL signs, this program
//! must accept — and both must refuse a message that has been changed since.
//!
//! That is the only kind of proof a signature scheme can have. A test that
//! checks its own signatures proves that the arithmetic is consistent with
//! itself, which a scheme with the padding written backwards would also pass.

use std::path::{Path, PathBuf};
use std::process::Command;

use wp_rsa::{Algorithm, PrivateKey, PublicKey};

/// A key and a self-signed certificate, made by OpenSSL.
struct Signer {
    folder: PathBuf,
    certificate: Vec<u8>,
    key: Vec<u8>,
}

impl Drop for Signer {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.folder);
    }
}

fn run(command: &mut Command) -> std::process::Output {
    let output = command.output().expect("openssl is in the build image");
    assert!(
        output.status.success(),
        "{:?} said {}",
        command,
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn signer(name: &str) -> Signer {
    let folder = std::env::temp_dir().join(format!("wp-rsa-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).expect("a folder to work in");

    let at = |file: &str| folder.join(file);
    run(Command::new("openssl").args([
        "req",
        "-x509",
        "-newkey",
        "rsa:2048",
        "-keyout",
        at("key.pem").to_str().expect("a path"),
        "-out",
        at("cert.pem").to_str().expect("a path"),
        "-days",
        "3650",
        "-nodes",
        "-sha256",
        "-subj",
        "/C=GB/O=Nobody in Particular/CN=A Signer",
    ]));
    run(Command::new("openssl").args([
        "x509",
        "-in",
        at("cert.pem").to_str().expect("a path"),
        "-outform",
        "DER",
        "-out",
        at("cert.der").to_str().expect("a path"),
    ]));
    run(Command::new("openssl").args([
        "pkey",
        "-in",
        at("key.pem").to_str().expect("a path"),
        "-outform",
        "DER",
        "-out",
        at("key.der").to_str().expect("a path"),
    ]));
    // The public half on its own, which is what checking a signature with
    // OpenSSL wants.
    let public = run(Command::new("openssl").args([
        "pkey",
        "-in",
        at("key.pem").to_str().expect("a path"),
        "-pubout",
    ]));
    std::fs::write(at("pub.pem"), public.stdout).expect("writing");

    Signer {
        certificate: std::fs::read(at("cert.der")).expect("the certificate"),
        key: std::fs::read(at("key.der")).expect("the key"),
        folder,
    }
}

/// Whether OpenSSL accepts a signature over a file.
fn openssl_accepts(folder: &Path, message: &Path, signature: &Path) -> bool {
    let output = Command::new("openssl")
        .args(["dgst", "-sha256", "-verify"])
        .arg(folder.join("pub.pem"))
        .arg("-signature")
        .arg(signature)
        .arg(message)
        .output()
        .expect("openssl is in the build image");
    String::from_utf8_lossy(&output.stdout).contains("Verified OK")
}

#[test]
fn a_certificate_openssl_wrote_reads_back_as_what_was_asked_for() {
    let signer = signer("certificate");
    let certificate = wp_asn1::Certificate::read(&signer.certificate).expect("a certificate");

    assert_eq!(certificate.subject, "CN=A Signer, O=Nobody in Particular, C=GB");
    assert_eq!(certificate.issuer, certificate.subject, "it signed itself");
    assert!(certificate.not_before < certificate.not_after);
    assert_eq!(certificate.not_before.len(), 20, "{}", certificate.not_before);
    assert!(!certificate.serial.is_empty());
    // Two thousand and forty-eight bits, and the exponent everybody uses.
    assert_eq!(certificate.key.modulus.len(), 256);
    assert_eq!(certificate.key.exponent, vec![0x01, 0x00, 0x01]);
    assert_eq!(certificate.der, signer.certificate);
}

#[test]
fn the_key_in_the_certificate_is_the_key_in_the_file() {
    let signer = signer("halves");
    let certificate = wp_asn1::Certificate::read(&signer.certificate).expect("a certificate");
    let private = wp_asn1::private_key(&signer.key).expect("a private key");
    assert_eq!(certificate.key.modulus, private.modulus);
    assert_ne!(certificate.key.exponent, private.exponent);
}

#[test]
fn what_this_program_signs_openssl_accepts() {
    let signer = signer("signing");
    let private = wp_asn1::private_key(&signer.key).expect("a private key");
    let key = PrivateKey::new(&private.modulus, &private.exponent);

    for (name, message) in [
        ("short", b"x".to_vec()),
        ("a sentence", b"The quick brown fox jumps over the lazy dog".to_vec()),
        ("nothing at all", Vec::new()),
        ("a long one", vec![0x5a; 100_000]),
    ] {
        let signature = key.sign(Algorithm::Sha256, &message).expect("a signature");
        assert_eq!(signature.len(), 256, "{name}");

        let message_path = signer.folder.join("message.bin");
        let signature_path = signer.folder.join("signature.bin");
        std::fs::write(&message_path, &message).expect("writing");
        std::fs::write(&signature_path, &signature).expect("writing");
        assert!(
            openssl_accepts(&signer.folder, &message_path, &signature_path),
            "OpenSSL refused a signature over {name}"
        );

        // And a message changed since is refused.
        let mut changed = message.clone();
        changed.push(b'!');
        std::fs::write(&message_path, &changed).expect("writing");
        assert!(
            !openssl_accepts(&signer.folder, &message_path, &signature_path),
            "OpenSSL accepted a signature over {name} after it was changed"
        );
    }
}

#[test]
fn what_openssl_signs_this_program_accepts() {
    let signer = signer("checking");
    let certificate = wp_asn1::Certificate::read(&signer.certificate).expect("a certificate");
    let key = PublicKey::new(&certificate.key.modulus, &certificate.key.exponent);
    assert_eq!(key.bits(), 2048);

    for (algorithm, flag) in
        [(Algorithm::Sha1, "-sha1"), (Algorithm::Sha256, "-sha256"), (Algorithm::Sha512, "-sha512")]
    {
        let message = b"Sphinx of black quartz, judge my vow.".to_vec();
        let message_path = signer.folder.join("message.bin");
        let signature_path = signer.folder.join("signature.bin");
        std::fs::write(&message_path, &message).expect("writing");
        run(Command::new("openssl")
            .args(["dgst", flag, "-sign"])
            .arg(signer.folder.join("key.pem"))
            .arg("-out")
            .arg(&signature_path)
            .arg(&message_path));
        let signature = std::fs::read(&signature_path).expect("the signature");

        assert!(key.verifies(algorithm, &message, &signature), "{flag}");
        assert!(
            !key.verifies(algorithm, b"Sphinx of black quartz, judge my cow.", &signature),
            "{flag}: a changed message was accepted"
        );
        // And the wrong hash must not do: what makes the wrapper worth
        // writing is that a signature names which hash it was over.
        let other =
            if algorithm == Algorithm::Sha256 { Algorithm::Sha512 } else { Algorithm::Sha256 };
        assert!(!key.verifies(other, &message, &signature), "{flag}: the wrong hash was accepted");

        // A signature with a byte changed is refused.
        let mut broken = signature.clone();
        broken[100] ^= 1;
        assert!(!key.verifies(algorithm, &message, &broken), "{flag}: a changed signature passed");
    }
}

#[test]
fn a_signature_from_another_key_is_refused() {
    let mine = signer("mine");
    let theirs = signer("theirs");
    let message = b"pay the bearer ten pounds".to_vec();

    let private = wp_asn1::private_key(&theirs.key).expect("a private key");
    let signature = PrivateKey::new(&private.modulus, &private.exponent)
        .sign(Algorithm::Sha256, &message)
        .expect("a signature");

    let certificate = wp_asn1::Certificate::read(&mine.certificate).expect("a certificate");
    let key = PublicKey::new(&certificate.key.modulus, &certificate.key.exponent);
    assert!(!key.verifies(Algorithm::Sha256, &message, &signature));
}
