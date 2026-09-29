//! The encryption of the binary formats themselves: a `.doc` enciphered
//! stream by stream, rather than a package enciphered whole.
//!
//! # How it differs from an encrypted package
//!
//! An encrypted `.docx` is a compound file holding a zip enciphered as one
//! run of bytes. An encrypted `.doc` is still a `.doc`: the same compound file
//! with the same streams, each of them enciphered where it lies, with the
//! first few bytes of the main stream left readable so that a reader can find
//! out that the rest is not. The block of numbers at the start of the
//! `WordDocument` stream says whether the document is encrypted and how, and
//! for everything but the weakest scheme the description of the encryption is
//! at the start of the table stream.
//!
//! So there is nothing here to open. What a reader gets is a [`StreamCipher`]:
//! the password proved and the keys worked out, ready to be run over each
//! stream in turn, every one of them from its first byte.
//!
//! # The three schemes
//!
//! **RC4**, Word 97's own: the description is the version, a salt, and a
//! verifier with its hash — [`crate::rc4`]'s oldest scheme exactly, with each
//! stream's blocks numbered from its own start.
//!
//! **RC4 CryptoAPI**, which Word 2002 and 2003 offered: the standard header
//! naming RC4, and the same again with SHA-1 for MD5.
//!
//! **XOR obfuscation**, Word 95's and an option in Word 97's successors under
//! the honest name "weak encryption": no cipher at all. The password is
//! turned into sixteen bytes, and each byte of the document is exclusive-ored
//! with the one of them its place in the stream picks — except a byte that is
//! nought, or that would become nought, which is left alone. There is no
//! description in the table stream: the block of numbers holds a sixteen-bit
//! key and a sixteen-bit verifier, both made from the password, and a password
//! is right when it makes both.

use crate::rc4::{self, BinaryKeys, CryptoApiKeys};
use crate::Error;

/// A binary document's streams' cipher, with the password proved.
#[derive(Clone, Debug)]
pub struct StreamCipher {
    keys: Keys,
}

#[derive(Clone, Debug)]
enum Keys {
    Rc4(BinaryKeys),
    CryptoApi(CryptoApiKeys),
    Xor([u8; 16]),
}

impl StreamCipher {
    /// Deciphers a stream in place, from its first byte — or enciphers it,
    /// which for every one of these schemes is the same thing.
    ///
    /// The whole stream: where a format leaves the first bytes of a stream
    /// readable, the reader keeps them and puts them back, because the
    /// keys run from the start of the stream whether or not the bytes there
    /// were enciphered.
    pub fn apply(&self, stream: &mut [u8]) {
        match &self.keys {
            Keys::Rc4(keys) => keys.apply(stream),
            Keys::CryptoApi(keys) => keys.apply(stream),
            Keys::Xor(array) => {
                for (at, byte) in stream.iter_mut().enumerate() {
                    let changed = *byte ^ array[at % 16];
                    if *byte != 0 && changed != 0 {
                        *byte = changed;
                    }
                }
            }
        }
    }
}

/// The cipher a description names, given the password: the description being
/// what the table stream begins with, a version and what follows it.
pub fn rc4_stream_cipher(description: &[u8], password: &str) -> Result<StreamCipher, Error> {
    let version =
        |at: usize| description.get(at..at + 2).map(|two| u16::from_le_bytes([two[0], two[1]]));
    let (Some(major), Some(minor)) = (version(0), version(2)) else {
        return Err(Error::Damaged("description"));
    };
    let rest = &description[4..];
    match (major, minor) {
        (1, 1) => Ok(StreamCipher { keys: Keys::Rc4(rc4::binary_keys(rest, password)?) }),
        (2..=4, 2) => {
            // The flags, the header's length, the header, and the verifier.
            let four = |at: usize| {
                rest.get(at..at + 4)
                    .map(|four| u32::from_le_bytes([four[0], four[1], four[2], four[3]]) as usize)
            };
            let header_size = four(4).ok_or(Error::Damaged("description"))?;
            let header = rest.get(8..8 + header_size).ok_or(Error::Damaged("description"))?;
            let verifier = rest.get(8 + header_size..).ok_or(Error::Damaged("description"))?;
            let algorithm = header
                .get(8..12)
                .map(|four| u32::from_le_bytes([four[0], four[1], four[2], four[3]]));
            if algorithm != Some(rc4::ALGORITHM) {
                return Err(Error::Unsupported(String::from("a cipher this program has not got")));
            }
            Ok(StreamCipher {
                keys: Keys::CryptoApi(rc4::cryptoapi_keys(header, verifier, password)?),
            })
        }
        _ => Err(Error::Unsupported(format!("version {major}.{minor} of the encryption"))),
    }
}

/// The weakest scheme's cipher, given the password and the four bytes the
/// file keeps for it: the verifier in the low half, the key in the high —
/// the order LibreOffice, reading a file made here, agrees with.
pub fn xor_stream_cipher(password: &str, stored: u32) -> Result<StreamCipher, Error> {
    let (key, verifier) = xor_values(password);
    if stored != xor_stored(key, verifier) {
        return Err(Error::WrongPassword);
    }
    Ok(StreamCipher { keys: Keys::Xor(xor_array(password, key)) })
}

/// The four bytes a file keeps for a password: the verifier and the key.
#[must_use]
pub fn xor_stored(key: u16, verifier: u16) -> u32 {
    u32::from(verifier) | (u32::from(key) << 16)
}

/// The key and the verifier the weakest scheme makes of a password.
#[must_use]
pub fn xor_values(password: &str) -> (u16, u16) {
    let bytes = single_bytes(password);
    (xor_key(&bytes), xor_verifier(&bytes))
}

/// A password as the weakest scheme takes it: one byte a letter, the low
/// byte of each or the high one where the low is nought, and no more than
/// fifteen of them.
fn single_bytes(password: &str) -> Vec<u8> {
    password
        .encode_utf16()
        .take(15)
        .map(|unit| {
            let low = (unit & 0x00FF) as u8;
            if low == 0 {
                (unit >> 8) as u8
            } else {
                low
            }
        })
        .collect()
}

/// The verifier: each byte from the last, and then the length, shifted in
/// fifteen bits at a time. [MS-OFFCRYPTO] 2.3.7.1.
fn xor_verifier(bytes: &[u8]) -> u16 {
    let mut verifier: u16 = 0;
    let length = bytes.len() as u8;
    for byte in bytes.iter().rev().chain(core::iter::once(&length)) {
        let carried = (verifier >> 14) & 1;
        verifier = (carried | ((verifier << 1) & 0x7FFF)) ^ u16::from(*byte);
    }
    verifier ^ 0xCE4B
}

/// Where the key starts, by the password's length.
const INITIAL_CODE: [u16; 15] = [
    0xE1F0, 0x1D0F, 0xCC9C, 0x84C0, 0x110C, 0x0E10, 0xF1CE, 0x313E, 0x1872, 0xE139, 0xD40F, 0x84F9,
    0x280C, 0xA96A, 0x4EC3,
];

/// What each bit of the password folds into the key: fifteen rows of seven,
/// each number in a row the one before it doubled with `0x1021` folded back
/// in where it overflowed — which is how this table can be checked, and how
/// the one place a copy of it elsewhere has wrong was found.
const XOR_MATRIX: [u16; 105] = [
    0xAEFC, 0x4DD9, 0x9BB2, 0x2745, 0x4E8A, 0x9D14, 0x2A09, 0x7B61, 0xF6C2, 0xFDA5, 0xEB6B, 0xC6F7,
    0x9DCF, 0x2BBF, 0x4563, 0x8AC6, 0x05AD, 0x0B5A, 0x16B4, 0x2D68, 0x5AD0, 0x0375, 0x06EA, 0x0DD4,
    0x1BA8, 0x3750, 0x6EA0, 0xDD40, 0xD849, 0xA0B3, 0x5147, 0xA28E, 0x553D, 0xAA7A, 0x44D5, 0x6F45,
    0xDE8A, 0xAD35, 0x4A4B, 0x9496, 0x390D, 0x721A, 0xEB23, 0xC667, 0x9CEF, 0x29FF, 0x53FE, 0xA7FC,
    0x5FD9, 0x47D3, 0x8FA6, 0x0F6D, 0x1EDA, 0x3DB4, 0x7B68, 0xF6D0, 0xB861, 0x60E3, 0xC1C6, 0x93AD,
    0x377B, 0x6EF6, 0xDDEC, 0x45A0, 0x8B40, 0x06A1, 0x0D42, 0x1A84, 0x3508, 0x6A10, 0xAA51, 0x4483,
    0x8906, 0x022D, 0x045A, 0x08B4, 0x1168, 0x76B4, 0xED68, 0xCAF1, 0x85C3, 0x1BA7, 0x374E, 0x6E9C,
    0x3730, 0x6E60, 0xDCC0, 0xA9A1, 0x4363, 0x86C6, 0x1DAD, 0x3331, 0x6662, 0xCCC4, 0x89A9, 0x0373,
    0x06E6, 0x0DCC, 0x1021, 0x2042, 0x4084, 0x8108, 0x1231, 0x2462, 0x48C4,
];

/// The key: the length's starting number, with a row of the matrix folded in
/// for every bit of every byte, the last byte first. [MS-OFFCRYPTO] 2.3.7.2.
fn xor_key(bytes: &[u8]) -> u16 {
    let Some(&start) = bytes.len().checked_sub(1).and_then(|at| INITIAL_CODE.get(at)) else {
        return 0;
    };
    let mut key = start;
    let mut element = XOR_MATRIX.len();
    for &byte in bytes.iter().rev() {
        let mut bits = byte;
        for _ in 0..7 {
            element -= 1;
            if bits & 0x40 != 0 {
                key ^= XOR_MATRIX[element];
            }
            bits <<= 1;
        }
    }
    key
}

/// What fills out a password shorter than sixteen bytes.
const PAD: [u8; 15] =
    [0xBB, 0xFF, 0xFF, 0xBA, 0xFF, 0xFF, 0xB9, 0x80, 0x00, 0xBE, 0x0F, 0x00, 0xBF, 0x0F, 0x00];

/// The sixteen bytes the document is exclusive-ored with: the password padded
/// out, each byte crossed with one half of the key — the low half at an even
/// place, the high at an odd — and turned one bit to the right. Word's form of
/// [MS-OFFCRYPTO] 2.3.7.2's array.
fn xor_array(password: &str, key: u16) -> [u8; 16] {
    let bytes = single_bytes(password);
    let halves = key.to_le_bytes();
    core::array::from_fn(|at| {
        let source = bytes.get(at).copied().unwrap_or_else(|| PAD[at - bytes.len()]);
        (source ^ halves[at % 2]).rotate_right(1)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_row_of_the_matrix_doubles_with_the_polynomial_folded_in() {
        for row in XOR_MATRIX.chunks_exact(7) {
            for pair in row.windows(2) {
                let doubled = pair[0] << 1;
                let wanted = if pair[0] & 0x8000 == 0 { doubled } else { doubled ^ 0x1021 };
                assert_eq!(pair[1], wanted, "{:04X} then {:04X}", pair[0], pair[1]);
            }
        }
    }

    #[test]
    fn the_verifier_is_the_same_worked_the_other_way_round() {
        // The same number worked as another program works it: the length
        // with the constant, and each byte turned left by its place, within
        // fifteen bits.
        let rotated = |bytes: &[u8]| {
            let mut hash = bytes.len() as u16;
            if !bytes.is_empty() {
                hash ^= 0xCE4B;
            }
            for (at, byte) in bytes.iter().enumerate() {
                let turn = ((at + 1) % 15) as u32;
                let value = u16::from(*byte);
                let turned = ((value << turn) | (value >> (15 - turn))) & 0x7FFF;
                hash ^= turned;
            }
            hash
        };
        for password in ["a", "secret", "Fenchurch", "correct horse", "123456789012345"] {
            let bytes = single_bytes(password);
            assert_eq!(xor_verifier(&bytes), rotated(&bytes), "{password}");
        }
    }

    #[test]
    fn the_array_is_the_password_padded_crossed_and_turned() {
        let key = 0x1234;
        let array = xor_array("ab", key);
        assert_eq!(array[0], (b'a' ^ 0x34).rotate_right(1));
        assert_eq!(array[1], (b'b' ^ 0x12).rotate_right(1));
        assert_eq!(array[2], (0xBBu8 ^ 0x34).rotate_right(1));
        assert_eq!(array[15], (PAD[13] ^ 0x12).rotate_right(1));
    }

    #[test]
    fn a_byte_that_is_nought_or_would_become_it_is_left_alone() {
        let cipher = StreamCipher { keys: Keys::Xor([0x5A; 16]) };
        let mut bytes = vec![0x00, 0x5A, 0x01, 0xFF];
        cipher.apply(&mut bytes);
        assert_eq!(bytes, vec![0x00, 0x5A, 0x01 ^ 0x5A, 0xFF ^ 0x5A]);
        cipher.apply(&mut bytes);
        assert_eq!(bytes, vec![0x00, 0x5A, 0x01, 0xFF], "the way back is the same way");
    }

    #[test]
    fn a_wrong_password_is_told_from_a_right_one() {
        let (key, verifier) = xor_values("secret");
        assert!(xor_stream_cipher("secret", xor_stored(key, verifier)).is_ok());
        assert!(xor_stream_cipher("secret", xor_stored(verifier, key)).is_err());
        assert_eq!(
            xor_stream_cipher("Secret", xor_stored(key, verifier)).err(),
            Some(Error::WrongPassword)
        );
    }
}
