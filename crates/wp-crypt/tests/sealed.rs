//! Opening and writing the encrypted documents Office makes.
//!
//! The document written by hand here is the 2007 one: its description is a
//! fixed run of bytes rather than XML, so a test can lay it out and do the
//! key's arithmetic itself, outside the code being tested. That is the
//! nearest thing to a document from another program that can be had without
//! one.

use wp_crypt::{Error, Fresh};

/// A package worth encrypting: long enough to cross the four-thousand-byte
/// line a piece of an encrypted package ends at, and recognisable in a heap
/// of bytes.
fn package() -> Vec<u8> {
    let mut out = b"PK\x03\x04 The quick brown fox jumps over the lazy dog. ".to_vec();
    while out.len() < 10_000 {
        out.extend_from_slice(b"Sphinx of black quartz, judge my vow. ");
    }
    out
}

fn fresh() -> Fresh {
    Fresh {
        package_salt: *b"sixteen bytes!!!",
        password_salt: *b"another sixteen!",
        key: *b"thirty-two bytes of key, exactly",
        hmac_key: [0x5a; 64],
        verifier: *b"and sixteen more",
    }
}

#[test]
fn what_was_sealed_opens_with_the_password() {
    let package = package();
    let sealed = wp_crypt::seal(&package, "Fenchurch St Paul", &fresh());

    assert!(wp_crypt::is_encrypted(&sealed));
    assert_eq!(wp_crypt::open(&sealed, "Fenchurch St Paul"), Ok(package));
}

#[test]
fn a_package_of_any_length_comes_back_the_length_it_went_in() {
    // On both sides of a block and of a whole piece, which is where a
    // padding mistake shows.
    for length in [0usize, 1, 15, 16, 17, 4095, 4096, 4097, 8192, 8193] {
        let package: Vec<u8> = (0..length).map(|at| (at * 7) as u8).collect();
        let sealed = wp_crypt::seal(&package, "secret", &fresh());
        assert_eq!(wp_crypt::open(&sealed, "secret"), Ok(package), "a package of {length} bytes");
    }
}

#[test]
fn a_wrong_password_does_not_open_it() {
    let sealed = wp_crypt::seal(&package(), "Fenchurch", &fresh());
    assert_eq!(wp_crypt::open(&sealed, "fenchurch"), Err(Error::WrongPassword));
    assert_eq!(wp_crypt::open(&sealed, ""), Err(Error::WrongPassword));
    assert_eq!(wp_crypt::open(&sealed, "Fenchurch "), Err(Error::WrongPassword));
}

#[test]
fn neither_the_document_nor_the_password_is_in_the_file() {
    let sealed = wp_crypt::seal(&package(), "Fenchurch", &fresh());
    for needle in [&b"Fenchurch"[..], b"quick brown fox", b"Sphinx of black quartz"] {
        assert!(
            !sealed.windows(needle.len()).any(|window| window == needle),
            "{} is in the sealed file",
            String::from_utf8_lossy(needle)
        );
    }
}

#[test]
fn a_byte_changed_by_somebody_who_could_not_read_it_is_noticed() {
    let sealed = wp_crypt::seal(&package(), "Fenchurch", &fresh());
    let file = wp_ole::CompoundFile::open(sealed).expect("a compound file");
    let info = file.stream("EncryptionInfo").expect("the description");
    let mut enciphered = file.stream("EncryptedPackage").expect("the document");

    // Well past the eight bytes of length at the front, so that what is
    // changed is the cipher text itself.
    enciphered[2000] ^= 1;
    let mut builder = wp_ole::Builder::new();
    builder.stream("EncryptionInfo", info);
    builder.stream("EncryptedPackage", enciphered);

    assert_eq!(wp_crypt::open(&builder.build(), "Fenchurch"), Err(Error::Tampered));
}

#[test]
fn something_that_is_not_an_encrypted_document_says_so() {
    assert_eq!(wp_crypt::open(b"PK\x03\x04 an ordinary package", "x"), Err(Error::NotEncrypted));
    assert!(!wp_crypt::is_encrypted(b"PK\x03\x04 an ordinary package"));
}

#[test]
fn a_scheme_this_program_has_not_got_is_named_rather_than_guessed_at() {
    for (version, expected) in [
        ([2u8, 0, 1, 0], "the RC4 cipher"),
        ([4, 0, 3, 0], "a key from a rights server"),
        ([9, 0, 9, 0], "version 9.9 of the encryption"),
    ] {
        let mut info = version.to_vec();
        info.extend_from_slice(&[0; 4]);
        let mut builder = wp_ole::Builder::new();
        builder.stream("EncryptionInfo", info);
        builder.stream("EncryptedPackage", vec![0; 64]);
        assert_eq!(
            wp_crypt::open(&builder.build(), "x"),
            Err(Error::Unsupported(expected.to_owned())),
        );
    }
}

// --- A document written the way Office 2007 wrote one -------------------------

/// The key that scheme makes out of a password, worked out here so that the
/// reader is held to arithmetic done outside it.
fn key_of_2007(password: &str, salt: &[u8], key_bytes: usize) -> Vec<u8> {
    let mut first = salt.to_vec();
    for unit in password.encode_utf16() {
        first.extend_from_slice(&unit.to_le_bytes());
    }
    let mut hash = wp_hash::sha1(&first).to_vec();
    for turn in 0..50_000u32 {
        let mut next = turn.to_le_bytes().to_vec();
        next.extend_from_slice(&hash);
        hash = wp_hash::sha1(&next).to_vec();
    }
    hash.extend_from_slice(&0u32.to_le_bytes());
    let hash = wp_hash::sha1(&hash);

    let mut inner = [0x36u8; 64];
    let mut outer = [0x5Cu8; 64];
    for (at, byte) in hash.iter().enumerate() {
        inner[at] ^= byte;
        outer[at] ^= byte;
    }
    let mut both = wp_hash::sha1(&inner).to_vec();
    both.extend_from_slice(&wp_hash::sha1(&outer));
    both.truncate(key_bytes);
    both
}

/// Lays out the two streams Office 2007 wrote, by hand.
fn document_of_2007(package: &[u8], password: &str) -> Vec<u8> {
    const SALT: &[u8; 16] = b"a salt of theirs";
    const VERIFIER: &[u8; 16] = b"sixteen bytes!!!";
    let key = wp_cipher::Key::new(&key_of_2007(password, SALT, 16)).expect("a key");

    let mut header = Vec::new();
    header.extend_from_slice(&0x24u32.to_le_bytes()); // Flags: AES, through the API.
    header.extend_from_slice(&0u32.to_le_bytes()); // Nothing extra.
    header.extend_from_slice(&0x660Eu32.to_le_bytes()); // AES with a short key.
    header.extend_from_slice(&0x8004u32.to_le_bytes()); // Hashed with SHA-1.
    header.extend_from_slice(&128u32.to_le_bytes()); // Which is sixteen bytes.
    header.extend_from_slice(&0x18u32.to_le_bytes()); // The provider's kind.
    header.extend_from_slice(&[0; 8]); // Two fields kept for nothing.
    for unit in "Microsoft Enhanced RSA and AES Cryptographic Provider".encode_utf16() {
        header.extend_from_slice(&unit.to_le_bytes());
    }
    header.extend_from_slice(&[0, 0]);

    let mut hash = wp_hash::sha1(VERIFIER).to_vec();
    hash.resize(32, 0);

    let mut info = vec![3, 0, 2, 0]; // Version three point two.
    info.extend_from_slice(&0x24u32.to_le_bytes());
    info.extend_from_slice(&(header.len() as u32).to_le_bytes());
    info.extend_from_slice(&header);
    info.extend_from_slice(&16u32.to_le_bytes());
    info.extend_from_slice(SALT);
    info.extend_from_slice(&wp_cipher::encrypt_ecb(&key, VERIFIER));
    info.extend_from_slice(&20u32.to_le_bytes());
    info.extend_from_slice(&wp_cipher::encrypt_ecb(&key, &hash));

    let mut whole = package.to_vec();
    whole.resize(package.len().div_ceil(16) * 16, 0);
    let mut enciphered = (package.len() as u64).to_le_bytes().to_vec();
    enciphered.extend_from_slice(&wp_cipher::encrypt_ecb(&key, &whole));

    let mut builder = wp_ole::Builder::new();
    builder.stream("EncryptionInfo", info);
    builder.stream("EncryptedPackage", enciphered);
    builder.build()
}

#[test]
fn a_document_encrypted_the_way_office_2007_did_it_opens() {
    let package = package();
    let document = document_of_2007(&package, "Trumpington");

    assert!(wp_crypt::is_encrypted(&document));
    assert_eq!(wp_crypt::open(&document, "Trumpington"), Ok(package));
    assert_eq!(wp_crypt::open(&document, "Trumpingtom"), Err(Error::WrongPassword));
}
