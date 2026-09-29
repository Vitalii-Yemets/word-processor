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
    let keys = cryptoapi_keys(header, verifier, password)?;
    let length = crate::declared_length(package)?;
    let mut out = package.get(8..).ok_or(Error::Damaged("package"))?.to_vec();
    keys.apply(&mut out);
    if out.len() < length {
        return Err(Error::Damaged("package"));
    }
    out.truncate(length);
    Ok(out)
}

/// The keys of the CryptoAPI scheme, once the password has been proved.
#[derive(Clone, Debug)]
pub(crate) struct CryptoApiKeys {
    base: [u8; 20],
    key_bytes: usize,
}

impl CryptoApiKeys {
    /// Deciphers — or enciphers, which is the same — a run of bytes that
    /// begins with a block, each block with its own key.
    pub(crate) fn apply(&self, bytes: &mut [u8]) {
        for (number, block) in bytes.chunks_mut(BLOCK).enumerate() {
            let key = cryptoapi_key(&self.base, number as u32, self.key_bytes);
            wp_cipher::Rc4::new(&key).apply(block);
        }
    }
}

/// Proves a password against the standard header and its verifier, and
/// gives back what every block's key is made from.
pub(crate) fn cryptoapi_keys(
    header: &[u8],
    verifier: &[u8],
    password: &str,
) -> Result<CryptoApiKeys, Error> {
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
    // Then how long the hash is, and the hash — twenty bytes, as SHA-1's are.
    let enciphered_hash =
        verifier.get(4 + salt_size + 20..4 + salt_size + 40).ok_or(Error::Damaged("verifier"))?;

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
    Ok(CryptoApiKeys { base, key_bytes })
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
    BinaryKeys { truncated: binary_base(password, salt) }.block(block)
}

/// What every block's key is made from in the oldest scheme.
///
/// The password is hashed, and the first five bytes of that are repeated
/// with the salt sixteen times over and hashed again. Written out because it
/// cannot be guessed: it is a stretch of the key nobody would invent.
fn binary_base(password: &str, salt: &[u8]) -> [u8; 5] {
    let hashed = wp_hash::md5(&utf16(password));
    let mut stretched = Vec::with_capacity(21 * 16);
    for _ in 0..16 {
        stretched.extend_from_slice(&hashed[..5]);
        stretched.extend_from_slice(salt);
    }
    let intermediate = wp_hash::md5(&stretched);
    let mut truncated = [0u8; 5];
    truncated.copy_from_slice(&intermediate[..5]);
    truncated
}

/// The keys of the oldest scheme, once the password has been proved.
#[derive(Clone, Debug)]
pub(crate) struct BinaryKeys {
    truncated: [u8; 5],
}

impl BinaryKeys {
    /// One block's key: the five bytes with the block's number after them,
    /// hashed.
    fn block(&self, number: u32) -> Vec<u8> {
        let mut with_block = self.truncated.to_vec();
        with_block.extend_from_slice(&number.to_le_bytes());
        wp_hash::md5(&with_block)[..BINARY_KEY_LENGTH].to_vec()
    }

    /// Deciphers — or enciphers, which is the same — a run of bytes that
    /// begins with a block, each block with its own key.
    pub(crate) fn apply(&self, bytes: &mut [u8]) {
        for (number, block) in bytes.chunks_mut(BLOCK).enumerate() {
            wp_cipher::Rc4::new(&self.block(number as u32)).apply(block);
        }
    }
}

/// How much of each block's hash the cipher is given: all sixteen bytes of
/// it. Forty bits of the password go into the hash, and the whole hash comes
/// out as the key — which a file LibreOffice encrypts is what settles, since
/// nine bytes, which this was once given, open nothing.
const BINARY_KEY_LENGTH: usize = 16;

/// Proves a password against the description that follows the version —
/// salt, verifier, and the verifier's hash, sixteen bytes each, one after
/// another, and nothing else — and gives back the keys.
pub(crate) fn binary_keys(info: &[u8], password: &str) -> Result<BinaryKeys, Error> {
    let salt = info.get(0..16).ok_or(Error::Damaged("description"))?;
    let enciphered = info.get(16..32).ok_or(Error::Damaged("description"))?;
    let enciphered_hash = info.get(32..48).ok_or(Error::Damaged("description"))?;

    let keys = BinaryKeys { truncated: binary_base(password, salt) };
    let mut both = Vec::with_capacity(32);
    both.extend_from_slice(enciphered);
    both.extend_from_slice(enciphered_hash);
    wp_cipher::Rc4::new(&keys.block(0)).apply(&mut both);
    let wanted = wp_hash::md5(&both[..16]);
    if !same(&wanted, &both[16..32]) {
        return Err(Error::WrongPassword);
    }
    Ok(keys)
}

/// Opens a document encrypted the oldest way, given the description that
/// follows the version.
pub(crate) fn open_binary(info: &[u8], package: &[u8], password: &str) -> Result<Vec<u8>, Error> {
    let keys = binary_keys(info, password)?;
    let mut out = package.to_vec();
    keys.apply(&mut out);
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
    fn the_oldest_key_is_the_whole_of_its_hash() {
        assert_eq!(binary_key("secret", &[1u8; 16], 0).len(), 16);
        assert_ne!(binary_key("secret", &[1u8; 16], 0), binary_key("secret", &[1u8; 16], 1));
        assert_ne!(binary_key("secret", &[1u8; 16], 0), binary_key("other", &[1u8; 16], 0));
    }
}
