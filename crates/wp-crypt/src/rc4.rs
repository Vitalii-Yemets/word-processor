//! The two ways Office encrypted a document before 2007.
//!
//! # Why they are read and never written
//!
//! RC4 with a forty-bit key is not encryption any more: a machine that would
//! have taken a year to try every key in 1997 takes an afternoon now. Nothing
//! this program writes uses either scheme — sealing a document is AES with a
//! two hundred and fifty-six bit key, which is [`crate::agile`].
//!
//! They are here because a document somebody encrypted in Word 97 or Word
//! 2003 is still their document, and a word processor that cannot open it has
//! lost it. Reading a weak cipher is not endorsing it; refusing to read one
//! is losing somebody's work to make a point.
//!
//! # The two schemes
//!
//! **Office binary RC4**, version 1.1: the key is MD5 over the salt and the
//! password, and the verifier is sixteen bytes and their hash under the same
//! key. Forty bits, always.
//!
//! **RC4 CryptoAPI**, the same header as the standard AES scheme with
//! `AlgID` naming RC4: SHA-1 over the salt and the password, a block number
//! folded in so that every block of the document has a key of its own, and a
//! key length the file states.
//!
//! The block number is the part that matters when reading: the document is
//! enciphered in blocks of five hundred and twelve bytes, each with a key of
//! its own, and a reader that used one key throughout would get the first
//! block right and nothing else.

use crate::{utf16, Error};

/// How long a block of the document is, each with its own key.
const BLOCK: usize = 512;

/// The number the header gives RC4.
pub(crate) const ALGORITHM: u32 = 0x6801;

/// Opens a document encrypted the way Office encrypted one before 2007, given
/// the standard header this shares with the AES scheme.
pub(crate) fn open_cryptoapi(
    header: &[u8],
    verifier: &[u8],
    package: &[u8],
    password: &str,
) -> Result<Vec<u8>, Error> {
    let key_bits = u32_at(header, 16).ok_or(Error::Damaged("description"))?;
    // A file that says nothing about its key length means forty bits, which
    // is what the scheme started as.
    let key_bytes = if key_bits == 0 { 5 } else { (key_bits / 8) as usize };
    if key_bytes == 0 || key_bytes > 16 {
        return Err(Error::Damaged("description"));
    }

    let salt_size = u32_at(verifier, 0).ok_or(Error::Damaged("verifier"))? as usize;
    let salt = verifier.get(4..4 + salt_size).ok_or(Error::Damaged("verifier"))?;
    let enciphered =
        verifier.get(4 + salt_size..4 + salt_size + 16).ok_or(Error::Damaged("verifier"))?;
    let enciphered_hash =
        verifier.get(4 + salt_size + 16..4 + salt_size + 36).ok_or(Error::Damaged("verifier"))?;

    let base = sha1_base(password, salt);
    let key = cryptoapi_key(&base, 0, key_bytes);

    // The verifier and its hash are one run of the cipher, not two: they sit
    // next to each other in the file and were enciphered as they sit.
    let mut both = Vec::with_capacity(36);
    both.extend_from_slice(enciphered);
    both.extend_from_slice(enciphered_hash);
    let mut cipher = wp_cipher::Rc4::new(&key);
    cipher.apply(&mut both);
    let wanted = wp_hash::sha1(&both[..16]);
    if !same(&wanted, &both[16..36]) {
        return Err(Error::WrongPassword);
    }

    let length = crate::declared_length(package)?;
    let mut out = package.get(8..).ok_or(Error::Damaged("package"))?.to_vec();
    for (number, block) in out.chunks_mut(BLOCK).enumerate() {
        let key = cryptoapi_key(&base, number as u32, key_bytes);
        wp_cipher::Rc4::new(&key).apply(block);
    }
    if out.len() < length {
        return Err(Error::Damaged("package"));
    }
    out.truncate(length);
    Ok(out)
}

/// What every block's key is made from: the salt and the password, hashed
/// once.
fn sha1_base(password: &str, salt: &[u8]) -> [u8; 20] {
    let mut first = salt.to_vec();
    first.extend_from_slice(&utf16(password));
    wp_hash::sha1(&first)
}

/// And one block's key: that, with the block's number after it.
fn cryptoapi_key(base: &[u8; 20], block: u32, key_bytes: usize) -> Vec<u8> {
    let mut with_block = base.to_vec();
    with_block.extend_from_slice(&block.to_le_bytes());
    let hash = wp_hash::sha1(&with_block);

    // A forty-bit key is used as sixteen bytes with the rest left at nought,
    // which is the one thing about this scheme that cannot be guessed from
    // the numbers: the key is short and the cipher is fed a long one.
    if key_bytes == 5 {
        let mut key = vec![0u8; 16];
        key[..5].copy_from_slice(&hash[..5]);
        return key;
    }
    hash[..key_bytes.min(hash.len())].to_vec()
}

/// Whether two runs of bytes are the same, without telling anybody watching
/// the clock where they stopped being the same.
fn same(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut differences = 0u8;
    for (a, b) in left.iter().zip(right) {
        differences |= a ^ b;
    }
    differences == 0
}

/// A number four bytes into something, least significant byte first.
fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    let slice = bytes.get(at..at + 4)?;
    Some(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

/// The key the oldest scheme makes: MD5 over the salt and the password.
///
/// Word 97's, and the one that is forty bits whatever anybody wanted. The
/// first five bytes of the hash, the block number, and another hash — the
/// same shape as the newer one with MD5 in place of SHA-1.
#[must_use]
pub fn binary_key(password: &str, salt: &[u8], block: u32) -> Vec<u8> {
    // The password is hashed, and the first five bytes of that are repeated
    // with the salt sixteen times over. Written out because it cannot be
    // guessed: it is a stretch of the key nobody would invent.
    let hashed = wp_hash::md5(&utf16(password));
    let mut stretched = Vec::with_capacity(21 * 16);
    for _ in 0..16 {
        stretched.extend_from_slice(&hashed[..5]);
        stretched.extend_from_slice(salt);
    }
    let intermediate = wp_hash::md5(&stretched);

    let mut with_block = intermediate[..5].to_vec();
    with_block.extend_from_slice(&block.to_le_bytes());
    let hash = wp_hash::md5(&with_block);
    // Forty bits again, given to the cipher as the nine bytes the scheme
    // says: five of key and four of the hash after them.
    hash[..9].to_vec()
}

/// Opens a document encrypted the oldest way, given the description that
/// follows the version.
pub(crate) fn open_binary(info: &[u8], package: &[u8], password: &str) -> Result<Vec<u8>, Error> {
    // Salt, verifier, and the verifier's hash: sixteen bytes each, one after
    // another, and nothing else.
    let salt = info.get(0..16).ok_or(Error::Damaged("description"))?;
    let enciphered = info.get(16..32).ok_or(Error::Damaged("description"))?;
    let enciphered_hash = info.get(32..48).ok_or(Error::Damaged("description"))?;

    let key = binary_key(password, salt, 0);
    let mut both = Vec::with_capacity(32);
    both.extend_from_slice(enciphered);
    both.extend_from_slice(enciphered_hash);
    wp_cipher::Rc4::new(&key).apply(&mut both);
    let wanted = wp_hash::md5(&both[..16]);
    if !same(&wanted, &both[16..32]) {
        return Err(Error::WrongPassword);
    }

    let mut out = package.to_vec();
    for (number, block) in out.chunks_mut(BLOCK).enumerate() {
        let key = binary_key(password, salt, number as u32);
        wp_cipher::Rc4::new(&key).apply(block);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_forty_bit_key_is_given_to_the_cipher_as_sixteen_bytes() {
        // The one thing about the scheme that cannot be guessed from the
        // numbers: five bytes of key, eleven of nought.
        let base = [0u8; 20];
        let key = cryptoapi_key(&base, 0, 5);
        assert_eq!(key.len(), 16);
        assert!(key[5..].iter().all(|byte| *byte == 0), "{key:?}");
    }

    #[test]
    fn a_longer_key_is_given_as_the_length_the_file_says() {
        let base = [0u8; 20];
        assert_eq!(cryptoapi_key(&base, 0, 16).len(), 16);
        assert_eq!(cryptoapi_key(&base, 0, 8).len(), 8);
    }

    #[test]
    fn every_block_has_a_key_of_its_own() {
        let base = [7u8; 20];
        assert_ne!(cryptoapi_key(&base, 0, 16), cryptoapi_key(&base, 1, 16));
        assert_eq!(cryptoapi_key(&base, 3, 16), cryptoapi_key(&base, 3, 16));
    }

    #[test]
    fn the_oldest_key_is_the_nine_bytes_the_scheme_says() {
        assert_eq!(binary_key("secret", &[1u8; 16], 0).len(), 9);
        assert_ne!(binary_key("secret", &[1u8; 16], 0), binary_key("secret", &[1u8; 16], 1));
        assert_ne!(binary_key("secret", &[1u8; 16], 0), binary_key("other", &[1u8; 16], 0));
    }
}
