//! The File Information Block: the table of contents at the start of the
//! `WordDocument` stream.
//!
//! [MS-DOC] 2.5. Every other structure in the file is found through it: how
//! many characters the text has, which of the two table streams holds the
//! tables, and where in that stream each table begins and how long it is.
//! It grew with every version of Word — the block of offsets is longer in a
//! Word 2003 file than in a Word 97 one — so the count of pairs is read and
//! believed, and an offset past the count is one the file does not have.

use crate::Error;

/// What the block says that this reader uses.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Fib {
    /// The version, as the file names it: `0x00C1` is Word 97.
    pub version: u16,
    /// Whether the file is encrypted, which this reader does not open.
    pub encrypted: bool,
    /// Whether the tables are in `1Table` rather than `0Table`.
    pub table_stream_one: bool,
    /// Whether the text is in pieces: a file saved quickly has its text
    /// scattered, and the piece table says where.
    pub complex: bool,
    /// How many characters the main text has.
    pub text_length: u32,
    /// And the footnotes, headers, comments, endnotes, text boxes and header
    /// text boxes after it, in that order, which is where the main text ends
    /// and the rest begins.
    pub footnotes_length: u32,
    pub headers_length: u32,
    pub comments_length: u32,
    pub endnotes_length: u32,
    pub text_boxes_length: u32,
    pub header_text_boxes_length: u32,
    /// The offsets into the table stream, and their lengths, by index.
    pairs: Vec<(u32, u32)>,
}

/// The pairs this reader asks for, by their index in the block.
#[derive(Clone, Copy, Debug)]
pub enum Table {
    StyleSheet = 1,
    Sections = 6,
    CharacterBins = 12,
    ParagraphBins = 13,
    Fonts = 15,
    Fields = 16,
    Clx = 33,
    Drawings = 50,
    Lists = 73,
    ListOverrides = 74,
}

impl Fib {
    /// Reads the block from the start of the `WordDocument` stream.
    pub fn parse(bytes: &[u8]) -> Result<Self, Error> {
        let u16_at = |at: usize| -> Result<u16, Error> {
            bytes
                .get(at..at + 2)
                .map(|two| u16::from_le_bytes([two[0], two[1]]))
                .ok_or(Error::Truncated("FIB"))
        };
        let u32_at = |at: usize| -> Result<u32, Error> {
            bytes
                .get(at..at + 4)
                .map(|four| u32::from_le_bytes([four[0], four[1], four[2], four[3]]))
                .ok_or(Error::Truncated("FIB"))
        };
        if u16_at(0)? != 0xA5EC {
            return Err(Error::NotWord);
        }
        let version = u16_at(2)?;
        let flags = u16_at(10)?;
        let encrypted = flags & 0x0100 != 0;
        let table_stream_one = flags & 0x0200 != 0;
        let complex = flags & 0x0004 != 0;

        // After the base come the counted blocks: the shorts, the longs, and
        // the offset pairs, each with its count in front.
        let mut at = 32;
        let csw = usize::from(u16_at(at)?);
        at += 2 + csw * 2;
        let cslw = usize::from(u16_at(at)?);
        let longs_at = at + 2;
        let long = |index: usize| -> Result<u32, Error> {
            if index >= cslw {
                return Ok(0);
            }
            u32_at(longs_at + index * 4)
        };
        let text_length = long(3)?;
        let footnotes_length = long(4)?;
        let headers_length = long(5)?;
        let comments_length = long(7)?;
        let endnotes_length = long(8)?;
        let text_boxes_length = long(9)?;
        let header_text_boxes_length = long(10)?;
        at = longs_at + cslw * 4;
        let pair_count = usize::from(u16_at(at)?);
        at += 2;
        let mut pairs = Vec::with_capacity(pair_count);
        for index in 0..pair_count {
            pairs.push((u32_at(at + index * 8)?, u32_at(at + index * 8 + 4)?));
        }

        Ok(Self {
            version,
            encrypted,
            table_stream_one,
            complex,
            text_length,
            footnotes_length,
            headers_length,
            comments_length,
            endnotes_length,
            text_boxes_length,
            header_text_boxes_length,
            pairs,
        })
    }

    /// Where a table is in the table stream, and how long, if the file has
    /// it and it is not empty.
    #[must_use]
    pub fn table(&self, which: Table) -> Option<(usize, usize)> {
        let (offset, length) = *self.pairs.get(which as usize)?;
        (length > 0).then_some((offset as usize, length as usize))
    }
}
