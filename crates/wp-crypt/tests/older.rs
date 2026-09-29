//! The encryption Office used before 2007, and the storage that says what was
//! done to a package.
//!
//! The files are built here rather than found: there is no Word in the build
//! image and no corpus of encrypted documents in the repository, so what holds
//! this to the specification is that each file is assembled by hand, byte by
//! byte, from the specification's own description — and then opened by code
//! that had no part in assembling it.

use wp_ole::{Builder, CompoundFile, Item};

/// A password to open, and a document to find inside.
const PASSWORD: &str = "Fenchurch";
const DOCUMENT: &[u8] = b"PK\x03\x04 and the rest of a perfectly ordinary zip";

/// The storage's name, which begins with a control character.
const SPACES: &str = "\u{6}DataSpaces";

/// A version, a header and a verifier, as the standard scheme writes them.
///
/// `algorithm` is the number the header gives the cipher and `key_bits` how
/// long the key is; the rest is what every one of these files says.
fn standard_info(algorithm: u32, key_bits: u32, salt: &[u8], verifier: &[u8]) -> Vec<u8> {
    let mut header = Vec::new();
    header.extend_from_slice(&0x24u32.to_le_bytes());
    header.extend_from_slice(&0u32.to_le_bytes());
    header.extend_from_slice(&algorithm.to_le_bytes());
    header.extend_from_slice(&0x8004u32.to_le_bytes());
    header.extend_from_slice(&key_bits.to_le_bytes());
    header.extend_from_slice(&0u32.to_le_bytes());
    header.extend_from_slice(&[0u8; 8]);
    header.extend_from_slice(&[0u8; 2]);

    let mut out = Vec::new();
    out.extend_from_slice(&3u16.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&0x24u32.to_le_bytes());
    out.extend_from_slice(&(header.len() as u32).to_le_bytes());
    out.extend_from_slice(&header);
    out.extend_from_slice(&(salt.len() as u32).to_le_bytes());
    out.extend_from_slice(salt);
    out.extend_from_slice(verifier);
    out
}

/// What the RC4 CryptoAPI scheme makes of a password and a salt, worked out
/// here from the specification rather than asked of the code being tested.
fn cryptoapi_key(password: &str, salt: &[u8], block: u32, key_bytes: usize) -> Vec<u8> {
    let mut first = salt.to_vec();
    for unit in password.encode_utf16() {
        first.extend_from_slice(&unit.to_le_bytes());
    }
    let base = wp_hash::sha1(&first);

    let mut with_block = base.to_vec();
    with_block.extend_from_slice(&block.to_le_bytes());
    let hash = wp_hash::sha1(&with_block);
    if key_bytes == 5 {
        let mut key = vec![0u8; 16];
        key[..5].copy_from_slice(&hash[..5]);
        return key;
    }
    hash[..key_bytes].to_vec()
}

/// A whole file encrypted the way Office did between 2002 and 2007.
fn cryptoapi_file(key_bits: u32) -> Vec<u8> {
    let salt = [0x11u8; 16];
    let key_bytes = (key_bits / 8) as usize;

    // The verifier: sixteen bytes and their hash, enciphered together, and
    // written with the hash's length between them.
    let plain = [0x22u8; 16];
    let mut both = plain.to_vec();
    both.extend_from_slice(&wp_hash::sha1(&plain));
    wp_cipher::Rc4::new(&cryptoapi_key(PASSWORD, &salt, 0, key_bytes)).apply(&mut both);

    let mut verifier = both[..16].to_vec();
    verifier.extend_from_slice(&20u32.to_le_bytes());
    verifier.extend_from_slice(&both[16..]);

    // The package: its length, then the document in blocks of five hundred
    // and twelve, each under a key of its own.
    let mut package = (DOCUMENT.len() as u64).to_le_bytes().to_vec();
    let mut body = DOCUMENT.to_vec();
    for (number, block) in body.chunks_mut(512).enumerate() {
        let key = cryptoapi_key(PASSWORD, &salt, number as u32, key_bytes);
        wp_cipher::Rc4::new(&key).apply(block);
    }
    package.extend_from_slice(&body);

    let mut builder = Builder::new();
    builder.stream("EncryptionInfo", standard_info(0x6801, key_bits, &salt, &verifier));
    builder.stream("EncryptedPackage", package);
    builder.build()
}

#[test]
fn a_document_encrypted_with_rc4_the_way_office_2003_did_it_opens() {
    for key_bits in [40u32, 128] {
        let file = cryptoapi_file(key_bits);
        let opened = wp_crypt::open(&file, PASSWORD).expect("the password is right");
        assert_eq!(opened, DOCUMENT, "a key of {key_bits} bits");
    }
}

#[test]
fn a_wrong_password_is_refused_rather_than_deciphering_to_nonsense() {
    let file = cryptoapi_file(128);
    assert!(matches!(wp_crypt::open(&file, "open sesame"), Err(wp_crypt::Error::WrongPassword)));
}

/// A file encrypted the oldest way, which has no header at all.
fn binary_file() -> Vec<u8> {
    let salt = [0x33u8; 16];
    let plain = [0x44u8; 16];

    let mut both = plain.to_vec();
    both.extend_from_slice(&wp_hash::md5(&plain));
    let key = wp_crypt::binary_key(PASSWORD, &salt, 0);
    wp_cipher::Rc4::new(&key).apply(&mut both);

    let mut info = 1u16.to_le_bytes().to_vec();
    info.extend_from_slice(&1u16.to_le_bytes());
    info.extend_from_slice(&0u32.to_le_bytes());
    info.extend_from_slice(&salt);
    info.extend_from_slice(&both);

    let mut body = DOCUMENT.to_vec();
    for (number, block) in body.chunks_mut(512).enumerate() {
        let key = wp_crypt::binary_key(PASSWORD, &salt, number as u32);
        wp_cipher::Rc4::new(&key).apply(block);
    }

    let mut builder = Builder::new();
    builder.stream("EncryptionInfo", info);
    builder.stream("EncryptedPackage", body);
    builder.build()
}

#[test]
fn a_document_encrypted_the_way_word_97_did_it_opens() {
    let file = binary_file();
    let opened = wp_crypt::open(&file, PASSWORD).expect("the password is right");
    assert_eq!(opened, DOCUMENT);
    assert!(matches!(wp_crypt::open(&file, "something else"), Err(wp_crypt::Error::WrongPassword)));
}

#[test]
fn a_document_this_program_seals_carries_the_storage_the_format_asks_for() {
    let fresh = wp_crypt::Fresh::from_bytes(&[0x5Au8; 144]);
    let sealed = wp_crypt::seal(DOCUMENT, PASSWORD, &fresh);
    let file = CompoundFile::open(sealed.clone()).expect("a compound file");

    assert!(file.walk(&[SPACES, "Version"]).is_some(), "no version");
    assert!(file.walk(&[SPACES, "DataSpaceMap"]).is_some(), "no map");
    assert!(
        file.walk(&[SPACES, "DataSpaceInfo", "StrongEncryptionDataSpace"]).is_some(),
        "no data space"
    );
    let primary = file
        .walk(&[SPACES, "TransformInfo", "StrongEncryptionTransform", "\u{6}Primary"])
        .expect("no transform");

    // And the transform is the one that means a password.
    let units: Vec<u16> =
        primary[12..].chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect();
    let text = String::from_utf16_lossy(&units);
    assert!(text.starts_with("{FF9A3F03-56EF-4613-BDD5-5A41C1D07246}"), "{text:?}");

    // And it still opens, which is what says the storage disturbed nothing.
    assert_eq!(wp_crypt::open(&sealed, PASSWORD).expect("opening"), DOCUMENT);
}

#[test]
fn a_file_whose_transform_is_not_a_password_is_refused() {
    // The rights-managed path uses the same machinery with another transform,
    // and its key comes from a server nobody here can ask. A program that
    // deciphered what it could and said nothing would be claiming to have
    // opened a document it has not.
    let fresh = wp_crypt::Fresh::from_bytes(&[0x5Au8; 144]);
    let sealed = wp_crypt::seal(DOCUMENT, PASSWORD, &fresh);
    let file = CompoundFile::open(sealed).expect("a compound file");

    let mut builder = Builder::new();
    builder.item(Item::storage(
        SPACES,
        vec![
            Item::stream("DataSpaceMap", file.walk(&[SPACES, "DataSpaceMap"]).unwrap_or_default()),
            Item::storage(
                "TransformInfo",
                vec![Item::storage(
                    "StrongEncryptionTransform",
                    // A transform of another name: the identifier is what is
                    // read, and this is not the one.
                    vec![Item::stream("\u{6}Primary", vec![0u8; 64])],
                )],
            ),
        ],
    ));
    builder.stream("EncryptionInfo", file.stream("EncryptionInfo").unwrap_or_default());
    builder.stream("EncryptedPackage", file.stream("EncryptedPackage").unwrap_or_default());

    let changed = builder.build();
    assert!(matches!(wp_crypt::open(&changed, PASSWORD), Err(wp_crypt::Error::Unsupported(_))));
}

#[test]
fn a_file_with_no_data_spaces_at_all_is_taken_at_its_word() {
    // Word opens one, and a document this program wrote before the storage
    // was written has none.
    let file = cryptoapi_file(128);
    assert!(wp_crypt::open(&file, PASSWORD).is_ok());
}
