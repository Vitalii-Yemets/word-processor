//! The scheme every stream of a Visual Basic project is squeezed with.
//!
//! [MS-OVBA] §2.4.1, and it is small enough to write out in full: a signature
//! byte, then chunks. A chunk holds up to four thousand and ninety-six bytes
//! of text and is written as groups of eight tokens with a flag byte in front
//! saying which of the eight are literals and which are copies. A copy says
//! how far back to go and how much to take, in a field whose width grows as
//! the chunk fills — four bits of offset at the start of a chunk, twelve by
//! the end — which is the only part of it anybody gets wrong.
//!
//! # Why there is a compressor here as well
//!
//! Because a reader that nothing can make a file for is a reader nobody has
//! tested. Every test in this crate builds a project and reads it back, and
//! building one means writing the compressed form. It is also half of what
//! editing a macro will need, which is a later item and not this one.

/// What a compressed stream begins with.
const SIGNATURE: u8 = 0x01;

/// How much text one chunk holds, and the most its compressed form may be.
const CHUNK: usize = 4096;

/// Why a compressed stream could not be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// It does not begin the way a compressed stream does.
    NotCompressed,
    /// A chunk header says something the format does not allow.
    BadChunk,
    /// A copy reaches back further than the chunk it is in.
    BadCopy,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotCompressed => write!(f, "not a compressed stream"),
            Self::BadChunk => write!(f, "a chunk header is not as the format says"),
            Self::BadCopy => write!(f, "a copy reaches back before its chunk"),
        }
    }
}

impl std::error::Error for Error {}

/// How many bits of a copy token are the offset, where a chunk has this many
/// bytes in it already.
///
/// The field grows with the chunk: there is no point spending twelve bits
/// saying "four bytes back" when only four bytes have been written. Four bits
/// at the least and twelve at the most.
fn offset_bits(written: usize) -> u32 {
    let mut bits = 4u32;
    while (1usize << bits) < written && bits < 12 {
        bits += 1;
    }
    bits
}

/// The text a compressed stream holds.
///
/// Lenient about the end: a chunk header that claims more bytes than the file
/// has is read as far as the file goes. This reads other people's files, and
/// what is there is worth more than a complaint about what is not.
pub fn decompress(bytes: &[u8]) -> Result<Vec<u8>, Error> {
    let Some((signature, mut rest)) = bytes.split_first() else {
        return Err(Error::NotCompressed);
    };
    if *signature != SIGNATURE {
        return Err(Error::NotCompressed);
    }

    let mut out = Vec::new();
    while rest.len() >= 2 {
        let header = u16::from_le_bytes([rest[0], rest[1]]);
        if (header >> 12) & 0b0111 != 0b011 {
            return Err(Error::BadChunk);
        }
        let size = (header & 0x0FFF) as usize + 3;
        let end = size.min(rest.len());
        let data = &rest[2..end];
        if header & 0x8000 == 0 {
            out.extend_from_slice(data);
        } else {
            chunk(data, &mut out)?;
        }
        rest = &rest[end..];
    }
    Ok(out)
}

/// One compressed chunk, onto the end of what is there.
fn chunk(data: &[u8], out: &mut Vec<u8>) -> Result<(), Error> {
    let start = out.len();
    let mut at = 0usize;
    while at < data.len() {
        let flags = data[at];
        at += 1;
        for bit in 0..8u8 {
            if at >= data.len() {
                break;
            }
            if flags & (1 << bit) == 0 {
                out.push(data[at]);
                at += 1;
                continue;
            }

            if at + 1 >= data.len() {
                return Err(Error::BadChunk);
            }
            let token = u16::from_le_bytes([data[at], data[at + 1]]);
            at += 2;

            let written = out.len() - start;
            if written == 0 {
                return Err(Error::BadCopy);
            }
            let bits = offset_bits(written);
            let length_mask = 0xFFFFu16 >> bits;
            let length = (token & length_mask) as usize + 3;
            let offset = ((token & !length_mask) >> (16 - bits)) as usize + 1;
            if offset > written {
                return Err(Error::BadCopy);
            }

            // Byte at a time, because a copy may reach into what it is
            // writing: that is how a run of the same byte is written down.
            let from = out.len() - offset;
            for index in 0..length {
                let byte = out[from + index];
                out.push(byte);
            }
        }
    }
    Ok(())
}

/// Text as a compressed stream.
///
/// A chunk is closed at four thousand and ninety-six bytes of text, or sooner
/// if its compressed form would not fit in a chunk — text that does not
/// compress takes more room written as tokens than it does as itself, and a
/// chunk has one length field for both.
#[must_use]
pub fn compress(bytes: &[u8]) -> Vec<u8> {
    let mut out = vec![SIGNATURE];
    let mut at = 0usize;
    while at < bytes.len() {
        let (data, taken) = compress_chunk(&bytes[at..]);
        at += taken;
        #[allow(clippy::cast_possible_truncation)]
        let header = 0xB000u16 | ((data.len() + 2 - 3) as u16 & 0x0FFF);
        out.extend_from_slice(&header.to_le_bytes());
        out.extend_from_slice(&data);
    }
    out
}

/// One chunk's worth, and how much text went into it.
fn compress_chunk(input: &[u8]) -> (Vec<u8>, usize) {
    let mut data = Vec::new();
    let mut taken = 0usize;

    // A group is a flag byte and up to eight tokens, so the most one can add
    // is seventeen bytes. Stopping that far short of the limit costs a few
    // bytes a chunk and saves having to unpick a group that did not fit.
    while taken < input.len() && taken < CHUNK && data.len() + 17 <= CHUNK {
        let flags_at = data.len();
        data.push(0);
        let mut flags = 0u8;
        for bit in 0..8u8 {
            if taken >= input.len() || taken >= CHUNK {
                break;
            }
            let bits = offset_bits(taken);
            let length_mask = 0xFFFFu16 >> bits;
            let (offset, length) = longest_match(input, taken, bits, length_mask as usize + 3);
            if length >= 3 {
                #[allow(clippy::cast_possible_truncation)]
                let token =
                    (((offset - 1) as u16) << (16 - bits)) | ((length - 3) as u16 & length_mask);
                data.extend_from_slice(&token.to_le_bytes());
                flags |= 1 << bit;
                taken += length;
            } else {
                data.push(input[taken]);
                taken += 1;
            }
        }
        data[flags_at] = flags;
    }
    (data, taken)
}

/// The longest stretch already written that the text at `at` repeats.
///
/// A match may reach into itself — an offset of one and a length of ten is
/// the same byte ten times — because the reader copies a byte at a time.
fn longest_match(input: &[u8], at: usize, bits: u32, most: usize) -> (usize, usize) {
    let window = at.min(1usize << bits);
    let (mut best_offset, mut best_length) = (0usize, 0usize);
    for offset in 1..=window {
        let from = at - offset;
        let mut length = 0usize;
        while length < most
            && at + length < input.len()
            && input[from + length] == input[at + length]
        {
            length += 1;
        }
        if length > best_length {
            best_offset = offset;
            best_length = length;
        }
        if best_length == most {
            break;
        }
    }
    (best_offset, best_length)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stream_that_is_not_one_is_refused() {
        assert_eq!(decompress(&[]), Err(Error::NotCompressed));
        assert_eq!(decompress(&[0x02, 0, 0]), Err(Error::NotCompressed));
    }

    #[test]
    fn a_stream_written_out_by_hand_reads_as_it_was_meant_to() {
        // Not this crate's own compressor reading its own output, which
        // would agree with itself whatever it did. Every byte here was
        // worked out against the format: a chunk header saying eleven plus
        // three, a flag byte of eight literals, then a flag byte with one
        // copy in it — three bytes back, five long, which runs off the end of
        // what has been written and carries on into what it is writing.
        let stream = [
            0x01, // a compressed stream
            0x0B, 0xB0, // a compressed chunk, twelve bytes of data
            0x00, b'a', b'b', b'c', b'a', b'b', b'c', b'a', b'b', // eight literals
            0x01, 0x02, 0x20, // one copy: back three, five bytes
        ];
        assert_eq!(decompress(&stream).expect("a stream built by hand"), b"abcabcabcabca");
    }

    #[test]
    fn what_is_compressed_comes_back() {
        for text in [
            &b""[..],
            b"Sub Hello()\r\n    MsgBox \"Hello\"\r\nEnd Sub\r\n",
            b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            b"#aaabcdefaaaaghijklmnopqrstuvwxyz",
        ] {
            let there_and_back = decompress(&compress(text)).expect("reading it back");
            assert_eq!(there_and_back, text, "{}", String::from_utf8_lossy(text));
        }
    }

    #[test]
    fn a_stream_longer_than_one_chunk_comes_back_whole() {
        // Three chunks and a bit, of text that does compress, and of text
        // that does not: the second is what the chunk limit is about.
        let squeezable: Vec<u8> = "Dim counter As Long\r\n".repeat(800).into_bytes();
        assert!(squeezable.len() > 3 * CHUNK);
        assert_eq!(decompress(&compress(&squeezable)).expect("reading"), squeezable);

        let stubborn: Vec<u8> =
            (0..14_000u32).map(|n| (n.wrapping_mul(2_654_435_761) >> 13) as u8).collect();
        let written = compress(&stubborn);
        assert_eq!(decompress(&written).expect("reading"), stubborn);
    }

    #[test]
    fn the_offset_field_grows_with_the_chunk() {
        // Four bits while a chunk holds sixteen bytes or fewer, and twelve
        // once it holds four thousand: getting this wrong reads every copy
        // token in the file at the wrong width.
        assert_eq!(offset_bits(1), 4);
        assert_eq!(offset_bits(16), 4);
        assert_eq!(offset_bits(17), 5);
        assert_eq!(offset_bits(4096), 12);
        assert_eq!(offset_bits(9000), 12);
    }

    #[test]
    fn a_copy_that_reaches_before_its_chunk_is_refused() {
        // A flag byte saying "copy", at the very start of a chunk, where
        // there is nothing yet to copy from.
        let broken = [0x01, 0x03, 0xB0, 0x01, 0x00, 0x00];
        assert_eq!(decompress(&broken), Err(Error::BadCopy));
    }
}
