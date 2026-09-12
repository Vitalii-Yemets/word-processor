//! The CCITT fax codings, to ITU-T T.4 and T.6.
//!
//! # What they are for
//!
//! A scanned page: one bit to the pixel, and nearly all of it white. These
//! codings are built entirely around that. Nothing says where a pixel is; a row
//! is written as the lengths of its runs of white and black, one after another,
//! always beginning with white — so a blank line is four or five bits however
//! wide the page is.
//!
//! # The three of them
//!
//! **Modified Huffman** writes every row that way and nothing more. **Group 3**
//! may write a row instead as its differences from the row above, which pays
//! because the rows of a page are so nearly alike; which of the two a row uses
//! is said at the start of it. **Group 4** drops the choice and writes every row
//! against the one above, with an imaginary white row above the first.
//!
//! # Changing elements
//!
//! The two-dimensional forms are all about the places where the colour changes.
//! Reading one is a matter of looking along the row above for the next two such
//! places, and then being told whether this row changes at the same place, a
//! little to one side of it, or somewhere that has to be spelled out in run
//! lengths. So a row is decoded into the list of places it changes at, and the
//! row after it is decoded against that list.

use crate::Error;

/// One entry of a code table: the bits, how many of them, and what they mean.
struct Code {
    bits: u16,
    length: u8,
    run: u16,
}

/// Shorthand for a table entry.
const fn code(bits: u16, length: u8, run: u16) -> Code {
    Code { bits, length, run }
}

/// Runs of white: the sixty-four exact lengths, then the multiples of
/// sixty-four that are added to them.
static WHITE: &[Code] = &[
    code(0b0011_0101, 8, 0),
    code(0b00_0111, 6, 1),
    code(0b0111, 4, 2),
    code(0b1000, 4, 3),
    code(0b1011, 4, 4),
    code(0b1100, 4, 5),
    code(0b1110, 4, 6),
    code(0b1111, 4, 7),
    code(0b1_0011, 5, 8),
    code(0b1_0100, 5, 9),
    code(0b0_0111, 5, 10),
    code(0b0_1000, 5, 11),
    code(0b00_1000, 6, 12),
    code(0b00_0011, 6, 13),
    code(0b11_0100, 6, 14),
    code(0b11_0101, 6, 15),
    code(0b10_1010, 6, 16),
    code(0b10_1011, 6, 17),
    code(0b010_0111, 7, 18),
    code(0b000_1100, 7, 19),
    code(0b000_1000, 7, 20),
    code(0b001_0111, 7, 21),
    code(0b000_0011, 7, 22),
    code(0b000_0100, 7, 23),
    code(0b010_1000, 7, 24),
    code(0b010_1011, 7, 25),
    code(0b001_0011, 7, 26),
    code(0b010_0100, 7, 27),
    code(0b001_1000, 7, 28),
    code(0b0000_0010, 8, 29),
    code(0b0000_0011, 8, 30),
    code(0b0001_1010, 8, 31),
    code(0b0001_1011, 8, 32),
    code(0b0001_0010, 8, 33),
    code(0b0001_0011, 8, 34),
    code(0b0001_0100, 8, 35),
    code(0b0001_0101, 8, 36),
    code(0b0001_0110, 8, 37),
    code(0b0001_0111, 8, 38),
    code(0b0010_1000, 8, 39),
    code(0b0010_1001, 8, 40),
    code(0b0010_1010, 8, 41),
    code(0b0010_1011, 8, 42),
    code(0b0010_1100, 8, 43),
    code(0b0010_1101, 8, 44),
    code(0b0000_0100, 8, 45),
    code(0b0000_0101, 8, 46),
    code(0b0000_1010, 8, 47),
    code(0b0000_1011, 8, 48),
    code(0b0101_0010, 8, 49),
    code(0b0101_0011, 8, 50),
    code(0b0101_0100, 8, 51),
    code(0b0101_0101, 8, 52),
    code(0b0010_0100, 8, 53),
    code(0b0010_0101, 8, 54),
    code(0b0101_1000, 8, 55),
    code(0b0101_1001, 8, 56),
    code(0b0101_1010, 8, 57),
    code(0b0101_1011, 8, 58),
    code(0b0100_1010, 8, 59),
    code(0b0100_1011, 8, 60),
    code(0b0011_0010, 8, 61),
    code(0b0011_0011, 8, 62),
    code(0b0011_0100, 8, 63),
    // The make-up codes, which say how many whole sixty-fours.
    code(0b1_1011, 5, 64),
    code(0b1_0010, 5, 128),
    code(0b01_0111, 6, 192),
    code(0b011_0111, 7, 256),
    code(0b0011_0110, 8, 320),
    code(0b0011_0111, 8, 384),
    code(0b0110_0100, 8, 448),
    code(0b0110_0101, 8, 512),
    code(0b0110_1000, 8, 576),
    code(0b0110_0111, 8, 640),
    code(0b0_1100_1100, 9, 704),
    code(0b0_1100_1101, 9, 768),
    code(0b0_1101_0010, 9, 832),
    code(0b0_1101_0011, 9, 896),
    code(0b0_1101_0100, 9, 960),
    code(0b0_1101_0101, 9, 1024),
    code(0b0_1101_0110, 9, 1088),
    code(0b0_1101_0111, 9, 1152),
    code(0b0_1101_1000, 9, 1216),
    code(0b0_1101_1001, 9, 1280),
    code(0b0_1101_1010, 9, 1344),
    code(0b0_1101_1011, 9, 1408),
    code(0b0_1001_1000, 9, 1472),
    code(0b0_1001_1001, 9, 1536),
    code(0b0_1001_1010, 9, 1600),
    code(0b01_1000, 6, 1664),
    code(0b0_1001_1011, 9, 1728),
];

/// Runs of black. The same shape, and entirely different codes: black runs are
/// short and frequent, so the short codes are spent on them.
static BLACK: &[Code] = &[
    code(0b00_0011_0111, 10, 0),
    code(0b010, 3, 1),
    code(0b11, 2, 2),
    code(0b10, 2, 3),
    code(0b011, 3, 4),
    code(0b0011, 4, 5),
    code(0b0010, 4, 6),
    code(0b0_0011, 5, 7),
    code(0b00_0101, 6, 8),
    code(0b00_0100, 6, 9),
    code(0b000_0100, 7, 10),
    code(0b000_0101, 7, 11),
    code(0b000_0111, 7, 12),
    code(0b0000_0100, 8, 13),
    code(0b0000_0111, 8, 14),
    code(0b0_0001_1000, 9, 15),
    code(0b00_0001_0111, 10, 16),
    code(0b00_0001_1000, 10, 17),
    code(0b00_0000_1000, 10, 18),
    code(0b000_0110_0111, 11, 19),
    code(0b000_0110_1000, 11, 20),
    code(0b000_0110_1100, 11, 21),
    code(0b000_0011_0111, 11, 22),
    code(0b000_0010_1000, 11, 23),
    code(0b000_0001_0111, 11, 24),
    code(0b000_0001_1000, 11, 25),
    code(0b0000_1100_1010, 12, 26),
    code(0b0000_1100_1011, 12, 27),
    code(0b0000_1100_1100, 12, 28),
    code(0b0000_1100_1101, 12, 29),
    code(0b0000_0110_1000, 12, 30),
    code(0b0000_0110_1001, 12, 31),
    code(0b0000_0110_1010, 12, 32),
    code(0b0000_0110_1011, 12, 33),
    code(0b0000_1101_0010, 12, 34),
    code(0b0000_1101_0011, 12, 35),
    code(0b0000_1101_0100, 12, 36),
    code(0b0000_1101_0101, 12, 37),
    code(0b0000_1101_0110, 12, 38),
    code(0b0000_1101_0111, 12, 39),
    code(0b0000_0110_1100, 12, 40),
    code(0b0000_0110_1101, 12, 41),
    code(0b0000_1101_1010, 12, 42),
    code(0b0000_1101_1011, 12, 43),
    code(0b0000_0101_0100, 12, 44),
    code(0b0000_0101_0101, 12, 45),
    code(0b0000_0101_0110, 12, 46),
    code(0b0000_0101_0111, 12, 47),
    code(0b0000_0110_0100, 12, 48),
    code(0b0000_0110_0101, 12, 49),
    code(0b0000_0101_0010, 12, 50),
    code(0b0000_0101_0011, 12, 51),
    code(0b0000_0010_0100, 12, 52),
    code(0b0000_0011_0111, 12, 53),
    code(0b0000_0011_1000, 12, 54),
    code(0b0000_0010_0111, 12, 55),
    code(0b0000_0010_1000, 12, 56),
    code(0b0000_0101_1000, 12, 57),
    code(0b0000_0101_1001, 12, 58),
    code(0b0000_0010_1011, 12, 59),
    code(0b0000_0010_1100, 12, 60),
    code(0b0000_0101_1010, 12, 61),
    code(0b0000_0110_0110, 12, 62),
    code(0b0000_0110_0111, 12, 63),
    code(0b00_0000_1111, 10, 64),
    code(0b0000_1100_1000, 12, 128),
    code(0b0000_1100_1001, 12, 192),
    code(0b0000_0101_1011, 12, 256),
    code(0b0000_0011_0011, 12, 320),
    code(0b0000_0011_0100, 12, 384),
    code(0b0000_0011_0101, 12, 448),
    code(0b0_0000_0110_1100, 13, 512),
    code(0b0_0000_0110_1101, 13, 576),
    code(0b0_0000_0100_1010, 13, 640),
    code(0b0_0000_0100_1011, 13, 704),
    code(0b0_0000_0100_1100, 13, 768),
    code(0b0_0000_0100_1101, 13, 832),
    code(0b0_0000_0111_0010, 13, 896),
    code(0b0_0000_0111_0011, 13, 960),
    code(0b0_0000_0111_0100, 13, 1024),
    code(0b0_0000_0111_0101, 13, 1088),
    code(0b0_0000_0111_0110, 13, 1152),
    code(0b0_0000_0111_0111, 13, 1216),
    code(0b0_0000_0101_0010, 13, 1280),
    code(0b0_0000_0101_0011, 13, 1344),
    code(0b0_0000_0101_0100, 13, 1408),
    code(0b0_0000_0101_0101, 13, 1472),
    code(0b0_0000_0101_1010, 13, 1536),
    code(0b0_0000_0101_1011, 13, 1600),
    code(0b0_0000_0110_0100, 13, 1664),
    code(0b0_0000_0110_0101, 13, 1728),
];

/// The make-up codes past 1728, which are the same for either colour.
///
/// Added later, for the wider papers: a page 2560 pixels across can say so in
/// one code rather than several.
static LONGER: &[Code] = &[
    code(0b000_0000_1000, 11, 1792),
    code(0b000_0000_1100, 11, 1856),
    code(0b000_0000_1101, 11, 1920),
    code(0b0000_0001_0010, 12, 1984),
    code(0b0000_0001_0011, 12, 2048),
    code(0b0000_0001_0100, 12, 2112),
    code(0b0000_0001_0101, 12, 2176),
    code(0b0000_0001_0110, 12, 2240),
    code(0b0000_0001_0111, 12, 2304),
    code(0b0000_0001_1100, 12, 2368),
    code(0b0000_0001_1101, 12, 2432),
    code(0b0000_0001_1110, 12, 2496),
    code(0b0000_0001_1111, 12, 2560),
];

/// What a row of a two-dimensional coding says about the row above it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    /// The run above ends before this one does: step past it and carry on in
    /// the same colour.
    Pass,
    /// This row does not follow the one above here: two run lengths follow.
    Horizontal,
    /// The colour changes where it does above, give or take three pixels.
    Vertical(i32),
    /// The end of a row, or of the picture.
    End,
}

/// Which coding the strip is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Every row in run lengths, and each row beginning on a byte.
    Huffman,
    /// Rows in run lengths or against the row above, each saying which.
    Group3 { two_dimensional: bool, byte_aligned: bool },
    /// Every row against the row above.
    Group4,
}

/// Reads bits from the top of each byte down, which is how these codings are
/// written.
struct Bits<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Bits<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, at: 0 }
    }

    fn bit(&mut self) -> Option<u8> {
        let byte = self.data.get(self.at / 8).copied()?;
        let bit = (byte >> (7 - self.at % 8)) & 1;
        self.at += 1;
        Some(bit)
    }

    /// The next so many bits without taking them.
    fn peek(&self, length: u8) -> Option<u16> {
        let mut value = 0u16;
        for step in 0..usize::from(length) {
            let at = self.at + step;
            let byte = self.data.get(at / 8).copied()?;
            value = (value << 1) | u16::from((byte >> (7 - at % 8)) & 1);
        }
        Some(value)
    }

    fn skip(&mut self, length: u8) {
        self.at += usize::from(length);
    }

    fn done(&self) -> bool {
        self.at >= self.data.len() * 8
    }

    /// Moves on to the start of the next byte.
    fn align(&mut self) {
        self.at = self.at.div_ceil(8) * 8;
    }
}

/// One run length, which may be several codes: any number of make-ups and then
/// one of the sixty-four exact lengths.
fn run_of(bits: &mut Bits<'_>, white: bool) -> Option<usize> {
    let table = if white { WHITE } else { BLACK };
    let mut total = 0usize;
    loop {
        let mut found = None;
        for length in 2..=13u8 {
            let Some(value) = bits.peek(length) else { break };
            if let Some(entry) =
                table.iter().chain(LONGER).find(|code| code.length == length && code.bits == value)
            {
                found = Some(entry);
                break;
            }
        }
        let entry = found?;
        bits.skip(entry.length);
        total += usize::from(entry.run);
        // A make-up code is followed by another code; one of the sixty-four
        // exact lengths ends the run.
        if entry.run < 64 {
            return Some(total);
        }
    }
}

/// The mode a two-dimensional row uses at this point.
fn mode_of(bits: &mut Bits<'_>) -> Option<Mode> {
    // Ordered by length, so the shortest that matches wins — which is what a
    // prefix code means.
    if bits.peek(1)? == 0b1 {
        bits.skip(1);
        return Some(Mode::Vertical(0));
    }
    match bits.peek(3)? {
        0b011 => {
            bits.skip(3);
            return Some(Mode::Vertical(1));
        }
        0b010 => {
            bits.skip(3);
            return Some(Mode::Vertical(-1));
        }
        0b001 => {
            bits.skip(3);
            return Some(Mode::Horizontal);
        }
        _ => {}
    }
    if bits.peek(4)? == 0b0001 {
        bits.skip(4);
        return Some(Mode::Pass);
    }
    match bits.peek(6)? {
        0b00_0011 => {
            bits.skip(6);
            return Some(Mode::Vertical(2));
        }
        0b00_0010 => {
            bits.skip(6);
            return Some(Mode::Vertical(-2));
        }
        _ => {}
    }
    match bits.peek(7)? {
        0b000_0011 => {
            bits.skip(7);
            return Some(Mode::Vertical(3));
        }
        0b000_0010 => {
            bits.skip(7);
            return Some(Mode::Vertical(-3));
        }
        _ => {}
    }
    // Twelve zeros and a one ends a row, and several of them in a row end the
    // picture. Anything else is something this does not read.
    if bits.peek(12)? == 0b0000_0000_0001 {
        bits.skip(12);
        return Some(Mode::End);
    }
    None
}

/// Steps over an end-of-row code and whatever padding came before it.
///
/// Returns whether one was there. The codings differ over whether rows are
/// separated by one at all, so this is asked rather than assumed.
fn past_end_of_row(bits: &mut Bits<'_>) -> bool {
    // The padding is zeros, and an end-of-row is eleven zeros and a one — so
    // any number of zeros followed by a one is one, however much padding.
    let mut zeros = 0usize;
    let start = bits.at;
    while let Some(value) = bits.peek(1) {
        if value == 1 {
            if zeros >= 11 {
                bits.skip(1);
                return true;
            }
            break;
        }
        bits.skip(1);
        zeros += 1;
        if zeros > 64 {
            break;
        }
    }
    bits.at = start;
    false
}

/// Decodes a strip of fax coding into rows of bits, one to the pixel.
///
/// A set bit is black, which is what the photometric of a scanned page says
/// zero means white.
pub fn decode(data: &[u8], width: usize, height: usize, kind: Kind) -> Result<Vec<u8>, Error> {
    if width == 0 {
        return Err(Error::Malformed("a picture with no width"));
    }
    let row_bytes = width.div_ceil(8);
    let mut out = vec![0u8; row_bytes * height];
    let mut bits = Bits::new(data);

    // Where the colour changes along the row above. The row above the first is
    // imaginary and all white, so it changes nowhere.
    let mut above: Vec<usize> = Vec::new();
    let mut here: Vec<usize> = Vec::new();

    for row in 0..height {
        if bits.done() {
            break;
        }

        // Whether this row is written against the one above it.
        let two_dimensional = match kind {
            Kind::Huffman => false,
            Kind::Group4 => true,
            Kind::Group3 { two_dimensional, byte_aligned } => {
                if byte_aligned {
                    bits.align();
                }
                let had_end = past_end_of_row(&mut bits);
                if two_dimensional {
                    // After the end of a row, one bit says which way the next
                    // one is written. Without an end-of-row there is nothing to
                    // say it, and the format then means one-dimensional.
                    if had_end {
                        bits.bit() == Some(0)
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
        };
        if kind == Kind::Huffman {
            // Every row of this one begins on a byte of its own.
            bits.align();
        }

        here.clear();
        let ended = if two_dimensional {
            read_against(&mut bits, &above, &mut here, width)
        } else {
            read_runs(&mut bits, &mut here, width)
        };
        if !ended && here.is_empty() {
            break;
        }

        paint(&here, &mut out[row * row_bytes..(row + 1) * row_bytes], width);
        core::mem::swap(&mut above, &mut here);
    }

    Ok(out)
}

/// Reads a row written as its own run lengths.
fn read_runs(bits: &mut Bits<'_>, changes: &mut Vec<usize>, width: usize) -> bool {
    let mut at = 0usize;
    let mut white = true;
    while at < width {
        let Some(run) = run_of(bits, white) else { return false };
        at = (at + run).min(width);
        changes.push(at);
        white = !white;
    }
    true
}

/// Reads a row written against the row above it.
fn read_against(
    bits: &mut Bits<'_>,
    above: &[usize],
    changes: &mut Vec<usize>,
    width: usize,
) -> bool {
    // Where this row has got to, and what colour it is in. It starts just
    // before the first pixel, in white, which is what makes the first run a
    // white one however short.
    let mut a0: i64 = -1;
    let mut white = true;

    while a0 < width as i64 {
        // The next place the row above changes to the colour this row is not,
        // and the one after it.
        let b1 = next_change(above, a0, white, width);
        let b2 = next_change_after(above, b1, width);

        let Some(mode) = mode_of(bits) else { return false };
        match mode {
            Mode::End => return true,
            Mode::Pass => {
                // The run above ends before this one does, so this run carries
                // on past it and nothing is written down yet.
                a0 = b2 as i64;
            }
            Mode::Horizontal => {
                let from = if a0 < 0 { 0 } else { a0 as usize };
                let Some(first) = run_of(bits, white) else { return false };
                let Some(second) = run_of(bits, !white) else { return false };
                let one = (from + first).min(width);
                let two = (one + second).min(width);
                changes.push(one);
                changes.push(two);
                a0 = two as i64;
            }
            Mode::Vertical(offset) => {
                let a1 = (b1 as i64 + i64::from(offset)).clamp(0, width as i64) as usize;
                changes.push(a1);
                a0 = a1 as i64;
                white = !white;
            }
        }
    }
    true
}

/// The next place the row above changes, to the right of where this row has got
/// to and to the colour this row is not.
///
/// The changes are the places the colour turns over, so every other one is a
/// change to black. Which of the two is wanted depends on the colour this row
/// is in, and getting it the wrong way round shifts every run by one.
fn next_change(above: &[usize], a0: i64, white: bool, width: usize) -> usize {
    let mut index = 0usize;
    while index < above.len() && (above[index] as i64) <= a0 {
        index += 1;
    }
    // A change at an even place is to black, at an odd one back to white.
    if (index % 2 == 0) != white {
        index += 1;
    }
    above.get(index).copied().unwrap_or(width)
}

fn next_change_after(above: &[usize], b1: usize, width: usize) -> usize {
    above.iter().copied().find(|place| *place > b1).unwrap_or(width)
}

/// Sets the bits of one row from the places its colour changes.
fn paint(changes: &[usize], row: &mut [u8], width: usize) {
    let mut at = 0usize;
    let mut white = true;
    for change in changes {
        let to = (*change).min(width);
        if !white {
            for x in at..to {
                row[x / 8] |= 0x80 >> (x % 8);
            }
        }
        at = to;
        white = !white;
    }
    if !white {
        for x in at..width {
            row[x / 8] |= 0x80 >> (x % 8);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Packs a list of (bits, length) into bytes, top of each byte first.
    fn packed(codes: &[(u16, u8)]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut held = 0u8;
        let mut count = 0u8;
        for (bits, length) in codes {
            for step in (0..*length).rev() {
                held = (held << 1) | ((bits >> step) & 1) as u8;
                count += 1;
                if count == 8 {
                    out.push(held);
                    held = 0;
                    count = 0;
                }
            }
        }
        if count > 0 {
            out.push(held << (8 - count));
        }
        out
    }

    fn row_of(image: &[u8], row: usize, width: usize) -> Vec<u8> {
        let row_bytes = width.div_ceil(8);
        (0..width).map(|x| (image[row * row_bytes + x / 8] >> (7 - x % 8)) & 1).collect()
    }

    #[test]
    fn a_run_of_white_is_read_from_its_code() {
        let data = packed(&[(0b0111, 4)]); // Two white.
        let mut bits = Bits::new(&data);
        assert_eq!(run_of(&mut bits, true), Some(2));
    }

    #[test]
    fn a_run_longer_than_sixty_three_takes_two_codes() {
        // The make-up for sixty-four, then the exact length three.
        let data = packed(&[(0b1_1011, 5), (0b1000, 4)]);
        let mut bits = Bits::new(&data);
        assert_eq!(run_of(&mut bits, true), Some(67));
    }

    #[test]
    fn black_and_white_have_tables_of_their_own() {
        // The same bits mean different things in the two tables, and the
        // shortest codes of all are spent on the short runs of black that a
        // page of type is made of.
        let data = packed(&[(0b11, 2)]);
        let mut bits = Bits::new(&data);
        assert_eq!(run_of(&mut bits, false), Some(2), "two ones is two black");

        let data = packed(&[(0b10, 2)]);
        let mut bits = Bits::new(&data);
        assert_eq!(run_of(&mut bits, false), Some(3));

        // The same two bits are not a code at all in the white table: they are
        // the start of a longer one.
        let data = packed(&[(0b1011, 4)]);
        let mut bits = Bits::new(&data);
        assert_eq!(run_of(&mut bits, true), Some(4), "four white takes four bits");
    }

    #[test]
    fn a_row_of_run_lengths_is_read() {
        // Two white, three black, three white: eight pixels.
        let data = packed(&[(0b0111, 4), (0b10, 2), (0b1000, 4)]);
        let image = decode(&data, 8, 1, Kind::Huffman).expect("a row");
        assert_eq!(row_of(&image, 0, 8), vec![0, 0, 1, 1, 1, 0, 0, 0]);
    }

    #[test]
    fn a_row_that_is_all_white_is_one_code() {
        let data = packed(&[(0b1_0011, 5)]); // Eight white.
        let image = decode(&data, 8, 1, Kind::Huffman).expect("a row");
        assert_eq!(row_of(&image, 0, 8), vec![0; 8]);
    }

    #[test]
    fn every_row_of_a_group_four_picture_is_read_against_the_one_above() {
        // The first row: two white, three black, three white. The second: the
        // same, said as three codes that each say "where it changed above".
        let codes = vec![
            (0b001, 3),  // Horizontal,
            (0b0111, 4), // two white,
            (0b10, 2),   // three black,
            (0b1, 1),    // and the row ends where it ends above.
            (0b1, 1),    // The second row: the same,
            (0b1, 1),    // and the same,
            (0b1, 1),    // and the same.
        ];
        let data = packed(&codes);

        let image = decode(&data, 8, 2, Kind::Group4).expect("two rows");
        assert_eq!(row_of(&image, 0, 8), vec![0, 0, 1, 1, 1, 0, 0, 0]);
        assert_eq!(row_of(&image, 1, 8), vec![0, 0, 1, 1, 1, 0, 0, 0], "the second row");
    }

    #[test]
    fn a_row_can_say_the_colour_changes_a_little_to_one_side() {
        let codes = vec![
            (0b001, 3), // The first row, as run lengths.
            (0b0111, 4),
            (0b10, 2),
            (0b1, 1),
            (0b011, 3), // The second row: the first change one to the right,
            (0b1, 1),   // and then where it changed above,
            (0b1, 1),   // and again.
        ];
        let data = packed(&codes);

        let image = decode(&data, 8, 2, Kind::Group4).expect("two rows");
        assert_eq!(row_of(&image, 0, 8), vec![0, 0, 1, 1, 1, 0, 0, 0]);
        assert_eq!(row_of(&image, 1, 8), vec![0, 0, 0, 1, 1, 0, 0, 0], "one to the right");
    }

    #[test]
    fn a_group_three_row_says_which_way_it_is_written() {
        // The one thing no encoder to hand writes: Group 3 with the rows
        // allowed to be written against each other. After the end of a row,
        // one bit says which the next one is — a one for run lengths, a nought
        // for the row above.
        const END: (u16, u8) = (0b0000_0000_0001, 12);
        let codes = vec![
            END,
            (0b1, 1),    // The first row, in run lengths:
            (0b0111, 4), // two white,
            (0b10, 2),   // three black,
            (0b1000, 4), // three white.
            END,
            (0b0, 1), // The second row, against the first:
            (0b1, 1), // where it changed above,
            (0b1, 1), // and again,
            (0b1, 1), // and again.
        ];
        let data = packed(&codes);

        let kind = Kind::Group3 { two_dimensional: true, byte_aligned: false };
        let image = decode(&data, 8, 2, kind).expect("two rows");
        assert_eq!(row_of(&image, 0, 8), vec![0, 0, 1, 1, 1, 0, 0, 0]);
        assert_eq!(row_of(&image, 1, 8), vec![0, 0, 1, 1, 1, 0, 0, 0], "the second row");
    }

    #[test]
    fn an_end_of_row_is_found_through_whatever_padding_precedes_it() {
        // The end of a row is eleven noughts and a one, and a writer may put
        // any number of noughts before it to bring the next row on to a byte.
        let data = packed(&[(0b0, 1), (0b0, 1), (0b0000_0000_0001, 12)]);
        let mut bits = Bits::new(&data);
        assert!(past_end_of_row(&mut bits), "the end of the row was not found");
        assert_eq!(bits.at, 14, "and everything before it was stepped over");
    }

    #[test]
    fn what_is_not_an_end_of_row_is_left_where_it_is() {
        let data = packed(&[(0b0111, 4)]);
        let mut bits = Bits::new(&data);
        assert!(!past_end_of_row(&mut bits));
        assert_eq!(bits.at, 0, "nothing should have been taken");
    }

    #[test]
    fn the_modes_are_told_apart_by_their_first_bits() {
        let cases: &[(&[(u16, u8)], Mode)] = &[
            (&[(0b1, 1)], Mode::Vertical(0)),
            (&[(0b011, 3)], Mode::Vertical(1)),
            (&[(0b010, 3)], Mode::Vertical(-1)),
            (&[(0b001, 3)], Mode::Horizontal),
            (&[(0b0001, 4)], Mode::Pass),
            (&[(0b00_0011, 6)], Mode::Vertical(2)),
            (&[(0b00_0010, 6)], Mode::Vertical(-2)),
            (&[(0b000_0011, 7)], Mode::Vertical(3)),
            (&[(0b000_0010, 7)], Mode::Vertical(-3)),
            (&[(0b0000_0000_0001, 12)], Mode::End),
        ];
        for (codes, wanted) in cases {
            let data = packed(codes);
            let mut bits = Bits::new(&data);
            assert_eq!(mode_of(&mut bits), Some(*wanted), "{codes:?}");
        }
    }

    #[test]
    fn the_tables_hold_every_length_exactly_once() {
        // Two codes of the same length and bits would be two meanings for one
        // code, which is the one thing a prefix code cannot have.
        for table in [WHITE, BLACK] {
            for (index, entry) in table.iter().enumerate() {
                assert!(
                    !table[..index]
                        .iter()
                        .any(|other| other.length == entry.length && other.bits == entry.bits),
                    "a code means two things: run {}",
                    entry.run
                );
            }
        }
    }

    #[test]
    fn a_picture_with_no_width_is_refused() {
        assert!(decode(&[], 0, 1, Kind::Group4).is_err());
    }
}
