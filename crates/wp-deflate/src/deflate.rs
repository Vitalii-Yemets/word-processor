//! DEFLATE (RFC 1951) compression: LZ77 matching plus fixed Huffman codes.
//!
//! That is enough for `.docx`: a package is almost entirely XML, and on text the
//! fixed codes come close to what dynamic codes would achieve. When the data does
//! not compress at all (already-packed images, embedded fonts) the encoder falls
//! back to stored blocks, so the output is never larger than the input.
//!
//! Dynamic Huffman codes are the next optimization step; they affect only the
//! size of the files we write, never their validity.

use crate::adler32::Adler32;
use crate::tables::{
    distance_code, length_code, DISTANCE_BASE, DISTANCE_EXTRA, END_OF_BLOCK, LENGTH_BASE,
    LENGTH_EXTRA, MAX_MATCH, MIN_MATCH, WINDOW_SIZE,
};

/// Size of the match-finder hash table. 32 Ki slots, the same as zlib uses.
const HASH_BITS: u32 = 15;
const HASH_SIZE: usize = 1 << HASH_BITS;

/// How many candidates to walk in one hash chain. A trade-off between density
/// and time: beyond this the gain is hundredths of a percent.
const MAX_CHAIN: usize = 128;

/// Largest payload of a single stored block (the LEN field is 16 bits).
const MAX_STORED_BLOCK: usize = 65_535;

/// Sentinel for "no such position" in the hash table.
const NO_POSITION: u32 = u32::MAX;

// --- Bit-level output ------------------------------------------------------

/// Writes bits least-significant first, as RFC 1951 requires.
struct BitWriter {
    out: Vec<u8>,
    buffer: u32,
    bits_in_buffer: u32,
}

impl BitWriter {
    fn with_capacity(capacity: usize) -> Self {
        Self { out: Vec::with_capacity(capacity), buffer: 0, bits_in_buffer: 0 }
    }

    /// Writes the low `count` bits of `value`, least-significant bit first.
    /// This is how header fields and the extra bits of lengths and distances go out.
    fn write_bits(&mut self, value: u32, count: u32) {
        debug_assert!(count <= 16);
        if count == 0 {
            return;
        }
        self.buffer |= (value & ((1u32 << count) - 1)) << self.bits_in_buffer;
        self.bits_in_buffer += count;
        while self.bits_in_buffer >= 8 {
            self.out.push((self.buffer & 0xFF) as u8);
            self.buffer >>= 8;
            self.bits_in_buffer -= 8;
        }
    }

    /// Writes a Huffman code. Codes are defined most-significant bit first but
    /// travel least-significant first, so they are reversed (RFC 1951, 3.1.1).
    fn write_code(&mut self, code: u32, count: u32) {
        self.write_bits(reverse_bits(code, count), count);
    }

    /// Pads with zero bits up to the next byte boundary.
    fn align_to_byte(&mut self) {
        if self.bits_in_buffer > 0 {
            self.out.push((self.buffer & 0xFF) as u8);
            self.buffer = 0;
            self.bits_in_buffer = 0;
        }
    }

    fn finish(mut self) -> Vec<u8> {
        self.align_to_byte();
        self.out
    }
}

/// Reverses the low `count` bits of a value.
fn reverse_bits(mut value: u32, count: u32) -> u32 {
    let mut result = 0u32;
    for _ in 0..count {
        result = (result << 1) | (value & 1);
        value >>= 1;
    }
    result
}

/// Code and bit length for a symbol of the fixed literal tree
/// (RFC 1951, section 3.2.6). The code is returned most-significant bit first.
fn fixed_literal_code(symbol: u16) -> (u32, u32) {
    match symbol {
        0..=143 => (0x30 + u32::from(symbol), 8),
        144..=255 => (0x190 + u32::from(symbol) - 144, 9),
        256..=279 => (u32::from(symbol) - 256, 7),
        280..=287 => (0xC0 + u32::from(symbol) - 280, 8),
        _ => unreachable!("symbol outside the fixed alphabet: {symbol}"),
    }
}

// --- Match finding ---------------------------------------------------------

/// Hash of three bytes — the key under which match candidates are looked up.
fn hash3(bytes: &[u8]) -> usize {
    let value =
        (usize::from(bytes[0]) << 10) ^ (usize::from(bytes[1]) << 5) ^ usize::from(bytes[2]);
    value & (HASH_SIZE - 1)
}

/// Position index: `head` holds the most recent position for a hash, `prev` the
/// previous position sharing that hash. Together they form a candidate chain.
struct MatchFinder {
    head: Vec<u32>,
    prev: Vec<u32>,
}

impl MatchFinder {
    fn new(input_len: usize) -> Self {
        Self { head: vec![NO_POSITION; HASH_SIZE], prev: vec![NO_POSITION; input_len.max(1)] }
    }

    fn insert(&mut self, input: &[u8], position: usize) {
        if position + MIN_MATCH > input.len() {
            return;
        }
        let slot = hash3(&input[position..position + MIN_MATCH]);
        self.prev[position] = self.head[slot];
        self.head[slot] = position as u32;
    }

    /// Finds the longest match for `position`, returning `(length, distance)`.
    fn find(&self, input: &[u8], position: usize) -> Option<(usize, usize)> {
        if position + MIN_MATCH > input.len() {
            return None;
        }

        let max_length = (input.len() - position).min(MAX_MATCH);
        let window_start = position.saturating_sub(WINDOW_SIZE);

        let mut best_length = 0usize;
        let mut best_distance = 0usize;

        let mut candidate = self.head[hash3(&input[position..position + MIN_MATCH])];
        let mut visited = 0usize;

        while candidate != NO_POSITION && visited < MAX_CHAIN {
            let start = candidate as usize;
            if start < window_start || start >= position {
                break;
            }

            // Quick rejection: if the byte at the current best length differs,
            // this candidate cannot beat the current best.
            let worth_checking = best_length == 0
                || (position + best_length < input.len()
                    && input[start + best_length] == input[position + best_length]);

            if worth_checking {
                let mut length = 0usize;
                while length < max_length && input[start + length] == input[position + length] {
                    length += 1;
                }
                if length > best_length {
                    best_length = length;
                    best_distance = position - start;
                    if length == max_length {
                        break;
                    }
                }
            }

            candidate = self.prev[start];
            visited += 1;
        }

        if best_length >= MIN_MATCH {
            Some((best_length, best_distance))
        } else {
            None
        }
    }
}

// --- Entry points ----------------------------------------------------------

/// Compresses data into a raw DEFLATE stream.
///
/// If compression does not pay off, stored blocks are emitted instead, so the
/// result is never meaningfully larger than the input.
#[must_use]
pub fn compress(input: &[u8]) -> Vec<u8> {
    let compressed = compress_fixed(input);
    if compressed.len() <= stored_size(input.len()) {
        compressed
    } else {
        compress_stored(input)
    }
}

/// Size the data would occupy if laid out as stored blocks.
fn stored_size(input_len: usize) -> usize {
    if input_len == 0 {
        return 5;
    }
    let blocks = input_len.div_ceil(MAX_STORED_BLOCK);
    input_len + blocks * 5
}

/// Lays the data out in stored (uncompressed) blocks. Useful for content that is
/// already compressed — JPEG, PNG, embedded fonts — where running the matcher
/// would only waste time.
#[must_use]
pub fn compress_stored(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(stored_size(input.len()));

    if input.is_empty() {
        // An empty final stored block: BFINAL=1, BTYPE=00, LEN=0, ~LEN=0xFFFF.
        out.extend_from_slice(&[0x01, 0x00, 0x00, 0xFF, 0xFF]);
        return out;
    }

    let mut offset = 0usize;
    while offset < input.len() {
        let end = (offset + MAX_STORED_BLOCK).min(input.len());
        let chunk = &input[offset..end];
        let is_final = end == input.len();

        // The header fits exactly one byte: BFINAL in the low bit, BTYPE=00,
        // and the remaining five bits are the alignment padding.
        out.push(u8::from(is_final));

        let length = chunk.len() as u16;
        out.extend_from_slice(&length.to_le_bytes());
        out.extend_from_slice(&(!length).to_le_bytes());
        out.extend_from_slice(chunk);

        offset = end;
    }

    out
}

/// Compresses everything into a single block using the fixed Huffman codes.
fn compress_fixed(input: &[u8]) -> Vec<u8> {
    let mut writer = BitWriter::with_capacity(input.len() / 2 + 16);

    writer.write_bits(1, 1); // BFINAL: this is the only and final block
    writer.write_bits(1, 2); // BTYPE = 01, fixed codes

    let mut finder = MatchFinder::new(input.len());
    let mut position = 0usize;

    while position < input.len() {
        match finder.find(input, position) {
            Some((length, distance)) => {
                write_match(&mut writer, length, distance);
                // Positions covered by the match still have to be indexed;
                // skipping them makes subsequent matches noticeably shorter.
                for offset in position..position + length {
                    finder.insert(input, offset);
                }
                position += length;
            }
            None => {
                let (code, bits) = fixed_literal_code(u16::from(input[position]));
                writer.write_code(code, bits);
                finder.insert(input, position);
                position += 1;
            }
        }
    }

    let (code, bits) = fixed_literal_code(END_OF_BLOCK);
    writer.write_code(code, bits);

    writer.finish()
}

/// Writes one length/distance pair.
fn write_match(writer: &mut BitWriter, length: usize, distance: usize) {
    let code = length_code(length);
    let (huffman, bits) = fixed_literal_code(257 + code as u16);
    writer.write_code(huffman, bits);
    writer.write_bits((length - LENGTH_BASE[code] as usize) as u32, u32::from(LENGTH_EXTRA[code]));

    let code = distance_code(distance);
    // In the fixed tree, distances use five-bit codes numerically equal to the
    // symbol number.
    writer.write_code(code as u32, 5);
    writer.write_bits(
        (distance - DISTANCE_BASE[code] as usize) as u32,
        u32::from(DISTANCE_EXTRA[code]),
    );
}

/// Compresses data into a zlib wrapper (RFC 1950). Needed when writing PNG.
#[must_use]
pub fn compress_zlib(input: &[u8]) -> Vec<u8> {
    let body = compress(input);
    let mut out = Vec::with_capacity(body.len() + 6);

    // CMF: method 8 (deflate), 32 KiB window. FLG is chosen so that the two
    // header bytes form a multiple of 31, as RFC 1950 requires.
    out.push(0x78);
    out.push(0x9C);
    out.extend_from_slice(&body);

    let mut checksum = Adler32::new();
    checksum.update(input);
    out.extend_from_slice(&checksum.finish().to_be_bytes());

    out
}
