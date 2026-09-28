//! The newer kind of layered glyph: `COLR` version 1.
//!
//! # A tree rather than a list
//!
//! Version 0 says a glyph is a list of other glyphs, each filled with one
//! colour and drawn over the one before. Version 1 says a glyph is a *tree of
//! paints*: a gradient along a line, between two circles or round a point;
//! that gradient seen only through the outline of another glyph; the result
//! turned, scaled or skewed; two such results put together another way than
//! one over the other — multiplied, screened, kept only where the other is.
//! That is what lets a font draw a shaded, lit emoji in outlines rather than
//! in pictures, and what the newest emoji fonts are.
//!
//! What is read here is the tree, with every colour looked up and every
//! transform turned into one matrix: [`Paint`]. Drawing it is the layout's,
//! which has pictures to draw into; a font only says what is to be drawn.
//!
//! # What is and is not read
//!
//! Every one of the thirty-two paint formats. The sixteen that can vary are
//! read at the font's default — the values written, without the deltas a
//! variable font would move them by — which is what the font is where no axis
//! has been moved. A glyph that draws another colour glyph has that glyph's
//! tree put in its place, clipped to that glyph's box. A tree that points back
//! into itself, or grows past any size a real glyph has, is refused.

use crate::colour::{Palette, Rgba};
use crate::GlyphId;

/// A glyph drawn from a tree of paints, and the box it is drawn within.
#[derive(Clone, Debug, PartialEq)]
pub struct ColourGlyph {
    pub paint: Paint,
    /// The box nothing is drawn outside of, in font units, when the font says
    /// one: `(x_min, y_min, x_max, y_max)`.
    pub clip: Option<(f32, f32, f32, f32)>,
}

/// One node of a colour glyph's tree.
///
/// Coordinates are in font units with y upwards, as the font writes them.
#[derive(Clone, Debug, PartialEq)]
pub enum Paint {
    /// Several paints, each drawn over the ones before it.
    Layers(Vec<Paint>),
    /// One colour, everywhere.
    Solid(Swatch),
    /// A gradient whose colours are the same along every line parallel to
    /// the one through the first point and the third, running from the first
    /// point to the second.
    Linear { line: ColourLine, from: (f32, f32), to: (f32, f32), rotation: (f32, f32) },
    /// A gradient from one circle to another: each a centre and a radius.
    Radial { line: ColourLine, from: ((f32, f32), f32), to: ((f32, f32), f32) },
    /// A gradient round a point, from one angle to another, in degrees
    /// counted anticlockwise from the direction of x.
    Sweep { line: ColourLine, centre: (f32, f32), start: f32, end: f32 },
    /// A paint seen only through the outline of a glyph.
    Glyph { glyph: GlyphId, paint: Box<Paint> },
    /// A paint seen only through a box: another colour glyph's, drawn here.
    Clip { clip: (f32, f32, f32, f32), paint: Box<Paint> },
    /// A paint drawn in a space moved by a matrix: `[xx, yx, xy, yy, dx, dy]`,
    /// so that a point `(x, y)` of it is `(xx·x + xy·y + dx, yx·x + yy·y +
    /// dy)` of the space it is drawn into.
    Transform { matrix: [f32; 6], paint: Box<Paint> },
    /// Two paints put together another way than one over the other.
    Composite { source: Box<Paint>, mode: CompositeMode, backdrop: Box<Paint> },
}

/// A colour a paint names: one of the palette's, or the colour of the text,
/// either with an opacity of its own on top.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Swatch {
    Colour(Rgba),
    /// The colour of the text around the glyph, at this much of its opacity.
    Text {
        alpha: f32,
    },
}

/// A run of colours and what it does past its ends.
#[derive(Clone, Debug, PartialEq)]
pub struct ColourLine {
    pub extend: Extend,
    /// Distances along the run and the colour at each, in the order written.
    pub stops: Vec<(f32, Swatch)>,
}

/// What a run of colours does past its ends.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Extend {
    #[default]
    Pad,
    Repeat,
    Reflect,
}

/// How a composite paint puts its source onto its backdrop: the thirteen
/// Porter–Duff operators and the fifteen blend modes, in the order the format
/// numbers them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompositeMode {
    Clear,
    Source,
    Destination,
    SourceOver,
    DestinationOver,
    SourceIn,
    DestinationIn,
    SourceOut,
    DestinationOut,
    SourceAtop,
    DestinationAtop,
    Xor,
    Plus,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    Multiply,
    Hue,
    Saturation,
    Color,
    Luminosity,
}

impl CompositeMode {
    const ALL: [Self; 28] = [
        Self::Clear,
        Self::Source,
        Self::Destination,
        Self::SourceOver,
        Self::DestinationOver,
        Self::SourceIn,
        Self::DestinationIn,
        Self::SourceOut,
        Self::DestinationOut,
        Self::SourceAtop,
        Self::DestinationAtop,
        Self::Xor,
        Self::Plus,
        Self::Screen,
        Self::Overlay,
        Self::Darken,
        Self::Lighten,
        Self::ColorDodge,
        Self::ColorBurn,
        Self::HardLight,
        Self::SoftLight,
        Self::Difference,
        Self::Exclusion,
        Self::Multiply,
        Self::Hue,
        Self::Saturation,
        Self::Color,
        Self::Luminosity,
    ];
}

/// How deep a tree may go before it is taken for one that points back into
/// itself, and how many nodes it may have before it is taken for one built
/// to make a reader do too much work.
const MAX_DEPTH: u8 = 64;
const MAX_NODES: usize = 1 << 16;

/// The glyph's tree, if the font draws it as one.
pub(crate) fn colour_glyph(
    colr: &[u8],
    cpal: Option<&[u8]>,
    glyph: GlyphId,
) -> Option<ColourGlyph> {
    let header = Header::parse(colr)?;
    let at = header.base_paint(colr, glyph)?;
    let palette = cpal.and_then(|table| Palette::parse(table).ok());
    let mut reader = Reader { table: colr, header, palette, nodes: 0 };
    let paint = reader.paint(at, 0)?;
    Some(ColourGlyph { paint, clip: header.clip(colr, glyph) })
}

/// Where the version 1 lists are, when the table has them.
#[derive(Clone, Copy, Debug)]
struct Header {
    bases: usize,
    layers: usize,
    clips: usize,
}

impl Header {
    fn parse(table: &[u8]) -> Option<Self> {
        if u16_at(table, 0)? < 1 {
            return None;
        }
        Some(Self {
            bases: u32_at(table, 14)? as usize,
            layers: u32_at(table, 18)? as usize,
            clips: u32_at(table, 22)? as usize,
        })
    }

    /// Where a glyph's tree begins, searched for among the glyphs in order.
    fn base_paint(&self, table: &[u8], glyph: GlyphId) -> Option<usize> {
        if self.bases == 0 {
            return None;
        }
        let count = u32_at(table, self.bases)? as usize;
        let (mut low, mut high) = (0usize, count);
        while low < high {
            let middle = (low + high) / 2;
            let at = self.bases + 4 + middle * 6;
            match u16_at(table, at)?.cmp(&glyph.0) {
                core::cmp::Ordering::Less => low = middle + 1,
                core::cmp::Ordering::Greater => high = middle,
                core::cmp::Ordering::Equal => {
                    return self.bases.checked_add(u32_at(table, at + 2)? as usize);
                }
            }
        }
        None
    }

    /// Where the nth paint of the layer list begins.
    fn layer(&self, table: &[u8], number: usize) -> Option<usize> {
        if self.layers == 0 || number >= u32_at(table, self.layers)? as usize {
            return None;
        }
        self.layers.checked_add(u32_at(table, self.layers + 4 + number * 4)? as usize)
    }

    /// The box a glyph is drawn within, when the font gives it one.
    fn clip(&self, table: &[u8], glyph: GlyphId) -> Option<(f32, f32, f32, f32)> {
        if self.clips == 0 {
            return None;
        }
        let count = u32_at(table, self.clips + 1)? as usize;
        for number in 0..count.min(MAX_NODES) {
            let at = self.clips + 5 + number * 7;
            let (first, last) = (u16_at(table, at)?, u16_at(table, at + 2)?);
            if glyph.0 < first || glyph.0 > last {
                continue;
            }
            let clip = self.clips.checked_add(u24_at(table, at + 4)?)?;
            return Some((
                f32::from(i16_at(table, clip + 1)?),
                f32::from(i16_at(table, clip + 3)?),
                f32::from(i16_at(table, clip + 5)?),
                f32::from(i16_at(table, clip + 7)?),
            ));
        }
        None
    }
}

struct Reader<'a> {
    table: &'a [u8],
    header: Header,
    palette: Option<Palette<'a>>,
    nodes: usize,
}

impl Reader<'_> {
    /// The paint beginning at an offset into the table, and everything under
    /// it.
    fn paint(&mut self, at: usize, depth: u8) -> Option<Paint> {
        self.nodes += 1;
        if depth > MAX_DEPTH || self.nodes > MAX_NODES {
            return None;
        }
        let table = self.table;
        let child =
            |offset_at: usize| -> Option<usize> { at.checked_add(u24_at(table, offset_at)?) };
        let word = |offset: usize| -> Option<f32> { i16_at(table, at + offset).map(f32::from) };
        let small = |offset: usize| -> Option<f32> { f2dot14(table, at + offset) };
        let next = depth + 1;

        Some(match *table.get(at)? {
            1 => {
                let count = usize::from(*table.get(at + 1)?);
                let first = u32_at(table, at + 2)? as usize;
                let mut layers = Vec::with_capacity(count);
                for number in first..first + count {
                    let layer = self.header.layer(table, number)?;
                    layers.push(self.paint(layer, next)?);
                }
                Paint::Layers(layers)
            }
            2 | 3 => Paint::Solid(self.swatch(u16_at(table, at + 1)?, small(3)?)),
            4 | 5 => Paint::Linear {
                line: self.line(child(at + 1)?, *table.get(at)? == 5)?,
                from: (word(4)?, word(6)?),
                to: (word(8)?, word(10)?),
                rotation: (word(12)?, word(14)?),
            },
            6 | 7 => Paint::Radial {
                line: self.line(child(at + 1)?, *table.get(at)? == 7)?,
                from: ((word(4)?, word(6)?), f32::from(u16_at(table, at + 8)?)),
                to: ((word(10)?, word(12)?), f32::from(u16_at(table, at + 14)?)),
            },
            // The two angles of a sweep are written one half turn short of
            // what they are — a bias of one — so that the number the format
            // has, which stops short of two, reaches a whole turn and more.
            // Only these two: a rotation or a skew is written as it is.
            8 | 9 => Paint::Sweep {
                line: self.line(child(at + 1)?, *table.get(at)? == 9)?,
                centre: (word(4)?, word(6)?),
                start: (small(8)? + 1.0) * 180.0,
                end: (small(10)? + 1.0) * 180.0,
            },
            10 => Paint::Glyph {
                glyph: GlyphId(u16_at(table, at + 4)?),
                paint: Box::new(self.paint(child(at + 1)?, next)?),
            },
            11 => {
                let glyph = GlyphId(u16_at(table, at + 1)?);
                let base = self.header.base_paint(table, glyph)?;
                let paint = self.paint(base, next)?;
                match self.header.clip(table, glyph) {
                    Some(clip) => Paint::Clip { clip, paint: Box::new(paint) },
                    None => paint,
                }
            }
            12 | 13 => {
                let affine = child(at + 4)?;
                let fixed = |number: usize| -> Option<f32> {
                    Some(u32_at(table, affine + number * 4)? as i32 as f32 / 65536.0)
                };
                let matrix = [fixed(0)?, fixed(1)?, fixed(2)?, fixed(3)?, fixed(4)?, fixed(5)?];
                self.transformed(matrix, child(at + 1)?, next)?
            }
            14 | 15 => {
                self.transformed([1.0, 0.0, 0.0, 1.0, word(4)?, word(6)?], child(at + 1)?, next)?
            }
            16 | 17 => {
                self.transformed([small(4)?, 0.0, 0.0, small(6)?, 0.0, 0.0], child(at + 1)?, next)?
            }
            18 | 19 => {
                let matrix = [small(4)?, 0.0, 0.0, small(6)?, 0.0, 0.0];
                self.transformed(about(matrix, word(8)?, word(10)?), child(at + 1)?, next)?
            }
            20 | 21 => {
                let scale = small(4)?;
                self.transformed([scale, 0.0, 0.0, scale, 0.0, 0.0], child(at + 1)?, next)?
            }
            22 | 23 => {
                let scale = small(4)?;
                let matrix = [scale, 0.0, 0.0, scale, 0.0, 0.0];
                self.transformed(about(matrix, word(6)?, word(8)?), child(at + 1)?, next)?
            }
            24 | 25 => self.transformed(rotation(small(4)?), child(at + 1)?, next)?,
            26 | 27 => {
                let matrix = about(rotation(small(4)?), word(6)?, word(8)?);
                self.transformed(matrix, child(at + 1)?, next)?
            }
            28 | 29 => self.transformed(skew(small(4)?, small(6)?), child(at + 1)?, next)?,
            30 | 31 => {
                let matrix = about(skew(small(4)?, small(6)?), word(8)?, word(10)?);
                self.transformed(matrix, child(at + 1)?, next)?
            }
            32 => Paint::Composite {
                source: Box::new(self.paint(child(at + 1)?, next)?),
                mode: *CompositeMode::ALL.get(usize::from(*table.get(at + 4)?))?,
                backdrop: Box::new(self.paint(child(at + 5)?, next)?),
            },
            _ => return None,
        })
    }

    fn transformed(&mut self, matrix: [f32; 6], at: usize, depth: u8) -> Option<Paint> {
        Some(Paint::Transform { matrix, paint: Box::new(self.paint(at, depth)?) })
    }

    /// A run of colours. The kind that can vary writes four more bytes for
    /// every stop, which are read past.
    fn line(&self, at: usize, varied: bool) -> Option<ColourLine> {
        let table = self.table;
        let extend = match *table.get(at)? {
            1 => Extend::Repeat,
            2 => Extend::Reflect,
            _ => Extend::Pad,
        };
        let count = usize::from(u16_at(table, at + 1)?);
        let stride = if varied { 10 } else { 6 };
        let mut stops = Vec::with_capacity(count);
        for number in 0..count.min(MAX_NODES) {
            let stop = at + 3 + number * stride;
            let offset = f2dot14(table, stop)?;
            stops.push((offset, self.swatch(u16_at(table, stop + 2)?, f2dot14(table, stop + 4)?)));
        }
        Some(ColourLine { extend, stops })
    }

    /// A colour of the palette at an opacity, or the colour of the text.
    fn swatch(&self, index: u16, alpha: f32) -> Swatch {
        let alpha = alpha.clamp(0.0, 1.0);
        if index == 0xFFFF {
            return Swatch::Text { alpha };
        }
        let colour =
            self.palette.as_ref().and_then(|palette| palette.colour(index)).unwrap_or_default();
        Swatch::Colour(Rgba { alpha: (f32::from(colour.alpha) * alpha).round() as u8, ..colour })
    }
}

/// A matrix made to act about a point rather than about the origin.
fn about(matrix: [f32; 6], x: f32, y: f32) -> [f32; 6] {
    let [xx, yx, xy, yy, dx, dy] = matrix;
    // Moved to the origin, acted on, and moved back.
    [xx, yx, xy, yy, dx + x - (xx * x + xy * y), dy + y - (yx * x + yy * y)]
}

/// A turn anticlockwise by an angle written as a fraction of half a turn.
fn rotation(half_turns: f32) -> [f32; 6] {
    let (sin, cos) = (half_turns * core::f32::consts::PI).sin_cos();
    [cos, sin, -sin, cos, 0.0, 0.0]
}

/// A skew by two angles written as fractions of half a turn: the format's
/// `xy = -tan(φ)`, `yx = tan(ψ)`.
fn skew(x: f32, y: f32) -> [f32; 6] {
    let (x, y) = (x * core::f32::consts::PI, y * core::f32::consts::PI);
    [1.0, y.tan(), -x.tan(), 1.0, 0.0, 0.0]
}

fn u16_at(table: &[u8], at: usize) -> Option<u16> {
    let bytes = table.get(at..at.checked_add(2)?)?;
    Some(u16::from_be_bytes([bytes[0], bytes[1]]))
}

fn i16_at(table: &[u8], at: usize) -> Option<i16> {
    u16_at(table, at).map(|value| value as i16)
}

fn u24_at(table: &[u8], at: usize) -> Option<usize> {
    let bytes = table.get(at..at.checked_add(3)?)?;
    Some((usize::from(bytes[0]) << 16) | (usize::from(bytes[1]) << 8) | usize::from(bytes[2]))
}

fn u32_at(table: &[u8], at: usize) -> Option<u32> {
    let bytes = table.get(at..at.checked_add(4)?)?;
    Some(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// A number with fourteen bits after the point, as fonts write the small
/// ones.
fn f2dot14(table: &[u8], at: usize) -> Option<f32> {
    i16_at(table, at).map(|value| f32::from(value) / 16384.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_matrix_about_a_point_leaves_the_point_where_it_is() {
        let turned = about(rotation(0.5), 100.0, 50.0);
        let [xx, yx, xy, yy, dx, dy] = turned;
        let (x, y) = (xx * 100.0 + xy * 50.0 + dx, yx * 100.0 + yy * 50.0 + dy);
        assert!((x - 100.0).abs() < 1e-3 && (y - 50.0).abs() < 1e-3);
    }

    #[test]
    fn a_quarter_turn_takes_x_to_y() {
        let [xx, yx, xy, yy, _, _] = rotation(0.5);
        assert!((xx).abs() < 1e-6 && (yx - 1.0).abs() < 1e-6);
        assert!((xy + 1.0).abs() < 1e-6 && yy.abs() < 1e-6);
    }

    #[test]
    fn a_table_that_is_not_version_one_has_no_trees() {
        let mut table = vec![0u8; 34];
        assert!(colour_glyph(&table, None, GlyphId(1)).is_none());
        table[1] = 1;
        assert!(colour_glyph(&table, None, GlyphId(1)).is_none());
    }
}
