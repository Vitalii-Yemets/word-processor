//! The encryption Office 2007 wrote.
//!
//! # Why it is read and not written
//!
//! Because documents written in it exist and a person who has one wants to
//! open it. It is weaker than what replaced it in every way that matters: the
//! hash is SHA-1, the password is hashed fifty thousand times rather than a
//! hundred thousand, there is no check that the bytes are the bytes that were
//! written, and — worst — the package is enciphered block by block with no
//! chaining at all, so two identical blocks of a document give two identical
//! blocks of cipher text and the shape of the file shows through. Writing a
//! new document this way would be making it weaker on purpose.
//!
//! # The shape of it
//!
//! No XML. A fixed run of bytes: a header saying which cipher and how long
//! the key, the name of the Windows cryptography provider that wrote it, and
//! then a salt, an enciphered verifier and the enciphered hash of that
//! verifier.

use wp_cipher::Key;

use crate::{declared_length, same, utf16, Error};

/// How many turns this scheme hashes a password for. Not a number anybody
/// chose twice: it is written in the specification and nowhere in the file.
const SPINS: u32 = 50_000;

/// Reads the four-byte numbers the header is made of.
fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    let four: [u8; 4] = bytes.get(at..at + 4)?.try_into().ok()?;
    Some(u32::from_le_bytes(four))
}

pub(crate) fn open(info: &[u8], package: &[u8], password: &str) -> Result<Vec<u8>, Error> {
    // The description: how long the header is, then the header, then the
    // verifier. The four bytes before the header are the flags, which say
    // nothing this needs.
    let header_size = u32_at(info, 0).ok_or(Error::Damaged("description"))? as usize;
    let header = info.get(4..4 + header_size).ok_or(Error::Damaged("description"))?;
    let verifier = info.get(4 + header_size..).ok_or(Error::Damaged("description"))?;

    // Inside the header: the algorithm at eight, its hash at twelve, the key
    // length in bits at sixteen.
    let algorithm = u32_at(header, 8).ok_or(Error::Damaged("description"))?;
    let key_bits = u32_at(header, 16).ok_or(Error::Damaged("description"))?;
    // 0x660E, 0x660F and 0x6610 are AES with a key of a hundred and
    // twenty-eight, a hundred and ninety-two and two hundred and fifty-six
    // bits. Anything else in this field is RC4 or older.
    if !matches!(algorithm, 0x660E..=0x6610) {
        return Err(Error::Unsupported(String::from("a cipher older than AES")));
    }
    let key_bytes = (key_bits / 8) as usize;

    let salt_size = u32_at(verifier, 0).ok_or(Error::Damaged("verifier"))? as usize;
    let salt = verifier.get(4..4 + salt_size).ok_or(Error::Damaged("verifier"))?;
    let enciphered =
        verifier.get(4 + salt_size..4 + salt_size + 16).ok_or(Error::Damaged("verifier"))?;
    let hash_size =
        u32_at(verifier, 4 + salt_size + 16).ok_or(Error::Damaged("verifier"))? as usize;
    let enciphered_hash = verifier
        .get(4 + salt_size + 20..4 + salt_size + 20 + 32)
        .ok_or(Error::Damaged("verifier"))?;

    let key = from_password(password, salt, key_bytes).ok_or(Error::Damaged("description"))?;

    // Whether it is the password: sixteen bytes and their hash, both
    // enciphered.
    let plain = wp_cipher::decrypt_ecb(&key, enciphered);
    let plain_hash = wp_cipher::decrypt_ecb(&key, enciphered_hash);
    let wanted = wp_hash::sha1(&plain);
    let hash_size = hash_size.min(wanted.len()).min(plain_hash.len());
    if !same(&wanted[..hash_size], &plain_hash[..hash_size]) {
        return Err(Error::WrongPassword);
    }

    let length = declared_length(package)?;
    let mut out = wp_cipher::decrypt_ecb(&key, &package[8..]);
    if out.len() < length {
        return Err(Error::Damaged("package"));
    }
    out.truncate(length);
    Ok(out)
}

/// The key this scheme makes out of a password.
///
/// Fifty thousand turns of SHA-1 with the turn's number in front, one more
/// turn with a nought after it, and then the odd last step: the result is
/// exclusive-ored with two different fillers, each of those hashed, and the
/// two hashes joined and cut to the key's length. It is HMAC in all but name,
/// written out by somebody who did not say so.
fn from_password(password: &str, salt: &[u8], key_bytes: usize) -> Option<Key> {
    let mut first = salt.to_vec();
    first.extend_from_slice(&utf16(password));
    let mut hash = wp_hash::sha1(&first).to_vec();

    for turn in 0..SPINS {
        let mut next = turn.to_le_bytes().to_vec();
        next.extend_from_slice(&hash);
        hash = wp_hash::sha1(&next).to_vec();
    }
    let mut last = hash;
    last.extend_from_slice(&0u32.to_le_bytes());
    let hash = wp_hash::sha1(&last);

    let mut inner = [0x36u8; 64];
    let mut outer = [0x5Cu8; 64];
    for (at, byte) in hash.iter().enumerate() {
        inner[at] ^= byte;
        outer[at] ^= byte;
    }
    let mut both = wp_hash::sha1(&inner).to_vec();
    both.extend_from_slice(&wp_hash::sha1(&outer));
    both.truncate(key_bytes);
    Key::new(&both)
}
