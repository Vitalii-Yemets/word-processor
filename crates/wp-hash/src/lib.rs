//! The hashes the Word formats use.
//!
//! # Why they are written out here
//!
//! Because a document says which one it used and expects the program
//! opening it to be able to compute the same number. Word's protection
//! writes a hash of the password; its encryption derives a key from one;
//! its signatures sign one. None of it is a matter of taste: the file
//! names the algorithm — `SHA-512`, or the older `SHA-1` — and there is
//! exactly one right answer for a given input.
//!
//! # What is here
//!
//! SHA-1 and SHA-512, which are the two the Word formats name: SHA-1 for
//! the protection older versions wrote and for the signatures they signed,
//! SHA-512 for what Word writes now. Both are the plainest possible
//! reading of the standard, which is FIPS 180-4, and both are held to the
//! standard's own test vectors below.
//!
//! # What is not here, and why
//!
//! Nothing that would make this look like a general-purpose cryptography
//! library: no constant-time comparisons for secrets this program does not
//! hold, no key exchange, no random numbers — the randomness a salt needs
//! comes from the operating system, through the shell. A password on a
//! `.docx` is not a secret the file keeps: the hash is written in the file
//! in plain sight, and any program may ignore the whole thing. What it is
//! for is saying "somebody meant this document not to be edited", and what
//! this computes is the number that says so.

#![forbid(unsafe_code)]

/// The SHA-1 of some bytes: twenty bytes.
#[must_use]
pub fn sha1(message: &[u8]) -> [u8; 20] {
    let mut state: [u32; 5] = [0x6745_2301, 0xEFCD_AB89, 0x98BA_DCFE, 0x1032_5476, 0xC3D2_E1F0];
    for block in blocks_of_64(message) {
        let mut words = [0u32; 80];
        for (index, word) in words.iter_mut().enumerate().take(16) {
            let at = index * 4;
            *word = u32::from_be_bytes([block[at], block[at + 1], block[at + 2], block[at + 3]]);
        }
        for index in 16..80 {
            let mixed = words[index - 3] ^ words[index - 8] ^ words[index - 14] ^ words[index - 16];
            words[index] = mixed.rotate_left(1);
        }

        let [mut a, mut b, mut c, mut d, mut e] = state;
        for (index, word) in words.iter().enumerate() {
            let (mixed, constant) = match index {
                0..=19 => ((b & c) | (!b & d), 0x5A82_7999),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let next = a
                .rotate_left(5)
                .wrapping_add(mixed)
                .wrapping_add(e)
                .wrapping_add(constant)
                .wrapping_add(*word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = next;
        }
        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
    }

    let mut out = [0u8; 20];
    for (index, word) in state.iter().enumerate() {
        out[index * 4..index * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

/// The SHA-512 of some bytes: sixty-four bytes.
#[must_use]
pub fn sha512(message: &[u8]) -> [u8; 64] {
    let mut state: [u64; 8] = [
        0x6A09_E667_F3BC_C908,
        0xBB67_AE85_84CA_A73B,
        0x3C6E_F372_FE94_F82B,
        0xA54F_F53A_5F1D_36F1,
        0x510E_527F_ADE6_82D1,
        0x9B05_688C_2B3E_6C1F,
        0x1F83_D9AB_FB41_BD6B,
        0x5BE0_CD19_137E_2179,
    ];

    for block in blocks_of_128(message) {
        let mut words = [0u64; 80];
        for (index, word) in words.iter_mut().enumerate().take(16) {
            let at = index * 8;
            *word = u64::from_be_bytes([
                block[at],
                block[at + 1],
                block[at + 2],
                block[at + 3],
                block[at + 4],
                block[at + 5],
                block[at + 6],
                block[at + 7],
            ]);
        }
        for index in 16..80 {
            let first = words[index - 15];
            let second = words[index - 2];
            let s0 = first.rotate_right(1) ^ first.rotate_right(8) ^ (first >> 7);
            let s1 = second.rotate_right(19) ^ second.rotate_right(61) ^ (second >> 6);
            words[index] =
                words[index - 16].wrapping_add(s0).wrapping_add(words[index - 7]).wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = state;
        for (word, constant) in words.iter().zip(ROUND_CONSTANTS.iter()) {
            let s1 = e.rotate_right(14) ^ e.rotate_right(18) ^ e.rotate_right(41);
            let choose = (e & f) ^ (!e & g);
            let first =
                h.wrapping_add(s1).wrapping_add(choose).wrapping_add(*constant).wrapping_add(*word);
            let s0 = a.rotate_right(28) ^ a.rotate_right(34) ^ a.rotate_right(39);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let second = s0.wrapping_add(majority);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(first);
            d = c;
            c = b;
            b = a;
            a = first.wrapping_add(second);
        }
        for (slot, value) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *slot = slot.wrapping_add(value);
        }
    }

    let mut out = [0u8; 64];
    for (index, word) in state.iter().enumerate() {
        out[index * 8..index * 8 + 8].copy_from_slice(&word.to_be_bytes());
    }
    out
}

/// The message in sixty-four byte blocks, with the padding the standard
/// says: a one bit, then zeroes, then the length in bits.
fn blocks_of_64(message: &[u8]) -> Vec<[u8; 64]> {
    let mut padded = message.to_vec();
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&((message.len() as u64) * 8).to_be_bytes());
    padded
        .chunks_exact(64)
        .map(|chunk| {
            let mut block = [0u8; 64];
            block.copy_from_slice(chunk);
            block
        })
        .collect()
}

/// The same in blocks of a hundred and twenty-eight, where the length is
/// written in sixteen bytes rather than eight.
fn blocks_of_128(message: &[u8]) -> Vec<[u8; 128]> {
    let mut padded = message.to_vec();
    padded.push(0x80);
    while padded.len() % 128 != 112 {
        padded.push(0);
    }
    // The standard counts the length in a hundred and twenty-eight bits;
    // no message this program hashes needs the top half of it.
    padded.extend_from_slice(&[0u8; 8]);
    padded.extend_from_slice(&((message.len() as u64) * 8).to_be_bytes());
    padded
        .chunks_exact(128)
        .map(|chunk| {
            let mut block = [0u8; 128];
            block.copy_from_slice(chunk);
            block
        })
        .collect()
}

/// The eighty constants SHA-512 rounds use: the first sixty-four bits of
/// the fractional parts of the cube roots of the first eighty primes.
const ROUND_CONSTANTS: [u64; 80] = [
    0x428A_2F98_D728_AE22,
    0x7137_4491_23EF_65CD,
    0xB5C0_FBCF_EC4D_3B2F,
    0xE9B5_DBA5_8189_DBBC,
    0x3956_C25B_F348_B538,
    0x59F1_11F1_B605_D019,
    0x923F_82A4_AF19_4F9B,
    0xAB1C_5ED5_DA6D_8118,
    0xD807_AA98_A303_0242,
    0x1283_5B01_4570_6FBE,
    0x2431_85BE_4EE4_B28C,
    0x550C_7DC3_D5FF_B4E2,
    0x72BE_5D74_F27B_896F,
    0x80DE_B1FE_3B16_96B1,
    0x9BDC_06A7_25C7_1235,
    0xC19B_F174_CF69_2694,
    0xE49B_69C1_9EF1_4AD2,
    0xEFBE_4786_384F_25E3,
    0x0FC1_9DC6_8B8C_D5B5,
    0x240C_A1CC_77AC_9C65,
    0x2DE9_2C6F_592B_0275,
    0x4A74_84AA_6EA6_E483,
    0x5CB0_A9DC_BD41_FBD4,
    0x76F9_88DA_8311_53B5,
    0x983E_5152_EE66_DFAB,
    0xA831_C66D_2DB4_3210,
    0xB003_27C8_98FB_213F,
    0xBF59_7FC7_BEEF_0EE4,
    0xC6E0_0BF3_3DA8_8FC2,
    0xD5A7_9147_930A_A725,
    0x06CA_6351_E003_826F,
    0x1429_2967_0A0E_6E70,
    0x27B7_0A85_46D2_2FFC,
    0x2E1B_2138_5C26_C926,
    0x4D2C_6DFC_5AC4_2AED,
    0x5338_0D13_9D95_B3DF,
    0x650A_7354_8BAF_63DE,
    0x766A_0ABB_3C77_B2A8,
    0x81C2_C92E_47ED_AEE6,
    0x9272_2C85_1482_353B,
    0xA2BF_E8A1_4CF1_0364,
    0xA81A_664B_BC42_3001,
    0xC24B_8B70_D0F8_9791,
    0xC76C_51A3_0654_BE30,
    0xD192_E819_D6EF_5218,
    0xD699_0624_5565_A910,
    0xF40E_3585_5771_202A,
    0x106A_A070_32BB_D1B8,
    0x19A4_C116_B8D2_D0C8,
    0x1E37_6C08_5141_AB53,
    0x2748_774C_DF8E_EB99,
    0x34B0_BCB5_E19B_48A8,
    0x391C_0CB3_C5C9_5A63,
    0x4ED8_AA4A_E341_8ACB,
    0x5B9C_CA4F_7763_E373,
    0x682E_6FF3_D6B2_B8A3,
    0x748F_82EE_5DEF_B2FC,
    0x78A5_636F_4317_2F60,
    0x84C8_7814_A1F0_AB72,
    0x8CC7_0208_1A64_39EC,
    0x90BE_FFFA_2363_1E28,
    0xA450_6CEB_DE82_BDE9,
    0xBEF9_A3F7_B2C6_7915,
    0xC671_78F2_E372_532B,
    0xCA27_3ECE_EA26_619C,
    0xD186_B8C7_21C0_C207,
    0xEADA_7DD6_CDE0_EB1E,
    0xF57D_4F7F_EE6E_D178,
    0x06F0_67AA_7217_6FBA,
    0x0A63_7DC5_A2C8_98A6,
    0x113F_9804_BEF9_0DAE,
    0x1B71_0B35_131C_471B,
    0x28DB_77F5_2304_7D84,
    0x32CA_AB7B_40C7_2493,
    0x3C9E_BE0A_15C9_BEBC,
    0x431D_67C4_9C10_0D4C,
    0x4CC5_D4BE_CB3E_42B6,
    0x597F_299C_FC65_7E2A,
    0x5FCB_6FAB_3AD6_FAEC,
    0x6C44_198C_4A47_5817,
];

/// Bytes as the letters and figures a file writes them in.
#[must_use]
pub fn to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::with_capacity(bytes.len() * 2), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

/// A message authenticated with a key: HMAC, as RFC 2104 lays it down.
///
/// # What it is for
///
/// An encrypted document carries one of these over its own encrypted bytes.
/// Decrypting tells you what the bytes say; this tells you whether they are
/// the bytes that were written, or whether somebody has changed a block in
/// the middle of a file they could not read. A cipher on its own does not
/// answer that question, and the format is right to ask it.
///
/// # Why it is not simply the hash of the key and the message
///
/// Because these hashes can be continued: given the hash of `key ++ message`
/// and nothing else, somebody can work out the hash of `key ++ message ++
/// more` without knowing the key at all. So the key goes in twice, round the
/// outside and the inside, with a different padding each time, and the outer
/// hash is over a fixed sixty-four or a hundred and twenty-eight bytes, which
/// leaves nothing to continue.
#[must_use]
pub fn hmac_sha512(key: &[u8], message: &[u8]) -> [u8; 64] {
    /// SHA-512 works on blocks of a hundred and twenty-eight bytes, and that
    /// is the length the key is brought to.
    const BLOCK: usize = 128;

    // A key longer than a block is hashed down to fit; a shorter one is
    // padded with zeroes. Both are what the standard says.
    let mut padded = [0u8; BLOCK];
    if key.len() > BLOCK {
        padded[..64].copy_from_slice(&sha512(key));
    } else {
        padded[..key.len()].copy_from_slice(key);
    }

    let mut inner = Vec::with_capacity(BLOCK + message.len());
    inner.extend(padded.iter().map(|byte| byte ^ 0x36));
    inner.extend_from_slice(message);

    let mut outer = Vec::with_capacity(BLOCK + 64);
    outer.extend(padded.iter().map(|byte| byte ^ 0x5C));
    outer.extend_from_slice(&sha512(&inner));
    sha512(&outer)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The standard's own examples, which are what "right" means here.
    #[test]
    fn sha1_agrees_with_the_standards_examples() {
        assert_eq!(to_hex(&sha1(b"abc")), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(to_hex(&sha1(b"")), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(
            to_hex(&sha1(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")),
            "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
        );
        // A million a's, which is the standard's long example.
        let long = vec![b'a'; 1_000_000];
        assert_eq!(to_hex(&sha1(&long)), "34aa973cd4c4daa4f61eeb2bdbad27316534016f");
    }

    #[test]
    fn sha512_agrees_with_the_standards_examples() {
        assert_eq!(
            to_hex(&sha512(b"abc")),
            "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a\
             2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
        );
        assert_eq!(
            to_hex(&sha512(b"")),
            "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce\
             47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e"
        );
        assert_eq!(
            to_hex(&sha512(
                b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmn\
                  hijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu"
                    .iter()
                    .copied()
                    .filter(|byte| !byte.is_ascii_whitespace())
                    .collect::<Vec<u8>>()
                    .as_slice()
            )),
            "8e959b75dae313da8cf4f72814fc143f8f7779c6eb9f7fa17299aeadb6889018\
             501d289e4900f7e4331b99dec4b5433ac7d329eeb6dd26545e96e55b874be909"
        );
        // And one that crosses the block boundary, where the padding is a
        // block of its own.
        let long = vec![b'a'; 1_000_000];
        assert_eq!(
            to_hex(&sha512(&long)),
            "e718483d0ce769644e2e42c7bc15b4638e1f98b13b2044285632a803afa973eb\
             de0ff244877ea60a4cb0432ce577c31beb009c5c2c49aa2e4eadb217ad8cc09b"
        );
    }

    #[test]
    fn a_hash_is_written_as_the_letters_a_file_carries() {
        assert_eq!(to_hex(&[0x00, 0x0f, 0xff]), "000fff");
        assert_eq!(to_hex(&[]), "");
    }

    /// RFC 4231's own examples, which are the test vectors for HMAC over the
    /// SHA-2 family.
    #[test]
    fn the_authenticated_messages_of_the_standards_own_examples() {
        assert_eq!(
            to_hex(&hmac_sha512(&[0x0b; 20], b"Hi There")),
            concat!(
                "87aa7cdea5ef619d4ff0b4241a1d6cb0",
                "2379f4e2ce4ec2787ad0b30545e17cde",
                "daa833b7d6b8a702038b274eaea3f4e4",
                "be9d914eeb61f1702e696c203a126854",
            )
        );
        assert_eq!(
            to_hex(&hmac_sha512(b"Jefe", b"what do ya want for nothing?")),
            concat!(
                "164b7a7bfcf819e2e395fbe73b56e0a3",
                "87bd64222e831fd610270cd7ea250554",
                "9758bf75c05a994a6d034f65f8f0e6fd",
                "caeab1a34d4a6b4b636e070a38bce737",
            )
        );
        // A key longer than the block, which is the case that is got wrong.
        assert_eq!(
            to_hex(&hmac_sha512(
                &[0xaa; 131],
                b"Test Using Larger Than Block-Size Key - Hash Key First"
            )),
            concat!(
                "80b24263c7c1a3ebb71493c1dd7be8b4",
                "9b46d1f41b4aeec1121b013783f8f352",
                "6b56d037e05f2598bd0fd2215d6a1e52",
                "95e64f73f63f0aec8b915a985d786598",
            )
        );
    }

    #[test]
    fn a_message_that_was_changed_does_not_authenticate() {
        let key = b"the key";
        let sealed = hmac_sha512(key, b"pay Alice ten pounds");
        assert_ne!(sealed, hmac_sha512(key, b"pay Alice ten pouNds"));
        assert_ne!(sealed, hmac_sha512(b"another key", b"pay Alice ten pounds"));
    }
}
