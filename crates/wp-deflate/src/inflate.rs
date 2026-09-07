//! DEFLATE (RFC 1951) and zlib (RFC 1950) decompression.
//!
//! The decoder is written to accept untrusted files safely: it never panics on
//! malformed input, it returns an [`Error`] instead, and it can cap the size of
//! its output — protection against "zip bombs", where a few kilobytes expand
//! into gigabytes.

use crate::adler32::Adler32;
use crate::tables::{
    CODE_LENGTH_ORDER, DISTANCE_BASE, DISTANCE_EXTRA, LENGTH_BASE, LENGTH_EXTRA,
};

/// Why a stream could not be decompressed.
///
/// These are diagnostics for developers and log files; they are deliberately in
/// English and are never shown to the user as-is. Anything the user sees is
/// produced by the localized presentation layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// Input ran out before the stream ended.
    UnexpectedEof,
    /// Block type 0b11 is reserved and unused.
    ReservedBlockType,
    /// LEN and ~LEN of a stored block are not complements of each other.
    CorruptStoredBlock,
    /// A bit pattern was read that no symbol in the Huffman table maps to.
    InvalidHuffmanCode,
    /// The code-length table describes an over-subscribed or incomplete code.
    InvalidCodeLengths,
    /// A back-reference points further back than the start of the output.
    DistanceTooFar,
    /// A length or distance code from the reserved range was used.
    InvalidSymbol,
    /// Output exceeded the configured limit — most likely a decompression bomb.
    OutputLimitExceeded,
    /// The zlib header is corrupt or requests an unsupported preset dictionary.
    InvalidZlibHeader,
    /// The trailing Adler-32 checksum of a zlib stream did not match.
    ChecksumMismatch,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let text = match self {
            Self::UnexpectedEof => "stream ends in the middle of the data",
            Self::ReservedBlockType => "reserved DEFLATE block type",
            Self::CorruptStoredBlock => "corrupt stored-block header",
            Self::InvalidHuffmanCode => "invalid Huffman code",
            Self::InvalidCodeLengths => "invalid code-length table",
            Self::DistanceTooFar => "back-reference points outside the window",
            Self::InvalidSymbol => "reserved length or distance code",
            Self::OutputLimitExceeded => "decompressed size limit exceeded",
            Self::InvalidZlibHeader => "corrupt zlib header",
            Self::ChecksumMismatch => "Adler-32 checksum mismatch",
        };
        f.write_str(text)
    }
}

impl std::error::Error for Error {}

// --- Bit-level input -------------------------------------------------------

/// Reads bits least-significant first, as RFC 1951 requires.
struct BitReader<'a> {
    data: &'a [u8],
    /// Index of the next byte not yet pulled into the buffer.
    next_byte: usize,
    buffer: u32,
    bits_in_buffer: u32,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, next_byte: 0, buffer: 0, bits_in_buffer: 0 }
    }

    /// Tops the buffer up to `count` bits. Valid for `count` up to 24: the shift
    /// applied to an incoming byte then stays within `u32`.
    fn fill(&mut self, count: u32) -> Result<(), Error> {
        debug_assert!(count <= 24);
        while self.bits_in_buffer < count {
            let byte = *self.data.get(self.next_byte).ok_or(Error::UnexpectedEof)?;
            self.buffer |= u32::from(byte) << self.bits_in_buffer;
            self.next_byte += 1;
            self.bits_in_buffer += 8;
        }
        Ok(())
    }

    fn read(&mut self, count: u32) -> Result<u32, Error> {
        if count == 0 {
            return Ok(0);
        }
        self.fill(count)?;
        let value = self.buffer & ((1u32 << count) - 1);
        self.buffer >>= count;
        self.bits_in_buffer -= count;
        Ok(value)
    }

    /// Discards bits up to the next byte boundary, as required before a stored block.
    fn align_to_byte(&mut self) {
        let extra = self.bits_in_buffer % 8;
        self.buffer >>= extra;
        self.bits_in_buffer -= extra;
    }

    /// Position in the source slice, accounting for bytes already sitting in the
    /// buffer. Only meaningful right after [`Self::align_to_byte`].
    fn byte_position(&self) -> usize {
        self.next_byte - (self.bits_in_buffer / 8) as usize
    }

    /// Resumes byte-wise reading at the given position, discarding the buffer.
    fn seek_bytes(&mut self, position: usize) {
        self.next_byte = position;
        self.buffer = 0;
        self.bits_in_buffer = 0;
    }
}

// --- Canonical Huffman code ------------------------------------------------

/// A canonical Huffman code described only by its symbols' code lengths
/// (RFC 1951, section 3.2.2). Decoding walks bit by bit: slower than a
/// direct-lookup table, but considerably simpler and free of `unsafe`.
struct Huffman {
    /// How many symbols have a code of each length 0..=15.
    counts: [u16; 16],
    /// Symbols sorted by (code length, symbol value).
    symbols: Vec<u16>,
}

impl Huffman {
    /// Builds the code and reports how many codes were left unused. Zero means a
    /// complete code; a positive value means an incomplete one, which RFC 1951
    /// only permits in degenerate cases.
    fn build(lengths: &[u8]) -> Result<(Self, i32), Error> {
        let mut counts = [0u16; 16];
        for &length in lengths {
            if length > 15 {
                return Err(Error::InvalidCodeLengths);
            }
            counts[length as usize] += 1;
        }
        counts[0] = 0;

        // Kraft's inequality: an over-subscribed code is never valid.
        let mut remaining: i32 = 1;
        for length in 1..16 {
            remaining <<= 1;
            remaining -= i32::from(counts[length]);
            if remaining < 0 {
                return Err(Error::InvalidCodeLengths);
            }
        }

        let mut offsets = [0u16; 16];
        for length in 1..15 {
            offsets[length + 1] = offsets[length] + counts[length];
        }

        let mut symbols = vec![0u16; lengths.len()];
        for (symbol, &length) in lengths.iter().enumerate() {
            if length != 0 {
                symbols[offsets[length as usize] as usize] = symbol as u16;
                offsets[length as usize] += 1;
            }
        }

        Ok((Self { counts, symbols }, remaining))
    }

    /// How many symbols take part in the code at all.
    fn symbol_count(&self) -> u16 {
        self.counts[1..].iter().sum()
    }

    fn decode(&self, reader: &mut BitReader<'_>) -> Result<u16, Error> {
        // Standard canonical-code walk: at each step compare the accumulated
        // code against the range of codes of the current length.
        let mut code: i32 = 0;
        let mut first: i32 = 0;
        let mut index: i32 = 0;

        for length in 1..16 {
            code |= reader.read(1)? as i32;
            let count = i32::from(self.counts[length]);
            if code - first < count {
                let position = (index + (code - first)) as usize;
                return self.symbols.get(position).copied().ok_or(Error::InvalidHuffmanCode);
            }
            index += count;
            first = (first + count) << 1;
            code <<= 1;
        }

        Err(Error::InvalidHuffmanCode)
    }
}

/// The fixed code trees from RFC 1951, section 3.2.6.
fn fixed_trees() -> (Huffman, Huffman) {
    let mut literal_lengths = [0u8; 288];
    literal_lengths[0..144].fill(8);
    literal_lengths[144..256].fill(9);
    literal_lengths[256..280].fill(7);
    literal_lengths[280..288].fill(8);

    let distance_lengths = [5u8; 30];

    // These tables are compile-time constants and are valid by construction.
    let literal = Huffman::build(&literal_lengths).expect("fixed literal tree").0;
    let distance = Huffman::build(&distance_lengths).expect("fixed distance tree").0;
    (literal, distance)
}

// --- Decompression ---------------------------------------------------------

/// Decompresses a raw DEFLATE stream with no cap on output size.
///
/// Use [`inflate_limited`] for data coming from untrusted files.
pub fn inflate(data: &[u8]) -> Result<Vec<u8>, Error> {
    inflate_limited(data, usize::MAX)
}

/// Decompresses a DEFLATE stream, failing as soon as output exceeds `max_output`.
pub fn inflate_limited(data: &[u8], max_output: usize) -> Result<Vec<u8>, Error> {
    let mut reader = BitReader::new(data);
    // Sensible initial capacity: XML typically compresses about 4:1.
    let capacity = data.len().saturating_mul(4).min(max_output).min(1 << 20);
    let mut out = Vec::with_capacity(capacity);

    loop {
        let is_final = reader.read(1)? == 1;
        match reader.read(2)? {
            0 => inflate_stored(&mut reader, &mut out, max_output)?,
            1 => {
                let (literal, distance) = fixed_trees();
                inflate_block(&mut reader, &mut out, &literal, &distance, max_output)?;
            }
            2 => {
                let (literal, distance) = read_dynamic_trees(&mut reader)?;
                inflate_block(&mut reader, &mut out, &literal, &distance, max_output)?;
            }
            _ => return Err(Error::ReservedBlockType),
        }
        if is_final {
            break;
        }
    }

    Ok(out)
}

/// Stored block: align to a byte, read LEN/~LEN, copy the bytes verbatim.
fn inflate_stored(
    reader: &mut BitReader<'_>,
    out: &mut Vec<u8>,
    max_output: usize,
) -> Result<(), Error> {
    reader.align_to_byte();
    let length = reader.read(16)? as usize;
    let complement = reader.read(16)? as usize;
    if length ^ 0xFFFF != complement {
        return Err(Error::CorruptStoredBlock);
    }

    let start = reader.byte_position();
    let end = start.checked_add(length).ok_or(Error::UnexpectedEof)?;
    let bytes = reader.data.get(start..end).ok_or(Error::UnexpectedEof)?;

    if out.len().saturating_add(length) > max_output {
        return Err(Error::OutputLimitExceeded);
    }
    out.extend_from_slice(bytes);
    reader.seek_bytes(end);
    Ok(())
}

/// Reads the code trees of a dynamic block (RFC 1951, section 3.2.7).
fn read_dynamic_trees(reader: &mut BitReader<'_>) -> Result<(Huffman, Huffman), Error> {
    let literal_count = reader.read(5)? as usize + 257;
    let distance_count = reader.read(5)? as usize + 1;
    let code_length_count = reader.read(4)? as usize + 4;

    if literal_count > 286 || distance_count > 30 {
        return Err(Error::InvalidCodeLengths);
    }

    // First the alphabet in which the main code lengths are themselves encoded.
    let mut code_lengths = [0u8; 19];
    for &slot in CODE_LENGTH_ORDER.iter().take(code_length_count) {
        code_lengths[slot] = reader.read(3)? as u8;
    }
    let (code_length_tree, remaining) = Huffman::build(&code_lengths)?;
    if remaining != 0 {
        // The auxiliary alphabet is required to be a complete code.
        return Err(Error::InvalidCodeLengths);
    }

    // Then the code lengths themselves, with run-length symbols 16/17/18.
    let total = literal_count + distance_count;
    let mut lengths = vec![0u8; total];
    let mut index = 0usize;
    while index < total {
        let symbol = code_length_tree.decode(reader)?;
        match symbol {
            0..=15 => {
                lengths[index] = symbol as u8;
                index += 1;
            }
            16 => {
                // Repeat the previous length 3 to 6 times.
                if index == 0 {
                    return Err(Error::InvalidCodeLengths);
                }
                let previous = lengths[index - 1];
                let repeat = 3 + reader.read(2)? as usize;
                if index + repeat > total {
                    return Err(Error::InvalidCodeLengths);
                }
                lengths[index..index + repeat].fill(previous);
                index += repeat;
            }
            17 => {
                // Repeat a zero length 3 to 10 times.
                let repeat = 3 + reader.read(3)? as usize;
                if index + repeat > total {
                    return Err(Error::InvalidCodeLengths);
                }
                index += repeat;
            }
            18 => {
                // Repeat a zero length 11 to 138 times.
                let repeat = 11 + reader.read(7)? as usize;
                if index + repeat > total {
                    return Err(Error::InvalidCodeLengths);
                }
                index += repeat;
            }
            _ => return Err(Error::InvalidCodeLengths),
        }
    }

    // Without an end-of-block code (256) the block could never terminate.
    if lengths[256] == 0 {
        return Err(Error::InvalidCodeLengths);
    }

    let (literal_tree, literal_remaining) = Huffman::build(&lengths[..literal_count])?;
    if literal_remaining != 0 {
        return Err(Error::InvalidCodeLengths);
    }

    let (distance_tree, distance_remaining) = Huffman::build(&lengths[literal_count..])?;
    // An incomplete distance tree is only legal in the degenerate case where at
    // most one distance is ever used. zlib accepts exactly this much.
    if distance_remaining != 0 && distance_tree.symbol_count() > 1 {
        return Err(Error::InvalidCodeLengths);
    }

    Ok((literal_tree, distance_tree))
}

/// Decodes a block body; shared by fixed and dynamic code trees.
fn inflate_block(
    reader: &mut BitReader<'_>,
    out: &mut Vec<u8>,
    literal_tree: &Huffman,
    distance_tree: &Huffman,
    max_output: usize,
) -> Result<(), Error> {
    loop {
        let symbol = literal_tree.decode(reader)?;

        if symbol < 256 {
            if out.len() >= max_output {
                return Err(Error::OutputLimitExceeded);
            }
            out.push(symbol as u8);
            continue;
        }
        if symbol == 256 {
            return Ok(());
        }

        // A match: length first, then distance.
        let length_index = usize::from(symbol) - 257;
        if length_index >= LENGTH_BASE.len() {
            return Err(Error::InvalidSymbol);
        }
        let length = usize::from(LENGTH_BASE[length_index])
            + reader.read(u32::from(LENGTH_EXTRA[length_index]))? as usize;

        let distance_symbol = usize::from(distance_tree.decode(reader)?);
        if distance_symbol >= DISTANCE_BASE.len() {
            return Err(Error::InvalidSymbol);
        }
        let distance = usize::from(DISTANCE_BASE[distance_symbol])
            + reader.read(u32::from(DISTANCE_EXTRA[distance_symbol]))? as usize;

        if distance > out.len() {
            return Err(Error::DistanceTooFar);
        }
        if out.len().saturating_add(length) > max_output {
            return Err(Error::OutputLimitExceeded);
        }

        // Copying one byte at a time is mandatory: source and destination may
        // overlap, and that is exactly how DEFLATE expresses runs (distance = 1).
        let mut source = out.len() - distance;
        out.reserve(length);
        for _ in 0..length {
            let byte = out[source];
            out.push(byte);
            source += 1;
        }
    }
}

/// Decompresses a zlib-wrapped stream (RFC 1950) and verifies its Adler-32.
pub fn inflate_zlib(data: &[u8], max_output: usize) -> Result<Vec<u8>, Error> {
    if data.len() < 6 {
        return Err(Error::UnexpectedEof);
    }

    let cmf = data[0];
    let flg = data[1];

    // Compression method must be 8 (deflate) and the window at most 32 KiB.
    if cmf & 0x0F != 8 || (cmf >> 4) > 7 {
        return Err(Error::InvalidZlibHeader);
    }
    // The two header bytes must form a multiple of 31.
    if (u16::from(cmf) * 256 + u16::from(flg)) % 31 != 0 {
        return Err(Error::InvalidZlibHeader);
    }
    // Preset dictionaries are unsupported; neither PNG nor OPC ever uses one.
    if flg & 0x20 != 0 {
        return Err(Error::InvalidZlibHeader);
    }

    let body = &data[2..data.len() - 4];
    let out = inflate_limited(body, max_output)?;

    let expected = u32::from_be_bytes([
        data[data.len() - 4],
        data[data.len() - 3],
        data[data.len() - 2],
        data[data.len() - 1],
    ]);
    let mut actual = Adler32::new();
    actual.update(&out);
    if actual.finish() != expected {
        return Err(Error::ChecksumMismatch);
    }

    Ok(out)
}
