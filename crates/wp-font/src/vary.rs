//! Variable fonts: one file that is a whole family of them.
//!
//! # What a variable font is
//!
//! An ordinary font file is one typeface at one weight and one width. A
//! variable font is the same file with a set of *axes* — weight, width, slant,
//! optical size — and a pile of deltas saying how every point of every glyph
//! moves as each axis is turned. Set the axes and you have a typeface; set them
//! differently and you have another, and the file holds every weight between
//! Thin and Black rather than nine of them.
//!
//! It is not a curiosity. Windows ships several, every font Google Fonts serves
//! is one, and a document written with one names a weight that only exists if
//! the deltas are applied. A reader that ignores them draws every weight as the
//! one the designer happened to draw first, which for most families is Regular:
//! a document set in Thin comes out Regular, and nothing says so.
//!
//! # The pieces
//!
//! `fvar` names the axes and the *named instances* — the places on those axes
//! the designer thought worth a name, which is what a font menu lists. `avar`
//! bends the axis between its ends, so that the middle of Weight is where the
//! designer says rather than halfway. `gvar` holds the deltas for the outlines,
//! `CFF2` holds them inside the charstrings, and `HVAR` holds them for the
//! advance widths — because a heavier letter is a wider letter, and text set
//! without that is text set at the wrong width.
//!
//! # The two awkward parts
//!
//! A delta is not stored for every point. A font stores them for the points
//! that matter and leaves the rest to be worked out from their neighbours,
//! which is a rule of its own and is called interpolation of untouched points.
//! Get it wrong and letters come apart at the seams.
//!
//! And a delta does not apply everywhere. Each one belongs to a *region* of the
//! axes — from here, peaking there, to there — and how much of it applies is a
//! product across the axes of how far into that region the setting is. That
//! product is the scalar, and everything below is either finding it or spending
//! it.

use crate::glyf::Point;
use crate::read::Reader;
use crate::{Error, GlyphId};

/// One axis of a variable font.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Axis {
    /// The four letters the axis goes by: `wght`, `wdth`, `slnt`, `ital`,
    /// `opsz`, or whatever the designer invented.
    pub tag: [u8; 4],
    pub min: f32,
    pub default: f32,
    pub max: f32,
    /// Which string in the `name` table names it.
    pub name_id: u16,
    /// Whether the designer means it for a menu or only for the machinery.
    pub hidden: bool,
}

impl Axis {
    /// A setting of this axis brought inside its own limits and turned into the
    /// -1 to 1 the deltas are written against.
    #[must_use]
    pub fn normalize(&self, value: f32) -> f32 {
        let value = value.clamp(self.min, self.max);
        if value < self.default {
            if self.default > self.min {
                (value - self.default) / (self.default - self.min)
            } else {
                0.0
            }
        } else if value > self.default {
            if self.max > self.default {
                (value - self.default) / (self.max - self.default)
            } else {
                0.0
            }
        } else {
            0.0
        }
    }
}

/// A place on the axes the designer gave a name to.
#[derive(Clone, Debug, PartialEq)]
pub struct Instance {
    /// What it is called — "SemiBold", "Condensed Light" — where the `name`
    /// table could be read.
    pub name: Option<String>,
    /// Where it sits, one value per axis, in the axes' own units.
    pub coordinates: Vec<f32>,
}

/// The axes of a font and the instances of it, read from `fvar`.
#[derive(Clone, Debug, Default)]
pub(crate) struct Axes {
    pub(crate) axes: Vec<Axis>,
    pub(crate) instances: Vec<Instance>,
}

impl Axes {
    pub(crate) fn parse(
        table: &[u8],
        names: &dyn Fn(u16) -> Option<String>,
    ) -> Result<Self, Error> {
        let mut reader = Reader::new(table);
        if reader.u16()? != 1 {
            return Ok(Self::default());
        }
        reader.skip(2)?; // minor version
        let axes_at = reader.u16()? as usize;
        reader.skip(2)?; // reserved
        let axis_count = reader.u16()? as usize;
        let axis_size = reader.u16()? as usize;
        let instance_count = reader.u16()? as usize;
        let instance_size = reader.u16()? as usize;

        if axis_count == 0 || axis_size < 20 {
            return Ok(Self::default());
        }

        let mut axes = Vec::with_capacity(axis_count);
        for index in 0..axis_count {
            let mut record = Reader::at(table, axes_at + index * axis_size)?;
            axes.push(Axis {
                tag: record.tag()?,
                min: fixed(&mut record)?,
                default: fixed(&mut record)?,
                max: fixed(&mut record)?,
                hidden: record.u16()? & 1 != 0,
                name_id: record.u16()?,
            });
        }

        // The instances follow the axes, and each is as long as the header
        // said — which may be four bytes more than the coordinates need,
        // because a font may name the PostScript name of each instance too.
        let instances_at = axes_at + axis_count * axis_size;
        let mut instances = Vec::with_capacity(instance_count);
        if instance_size >= 4 + axis_count * 4 {
            for index in 0..instance_count {
                let mut record = Reader::at(table, instances_at + index * instance_size)?;
                let name_id = record.u16()?;
                reader.skip(0)?;
                record.skip(2)?; // flags
                let mut coordinates = Vec::with_capacity(axis_count);
                for _ in 0..axis_count {
                    coordinates.push(fixed(&mut record)?);
                }
                instances.push(Instance { name: names(name_id), coordinates });
            }
        }

        Ok(Self { axes, instances })
    }

    /// A setting of every axis, in the axes' own units, turned into the -1 to 1
    /// the deltas are written against.
    pub(crate) fn normalize(&self, coordinates: &[f32], bend: Option<&[u8]>) -> Vec<f32> {
        let mut out: Vec<f32> = self
            .axes
            .iter()
            .enumerate()
            .map(|(index, axis)| {
                axis.normalize(coordinates.get(index).copied().unwrap_or(axis.default))
            })
            .collect();
        if let Some(table) = bend {
            bend_axes(table, &mut out);
        }
        out
    }
}

/// A 16.16 number, which is what `fvar` measures an axis in.
fn fixed(reader: &mut Reader<'_>) -> Result<f32, Error> {
    Ok(reader.u32()? as i32 as f32 / 65536.0)
}

/// `avar`: the piecewise bend a font may put on an axis.
///
/// Without it the middle of the Weight axis is halfway between Thin and Black,
/// which is not where anybody would put Regular. The table is a list of
/// "this setting means that one" for each axis, and everything between two of
/// them is a straight line.
fn bend_axes(table: &[u8], coordinates: &mut [f32]) {
    let Ok(()) = try_bend(table, coordinates) else { return };
}

fn try_bend(table: &[u8], coordinates: &mut [f32]) -> Result<(), Error> {
    let mut reader = Reader::new(table);
    if reader.u16()? != 1 {
        return Ok(());
    }
    reader.skip(4)?; // minor version and a reserved field
    let axis_count = reader.u16()? as usize;

    for index in 0..axis_count {
        let pairs = reader.u16()? as usize;
        let mut maps = Vec::with_capacity(pairs);
        for _ in 0..pairs {
            maps.push((reader.f2dot14()?, reader.f2dot14()?));
        }
        let Some(value) = coordinates.get_mut(index) else { continue };
        *value = bend_one(&maps, *value);
    }
    Ok(())
}

/// One axis bent by its own list of pairs.
fn bend_one(maps: &[(f32, f32)], value: f32) -> f32 {
    if maps.len() < 2 {
        return value;
    }
    if value <= maps[0].0 {
        return maps[0].1;
    }
    for pair in maps.windows(2) {
        let (from, to) = (pair[0], pair[1]);
        if value < to.0 {
            if to.0 <= from.0 {
                return from.1;
            }
            let along = (value - from.0) / (to.0 - from.0);
            return from.1 + (to.1 - from.1) * along;
        }
    }
    maps[maps.len() - 1].1
}

// ---------------------------------------------------------------------------
// The store of deltas, which several tables share
// ---------------------------------------------------------------------------

/// How much of each region's deltas applies at a given setting of the axes.
///
/// A region says, for each axis, where its influence starts, where it is whole,
/// and where it ends. How much of it applies is how far into that triangle the
/// setting is, multiplied across every axis — so a delta drawn for "bold and
/// condensed" applies in full only when the text is both.
#[derive(Clone, Debug, Default)]
pub(crate) struct Regions {
    /// Per region, per axis: where it starts, peaks and ends.
    regions: Vec<Vec<(f32, f32, f32)>>,
}

impl Regions {
    fn parse(table: &[u8], at: usize) -> Result<Self, Error> {
        let mut reader = Reader::at(table, at)?;
        let axis_count = reader.u16()? as usize;
        let region_count = reader.u16()? as usize;

        let mut regions = Vec::with_capacity(region_count);
        for _ in 0..region_count {
            let mut axes = Vec::with_capacity(axis_count);
            for _ in 0..axis_count {
                axes.push((reader.f2dot14()?, reader.f2dot14()?, reader.f2dot14()?));
            }
            regions.push(axes);
        }
        Ok(Self { regions })
    }

    /// How much of one region applies, between nought and one.
    fn scalar(&self, region: usize, coordinates: &[f32]) -> f32 {
        let Some(axes) = self.regions.get(region) else { return 0.0 };
        let mut product = 1.0f32;
        for (index, (start, peak, end)) in axes.iter().enumerate() {
            // An axis this region says nothing about does not narrow it.
            if *peak == 0.0 {
                continue;
            }
            let value = coordinates.get(index).copied().unwrap_or(0.0);
            if value == *peak {
                continue;
            }
            if value <= *start || value >= *end {
                return 0.0;
            }
            product *= if value < *peak {
                (value - start) / (peak - start)
            } else {
                (end - value) / (end - peak)
            };
        }
        product
    }
}

/// One store of deltas: the regions, and the sets of numbers drawn for them.
///
/// `HVAR`, `MVAR` and `CFF2` all keep their deltas this way, so this is read
/// once and asked by all three.
#[derive(Clone, Debug, Default)]
pub(crate) struct Store<'a> {
    table: &'a [u8],
    regions: Regions,
    /// Where each set of deltas is, and how many regions it names.
    sets: Vec<usize>,
}

impl<'a> Store<'a> {
    pub(crate) fn parse(table: &'a [u8], at: usize) -> Result<Self, Error> {
        let mut reader = Reader::at(table, at)?;
        if reader.u16()? != 1 {
            return Ok(Self::default());
        }
        let regions_at = reader.u32()? as usize;
        let regions = Regions::parse(table, at.checked_add(regions_at).ok_or(Error::OutOfBounds)?)?;

        let count = reader.u16()? as usize;
        let mut sets = Vec::with_capacity(count);
        for _ in 0..count {
            let offset = reader.u32()? as usize;
            sets.push(at.checked_add(offset).ok_or(Error::OutOfBounds)?);
        }
        Ok(Self { table, regions, sets })
    }

    /// The deltas of one set, each scaled by how much of its region applies.
    pub(crate) fn scalars(&self, set: usize, coordinates: &[f32]) -> Vec<f32> {
        let Some(at) = self.sets.get(set) else { return Vec::new() };
        let Ok(mut reader) = Reader::at(self.table, at + 4) else { return Vec::new() };
        let Ok(count) = reader.u16() else { return Vec::new() };
        (0..count as usize)
            .map(|_| {
                reader.u16().map_or(0.0, |region| self.regions.scalar(region as usize, coordinates))
            })
            .collect()
    }

    /// One number out of the store, added up over the regions that apply.
    pub(crate) fn delta(&self, outer: usize, inner: usize, coordinates: &[f32]) -> f32 {
        let Some(at) = self.sets.get(outer) else { return 0.0 };
        let Ok(mut header) = Reader::at(self.table, *at) else { return 0.0 };
        let (Ok(items), Ok(word_count), Ok(region_count)) =
            (header.u16(), header.u16(), header.u16())
        else {
            return 0.0;
        };
        if inner >= items as usize {
            return 0.0;
        }

        // The newer form says its words are four bytes wide rather than two,
        // and the count of them is in the bits below that.
        let long = word_count & 0x8000 != 0;
        let words = (word_count & 0x7FFF) as usize;
        let region_count = region_count as usize;
        let wide = if long { 4 } else { 2 };
        let size = words * wide + (region_count.saturating_sub(words)) * (wide / 2);

        let scalars = self.scalars(outer, coordinates);
        let Ok(mut reader) = Reader::at(self.table, at + 6 + region_count * 2 + inner * size)
        else {
            return 0.0;
        };

        let mut total = 0.0f32;
        for region in 0..region_count {
            let delta = if region < words {
                if long {
                    reader.u32().map_or(0, |value| value as i32)
                } else {
                    reader.i16().map_or(0, i32::from)
                }
            } else if long {
                reader.i16().map_or(0, i32::from)
            } else {
                reader.i8().map_or(0, i32::from)
            };
            total += delta as f32 * scalars.get(region).copied().unwrap_or(0.0);
        }
        total
    }
}

/// A map from a glyph to which delta set holds its numbers.
///
/// Without one, the glyph number is the delta set's own number, which is what
/// a font with nothing unusual to say leaves it at.
#[derive(Clone, Copy, Debug)]
struct IndexMap<'a> {
    data: &'a [u8],
    at: usize,
    entry_size: usize,
    inner_bits: u32,
    count: usize,
}

impl<'a> IndexMap<'a> {
    fn parse(data: &'a [u8], at: usize) -> Result<Self, Error> {
        let mut reader = Reader::at(data, at)?;
        let format = reader.u8()?;
        let entry = reader.u8()?;
        let count = if format == 0 { reader.u16()? as usize } else { reader.u32()? as usize };
        Ok(Self {
            data,
            at: reader.position(),
            entry_size: usize::from(entry >> 4 & 0x0F) + 1,
            inner_bits: u32::from(entry & 0x0F) + 1,
            count,
        })
    }

    /// Which set and which number in it a glyph's delta is.
    fn of(&self, index: usize) -> (usize, usize) {
        // Past the end, the last entry stands for everything after it.
        let index = index.min(self.count.saturating_sub(1));
        let at = self.at + index * self.entry_size;
        let mut value = 0usize;
        for offset in 0..self.entry_size {
            let Some(byte) = self.data.get(at + offset) else { return (0, 0) };
            value = (value << 8) | *byte as usize;
        }
        (value >> self.inner_bits, value & ((1 << self.inner_bits) - 1))
    }
}

/// `HVAR`: how much wider or narrower each glyph is at a setting of the axes.
#[derive(Clone, Debug)]
pub(crate) struct Advances<'a> {
    store: Store<'a>,
    map: Option<IndexMap<'a>>,
}

impl<'a> Advances<'a> {
    pub(crate) fn parse(table: &'a [u8]) -> Result<Self, Error> {
        let mut reader = Reader::new(table);
        reader.skip(4)?; // version
        let store_at = reader.u32()? as usize;
        let map_at = reader.u32()? as usize;

        let store = Store::parse(table, store_at)?;
        let map = if map_at == 0 { None } else { Some(IndexMap::parse(table, map_at)?) };
        Ok(Self { store, map })
    }

    /// How much to add to one glyph's advance.
    pub(crate) fn delta(&self, glyph: GlyphId, coordinates: &[f32]) -> f32 {
        let (outer, inner) = match &self.map {
            Some(map) => map.of(usize::from(glyph.0)),
            // No map: the glyph number is the number in the first set.
            None => (0, usize::from(glyph.0)),
        };
        self.store.delta(outer, inner, coordinates)
    }
}

// ---------------------------------------------------------------------------
// gvar: how the outlines themselves move
// ---------------------------------------------------------------------------

/// `gvar`: the deltas for the points of every glyph.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Outlines<'a> {
    table: &'a [u8],
    axis_count: usize,
    shared_at: usize,
    shared_count: usize,
    data_at: usize,
    offsets_at: usize,
    long_offsets: bool,
    glyph_count: usize,
}

impl<'a> Outlines<'a> {
    pub(crate) fn parse(table: &'a [u8]) -> Result<Self, Error> {
        let mut reader = Reader::new(table);
        reader.skip(4)?; // version
        let axis_count = reader.u16()? as usize;
        let shared_count = reader.u16()? as usize;
        let shared_at = reader.u32()? as usize;
        let glyph_count = reader.u16()? as usize;
        let long_offsets = reader.u16()? & 1 != 0;
        let data_at = reader.u32()? as usize;
        let offsets_at = reader.position();

        Ok(Self {
            table,
            axis_count,
            shared_at,
            shared_count,
            data_at,
            offsets_at,
            long_offsets,
            glyph_count,
        })
    }

    /// Where one glyph's variation data sits.
    fn data_for(&self, glyph: GlyphId) -> Option<&'a [u8]> {
        let index = usize::from(glyph.0);
        if index >= self.glyph_count {
            return None;
        }
        let read = |at: usize| -> Option<usize> {
            if self.long_offsets {
                let bytes = self.table.get(at..at + 4)?;
                Some(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize)
            } else {
                let bytes = self.table.get(at..at + 2)?;
                Some(usize::from(u16::from_be_bytes([bytes[0], bytes[1]])) * 2)
            }
        };
        let step = if self.long_offsets { 4 } else { 2 };
        let from = read(self.offsets_at + index * step)?;
        let to = read(self.offsets_at + (index + 1) * step)?;
        if to <= from {
            return None;
        }
        self.table.get(self.data_at + from..self.data_at + to)
    }

    /// One of the tuples every glyph may point at rather than write out.
    fn shared_tuple(&self, index: usize) -> Option<Vec<f32>> {
        if index >= self.shared_count {
            return None;
        }
        let mut reader =
            Reader::at(self.table, self.shared_at + index * self.axis_count * 2).ok()?;
        (0..self.axis_count).map(|_| reader.f2dot14().ok()).collect()
    }

    /// How far every point of a glyph moves at a setting of the axes.
    ///
    /// `points` is what the outline holds before anything is applied, and
    /// `ends` where each contour of it ends, because the rule for the points a
    /// font did not bother to give a delta for works within one contour.
    pub(crate) fn deltas(
        &self,
        glyph: GlyphId,
        coordinates: &[f32],
        points: &[Point],
        ends: &[usize],
    ) -> Option<Vec<Point>> {
        let data = self.data_for(glyph)?;
        // A font numbers four more points than the glyph has: the two that say
        // where it begins and ends across, and the two for up and down. They
        // are not drawn, but they are counted, and a reader that does not count
        // them reads the next tuple's deltas as this one's.
        let total = points.len() + 4;
        let mut reader = Reader::new(data);
        let header = reader.u16().ok()?;
        let tuple_count = (header & 0x0FFF) as usize;
        let shared_points = header & 0x8000 != 0;
        let mut serialized = reader.u16().ok()? as usize;

        // The headers come one after another; the data they point into follows
        // all of them, each tuple's after the last.
        let mut headers = Vec::with_capacity(tuple_count);
        for _ in 0..tuple_count {
            let size = reader.u16().ok()? as usize;
            let index = reader.u16().ok()?;

            let peak = if index & 0x8000 != 0 {
                let mut tuple = Vec::with_capacity(self.axis_count);
                for _ in 0..self.axis_count {
                    tuple.push(reader.f2dot14().ok()?);
                }
                tuple
            } else {
                self.shared_tuple((index & 0x0FFF) as usize)?
            };

            let between = if index & 0x4000 != 0 {
                let mut start = Vec::with_capacity(self.axis_count);
                for _ in 0..self.axis_count {
                    start.push(reader.f2dot14().ok()?);
                }
                let mut end = Vec::with_capacity(self.axis_count);
                for _ in 0..self.axis_count {
                    end.push(reader.f2dot14().ok()?);
                }
                Some((start, end))
            } else {
                None
            };

            headers.push((size, index & 0x2000 != 0, peak, between));
        }

        // Point numbers shared by every tuple that does not bring its own.
        let mut shared: Option<Vec<usize>> = None;
        if shared_points {
            let mut at = Reader::at(data, serialized).ok()?;
            let numbers = packed_points(&mut at, total).ok()?;
            serialized = at.position();
            shared = Some(numbers);
        }

        let mut moved = vec![Point::new(0.0, 0.0); points.len()];
        let mut any = false;

        for (size, private, peak, between) in headers {
            let end = serialized.checked_add(size)?;
            let piece = data.get(serialized..end)?;
            serialized = end;

            let scalar = tuple_scalar(&peak, between.as_ref(), coordinates);
            if scalar == 0.0 {
                continue;
            }

            let mut at = Reader::new(piece);
            let numbers = if private {
                packed_points(&mut at, total).ok()?
            } else {
                match &shared {
                    Some(numbers) => numbers.clone(),
                    // No point numbers anywhere means every point of the glyph.
                    None => (0..total).collect(),
                }
            };

            let xs = packed_deltas(&mut at, numbers.len()).ok()?;
            let ys = packed_deltas(&mut at, numbers.len()).ok()?;

            // The deltas that were given, and then the ones that were not,
            // worked out from their neighbours.
            let mut given = vec![None; total];
            for (index, number) in numbers.iter().enumerate() {
                if *number < points.len() {
                    given[*number] = Some(Point::new(
                        xs.get(index).copied().unwrap_or(0.0),
                        ys.get(index).copied().unwrap_or(0.0),
                    ));
                }
            }
            infer(&mut given[..points.len()], points, ends);

            for (index, delta) in given.iter().take(points.len()).enumerate() {
                if let Some(delta) = delta {
                    moved[index].x += delta.x * scalar;
                    moved[index].y += delta.y * scalar;
                }
            }
            any = true;
        }

        any.then_some(moved)
    }
}

/// How much of one tuple applies at a setting of the axes.
fn tuple_scalar(peak: &[f32], between: Option<&(Vec<f32>, Vec<f32>)>, coordinates: &[f32]) -> f32 {
    let mut product = 1.0f32;
    for (index, peak) in peak.iter().enumerate() {
        if *peak == 0.0 {
            continue;
        }
        let value = coordinates.get(index).copied().unwrap_or(0.0);
        if value == *peak {
            continue;
        }
        if value == 0.0 {
            return 0.0;
        }

        // Without an in-between region a tuple reaches from nought to its peak
        // and back; with one it reaches wherever the font says.
        let (start, end) = match between {
            Some((start, end)) => {
                (start.get(index).copied().unwrap_or(0.0), end.get(index).copied().unwrap_or(0.0))
            }
            None => {
                if *peak > 0.0 {
                    (0.0, *peak)
                } else {
                    (*peak, 0.0)
                }
            }
        };
        if value <= start || value >= end {
            return 0.0;
        }
        product *= if value < *peak {
            (value - start) / (peak - start)
        } else {
            (end - value) / (end - peak)
        };
    }
    product
}

/// The point numbers a tuple's deltas belong to, packed as runs.
fn packed_points(reader: &mut Reader<'_>, total: usize) -> Result<Vec<usize>, Error> {
    let first = reader.u8()?;
    let count = if first & 0x80 != 0 {
        (usize::from(first & 0x7F) << 8) | usize::from(reader.u8()?)
    } else {
        usize::from(first)
    };
    // Nought means the deltas are for every point there is.
    if count == 0 {
        return Ok((0..total).collect());
    }

    let mut numbers = Vec::with_capacity(count);
    let mut value = 0usize;
    while numbers.len() < count {
        let control = reader.u8()?;
        let run = usize::from(control & 0x7F) + 1;
        for _ in 0..run {
            if numbers.len() >= count {
                break;
            }
            value += if control & 0x80 != 0 {
                usize::from(reader.u16()?)
            } else {
                usize::from(reader.u8()?)
            };
            numbers.push(value);
        }
    }
    Ok(numbers)
}

/// The deltas themselves, packed as runs of the same width — and a run that is
/// all noughts, which costs one byte however long it is.
fn packed_deltas(reader: &mut Reader<'_>, count: usize) -> Result<Vec<f32>, Error> {
    let mut out = Vec::with_capacity(count);
    while out.len() < count {
        let control = reader.u8()?;
        let run = usize::from(control & 0x3F) + 1;
        for _ in 0..run {
            if out.len() >= count {
                break;
            }
            if control & 0x80 != 0 {
                out.push(0.0);
            } else if control & 0x40 != 0 {
                out.push(f32::from(reader.i16()?));
            } else {
                out.push(f32::from(reader.i8()?));
            }
        }
    }
    Ok(out)
}

/// Works out the deltas of the points a font did not give one for.
///
/// # Why this is not optional
///
/// A font gives deltas for the points that carry the shape and leaves the rest
/// — the ones on a flat side, the ones in the middle of a curve — to be worked
/// out. The rule is per contour: a point between two that did move goes with
/// them, in proportion to where it sat between them. A reader that leaves them
/// where they were tears the letter apart at every one of them.
fn infer(deltas: &mut [Option<Point>], points: &[Point], ends: &[usize]) {
    let mut first = 0usize;
    for end in ends {
        let end = (*end).min(points.len().saturating_sub(1));
        if end < first {
            continue;
        }
        infer_contour(&mut deltas[first..=end], &points[first..=end]);
        first = end + 1;
    }
}

fn infer_contour(deltas: &mut [Option<Point>], points: &[Point]) {
    let count = deltas.len();
    let touched: Vec<usize> = (0..count).filter(|index| deltas[*index].is_some()).collect();

    match touched.len() {
        // Nothing moved, so nothing moves.
        0 => {}
        // One point moved, and the whole contour goes with it.
        1 => {
            let delta = deltas[touched[0]].expect("the point that moved");
            for slot in deltas.iter_mut() {
                *slot = Some(delta);
            }
        }
        _ => {
            for pair in 0..touched.len() {
                let from = touched[pair];
                let to = touched[(pair + 1) % touched.len()];
                let mut index = (from + 1) % count;
                while index != to {
                    let along = |get: fn(&Point) -> f32| {
                        between(
                            get(&points[index]),
                            get(&points[from]),
                            get(&points[to]),
                            get(&deltas[from].expect("touched")),
                            get(&deltas[to].expect("touched")),
                        )
                    };
                    deltas[index] =
                        Some(Point::new(along(|point| point.x), along(|point| point.y)));
                    index = (index + 1) % count;
                }
            }
        }
    }
}

/// One coordinate of an untouched point, worked out from the two touched ones
/// either side of it.
fn between(value: f32, from: f32, to: f32, from_delta: f32, to_delta: f32) -> f32 {
    let (low, low_delta, high, high_delta) = if from <= to {
        (from, from_delta, to, to_delta)
    } else {
        (to, to_delta, from, from_delta)
    };

    // Two points in the same place say nothing about which way to go, so
    // either they agree and the answer is what they agree on, or they do not
    // and the point stays where it is.
    if (high - low).abs() < f32::EPSILON {
        return if (from_delta - to_delta).abs() < f32::EPSILON { from_delta } else { 0.0 };
    }
    if value <= low {
        return low_delta;
    }
    if value >= high {
        return high_delta;
    }
    let along = (value - low) / (high - low);
    low_delta + (high_delta - low_delta) * along
}
