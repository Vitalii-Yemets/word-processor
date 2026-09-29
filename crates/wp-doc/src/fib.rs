//! The File Information Block: the table of contents at the start of the
//! `WordDocument` stream.
//!
//! [MS-DOC] 2.5. Every other structure in the file is found through it: how
//! many characters the text has, which of the two table streams holds the
//! tables, and where in that stream each table begins and how long it is.
//! It grew with every version of Word — the block of offsets is longer in a
//! Word 2003 file than in a Word 97 one — so the count of pairs is read and
//! believed, and an offset past the count is one the file does not have.
//!
//! # Word 6 and Word 95
//!
//! Their block is the same table of contents laid out at fixed places, with
//! no counts: the lengths of the stories at `0x34`, and the offsets from
//! `0x58` on, eight bytes a pair, in the order Word 97 kept — with five short
//! numbers about the formatting pages in the middle of them, which Word 97
//! dropped. So the pairs are read into the same places Word 97's are, and
//! everything after this block asks for a table the same way whichever Word
//! wrote the file. What differs is where the tables are: a Word 6 file has
//! no table stream, and its tables are in the `WordDocument` stream itself.
//! And its text may not be in pieces at all: a file that was not saved
//! quickly has its text in one stretch, from `fcMin` to `fcMac`, and no
//! piece table says so.

use crate::Error;

/// What the first thirty-two bytes say: enough to know which Word wrote the
/// file and whether it is encrypted, which is all that can be read of an
/// encrypted file before it is deciphered — the rest of the block is
/// enciphered with the rest of the stream.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Base {
    /// The version, as the file names it: `0x00C1` is Word 97, `0x0065`
    /// Word 6 and `0x0068` Word 95.
    pub version: u16,
    /// Whether the block is Word 6's and Word 95's, laid out at fixed places.
    pub old: bool,
    pub encrypted: bool,
    /// Whether the encryption is only an exclusive-or. Word 6 and Word 95
    /// had nothing else, whatever this says.
    pub obfuscated: bool,
    /// Whether the tables are in `1Table` rather than `0Table`.
    pub table_stream_one: bool,
    /// The four bytes the weakest encryption keeps its key and its verifier
    /// in, or the length of the description of any other at the start of the
    /// table stream.
    pub key: u32,
}

impl Base {
    /// Reads the first thirty-two bytes of the `WordDocument` stream.
    pub fn parse(bytes: &[u8]) -> Result<Self, Error> {
        let u16_at = |at: usize| -> Result<u16, Error> {
            bytes
                .get(at..at + 2)
                .map(|two| u16::from_le_bytes([two[0], two[1]]))
                .ok_or(Error::Truncated("FIB"))
        };
        let identifier = u16_at(0)?;
        let version = u16_at(2)?;
        let old = match identifier {
            0xA5EC => version < 0x00C0,
            0xA5DC => true,
            _ => return Err(Error::NotWord),
        };
        // Word 2 and Word 1 wrote a block of their own, and not in a compound
        // file at all; a version this low in one is not something to guess at.
        if old && version < 101 {
            return Err(Error::Unsupported(format!("version {version} of the format")));
        }
        let flags = u16_at(10)?;
        let key = u32::from(u16_at(14)?) | (u32::from(u16_at(16)?) << 16);
        Ok(Self {
            version,
            old,
            encrypted: flags & 0x0100 != 0,
            obfuscated: old || flags & 0x8000 != 0,
            table_stream_one: !old && flags & 0x0200 != 0,
            key,
        })
    }

    /// How much of the start of the `WordDocument` stream is left readable
    /// when the rest is encrypted: the first part of the block.
    #[must_use]
    pub fn readable(&self) -> usize {
        if self.old {
            0x34
        } else {
            0x44
        }
    }
}

/// What the block says that this reader uses.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Fib {
    pub base: Base,
    /// Whether the text is in pieces: a file saved quickly has its text
    /// scattered, and the piece table says where.
    pub complex: bool,
    /// For a Word 6 file whose text is not in pieces: where it begins and
    /// ends in the stream.
    pub text_start: u32,
    pub text_end: u32,
    /// How many characters the main text has.
    pub text_length: u32,
    /// And the footnotes, headers, macros, comments, endnotes, text boxes and
    /// header text boxes after it, in that order, which is where the main
    /// text ends and the rest begins.
    pub footnotes_length: u32,
    pub headers_length: u32,
    pub macros_length: u32,
    pub comments_length: u32,
    pub endnotes_length: u32,
    pub text_boxes_length: u32,
    pub header_text_boxes_length: u32,
    /// For a Word 6 file: the first formatting page of each kind, and how
    /// many there are. A Word 6 file that was not saved quickly may list
    /// fewer pages than it has, the rest following the last one listed.
    pub first_character_page: u16,
    pub first_paragraph_page: u16,
    pub character_pages: u16,
    pub paragraph_pages: u16,
    /// The offsets into the table stream, and their lengths, by index.
    pairs: Vec<(u32, u32)>,
}

/// The pairs this reader asks for, by their index in the block.
#[derive(Clone, Copy, Debug)]
pub enum Table {
    StyleSheet = 1,
    FootnoteReferences = 2,
    FootnoteTexts = 3,
    CommentReferences = 4,
    CommentTexts = 5,
    Sections = 6,
    Headers = 11,
    CharacterBins = 12,
    ParagraphBins = 13,
    Fonts = 15,
    Fields = 16,
    BookmarkNames = 21,
    BookmarkStarts = 22,
    BookmarkEnds = 23,
    Dop = 31,
    Clx = 33,
    CommentAuthors = 36,
    CommentBookmarks = 37,
    MainShapes = 40,
    HeaderShapes = 41,
    CommentBookmarkStarts = 42,
    CommentBookmarkEnds = 43,
    EndnoteReferences = 46,
    EndnoteTexts = 47,
    Drawings = 50,
    RevisionAuthors = 51,
    TextBoxTexts = 56,
    HeaderTextBoxTexts = 58,
    Lists = 73,
    ListOverrides = 74,
    CommentsExtra = 112,
}

impl Fib {
    /// Reads the block from the start of the `WordDocument` stream — once it
    /// can be read, which for an encrypted file is once it is deciphered.
    pub fn parse(bytes: &[u8]) -> Result<Self, Error> {
        let base = Base::parse(bytes)?;
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
        let complex = u16_at(10)? & 0x0004 != 0;
        let text_start = u32_at(0x18)?;
        let text_end = u32_at(0x1C)?;
        if base.old {
            return Self::parse_old(base, complex, bytes);
        }

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
        let mut fib = Self {
            base,
            complex,
            text_start,
            text_end,
            text_length: long(3)?,
            footnotes_length: long(4)?,
            headers_length: long(5)?,
            macros_length: long(6)?,
            comments_length: long(7)?,
            endnotes_length: long(8)?,
            text_boxes_length: long(9)?,
            header_text_boxes_length: long(10)?,
            ..Self::default()
        };
        at = longs_at + cslw * 4;
        let pair_count = usize::from(u16_at(at)?);
        at += 2;
        for index in 0..pair_count {
            fib.pairs.push((u32_at(at + index * 8)?, u32_at(at + index * 8 + 4)?));
        }
        Ok(fib)
    }

    /// Word 6's and Word 95's block, at its fixed places.
    fn parse_old(base: Base, complex: bool, bytes: &[u8]) -> Result<Self, Error> {
        let u16_at = |at: usize| {
            bytes
                .get(at..at + 2)
                .map(|two| u16::from_le_bytes([two[0], two[1]]))
                .ok_or(Error::Truncated("FIB"))
        };
        let u32_at = |at: usize| {
            bytes
                .get(at..at + 4)
                .map(|four| u32::from_le_bytes([four[0], four[1], four[2], four[3]]))
                .ok_or(Error::Truncated("FIB"))
        };
        let text_start = u32_at(0x18)?;
        let mut fib = Self {
            base,
            complex,
            text_start,
            text_end: u32_at(0x1C)?,
            text_length: u32_at(0x34)?,
            footnotes_length: u32_at(0x38)?,
            headers_length: u32_at(0x3C)?,
            macros_length: u32_at(0x40)?,
            comments_length: u32_at(0x44)?,
            endnotes_length: u32_at(0x48)?,
            text_boxes_length: u32_at(0x4C)?,
            header_text_boxes_length: u32_at(0x50)?,
            first_character_page: u16_at(0x18A)?,
            first_paragraph_page: u16_at(0x18C)?,
            character_pages: u16_at(0x18E)?,
            paragraph_pages: u16_at(0x190)?,
            ..Self::default()
        };
        // The pairs up to the comments' bookmarks, then the five shorts,
        // then the rest — as far as the block goes, which is no further
        // than where the text begins.
        for index in 0..62 {
            let at = if index <= 37 { 0x58 + index * 8 } else { 0x192 + (index - 38) * 8 };
            if at + 8 > text_start as usize {
                break;
            }
            let (Ok(offset), Ok(length)) = (u32_at(at), u32_at(at + 4)) else { break };
            fib.pairs.push((offset, length));
        }
        Ok(fib)
    }

    /// Where a table is in the table stream, and how long, if the file has
    /// it and it is not empty.
    #[must_use]
    pub fn table(&self, which: Table) -> Option<(usize, usize)> {
        let (offset, length) = *self.pairs.get(which as usize)?;
        (length > 0).then_some((offset as usize, length as usize))
    }

    /// Where each story begins, as a character position: the main text, the
    /// footnotes, the headers, the macros, the comments, the endnotes, the
    /// text boxes and the header text boxes, one after another; and where the
    /// last of them ends.
    #[must_use]
    pub fn story_starts(&self) -> [u32; 9] {
        let lengths = [
            self.text_length,
            self.footnotes_length,
            self.headers_length,
            self.macros_length,
            self.comments_length,
            self.endnotes_length,
            self.text_boxes_length,
            self.header_text_boxes_length,
        ];
        let mut starts = [0u32; 9];
        for (index, length) in lengths.iter().enumerate() {
            starts[index + 1] = starts[index].saturating_add(*length);
        }
        starts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_word_six_block_is_read_at_its_fixed_places() {
        let mut bytes = vec![0u8; 0x300];
        bytes[0..2].copy_from_slice(&0xA5DCu16.to_le_bytes());
        bytes[2..4].copy_from_slice(&101u16.to_le_bytes());
        bytes[0x18..0x1C].copy_from_slice(&0x300u32.to_le_bytes());
        bytes[0x1C..0x20].copy_from_slice(&0x310u32.to_le_bytes());
        bytes[0x34..0x38].copy_from_slice(&15u32.to_le_bytes());
        bytes[0x38..0x3C].copy_from_slice(&3u32.to_le_bytes());
        // The stylesheet, pair one, and the comments' bookmark ends, pair
        // forty-three, past the five shorts.
        bytes[0x60..0x64].copy_from_slice(&0x400u32.to_le_bytes());
        bytes[0x64..0x68].copy_from_slice(&0x20u32.to_le_bytes());
        bytes[0x1BA..0x1BE].copy_from_slice(&0x500u32.to_le_bytes());
        bytes[0x1BE..0x1C2].copy_from_slice(&0x10u32.to_le_bytes());
        let fib = Fib::parse(&bytes).expect("a block");
        assert!(fib.base.old);
        assert_eq!((fib.text_start, fib.text_end), (0x300, 0x310));
        assert_eq!(fib.text_length, 15);
        assert_eq!(fib.story_starts()[2], 18);
        assert_eq!(fib.table(Table::StyleSheet), Some((0x400, 0x20)));
        assert_eq!(fib.table(Table::CommentBookmarkEnds), Some((0x500, 0x10)));
    }

    #[test]
    fn word_two_is_not_guessed_at() {
        let mut bytes = vec![0u8; 64];
        bytes[0..2].copy_from_slice(&0xA5DCu16.to_le_bytes());
        bytes[2..4].copy_from_slice(&45u16.to_le_bytes());
        assert!(matches!(Base::parse(&bytes), Err(Error::Unsupported(_))));
    }
}
