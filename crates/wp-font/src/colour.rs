//! Glyphs that are not one shape in one colour.
//!
//! # The three ways a font draws an emoji
//!
//! An ordinary glyph is an outline filled with whatever colour the text is.
//! That is no use for an emoji, which is a small picture: a yellow face with
//! brown eyes is not one shape and not one colour. Fonts solve it three ways,
//! and a word processor meets all three.
//!
//! **Layers.** `COLR` says a glyph is really several other glyphs drawn one on
//! top of another, and `CPAL` holds the palettes of colours to draw them in.
//! The glyphs are ordinary outlines, so they scale and print like any letter.
//! This is what Windows does — Segoe UI Emoji is a layered font — and so it is
//! what Word draws.
//!
//! **Bitmaps.** `CBDT` and `CBLC` hold a PNG per glyph per size, which is what
//! Android does and what Noto Color Emoji is. `sbix` is Apple's version of the
//! same idea. A bitmap is a photograph of a glyph: it does not scale, so a font
//! holds several sizes and the nearest is used.
//!
//! **Drawings.** `SVG ` holds an SVG document per glyph, and Apple's `sbix`
//! holds pictures the way `CBDT` does with the sizes listed rather than
//! indexed. Neither is here: there is no font of either kind to hold a reader
//! of it to, and a reader nobody has ever run is a claim rather than a feature.
//! Both are named in the roadmap.
//!
//! # What a palette is for
//!
//! A font may hold several, and the layers name a colour by its number in
//! whichever palette is in use rather than by its value. That is how one font
//! offers a light-background and a dark-background set of the same emoji. The
//! first palette is the one the designer means by default, and it is what is
//! used here.
//!
//! One palette entry number is special: `0xFFFF` means "whatever colour the
//! text is", which is how a layered glyph keeps a part of itself in the
//! document's own colour.

use crate::read::Reader;
use crate::{Error, GlyphId};

/// A colour as a font writes one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rgba {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub alpha: u8,
}

/// One layer of a glyph drawn as several: which glyph to draw, and in what.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layer {
    pub glyph: GlyphId,
    /// `None` where the layer is drawn in the colour of the text around it.
    pub colour: Option<Rgba>,
}

/// A glyph kept as a picture rather than as an outline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bitmap<'a> {
    /// The size the picture was drawn for, in pixels to the em. A glyph asked
    /// for at another size is this one scaled.
    pub pixels_per_em: u16,
    /// How far left of the pen the picture starts, and how far above the
    /// baseline its top edge is, in the pixels of that size.
    pub bearing_x: i8,
    pub bearing_y: i8,
    /// The picture itself. PNG, which every font of this kind uses.
    pub png: &'a [u8],
}

// ---------------------------------------------------------------------------
// Layers: COLR and CPAL
// ---------------------------------------------------------------------------

/// `CPAL`: the colours a layered font draws in.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Palette<'a> {
    table: &'a [u8],
    entries: usize,
    /// Where the first palette's colours begin.
    first: usize,
}

impl<'a> Palette<'a> {
    pub(crate) fn parse(table: &'a [u8]) -> Result<Self, Error> {
        let mut reader = Reader::new(table);
        reader.skip(2)?; // version
        let entries = reader.u16()? as usize;
        let palettes = reader.u16()? as usize;
        reader.skip(2)?; // how many colour records there are in all
        let records_at = reader.u32()? as usize;
        if palettes == 0 {
            return Err(Error::MalformedTable("CPAL"));
        }

        // The first palette is the one the designer means by default.
        let index = reader.u16()? as usize;
        let first = records_at
            .checked_add(index.checked_mul(4).ok_or(Error::OutOfBounds)?)
            .ok_or(Error::OutOfBounds)?;
        Ok(Self { table, entries, first })
    }

    /// One colour of the first palette.
    fn colour(&self, index: u16) -> Option<Rgba> {
        let index = usize::from(index);
        if index >= self.entries {
            return None;
        }
        // Written blue first, which is the order a Windows bitmap uses and
        // the one this table inherited from it.
        let at = self.first.checked_add(index * 4)?;
        let bytes = self.table.get(at..at + 4)?;
        Some(Rgba { blue: bytes[0], green: bytes[1], red: bytes[2], alpha: bytes[3] })
    }
}

/// `COLR`: which glyphs a glyph is really drawn from.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Layers<'a> {
    table: &'a [u8],
    bases: usize,
    base_count: usize,
    records: usize,
    record_count: usize,
}

impl<'a> Layers<'a> {
    pub(crate) fn parse(table: &'a [u8]) -> Result<Self, Error> {
        let mut reader = Reader::new(table);
        // Version 1 adds gradients and transforms; the list of plain layers is
        // in the same place in both, so a version 1 font still draws — without
        // its gradients, which is a separate item.
        reader.skip(2)?;
        let base_count = reader.u16()? as usize;
        let bases = reader.u32()? as usize;
        let records = reader.u32()? as usize;
        let record_count = reader.u16()? as usize;
        Ok(Self { table, bases, base_count, records, record_count })
    }

    /// The layers one glyph is drawn from, if it is drawn from any.
    fn of(&self, glyph: GlyphId, palette: Option<&Palette<'_>>) -> Option<Vec<Layer>> {
        // The base glyphs are in order, so this is a search rather than a walk:
        // a colour font has thousands of them.
        let mut low = 0usize;
        let mut high = self.base_count;
        let mut found = None;
        while low < high {
            let middle = (low + high) / 2;
            let at = self.bases.checked_add(middle * 6)?;
            let bytes = self.table.get(at..at + 6)?;
            let id = u16::from_be_bytes([bytes[0], bytes[1]]);
            match id.cmp(&glyph.0) {
                core::cmp::Ordering::Less => low = middle + 1,
                core::cmp::Ordering::Greater => high = middle,
                core::cmp::Ordering::Equal => {
                    found = Some((
                        usize::from(u16::from_be_bytes([bytes[2], bytes[3]])),
                        usize::from(u16::from_be_bytes([bytes[4], bytes[5]])),
                    ));
                    break;
                }
            }
        }

        let (first, count) = found?;
        if count == 0 || first + count > self.record_count {
            return None;
        }

        let mut out = Vec::with_capacity(count);
        for index in 0..count {
            let at = self.records.checked_add((first + index) * 4)?;
            let bytes = self.table.get(at..at + 4)?;
            let id = GlyphId(u16::from_be_bytes([bytes[0], bytes[1]]));
            let entry = u16::from_be_bytes([bytes[2], bytes[3]]);
            // The one number that is not a colour: this layer takes whatever
            // colour the text around it is.
            let colour = if entry == 0xFFFF {
                None
            } else {
                palette.and_then(|palette| palette.colour(entry))
            };
            out.push(Layer { glyph: id, colour });
        }
        Some(out)
    }
}

/// The layers of one glyph, given both tables.
pub(crate) fn layers_of(colr: &[u8], cpal: Option<&[u8]>, glyph: GlyphId) -> Option<Vec<Layer>> {
    let layers = Layers::parse(colr).ok()?;
    let palette = cpal.and_then(|table| Palette::parse(table).ok());
    layers.of(glyph, palette.as_ref())
}

// ---------------------------------------------------------------------------
// Bitmaps: CBLC and CBDT
// ---------------------------------------------------------------------------

/// One size a bitmap font was drawn at, and where its glyphs are listed.
#[derive(Clone, Copy, Debug)]
struct Strike {
    subtables_at: usize,
    subtable_count: usize,
    pixels_per_em: u16,
}

/// Finds a glyph's picture in a font that keeps them as bitmaps.
///
/// `wanted` is the size the text is being drawn at; the strike nearest it is
/// used, preferring one that is large enough, because a picture scaled down
/// looks like a picture and one scaled up looks like a mistake.
pub(crate) fn bitmap_of<'a>(
    locations: &'a [u8],
    data: &'a [u8],
    glyph: GlyphId,
    wanted: u16,
) -> Option<Bitmap<'a>> {
    let mut reader = Reader::new(locations);
    reader.skip(4).ok()?; // version
    let count = reader.u32().ok()? as usize;

    let mut best: Option<Strike> = None;
    for index in 0..count.min(64) {
        let at = 8 + index * 48;
        let mut strike = Reader::at(locations, at).ok()?;
        let subtables_at = strike.u32().ok()? as usize;
        strike.skip(4).ok()?; // how long the index tables are in all
        let subtable_count = strike.u32().ok()? as usize;
        strike.skip(4 + 12 + 12).ok()?; // the colour reference and the line metrics
        let first_glyph = strike.u16().ok()?;
        let last_glyph = strike.u16().ok()?;
        let pixels_per_em = u16::from(strike.u8().ok()?);

        if glyph.0 < first_glyph || glyph.0 > last_glyph || pixels_per_em == 0 {
            continue;
        }
        let found = Strike { subtables_at, subtable_count, pixels_per_em };
        best = Some(match best {
            Some(held) if nearer(held.pixels_per_em, found.pixels_per_em, wanted) => held,
            _ => found,
        });
    }

    let strike = best?;
    find_in_strike(locations, data, &strike, glyph)
}

/// Whether the size already held is the better answer for what was asked.
fn nearer(held: u16, other: u16, wanted: u16) -> bool {
    let score = |size: u16| {
        // A size at least as large as what is wanted beats every smaller one,
        // and among those the smallest wins.
        if size >= wanted {
            (0u8, u32::from(size) - u32::from(wanted))
        } else {
            (1u8, u32::from(wanted) - u32::from(size))
        }
    };
    score(held) <= score(other)
}

/// Walks one strike's index subtables for the glyph.
fn find_in_strike<'a>(
    locations: &'a [u8],
    data: &'a [u8],
    strike: &Strike,
    glyph: GlyphId,
) -> Option<Bitmap<'a>> {
    for index in 0..strike.subtable_count.min(4096) {
        let at = strike.subtables_at.checked_add(index * 8)?;
        let mut entry = Reader::at(locations, at).ok()?;
        let first = entry.u16().ok()?;
        let last = entry.u16().ok()?;
        let offset = entry.u32().ok()? as usize;
        if glyph.0 < first || glyph.0 > last {
            continue;
        }

        let subtable = strike.subtables_at.checked_add(offset)?;
        let mut header = Reader::at(locations, subtable).ok()?;
        let format = header.u16().ok()?;
        let image_format = header.u16().ok()?;
        let images_at = header.u32().ok()? as usize;
        let which = usize::from(glyph.0 - first);

        let (from, to) = match format {
            // Four-byte offsets, one per glyph and one past the end.
            1 => {
                let mut offsets = Reader::at(locations, subtable + 8 + which * 4).ok()?;
                (offsets.u32().ok()? as usize, offsets.u32().ok()? as usize)
            }
            // Every glyph the same size, one after another.
            2 => {
                let size = header.u32().ok()? as usize;
                (which * size, (which + 1) * size)
            }
            // Two-byte offsets, which is the same again for a small strike.
            3 => {
                let mut offsets = Reader::at(locations, subtable + 8 + which * 2).ok()?;
                (usize::from(offsets.u16().ok()?), usize::from(offsets.u16().ok()?))
            }
            // A list of the glyphs that are there at all, each with its offset.
            4 => {
                let pairs = header.u32().ok()? as usize;
                let mut found = None;
                for pair in 0..pairs.min(0xFFFF) {
                    let mut record = Reader::at(locations, subtable + 12 + pair * 4).ok()?;
                    if record.u16().ok()? == glyph.0 {
                        let from = usize::from(record.u16().ok()?);
                        let mut next =
                            Reader::at(locations, subtable + 12 + (pair + 1) * 4).ok()?;
                        next.skip(2).ok()?;
                        found = Some((from, usize::from(next.u16().ok()?)));
                        break;
                    }
                }
                found?
            }
            // Every glyph the same size again, with a list of which are there.
            5 => {
                let size = header.u32().ok()? as usize;
                header.skip(8).ok()?; // the metrics they all share
                let glyphs = header.u32().ok()? as usize;
                let mut found = None;
                for slot in 0..glyphs.min(0xFFFF) {
                    let mut record = Reader::at(locations, subtable + 24 + slot * 2).ok()?;
                    if record.u16().ok()? == glyph.0 {
                        found = Some((slot * size, (slot + 1) * size));
                        break;
                    }
                }
                found?
            }
            _ => continue,
        };

        if to <= from {
            return None;
        }
        let piece = data.get(images_at.checked_add(from)?..images_at.checked_add(to)?)?;
        return read_image(piece, image_format, strike.pixels_per_em);
    }
    None
}

/// One glyph's picture, in whichever of the three shapes the font wrote it.
fn read_image(piece: &[u8], format: u16, pixels_per_em: u16) -> Option<Bitmap<'_>> {
    match format {
        // Small metrics, then the length, then the PNG.
        17 => {
            let mut reader = Reader::new(piece);
            reader.skip(2).ok()?; // how tall and how wide, which the PNG says too
            let bearing_x = reader.i8().ok()?;
            let bearing_y = reader.i8().ok()?;
            reader.skip(1).ok()?; // the advance, which hmtx says as well
            let length = reader.u32().ok()? as usize;
            let at = reader.position();
            Some(Bitmap { pixels_per_em, bearing_x, bearing_y, png: piece.get(at..at + length)? })
        }
        // The same with the vertical metrics as well.
        18 => {
            let mut reader = Reader::new(piece);
            reader.skip(2).ok()?;
            let bearing_x = reader.i8().ok()?;
            let bearing_y = reader.i8().ok()?;
            reader.skip(4).ok()?;
            let length = reader.u32().ok()? as usize;
            let at = reader.position();
            Some(Bitmap { pixels_per_em, bearing_x, bearing_y, png: piece.get(at..at + length)? })
        }
        // No metrics at all: the strike said them once for every glyph in it.
        19 => {
            let mut reader = Reader::new(piece);
            let length = reader.u32().ok()? as usize;
            let at = reader.position();
            Some(Bitmap {
                pixels_per_em,
                bearing_x: 0,
                bearing_y: pixels_per_em.min(127) as i8,
                png: piece.get(at..at + length)?,
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `CPAL` holding one palette of the colours given.
    fn palette(colours: &[(u8, u8, u8, u8)]) -> Vec<u8> {
        let mut out = 0u16.to_be_bytes().to_vec(); // version
        out.extend_from_slice(&(colours.len() as u16).to_be_bytes());
        out.extend_from_slice(&1u16.to_be_bytes()); // one palette
        out.extend_from_slice(&(colours.len() as u16).to_be_bytes());
        // The colours follow the header and the one index into them.
        out.extend_from_slice(&14u32.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes()); // where this palette starts
        for (red, green, blue, alpha) in colours {
            out.extend_from_slice(&[*blue, *green, *red, *alpha]);
        }
        out
    }

    /// A `COLR` saying each base glyph is drawn from the layers given, as
    /// pairs of a glyph and a colour out of the palette.
    fn layered(bases: &[(u16, Vec<(u16, u16)>)]) -> Vec<u8> {
        let layer_count: usize = bases.iter().map(|(_, layers)| layers.len()).sum();
        let records_at = 14 + bases.len() * 6;

        let mut out = 0u16.to_be_bytes().to_vec(); // version
        out.extend_from_slice(&(bases.len() as u16).to_be_bytes());
        out.extend_from_slice(&14u32.to_be_bytes());
        out.extend_from_slice(&(records_at as u32).to_be_bytes());
        out.extend_from_slice(&(layer_count as u16).to_be_bytes());

        let mut first = 0u16;
        for (glyph, layers) in bases {
            out.extend_from_slice(&glyph.to_be_bytes());
            out.extend_from_slice(&first.to_be_bytes());
            out.extend_from_slice(&(layers.len() as u16).to_be_bytes());
            first += layers.len() as u16;
        }
        for (_, layers) in bases {
            for (glyph, colour) in layers {
                out.extend_from_slice(&glyph.to_be_bytes());
                out.extend_from_slice(&colour.to_be_bytes());
            }
        }
        out
    }

    #[test]
    fn a_layered_glyph_is_several_glyphs_in_their_own_colours() {
        let cpal = palette(&[(255, 200, 0, 255), (90, 60, 30, 255)]);
        let colr = layered(&[(5, vec![(40, 0), (41, 1)]), (9, vec![(50, 0)])]);

        let layers = layers_of(&colr, Some(&cpal), GlyphId(5)).expect("a layered glyph");
        assert_eq!(
            layers,
            vec![
                Layer {
                    glyph: GlyphId(40),
                    colour: Some(Rgba { red: 255, green: 200, blue: 0, alpha: 255 })
                },
                Layer {
                    glyph: GlyphId(41),
                    colour: Some(Rgba { red: 90, green: 60, blue: 30, alpha: 255 })
                },
            ]
        );

        // The base glyphs are searched rather than walked, so one at the end
        // of the list has to come back as surely as one at the front.
        let other = layers_of(&colr, Some(&cpal), GlyphId(9)).expect("the other one");
        assert_eq!(other.len(), 1);
        assert_eq!(other[0].glyph, GlyphId(50));
    }

    #[test]
    fn a_glyph_with_no_layers_is_an_ordinary_glyph() {
        let cpal = palette(&[(255, 0, 0, 255)]);
        let colr = layered(&[(5, vec![(40, 0)])]);
        assert!(layers_of(&colr, Some(&cpal), GlyphId(4)).is_none());
        assert!(layers_of(&colr, Some(&cpal), GlyphId(6)).is_none());
    }

    #[test]
    fn a_layer_may_ask_for_the_colour_of_the_text_around_it() {
        // Which is how a layered glyph keeps a part of itself in whatever
        // colour the document is written in.
        let cpal = palette(&[(255, 0, 0, 255)]);
        let colr = layered(&[(5, vec![(40, 0xFFFF), (41, 0)])]);

        let layers = layers_of(&colr, Some(&cpal), GlyphId(5)).unwrap();
        assert_eq!(layers[0].colour, None, "the first layer takes the text's colour");
        assert!(layers[1].colour.is_some());
    }

    #[test]
    fn a_truncated_table_is_refused_rather_than_guessed_at() {
        let cpal = palette(&[(255, 0, 0, 255), (0, 255, 0, 255)]);
        let colr = layered(&[(5, vec![(40, 0), (41, 1)])]);
        for length in 0..colr.len() {
            let _ = layers_of(&colr[..length], Some(&cpal), GlyphId(5));
        }
        for length in 0..cpal.len() {
            let _ = layers_of(&colr, Some(&cpal[..length]), GlyphId(5));
        }
    }
}
