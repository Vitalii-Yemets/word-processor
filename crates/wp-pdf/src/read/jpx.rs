//! JPEG 2000 pictures, as a PDF's `JPXDecode` filter carries them.
//!
//! [ITU-T T.800]. A picture is cut into tiles; each tile's components are
//! taken apart by a wavelet into bands of detail at halving resolutions;
//! each band is cut into code-blocks, and each block's coefficients are
//! written a bit-plane at a time with the MQ coder ([`blocks`]). The
//! blocks' bytes are then gathered into packets — one layer of one
//! precinct, a region, of one resolution of one component — each with a
//! header saying which blocks are in it and how much of each, and the
//! packets set out in one of five orders, which may change part-way.
//! Reading a picture is all of that backwards: the packets, each block's
//! bit-planes, the quantisation undone, the wavelet put back a resolution
//! at a time ([`wavelet`]), and the colour transform undone.
//!
//! What a PDF holds is the bare codestream or the JP2 file around it,
//! whose header may say the colour space, give a palette, and say which
//! channel is alpha.

mod blocks;
mod wavelet;

use std::collections::HashMap;

use blocks::{Orientation, Segment};
use wavelet::Plane;

/// The colour space a JP2 header names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Colour {
    Gray,
    Rgb,
    Cmyk,
}

/// A decoded picture: eight bits a sample, each pixel's channels together.
#[derive(Clone, Debug)]
pub struct Picture {
    pub width: usize,
    pub height: usize,
    pub channels: usize,
    pub samples: Vec<u8>,
    pub colour: Option<Colour>,
    /// Whether the last channel is the alpha.
    pub has_alpha: bool,
}

/// Decodes a JP2 file or a bare codestream.
#[must_use]
pub fn decode(data: &[u8]) -> Option<Picture> {
    let (codestream, header) = if data.starts_with(&[0xFF, 0x4F]) {
        (data, Header::default())
    } else if data.get(4..8) == Some(b"jP  ") {
        container(data)?
    } else {
        let at = (0..data.len().saturating_sub(3))
            .find(|&at| data[at..].starts_with(&[0xFF, 0x4F, 0xFF, 0x51]))?;
        (&data[at..], Header::default())
    };
    let image = Codestream::read(codestream)?.decode()?;
    Some(picture(image, &header))
}

// ---------------------------------------------------------------------------
// The JP2 file

/// What the JP2 header says.
#[derive(Clone, Debug, Default)]
struct Header {
    /// The enumerated colour space.
    space: Option<u32>,
    /// A palette: its columns' precisions, and its rows.
    palette: Option<(Vec<u32>, Vec<Vec<u32>>)>,
    /// For each channel out: its component, and the palette column or
    /// none.
    mapping: Vec<(usize, Option<usize>)>,
    alpha: bool,
}

fn boxes(data: &[u8]) -> Vec<([u8; 4], &[u8])> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while at + 8 <= data.len() {
        let length = u32::from_be_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]]);
        let kind = [data[at + 4], data[at + 5], data[at + 6], data[at + 7]];
        let (start, end) = match length {
            0 => (at + 8, data.len()),
            1 => {
                let Some(long) = data.get(at + 8..at + 16) else { break };
                let long = u64::from_be_bytes(long.try_into().ok().unwrap_or([0; 8]));
                (at + 16, at.saturating_add(usize::try_from(long).unwrap_or(usize::MAX)))
            }
            _ => (at + 8, at.saturating_add(length as usize)),
        };
        let end = end.min(data.len());
        if end < start {
            break;
        }
        out.push((kind, &data[start..end]));
        at = end;
    }
    out
}

fn container(data: &[u8]) -> Option<(&[u8], Header)> {
    let mut header = Header::default();
    let mut codestream = None;
    for (kind, content) in boxes(data) {
        match &kind {
            b"jp2h" => {
                for (kind, content) in boxes(content) {
                    match &kind {
                        b"colr" if content.first() == Some(&1) && content.len() >= 7 => {
                            header.space = Some(u32::from_be_bytes([
                                content[3], content[4], content[5], content[6],
                            ]));
                        }
                        b"pclr" => header.palette = palette(content),
                        b"cmap" => {
                            header.mapping = content
                                .chunks_exact(4)
                                .map(|entry| {
                                    let component =
                                        usize::from(u16::from_be_bytes([entry[0], entry[1]]));
                                    let column = (entry[2] == 1).then_some(usize::from(entry[3]));
                                    (component, column)
                                })
                                .collect();
                        }
                        b"cdef" => {
                            header.alpha =
                                content.get(2..).unwrap_or(&[]).chunks_exact(6).any(|entry| {
                                    matches!(u16::from_be_bytes([entry[2], entry[3]]), 1 | 2)
                                });
                        }
                        _ => {}
                    }
                }
            }
            b"jp2c" => codestream = Some(content),
            _ => {}
        }
    }
    Some((codestream?, header))
}

fn palette(content: &[u8]) -> Option<(Vec<u32>, Vec<Vec<u32>>)> {
    let entries = usize::from(u16::from_be_bytes([*content.first()?, *content.get(1)?]));
    let columns = usize::from(*content.get(2)?);
    let precisions: Vec<u32> =
        content.get(3..3 + columns)?.iter().map(|b| u32::from(b & 0x7F) + 1).collect();
    let mut at = 3 + columns;
    let mut rows = Vec::with_capacity(entries);
    for _ in 0..entries {
        let mut row = Vec::with_capacity(columns);
        for &precision in &precisions {
            let bytes = precision.div_ceil(8) as usize;
            let value =
                content.get(at..at + bytes)?.iter().fold(0u32, |v, b| (v << 8) | u32::from(*b));
            at += bytes;
            row.push(value);
        }
        rows.push(row);
    }
    Some((precisions, rows))
}

// ---------------------------------------------------------------------------
// The codestream's markers

#[derive(Clone, Copy, Debug)]
struct Component {
    signed: bool,
    precision: u32,
    dx: i64,
    dy: i64,
}

/// How one component's tiles are coded.
#[derive(Clone, Debug)]
struct Coding {
    levels: usize,
    block_width: u32,
    block_height: u32,
    style: u8,
    reversible: bool,
    /// The precinct size exponents by resolution.
    precincts: Vec<(u32, u32)>,
}

impl Default for Coding {
    fn default() -> Self {
        Self {
            levels: 5,
            block_width: 6,
            block_height: 6,
            style: 0,
            reversible: true,
            precincts: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Default)]
struct Quantisation {
    /// None, derived from the first band's step, or every band's given.
    style: u8,
    guard: u32,
    /// Exponent and mantissa, by band.
    steps: Vec<(u32, u32)>,
}

/// A progression order change: resolutions, components and layers, and
/// the order within.
#[derive(Clone, Copy, Debug)]
struct Change {
    resolutions: (usize, usize),
    components: (usize, usize),
    layers: usize,
    order: u8,
}

/// Everything the markers set, for the picture or for one tile.
#[derive(Clone, Debug, Default)]
struct Settings {
    sop: bool,
    eph: bool,
    order: u8,
    layers: usize,
    transform: bool,
    coding: Vec<Coding>,
    quantisation: Vec<Quantisation>,
    shift: Vec<u32>,
    changes: Vec<Change>,
}

/// Which components a component-specific marker has set at this level,
/// so a general one after does not undo it.
#[derive(Clone, Debug, Default)]
struct Specific {
    coding: Vec<bool>,
    quantisation: Vec<bool>,
}

struct Cursor<'a> {
    data: &'a [u8],
    at: usize,
}

impl Cursor<'_> {
    fn u8(&mut self) -> Option<u8> {
        let value = *self.data.get(self.at)?;
        self.at += 1;
        Some(value)
    }

    fn u16(&mut self) -> Option<u16> {
        Some(u16::from(self.u8()?) << 8 | u16::from(self.u8()?))
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from(self.u16()?) << 16 | u32::from(self.u16()?))
    }

    fn component(&mut self, many: bool) -> Option<usize> {
        Some(if many { usize::from(self.u16()?) } else { usize::from(self.u8()?) })
    }
}

/// The codestream read: its size, components, settings and tiles.
struct Codestream {
    width: i64,
    height: i64,
    x0: i64,
    y0: i64,
    tile_width: i64,
    tile_height: i64,
    tile_x0: i64,
    tile_y0: i64,
    components: Vec<Component>,
    tiles: Vec<TileData>,
}

#[derive(Default)]
struct TileData {
    index: usize,
    settings: Settings,
    data: Vec<u8>,
    /// Packet headers kept apart from the packets, if they were.
    headers: Option<Vec<u8>>,
}

impl Codestream {
    fn read(data: &[u8]) -> Option<Self> {
        let mut cursor = Cursor { data, at: 0 };
        if cursor.u16()? != 0xFF4F {
            return None;
        }
        let mut stream: Option<Self> = None;
        let mut main = Settings::default();
        let mut main_specific = Specific::default();
        let mut packed_main: Vec<u8> = Vec::new();
        let mut tiles: HashMap<usize, (TileData, Specific)> = HashMap::new();
        let mut order: Vec<usize> = Vec::new();
        let mut tile_parts = 0usize;
        let mut packed_entries: Option<Vec<Vec<u8>>> = None;
        loop {
            let marker = cursor.u16()?;
            match marker {
                0xFF90 => {
                    // A tile-part.
                    let start = cursor.at - 2;
                    let length = usize::from(cursor.u16()?);
                    let index = usize::from(cursor.u16()?);
                    let part_length = cursor.u32()? as usize;
                    let _part = cursor.u8()?;
                    let _parts = cursor.u8()?;
                    cursor.at = start + 2 + length;
                    let stream = stream.as_ref()?;
                    let count = stream.components.len();
                    if packed_entries.is_none() && !packed_main.is_empty() {
                        packed_entries = Some(ppm_entries(&packed_main));
                    }
                    let (tile, specific) = tiles.entry(index).or_insert_with(|| {
                        order.push(index);
                        (
                            TileData { index, settings: main.clone(), ..TileData::default() },
                            Specific {
                                coding: vec![false; count],
                                quantisation: vec![false; count],
                            },
                        )
                    });
                    // The tile-part's own markers, to the start of its data.
                    loop {
                        let marker = cursor.u16()?;
                        if marker == 0xFF93 {
                            break;
                        }
                        let length = usize::from(cursor.u16()?);
                        let content = data.get(cursor.at..cursor.at + length.saturating_sub(2))?;
                        if marker == 0xFF61 {
                            // Packed packet headers for this tile.
                            tile.headers
                                .get_or_insert_with(Vec::new)
                                .extend_from_slice(content.get(1..).unwrap_or(&[]));
                        } else {
                            apply_marker(marker, content, &mut tile.settings, specific, count)?;
                        }
                        cursor.at += length.saturating_sub(2);
                    }
                    let end = if part_length == 0 {
                        data.len()
                    } else {
                        (start + part_length).min(data.len())
                    };
                    let end = end.max(cursor.at);
                    tile.data.extend_from_slice(&data[cursor.at..end]);
                    if let Some(entries) = &packed_entries {
                        if let Some(entry) = entries.get(tile_parts) {
                            tile.headers.get_or_insert_with(Vec::new).extend_from_slice(entry);
                        }
                    }
                    tile_parts += 1;
                    cursor.at = end;
                    if cursor.at + 2 > data.len() {
                        break;
                    }
                }
                0xFFD9 => break,
                0xFF51 => {
                    let length = usize::from(cursor.u16()?);
                    let end = cursor.at + length - 2;
                    let _capabilities = cursor.u16()?;
                    let width = i64::from(cursor.u32()?);
                    let height = i64::from(cursor.u32()?);
                    let x0 = i64::from(cursor.u32()?);
                    let y0 = i64::from(cursor.u32()?);
                    let tile_width = i64::from(cursor.u32()?);
                    let tile_height = i64::from(cursor.u32()?);
                    let tile_x0 = i64::from(cursor.u32()?);
                    let tile_y0 = i64::from(cursor.u32()?);
                    let count = usize::from(cursor.u16()?);
                    let mut components = Vec::with_capacity(count);
                    for _ in 0..count {
                        let size = cursor.u8()?;
                        components.push(Component {
                            signed: size & 0x80 != 0,
                            precision: u32::from(size & 0x7F) + 1,
                            dx: i64::from(cursor.u8()?.max(1)),
                            dy: i64::from(cursor.u8()?.max(1)),
                        });
                    }
                    if width <= x0 || height <= y0 || tile_width <= 0 || tile_height <= 0 {
                        return None;
                    }
                    if count == 0 || (width - x0) * (height - y0) > 1 << 28 {
                        return None;
                    }
                    main.coding = vec![Coding::default(); count];
                    main.quantisation = vec![Quantisation::default(); count];
                    main.shift = vec![0; count];
                    main.layers = 1;
                    main_specific =
                        Specific { coding: vec![false; count], quantisation: vec![false; count] };
                    stream = Some(Self {
                        width,
                        height,
                        x0,
                        y0,
                        tile_width,
                        tile_height,
                        tile_x0,
                        tile_y0,
                        components,
                        tiles: Vec::new(),
                    });
                    cursor.at = end;
                }
                0xFF30..=0xFF3F => {}
                _ => {
                    let length = usize::from(cursor.u16()?);
                    let content = data.get(cursor.at..cursor.at + length.saturating_sub(2))?;
                    if marker == 0xFF60 {
                        packed_main.extend_from_slice(content.get(1..).unwrap_or(&[]));
                    } else if let Some(stream) = &stream {
                        let count = stream.components.len();
                        apply_marker(marker, content, &mut main, &mut main_specific, count)?;
                    }
                    cursor.at += length.saturating_sub(2);
                }
            }
        }
        let mut stream = stream?;
        for index in order {
            if let Some((tile, _)) = tiles.remove(&index) {
                stream.tiles.push(tile);
            }
        }
        Some(stream)
    }
}

/// The packed packet headers of the main header, one entry a tile-part.
fn ppm_entries(data: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut at = 0;
    while at + 4 <= data.len() {
        let length =
            u32::from_be_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]]) as usize;
        at += 4;
        let end = (at + length).min(data.len());
        out.push(data[at..end].to_vec());
        at = end;
    }
    out
}

fn coding_of(cursor: &mut Cursor<'_>, precincts_given: bool) -> Option<Coding> {
    let levels = usize::from(cursor.u8()?).min(32);
    let block_width = u32::from(cursor.u8()? & 0x0F) + 2;
    let block_height = u32::from(cursor.u8()? & 0x0F) + 2;
    let style = cursor.u8()?;
    let reversible = cursor.u8()? == 1;
    let mut precincts = Vec::new();
    if precincts_given {
        for _ in 0..=levels {
            let size = cursor.u8()?;
            precincts.push((u32::from(size & 0x0F), u32::from(size >> 4)));
        }
    }
    Some(Coding { levels, block_width, block_height, style, reversible, precincts })
}

fn quantisation_of(cursor: &mut Cursor<'_>) -> Option<Quantisation> {
    let kind = cursor.u8()?;
    let style = kind & 0x1F;
    let guard = u32::from(kind >> 5);
    let mut steps = Vec::new();
    if style == 0 {
        while let Some(byte) = cursor.u8() {
            steps.push((u32::from(byte >> 3), 0));
        }
    } else {
        while let Some(value) = cursor.u16() {
            steps.push((u32::from(value >> 11), u32::from(value & 0x7FF)));
        }
    }
    Some(Quantisation { style, guard, steps })
}

fn apply_marker(
    marker: u16,
    content: &[u8],
    settings: &mut Settings,
    specific: &mut Specific,
    count: usize,
) -> Option<()> {
    let many = count >= 257;
    let mut cursor = Cursor { data: content, at: 0 };
    match marker {
        0xFF52 => {
            let scod = cursor.u8()?;
            settings.sop = scod & 2 != 0;
            settings.eph = scod & 4 != 0;
            settings.order = cursor.u8()?;
            settings.layers = usize::from(cursor.u16()?).max(1);
            settings.transform = cursor.u8()? != 0;
            let coding = coding_of(&mut cursor, scod & 1 != 0)?;
            for (index, slot) in settings.coding.iter_mut().enumerate() {
                if !specific.coding[index] {
                    *slot = coding.clone();
                }
            }
        }
        0xFF53 => {
            let component = cursor.component(many)?;
            let scoc = cursor.u8()?;
            let coding = coding_of(&mut cursor, scoc & 1 != 0)?;
            if component < count {
                settings.coding[component] = coding;
                specific.coding[component] = true;
            }
        }
        0xFF5C => {
            let quantisation = quantisation_of(&mut cursor)?;
            for (index, slot) in settings.quantisation.iter_mut().enumerate() {
                if !specific.quantisation[index] {
                    *slot = quantisation.clone();
                }
            }
        }
        0xFF5D => {
            let component = cursor.component(many)?;
            let quantisation = quantisation_of(&mut cursor)?;
            if component < count {
                settings.quantisation[component] = quantisation;
                specific.quantisation[component] = true;
            }
        }
        0xFF5E => {
            let component = cursor.component(many)?;
            let _style = cursor.u8()?;
            let shift = u32::from(cursor.u8()?);
            if component < count {
                settings.shift[component] = shift;
            }
        }
        0xFF5F => {
            settings.changes.clear();
            while cursor.at < content.len() {
                let r0 = usize::from(cursor.u8()?);
                let c0 = cursor.component(many)?;
                let layers = usize::from(cursor.u16()?);
                let r1 = usize::from(cursor.u8()?);
                let c1 = match cursor.component(many)? {
                    0 => 256,
                    c => c,
                };
                let order = cursor.u8()?;
                settings.changes.push(Change {
                    resolutions: (r0, r1),
                    components: (c0, c1),
                    layers,
                    order,
                });
            }
        }
        _ => {}
    }
    Some(())
}

// ---------------------------------------------------------------------------
// Tiles

fn ceil_div(a: i64, b: i64) -> i64 {
    a.div_euclid(b) + i64::from(a.rem_euclid(b) != 0)
}

/// One level of a tag tree: its width, its height, and each node's value
/// and the least it is known to be.
type TreeLevel = (usize, usize, Vec<(u32, u32)>);

/// A tag tree: a quad-tree of numbers, each node the least of those below
/// it, coded from the root down so shared prefixes are said once.
#[derive(Clone, Debug)]
struct TagTree {
    /// Each level's width, height and nodes as (value, lower bound).
    levels: Vec<TreeLevel>,
}

impl TagTree {
    fn new(width: usize, height: usize) -> Self {
        let mut levels = Vec::new();
        let (mut w, mut h) = (width.max(1), height.max(1));
        loop {
            levels.push((w, h, vec![(u32::MAX, 0); w * h]));
            if w == 1 && h == 1 {
                break;
            }
            w = w.div_ceil(2);
            h = h.div_ceil(2);
        }
        Self { levels }
    }

    /// Whether the leaf's value is below `threshold`, reading as much as
    /// that takes.
    fn below(
        &mut self,
        bits: &mut HeaderBits<'_>,
        x: usize,
        y: usize,
        threshold: u32,
    ) -> Option<bool> {
        let mut low = 0;
        for level in (0..self.levels.len()).rev() {
            let (w, _, nodes) = &mut self.levels[level];
            let node = &mut nodes[(y >> level) * *w + (x >> level)];
            if low > node.1 {
                node.1 = low;
            } else {
                low = node.1;
            }
            while low < threshold && low < node.0 {
                if bits.bit()? == 1 {
                    node.0 = low;
                } else {
                    low += 1;
                }
            }
            node.1 = low;
        }
        let (_, _, leaves) = &self.levels[0];
        Some(leaves[y * self.levels[0].0 + x].0 < threshold)
    }

    /// The leaf's value, read all the way.
    fn value(&mut self, bits: &mut HeaderBits<'_>, x: usize, y: usize) -> Option<u32> {
        let mut threshold = 1;
        while !self.below(bits, x, y, threshold)? {
            threshold += 1;
            if threshold > 64 {
                return None;
            }
        }
        Some(threshold - 1)
    }
}

/// A packet header's bits: most significant first, with a nought stuffed
/// after every 0xFF.
struct HeaderBits<'a> {
    data: &'a [u8],
    at: usize,
    byte: u8,
    left: u32,
}

impl HeaderBits<'_> {
    fn bit(&mut self) -> Option<u32> {
        if self.left == 0 {
            let previous = if self.at > 0 { self.data.get(self.at - 1).copied() } else { None };
            self.byte = *self.data.get(self.at)?;
            self.at += 1;
            self.left = if previous == Some(0xFF) { 7 } else { 8 };
        }
        self.left -= 1;
        Some(u32::from((self.byte >> self.left) & 1))
    }

    fn bits(&mut self, count: u32) -> Option<u32> {
        let mut value = 0;
        for _ in 0..count {
            value = (value << 1) | self.bit()?;
        }
        Some(value)
    }

    /// To the next byte, past a stuffed one.
    fn align(&mut self) {
        self.left = 0;
        if self.at > 0 && self.data.get(self.at - 1) == Some(&0xFF) {
            self.at += 1;
        }
    }
}

#[derive(Clone, Debug)]
struct Block {
    x0: i64,
    y0: i64,
    x1: i64,
    y1: i64,
    included: bool,
    zero_planes: u32,
    length_bits: u32,
    passes: usize,
    segments: Vec<Segment>,
}

#[derive(Clone, Debug)]
struct PrecinctBand {
    wide: usize,
    blocks: Vec<Block>,
    inclusion: TagTree,
    zero_planes: TagTree,
}

#[derive(Clone, Debug)]
struct Band {
    orientation: Orientation,
    x0: i64,
    y0: i64,
    x1: i64,
    y1: i64,
    /// Magnitude bits, and the step to multiply by.
    planes: u32,
    step: f32,
    /// By precinct.
    precincts: Vec<PrecinctBand>,
}

#[derive(Clone, Debug)]
struct Resolution {
    x0: i64,
    y0: i64,
    x1: i64,
    y1: i64,
    precinct_width: u32,
    precinct_height: u32,
    precincts_wide: usize,
    precincts_high: usize,
    bands: Vec<Band>,
}

struct TileComponent {
    x0: i64,
    y0: i64,
    x1: i64,
    y1: i64,
    coding: Coding,
    shift: u32,
    resolutions: Vec<Resolution>,
}

impl Codestream {
    fn decode(&self) -> Option<Image> {
        let mut image = Image {
            x0: self.x0,
            y0: self.y0,
            width: (self.width - self.x0) as usize,
            height: (self.height - self.y0) as usize,
            planes: self
                .components
                .iter()
                .map(|c| {
                    let x0 = ceil_div(self.x0, c.dx);
                    let y0 = ceil_div(self.y0, c.dy);
                    Plane::new(x0, y0, ceil_div(self.width, c.dx), ceil_div(self.height, c.dy))
                })
                .collect(),
            components: self.components.clone(),
        };
        let tiles_wide = ceil_div(self.width - self.tile_x0, self.tile_width).max(1);
        for tile in &self.tiles {
            let p = tile.index as i64 % tiles_wide;
            let q = tile.index as i64 / tiles_wide;
            let x0 = (self.tile_x0 + p * self.tile_width).max(self.x0);
            let x1 = (self.tile_x0 + (p + 1) * self.tile_width).min(self.width);
            let y0 = (self.tile_y0 + q * self.tile_height).max(self.y0);
            let y1 = (self.tile_y0 + (q + 1) * self.tile_height).min(self.height);
            if x1 <= x0 || y1 <= y0 {
                continue;
            }
            self.decode_tile(tile, (x0, y0, x1, y1), &mut image);
        }
        Some(image)
    }

    fn decode_tile(&self, tile: &TileData, area: (i64, i64, i64, i64), image: &mut Image) {
        let settings = &tile.settings;
        let mut components: Vec<TileComponent> = self
            .components
            .iter()
            .enumerate()
            .map(|(index, component)| {
                tile_component(
                    component,
                    settings.coding.get(index).cloned().unwrap_or_default(),
                    settings.quantisation.get(index).cloned().unwrap_or_default(),
                    settings.shift.get(index).copied().unwrap_or(0),
                    area,
                )
            })
            .collect();
        read_packets(tile, settings, &mut components, &self.components, area);
        let mut planes: Vec<Plane> = components.iter().map(reconstruct).collect();
        if settings.transform && planes.len() >= 3 {
            let reversible = components[0].coding.reversible;
            let size = planes[0].data.len();
            if planes[1].data.len() == size && planes[2].data.len() == size {
                for index in 0..size {
                    let (y0, y1, y2) =
                        (planes[0].data[index], planes[1].data[index], planes[2].data[index]);
                    let (r, g, b) = if reversible {
                        let g = y0 - ((y1 + y2) / 4.0).floor();
                        (y2 + g, g, y1 + g)
                    } else {
                        (y0 + 1.402 * y2, y0 - 0.344_136 * y1 - 0.714_136 * y2, y0 + 1.772 * y1)
                    };
                    planes[0].data[index] = r;
                    planes[1].data[index] = g;
                    planes[2].data[index] = b;
                }
            }
        }
        for ((plane, component), target) in
            planes.iter().zip(&self.components).zip(&mut image.planes)
        {
            let offset =
                if component.signed { 0.0 } else { (1u64 << (component.precision - 1)) as f32 };
            let width = plane.width();
            for y in plane.y0..plane.y1 {
                for x in plane.x0..plane.x1 {
                    if x < target.x0 || y < target.y0 || x >= target.x1 || y >= target.y1 {
                        continue;
                    }
                    let value =
                        plane.data[((y - plane.y0) as usize) * width + (x - plane.x0) as usize];
                    let at = ((y - target.y0) * (target.x1 - target.x0) + (x - target.x0)) as usize;
                    target.data[at] = value + offset;
                }
            }
        }
    }
}

/// A tile's component laid out: its resolutions, bands, precincts and
/// code-blocks.
fn tile_component(
    component: &Component,
    coding: Coding,
    quantisation: Quantisation,
    shift: u32,
    (tx0, ty0, tx1, ty1): (i64, i64, i64, i64),
) -> TileComponent {
    let x0 = ceil_div(tx0, component.dx);
    let y0 = ceil_div(ty0, component.dy);
    let x1 = ceil_div(tx1, component.dx);
    let y1 = ceil_div(ty1, component.dy);
    let levels = coding.levels;
    let mut resolutions = Vec::with_capacity(levels + 1);
    for r in 0..=levels {
        let scale = 1i64 << (levels - r);
        let (rx0, ry0, rx1, ry1) =
            (ceil_div(x0, scale), ceil_div(y0, scale), ceil_div(x1, scale), ceil_div(y1, scale));
        let (ppx, ppy) = coding.precincts.get(r).copied().unwrap_or((15, 15));
        let count = |from: i64, to: i64, exponent: u32| -> usize {
            if to > from {
                (ceil_div(to, 1 << exponent) - from.div_euclid(1 << exponent)) as usize
            } else {
                0
            }
        };
        let precincts_wide = count(rx0, rx1, ppx);
        let precincts_high = count(ry0, ry1, ppy);
        let kinds: &[(Orientation, i64, i64)] = if r == 0 {
            &[(Orientation::LowLow, 0, 0)]
        } else {
            &[
                (Orientation::HighLow, 1, 0),
                (Orientation::LowHigh, 0, 1),
                (Orientation::HighHigh, 1, 1),
            ]
        };
        let mut bands = Vec::new();
        for (kind_index, &(orientation, xo, yo)) in kinds.iter().enumerate() {
            let (bx0, by0, bx1, by1) = if r == 0 {
                (rx0, ry0, rx1, ry1)
            } else {
                let level = (levels - r + 1) as u32;
                let half = 1i64 << (level - 1);
                let full = 1i64 << level;
                (
                    ceil_div(x0 - half * xo, full),
                    ceil_div(y0 - half * yo, full),
                    ceil_div(x1 - half * xo, full),
                    ceil_div(y1 - half * yo, full),
                )
            };
            // The band's step and magnitude bits.
            let band_index = if r == 0 { 0 } else { 1 + 3 * (r - 1) + kind_index };
            let (exponent, mantissa) = match quantisation.style {
                1 => {
                    let (e0, m0) = quantisation.steps.first().copied().unwrap_or((8, 0));
                    let level = if r == 0 { levels } else { levels - r + 1 };
                    ((e0 + level as u32).saturating_sub(levels as u32), m0)
                }
                _ => quantisation
                    .steps
                    .get(band_index)
                    .or(quantisation.steps.last())
                    .copied()
                    .unwrap_or((8, 0)),
            };
            let gain = match orientation {
                Orientation::LowLow => 0,
                Orientation::HighLow | Orientation::LowHigh => 1,
                Orientation::HighHigh => 2,
            };
            let planes = (quantisation.guard + exponent).saturating_sub(1);
            let step = if quantisation.style == 0 {
                1.0
            } else {
                let range = component.precision as i32 + gain;
                2f32.powi(range - exponent as i32) * (1.0 + mantissa as f32 / 2048.0)
            };
            // Precincts and code-blocks.
            let (pw, ph) =
                if r == 0 { (ppx, ppy) } else { (ppx.saturating_sub(1), ppy.saturating_sub(1)) };
            let (cw, ch) = (coding.block_width.min(pw), coding.block_height.min(ph));
            let first_px = rx0.div_euclid(1 << ppx);
            let first_py = ry0.div_euclid(1 << ppy);
            let mut precincts = Vec::with_capacity(precincts_wide * precincts_high);
            for py in 0..precincts_high as i64 {
                for px in 0..precincts_wide as i64 {
                    let (kx, ky) = (first_px + px, first_py + py);
                    let (rx0, ry0) = ((kx << pw).max(bx0), (ky << ph).max(by0));
                    let (rx1, ry1) = (((kx + 1) << pw).min(bx1), ((ky + 1) << ph).min(by1));
                    let (mut blocks, mut wide, mut high) = (Vec::new(), 0, 0);
                    if rx1 > rx0 && ry1 > ry0 {
                        let (bx_first, by_first) = (rx0 >> cw, ry0 >> ch);
                        let (bx_end, by_end) = (ceil_div(rx1, 1 << cw), ceil_div(ry1, 1 << ch));
                        wide = (bx_end - bx_first) as usize;
                        high = (by_end - by_first) as usize;
                        for by in by_first..by_end {
                            for bx in bx_first..bx_end {
                                blocks.push(Block {
                                    x0: (bx << cw).max(rx0),
                                    y0: (by << ch).max(ry0),
                                    x1: ((bx + 1) << cw).min(rx1),
                                    y1: ((by + 1) << ch).min(ry1),
                                    included: false,
                                    zero_planes: 0,
                                    length_bits: 3,
                                    passes: 0,
                                    segments: Vec::new(),
                                });
                            }
                        }
                    }
                    precincts.push(PrecinctBand {
                        wide,
                        blocks,
                        inclusion: TagTree::new(wide, high),
                        zero_planes: TagTree::new(wide, high),
                    });
                }
            }
            bands.push(Band {
                orientation,
                x0: bx0,
                y0: by0,
                x1: bx1,
                y1: by1,
                planes,
                step,
                precincts,
            });
        }
        resolutions.push(Resolution {
            x0: rx0,
            y0: ry0,
            x1: rx1,
            y1: ry1,
            precinct_width: ppx,
            precinct_height: ppy,
            precincts_wide,
            precincts_high,
            bands,
        });
    }
    TileComponent { x0, y0, x1, y1, coding, shift, resolutions }
}

/// One packet's place: layer, resolution, component, precinct.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct PacketId {
    layer: usize,
    resolution: usize,
    component: usize,
    precinct: usize,
}

/// The packets of a tile in the order they come.
fn packet_order(
    settings: &Settings,
    components: &[TileComponent],
    image_components: &[Component],
    (tx0, ty0, _, _): (i64, i64, i64, i64),
) -> Vec<PacketId> {
    let most_levels = components.iter().map(|c| c.coding.levels).max().unwrap_or(0);
    let mut progressions = settings.changes.clone();
    progressions.push(Change {
        resolutions: (0, most_levels + 1),
        components: (0, components.len()),
        layers: settings.layers,
        order: settings.order,
    });
    let mut next_layer: HashMap<(usize, usize, usize), usize> = HashMap::new();
    let mut out = Vec::new();
    for change in progressions {
        let mut candidates: Vec<([i64; 5], PacketId)> = Vec::new();
        for (c, component) in components.iter().enumerate() {
            if c < change.components.0 || c >= change.components.1 {
                continue;
            }
            let (dx, dy) = (image_components[c].dx, image_components[c].dy);
            for (r, resolution) in component.resolutions.iter().enumerate() {
                if r < change.resolutions.0 || r >= change.resolutions.1 {
                    continue;
                }
                let level = (component.coding.levels - r) as u32;
                let (ppx, ppy) = (resolution.precinct_width, resolution.precinct_height);
                let first_px = resolution.x0.div_euclid(1 << ppx);
                let first_py = resolution.y0.div_euclid(1 << ppy);
                for p in 0..resolution.precincts_wide * resolution.precincts_high {
                    let kx = first_px + (p % resolution.precincts_wide.max(1)) as i64;
                    let ky = first_py + (p / resolution.precincts_wide.max(1)) as i64;
                    // Where the precinct falls on the picture's grid, for
                    // the orders that go by position.
                    let x =
                        if (kx << ppx) < resolution.x0 { tx0 } else { (kx << ppx << level) * dx };
                    let y =
                        if (ky << ppy) < resolution.y0 { ty0 } else { (ky << ppy << level) * dy };
                    for layer in 0..change.layers.min(settings.layers) {
                        let (l, r, c, p) = (layer as i64, r as i64, c as i64, p as i64);
                        let key = match change.order {
                            0 => [l, r, c, p, 0],
                            1 => [r, l, c, p, 0],
                            2 => [r, y, x, c, l],
                            3 => [y, x, c, r, l],
                            _ => [c, y, x, r, l],
                        };
                        candidates.push((
                            key,
                            PacketId {
                                layer,
                                resolution: r as usize,
                                component: c as usize,
                                precinct: p as usize,
                            },
                        ));
                    }
                }
            }
        }
        candidates.sort_by_key(|(key, _)| *key);
        for (_, id) in candidates {
            let next = next_layer.entry((id.component, id.resolution, id.precinct)).or_insert(0);
            if id.layer == *next {
                *next += 1;
                out.push(id);
            }
        }
    }
    out
}

/// Reads a tile's packets into its code-blocks.
fn read_packets(
    tile: &TileData,
    settings: &Settings,
    components: &mut [TileComponent],
    image_components: &[Component],
    area: (i64, i64, i64, i64),
) {
    let order = packet_order(settings, components, image_components, area);
    let body = &tile.data;
    let mut body_at = 0usize;
    let mut header_at = 0usize;
    for id in order {
        if body_at >= body.len() && tile.headers.is_none() {
            break;
        }
        // A start-of-packet marker, if the packets carry them.
        if settings.sop && body.get(body_at..body_at + 2) == Some(&[0xFF, 0x91]) {
            body_at += 6;
        }
        let (headers, start) = match &tile.headers {
            Some(headers) => (headers.as_slice(), header_at),
            None => (body.as_slice(), body_at),
        };
        let mut bits = HeaderBits { data: headers, at: start, byte: 0, left: 0 };
        let component = &mut components[id.component];
        let style = component.coding.style;
        let Some(resolution) = component.resolutions.get_mut(id.resolution) else { continue };
        let pieces = match packet_header(&mut bits, resolution, id, style) {
            Some(pieces) => pieces,
            None => break,
        };
        bits.align();
        let mut end = bits.at;
        if settings.eph && headers.get(end..end + 2) == Some(&[0xFF, 0x92]) {
            end += 2;
        }
        if tile.headers.is_some() {
            header_at = end;
        } else {
            body_at = end;
        }
        for (band, block, segment, length) in pieces {
            let to = (body_at + length).min(body.len());
            let bytes = body.get(body_at..to).unwrap_or(&[]);
            let block = &mut resolution.bands[band].precincts[id.precinct].blocks[block];
            block.segments[segment].data.extend_from_slice(bytes);
            body_at = to;
        }
    }
}

/// A packet's header: which blocks are in it, how many passes each, and
/// how many bytes of which segment — the bytes follow in that order.
fn packet_header(
    bits: &mut HeaderBits<'_>,
    resolution: &mut Resolution,
    id: PacketId,
    style: u8,
) -> Option<Vec<(usize, usize, usize, usize)>> {
    let mut pieces = Vec::new();
    if bits.bit()? == 0 {
        return Some(pieces);
    }
    for (band_index, band) in resolution.bands.iter_mut().enumerate() {
        let Some(precinct) = band.precincts.get_mut(id.precinct) else { continue };
        let wide = precinct.wide.max(1);
        for block_index in 0..precinct.blocks.len() {
            let (x, y) = (block_index % wide, block_index / wide);
            let included = if precinct.blocks[block_index].included {
                bits.bit()? == 1
            } else {
                precinct.inclusion.below(bits, x, y, id.layer as u32 + 1)?
            };
            if !included {
                continue;
            }
            if !precinct.blocks[block_index].included {
                let zero = precinct.zero_planes.value(bits, x, y)?;
                let block = &mut precinct.blocks[block_index];
                block.zero_planes = zero;
                block.included = true;
            }
            let passes = passes_of(bits)?;
            let block = &mut precinct.blocks[block_index];
            while bits.bit()? == 1 {
                block.length_bits += 1;
            }
            // The new passes, split where the style ends a segment.
            let mut left = passes;
            while left > 0 {
                let open = block.segments.last().is_some_and(|s| s.passes < s.most);
                if !open {
                    block.segments.push(Segment {
                        data: Vec::new(),
                        passes: 0,
                        most: blocks::segment_room(style, block.passes),
                    });
                }
                let segment_index = block.segments.len() - 1;
                let segment = &mut block.segments[segment_index];
                let take = left.min(segment.most - segment.passes);
                segment.passes += take;
                block.passes += take;
                left -= take;
                let length_bits = block.length_bits + (usize::BITS - 1 - take.leading_zeros());
                let length = bits.bits(length_bits)? as usize;
                pieces.push((band_index, block_index, segment_index, length));
            }
        }
    }
    Some(pieces)
}

/// How many coding passes a block adds.
fn passes_of(bits: &mut HeaderBits<'_>) -> Option<usize> {
    if bits.bit()? == 0 {
        return Some(1);
    }
    if bits.bit()? == 0 {
        return Some(2);
    }
    let two = bits.bits(2)?;
    if two < 3 {
        return Some(3 + two as usize);
    }
    let five = bits.bits(5)?;
    if five < 31 {
        return Some(6 + five as usize);
    }
    Some(37 + bits.bits(7)? as usize)
}

/// A tile-component's samples, from its blocks through the wavelet.
fn reconstruct(component: &TileComponent) -> Plane {
    let reversible = component.coding.reversible;
    let mut bands: Vec<Vec<Plane>> = Vec::new();
    for resolution in &component.resolutions {
        let mut planes = Vec::new();
        for band in &resolution.bands {
            let mut plane = Plane::new(band.x0, band.y0, band.x1, band.y1);
            let width = plane.width();
            for precinct in &band.precincts {
                for block in &precinct.blocks {
                    if block.segments.is_empty() {
                        continue;
                    }
                    let (w, h) = ((block.x1 - block.x0) as usize, (block.y1 - block.y0) as usize);
                    let planes = (band.planes + component.shift).saturating_sub(block.zero_planes);
                    let (values, coded) = blocks::decode(
                        w,
                        h,
                        band.orientation,
                        planes,
                        &block.segments,
                        component.coding.style,
                    );
                    let threshold = 1i64 << component.shift.min(62);
                    for y in 0..h {
                        for x in 0..w {
                            let mut value = values[y * w + x];
                            if value == 0 {
                                continue;
                            }
                            if component.shift > 0 && value.abs() >= threshold {
                                value = value.signum() * (value.abs() >> component.shift);
                            }
                            // Halfway through what was not decoded; a reversible
                            // coefficient decoded to its last plane is exact.
                            let lowest = coded[y * w + x];
                            let half = if reversible && lowest == 0 {
                                0.0
                            } else {
                                (1i64 << lowest) as f32 / 2.0
                            };
                            let magnitude = value.abs() as f32 + half;
                            let sample = magnitude * band.step * value.signum() as f32;
                            let at = ((block.y0 - band.y0) as usize + y) * width
                                + (block.x0 - band.x0) as usize
                                + x;
                            if let Some(slot) = plane.data.get_mut(at) {
                                *slot = sample;
                            }
                        }
                    }
                }
            }
            planes.push(plane);
        }
        bands.push(planes);
    }
    let mut current = bands.first().and_then(|b| b.first()).cloned().unwrap_or_default();
    for (resolution, planes) in component.resolutions.iter().zip(&bands).skip(1) {
        if planes.len() < 3 {
            break;
        }
        current = wavelet::compose(
            &current,
            [&planes[0], &planes[1], &planes[2]],
            (resolution.x0, resolution.y0, resolution.x1, resolution.y1),
            reversible,
        );
    }
    let _ = (component.x0, component.y0, component.x1, component.y1);
    current
}

// ---------------------------------------------------------------------------
// The picture

/// The components' samples, on their own grids.
struct Image {
    x0: i64,
    y0: i64,
    width: usize,
    height: usize,
    planes: Vec<Plane>,
    components: Vec<Component>,
}

fn picture(image: Image, header: &Header) -> Picture {
    let Image { x0: x_origin, y0: y_origin, width, height, planes, components } = image;
    // Each component to eight bits, on the picture's grid.
    let mut channels: Vec<Vec<u32>> = Vec::with_capacity(planes.len());
    for (plane, component) in planes.iter().zip(&components) {
        let most = ((1u64 << component.precision) - 1) as f32;
        let mut samples = Vec::with_capacity(width * height);
        let plane_width = plane.width().max(1);
        for y in 0..height as i64 {
            let py = ((y_origin + y).div_euclid(component.dy) - plane.y0)
                .clamp(0, plane.height() as i64 - 1);
            for x in 0..width as i64 {
                let px = ((x_origin + x).div_euclid(component.dx) - plane.x0)
                    .clamp(0, plane_width as i64 - 1);
                let value =
                    plane.data.get(py as usize * plane_width + px as usize).copied().unwrap_or(0.0);
                samples.push(value.round().clamp(0.0, most) as u32);
            }
        }
        channels.push(samples);
    }
    let precisions: Vec<u32> = components.iter().map(|c| c.precision).collect();
    // The palette, where there is one.
    let (channels, precisions) = match &header.palette {
        Some((columns, rows)) if !header.mapping.is_empty() => {
            let mut out = Vec::new();
            let mut out_precisions = Vec::new();
            for &(component, column) in &header.mapping {
                let Some(source) = channels.get(component) else { continue };
                match column {
                    Some(column) => {
                        out.push(
                            source
                                .iter()
                                .map(|&index| {
                                    rows.get(index as usize)
                                        .and_then(|row| row.get(column))
                                        .copied()
                                        .unwrap_or(0)
                                })
                                .collect(),
                        );
                        out_precisions.push(columns.get(column).copied().unwrap_or(8));
                    }
                    None => {
                        out.push(source.clone());
                        out_precisions.push(precisions.get(component).copied().unwrap_or(8));
                    }
                }
            }
            (out, out_precisions)
        }
        _ => (channels, precisions),
    };
    let count = channels.len();
    let mut samples = vec![0u8; width * height * count];
    for (channel, (values, &precision)) in channels.iter().zip(&precisions).enumerate() {
        for (index, &value) in values.iter().enumerate() {
            let byte = if precision >= 8 {
                (value >> (precision - 8)) as u8
            } else {
                (value * 255 / ((1 << precision) - 1).max(1)) as u8
            };
            samples[index * count + channel] = byte;
        }
    }
    let colour = match header.space {
        Some(16 | 18 | 20 | 21) => Some(Colour::Rgb),
        Some(17) => Some(Colour::Gray),
        Some(12) => Some(Colour::Cmyk),
        _ => None,
    };
    if header.space == Some(18) && count >= 3 {
        // sYCC, which the component transform did not already undo.
        for pixel in samples.chunks_exact_mut(count) {
            let (y, cb, cr) =
                (f32::from(pixel[0]), f32::from(pixel[1]) - 128.0, f32::from(pixel[2]) - 128.0);
            pixel[0] = (y + 1.402 * cr).round().clamp(0.0, 255.0) as u8;
            pixel[1] = (y - 0.344_136 * cb - 0.714_136 * cr).round().clamp(0.0, 255.0) as u8;
            pixel[2] = (y + 1.772 * cb).round().clamp(0.0, 255.0) as u8;
        }
    }
    Picture { width, height, channels: count, samples, colour, has_alpha: header.alpha }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tag_tree_reads_its_leaves() {
        // A 2 by 1 grid holding 1 and 2. The root, their least, is "0 1";
        // the first leaf is the same as the root, "1"; the second is one
        // more, "0 1".
        let data = [0b0110_1000u8, 0];
        let mut bits = HeaderBits { data: &data, at: 0, byte: 0, left: 0 };
        let mut tree = TagTree::new(2, 1);
        assert_eq!(tree.value(&mut bits, 0, 0), Some(1));
        assert_eq!(tree.value(&mut bits, 1, 0), Some(2));
    }

    #[test]
    fn pass_counts_come_in_their_codes() {
        let data = [0x5Du8, 0xFE, 0];
        let mut bits = HeaderBits { data: &data, at: 0, byte: 0, left: 0 };
        assert_eq!(passes_of(&mut bits), Some(1));
        assert_eq!(passes_of(&mut bits), Some(2));
        assert_eq!(passes_of(&mut bits), Some(5));
        assert_eq!(passes_of(&mut bits), Some(6 + 30));
    }
}
