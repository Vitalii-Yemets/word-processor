//! The formatting: styles, fonts, lists, the pages of sprms, and what the
//! sprms say.
//!
//! Every property is a sprm, and a paragraph's or a run's formatting is its
//! style's sprms with its own on top. The styles, the fonts and the lists are
//! tables of their own; the paragraphs' and the runs' own sprms are in pages
//! of five hundred and twelve bytes, each covering a range of file positions,
//! found through a table of which page covers which range.
//!
//! A Word 6 file keeps all of these in slightly different shapes — names one
//! byte a character, pages whose entries are narrower, sprms of one byte —
//! and each is read here into the shape Word 97's has, the sprms through
//! [`crate::old`], so that nothing after this knows which Word wrote it.

use std::collections::HashMap;

use wp_docx::fonts::{FontClass, FontEntry};
use wp_docx::model::{
    Alignment, Border, LineRule, LineSpacing, NumberingReference, ParagraphProperties,
    RunProperties, TabAlignment, TabLeader, TabStop, Underline, VerticalAlignment,
};

use crate::fib::{Fib, Table};
use crate::plc::Plc;
use crate::sprm::{self, Sprm};

/// A style of the stylesheet.
#[derive(Clone, Debug, Default)]
pub(crate) struct Style {
    /// Which of Word's own styles it is, whatever it is called: the headings
    /// are one to nine in every language.
    pub sti: u16,
    pub name: String,
    pub base: u16,
    pub papx: Vec<u8>,
    pub chpx: Vec<u8>,
}

/// The stylesheet: every style with its name, its base, and its sprms.
pub(crate) fn styles_of(table: &[u8], fib: &Fib) -> Vec<Style> {
    let Some((offset, length)) = fib.table(Table::StyleSheet) else { return Vec::new() };
    let Some(stsh) = table.get(offset..offset + length) else { return Vec::new() };
    let u16_at = |at: usize| stsh.get(at..at + 2).map(|two| u16::from_le_bytes([two[0], two[1]]));
    let Some(cb_stshi) = u16_at(0) else { return Vec::new() };
    let Some(count) = u16_at(2) else { return Vec::new() };
    let base_size = u16_at(4).unwrap_or(if fib.base.old { 8 } else { 10 }) as usize;
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
        styles.push(parse_style(std, base_size, fib.base.old));
    }
    styles
}

fn parse_style(std: &[u8], base_size: usize, old: bool) -> Style {
    let u16_at =
        |at: usize| std.get(at..at + 2).map_or(0, |two| u16::from_le_bytes([two[0], two[1]]));
    let sti = u16_at(0) & 0x0FFF;
    let kind = (u16_at(2) & 0x000F) as u8;
    let base = u16_at(2) >> 4;
    let cupx = (u16_at(4) & 0x000F) as usize;
    let mut at = base_size;
    // The name: a count of characters, the characters, a terminator — two
    // bytes each in Word 97, one in Word 6.
    let name = if old {
        let cch = usize::from(std.get(at).copied().unwrap_or(0));
        let bytes = std.get(at + 1..at + 1 + cch).unwrap_or(&[]);
        at += 1 + cch + 1;
        bytes.iter().map(|byte| crate::text::old_character(*byte)).collect()
    } else {
        let cch = usize::from(u16_at(at));
        at += 2;
        let units: Vec<u16> = (0..cch).map(|index| u16_at(at + index * 2)).collect();
        at += cch * 2 + 2;
        String::from_utf16_lossy(&units)
    };
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
        let translate =
            |bytes: &[u8]| if old { crate::old::translate(bytes) } else { bytes.to_vec() };
        if kind == 1 && index == 0 {
            papx = translate(group.get(2..).unwrap_or(&[]));
        } else {
            chpx = translate(group);
        }
    }
    Style { sti, name, base, papx, chpx }
}

/// A font of the font table: its name, the character set its text is in,
/// and what the document's font table says of it.
#[derive(Clone, Debug, Default)]
pub(crate) struct Font {
    pub name: String,
    pub charset: u8,
    pub entry: FontEntry,
}

/// The fonts, by index.
pub(crate) fn fonts_of(table: &[u8], fib: &Fib) -> Vec<Font> {
    let Some((offset, length)) = fib.table(Table::Fonts) else { return Vec::new() };
    let Some(sttb) = table.get(offset..offset + length) else { return Vec::new() };
    let u16_at =
        |at: usize| sttb.get(at..at + 2).map_or(0, |two| u16::from_le_bytes([two[0], two[1]]));
    let mut fonts = Vec::new();
    // Word 97's begins with a count; Word 6's with its own length.
    let (count, end, mut at) = if fib.base.old {
        (usize::MAX, usize::from(u16_at(0)).min(sttb.len()), 2)
    } else if u16_at(0) == 0xFFFF {
        (usize::from(u16_at(2)), sttb.len(), 6)
    } else {
        (usize::from(u16_at(0)), sttb.len(), 4)
    };
    while fonts.len() < count && at < end {
        let Some(&cb) = sttb.get(at) else { break };
        let entry = sttb.get(at..at + 1 + usize::from(cb)).unwrap_or(&[]);
        at += 1 + usize::from(cb);
        fonts.push(parse_font(entry, fib.base.old));
    }
    fonts
}

/// One entry: its length, what kind of letter it has and how heavy, its
/// character set, where in the name another name begins, and — in Word 97 —
/// its PANOSE numbers and the ranges it covers; then the name.
fn parse_font(entry: &[u8], old: bool) -> Font {
    let bits = entry.get(1).copied().unwrap_or(0);
    let charset = entry.get(4).copied().unwrap_or(0);
    let (name, alt_name, panose) = if old {
        let names = entry.get(6..).unwrap_or(&[]);
        let mut parts = names.split(|byte| *byte == 0).map(|bytes| {
            bytes.iter().map(|byte| crate::text::old_character(*byte)).collect::<String>()
        });
        (parts.next().unwrap_or_default(), parts.next().unwrap_or_default(), None)
    } else {
        let units: Vec<u16> = entry
            .get(40..)
            .unwrap_or(&[])
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        let mut parts = units.split(|unit| *unit == 0).map(String::from_utf16_lossy);
        let panose = entry.get(6..16).filter(|bytes| bytes.iter().any(|byte| *byte != 0));
        let panose = panose.map(|bytes| bytes.iter().map(|byte| format!("{byte:02X}")).collect());
        (parts.next().unwrap_or_default(), parts.next().unwrap_or_default(), panose)
    };
    let class = match (bits >> 4) & 0x07 {
        1 => FontClass::Roman,
        2 => FontClass::Swiss,
        3 => FontClass::Modern,
        4 => FontClass::Script,
        5 => FontClass::Decorative,
        _ => FontClass::Auto,
    };
    let fixed_pitch = match bits & 0x03 {
        1 => Some(true),
        2 => Some(false),
        _ => None,
    };
    Font {
        name: name.clone(),
        charset,
        entry: FontEntry {
            name,
            alt_name: Some(alt_name).filter(|alt| !alt.is_empty()),
            panose,
            charset: Some(charset),
            class,
            fixed_pitch,
        },
    }
}

/// The lists: for each list-format override number, whether its levels are
/// bulleted.
#[derive(Debug, Default)]
pub(crate) struct Lists {
    /// By list id: whether each of the nine levels is a bullet.
    lists: Vec<(i32, [bool; 9])>,
    /// By override number, from one: the list id.
    overrides: Vec<i32>,
}

impl Lists {
    pub fn is_bullet(&self, ilfo: u16, level: u8) -> Option<bool> {
        let id = *self.overrides.get(usize::from(ilfo).checked_sub(1)?)?;
        let (_, levels) = self.lists.iter().find(|(held, _)| *held == id)?;
        levels.get(usize::from(level)).copied()
    }
}

pub(crate) fn lists_of(table: &[u8], fib: &Fib) -> Lists {
    let mut lists = Lists::default();
    if let Some((offset, _length)) = fib.table(Table::Lists) {
        // The levels follow the table of lists in the stream, past the
        // length the block gives for it, so the stream is read from the
        // table's start to wherever the levels end.
        if let Some(plf) = table.get(offset..) {
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
    if let Some((offset, length)) = fib.table(Table::ListOverrides) {
        if let Some(plf) = table.get(offset..offset + length) {
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
pub(crate) struct Bins {
    /// Each range of file positions, and its page.
    ranges: Vec<(u32, u32, u32)>,
}

impl Bins {
    /// The page covering a file position.
    pub fn page_for(&self, fc: u32) -> Option<u32> {
        self.ranges.iter().find(|(from, to, _)| fc >= *from && fc < *to).map(|(_, _, page)| *page)
    }
}

/// Which of the two kinds of formatting page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Paragraph,
    Character,
}

pub(crate) fn bins_of(word: &[u8], table: &[u8], fib: &Fib, kind: Kind) -> Bins {
    let which = match kind {
        Kind::Paragraph => Table::ParagraphBins,
        Kind::Character => Table::CharacterBins,
    };
    let mut bins = Bins::default();
    // Word 97 numbers its pages in four bytes, Word 6 in two.
    let size = if fib.base.old { 2 } else { 4 };
    if let Some(plc) = fib
        .table(which)
        .and_then(|(offset, length)| table.get(offset..offset + length))
        .map(|bytes| Plc::parse(bytes, size))
    {
        for index in 0..plc.len() {
            let (Some(from), Some(to), Some(entry)) =
                (plc.start(index), plc.end(index), plc.entry(index))
            else {
                continue;
            };
            let page = if size == 2 {
                u32::from(u16::from_le_bytes([entry[0], entry[1]]))
            } else {
                u32::from_le_bytes([entry[0], entry[1], entry[2], entry[3]])
            };
            bins.ranges.push((from, to, page));
        }
    }
    // A Word 6 file that was not saved quickly may list fewer pages than it
    // has: the rest follow the last one listed, each saying for itself
    // which positions it covers.
    if fib.base.old && !fib.complex {
        let wanted = match kind {
            Kind::Paragraph => fib.paragraph_pages,
            Kind::Character => fib.character_pages,
        };
        let first = match kind {
            Kind::Paragraph => fib.first_paragraph_page,
            Kind::Character => fib.first_character_page,
        };
        let mut next = bins.ranges.last().map_or(u32::from(first), |(_, _, page)| page + 1);
        while bins.ranges.len() < usize::from(wanted) {
            let Some(bytes) = word.get(next as usize * 512..next as usize * 512 + 512) else {
                break;
            };
            let count = usize::from(bytes[511]);
            let fc = |index: usize| {
                u32::from_le_bytes([
                    bytes[index * 4],
                    bytes[index * 4 + 1],
                    bytes[index * 4 + 2],
                    bytes[index * 4 + 3],
                ])
            };
            if count == 0 || (count + 1) * 4 > 511 {
                break;
            }
            bins.ranges.push((fc(0), fc(count), next));
            next += 1;
        }
    }
    bins
}

/// A formatting page: which file positions it covers, and the group of
/// sprms for each, in Word 97's words.
#[derive(Clone, Debug, Default)]
pub(crate) struct Page {
    pub fcs: Vec<u32>,
    pub entries: Vec<Option<(u16, Vec<u8>)>>,
}

impl Page {
    /// The entry covering a file position: its style, its sprms, and the
    /// range it covers.
    pub fn entry_for(&self, fc: u32) -> Option<(u32, u32, u16, &[u8])> {
        let index = self.fcs.iter().position(|edge| fc < *edge)?.checked_sub(1)?;
        let (istd, grpprl) = self.entries.get(index)?.as_ref()?;
        Some((self.fcs[index], self.fcs[index + 1], *istd, grpprl))
    }
}

/// The pages already read, so that each is read once.
#[derive(Debug, Default)]
pub(crate) struct Pages {
    read: HashMap<(u32, bool), Page>,
}

impl Pages {
    pub fn get(&mut self, word: &[u8], page: u32, kind: Kind, old: bool) -> &Page {
        self.read.entry((page, kind == Kind::Paragraph)).or_insert_with(|| {
            match kind {
                Kind::Paragraph => paragraph_page(word, page, old),
                Kind::Character => character_page(word, page, old),
            }
            .unwrap_or_default()
        })
    }
}

/// A page of paragraph formatting.
fn paragraph_page(word: &[u8], page: u32, old: bool) -> Option<Page> {
    let bytes = word.get(page as usize * 512..page as usize * 512 + 512)?;
    let count = usize::from(bytes[511]);
    let u32_at =
        |at: usize| u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    if (count + 1) * 4 > 511 {
        return None;
    }
    let fcs: Vec<u32> = (0..=count).map(|index| u32_at(index * 4)).collect();
    // Each entry's place on the page, and what the page says of its height:
    // thirteen bytes in Word 97, seven in Word 6.
    let stride = if old { 7 } else { 13 };
    let mut entries = Vec::with_capacity(count);
    for index in 0..count {
        let offset =
            usize::from(bytes.get((count + 1) * 4 + index * stride).copied().unwrap_or(0)) * 2;
        if offset == 0 {
            entries.push(None);
            continue;
        }
        // The length is a count of words. In Word 97 it counts one byte
        // fewer than it says, or, when it is nought, the next byte is the
        // count instead — because a paragraph can carry more than a byte's
        // worth of formatting.
        let (cb, start) = match bytes.get(offset).copied() {
            Some(cw) if old => (usize::from(cw) * 2, offset + 1),
            Some(0) => (usize::from(bytes.get(offset + 1).copied().unwrap_or(0)) * 2, offset + 2),
            Some(cb) => (usize::from(cb) * 2 - 1, offset + 1),
            None => (0, offset),
        };
        let Some(papx) = bytes.get(start..(start + cb).min(512)) else {
            entries.push(None);
            continue;
        };
        if papx.len() < 2 {
            entries.push(None);
            continue;
        }
        let istd = u16::from_le_bytes([papx[0], papx[1]]);
        let grpprl = if old { crate::old::translate(&papx[2..]) } else { papx[2..].to_vec() };
        entries.push(Some((istd, grpprl)));
    }
    Some(Page { fcs, entries })
}

/// A page of character formatting.
fn character_page(word: &[u8], page: u32, old: bool) -> Option<Page> {
    let bytes = word.get(page as usize * 512..page as usize * 512 + 512)?;
    let count = usize::from(bytes[511]);
    let u32_at =
        |at: usize| u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    if (count + 1) * 4 + count > 511 {
        return None;
    }
    let fcs: Vec<u32> = (0..=count).map(|index| u32_at(index * 4)).collect();
    let mut entries = Vec::with_capacity(count);
    for index in 0..count {
        let offset = usize::from(bytes[(count + 1) * 4 + index]) * 2;
        if offset == 0 {
            entries.push(Some((0, Vec::new())));
            continue;
        }
        let cb = usize::from(bytes.get(offset).copied().unwrap_or(0));
        let chpx = bytes.get(offset + 1..offset + 1 + cb).unwrap_or(&[]);
        entries.push(Some((0, if old { crate::old::translate(chpx) } else { chpx.to_vec() })));
    }
    Some(Page { fcs, entries })
}

// --- What the sprms say ------------------------------------------------------------

pub(crate) fn apply_paragraph(properties: &mut ParagraphProperties, sprms: &[Sprm<'_>]) {
    for sprm in sprms {
        match sprm.code {
            sprm::P_JC | sprm::P_JC_OLD => {
                properties.alignment = Some(match sprm.byte() {
                    1 => Alignment::Center,
                    2 => Alignment::End,
                    3..=5 => Alignment::Both,
                    _ => Alignment::Start,
                });
            }
            sprm::P_DXA_LEFT | sprm::P_DXA_LEFT_80 => {
                properties.indent_start = Some(i32::from(sprm.i16()));
            }
            sprm::P_DXA_RIGHT | sprm::P_DXA_RIGHT_80 => {
                properties.indent_end = Some(i32::from(sprm.i16()));
            }
            sprm::P_DXA_LEFT1 | sprm::P_DXA_LEFT1_80 => {
                properties.indent_first_line = Some(i32::from(sprm.i16()));
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
            sprm::P_NO_AUTO_HYPHEN => properties.no_hyphenation = Some(sprm.on()),
            sprm::P_BIDI => properties.right_to_left = Some(sprm.on()),
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
            sprm::P_BORDER_TOP_80 => properties.borders.top = border_80(sprm.operand),
            sprm::P_BORDER_LEFT_80 => properties.borders.start = border_80(sprm.operand),
            sprm::P_BORDER_BOTTOM_80 => properties.borders.bottom = border_80(sprm.operand),
            sprm::P_BORDER_RIGHT_80 => properties.borders.end = border_80(sprm.operand),
            sprm::P_BORDER_BETWEEN_80 => properties.borders.between = border_80(sprm.operand),
            sprm::P_BORDER_TOP => properties.borders.top = border(sprm.operand),
            sprm::P_BORDER_LEFT => properties.borders.start = border(sprm.operand),
            sprm::P_BORDER_BOTTOM => properties.borders.bottom = border(sprm.operand),
            sprm::P_BORDER_RIGHT => properties.borders.end = border(sprm.operand),
            sprm::P_BORDER_BETWEEN => properties.borders.between = border(sprm.operand),
            sprm::P_SHADING_80 => properties.shading = shading_80(sprm.u16()),
            sprm::P_SHADING => properties.shading = shading(sprm.operand),
            sprm::P_CHANGE_TABS => change_tabs(&mut properties.tab_stops, sprm.operand),
            _ => {}
        }
    }
}

/// Tab stops taken away, then tab stops put in: a count and the places of
/// each, and for those put in a byte each saying how the text lines up at it
/// and what fills the space before it.
fn change_tabs(stops: &mut Vec<TabStop>, operand: &[u8]) {
    let i16_at = |at: usize| {
        operand.get(at..at + 2).map(|two| i32::from(i16::from_le_bytes([two[0], two[1]])))
    };
    let removed = usize::from(operand.first().copied().unwrap_or(0));
    for index in 0..removed {
        if let Some(position) = i16_at(1 + index * 2) {
            stops.retain(|stop| stop.position != position);
        }
    }
    let at = 1 + removed * 2;
    let added = usize::from(operand.get(at).copied().unwrap_or(0));
    for index in 0..added {
        let (Some(position), Some(&descriptor)) =
            (i16_at(at + 1 + index * 2), operand.get(at + 1 + added * 2 + index))
        else {
            break;
        };
        let alignment = match descriptor & 0x07 {
            1 => TabAlignment::Center,
            2 => TabAlignment::End,
            3 => TabAlignment::Decimal,
            4 => TabAlignment::Bar,
            _ => TabAlignment::Start,
        };
        let leader = match (descriptor >> 3) & 0x07 {
            1 => TabLeader::Dot,
            2 => TabLeader::Hyphen,
            3 | 4 => TabLeader::Underscore,
            5 => TabLeader::MiddleDot,
            _ => TabLeader::None,
        };
        stops.retain(|stop| stop.position != position);
        stops.push(TabStop { position, alignment, leader });
    }
    stops.sort_by_key(|stop| stop.position);
}

pub(crate) fn apply_character(properties: &mut RunProperties, sprms: &[Sprm<'_>], fonts: &[Font]) {
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
            sprm::C_BIDI => toggle(&mut properties.right_to_left, sprm),
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
                properties.font = fonts.get(usize::from(sprm.u16())).map(|font| font.name.clone());
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
            sprm::C_SPACING => properties.spacing_twentieths = Some(i32::from(sprm.i16())),
            sprm::C_LANGUAGE | sprm::C_LANGUAGE_NEW | sprm::C_LID => {
                properties.language = language_tag(sprm.u16()).map(str::to_owned);
            }
            _ => {}
        }
    }
}

// --- Lines and colours --------------------------------------------------------------

/// The line a border's kind names, as the model names it.
fn line_style(kind: u8) -> Option<&'static str> {
    Some(match kind {
        1 | 5 => "single",
        2 => "thick",
        3 => "double",
        6 => "dotted",
        7 => "dashed",
        8 => "dotDash",
        9 => "dotDotDash",
        10 => "triple",
        11 => "thinThickSmallGap",
        12 => "thickThinSmallGap",
        13 => "thinThickThinSmallGap",
        14 => "thinThickMediumGap",
        15 => "thickThinMediumGap",
        16 => "thinThickThinMediumGap",
        17 => "thinThickLargeGap",
        18 => "thickThinLargeGap",
        19 => "thinThickThinLargeGap",
        20 => "wave",
        21 => "doubleWave",
        22 => "dashSmallGap",
        23 => "dashDotStroked",
        24 => "threeDEmboss",
        25 => "threeDEngrave",
        26 => "outset",
        27 => "inset",
        _ => return None,
    })
}

/// A border as Word 97 wrote it, in four bytes: its width in eighths of a
/// point, its kind, its colour from the sixteen, and its space with a
/// shadow and a frame. Nothing, or all four bytes full, is no line.
pub(crate) fn border_80(bytes: &[u8]) -> Option<Border> {
    let four = bytes.get(0..4)?;
    if four == [0xFF; 4] {
        return None;
    }
    let style = line_style(four[1])?;
    let colour = sprm::colour_by_index(four[2]).unwrap_or("auto");
    Some(
        Border::line(style, u32::from(four[0]).max(2), Some(colour))
            .with_effect(four[3] & 0x20 != 0, four[3] & 0x40 != 0),
    )
}

/// A border as Word 2000 writes it, in eight: a colour of its own, then the
/// width, the kind, and the space with a shadow and a frame.
pub(crate) fn border(bytes: &[u8]) -> Option<Border> {
    let eight = bytes.get(0..8)?;
    let style = line_style(eight[5])?;
    let colour = if eight[3] == 0xFF {
        "auto".to_owned()
    } else {
        format!("{:02X}{:02X}{:02X}", eight[0], eight[1], eight[2])
    };
    Some(
        Border::line(style, u32::from(eight[4]).max(2), Some(&colour))
            .with_effect(eight[6] & 0x20 != 0, eight[6] & 0x40 != 0),
    )
}

/// How much of the colour in front a shading pattern shows, in tenths of a
/// percent; `None` for a pattern of lines, which is read as the colour
/// behind.
fn pattern_share(pattern: u16) -> Option<u32> {
    const FINER: [u32; 28] = [
        25, 75, 125, 150, 175, 225, 275, 325, 350, 375, 425, 450, 475, 525, 550, 575, 625, 650,
        675, 725, 775, 825, 850, 875, 925, 950, 975, 970,
    ];
    Some(match pattern {
        0 => 0,
        1 => 1000,
        2 => 50,
        3 => 100,
        4 => 200,
        5 => 250,
        6 => 300,
        7 => 400,
        8 => 500,
        9 => 600,
        10 => 700,
        11 => 750,
        12 => 800,
        13 => 900,
        35..=62 => FINER[usize::from(pattern - 35)],
        _ => return None,
    })
}

/// The one colour the model keeps for a shading: the colour behind, with the
/// colour in front laid over it as far as the pattern says.
fn shading_colour(fore: Option<[u8; 3]>, back: Option<[u8; 3]>, pattern: u16) -> Option<String> {
    let share = pattern_share(pattern).unwrap_or(0);
    if share == 0 {
        let [red, green, blue] = back?;
        return Some(format!("{red:02X}{green:02X}{blue:02X}"));
    }
    let fore = fore.unwrap_or([0, 0, 0]);
    let back = back.unwrap_or([0xFF, 0xFF, 0xFF]);
    let mixed: String = (0..3)
        .map(|at| {
            let (under, over) = (i64::from(back[at]), i64::from(fore[at]));
            format!("{:02X}", (under * 1000 + (over - under) * i64::from(share) + 500) / 1000)
        })
        .collect();
    Some(mixed)
}

/// Word 97's two-byte shading: the colours in front and behind from the
/// sixteen, and the pattern.
pub(crate) fn shading_80(value: u16) -> Option<String> {
    if value == 0xFFFF {
        return None;
    }
    let of_index = |index: u16| {
        let hex = sprm::colour_by_index(index as u8)?;
        let channel = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).unwrap_or(0);
        Some([channel(0), channel(2), channel(4)])
    };
    shading_colour(of_index(value & 0x1F), of_index((value >> 5) & 0x1F), value >> 10)
}

/// Word 2000's ten bytes: the colours written out, and the pattern.
pub(crate) fn shading(bytes: &[u8]) -> Option<String> {
    let ten = bytes.get(0..10)?;
    let pattern = u16::from_le_bytes([ten[8], ten[9]]);
    if pattern == 0xFFFF {
        return None;
    }
    let colour = |four: &[u8]| (four[3] != 0xFF).then(|| [four[0], four[1], four[2]]);
    shading_colour(colour(&ten[0..4]), colour(&ten[4..8]), pattern)
}

/// Word's packed date: minutes, hours, day, month and years since 1900, in
/// six, five, five, four and nine bits.
pub(crate) fn dttm(value: u32) -> String {
    let minute = value & 0x3F;
    let hour = (value >> 6) & 0x1F;
    let day = (value >> 11) & 0x1F;
    let month = (value >> 16) & 0x0F;
    let year = 1900 + ((value >> 20) & 0x1FF);
    if day == 0 || !(1..=12).contains(&month) {
        return String::new();
    }
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:00Z")
}

/// The language tag for a Windows language number, for the common ones.
pub(crate) fn language_tag(number: u16) -> Option<&'static str> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pattern_lays_the_colour_in_front_over_the_one_behind() {
        // Black at a quarter over white; the colour behind alone; nothing.
        assert_eq!(shading_80(1 | (8 << 5) | (5 << 10)).as_deref(), Some("BFBFBF"));
        assert_eq!(shading_80(7 << 5).as_deref(), Some("FFFF00"));
        assert_eq!(shading_80(0), None);
        let mut ten = [0u8; 10];
        ten[3] = 0xFF;
        ten[4..8].copy_from_slice(&[0xD9, 0xE2, 0xF3, 0]);
        assert_eq!(shading(&ten).as_deref(), Some("D9E2F3"));
    }

    #[test]
    fn a_border_is_its_kind_width_and_colour() {
        let line = border_80(&[12, 3, 6, 0]).expect("a line");
        assert_eq!((line.style.as_str(), line.size), ("double", 12));
        assert_eq!(line.color.as_deref(), Some("FF0000"));
        assert!(border_80(&[0xFF; 4]).is_none());
        assert!(border_80(&[0; 4]).is_none());
        let line = border(&[0, 0, 0xFF, 0, 8, 1, 0, 0]).expect("a line");
        assert_eq!(line.color.as_deref(), Some("0000FF"));
    }

    #[test]
    fn tabs_are_taken_away_and_put_in() {
        let mut stops = vec![TabStop {
            position: 720,
            alignment: TabAlignment::Start,
            leader: TabLeader::None,
        }];
        // Take away 720; put in 1440 centred and 2880 right-aligned with dots.
        let operand = [1, 0xD0, 0x02, 2, 0xA0, 0x05, 0x40, 0x0B, 1, 2 | (1 << 3)];
        change_tabs(&mut stops, &operand);
        assert_eq!(stops.len(), 2);
        assert_eq!((stops[0].position, stops[0].alignment), (1440, TabAlignment::Center));
        assert_eq!((stops[1].alignment, stops[1].leader), (TabAlignment::End, TabLeader::Dot));
    }
}
