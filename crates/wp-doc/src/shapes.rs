//! Drawings: the pictures in the line, and the shapes, text boxes and
//! pictures that float.
//!
//! # Where a drawing is
//!
//! In three places at once. The text has a mark where it is anchored — one
//! for a picture in the line, which says where in the data stream its
//! header and bytes are, and another for a floating drawing. A table beside
//! each story says, for each floating drawing's mark, the drawing's number
//! and the rectangle it occupies and how the text runs round it. And the
//! drawing itself — its shape, its colours, its line, where it is measured
//! from, its name — is a record in the Office drawing format, among all the
//! others in one container in the table stream, found by that number; the
//! pictures they show are kept in a store at the head of that container, or
//! in the `WordDocument` stream where the store says.
//!
//! A text box's words are a story of their own, like a footnote's: the
//! drawing names its story by number, and the story is read as any other
//! and put inside the shape.

use std::collections::HashMap;

use wp_docx::anchor::{Anchor, Placement, Relative, Wrap, WrapSide, USUAL_DEPTH};
use wp_docx::colour::Colour;
use wp_docx::fills::Fill;
use wp_docx::model::Paragraph;
use wp_docx::shapes::Shape;

use crate::fib::{Fib, Table};
use crate::plc::Plc;

/// English metric units in a twip.
pub(crate) const EMU_PER_TWIP: i64 = 635;

/// One drawing's record: which shape, and what its properties say.
#[derive(Clone, Debug, Default)]
pub(crate) struct Drawn {
    /// The shape's number: a rectangle is one, a text box two hundred and two,
    /// a picture's frame seventy-five.
    pub kind: u16,
    /// Whether it is flipped across or down, a group, or a group's part.
    flags: u32,
    properties: HashMap<u16, u32>,
    complex: HashMap<u16, Vec<u8>>,
}

impl Drawn {
    fn number(&self, id: u16) -> Option<u32> {
        self.properties.get(&id).copied()
    }

    /// A property that is a run of characters: a name, a description.
    fn text(&self, id: u16) -> Option<String> {
        let bytes = self.complex.get(&id)?;
        let units: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .take_while(|unit| *unit != 0)
            .collect();
        Some(String::from_utf16_lossy(&units))
    }

    /// A property that is a colour, when it is one written out rather than
    /// one of a scheme's or the system's.
    fn colour(&self, id: u16) -> Option<String> {
        let value = self.number(id)?;
        let [red, green, blue, flags] = value.to_le_bytes();
        (flags & !0x02 == 0).then(|| format!("{red:02X}{green:02X}{blue:02X}"))
    }

    /// One of the switches a group of them holds: the switch, and beside it
    /// sixteen places on whether it is said at all. Unsaid is `None`.
    fn switch(&self, id: u16, bit: u32) -> Option<bool> {
        let value = self.number(id)?;
        (value & (1 << (bit + 16)) != 0).then_some(value & (1 << bit) != 0)
    }

    /// Whether it is a group, or a part of one — which this does not draw.
    pub fn grouped(&self) -> bool {
        self.flags & 0x03 != 0
    }

    /// The picture it frames, by its number in the store.
    pub fn picture(&self) -> Option<u32> {
        self.number(PICTURE).filter(|pib| *pib > 0)
    }

    /// The story of a text box: its number among the text boxes, from one.
    pub fn text_story(&self) -> Option<u32> {
        self.number(TEXT_ID).map(|id| id >> 16).filter(|story| *story > 0)
    }
}

/// The properties read here, by number.
const ROTATION: u16 = 0x0004;
const TEXT_ID: u16 = 0x0080;
const PICTURE: u16 = 0x0104;
const FILL_COLOUR: u16 = 0x0181;
const FILL_SWITCHES: u16 = 0x01BF;
const LINE_COLOUR: u16 = 0x01C0;
const LINE_WIDTH: u16 = 0x01CB;
const LINE_SWITCHES: u16 = 0x01FF;
const NAME: u16 = 0x0380;
const DESCRIPTION: u16 = 0x0381;
const WRAP_LEFT: u16 = 0x0384;
const WRAP_TOP: u16 = 0x0385;
const WRAP_RIGHT: u16 = 0x0386;
const WRAP_BOTTOM: u16 = 0x0387;
const ALIGN_ACROSS: u16 = 0x038F;
const FROM_ACROSS: u16 = 0x0390;
const ALIGN_DOWN: u16 = 0x0391;
const FROM_DOWN: u16 = 0x0392;
const GROUP_SWITCHES: u16 = 0x03BF;

/// Where a floating drawing sits: its number, the rectangle it takes up in
/// twips, and how the text goes round it.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Placed {
    pub spid: u32,
    pub rectangle: [i32; 4],
    flags: u16,
}

/// Every drawing in the file, and where the floating ones are placed.
#[derive(Debug, Default)]
pub(crate) struct Drawings {
    /// The store: each picture's entry, from the first.
    store: Vec<Vec<u8>>,
    shapes: HashMap<u32, Drawn>,
    main: Vec<(u32, Placed)>,
    header: Vec<(u32, Placed)>,
}

impl Drawings {
    pub fn parse(table: &[u8], fib: &Fib) -> Self {
        let mut drawings = Self::default();
        let placed = |which: Table| {
            let Some(bytes) =
                fib.table(which).and_then(|(offset, length)| table.get(offset..offset + length))
            else {
                return Vec::new();
            };
            let plc = Plc::parse(bytes, 26);
            (0..plc.len())
                .filter_map(|index| {
                    let entry = plc.entry(index)?;
                    let i32_at = |at: usize| {
                        i32::from_le_bytes([entry[at], entry[at + 1], entry[at + 2], entry[at + 3]])
                    };
                    let placed = Placed {
                        spid: u32::from_le_bytes([entry[0], entry[1], entry[2], entry[3]]),
                        rectangle: [i32_at(4), i32_at(8), i32_at(12), i32_at(16)],
                        flags: u16::from_le_bytes([entry[20], entry[21]]),
                    };
                    Some((plc.start(index)?, placed))
                })
                .collect()
        };
        drawings.main = placed(Table::MainShapes);
        drawings.header = placed(Table::HeaderShapes);

        let Some(content) = fib
            .table(Table::Drawings)
            .and_then(|(offset, length)| table.get(offset..offset + length))
        else {
            return drawings;
        };
        // The drawing group, with the store; then each drawing, a byte
        // saying whose — the main text's or the headers' — before it.
        let mut at = 0;
        let mut first = true;
        while at + 8 <= content.len() {
            if !first {
                at += 1;
            }
            let Some((kind, _, body, next)) = record(content, at) else { break };
            if first {
                let mut entries = Vec::new();
                collect_records(body, 0xF007, &mut entries);
                drawings.store = entries.into_iter().map(<[u8]>::to_vec).collect();
            } else if kind == 0xF002 {
                walk(body, 0, &mut drawings.shapes);
            }
            first = false;
            at = next;
        }
        drawings
    }

    /// The floating drawing anchored at a place in a story: the main text's,
    /// or the headers'.
    pub fn placed_at(&self, header: bool, cp: u32) -> Option<Placed> {
        let list = if header { &self.header } else { &self.main };
        list.iter().find(|(at, _)| *at == cp).map(|(_, placed)| *placed)
    }

    pub fn drawn(&self, spid: u32) -> Option<&Drawn> {
        self.shapes.get(&spid)
    }

    /// A picture from the store, by its number there: kept in the entry, or
    /// in the `WordDocument` stream where the entry says.
    pub fn stored_picture(&self, pib: u32, word: &[u8]) -> Option<(Vec<u8>, &'static str)> {
        let entry = self.store.get(pib.checked_sub(1)? as usize)?;
        // Thirty-six bytes about the picture, a name as long as it says, and
        // the picture's own record where it is kept here.
        let name_length = usize::from(*entry.get(33)?);
        if let Some(found) = entry.get(36 + name_length..).and_then(blip_in) {
            return Some(found);
        }
        let size = crate::plc::u32_at(entry, 20) as usize;
        let at = crate::plc::u32_at(entry, 28) as usize;
        blip_in(word.get(at..at.checked_add(size)?)?)
    }
}

/// One record's header, and its body: its type, its instance, the bytes, and
/// where the next begins.
fn record(bytes: &[u8], at: usize) -> Option<(u16, u16, &[u8], usize)> {
    let header = bytes.get(at..at + 8)?;
    let version = u16::from_le_bytes([header[0], header[1]]);
    let kind = u16::from_le_bytes([header[2], header[3]]);
    let length = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as usize;
    let body = bytes.get(at + 8..at + 8 + length)?;
    Some((kind, version >> 4, body, at + 8 + length))
}

/// Walks a drawing's records, collecting each shape by its number. `depth`
/// is how many groups deep the walk is: the drawing's own group is one, and
/// a shape inside a group inside that is part of a group.
fn walk(bytes: &[u8], depth: usize, shapes: &mut HashMap<u32, Drawn>) {
    let mut at = 0;
    while let Some((kind, _, body, next)) = record(bytes, at) {
        match kind {
            0xF003 => walk(body, depth + 1, shapes),
            0xF004 => {
                if let Some((spid, drawn)) = shape_record(body, depth) {
                    shapes.insert(spid, drawn);
                }
            }
            0xF002 => walk(body, depth, shapes),
            _ => {}
        }
        at = next;
    }
}

/// One shape's container: its number and kind, and its properties, from the
/// table of them and the second table Word 2000 keeps the newer ones in.
fn shape_record(bytes: &[u8], depth: usize) -> Option<(u32, Drawn)> {
    let mut at = 0;
    let mut found = None;
    let mut drawn = Drawn::default();
    while let Some((kind, instance, body, next)) = record(bytes, at) {
        match kind {
            0xF00A => {
                let spid = crate::plc::u32_at(body, 0);
                drawn.kind = instance;
                let flags = crate::plc::u32_at(body, 4);
                // A group, or a part of one: the flag, or being deeper than
                // the drawing's own group.
                let grouped = u32::from(flags & 0x0003 != 0 || depth > 1);
                drawn.flags = (flags & !0x0003) | grouped;
                found = Some(spid);
            }
            0xF00B | 0xF122 => options(body, usize::from(instance), &mut drawn),
            _ => {}
        }
        at = next;
    }
    Some((found?, drawn))
}

/// A table of properties: six bytes each, a number and a value, and after
/// them the bytes of each whose value is a length.
fn options(body: &[u8], count: usize, drawn: &mut Drawn) {
    let mut complex_at = count * 6;
    for index in 0..count {
        let Some(entry) = body.get(index * 6..index * 6 + 6) else { break };
        let id = u16::from_le_bytes([entry[0], entry[1]]);
        let value = u32::from_le_bytes([entry[2], entry[3], entry[4], entry[5]]);
        if id & 0x8000 != 0 {
            let length = value as usize;
            let bytes = body.get(complex_at..complex_at + length).unwrap_or(&[]).to_vec();
            complex_at += length;
            drawn.complex.insert(id & 0x3FFF, bytes);
        } else {
            drawn.properties.insert(id & 0x3FFF, value);
        }
    }
}

/// Where a floating drawing sits and how the text goes round it, from its
/// place in the story's table and its own properties — which, from Word 2000
/// on, say what it is measured from more finely than the table does.
pub(crate) fn anchor_of(placed: &Placed, drawn: &Drawn) -> Anchor {
    let flags = placed.flags;
    let across = match drawn.number(FROM_ACROSS) {
        Some(0) => Relative::Margin,
        Some(1) => Relative::Page,
        Some(3) => Relative::Character,
        Some(4) => Relative::LeftMargin,
        Some(5) => Relative::RightMargin,
        Some(6) => Relative::InsideMargin,
        Some(7) => Relative::OutsideMargin,
        Some(_) => Relative::Column,
        None => match (flags >> 1) & 0x03 {
            0 => Relative::Margin,
            1 => Relative::Page,
            _ => Relative::Column,
        },
    };
    let down = match drawn.number(FROM_DOWN) {
        Some(0) => Relative::Margin,
        Some(1) => Relative::Page,
        Some(3) => Relative::Line,
        Some(4) => Relative::TopMargin,
        Some(5) => Relative::BottomMargin,
        Some(6) => Relative::InsideMargin,
        Some(7) => Relative::OutsideMargin,
        Some(_) => Relative::Paragraph,
        None => match (flags >> 3) & 0x03 {
            0 => Relative::Margin,
            1 => Relative::Page,
            _ => Relative::Paragraph,
        },
    };
    let aligned = |word: &str| Placement::Aligned(word.to_owned());
    let [left, top, ..] = placed.rectangle;
    let horizontal = match drawn.number(ALIGN_ACROSS) {
        Some(1) => aligned("left"),
        Some(2) => aligned("center"),
        Some(3) => aligned("right"),
        Some(4) => aligned("inside"),
        Some(5) => aligned("outside"),
        _ => Placement::Offset(i64::from(left) * EMU_PER_TWIP),
    };
    let vertical = match drawn.number(ALIGN_DOWN) {
        Some(1) => aligned("top"),
        Some(2) => aligned("center"),
        Some(3) => aligned("bottom"),
        Some(4) => aligned("inside"),
        Some(5) => aligned("outside"),
        _ => Placement::Offset(i64::from(top) * EMU_PER_TWIP),
    };
    let wrap = match (flags >> 5) & 0x0F {
        1 => Wrap::TopAndBottom,
        3 => Wrap::None,
        4 => Wrap::Tight,
        5 => Wrap::Through,
        _ => Wrap::Square,
    };
    let side = match (flags >> 9) & 0x0F {
        1 => WrapSide::Left,
        2 => WrapSide::Right,
        3 => WrapSide::Largest,
        _ => WrapSide::BothSides,
    };
    let below = flags & 0x4000 != 0;
    let behind = drawn.switch(GROUP_SWITCHES, 5).unwrap_or(below);
    let distance = |id: u16, usual: i64| drawn.number(id).map_or(usual, i64::from);
    Anchor {
        wrap,
        side,
        behind_text: behind && wrap == Wrap::None,
        locked: flags & 0x8000 != 0,
        horizontal_from: across,
        horizontal,
        vertical_from: down,
        vertical,
        distance: (
            distance(WRAP_LEFT, 114_300),
            distance(WRAP_RIGHT, 114_300),
            distance(WRAP_TOP, 0),
            distance(WRAP_BOTTOM, 0),
        ),
        depth: USUAL_DEPTH,
        ..Anchor::default()
    }
}

/// The size a drawing is drawn at, in English metric units.
pub(crate) fn size_of(placed: &Placed) -> (i64, i64) {
    let [left, top, right, bottom] = placed.rectangle;
    (
        i64::from((right - left).max(0)) * EMU_PER_TWIP,
        i64::from((bottom - top).max(0)) * EMU_PER_TWIP,
    )
}

/// A floating shape, as the model has one: a text box with its words, a
/// rectangle, an oval, a line. What the model has no preset for — a
/// freeform, WordArt — is `None`.
pub(crate) fn shape_of(placed: &Placed, drawn: &Drawn, text: Vec<Paragraph>) -> Option<Shape> {
    let kind = if drawn.kind == 0 && drawn.text_story().is_some() { 202 } else { drawn.kind };
    let preset = wp_docx::shapes::office_preset(i64::from(kind))?;
    let (width_emu, height_emu) = size_of(placed);
    let line = matches!(preset, "line" | "straightConnector1");
    let fill = if line || drawn.switch(FILL_SWITCHES, 4) == Some(false) {
        Fill::None
    } else {
        Fill::Solid(Colour::rgb(&drawn.colour(FILL_COLOUR).unwrap_or_else(|| "FFFFFF".to_owned())))
    };
    let lined = drawn.switch(LINE_SWITCHES, 3) != Some(false);
    let outline = lined
        .then(|| Colour::rgb(&drawn.colour(LINE_COLOUR).unwrap_or_else(|| "000000".to_owned())));
    // A turn is a whole number of degrees and a fraction in sixty-five
    // thousand five hundred and thirty-sixths.
    let rotation = drawn
        .number(ROTATION)
        .map_or(0, |turn| i32::try_from(i64::from(turn as i32) * 60_000 / 65_536).unwrap_or(0));
    let name = drawn.text(NAME).filter(|name| !name.is_empty()).unwrap_or_else(|| {
        if kind == 202 {
            "Text Box".to_owned()
        } else {
            "Shape".to_owned()
        }
    });
    Some(Shape {
        preset: preset.to_owned(),
        width_emu,
        height_emu,
        fill,
        outline,
        outline_emu: if lined { drawn.number(LINE_WIDTH).map_or(9525, i64::from) } else { 0 },
        ink: None,
        name,
        id: placed.spid,
        anchor: Some(anchor_of(placed, drawn)),
        description: drawn.text(DESCRIPTION).unwrap_or_default(),
        rotation,
        flipped_across: drawn.flags & 0x40 != 0,
        flipped_down: drawn.flags & 0x80 != 0,
        text,
        ..Shape::default()
    })
}

/// A picture in the line, from its place in the data stream: the header that
/// says how big it is drawn, and the drawing record that holds its bytes or
/// names them in the store. Its bytes, their kind, and its size.
pub(crate) fn inline_picture(
    data: &[u8],
    word: &[u8],
    fc: u32,
    drawings: &Drawings,
) -> Option<(Vec<u8>, &'static str, i64, i64)> {
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
        // A drawing container holding the picture's bytes, or naming them
        // in the drawing store.
        0x64 | 0x66 => {
            blip_in(picture).or_else(|| drawings.stored_picture(pib_in(picture)?, word))?
        }
        // The oldest form: a metafile straight after the header.
        _ => (picture.to_vec(), "wmf"),
    };
    let width_emu = dxa_goal * mx / 1000 * EMU_PER_TWIP;
    let height_emu = dya_goal * my / 1000 * EMU_PER_TWIP;
    Some((bytes, extension, width_emu.max(9525), height_emu.max(9525)))
}

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
            if id == PICTURE {
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
