//! Encrypted documents laid out by hand from the specification.
//!
//! # Why this exists beside the round trip
//!
//! Because a round trip proves that what one half of this crate writes, the
//! other half reads — and would go on proving it if both halves had the same
//! mistake in them. So the arithmetic is done here too, out of the
//! specification and not out of the code being tested, and the file is put
//! together byte by byte.
//!
//! It is also the only way to reach the descriptions this program writes but
//! never produces: SHA-1 instead of SHA-512, a key of a hundred and
//! twenty-eight bits, a spin count that is not Office's, and a document with
//! no integrity check on it at all. Files like that exist, and a program that
//! only ever read its own output would meet the first one in the wild.

use wp_crypt::Error;

const VERIFIER_INPUT: [u8; 8] = [0xfe, 0xa7, 0xd2, 0x76, 0x3b, 0x4b, 0x9e, 0x79];
const VERIFIER_VALUE: [u8; 8] = [0xd7, 0xaa, 0x0f, 0x6d, 0x30, 0x61, 0x34, 0x4e];
const KEY_VALUE: [u8; 8] = [0x14, 0x6e, 0x0b, 0xe7, 0xab, 0xac, 0xd0, 0xd6];
const HMAC_KEY: [u8; 8] = [0x5f, 0xb2, 0xad, 0x01, 0x0c, 0xb9, 0xe1, 0xf6];
const HMAC_VALUE: [u8; 8] = [0xa0, 0x67, 0x7f, 0x02, 0xb2, 0x2c, 0x84, 0x33];

/// How the description is to be laid out.
#[derive(Clone, Copy)]
struct Shape {
    sha512: bool,
    key_bytes: usize,
    spins: u32,
    integrity: bool,
}

fn hash(shape: Shape, bytes: &[u8]) -> Vec<u8> {
    if shape.sha512 {
        wp_hash::sha512(bytes).to_vec()
    } else {
        wp_hash::sha1(bytes).to_vec()
    }
}

fn hash_of_two(shape: Shape, first: &[u8], second: &[u8]) -> Vec<u8> {
    let mut both = first.to_vec();
    both.extend_from_slice(second);
    hash(shape, &both)
}

fn utf16(password: &str) -> Vec<u8> {
    password.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

fn fitted(bytes: &[u8], length: usize) -> Vec<u8> {
    let mut out = bytes.to_vec();
    out.resize(length, 0x36);
    out
}

fn sixteen(bytes: &[u8]) -> [u8; 16] {
    let mut out = [0u8; 16];
    out.copy_from_slice(&bytes[..16]);
    out
}

/// The hash a password comes to: the salt and the password, then the turn's
/// number before each following hash.
fn hashed(shape: Shape, password: &str, salt: &[u8]) -> Vec<u8> {
    let mut hash = hash_of_two(shape, salt, &utf16(password));
    for turn in 0..shape.spins {
        hash = hash_of_two(shape, &turn.to_le_bytes(), &hash);
    }
    hash
}

fn key_for(shape: Shape, hashed: &[u8], block: &[u8; 8]) -> wp_cipher::Key {
    wp_cipher::Key::new(&fitted(&hash_of_two(shape, hashed, block), shape.key_bytes))
        .expect("a key")
}

/// An encrypted document of the given shape, holding the given package.
fn document(shape: Shape, package: &[u8], password: &str) -> Vec<u8> {
    const DATA_SALT: &[u8; 16] = b"a salt for bytes";
    const PASSWORD_SALT: &[u8; 16] = b"a salt for words";
    const VERIFIER: &[u8; 16] = b"sixteen bytes!!!";
    let key: Vec<u8> = (0..shape.key_bytes).map(|at| (at * 11 + 5) as u8).collect();
    let secret = wp_cipher::Key::new(&key).expect("a key");

    // The package, in pieces of four thousand and ninety-six, each chained
    // from a vector made out of the salt and the piece's number.
    let mut enciphered = (package.len() as u64).to_le_bytes().to_vec();
    for (number, piece) in package.chunks(4096).enumerate() {
        let start =
            sixteen(&fitted(&hash_of_two(shape, DATA_SALT, &(number as u32).to_le_bytes()), 16));
        let mut whole = piece.to_vec();
        whole.resize(piece.len().div_ceil(16) * 16, 0);
        enciphered.extend_from_slice(&wp_cipher::encrypt_cbc(&secret, &start, &whole));
    }

    let hashed = hashed(shape, password, PASSWORD_SALT);
    let start = *PASSWORD_SALT;
    let verifier_hash = fitted(&hash(shape, VERIFIER), hash(shape, b"").len().div_ceil(16) * 16);
    let encrypted_input =
        wp_cipher::encrypt_cbc(&key_for(shape, &hashed, &VERIFIER_INPUT), &start, VERIFIER);
    let encrypted_value =
        wp_cipher::encrypt_cbc(&key_for(shape, &hashed, &VERIFIER_VALUE), &start, &verifier_hash);
    let encrypted_key = wp_cipher::encrypt_cbc(&key_for(shape, &hashed, &KEY_VALUE), &start, &key);

    let base64 = |bytes: &[u8]| wp_text::base64::encode(bytes);
    let integrity = if shape.integrity {
        let hmac_key = [0x7u8; 64];
        let hmac = wp_hash::hmac_sha512(&hmac_key, &enciphered);
        let start_of =
            |block: &[u8; 8]| sixteen(&fitted(&hash_of_two(shape, DATA_SALT, block), 16));
        format!(
            r#"<dataIntegrity encryptedHmacKey="{}" encryptedHmacValue="{}"/>"#,
            base64(&wp_cipher::encrypt_cbc(&secret, &start_of(&HMAC_KEY), &hmac_key)),
            base64(&wp_cipher::encrypt_cbc(&secret, &start_of(&HMAC_VALUE), &hmac)),
        )
    } else {
        String::new()
    };

    let named = if shape.sha512 { "SHA512" } else { "SHA1" };
    let hash_size = hash(shape, b"").len();
    let description = format!(
        concat!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
            r#"<encryption xmlns="http://schemas.microsoft.com/office/2006/encryption""#,
            r#" xmlns:p="http://schemas.microsoft.com/office/2006/keyEncryptor/password">"#,
            r#"<keyData saltSize="16" blockSize="16" keyBits="{bits}" hashSize="{hash_size}""#,
            r#" cipherAlgorithm="AES" cipherChaining="ChainingModeCBC""#,
            r#" hashAlgorithm="{named}" saltValue="{data_salt}"/>{integrity}"#,
            r#"<keyEncryptors><keyEncryptor"#,
            r#" uri="http://schemas.microsoft.com/office/2006/keyEncryptor/password">"#,
            r#"<p:encryptedKey spinCount="{spins}" saltSize="16" blockSize="16""#,
            r#" keyBits="{bits}" hashSize="{hash_size}" cipherAlgorithm="AES""#,
            r#" cipherChaining="ChainingModeCBC" hashAlgorithm="{named}""#,
            r#" saltValue="{password_salt}" encryptedVerifierHashInput="{input}""#,
            r#" encryptedVerifierHashValue="{value}" encryptedKeyValue="{secret}"/>"#,
            r#"</keyEncryptor></keyEncryptors></encryption>"#,
        ),
        bits = shape.key_bytes * 8,
        hash_size = hash_size,
        named = named,
        data_salt = base64(DATA_SALT),
        integrity = integrity,
        spins = shape.spins,
        password_salt = base64(PASSWORD_SALT),
        input = base64(&encrypted_input),
        value = base64(&encrypted_value),
        secret = base64(&encrypted_key),
    );

    let mut info = vec![4, 0, 4, 0, 0x40, 0, 0, 0];
    info.extend_from_slice(description.as_bytes());

    let mut builder = wp_ole::Builder::new();
    builder.stream("EncryptionInfo", info);
    builder.stream("EncryptedPackage", enciphered);
    builder.build()
}

fn package() -> Vec<u8> {
    let mut out = b"PK\x03\x04".to_vec();
    while out.len() < 9000 {
        out.extend_from_slice(b"Sphinx of black quartz, judge my vow. ");
    }
    out
}

#[test]
fn the_shape_office_writes_now() {
    let shape = Shape { sha512: true, key_bytes: 32, spins: 1000, integrity: true };
    let package = package();
    let document = document(shape, &package, "Fenchurch");

    assert!(wp_crypt::is_encrypted(&document));
    assert_eq!(wp_crypt::open(&document, "Fenchurch"), Ok(package));
    assert_eq!(wp_crypt::open(&document, "Fenchurcj"), Err(Error::WrongPassword));
}

/// The parameters this program never writes: the older hash, the shorter key,
/// and a spin count that is nobody's default — so that the count is read out
/// of the file rather than assumed.
#[test]
fn the_shapes_this_program_reads_but_does_not_write() {
    for shape in [
        Shape { sha512: false, key_bytes: 16, spins: 500, integrity: false },
        Shape { sha512: false, key_bytes: 32, spins: 1, integrity: false },
        Shape { sha512: true, key_bytes: 16, spins: 0, integrity: true },
        Shape { sha512: true, key_bytes: 24, spins: 100, integrity: false },
    ] {
        let package = package();
        let document = document(shape, &package, "Trumpington");
        assert_eq!(
            wp_crypt::open(&document, "Trumpington"),
            Ok(package),
            "a {} bit key hashed with {} over {} turns",
            shape.key_bytes * 8,
            if shape.sha512 { "SHA-512" } else { "SHA-1" },
            shape.spins,
        );
        assert_eq!(wp_crypt::open(&document, "Trumpingtom"), Err(Error::WrongPassword));
    }
}

#[test]
fn a_cipher_this_program_has_not_got_is_named_rather_than_guessed_at() {
    let shape = Shape { sha512: true, key_bytes: 32, spins: 10, integrity: false };
    let document = document(shape, &package(), "Fenchurch");
    let file = wp_ole::CompoundFile::open(document).expect("a compound file");
    let info = String::from_utf8(file.stream("EncryptionInfo").expect("the description"))
        .expect("the description is text after its first eight bytes");

    for (was, now, complaint) in [
        (r#"cipherAlgorithm="AES""#, r#"cipherAlgorithm="RC2""#, "the RC2 cipher"),
        (
            r#"cipherChaining="ChainingModeCBC""#,
            r#"cipherChaining="ChainingModeCFB""#,
            "ChainingModeCFB",
        ),
        (r#"hashAlgorithm="SHA512""#, r#"hashAlgorithm="MD5""#, "the MD5 hash"),
    ] {
        let mut builder = wp_ole::Builder::new();
        builder.stream("EncryptionInfo", info.replacen(was, now, 1).into_bytes());
        builder.stream("EncryptedPackage", file.stream("EncryptedPackage").expect("the document"));
        assert_eq!(
            wp_crypt::open(&builder.build(), "Fenchurch"),
            Err(Error::Unsupported(complaint.to_owned())),
        );
    }
}
