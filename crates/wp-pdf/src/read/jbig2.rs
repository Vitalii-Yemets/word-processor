//! JBIG2 pictures, as a PDF's `JBIG2Decode` filter carries them.
//!
//! [ITU-T T.88]. A JBIG2 page is built from segments, each a header saying
//! what kind it is and which segments before it it draws on, and its data:
//! the page's size, then regions drawn onto it — a bitmap coded outright
//! ([`generic`]), text drawn with the symbols of a symbol dictionary
//! ([`symbols`]), a halftone drawn with the patterns of a pattern
//! dictionary ([`halftone`]), or a refinement of what is already there.
//! What a PDF holds is the page's segments, one after another without the
//! file's header, and in a stream of its own the segments pages share —
//! the symbol dictionaries, mostly.

mod bitmap;
mod generic;
mod halftone;
mod huffman;
mod integers;
mod symbols;

use std::collections::HashMap;

use super::mq::Decoder;
use bitmap::{Bitmap, Combine};
use generic::{Generic, Refinement};
use huffman::{Bits, Table};

/// A decoded page: its rows packed a byte to eight pixels, a one bit black.
pub struct Picture {
    pub width: usize,
    pub height: usize,
    pub data: Vec<u8>,
}

/// Decodes an embedded JBIG2 stream, with the segments of the globals
/// stream before it.
#[must_use]
pub fn decode(data: &[u8], globals: Option<&[u8]>) -> Option<Picture> {
    let mut state = State::default();
    for segment in globals.map(segments).unwrap_or_default() {
        state.apply(&segment);
    }
    for segment in segments(data) {
        state.apply(&segment);
    }
    let page = state.page?;
    Some(Picture { width: page.width, height: page.height, data: page.packed() })
}

struct Segment<'a> {
    number: u32,
    kind: u8,
    referred: Vec<u32>,
    data: &'a [u8],
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

/// The segments of a stream, header and data one after another.
fn segments(data: &[u8]) -> Vec<Segment<'_>> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while at + 11 <= data.len() {
        let Some(segment) = segment_at(data, &mut at) else { break };
        let end_of_file = segment.kind == 51;
        out.push(segment);
        if end_of_file {
            break;
        }
    }
    out
}

fn segment_at<'a>(data: &'a [u8], at: &mut usize) -> Option<Segment<'a>> {
    let number = u32_at(data, *at)?;
    let flags = *data.get(*at + 4)?;
    let kind = flags & 0x3F;
    let long_page = flags & 0x40 != 0;
    *at += 5;
    let first = *data.get(*at)?;
    let count = if first >> 5 == 7 {
        let count = (u32_at(data, *at)? & 0x1FFF_FFFF) as usize;
        *at += 4 + (count + 1).div_ceil(8);
        count
    } else {
        *at += 1;
        usize::from(first >> 5)
    };
    let size = if number <= 256 {
        1
    } else if number <= 65536 {
        2
    } else {
        4
    };
    let mut referred = Vec::with_capacity(count.min(1024));
    for _ in 0..count {
        let bytes = data.get(*at..*at + size)?;
        referred.push(bytes.iter().fold(0u32, |value, &b| (value << 8) | u32::from(b)));
        *at += size;
    }
    *at += if long_page { 4 } else { 1 };
    let mut length = u32_at(data, *at)? as usize;
    *at += 4;
    if length == 0xFFFF_FFFF && kind == 38 {
        // An immediate generic region of unsaid length ends with a mark
        // and its count of rows.
        let mmr = data.get(*at + 17).is_some_and(|flags| flags & 1 != 0);
        let mark: [u8; 2] = if mmr { [0x00, 0x00] } else { [0xFF, 0xAC] };
        let from = *at + 18;
        let found = (from..data.len().saturating_sub(1)).find(|&i| data[i..i + 2] == mark)?;
        length = found + 2 + 4 - *at;
    }
    let end = at.checked_add(length)?.min(data.len());
    let segment = Segment { number, kind, referred, data: &data[*at..end] };
    *at = end;
    Some(segment)
}

/// What the segments so far have made.
#[derive(Default)]
struct State {
    page: Option<Bitmap>,
    /// The page's height is said by its stripes as they end.
    striped: bool,
    default_pixel: u8,
    dictionaries: HashMap<u32, symbols::Dictionary>,
    patterns: HashMap<u32, Vec<Bitmap>>,
    tables: HashMap<u32, Table>,
    /// Intermediate regions, kept for a refinement to take up.
    regions: HashMap<u32, (Bitmap, i64, i64, Combine)>,
}

/// A region segment's information: its size, place and how it goes onto
/// the page.
struct RegionInfo {
    width: usize,
    height: usize,
    x: i64,
    y: i64,
    combine: Combine,
}

fn region_info(data: &[u8]) -> Option<RegionInfo> {
    let width = u32_at(data, 0)? as usize;
    let height = u32_at(data, 4)? as usize;
    if width.checked_mul(height)? > 1 << 28 {
        return None;
    }
    Some(RegionInfo {
        width,
        height,
        x: i64::from(u32_at(data, 8)? as i32),
        y: i64::from(u32_at(data, 12)? as i32),
        combine: Combine::of(*data.get(16)? & 7),
    })
}

/// Moveable pixels, a signed byte each way.
fn pixels_at<const N: usize>(data: &[u8]) -> Option<[(i64, i64); N]> {
    let mut out = [(0, 0); N];
    for (index, slot) in out.iter_mut().enumerate() {
        let x = *data.get(index * 2)? as i8;
        let y = *data.get(index * 2 + 1)? as i8;
        *slot = (i64::from(x), i64::from(y));
    }
    Some(out)
}

impl State {
    fn apply(&mut self, segment: &Segment<'_>) {
        let _ = self.try_apply(segment);
    }

    fn try_apply(&mut self, segment: &Segment<'_>) -> Option<()> {
        let data = segment.data;
        match segment.kind {
            48 => {
                let width = u32_at(data, 0)? as usize;
                let height = u32_at(data, 4)?;
                let flags = *data.get(16)?;
                self.default_pixel = (flags >> 2) & 1;
                self.striped = height == 0xFFFF_FFFF;
                let height = if self.striped { 0 } else { height as usize };
                if width.checked_mul(height)? > 1 << 28 {
                    return None;
                }
                self.page = Some(Bitmap::filled(width, height, self.default_pixel));
            }
            50 => {
                let end = u32_at(data, 0)? as usize;
                let value = self.default_pixel;
                if let Some(page) = &mut self.page {
                    if self.striped && end < 1 << 20 {
                        page.grow_to(end + 1, value);
                    }
                }
            }
            0 => {
                let dictionary = self.symbol_dictionary(segment)?;
                self.dictionaries.insert(segment.number, dictionary);
            }
            16 => {
                self.patterns.insert(segment.number, halftone::decode_patterns(data)?);
            }
            53 => {
                self.tables.insert(segment.number, huffman::custom(data)?);
            }
            4 | 6 | 7 => {
                let info = region_info(data)?;
                let bitmap = self.text_region(segment, &info)?;
                self.place(segment, info, bitmap);
            }
            20 | 22 | 23 => {
                let info = region_info(data)?;
                let patterns = segment.referred.iter().find_map(|n| self.patterns.get(n))?;
                let bitmap =
                    halftone::decode_halftone(&data[17..], info.width, info.height, patterns)?;
                self.place(segment, info, bitmap);
            }
            36 | 38 | 39 => {
                let info = region_info(data)?;
                let bitmap = generic_region(&data[17..], &info)?;
                self.place(segment, info, bitmap);
            }
            40 | 42 | 43 => {
                let info = region_info(data)?;
                let flags = *data.get(17)?;
                let mut parameters = Refinement::with_template(flags & 1);
                parameters.typical = flags & 2 != 0;
                let mut at = 18;
                if parameters.template == 0 {
                    parameters.at = pixels_at::<2>(&data[at..])?;
                    at += 4;
                }
                // What it refines: the intermediate region it refers to,
                // or else the page where it stands; either way it goes
                // onto the page by its own operator.
                let reference = match segment.referred.iter().find_map(|n| self.regions.remove(n)) {
                    Some((bitmap, ..)) => bitmap,
                    None => self.page.as_ref()?.part(info.x, info.y, info.width, info.height),
                };
                let coded = &data[at..];
                let mut decoder = Decoder::new(coded, 0, coded.len());
                let mut contexts = generic::refinement_contexts(parameters.template);
                let bitmap = generic::decode_refinement(
                    &mut decoder,
                    &mut contexts,
                    info.width,
                    info.height,
                    &reference,
                    &parameters,
                );
                self.place(segment, info, bitmap);
            }
            _ => {}
        }
        Some(())
    }

    /// A region onto the page, or kept, if it is an intermediate one.
    fn place(&mut self, segment: &Segment<'_>, info: RegionInfo, bitmap: Bitmap) {
        if matches!(segment.kind, 4 | 20 | 36 | 40) {
            self.regions.insert(segment.number, (bitmap, info.x, info.y, info.combine));
            return;
        }
        let page = self.page.get_or_insert_with(|| {
            Bitmap::new(
                (info.x.max(0) as usize) + info.width,
                (info.y.max(0) as usize) + info.height,
            )
        });
        if self.striped {
            let bottom = (info.y.max(0) as usize) + info.height;
            page.grow_to(bottom, self.default_pixel);
        }
        page.compose(&bitmap, info.x, info.y, info.combine);
    }

    /// The symbols of the dictionaries a segment refers to, in order.
    fn symbols_referred(&self, segment: &Segment<'_>) -> Vec<Bitmap> {
        segment
            .referred
            .iter()
            .filter_map(|n| self.dictionaries.get(n))
            .flat_map(|dictionary| dictionary.symbols.iter().cloned())
            .collect()
    }

    /// The tables a segment refers to, in order, for those its flags say
    /// are its own.
    fn tables_referred(&self, segment: &Segment<'_>) -> std::vec::IntoIter<Table> {
        segment
            .referred
            .iter()
            .filter_map(|n| self.tables.get(n).cloned())
            .collect::<Vec<_>>()
            .into_iter()
    }

    fn symbol_dictionary(&self, segment: &Segment<'_>) -> Option<symbols::Dictionary> {
        let data = segment.data;
        let flags = u16::from_be_bytes([*data.first()?, *data.get(1)?]);
        let huffman = flags & 1 != 0;
        let aggregate = flags & 2 != 0;
        let template = ((flags >> 10) & 3) as u8;
        let refinement_template = ((flags >> 12) & 1) as u8;
        let mut at = 2;
        let mut generic = Generic::with_template(template);
        if !huffman {
            if template == 0 {
                generic.at = pixels_at::<4>(&data[at..])?;
                at += 8;
            } else {
                generic.at[0] = pixels_at::<1>(&data[at..])?[0];
                at += 2;
            }
        }
        let mut refinement = Refinement::with_template(refinement_template);
        if aggregate && refinement_template == 0 {
            refinement.at = pixels_at::<2>(&data[at..])?;
            at += 4;
        }
        let exported = u32_at(data, at)? as usize;
        let new = u32_at(data, at + 4)? as usize;
        at += 8;
        let mut custom = self.tables_referred(segment);
        let dh = match (flags >> 2) & 3 {
            0 => huffman::standard(4),
            1 => huffman::standard(5),
            _ if huffman => custom.next()?,
            _ => Table::default(),
        };
        let dw = match (flags >> 4) & 3 {
            0 => huffman::standard(2),
            1 => huffman::standard(3),
            _ if huffman => custom.next()?,
            _ => Table::default(),
        };
        let size =
            if (flags >> 6) & 1 == 0 || !huffman { huffman::standard(1) } else { custom.next()? };
        let instances =
            if (flags >> 7) & 1 == 0 || !huffman { huffman::standard(1) } else { custom.next()? };
        let input = self.symbols_referred(segment);
        // Carrying on with the contexts the last dictionary referred to left.
        let carried = if flags & 0x100 != 0 {
            segment
                .referred
                .iter()
                .rev()
                .find_map(|n| self.dictionaries.get(n))
                .map(|d| (d.generic.clone(), d.refinement.clone()))
        } else {
            None
        };
        let parameters = symbols::DictionaryParameters {
            huffman,
            aggregate,
            generic,
            refinement,
            exported,
            new,
            dh,
            dw,
            size,
            instances,
        };
        symbols::decode_dictionary(&parameters, &input, &data[at..], carried)
    }

    fn text_region(&self, segment: &Segment<'_>, info: &RegionInfo) -> Option<Bitmap> {
        let data = segment.data;
        let flags = u16::from_be_bytes([*data.get(17)?, *data.get(18)?]);
        let huffman = flags & 1 != 0;
        let refine = flags & 2 != 0;
        let log_strips = u32::from((flags >> 2) & 3);
        let corner = ((flags >> 4) & 3) as u8;
        let transposed = flags & 0x40 != 0;
        let combine = Combine::of(((flags >> 7) & 3) as u8);
        let default_pixel = ((flags >> 9) & 1) as u8;
        let offset = i64::from((flags >> 10) & 0x1F);
        let offset = if offset >= 16 { offset - 32 } else { offset };
        let refinement_template = ((flags >> 15) & 1) as u8;
        let mut at = 19;
        let huffman_flags = if huffman {
            let value = u16::from_be_bytes([*data.get(at)?, *data.get(at + 1)?]);
            at += 2;
            value
        } else {
            0
        };
        let mut refinement = Refinement::with_template(refinement_template);
        if refine && refinement_template == 0 {
            refinement.at = pixels_at::<2>(&data[at..])?;
            at += 4;
        }
        let instances = u32_at(data, at)? as usize;
        at += 4;
        let symbols = self.symbols_referred(segment);
        let code_length = integers::bits_for(symbols.len());
        let text = symbols::Text {
            refine,
            width: info.width,
            height: info.height,
            instances,
            log_strips,
            symbols: &symbols,
            code_length,
            default_pixel,
            combine,
            transposed,
            corner,
            offset,
            refinement,
        };
        let coded = &data[at..];
        let mut contexts = generic::refinement_contexts(refinement_template);
        if huffman {
            let mut custom = self.tables_referred(segment);
            let mut pick = |selection: u16, standard: &[usize]| -> Option<Table> {
                match standard.get(usize::from(selection)) {
                    Some(&number) if number > 0 => Some(huffman::standard(number)),
                    _ => custom.next(),
                }
            };
            let fs = pick(huffman_flags & 3, &[6, 7, 0, 0])?;
            let ds = pick((huffman_flags >> 2) & 3, &[8, 9, 10, 0])?;
            let dt = pick((huffman_flags >> 4) & 3, &[11, 12, 13, 0])?;
            let rdw = pick((huffman_flags >> 6) & 3, &[14, 15, 0, 0])?;
            let rdh = pick((huffman_flags >> 8) & 3, &[14, 15, 0, 0])?;
            let rdx = pick((huffman_flags >> 10) & 3, &[14, 15, 0, 0])?;
            let rdy = pick((huffman_flags >> 12) & 3, &[14, 15, 0, 0])?;
            let rsize = pick((huffman_flags >> 14) & 1, &[1, 0])?;
            let mut bits = Bits { data: coded, at: 0 };
            let ids = symbols::symbol_code_table(&mut bits, symbols.len())?;
            let tables =
                symbols::TextTables { fs, ds, dt, rdw, rdh, rdx, rdy, rsize, ids: Some(ids) };
            let mut coding = symbols::Coding::Huffman { bits: &mut bits, tables: &tables };
            symbols::decode_text(&text, &mut coding, coded, &mut contexts)
        } else {
            let mut decoder = Decoder::new(coded, 0, coded.len());
            let mut integers = symbols::TextIntegers::new(code_length);
            let mut coding =
                symbols::Coding::Arithmetic { decoder: &mut decoder, integers: &mut integers };
            symbols::decode_text(&text, &mut coding, coded, &mut contexts)
        }
    }
}

/// A generic region segment's bitmap, from its data after the region's
/// own information.
fn generic_region(data: &[u8], info: &RegionInfo) -> Option<Bitmap> {
    let flags = *data.first()?;
    let mmr = flags & 1 != 0;
    let template = (flags >> 1) & 3;
    let mut parameters = Generic::with_template(template);
    parameters.typical = flags & 8 != 0;
    let mut at = 1;
    if mmr {
        return Some(generic::decode_mmr(&data[at..], info.width, info.height)?.0);
    }
    if template == 0 {
        parameters.at = pixels_at::<4>(&data[at..])?;
        at += 8;
    } else {
        parameters.at[0] = pixels_at::<1>(&data[at..])?[0];
        at += 2;
    }
    let coded = &data[at..];
    let mut decoder = Decoder::new(coded, 0, coded.len());
    let mut contexts = generic::generic_contexts(template);
    Some(generic::decode_generic(&mut decoder, &mut contexts, info.width, info.height, &parameters))
}
