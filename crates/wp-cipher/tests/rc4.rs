//! RC4 against the standard's own test vectors and against OpenSSL.

use std::process::Command;

/// What OpenSSL makes of the same bytes under the same key.
fn openssl_rc4(key: &[u8], message: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let key = wp_hash::to_hex(key);
    // RC4 is in OpenSSL 3's legacy provider, which has to be asked for by
    // name - the default one will not do it, which is a fair comment on the
    // cipher and no help to somebody with a document encrypted in 2003.
    let mut child = Command::new("openssl")
        .args(["enc", "-rc4", "-K", &key, "-nopad", "-provider", "legacy", "-provider", "default"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("openssl is in the build image");
    child.stdin.as_mut().expect("its input").write_all(message).expect("writing");
    let output = child.wait_with_output().expect("its answer");
    output.stdout
}

#[test]
fn the_first_bytes_of_the_stream_are_what_the_standard_says() {
    // RFC 6229's own vectors: the key, and the first sixteen bytes of the
    // stream it makes, which is what enciphering sixteen noughts gives.
    let wanted: &[(&[u8], &str)] = &[
        (&[0x01, 0x02, 0x03, 0x04, 0x05], "b2396305f03dc027ccc3524a0a1118a8"),
        (
            &[0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a],
            "ede3b04643e586cc907dc21851709902",
        ),
        (
            &[
                0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
                0x0f, 0x10,
            ],
            "9ac7cc9a609d1ef7b2932899cde41b97",
        ),
    ];

    for (key, stream) in wanted {
        let out = wp_cipher::rc4(key, &[0u8; 16]);
        assert_eq!(wp_hash::to_hex(&out), *stream, "a key of {} bytes", key.len());
    }
}

#[test]
fn enciphering_and_deciphering_are_the_same_thing() {
    let key = b"a key of some length";
    let message = b"Yours faithfully, and a good deal more besides.";
    let enciphered = wp_cipher::rc4(key, message);
    assert_ne!(enciphered, message.to_vec(), "it did nothing");
    assert_eq!(wp_cipher::rc4(key, &enciphered), message.to_vec());
}

#[test]
fn what_openssl_enciphers_this_deciphers_and_the_other_way_about() {
    // Sixteen bytes, which is what `openssl enc -rc4` takes: a shorter one
    // is padded with noughts and then the two are not comparing the same key.
    let key = [
        0x01u8, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
        0x10,
    ];
    for length in [1usize, 15, 16, 17, 256, 257, 1000] {
        let message: Vec<u8> = (0..length).map(|at| (at % 251) as u8).collect();
        let theirs = openssl_rc4(&key, &message);
        assert_eq!(wp_cipher::rc4(&key, &message), theirs, "a message of {length} bytes");
        assert_eq!(wp_cipher::rc4(&key, &theirs), message, "and back again");
    }
}

#[test]
fn a_key_of_nothing_leaves_the_message_alone_rather_than_crashing() {
    // Not a key anybody should use; what matters is that it does not panic,
    // because the key comes out of a file somebody else wrote.
    let message = b"words";
    assert_eq!(wp_cipher::rc4(&[], message).len(), message.len());
}
