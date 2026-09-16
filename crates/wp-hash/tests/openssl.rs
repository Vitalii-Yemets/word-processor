//! The hashes, held to somebody else's arithmetic.
//!
//! The standard's own vectors are beside the code, which is where a vector
//! belongs. This is the other kind of check: the same bytes put through
//! OpenSSL, which is in the build image and had no part in writing this. A
//! table of constants copied wrongly passes a test written from the same
//! wrong table; it does not pass this.

use std::io::Write;
use std::process::{Command, Stdio};

/// What OpenSSL makes of some bytes.
fn openssl(algorithm: &str, bytes: &[u8]) -> String {
    let mut child = Command::new("openssl")
        .args(["dgst", algorithm, "-r"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("openssl is in the build image");
    child.stdin.take().expect("its input").write_all(bytes).expect("writing");
    let output = child.wait_with_output().expect("waiting");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    // `-r` writes the hash, a space, and where it came from.
    String::from_utf8_lossy(&output.stdout).split_whitespace().next().unwrap_or_default().to_owned()
}

/// Messages of every awkward length: side by side with the block boundary,
/// and on both sides of the point where the padding needs a block of its own.
fn messages() -> Vec<Vec<u8>> {
    let mut out = vec![Vec::new()];
    for length in [1usize, 55, 56, 57, 63, 64, 65, 111, 112, 113, 127, 128, 129, 1000] {
        out.push((0..length).map(|at| (at * 37 + 11) as u8).collect());
    }
    out
}

#[test]
fn every_hash_agrees_with_openssl() {
    for message in messages() {
        assert_eq!(
            wp_hash::to_hex(&wp_hash::sha1(&message)),
            openssl("-sha1", &message),
            "SHA-1 of {} bytes",
            message.len()
        );
        assert_eq!(
            wp_hash::to_hex(&wp_hash::sha256(&message)),
            openssl("-sha256", &message),
            "SHA-256 of {} bytes",
            message.len()
        );
        assert_eq!(
            wp_hash::to_hex(&wp_hash::sha512(&message)),
            openssl("-sha512", &message),
            "SHA-512 of {} bytes",
            message.len()
        );
    }
}

#[test]
fn the_authenticated_message_agrees_with_openssl_too() {
    for message in messages() {
        for key in [&b"key"[..], &[0x0b; 20], &[0xaa; 200]] {
            let mut child = Command::new("openssl")
                .args(["dgst", "-sha512", "-r", "-mac", "HMAC", "-macopt"])
                .arg(format!("hexkey:{}", wp_hash::to_hex(key)))
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .expect("openssl is in the build image");
            child.stdin.take().expect("its input").write_all(&message).expect("writing");
            let output = child.wait_with_output().expect("waiting");
            assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
            let theirs = String::from_utf8_lossy(&output.stdout)
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .to_owned();
            assert_eq!(
                wp_hash::to_hex(&wp_hash::hmac_sha512(key, &message)),
                theirs,
                "a key of {} bytes over a message of {}",
                key.len(),
                message.len()
            );
        }
    }
}
