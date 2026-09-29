//! The text, character by character, and where each character is in the
//! file.
//!
//! The text is not in the file in order. The piece table says where each
//! stretch of it is and whether that stretch is one byte a character or two;
//! a Word 6 file that was not saved quickly has no piece table at all, and
//! its text is in one stretch of single bytes where the block says. Every
//! story is in it — the main text, then the notes, the headers, the comments
//! and the rest — one after another, which is how a story is found: by the
//! positions the block gives for each.

use wp_text::Encoding;

use crate::fib::{Fib, Table};
use crate::Error;

/// One character of the text, and where it is in the file.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Char {
    pub character: char,
    pub fc: u32,
    /// Whether it was one byte in the file, which in a Word 6 file is a byte
    /// in the code page of its font and may need reading again once the font
    /// is known.
    pub narrow: bool,
}

/// One stretch of the text: which characters, where they are, how wide.
#[derive(Clone, Copy, Debug)]
struct Piece {
    cp_start: u32,
    cp_end: u32,
    fc: u32,
    narrow: bool,
}

/// Every story's text, character by character with its place in the file.
pub(crate) fn text_of(word: &[u8], table: &[u8], fib: &Fib) -> Result<Vec<Char>, Error> {
    let pieces = if fib.base.old && !fib.complex {
        // One stretch, from where the block says the text begins to where it
        // says it ends.
        let length = fib.text_end.saturating_sub(fib.text_start);
        vec![Piece { cp_start: 0, cp_end: length, fc: fib.text_start, narrow: true }]
    } else {
        pieces_of(table, fib)?
    };
    let mut out = Vec::new();
    for piece in pieces {
        for cp in piece.cp_start..piece.cp_end {
            let offset = cp - piece.cp_start;
            let (character, fc) = if piece.narrow {
                let fc = piece.fc + offset;
                let byte = *word.get(fc as usize).ok_or(Error::Truncated("text"))?;
                (old_character(byte), fc)
            } else {
                let fc = piece.fc + offset * 2;
                let two = word.get(fc as usize..fc as usize + 2).ok_or(Error::Truncated("text"))?;
                let unit = u16::from_le_bytes([two[0], two[1]]);
                (char::from_u32(u32::from(unit)).unwrap_or('\u{FFFD}'), fc)
            };
            out.push(Char { character, fc, narrow: piece.narrow });
        }
    }
    Ok(out)
}

/// A byte of the old one-byte text: the Western code page, with the few
/// places the format keeps for itself.
pub(crate) fn old_character(byte: u8) -> char {
    match byte {
        0x82 => '\u{201A}',
        0x83 => '\u{0192}',
        0x84 => '\u{201E}',
        0x85 => '\u{2026}',
        0x86 => '\u{2020}',
        0x87 => '\u{2021}',
        0x88 => '\u{02C6}',
        0x89 => '\u{2030}',
        0x8A => '\u{0160}',
        0x8B => '\u{2039}',
        0x8C => '\u{0152}',
        0x91 => '\u{2018}',
        0x92 => '\u{2019}',
        0x93 => '\u{201C}',
        0x94 => '\u{201D}',
        0x95 => '\u{2022}',
        0x96 => '\u{2013}',
        0x97 => '\u{2014}',
        0x98 => '\u{02DC}',
        0x99 => '\u{2122}',
        0x9A => '\u{0161}',
        0x9B => '\u{203A}',
        0x9C => '\u{0153}',
        0x9F => '\u{0178}',
        other => Encoding::CodePage(1252).decode(&[other]).chars().next().unwrap_or('\u{FFFD}'),
    }
}

/// A byte in the code page a font's character set names, where that is not
/// the Western one.
pub(crate) fn in_character_set(byte: u8, charset: u8) -> Option<char> {
    let page = match charset {
        238 => 1250,
        204 => 1251,
        161 => 1253,
        162 => 1254,
        177 => 1255,
        178 => 1256,
        186 => 1257,
        163 => 1258,
        222 => 874,
        _ => return None,
    };
    Encoding::code_page(page)?.decode(&[byte]).chars().next()
}

/// The piece table, out of the Clx.
fn pieces_of(table: &[u8], fib: &Fib) -> Result<Vec<Piece>, Error> {
    let Some((offset, length)) = fib.table(Table::Clx) else {
        return Err(Error::Malformed("no piece table"));
    };
    let clx = table.get(offset..offset + length).ok_or(Error::Truncated("Clx"))?;
    // Property modifiers first, each `01 cb[2] grpprl`, then the piece
    // table, `02 lcb[4] plcpcd`.
    let mut at = 0;
    while at < clx.len() {
        match clx[at] {
            1 => {
                let cb = clx
                    .get(at + 1..at + 3)
                    .map(|two| usize::from(u16::from_le_bytes([two[0], two[1]])))
                    .ok_or(Error::Truncated("Clx"))?;
                at += 3 + cb;
            }
            2 => {
                let lcb = clx
                    .get(at + 1..at + 5)
                    .map(|four| u32::from_le_bytes([four[0], four[1], four[2], four[3]]) as usize)
                    .ok_or(Error::Truncated("Clx"))?;
                let plc = clx.get(at + 5..at + 5 + lcb).ok_or(Error::Truncated("piece table"))?;
                return Ok(parse_pieces(plc, fib.base.old));
            }
            _ => return Err(Error::Malformed("Clx")),
        }
    }
    Err(Error::Malformed("no piece table in the Clx"))
}

fn parse_pieces(plc: &[u8], old: bool) -> Vec<Piece> {
    // n pieces: n+1 positions of four bytes, then n descriptors of eight.
    let n = (plc.len().saturating_sub(4)) / 12;
    let u32_at = |at: usize| u32::from_le_bytes([plc[at], plc[at + 1], plc[at + 2], plc[at + 3]]);
    let mut pieces = Vec::with_capacity(n);
    for index in 0..n {
        let cp_start = u32_at(index * 4);
        let cp_end = u32_at(index * 4 + 4);
        let descriptor = (n + 1) * 4 + index * 8;
        let fc = u32_at(descriptor + 2);
        // Word 97 marks a stretch of single bytes by a bit in its position,
        // which then counts in half-bytes; Word 6's text is all single bytes.
        let (fc, narrow) = if old {
            (fc, true)
        } else if fc & 0x4000_0000 != 0 {
            ((fc & !0x4000_0000) / 2, true)
        } else {
            (fc, false)
        };
        pieces.push(Piece { cp_start, cp_end, fc, narrow });
    }
    pieces
}
