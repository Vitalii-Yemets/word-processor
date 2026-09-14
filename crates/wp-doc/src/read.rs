//! Reading the binary document into the model.
//!
//! # How a binary document is put together
//!
//! The text is one long run of characters, a paragraph ending in a carriage
//! return and a table cell in a bell — but it is not in the file in order.
//! The piece table says where each stretch of it is and whether that
//! stretch is one byte a character or two. The formatting is not on the
//! text either: it is in pages of five hundred and twelve bytes, each
//! covering a range of file positions, one set of pages for paragraphs and
//! one for runs, found through a table of which page covers which range.
//! Every property is a sprm, and a paragraph's or a run's formatting is its
//! style's sprms with its own on top. Lists, fonts, styles and section
//! properties are tables of their own in the second stream; pictures are in
//! a third, behind a header and inside a drawing record.
//!
//! So the reading is: the text with the file position of every character;
//! the paragraphs cut at their marks, each looked up by the position of its
//! mark; the runs cut where the character pages cut them; and the tables,
//! lists, fields and pictures recognised from the marks and sprms as the
//! text goes by.

use wp_docx::model::{
    Alignment, Block, Body, BreakKind, LineRule, LineSpacing, NumberingReference, Paragraph,
    ParagraphProperties, Run, RunContent, RunProperties, Table, TableCell, TableRow, Underline,
    VerticalAlignment,
};
use wp_text::Encoding;

use crate::cfb::CompoundFile;
use crate::fib::{Fib, Table as FibTable};
use crate::sprm::{self, Sprm};
use crate::Error;

/// The character that stands where a picture goes in the text, until the
/// picture is put in.
pub const PICTURE_MARK: char = '\u{FFFC}';

/// What was read.
#[derive(Debug, Default)]
pub struct Reading {
    pub body: Body,
    pub pictures: Vec<PictureFound>,
    pub links: Vec<LinkFound>,
    /// The page: width, height, and the margins top, right, bottom, left,
    /// in twips, from the first section.
    pub page: Option<(i32, i32, [i32; 4])>,
}

#[derive(Debug)]
pub struct PictureFound {
    pub paragraph: usize,
    pub offset: usize,
    pub bytes: Vec<u8>,
    pub extension: &'static str,
    pub width_emu: i64,
    pub height_emu: i64,
}

#[derive(Debug)]
pub struct LinkFound {
    pub paragraph: usize,
    pub start: usize,
    pub end: usize,
    pub address: String,
}

/// The streams the document is in.
struct Streams {
    word: Vec<u8>,
    table: Vec<u8>,
    data: Vec<u8>,
    fib: Fib,
}

/// One stretch of the text: which characters, where they are, how wide.
#[derive(Clone, Copy, Debug)]
struct Piece {
    cp_start: u32,
    cp_end: u32,
    fc: u32,
    compressed: bool,
}

/// A style of the stylesheet.
#[derive(Clone, Debug, Default)]
struct Style {
    name: String,
    /// One for a paragraph style, two for a character style.
    kind: u8,
    base: u16,
    papx: Vec<u8>,
    chpx: Vec<u8>,
}

/// One character of the text, and where it is in the file.
#[derive(Clone, Copy, Debug)]
struct Char {
    character: char,
    fc: u32,
}

/// Reads a document's bytes into a body, with the pictures and links beside
/// it.
pub fn read(bytes: &[u8]) -> Result<Reading, Error> {
    let file = CompoundFile::open(bytes.to_vec())?;
    let word = file.stream("WordDocument").ok_or(Error::Malformed("no WordDocument stream"))?;
    let fib = Fib::parse(&word)?;
    if fib.encrypted {
        return Err(Error::Encrypted);
    }
    let table_name = if fib.table_stream_one { "1Table" } else { "0Table" };
    let table = file
        .stream(table_name)
        .or_else(|| file.stream("1Table"))
        .or_else(|| file.stream("0Table"))
        .ok_or(Error::Malformed("no table stream"))?;
    let data = file.stream("Data").unwrap_or_default();
    let streams = Streams { word, table, data, fib };

    let text = text_of(&streams)?;
    let styles = styles_of(&streams);
    let fonts = fonts_of(&streams);
    let lists = lists_of(&streams);
    let sections = section_boundaries(&streams);
    let paragraph_bins = bins_of(&streams, FibTable::ParagraphBins);
    let character_bins = bins_of(&streams, FibTable::CharacterBins);
    let page = page_of(&streams);

    let mut builder = Builder {
        streams: &streams,
        styles: &styles,
        fonts: &fonts,
        lists: &lists,
        paragraph_bins: &paragraph_bins,
        character_bins: &character_bins,
        body: Body::default(),
        pictures: Vec::new(),
        links: Vec::new(),
        paragraphs_done: 0,
        table_rows: Vec::new(),
        row_cells: Vec::new(),
        cell_blocks: Vec::new(),
        fields: Vec::new(),
    };
    builder.build(&text, &sections);
    let mut body = builder.body;
    if body.blocks.is_empty() {
        body.blocks.push(Block::Paragraph(Paragraph::default()));
    }
    Ok(Reading { body, pictures: builder.pictures, links: builder.links, page })
}

// --- The text -----------------------------------------------------------------

/// The main text, character by character with its place in the file.
fn text_of(streams: &Streams) -> Result<Vec<Char>, Error> {
    let pieces = pieces_of(streams)?;
    let end = streams.fib.text_length;
    let mut out = Vec::with_capacity(end as usize);
    for piece in pieces {
        if piece.cp_start >= end {
            break;
        }
        let last = piece.cp_end.min(end);
        for cp in piece.cp_start..last {
            let offset = cp - piece.cp_start;
            let (character, fc) = if piece.compressed {
                let fc = piece.fc + offset;
                let byte = *streams.word.get(fc as usize).ok_or(Error::Truncated("text"))?;
                (old_character(byte), fc)
            } else {
                let fc = piece.fc + offset * 2;
                let two = streams
                    .word
                    .get(fc as usize..fc as usize + 2)
                    .ok_or(Error::Truncated("text"))?;
                let unit = u16::from_le_bytes([two[0], two[1]]);
                (char::from_u32(u32::from(unit)).unwrap_or('\u{FFFD}'), fc)
            };
            out.push(Char { character, fc });
        }
    }
    Ok(out)
}

/// A byte of the old one-byte text: the Western code page, with the few
/// places the format keeps for itself.
fn old_character(byte: u8) -> char {
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

/// The piece table, out of the Clx.
fn pieces_of(streams: &Streams) -> Result<Vec<Piece>, Error> {
    let Some((offset, length)) = streams.fib.table(FibTable::Clx) else {
        return Err(Error::Malformed("no piece table"));
    };
    let clx = streams.table.get(offset..offset + length).ok_or(Error::Truncated("Clx"))?;
    // Property modifiers first, each `01 cb[2] grpprl`, then the piece
    // table, `02 lcb[4] plcpcd`.
    let mut at = 0;
    while at < clx.len() {
        match clx[at] {
            1 => {
                let cb = usize::from(u16::from_le_bytes([clx[at + 1], clx[at + 2]]));
                at += 3 + cb;
            }
            2 => {
                let lcb = u32::from_le_bytes([clx[at + 1], clx[at + 2], clx[at + 3], clx[at + 4]])
                    as usize;
                let plc = clx.get(at + 5..at + 5 + lcb).ok_or(Error::Truncated("piece table"))?;
                return Ok(parse_pieces(plc));
            }
            _ => return Err(Error::Malformed("Clx")),
        }
    }
    Err(Error::Malformed("no piece table in the Clx"))
}

fn parse_pieces(plc: &[u8]) -> Vec<Piece> {
    // n pieces: n+1 positions of four bytes, then n descriptors of eight.
    let n = (plc.len().saturating_sub(4)) / 12;
    let u32_at = |at: usize| u32::from_le_bytes([plc[at], plc[at + 1], plc[at + 2], plc[at + 3]]);
    let mut pieces = Vec::with_capacity(n);
    for index in 0..n {
        let cp_start = u32_at(index * 4);
        let cp_end = u32_at(index * 4 + 4);
        let descriptor = (n + 1) * 4 + index * 8;
        let fc = u32_at(descriptor + 2);
        let compressed = fc & 0x4000_0000 != 0;
        let fc = if compressed { (fc & !0x4000_0000) / 2 } else { fc };
        pieces.push(Piece { cp_start, cp_end, fc, compressed });
    }
    pieces
}

/// Where the sections end, as character positions.
fn section_boundaries(streams: &Streams) -> Vec<u32> {
    let Some((offset, length)) = streams.fib.table(FibTable::Sections) else { return Vec::new() };
    let Some(plc) = streams.table.get(offset..offset + length) else { return Vec::new() };
    let n = plc.len().saturating_sub(4) / 16;
    (0..=n)
        .filter_map(|index| plc.get(index * 4..index * 4 + 4))
        .map(|four| u32::from_le_bytes([four[0], four[1], four[2], four[3]]))
        .collect()
}

/// The first section's page: size and margins.
fn page_of(streams: &Streams) -> Option<(i32, i32, [i32; 4])> {
    let (offset, length) = streams.fib.table(FibTable::Sections)?;
    let plc = streams.table.get(offset..offset + length)?;
    let n = plc.len().saturating_sub(4) / 16;
    if n == 0 {
        return None;
    }
    let sed = plc.get((n + 1) * 4..(n + 1) * 4 + 12)?;
    let fc = u32::from_le_bytes([sed[2], sed[3], sed[4], sed[5]]);
    if fc == 0xFFFF_FFFF {
        return None;
    }
    let cb = usize::from(u16::from_le_bytes([
        *streams.word.get(fc as usize)?,
        *streams.word.get(fc as usize + 1)?,
    ]));
    let grpprl = streams.word.get(fc as usize + 2..fc as usize + 2 + cb)?;
    let (mut width, mut height) = (12240, 15840);
    let mut margins = [1440, 1440, 1440, 1440];
    let mut landscape = false;
    for sprm in sprm::parse(grpprl) {
        match sprm.code {
            sprm::S_PAGE_WIDTH => width = i32::from(sprm.u16()),
            sprm::S_PAGE_HEIGHT => height = i32::from(sprm.u16()),
            sprm::S_MARGIN_TOP => margins[0] = i32::from(sprm.i16()),
            sprm::S_MARGIN_RIGHT => margins[1] = i32::from(sprm.u16()),
            sprm::S_MARGIN_BOTTOM => margins[2] = i32::from(sprm.i16()),
            sprm::S_MARGIN_LEFT => margins[3] = i32::from(sprm.u16()),
            sprm::S_ORIENTATION => landscape = sprm.byte() == 2,
            _ => {}
        }
    }
    let _ = landscape;
    Some((width, height, margins))
}

// --- The tables in the table stream ---------------------------------------------

/// The stylesheet: every style with its name, its base, and its sprms.
fn styles_of(streams: &Streams) -> Vec<Style> {
    let Some((offset, length)) = streams.fib.table(FibTable::StyleSheet) else { return Vec::new() };
    let Some(stsh) = streams.table.get(offset..offset + length) else { return Vec::new() };
    let u16_at = |at: usize| stsh.get(at..at + 2).map(|two| u16::from_le_bytes([two[0], two[1]]));
    let Some(cb_stshi) = u16_at(0) else { return Vec::new() };
    let Some(count) = u16_at(2) else { return Vec::new() };
    let base_size = u16_at(4).unwrap_or(10) as usize;
    let mut at = 2 + cb_stshi as usize;
    let mut styles = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let Some(cb) = u16_at(at) else { break };
        at += 2;
        let cb = cb as usize;
        if cb == 0 {
            styles.push(Style::default());
            continue;
        }
        let Some(std) = stsh.get(at..at + cb) else { break };
        at += cb;
        styles.push(parse_style(std, base_size));
    }
    styles
}

fn parse_style(std: &[u8], base_size: usize) -> Style {
    let u16_at =
        |at: usize| std.get(at..at + 2).map_or(0, |two| u16::from_le_bytes([two[0], two[1]]));
    let kind = (u16_at(2) & 0x000F) as u8;
    let base = u16_at(2) >> 4;
    let cupx = (u16_at(4) & 0x000F) as usize;
    let mut at = base_size;
    // The name: a count of characters, the characters, a terminator.
    let cch = usize::from(u16_at(at));
    at += 2;
    let units: Vec<u16> = (0..cch).map(|index| u16_at(at + index * 2)).collect();
    let name = String::from_utf16_lossy(&units);
    at += cch * 2 + 2;
    // The property groups: for a paragraph style its paragraph sprms (behind
    // a style number) and then its character sprms; for a character style
    // just the character ones. Each is padded to an even length.
    let mut papx = Vec::new();
    let mut chpx = Vec::new();
    for index in 0..cupx {
        if at % 2 == 1 {
            at += 1;
        }
        let cb = usize::from(u16_at(at));
        at += 2;
        let Some(group) = std.get(at..at + cb) else { break };
        at += cb;
        if kind == 1 && index == 0 {
            papx = group.get(2..).unwrap_or(&[]).to_vec();
        } else {
            chpx = group.to_vec();
        }
    }
    Style { name, kind, base, papx, chpx }
}

/// The fonts, by index: the font table's names.
fn fonts_of(streams: &Streams) -> Vec<String> {
    let Some((offset, length)) = streams.fib.table(FibTable::Fonts) else { return Vec::new() };
    let Some(sttb) = streams.table.get(offset..offset + length) else { return Vec::new() };
    let u16_at =
        |at: usize| sttb.get(at..at + 2).map_or(0, |two| u16::from_le_bytes([two[0], two[1]]));
    let extended = u16_at(0) == 0xFFFF;
    let (count, mut at) = if extended { (u16_at(2), 6) } else { (u16_at(0), 4) };
    let mut fonts = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
        let Some(&cb) = sttb.get(at) else { break };
        let entry = sttb.get(at + 1..at + 1 + usize::from(cb)).unwrap_or(&[]);
        at += 1 + usize::from(cb);
        // The name is at the end of the entry, after thirty-nine bytes of
        // what the font is like, ended by a zero.
        let units: Vec<u16> = entry
            .get(39..)
            .unwrap_or(&[])
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .take_while(|unit| *unit != 0)
            .collect();
        fonts.push(String::from_utf16_lossy(&units));
    }
    fonts
}

/// The lists: for each list-format override number, whether its levels are
/// bulleted.
#[derive(Debug, Default)]
struct Lists {
    /// By list id: whether each of the nine levels is a bullet.
    lists: Vec<(i32, [bool; 9])>,
    /// By override number, from one: the list id.
    overrides: Vec<i32>,
}

impl Lists {
    fn is_bullet(&self, ilfo: u16, level: u8) -> Option<bool> {
        let id = *self.overrides.get(usize::from(ilfo).checked_sub(1)?)?;
        let (_, levels) = self.lists.iter().find(|(held, _)| *held == id)?;
        levels.get(usize::from(level)).copied()
    }
}

fn lists_of(streams: &Streams) -> Lists {
    let mut lists = Lists::default();
    if let Some((offset, _length)) = streams.fib.table(FibTable::Lists) {
        // The levels follow the table of lists in the stream, past the
        // length the block gives for it, so the stream is read from the
        // table's start to wherever the levels end.
        if let Some(plf) = streams.table.get(offset..) {
            let count =
                plf.get(0..2).map_or(0, |two| usize::from(u16::from_le_bytes([two[0], two[1]])));
            let mut at = 2;
            let mut simple = Vec::with_capacity(count);
            for _ in 0..count {
                let Some(lstf) = plf.get(at..at + 28) else { break };
                let id = i32::from_le_bytes([lstf[0], lstf[1], lstf[2], lstf[3]]);
                simple.push(lstf[26] & 1 != 0);
                lists.lists.push((id, [false; 9]));
                at += 28;
            }
            // Then the levels of every list, nine each or one for a simple
            // list, each a fixed part, two sprm groups, and the number text.
            for (index, is_simple) in simple.into_iter().enumerate() {
                let levels = if is_simple { 1 } else { 9 };
                for level in 0..levels {
                    let Some(lvlf) = plf.get(at..at + 28) else { break };
                    let format = lvlf[4];
                    let cb_chpx = usize::from(lvlf[24]);
                    let cb_papx = usize::from(lvlf[25]);
                    at += 28 + cb_papx + cb_chpx;
                    let cch = plf
                        .get(at..at + 2)
                        .map_or(0, |two| usize::from(u16::from_le_bytes([two[0], two[1]])));
                    at += 2 + cch * 2;
                    if let Some((_, bullets)) = lists.lists.get_mut(index) {
                        bullets[level] = format == 23;
                        if is_simple {
                            for later in bullets.iter_mut().skip(1) {
                                *later = format == 23;
                            }
                        }
                    }
                }
            }
        }
    }
    if let Some((offset, length)) = streams.fib.table(FibTable::ListOverrides) {
        if let Some(plf) = streams.table.get(offset..offset + length) {
            let count = plf.get(0..4).map_or(0, |four| {
                u32::from_le_bytes([four[0], four[1], four[2], four[3]]) as usize
            });
            for index in 0..count {
                let Some(lfo) = plf.get(4 + index * 16..4 + index * 16 + 4) else { break };
                lists.overrides.push(i32::from_le_bytes([lfo[0], lfo[1], lfo[2], lfo[3]]));
            }
        }
    }
    lists
}

/// A bin table: which formatting page covers which range of file positions.
#[derive(Debug, Default)]
struct Bins {
    /// Ranges of file positions, one more than the pages.
    fcs: Vec<u32>,
    /// The page number of each range.
    pages: Vec<u32>,
}

fn bins_of(streams: &Streams, which: FibTable) -> Bins {
    let Some((offset, length)) = streams.fib.table(which) else { return Bins::default() };
    let Some(plc) = streams.table.get(offset..offset + length) else { return Bins::default() };
    let n = plc.len().saturating_sub(4) / 8;
    let u32_at = |at: usize| {
        plc.get(at..at + 4)
            .map_or(0, |four| u32::from_le_bytes([four[0], four[1], four[2], four[3]]))
    };
    Bins {
        fcs: (0..=n).map(|index| u32_at(index * 4)).collect(),
        pages: (0..n).map(|index| u32_at((n + 1) * 4 + index * 4)).collect(),
    }
}

impl Bins {
    /// The page covering a file position.
    fn page_for(&self, fc: u32) -> Option<u32> {
        let index = self.fcs.iter().position(|edge| fc < *edge)?.checked_sub(1)?;
        self.pages.get(index).copied()
    }
}

/// A formatting page: which file positions it covers, and the group of
/// sprms for each.
struct Page<'a> {
    fcs: Vec<u32>,
    entries: Vec<Option<(u16, &'a [u8])>>,
}

impl Page<'_> {
    /// The entry covering a file position: its style, and its sprms.
    fn entry_for(&self, fc: u32) -> Option<(u16, &[u8])> {
        let index = self.fcs.iter().position(|edge| fc < *edge)?.checked_sub(1)?;
        self.entries.get(index).copied().flatten()
    }
}

/// A page of paragraph formatting.
fn paragraph_page(word: &[u8], page: u32) -> Option<Page<'_>> {
    let bytes = word.get(page as usize * 512..page as usize * 512 + 512)?;
    let count = usize::from(bytes[511]);
    let u32_at =
        |at: usize| u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    let fcs: Vec<u32> = (0..=count).map(|index| u32_at(index * 4)).collect();
    let mut entries = Vec::with_capacity(count);
    for index in 0..count {
        let offset = usize::from(bytes[(count + 1) * 4 + index * 13]) * 2;
        if offset == 0 {
            entries.push(None);
            continue;
        }
        // The length is a count of words, or, when that is zero, the next
        // byte is a count of words instead — because a paragraph can carry
        // more than a byte's worth of formatting.
        let (cb, start) = match bytes.get(offset).copied() {
            Some(0) => (usize::from(bytes.get(offset + 1).copied().unwrap_or(0)) * 2, offset + 2),
            Some(cb) => (usize::from(cb) * 2 - 1, offset + 1),
            None => (0, offset),
        };
        let Some(papx) = bytes.get(start..start + cb) else {
            entries.push(None);
            continue;
        };
        if papx.len() < 2 {
            entries.push(None);
            continue;
        }
        let istd = u16::from_le_bytes([papx[0], papx[1]]);
        entries.push(Some((istd, &papx[2..])));
    }
    Some(Page { fcs, entries })
}

/// A page of character formatting.
fn character_page(word: &[u8], page: u32) -> Option<Page<'_>> {
    let bytes = word.get(page as usize * 512..page as usize * 512 + 512)?;
    let count = usize::from(bytes[511]);
    let u32_at =
        |at: usize| u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    let fcs: Vec<u32> = (0..=count).map(|index| u32_at(index * 4)).collect();
    let mut entries = Vec::with_capacity(count);
    for index in 0..count {
        let offset = usize::from(bytes[(count + 1) * 4 + index]) * 2;
        if offset == 0 {
            entries.push(Some((0, &[][..])));
            continue;
        }
        let cb = usize::from(bytes.get(offset).copied().unwrap_or(0));
        entries.push(bytes.get(offset + 1..offset + 1 + cb).map(|chpx| (0, chpx)));
    }
    Some(Page { fcs, entries })
}

// --- Putting the document together ------------------------------------------------

/// A field being read: its code, and where its result began.
struct Field {
    code: String,
    in_code: bool,
    paragraph: usize,
    start: usize,
}

struct Builder<'a> {
    streams: &'a Streams,
    styles: &'a [Style],
    fonts: &'a [String],
    lists: &'a Lists,
    paragraph_bins: &'a Bins,
    character_bins: &'a Bins,
    body: Body,
    pictures: Vec<PictureFound>,
    links: Vec<LinkFound>,
    paragraphs_done: usize,
    table_rows: Vec<TableRow>,
    row_cells: Vec<TableCell>,
    cell_blocks: Vec<Block>,
    fields: Vec<Field>,
}

impl Builder<'_> {
    /// Walks the text, paragraph by paragraph.
    fn build(&mut self, text: &[Char], sections: &[u32]) {
        let mut start = 0;
        for (index, item) in text.iter().enumerate() {
            let cp = index as u32;
            let ends_paragraph = match item.character {
                '\r' | '\u{7}' => true,
                '\u{c}' => sections.contains(&(cp + 1)),
                _ => false,
            };
            if ends_paragraph {
                self.paragraph(&text[start..=index]);
                start = index + 1;
            }
        }
        if start < text.len() {
            self.paragraph(&text[start..]);
        }
        self.end_table();
    }

    /// One paragraph: its formatting from its mark's page, its runs from
    /// the character pages, and its place in a table if it is in one.
    fn paragraph(&mut self, chars: &[Char]) {
        let Some(mark) = chars.last() else { return };
        let word = &self.streams.word;

        // The paragraph's own sprms, and the style they sit on.
        let (istd, papx) = self
            .paragraph_bins
            .page_for(mark.fc)
            .and_then(|page| paragraph_page(word, page))
            .and_then(|page| page.entry_for(mark.fc).map(|(istd, grpprl)| (istd, grpprl.to_vec())))
            .unwrap_or((0, Vec::new()));
        let paragraph_sprms = sprm::parse(&papx);
        let mut properties = ParagraphProperties::default();
        let mut base_chars = RunProperties::default();
        self.apply_style(istd, &mut properties, &mut base_chars);
        properties.style = self.style_id(istd);
        let mut in_table = false;
        let mut row_end = false;
        let mut table_definition: Option<Vec<i32>> = None;
        let mut ilfo = 0u16;
        for sprm in &paragraph_sprms {
            match sprm.code {
                sprm::P_IN_TABLE => in_table = sprm.on(),
                sprm::P_TABLE_ROW_END => row_end = sprm.on(),
                sprm::T_DEFINITION => table_definition = Some(cell_edges(sprm.operand)),
                sprm::P_ILFO => ilfo = sprm.u16(),
                _ => {}
            }
        }
        apply_paragraph(&mut properties, &paragraph_sprms);
        if ilfo != 0 || properties.numbering.is_some() {
            let level = properties.numbering.map_or(0, |n| n.level);
            let bullet = self.lists.is_bullet(ilfo, level).unwrap_or(false);
            properties.numbering = if ilfo == 0 {
                None
            } else {
                Some(NumberingReference {
                    id: if bullet { wp_docx::BULLET_LIST } else { wp_docx::NUMBERED_LIST },
                    level,
                })
            };
        }

        // A row's end is not a paragraph: it is where the row's cells are
        // gathered up, with the widths the row's definition gives.
        if row_end {
            let cells = core::mem::take(&mut self.row_cells);
            if !cells.is_empty() {
                let mut cells = cells;
                if let Some(edges) = table_definition {
                    for (index, cell) in cells.iter_mut().enumerate() {
                        if let (Some(left), Some(right)) = (edges.get(index), edges.get(index + 1))
                        {
                            cell.width = Some(right - left).filter(|width| *width > 0);
                        }
                    }
                }
                self.table_rows.push(TableRow { cells, ..TableRow::default() });
            }
            return;
        }

        // The runs.
        let runs = self.runs(&chars[..chars.len() - 1], &base_chars, mark.character == '\u{7}');
        let paragraph = Paragraph { properties, runs };
        self.paragraphs_done += 1;

        if in_table {
            self.cell_blocks.push(Block::Paragraph(paragraph));
            if mark.character == '\u{7}' {
                let blocks = core::mem::take(&mut self.cell_blocks);
                self.row_cells.push(TableCell { blocks, ..TableCell::default() });
            }
        } else {
            self.end_table();
            self.body.blocks.push(Block::Paragraph(paragraph));
        }
    }

    fn end_table(&mut self) {
        if !self.cell_blocks.is_empty() {
            let blocks = core::mem::take(&mut self.cell_blocks);
            self.row_cells.push(TableCell { blocks, ..TableCell::default() });
        }
        if !self.row_cells.is_empty() {
            let cells = core::mem::take(&mut self.row_cells);
            self.table_rows.push(TableRow { cells, ..TableRow::default() });
        }
        if self.table_rows.is_empty() {
            return;
        }
        let rows = core::mem::take(&mut self.table_rows);
        let grid = rows
            .first()
            .map(|row| row.cells.iter().map(|cell| cell.width.unwrap_or(2880)).collect())
            .unwrap_or_default();
        self.body.blocks.push(Block::Table(Box::new(Table { rows, grid, ..Table::default() })));
    }

    /// The runs of a paragraph: the characters cut where their formatting
    /// changes, with the special ones — tabs, breaks, fields, pictures —
    /// made into what they are.
    fn runs(&mut self, chars: &[Char], base: &RunProperties, _in_cell: bool) -> Vec<Run> {
        let mut runs: Vec<Run> = Vec::new();
        let mut text = String::new();
        let mut current: Option<(u32, u32, RunProperties, bool, u32)> = None;
        let mut offset = 0usize;
        let paragraph = self.paragraphs_done;

        let flush = |runs: &mut Vec<Run>, text: &mut String, properties: &RunProperties| {
            if text.is_empty() {
                return;
            }
            runs.push(Run {
                properties: properties.clone(),
                content: vec![RunContent::Text(core::mem::take(text))],
                field: None,
                revision: None,
                format_change: None,
            });
        };

        for item in chars {
            // The formatting in force at this character: looked up afresh
            // when its file position leaves the range the last lookup
            // covered.
            let outside =
                current.as_ref().is_none_or(|(from, to, ..)| item.fc < *from || item.fc >= *to);
            if outside {
                let found = self
                    .character_bins
                    .page_for(item.fc)
                    .and_then(|page| character_page(&self.streams.word, page))
                    .and_then(|page| {
                        let index =
                            page.fcs.iter().position(|edge| item.fc < *edge)?.checked_sub(1)?;
                        let (_, chpx) = page.entries.get(index).copied().flatten()?;
                        Some((page.fcs[index], page.fcs[index + 1], chpx.to_vec()))
                    });
                let (from, to, chpx) = found.unwrap_or((item.fc, item.fc + 1, Vec::new()));
                let sprms = sprm::parse(&chpx);
                let mut properties = base.clone();
                // A character style under the run's own sprms.
                if let Some(style) = sprms.iter().find(|sprm| sprm.code == sprm::C_ISTD) {
                    let mut unused = ParagraphProperties::default();
                    self.apply_style(style.u16(), &mut unused, &mut properties);
                }
                let mut special = false;
                let mut picture = 0;
                for sprm in &sprms {
                    match sprm.code {
                        sprm::C_SPECIAL => special = sprm.on(),
                        sprm::C_PICTURE => picture = sprm.u32(),
                        _ => {}
                    }
                }
                apply_character(&mut properties, &sprms, self.fonts);
                if current.as_ref().is_none_or(|(_, _, held, ..)| *held != properties) {
                    if let Some((_, _, held, ..)) = &current {
                        flush(&mut runs, &mut text, held);
                    }
                }
                current = Some((from, to, properties, special, picture));
            }
            let Some((_, _, properties, special, picture)) = &current else { continue };
            let properties = properties.clone();
            let (special, picture) = (*special, *picture);

            // Fields: the code between the begin and the separator, the
            // result between the separator and the end.
            let in_field_code = self.fields.last().is_some_and(|field| field.in_code);
            match item.character {
                '\u{13}' => {
                    flush(&mut runs, &mut text, &properties);
                    self.fields.push(Field {
                        code: String::new(),
                        in_code: true,
                        paragraph,
                        start: offset,
                    });
                    continue;
                }
                '\u{14}' => {
                    if let Some(field) = self.fields.last_mut() {
                        field.in_code = false;
                        field.start = offset;
                    }
                    continue;
                }
                '\u{15}' => {
                    flush(&mut runs, &mut text, &properties);
                    if let Some(field) = self.fields.pop() {
                        let code = field.code.trim();
                        if let Some(rest) = code.strip_prefix("HYPERLINK") {
                            let address = rest
                                .split_whitespace()
                                .find(|piece| !piece.starts_with('\\'))
                                .map(|piece| piece.trim_matches('"').to_owned())
                                .unwrap_or_default();
                            if !address.is_empty()
                                && field.paragraph == paragraph
                                && offset > field.start
                            {
                                self.links.push(LinkFound {
                                    paragraph,
                                    start: field.start,
                                    end: offset,
                                    address,
                                });
                            }
                        }
                    }
                    continue;
                }
                _ if in_field_code => {
                    // The code is text; the picture mark of the field's own
                    // data is not part of it.
                    if let Some(field) = self.fields.last_mut() {
                        if (item.character as u32) >= 0x20 {
                            field.code.push(item.character);
                        }
                    }
                    continue;
                }
                _ => {}
            }

            match item.character {
                '\t' => {
                    flush(&mut runs, &mut text, &properties);
                    runs.push(Run {
                        properties,
                        content: vec![RunContent::Tab],
                        field: None,
                        revision: None,
                        format_change: None,
                    });
                    offset += 1;
                }
                '\u{b}' => {
                    flush(&mut runs, &mut text, &properties);
                    runs.push(Run {
                        properties,
                        content: vec![RunContent::Break(BreakKind::Line)],
                        field: None,
                        revision: None,
                        format_change: None,
                    });
                    offset += 1;
                }
                '\u{c}' => {
                    flush(&mut runs, &mut text, &properties);
                    runs.push(Run {
                        properties,
                        content: vec![RunContent::Break(BreakKind::Page)],
                        field: None,
                        revision: None,
                        format_change: None,
                    });
                    offset += 1;
                }
                '\u{1}' if special => {
                    flush(&mut runs, &mut text, &properties);
                    if let Some(found) = self.picture_at(picture, paragraph, offset) {
                        self.pictures.push(found);
                        runs.push(Run {
                            properties,
                            content: vec![RunContent::Text(PICTURE_MARK.to_string())],
                            field: None,
                            revision: None,
                            format_change: None,
                        });
                        offset += PICTURE_MARK.len_utf8();
                    }
                }
                '\u{1e}' => {
                    text.push('\u{2011}');
                    offset += '\u{2011}'.len_utf8();
                }
                '\u{1f}' => {
                    text.push('\u{00AD}');
                    offset += '\u{00AD}'.len_utf8();
                }
                // The other marks: footnote and comment references, drawn
                // objects, and whatever else the format keeps for itself.
                c if (c as u32) < 0x20 || c == '\u{7}' => {}
                c => {
                    text.push(c);
                    offset += c.len_utf8();
                }
            }
        }
        if let Some((_, _, properties, ..)) = &current {
            flush(&mut runs, &mut text, properties);
        }
        runs
    }

    /// A style's formatting, its base's under it, onto a paragraph's
    /// properties and the run properties its text starts from.
    fn apply_style(
        &self,
        istd: u16,
        paragraph: &mut ParagraphProperties,
        chars: &mut RunProperties,
    ) {
        // The chain from the base up, so that each style's sprms land on
        // its base's.
        let mut chain = Vec::new();
        let mut current = istd;
        let mut guard = 0;
        while let Some(style) = self.styles.get(usize::from(current)) {
            chain.push(style);
            if style.base == 0x0FFF || style.base == current || guard > 20 {
                break;
            }
            current = style.base;
            guard += 1;
        }
        for style in chain.iter().rev() {
            apply_paragraph(paragraph, &sprm::parse(&style.papx));
            apply_character(chars, &sprm::parse(&style.chpx), self.fonts);
        }
        // A style is not a paragraph's own list, table or table-row mark.
        paragraph.numbering = paragraph.numbering.take();
    }

    /// The identifier of the document style a style's name maps to.
    fn style_id(&self, istd: u16) -> Option<String> {
        let name = self.styles.get(usize::from(istd))?.name.to_lowercase();
        if let Some(level) = name.strip_prefix("heading ") {
            if let Ok(level) = level.trim().parse::<u8>() {
                if (1..=9).contains(&level) {
                    return Some(format!("Heading{level}"));
                }
            }
        }
        (name == "title").then(|| "Title".to_owned())
    }

    /// A picture, from its place in the data stream: the header that says
    /// how big it is drawn, and the drawing record that holds its bytes.
    fn picture_at(&self, fc: u32, paragraph: usize, offset: usize) -> Option<PictureFound> {
        let data = &self.streams.data;
        let at = fc as usize;
        let header = data.get(at..at + 68)?;
        let u16_at = |from: usize| u16::from_le_bytes([header[from], header[from + 1]]);
        let lcb = u32::from_le_bytes([header[0], header[1], header[2], header[3]]) as usize;
        let cb_header = usize::from(u16_at(4));
        let format = u16_at(6);
        let dxa_goal = i64::from(u16_at(28) as i16);
        let dya_goal = i64::from(u16_at(30) as i16);
        let mx = i64::from(u16_at(32)).max(1);
        let my = i64::from(u16_at(34)).max(1);
        let picture = data.get(at + cb_header..at + lcb.max(cb_header))?;
        let (bytes, extension) = match format {
            // A drawing container holding the picture's bytes, or naming
            // them in the drawing store.
            0x64 | 0x66 => blip_in(picture).or_else(|| {
                let pib = pib_in(picture)?;
                self.blip_from_store(pib)
            })?,
            // The oldest form: a metafile straight after the header.
            _ => (picture.to_vec(), "wmf"),
        };
        let width_emu = dxa_goal * mx / 1000 * 635;
        let height_emu = dya_goal * my / 1000 * 635;
        Some(PictureFound {
            paragraph,
            offset,
            bytes,
            extension,
            width_emu: width_emu.max(9525),
            height_emu: height_emu.max(9525),
        })
    }

    /// The picture a drawing refers to by number, from the drawing store in
    /// the table stream.
    fn blip_from_store(&self, pib: u32) -> Option<(Vec<u8>, &'static str)> {
        let (offset, length) = self.streams.fib.table(FibTable::Drawings)?;
        let store = self.streams.table.get(offset..offset + length)?;
        let mut found = Vec::new();
        collect_records(store, 0xF007, &mut found);
        let bse = found.get(pib.checked_sub(1)? as usize)?;
        // The store entry: thirty-six bytes about the picture, then the
        // picture's own record where it is kept here.
        blip_in(bse.get(36..)?)
    }
}

/// The address a HYPERLINK field names: what is quoted, or the first word
/// that is not a switch.
fn link_address(rest: &str) -> String {
    let rest = rest.trim();
    if let Some(after) = rest.strip_prefix('"') {
        if let Some((quoted, _)) = after.split_once('"') {
            return quoted.to_owned();
        }
    }
    rest.split_whitespace()
        .find(|piece| !piece.starts_with('\\'))
        .map(|piece| piece.trim_matches('"').to_owned())
        .unwrap_or_default()
}

/// The right edges of a table's cells, from its definition sprm.
fn cell_edges(operand: &[u8]) -> Vec<i32> {
    let Some(&count) = operand.first() else { return Vec::new() };
    (0..=usize::from(count))
        .filter_map(|index| operand.get(1 + index * 2..3 + index * 2))
        .map(|two| i32::from(i16::from_le_bytes([two[0], two[1]])))
        .collect()
}

// --- Drawings ----------------------------------------------------------------------

/// Walks the records of a drawing container, collecting those of a type.
fn collect_records<'a>(bytes: &'a [u8], wanted: u16, out: &mut Vec<&'a [u8]>) {
    let mut at = 0;
    while at + 8 <= bytes.len() {
        let version = u16::from_le_bytes([bytes[at], bytes[at + 1]]);
        let kind = u16::from_le_bytes([bytes[at + 2], bytes[at + 3]]);
        let length =
            u32::from_le_bytes([bytes[at + 4], bytes[at + 5], bytes[at + 6], bytes[at + 7]])
                as usize;
        let Some(body) = bytes.get(at + 8..at + 8 + length) else { break };
        if kind == wanted {
            out.push(body);
        }
        // A container holds records; anything else holds bytes.
        if version & 0x000F == 0x000F {
            collect_records(body, wanted, out);
        }
        at += 8 + length;
    }
}

/// The first picture record in a drawing, as bytes of a format this program
/// draws, with its extension.
fn blip_in(bytes: &[u8]) -> Option<(Vec<u8>, &'static str)> {
    let mut at = 0;
    while at + 8 <= bytes.len() {
        let version = u16::from_le_bytes([bytes[at], bytes[at + 1]]);
        let kind = u16::from_le_bytes([bytes[at + 2], bytes[at + 3]]);
        let length =
            u32::from_le_bytes([bytes[at + 4], bytes[at + 5], bytes[at + 6], bytes[at + 7]])
                as usize;
        let body = bytes.get(at + 8..at + 8 + length)?;
        let instance = version >> 4;
        match kind {
            0xF01A..=0xF01F | 0xF029 | 0xF02A => {
                if let Some(found) = decode_blip(kind, instance, body) {
                    return Some(found);
                }
            }
            // The store entry, whose picture record follows its header.
            0xF007 => {
                if let Some(found) = body.get(36..).and_then(blip_in) {
                    return Some(found);
                }
            }
            _ => {}
        }
        if version & 0x000F == 0x000F {
            if let Some(found) = blip_in(body) {
                return Some(found);
            }
        }
        at += 8 + length;
    }
    None
}

/// The bytes of one picture record.
///
/// Every kind begins with a sixteen-byte identifier, or two when the
/// instance says so. A bitmap then has one byte of tag and the file; a
/// metafile has a header saying how big it is and then the file, squeezed.
fn decode_blip(kind: u16, instance: u16, body: &[u8]) -> Option<(Vec<u8>, &'static str)> {
    let two_ids = matches!(instance, 0x6E1 | 0x46B | 0x6E3 | 0x7A9 | 0x6E5 | 0x3D5 | 0x217 | 0x543);
    let mut at = if two_ids { 32 } else { 16 };
    match kind {
        0xF01D | 0xF02A => {
            at += 1;
            Some((body.get(at..)?.to_vec(), "jpeg"))
        }
        0xF01E => {
            at += 1;
            Some((body.get(at..)?.to_vec(), "png"))
        }
        0xF029 => {
            at += 1;
            Some((body.get(at..)?.to_vec(), "tiff"))
        }
        0xF01F => {
            // A device-independent bitmap is a bitmap file without its
            // first fourteen bytes; put them back and it is one.
            at += 1;
            let dib = body.get(at..)?;
            let header_size =
                u32::from_le_bytes([*dib.first()?, *dib.get(1)?, *dib.get(2)?, *dib.get(3)?]);
            let bits = u16::from_le_bytes([*dib.get(14)?, *dib.get(15)?]);
            let colours = if bits <= 8 { 1u32 << bits } else { 0 };
            let offset = 14 + header_size + colours * 4;
            let mut bmp = Vec::with_capacity(dib.len() + 14);
            bmp.extend_from_slice(b"BM");
            bmp.extend_from_slice(&((dib.len() + 14) as u32).to_le_bytes());
            bmp.extend_from_slice(&[0, 0, 0, 0]);
            bmp.extend_from_slice(&offset.to_le_bytes());
            bmp.extend_from_slice(dib);
            Some((bmp, "bmp"))
        }
        0xF01A | 0xF01B => {
            let header = body.get(at..at + 34)?;
            let compression = header[32];
            let data = body.get(at + 34..)?;
            let bytes = if compression == 0 {
                wp_deflate::inflate_zlib(data, 64 * 1024 * 1024).ok()?
            } else {
                data.to_vec()
            };
            Some((bytes, if kind == 0xF01A { "emf" } else { "wmf" }))
        }
        _ => None,
    }
}

/// The picture number a shape's properties name.
fn pib_in(bytes: &[u8]) -> Option<u32> {
    let mut options = Vec::new();
    collect_records(bytes, 0xF00B, &mut options);
    for option in options {
        for property in option.chunks_exact(6) {
            let id = u16::from_le_bytes([property[0], property[1]]) & 0x3FFF;
            if id == 0x0104 {
                return Some(u32::from_le_bytes([
                    property[2],
                    property[3],
                    property[4],
                    property[5],
                ]));
            }
        }
    }
    None
}

// --- What the sprms say ------------------------------------------------------------

fn apply_paragraph(properties: &mut ParagraphProperties, sprms: &[Sprm<'_>]) {
    for sprm in sprms {
        match sprm.code {
            sprm::P_JC | sprm::P_JC_OLD => {
                properties.alignment = Some(match sprm.byte() {
                    1 => Alignment::Center,
                    2 => Alignment::End,
                    3 | 4 | 5 => Alignment::Both,
                    _ => Alignment::Start,
                });
            }
            sprm::P_DXA_LEFT | sprm::P_DXA_LEFT_NEW => {
                properties.indent_start = Some(i32::from(sprm.i16()))
            }
            sprm::P_DXA_RIGHT | sprm::P_DXA_RIGHT_NEW => {
                properties.indent_end = Some(i32::from(sprm.i16()))
            }
            sprm::P_DXA_LEFT1 | sprm::P_DXA_LEFT1_NEW => {
                properties.indent_first_line = Some(i32::from(sprm.i16()))
            }
            sprm::P_DYA_BEFORE => properties.space_before = Some(i32::from(sprm.u16())),
            sprm::P_DYA_AFTER => properties.space_after = Some(i32::from(sprm.u16())),
            sprm::P_DYA_LINE => {
                let value = i32::from(sprm.i16());
                let multiple = sprm
                    .operand
                    .get(2..4)
                    .is_some_and(|two| u16::from_le_bytes([two[0], two[1]]) != 0);
                properties.line_spacing = if multiple {
                    Some(LineSpacing { value, rule: LineRule::Auto })
                } else if value < 0 {
                    Some(LineSpacing { value: -value, rule: LineRule::Exact })
                } else if value > 0 {
                    Some(LineSpacing { value, rule: LineRule::AtLeast })
                } else {
                    None
                };
            }
            sprm::P_KEEP => properties.keep_lines = Some(sprm.on()),
            sprm::P_KEEP_FOLLOW => properties.keep_next = Some(sprm.on()),
            sprm::P_PAGE_BREAK_BEFORE => properties.page_break_before = Some(sprm.on()),
            sprm::P_WIDOW_CONTROL => properties.widow_control = Some(sprm.on()),
            sprm::P_CONTEXTUAL_SPACING => properties.contextual_spacing = Some(sprm.on()),
            sprm::P_OUTLINE_LEVEL => {
                let level = sprm.byte();
                properties.outline_level = (level < 9).then_some(level);
            }
            sprm::P_ILVL => {
                let level = sprm.byte().min(8);
                match &mut properties.numbering {
                    Some(reference) => reference.level = level,
                    none => *none = Some(NumberingReference { id: wp_docx::NUMBERED_LIST, level }),
                }
            }
            _ => {}
        }
    }
}

fn apply_character(properties: &mut RunProperties, sprms: &[Sprm<'_>], fonts: &[String]) {
    let toggle = |held: &mut Option<bool>, sprm: &Sprm<'_>| match sprm.byte() {
        0 => *held = Some(false),
        1 => *held = Some(true),
        129 => *held = Some(!held.unwrap_or(false)),
        _ => {}
    };
    for sprm in sprms {
        match sprm.code {
            sprm::C_BOLD => toggle(&mut properties.bold, sprm),
            sprm::C_ITALIC => toggle(&mut properties.italic, sprm),
            sprm::C_STRIKE => toggle(&mut properties.strike, sprm),
            sprm::C_DOUBLE_STRIKE => toggle(&mut properties.double_strike, sprm),
            sprm::C_SMALL_CAPS => toggle(&mut properties.small_caps, sprm),
            sprm::C_CAPS => toggle(&mut properties.caps, sprm),
            sprm::C_HIDDEN => toggle(&mut properties.hidden, sprm),
            sprm::C_UNDERLINE => {
                properties.underline = Some(match sprm.byte() {
                    0 => Underline::None,
                    3 => Underline::Double,
                    4 => Underline::Dotted,
                    6 => Underline::Thick,
                    7 => Underline::Dashed,
                    11 => Underline::Wave,
                    _ => Underline::Single,
                });
            }
            sprm::C_SIZE => properties.size_half_points = Some(u32::from(sprm.u16())),
            sprm::C_FONT => {
                properties.font = fonts.get(usize::from(sprm.u16())).cloned();
            }
            sprm::C_COLOUR_INDEX => {
                properties.color = sprm::colour_by_index(sprm.byte()).map(str::to_owned);
            }
            sprm::C_COLOUR => {
                let bytes = sprm.operand;
                if bytes.len() >= 4 && bytes[3] == 0 {
                    properties.color =
                        Some(format!("{:02X}{:02X}{:02X}", bytes[0], bytes[1], bytes[2]));
                } else {
                    properties.color = None;
                }
            }
            sprm::C_HIGHLIGHT => {
                properties.highlight = sprm::highlight_by_index(sprm.byte()).map(str::to_owned);
            }
            // A colour behind the characters, which is a highlight when it
            // is one of the sixteen a highlight can be.
            sprm::C_SHADING_OLD => {
                let back = ((sprm.u16() >> 5) & 0x1F) as u8;
                if back != 0 {
                    properties.highlight = sprm::highlight_by_index(back).map(str::to_owned);
                }
            }
            sprm::C_SHADING => {
                // The colour in front, the colour behind, the pattern: a
                // clear pattern shows the colour behind, a solid one the
                // colour in front.
                let operand = sprm.operand;
                if let (Some(fore), Some(back)) = (operand.get(0..4), operand.get(4..8)) {
                    let pattern =
                        operand.get(8..10).map_or(0, |two| u16::from_le_bytes([two[0], two[1]]));
                    let colour = if pattern == 1 && fore[3] == 0 { fore } else { back };
                    if colour[3] == 0 {
                        properties.highlight =
                            sprm::highlight_by_colour(colour[0], colour[1], colour[2])
                                .map(str::to_owned);
                    }
                }
            }
            sprm::C_SUPER_SUB => {
                properties.vertical_align = Some(match sprm.byte() {
                    1 => VerticalAlignment::Superscript,
                    2 => VerticalAlignment::Subscript,
                    _ => VerticalAlignment::Baseline,
                });
            }
            // Raised or lowered by so many half-points, which is how the
            // older files write what is not quite a superscript.
            sprm::C_POSITION => properties.position_half_points = Some(i32::from(sprm.i16())),
            sprm::C_LANGUAGE | sprm::C_LANGUAGE_OLD => {
                properties.language = language_tag(sprm.u16()).map(str::to_owned);
            }
            _ => {}
        }
    }
}

/// The language tag for a Windows language number, for the common ones.
fn language_tag(number: u16) -> Option<&'static str> {
    Some(match number {
        1033 => "en-US",
        2057 => "en-GB",
        1031 => "de-DE",
        1036 => "fr-FR",
        1034 | 3082 => "es-ES",
        1040 => "it-IT",
        1043 => "nl-NL",
        1046 => "pt-BR",
        2070 => "pt-PT",
        1049 => "ru-RU",
        1058 => "uk-UA",
        1045 => "pl-PL",
        1029 => "cs-CZ",
        1030 => "da-DK",
        1053 => "sv-SE",
        1044 => "nb-NO",
        1035 => "fi-FI",
        1038 => "hu-HU",
        1055 => "tr-TR",
        1032 => "el-GR",
        1037 => "he-IL",
        1025 => "ar-SA",
        1041 => "ja-JP",
        1042 => "ko-KR",
        2052 => "zh-CN",
        1028 => "zh-TW",
        _ => return None,
    })
}
