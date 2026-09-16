//! Bytes written as letters.
//!
//! # Why a text crate carries it
//!
//! Because that is what it is: a way of writing bytes so that something
//! which can only carry text can carry them. A picture inside a web page,
//! a picture inside a mail message, the hash of a password inside an XML
//! attribute — all three are bytes in a place where only letters may go,
//! and all three use the same sixty-four of them.
//!
//! # Two shapes of the same thing
//!
//! Mail wraps its lines at seventy-six letters and ends them with a return
//! and a newline; XML wants one unbroken run. So the wrapping is asked for
//! rather than assumed — [`encode`] gives the plain run, [`encode_wrapped`]
//! the one mail wants — and reading takes either, along with the variant
//! that writes `-` and `_` so that it can go in an address.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// The bytes as one unbroken run of letters.
#[must_use]
pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let mut value: u32 = 0;
        for (index, byte) in chunk.iter().enumerate() {
            value |= u32::from(*byte) << (16 - 8 * index);
        }
        for index in 0..4 {
            if index <= chunk.len() {
                let digit = (value >> (18 - 6 * index)) & 63;
                out.push(ALPHABET[digit as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The same, in lines of seventy-six letters ending in a return and a
/// newline, which is what a mail message wants.
#[must_use]
pub fn encode_wrapped(bytes: &[u8]) -> String {
    let run = encode(bytes);
    let mut out = String::with_capacity(run.len() + run.len() / 76 * 2 + 2);
    for (index, character) in run.chars().enumerate() {
        if index > 0 && index % 76 == 0 {
            out.push_str("\r\n");
        }
        out.push(character);
    }
    if !out.is_empty() {
        out.push_str("\r\n");
    }
    out
}

/// The bytes back.
///
/// Anything that is not one of the sixty-four letters is passed over —
/// the line breaks a mail message puts in, the padding at the end — and
/// the two letters an address-safe run uses are taken as the two they
/// stand for.
#[must_use]
pub fn decode(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    let mut value: u32 = 0;
    let mut held = 0;
    for &byte in bytes {
        let digit = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => continue,
        };
        value = (value << 6) | u32::from(digit);
        held += 6;
        if held >= 8 {
            held -= 8;
            out.push((value >> held) as u8);
            value &= (1 << held) - 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_examples_everybody_writes_it_with() {
        // The standard's own, which is RFC 4648.
        assert_eq!(encode(b""), "");
        assert_eq!(encode(b"f"), "Zg==");
        assert_eq!(encode(b"fo"), "Zm8=");
        assert_eq!(encode(b"foo"), "Zm9v");
        assert_eq!(encode(b"foob"), "Zm9vYg==");
        assert_eq!(encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn what_was_written_reads_back() {
        let bytes: Vec<u8> = (0..=255u8).collect();
        assert_eq!(decode(encode(&bytes).as_bytes()), bytes);
        assert_eq!(decode(encode_wrapped(&bytes).as_bytes()), bytes, "line breaks and all");
    }

    #[test]
    fn a_mail_message_gets_its_lines_broken() {
        let long = vec![b'a'; 200];
        let wrapped = encode_wrapped(&long);
        for line in wrapped.lines() {
            assert!(line.len() <= 76, "a line is at most seventy-six letters: {}", line.len());
        }
        assert!(wrapped.ends_with("\r\n"));
    }

    #[test]
    fn the_letters_an_address_uses_are_read_as_the_two_they_stand_for() {
        assert_eq!(decode(b"-_"), decode(b"+/"));
    }
}
