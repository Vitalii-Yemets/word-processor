//! The encryption Office has written since 2010.
//!
//! # The shape of it
//!
//! The description is XML, and it says two separate things. `keyData`
//! describes how the package itself is enciphered — which cipher, which hash,
//! how long the key, and the salt the initialisation vectors are made from.
//! `keyEncryptors` describes how the key that does it is itself kept: hashed
//! out of the password and used to encipher the real key.
//!
//! That indirection is the point. The document is enciphered with a key that
//! has nothing to do with the password; the password only unlocks that key.
//! So changing a password means enciphering thirty-two bytes again, not the
//! whole document — and two people can be given the same document under
//! different passwords without it being written twice.
//!
//! # The segments
//!
//! The package is not one long run of chained blocks. It is cut into pieces
//! of four thousand and ninety-six bytes and each piece is chained on its
//! own, from an initialisation vector made out of the salt and the piece's
//! number. That is what lets a program read the middle of a large document
//! without deciphering everything before it.
//!
//! # The check at the end
//!
//! `dataIntegrity` carries an authenticated hash of the whole enciphered
//! stream, under a key of its own that is itself enciphered. Somebody who
//! cannot read the document can still change a byte in it; this is what
//! notices.

use wp_cipher::Key;
use wp_xml::tree::{Element, XmlTree};

use crate::{declared_length, fitted, same, utf16, Error, Fresh, SPINS};

/// How big a piece of the package is.
const SEGMENT: usize = 4096;

/// The fixed eight-byte values that make one hash into five different keys.
///
/// They have no meaning; they are there so that the key that deciphers the
/// verifier cannot also decipher the document. The format writes them out and
/// so does this.
const VERIFIER_INPUT: [u8; 8] = [0xfe, 0xa7, 0xd2, 0x76, 0x3b, 0x4b, 0x9e, 0x79];
const VERIFIER_VALUE: [u8; 8] = [0xd7, 0xaa, 0x0f, 0x6d, 0x30, 0x61, 0x34, 0x4e];
const KEY_VALUE: [u8; 8] = [0x14, 0x6e, 0x0b, 0xe7, 0xab, 0xac, 0xd0, 0xd6];
const HMAC_KEY: [u8; 8] = [0x5f, 0xb2, 0xad, 0x01, 0x0c, 0xb9, 0xe1, 0xf6];
const HMAC_VALUE: [u8; 8] = [0xa0, 0x67, 0x7f, 0x02, 0xb2, 0x2c, 0x84, 0x33];

/// Which hash a description names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Hash {
    Sha1,
    Sha512,
}

impl Hash {
    fn named(name: &str) -> Option<Self> {
        match name {
            "SHA1" | "SHA-1" => Some(Self::Sha1),
            "SHA512" | "SHA-512" => Some(Self::Sha512),
            _ => None,
        }
    }

    fn of(self, bytes: &[u8]) -> Vec<u8> {
        match self {
            Self::Sha1 => wp_hash::sha1(bytes).to_vec(),
            Self::Sha512 => wp_hash::sha512(bytes).to_vec(),
        }
    }

    fn of_two(self, first: &[u8], second: &[u8]) -> Vec<u8> {
        let mut both = first.to_vec();
        both.extend_from_slice(second);
        self.of(&both)
    }
}

/// What one half of the description says.
#[derive(Clone, Debug)]
struct Part {
    salt: Vec<u8>,
    block_size: usize,
    key_bytes: usize,
    hash: Hash,
    spins: u32,
    verifier_input: Vec<u8>,
    verifier_value: Vec<u8>,
    key_value: Vec<u8>,
}

impl Part {
    /// Reads the attributes both halves share, and the ones only the password
    /// half has.
    fn read(element: &Element) -> Result<Self, Error> {
        let text = |name: &str| element.attribute(None, name).unwrap_or_default();
        let number = |name: &str, fallback: usize| {
            element.attribute(None, name).and_then(|v| v.parse().ok()).unwrap_or(fallback)
        };
        let bytes = |name: &str| wp_text::base64::decode(text(name).as_bytes());

        let cipher = text("cipherAlgorithm");
        if !cipher.is_empty() && cipher != "AES" {
            return Err(Error::Unsupported(format!("the {cipher} cipher")));
        }
        let chaining = text("cipherChaining");
        if !chaining.is_empty() && chaining != "ChainingModeCBC" {
            return Err(Error::Unsupported(chaining.to_owned()));
        }
        let hash = Hash::named(text("hashAlgorithm"))
            .ok_or_else(|| Error::Unsupported(format!("the {} hash", text("hashAlgorithm"))))?;

        Ok(Self {
            salt: bytes("saltValue"),
            block_size: number("blockSize", 16),
            key_bytes: number("keyBits", 256) / 8,
            hash,
            spins: element.attribute(None, "spinCount").and_then(|v| v.parse().ok()).unwrap_or(0),
            verifier_input: bytes("encryptedVerifierHashInput"),
            verifier_value: bytes("encryptedVerifierHashValue"),
            key_value: bytes("encryptedKeyValue"),
        })
    }

    /// The hash the password comes to after all its turns.
    ///
    /// The turn's number goes in **before** the last hash here, which is the
    /// other way round from the hash a restriction's password is checked
    /// with. Two parts of the same format, written by different people, a few
    /// years apart; there is no reason for it and no choice about it.
    fn hashed_password(&self, password: &str) -> Vec<u8> {
        let mut hash = self.hash.of_two(&self.salt, &utf16(password));
        for turn in 0..self.spins {
            hash = self.hash.of_two(&turn.to_le_bytes(), &hash);
        }
        hash
    }

    /// One of the five keys that hash stands for.
    fn key_for(&self, hashed: &[u8], block: &[u8; 8]) -> Option<Key> {
        Key::new(&fitted(&self.hash.of_two(hashed, block), self.key_bytes))
    }

    /// The initialisation vector for a piece of the package, or for one of
    /// the fixed values.
    fn start(&self, block: &[u8]) -> [u8; 16] {
        let made = if block.is_empty() {
            fitted(&self.salt, self.block_size)
        } else {
            fitted(&self.hash.of_two(&self.salt, block), self.block_size)
        };
        let mut out = [0u8; 16];
        let take = made.len().min(16);
        out[..take].copy_from_slice(&made[..take]);
        out
    }
}

/// Deciphers a package.
pub(crate) fn open(xml: &[u8], package: &[u8], password: &str) -> Result<Vec<u8>, Error> {
    let text = std::str::from_utf8(xml).map_err(|_| Error::Damaged("description"))?;
    let tree = XmlTree::parse(text).map_err(|_| Error::Damaged("description"))?;
    let data = find(&tree.root, "keyData").ok_or(Error::Damaged("description"))?;
    let encryptor = find(&tree.root, "encryptedKey").ok_or(Error::Damaged("description"))?;
    let data = Part::read(data)?;
    let password_part = Part::read(encryptor)?;

    // The password, hashed as many times as the file says, and the three keys
    // that hash stands for.
    let hashed = password_part.hashed_password(password);
    let start = password_part.start(&[]);
    let key = |block: &[u8; 8]| {
        password_part.key_for(&hashed, block).ok_or(Error::Damaged("description"))
    };

    // Whether it is the password: decipher a value and its hash, and see
    // whether the one hashes to the other. Neither of them is the key.
    let input =
        wp_cipher::decrypt_cbc(&key(&VERIFIER_INPUT)?, &start, &password_part.verifier_input);
    let value =
        wp_cipher::decrypt_cbc(&key(&VERIFIER_VALUE)?, &start, &password_part.verifier_value);
    let wanted = password_part.hash.of(&input[..password_part.salt.len().min(input.len())]);
    if !same(&wanted, &value[..wanted.len().min(value.len())]) {
        return Err(Error::WrongPassword);
    }

    // The key the document is really enciphered with, which the password only
    // unlocks.
    let secret = wp_cipher::decrypt_cbc(&key(&KEY_VALUE)?, &start, &password_part.key_value);
    let secret = Key::new(&secret[..data.key_bytes.min(secret.len())])
        .ok_or(Error::Damaged("description"))?;

    check_integrity(&tree.root, &data, &secret, package)?;

    let length = declared_length(package)?;
    let mut out = Vec::with_capacity(length);
    for (number, piece) in package[8..].chunks(SEGMENT).enumerate() {
        let start = data.start(&(number as u32).to_le_bytes());
        out.extend_from_slice(&wp_cipher::decrypt_cbc(&secret, &start, piece));
    }
    if out.len() < length {
        return Err(Error::Damaged("package"));
    }
    out.truncate(length);
    Ok(out)
}

/// Whether the enciphered bytes are the bytes that were enciphered.
///
/// A file with no `dataIntegrity` is one written before that was part of the
/// format; it is opened without the check rather than refused, because a
/// document somebody has is a document they should be able to read.
fn check_integrity(root: &Element, data: &Part, secret: &Key, package: &[u8]) -> Result<(), Error> {
    let Some(integrity) = find(root, "dataIntegrity") else { return Ok(()) };
    let encrypted_key = wp_text::base64::decode(
        integrity.attribute(None, "encryptedHmacKey").unwrap_or_default().as_bytes(),
    );
    let encrypted_value = wp_text::base64::decode(
        integrity.attribute(None, "encryptedHmacValue").unwrap_or_default().as_bytes(),
    );
    if encrypted_key.is_empty() || encrypted_value.is_empty() {
        return Ok(());
    }

    let key = wp_cipher::decrypt_cbc(secret, &data.start(&HMAC_KEY), &encrypted_key);
    let value = wp_cipher::decrypt_cbc(secret, &data.start(&HMAC_VALUE), &encrypted_value);
    // Only SHA-512 is authenticated here, which is the only hash Office has
    // ever written a `dataIntegrity` with.
    if data.hash != Hash::Sha512 {
        return Ok(());
    }
    let wanted = wp_hash::hmac_sha512(&key[..64.min(key.len())], package);
    if !same(&wanted, &value[..wanted.len().min(value.len())]) {
        return Err(Error::Tampered);
    }
    Ok(())
}

/// An element by its local name, wherever it is in the description.
///
/// By local name because the description uses three namespaces and puts the
/// password's own half in a prefixed one; nothing in it repeats a name, so
/// looking for the name is enough and is what survives a file that declares
/// its namespaces differently.
fn find<'a>(element: &'a Element, local: &str) -> Option<&'a Element> {
    if element.local_name() == local {
        return Some(element);
    }
    element.child_elements().find_map(|child| find(child, local))
}

/// Writes one.
pub(crate) fn seal(package: &[u8], password: &str, fresh: &Fresh) -> Vec<u8> {
    let data = Part {
        salt: fresh.package_salt.to_vec(),
        block_size: 16,
        key_bytes: 32,
        hash: Hash::Sha512,
        spins: 0,
        verifier_input: Vec::new(),
        verifier_value: Vec::new(),
        key_value: Vec::new(),
    };
    let password_part = Part { salt: fresh.password_salt.to_vec(), spins: SPINS, ..data.clone() };

    let secret = Key::new(&fresh.key).expect("thirty-two bytes is a key");

    // The package, a piece at a time, each piece chained on its own.
    let mut enciphered = (package.len() as u64).to_le_bytes().to_vec();
    for (number, piece) in package.chunks(SEGMENT).enumerate() {
        let start = data.start(&(number as u32).to_le_bytes());
        // The last piece is padded out to whole blocks; how long the package
        // really was is in the eight bytes at the front.
        let mut whole = piece.to_vec();
        whole.resize(piece.len().div_ceil(16) * 16, 0);
        enciphered.extend_from_slice(&wp_cipher::encrypt_cbc(&secret, &start, &whole));
    }

    // The check over those bytes, under a key of its own.
    let hmac = wp_hash::hmac_sha512(&fresh.hmac_key, &enciphered);
    let mut key_room = fresh.hmac_key.to_vec();
    key_room.resize(64, 0);
    let mut value_room = hmac.to_vec();
    value_room.resize(64, 0);
    let encrypted_hmac_key = wp_cipher::encrypt_cbc(&secret, &data.start(&HMAC_KEY), &key_room);
    let encrypted_hmac_value =
        wp_cipher::encrypt_cbc(&secret, &data.start(&HMAC_VALUE), &value_room);

    // The password: a value, its hash, and the real key, each enciphered
    // under a key of its own made out of the hashed password.
    let hashed = password_part.hashed_password(password);
    let start = password_part.start(&[]);
    let key =
        |block: &[u8; 8]| password_part.key_for(&hashed, block).expect("thirty-two bytes is a key");
    // The verifier: sixteen bytes nobody can guess, enciphered, beside the
    // hash of those same bytes, also enciphered. Whoever knows the password
    // can decipher both and see that one hashes to the other; whoever does
    // not, cannot. Bytes of its own rather than the salt again, because the
    // salt is written in the file in plain sight, and enciphering something
    // an onlooker already has is a favour to them.
    let verifier = fresh.verifier;
    let mut verifier_hash = password_part.hash.of(&verifier);
    verifier_hash.resize(64, 0);
    let verifier_input = wp_cipher::encrypt_cbc(&key(&VERIFIER_INPUT), &start, &verifier);
    let verifier_value = wp_cipher::encrypt_cbc(&key(&VERIFIER_VALUE), &start, &verifier_hash);
    let key_value = wp_cipher::encrypt_cbc(&key(&KEY_VALUE), &start, &fresh.key);

    let base64 = |bytes: &[u8]| wp_text::base64::encode(bytes);
    let description = format!(
        concat!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
            "\r\n",
            r#"<encryption xmlns="http://schemas.microsoft.com/office/2006/encryption""#,
            r#" xmlns:p="http://schemas.microsoft.com/office/2006/keyEncryptor/password""#,
            r#" xmlns:c="http://schemas.microsoft.com/office/2006/keyEncryptor/certificate">"#,
            r#"<keyData saltSize="16" blockSize="16" keyBits="256" hashSize="64""#,
            r#" cipherAlgorithm="AES" cipherChaining="ChainingModeCBC""#,
            r#" hashAlgorithm="SHA512" saltValue="{data_salt}"/>"#,
            r#"<dataIntegrity encryptedHmacKey="{hmac_key}""#,
            r#" encryptedHmacValue="{hmac_value}"/>"#,
            r#"<keyEncryptors><keyEncryptor"#,
            r#" uri="http://schemas.microsoft.com/office/2006/keyEncryptor/password">"#,
            r#"<p:encryptedKey spinCount="{spins}" saltSize="16" blockSize="16""#,
            r#" keyBits="256" hashSize="64" cipherAlgorithm="AES""#,
            r#" cipherChaining="ChainingModeCBC" hashAlgorithm="SHA512""#,
            r#" saltValue="{password_salt}" encryptedVerifierHashInput="{verifier_input}""#,
            r#" encryptedVerifierHashValue="{verifier_value}""#,
            r#" encryptedKeyValue="{key_value}"/>"#,
            r#"</keyEncryptor></keyEncryptors></encryption>"#,
        ),
        data_salt = base64(&fresh.package_salt),
        hmac_key = base64(&encrypted_hmac_key),
        hmac_value = base64(&encrypted_hmac_value),
        spins = SPINS,
        password_salt = base64(&fresh.password_salt),
        verifier_input = base64(&verifier_input),
        verifier_value = base64(&verifier_value),
        key_value = base64(&key_value),
    );

    // Version four point four, and the flag that says the description is XML.
    let mut info = vec![4, 0, 4, 0, 0x40, 0, 0, 0];
    info.extend_from_slice(description.as_bytes());

    let mut builder = wp_ole::Builder::new();
    // What the format says is done to the package, beside the package it was
    // done to. Office writes it; a reader stricter than Word would be within
    // its rights to refuse a file without it. See [`crate::spaces`].
    builder.item(crate::spaces::storage());
    builder.stream(crate::INFO, info);
    builder.stream(crate::PACKAGE, enciphered);
    builder.build()
}
