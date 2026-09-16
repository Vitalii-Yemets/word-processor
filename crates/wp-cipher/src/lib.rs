//! AES, and the two ways of using it that the Office formats ask for.
//!
//! # Why this exists
//!
//! An encrypted `.docx` is a document this program has to be able to open,
//! and opening one means decrypting it: there is no way to read the text
//! without doing the arithmetic. This crate is that arithmetic and nothing
//! else — no key derivation, no file format, no opinion about passwords. It
//! takes a key and some bytes and gives back some other bytes.
//!
//! # What AES is, in one paragraph
//!
//! A block of sixteen bytes is written out as four columns of four, and then
//! put through the same four steps ten, twelve or fourteen times depending on
//! how long the key is. Each byte is replaced by another out of a fixed table
//! (`SubBytes`); the rows are turned sideways by nought, one, two and three
//! places (`ShiftRows`); each column is multiplied by a fixed matrix in a
//! field of two hundred and fifty-six elements (`MixColumns`); and the round's
//! own key is added by exclusive-or (`AddRoundKey`). Decryption is the same
//! four steps inverted and run backwards. The tables below are that field's
//! arithmetic worked out once at the start rather than on every byte.
//!
//! # What is deliberately not here
//!
//! Any cipher but AES, and any mode but the two the Office formats use. RC4,
//! which Office used before 2007, is not here: a document encrypted with it
//! is one this program cannot open, and saying so is better than implementing
//! a cipher that has been broken for twenty years as though it were a feature.
//!
//! Nor is this constant-time against an attacker who can measure the cache.
//! The table lookups leak which entries were touched, which is the well-known
//! weakness of a table-driven AES. What is being defended here is a document
//! on a person's own machine against somebody who has the file and not the
//! password; an attacker who can already run code on the machine and watch the
//! cache has the document itself.

#![forbid(unsafe_code)]

/// The substitution table: the one table AES is built round.
///
/// Each byte's multiplicative inverse in the field, put through a fixed
/// affine map. Written out rather than worked out because it is written out
/// in the standard, and a table copied from the standard can be checked
/// against it.
const S_BOX: [u8; 256] = [
    0x63, 0x7c, 0x77, 0x7b, 0xf2, 0x6b, 0x6f, 0xc5, 0x30, 0x01, 0x67, 0x2b, 0xfe, 0xd7, 0xab, 0x76,
    0xca, 0x82, 0xc9, 0x7d, 0xfa, 0x59, 0x47, 0xf0, 0xad, 0xd4, 0xa2, 0xaf, 0x9c, 0xa4, 0x72, 0xc0,
    0xb7, 0xfd, 0x93, 0x26, 0x36, 0x3f, 0xf7, 0xcc, 0x34, 0xa5, 0xe5, 0xf1, 0x71, 0xd8, 0x31, 0x15,
    0x04, 0xc7, 0x23, 0xc3, 0x18, 0x96, 0x05, 0x9a, 0x07, 0x12, 0x80, 0xe2, 0xeb, 0x27, 0xb2, 0x75,
    0x09, 0x83, 0x2c, 0x1a, 0x1b, 0x6e, 0x5a, 0xa0, 0x52, 0x3b, 0xd6, 0xb3, 0x29, 0xe3, 0x2f, 0x84,
    0x53, 0xd1, 0x00, 0xed, 0x20, 0xfc, 0xb1, 0x5b, 0x6a, 0xcb, 0xbe, 0x39, 0x4a, 0x4c, 0x58, 0xcf,
    0xd0, 0xef, 0xaa, 0xfb, 0x43, 0x4d, 0x33, 0x85, 0x45, 0xf9, 0x02, 0x7f, 0x50, 0x3c, 0x9f, 0xa8,
    0x51, 0xa3, 0x40, 0x8f, 0x92, 0x9d, 0x38, 0xf5, 0xbc, 0xb6, 0xda, 0x21, 0x10, 0xff, 0xf3, 0xd2,
    0xcd, 0x0c, 0x13, 0xec, 0x5f, 0x97, 0x44, 0x17, 0xc4, 0xa7, 0x7e, 0x3d, 0x64, 0x5d, 0x19, 0x73,
    0x60, 0x81, 0x4f, 0xdc, 0x22, 0x2a, 0x90, 0x88, 0x46, 0xee, 0xb8, 0x14, 0xde, 0x5e, 0x0b, 0xdb,
    0xe0, 0x32, 0x3a, 0x0a, 0x49, 0x06, 0x24, 0x5c, 0xc2, 0xd3, 0xac, 0x62, 0x91, 0x95, 0xe4, 0x79,
    0xe7, 0xc8, 0x37, 0x6d, 0x8d, 0xd5, 0x4e, 0xa9, 0x6c, 0x56, 0xf4, 0xea, 0x65, 0x7a, 0xae, 0x08,
    0xba, 0x78, 0x25, 0x2e, 0x1c, 0xa6, 0xb4, 0xc6, 0xe8, 0xdd, 0x74, 0x1f, 0x4b, 0xbd, 0x8b, 0x8a,
    0x70, 0x3e, 0xb5, 0x66, 0x48, 0x03, 0xf6, 0x0e, 0x61, 0x35, 0x57, 0xb9, 0x86, 0xc1, 0x1d, 0x9e,
    0xe1, 0xf8, 0x98, 0x11, 0x69, 0xd9, 0x8e, 0x94, 0x9b, 0x1e, 0x87, 0xe9, 0xce, 0x55, 0x28, 0xdf,
    0x8c, 0xa1, 0x89, 0x0d, 0xbf, 0xe6, 0x42, 0x68, 0x41, 0x99, 0x2d, 0x0f, 0xb0, 0x54, 0xbb, 0x16,
];

/// The round constants: the powers of two in the field, which is what makes
/// each round's key different from the last.
const ROUND_CONSTANTS: [u8; 11] =
    [0x00, 0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80, 0x1b, 0x36];

/// The same table read backwards, for undoing a substitution.
fn inverse_s_box() -> [u8; 256] {
    let mut out = [0u8; 256];
    let mut index = 0;
    while index < 256 {
        out[S_BOX[index] as usize] = index as u8;
        index += 1;
    }
    out
}

/// Multiplication in the field the standard uses, which is the integers
/// modulo the polynomial `x⁸ + x⁴ + x³ + x + 1`.
///
/// Long multiplication in binary, with the one difference that makes it a
/// field: a value that grows past eight bits is brought back by taking the
/// polynomial away, and taking away in this field is exclusive-or.
fn times(left: u8, right: u8) -> u8 {
    let mut product = 0u8;
    let mut left = left;
    let mut right = right;
    while right != 0 {
        if right & 1 != 0 {
            product ^= left;
        }
        let overflowed = left & 0x80 != 0;
        left <<= 1;
        if overflowed {
            left ^= 0x1B;
        }
        right >>= 1;
    }
    product
}

/// A key, expanded into the round keys it stands for.
///
/// Made once and used for every block: expanding a key is a tenth of the work
/// of encrypting one block, and a document is a hundred thousand blocks.
#[derive(Clone)]
pub struct Key {
    /// Four bytes a word, four words a round key.
    schedule: Vec<[u8; 4]>,
    rounds: usize,
}

impl std::fmt::Debug for Key {
    /// How long the key is, and not the key.
    ///
    /// A key written into a log is a key somebody else has. What is worth
    /// knowing about one in a message is its length, and that is all this
    /// says.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "Key of {} bytes", self.length())
    }
}

impl Key {
    /// Expands a key of sixteen, twenty-four or thirty-two bytes.
    ///
    /// Any other length is not an AES key and there is nothing sensible to do
    /// with one, so it is `None` rather than a guess.
    #[must_use]
    pub fn new(key: &[u8]) -> Option<Self> {
        let words = match key.len() {
            16 => 4,
            24 => 6,
            32 => 8,
            _ => return None,
        };
        let rounds = words + 6;
        let mut schedule: Vec<[u8; 4]> =
            key.chunks_exact(4).map(|four| [four[0], four[1], four[2], four[3]]).collect();

        for index in words..(rounds + 1) * 4 {
            let mut word = schedule[index - 1];
            if index % words == 0 {
                // Turned round by one byte, substituted, and the round's own
                // constant added to the first byte.
                word = [
                    S_BOX[word[1] as usize] ^ ROUND_CONSTANTS[index / words],
                    S_BOX[word[2] as usize],
                    S_BOX[word[3] as usize],
                    S_BOX[word[0] as usize],
                ];
            } else if words > 6 && index % words == 4 {
                // The longest key substitutes in the middle of its run as
                // well, which is the one thing that makes its schedule
                // different in shape rather than only in length.
                word = [
                    S_BOX[word[0] as usize],
                    S_BOX[word[1] as usize],
                    S_BOX[word[2] as usize],
                    S_BOX[word[3] as usize],
                ];
            }
            let previous = schedule[index - words];
            schedule.push([
                previous[0] ^ word[0],
                previous[1] ^ word[1],
                previous[2] ^ word[2],
                previous[3] ^ word[3],
            ]);
        }
        Some(Self { schedule, rounds })
    }

    /// How many bytes the key was.
    #[must_use]
    pub fn length(&self) -> usize {
        (self.rounds - 6) * 4
    }

    fn add_round_key(&self, block: &mut [u8; 16], round: usize) {
        for column in 0..4 {
            let word = self.schedule[round * 4 + column];
            for row in 0..4 {
                block[column * 4 + row] ^= word[row];
            }
        }
    }

    /// One block of sixteen bytes, enciphered in place.
    pub fn encrypt_block(&self, block: &mut [u8; 16]) {
        self.add_round_key(block, 0);
        for round in 1..=self.rounds {
            for byte in block.iter_mut() {
                *byte = S_BOX[*byte as usize];
            }
            shift_rows(block);
            // The last round leaves the columns alone, which is what makes
            // decryption possible without a step that has no inverse.
            if round != self.rounds {
                mix_columns(block);
            }
            self.add_round_key(block, round);
        }
    }

    /// And deciphered.
    pub fn decrypt_block(&self, block: &mut [u8; 16]) {
        let inverse = inverse_s_box();
        self.add_round_key(block, self.rounds);
        for round in (0..self.rounds).rev() {
            unshift_rows(block);
            for byte in block.iter_mut() {
                *byte = inverse[*byte as usize];
            }
            self.add_round_key(block, round);
            if round != 0 {
                unmix_columns(block);
            }
        }
    }
}

/// The rows turned sideways by nought, one, two and three places.
///
/// The block is held column by column, so a row is every fourth byte.
fn shift_rows(block: &mut [u8; 16]) {
    let was = *block;
    for column in 0..4 {
        for row in 0..4 {
            block[column * 4 + row] = was[(column + row) % 4 * 4 + row];
        }
    }
}

fn unshift_rows(block: &mut [u8; 16]) {
    let was = *block;
    for column in 0..4 {
        for row in 0..4 {
            block[(column + row) % 4 * 4 + row] = was[column * 4 + row];
        }
    }
}

/// Each column multiplied by the standard's matrix.
fn mix_columns(block: &mut [u8; 16]) {
    for column in block.chunks_exact_mut(4) {
        let was = [column[0], column[1], column[2], column[3]];
        column[0] = times(was[0], 2) ^ times(was[1], 3) ^ was[2] ^ was[3];
        column[1] = was[0] ^ times(was[1], 2) ^ times(was[2], 3) ^ was[3];
        column[2] = was[0] ^ was[1] ^ times(was[2], 2) ^ times(was[3], 3);
        column[3] = times(was[0], 3) ^ was[1] ^ was[2] ^ times(was[3], 2);
    }
}

/// And by its inverse, whose numbers are the reason the field arithmetic is
/// worth having at all.
fn unmix_columns(block: &mut [u8; 16]) {
    for column in block.chunks_exact_mut(4) {
        let was = [column[0], column[1], column[2], column[3]];
        column[0] = times(was[0], 14) ^ times(was[1], 11) ^ times(was[2], 13) ^ times(was[3], 9);
        column[1] = times(was[0], 9) ^ times(was[1], 14) ^ times(was[2], 11) ^ times(was[3], 13);
        column[2] = times(was[0], 13) ^ times(was[1], 9) ^ times(was[2], 14) ^ times(was[3], 11);
        column[3] = times(was[0], 11) ^ times(was[1], 13) ^ times(was[2], 9) ^ times(was[3], 14);
    }
}

/// Block after block, each on its own: the simplest way of using a block
/// cipher and the one the 2007 format uses for its verifier.
///
/// Two identical blocks encipher to two identical blocks, which is why this
/// is the wrong way to encrypt anything long. It is here because the format
/// asks for it, over sixteen and thirty-two byte values where it does no
/// harm.
#[must_use]
pub fn encrypt_ecb(key: &Key, bytes: &[u8]) -> Vec<u8> {
    let mut out = bytes.to_vec();
    for block in out.chunks_exact_mut(16) {
        let mut sixteen: [u8; 16] = block.try_into().expect("a chunk of sixteen");
        key.encrypt_block(&mut sixteen);
        block.copy_from_slice(&sixteen);
    }
    out
}

#[must_use]
pub fn decrypt_ecb(key: &Key, bytes: &[u8]) -> Vec<u8> {
    let mut out = bytes.to_vec();
    for block in out.chunks_exact_mut(16) {
        let mut sixteen: [u8; 16] = block.try_into().expect("a chunk of sixteen");
        key.decrypt_block(&mut sixteen);
        block.copy_from_slice(&sixteen);
    }
    out
}

/// Each block mixed with the one before it, so that the same plain text twice
/// does not give the same cipher text twice.
///
/// The first block is mixed with a starting value instead, which is what the
/// initialisation vector is for. No padding is added or taken away: the
/// formats that use this say for themselves how long the plain text was, and
/// a mode that quietly added bytes would make a document longer every time it
/// was saved.
///
/// Bytes past the last whole block are copied as they are, which is what the
/// Office formats do with the tail of a stream that does not divide.
#[must_use]
pub fn encrypt_cbc(key: &Key, start: &[u8; 16], bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut chain = *start;
    let mut blocks = bytes.chunks_exact(16);
    for block in blocks.by_ref() {
        let mut sixteen = [0u8; 16];
        for (at, byte) in block.iter().enumerate() {
            sixteen[at] = byte ^ chain[at];
        }
        key.encrypt_block(&mut sixteen);
        out.extend_from_slice(&sixteen);
        chain = sixteen;
    }
    out.extend_from_slice(blocks.remainder());
    out
}

#[must_use]
pub fn decrypt_cbc(key: &Key, start: &[u8; 16], bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut chain = *start;
    let mut blocks = bytes.chunks_exact(16);
    for block in blocks.by_ref() {
        let sixteen: [u8; 16] = block.try_into().expect("a chunk of sixteen");
        let mut plain = sixteen;
        key.decrypt_block(&mut plain);
        for (at, byte) in plain.iter_mut().enumerate() {
            *byte ^= chain[at];
        }
        out.extend_from_slice(&plain);
        chain = sixteen;
    }
    out.extend_from_slice(blocks.remainder());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(hex: &str) -> Vec<u8> {
        hex.as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                let digit = |byte: u8| match byte {
                    b'0'..=b'9' => byte - b'0',
                    b'a'..=b'f' => byte - b'a' + 10,
                    _ => byte - b'A' + 10,
                };
                digit(pair[0]) * 16 + digit(pair[1])
            })
            .collect()
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    /// The standard's own worked examples, which are appendix C of FIPS 197:
    /// one plain text, one block, and each of the three key lengths.
    #[test]
    fn the_standards_own_three_examples() {
        let plain = bytes("00112233445566778899aabbccddeeff");
        for (key, expected) in [
            ("000102030405060708090a0b0c0d0e0f", "69c4e0d86a7b0430d8cdb78070b4c55a"),
            (
                "000102030405060708090a0b0c0d0e0f1011121314151617",
                "dda97ca4864cdfe06eaf70a0ec0d7191",
            ),
            (
                "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
                "8ea2b7ca516745bfeafc49904b496089",
            ),
        ] {
            let key = Key::new(&bytes(key)).expect("a key");
            let mut block: [u8; 16] = plain.clone().try_into().expect("a block");
            key.encrypt_block(&mut block);
            assert_eq!(hex(&block), expected, "encrypting with a key of {} bytes", key.length());
            key.decrypt_block(&mut block);
            assert_eq!(hex(&block), hex(&plain), "decrypting it again");
        }
    }

    /// And the recommendation's, which is SP 800-38A: four blocks of chained
    /// cipher text for each key length, where the standard's single block
    /// says nothing about the chaining.
    #[test]
    fn the_recommendations_own_chained_examples() {
        let plain = bytes(concat!(
            "6bc1bee22e409f96e93d7e117393172a",
            "ae2d8a571e03ac9c9eb76fac45af8e51",
            "30c81c46a35ce411e5fbc1191a0a52ef",
            "f69f2445df4f9b17ad2b417be66c3710",
        ));
        let start: [u8; 16] =
            bytes("000102030405060708090a0b0c0d0e0f").try_into().expect("a block");
        for (key, expected) in [
            (
                "2b7e151628aed2a6abf7158809cf4f3c",
                concat!(
                    "7649abac8119b246cee98e9b12e9197d",
                    "5086cb9b507219ee95db113a917678b2",
                    "73bed6b8e3c1743b7116e69e22229516",
                    "3ff1caa1681fac09120eca307586e1a7",
                ),
            ),
            (
                "8e73b0f7da0e6452c810f32b809079e562f8ead2522c6b7b",
                concat!(
                    "4f021db243bc633d7178183a9fa071e8",
                    "b4d9ada9ad7dedf4e5e738763f69145a",
                    "571b242012fb7ae07fa9baac3df102e0",
                    "08b0e27988598881d920a9e64f5615cd",
                ),
            ),
            (
                "603deb1015ca71be2b73aef0857d77811f352c073b6108d72d9810a30914dff4",
                concat!(
                    "f58c4c04d6e5f1ba779eabfb5f7bfbd6",
                    "9cfc4e967edb808d679f777bc6702c7d",
                    "39f23369a9d9bacfa530e26304231461",
                    "b2eb05e2c39be9fcda6c19078c6a9d1b",
                ),
            ),
        ] {
            let key = Key::new(&bytes(key)).expect("a key");
            let sealed = encrypt_cbc(&key, &start, &plain);
            assert_eq!(hex(&sealed), expected, "chaining with a key of {} bytes", key.length());
            assert_eq!(hex(&decrypt_cbc(&key, &start, &sealed)), hex(&plain), "and back again");
        }
    }

    #[test]
    fn a_key_of_the_wrong_length_is_not_a_key() {
        assert!(Key::new(&[]).is_none());
        assert!(Key::new(&[0; 15]).is_none());
        assert!(Key::new(&[0; 20]).is_none());
        assert!(Key::new(&[0; 64]).is_none());
    }

    #[test]
    fn the_field_multiplies_the_way_the_standard_says() {
        // The standard's own example: 0x57 times 0x83 is 0xc1.
        assert_eq!(times(0x57, 0x83), 0xc1);
        assert_eq!(times(0x57, 0x13), 0xfe);
        // And one is one.
        for byte in 0..=255u8 {
            assert_eq!(times(byte, 1), byte);
            assert_eq!(times(byte, 0), 0);
        }
    }

    #[test]
    fn the_tail_that_does_not_fill_a_block_is_carried_across_as_it_is() {
        let key = Key::new(&[7; 32]).expect("a key");
        let plain: Vec<u8> = (0..20u8).collect();
        let sealed = encrypt_cbc(&key, &[0; 16], &plain);
        assert_eq!(sealed.len(), plain.len());
        assert_eq!(&sealed[16..], &plain[16..], "the four bytes over are not enciphered");
        assert_eq!(decrypt_cbc(&key, &[0; 16], &sealed), plain);
    }

    #[test]
    fn the_same_block_twice_gives_the_same_cipher_text_only_without_chaining() {
        let key = Key::new(&[0; 16]).expect("a key");
        let twice = [9u8; 32];
        let plain = encrypt_ecb(&key, &twice);
        assert_eq!(plain[..16], plain[16..], "which is what is wrong with it");
        let chained = encrypt_cbc(&key, &[0; 16], &twice);
        assert_ne!(chained[..16], chained[16..], "and what chaining is for");
    }
}
