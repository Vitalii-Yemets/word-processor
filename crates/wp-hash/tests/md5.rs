//! MD5 against the standard's own examples and against OpenSSL.

use std::process::Command;

/// What OpenSSL makes of the same bytes, which is the check that matters:
/// nothing here had any part in writing this program.
fn openssl_md5(message: &[u8]) -> String {
    use std::io::Write;
    let mut child = Command::new("openssl")
        .args(["dgst", "-md5", "-r"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("openssl is in the build image");
    child.stdin.as_mut().expect("its input").write_all(message).expect("writing");
    let output = child.wait_with_output().expect("its answer");
    String::from_utf8_lossy(&output.stdout).split_whitespace().next().unwrap_or_default().to_owned()
}

#[test]
fn the_examples_in_the_standard_come_out_as_the_standard_says() {
    // RFC 1321's own seven, which is the list every implementation is held
    // to.
    let wanted: &[(&str, &str)] = &[
        ("", "d41d8cd98f00b204e9800998ecf8427e"),
        ("a", "0cc175b9c0f1b6a831c399e269772661"),
        ("abc", "900150983cd24fb0d6963f7d28e17f72"),
        ("message digest", "f96b697d7cb7938d525a2f31aaf161d0"),
        ("abcdefghijklmnopqrstuvwxyz", "c3fcd3d76192e4007dfb496cca67e13b"),
        (
            "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789",
            "d174ab98d277d9f5a5611c2c9f419d9f",
        ),
        (
            "12345678901234567890123456789012345678901234567890123456789012345678901234567890",
            "57edf4a22be3c955ac49da2e2107b67a",
        ),
    ];

    for (message, digest) in wanted {
        assert_eq!(wp_hash::to_hex(&wp_hash::md5(message.as_bytes())), *digest, "{message:?}");
    }
}

#[test]
fn every_awkward_length_comes_out_as_openssl_says() {
    // The lengths where the padding is decided: just under a block, exactly
    // a block, and the one where the length itself needs a block of its own.
    for length in [0usize, 1, 55, 56, 57, 63, 64, 65, 119, 120, 128, 1000] {
        let message: Vec<u8> = (0..length).map(|at| (at % 251) as u8).collect();
        assert_eq!(
            wp_hash::to_hex(&wp_hash::md5(&message)),
            openssl_md5(&message),
            "a message of {length} bytes"
        );
    }
}
