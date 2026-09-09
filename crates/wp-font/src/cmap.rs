//! The character-to-glyph mapping.
//!
//! A font does not store letters, it stores glyphs, and the `cmap` table says
//! which glyph a character is drawn with. There is no single format: a font
//! carries several subtables for different platforms, and the right one has to
//! be chosen. This picks the widest coverage available, preferring the format
//! that reaches beyond the Basic Multilingual Plane so that emoji and rarer CJK
//! characters are not silently lost.
//!
//! The chosen subtable is decoded once, into ranges, rather than being walked
//! again for every character laid out.

use crate::read::Reader;
use crate::{Error, GlyphId};

/// How a stretch of character codes maps to glyphs.
#[derive(Clone, Debug)]
enum Mapping {
    /// Glyph is the character code plus a fixed offset.
    Delta(i32),
    /// Glyph is looked up in a list, one entry per code in the range.
    Indices(Vec<u16>),
}

/// A contiguous run of character codes that map the same way.
#[derive(Clone, Debug)]
struct Segment {
    start: u32,
    end: u32,
    mapping: Mapping,
}

/// A decoded character-to-glyph mapping.
#[derive(Clone, Debug, Default)]
pub struct CharacterMap {
    /// Sorted by `start`, so a lookup is a binary search.
    segments: Vec<Segment>,
    /// True for a symbol font, whose characters live in the private use area at
    /// 0xF000 even though documents refer to them as ordinary Latin-1 codes.
    symbol_encoded: bool,
}

impl CharacterMap {
    /// A mapping with nothing in it, for a font with no usable `cmap`.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// How many code ranges the mapping holds.
    #[must_use]
    pub fn segment_count(&self) -> usize {
        self.segments.len()
    }

    /// Whether the font maps its characters through the symbol area.
    #[must_use]
    pub fn is_symbol_encoded(&self) -> bool {
        self.symbol_encoded
    }

    /// Every character the font can draw, with the glyph that draws it.
    ///
    /// The mapping the other way round from the one a renderer wants, and the
    /// one a program *writing* a PDF needs: a reader copying text out of the
    /// file gets its characters back through it.
    #[must_use]
    pub fn pairs(&self) -> Vec<(char, GlyphId)> {
        let mut out = Vec::new();
        for segment in &self.segments {
            for code in segment.start..=segment.end {
                let glyph = match &segment.mapping {
                    Mapping::Delta(delta) => {
                        let raw = (i64::from(code) + i64::from(*delta)) & 0xFFFF;
                        GlyphId(raw as u16)
                    }
                    Mapping::Indices(indices) => {
                        let at = (code - segment.start) as usize;
                        match indices.get(at) {
                            Some(0) | None => continue,
                            Some(index) => GlyphId(*index),
                        }
                    }
                };
                if glyph.0 == 0 {
                    continue;
                }
                if let Some(character) = char::from_u32(code) {
                    out.push((character, glyph));
                }
            }
        }
        out
    }

    /// The glyph a character maps to.
    #[must_use]
    pub fn glyph_for(&self, character: char) -> Option<GlyphId> {
        let code = character as u32;
        if let Some(glyph) = self.lookup(code) {
            return Some(glyph);
        }
        // A symbol font hides Latin-1 codes in the private use area.
        if self.symbol_encoded && code < 0x100 {
            return self.lookup(0xF000 + code);
        }
        None
    }

    fn lookup(&self, code: u32) -> Option<GlyphId> {
        let index = self
            .segments
            .binary_search_by(|segment| {
                if segment.end < code {
                    core::cmp::Ordering::Less
                } else if segment.start > code {
                    core::cmp::Ordering::Greater
                } else {
                    core::cmp::Ordering::Equal
                }
            })
            .ok()?;

        let segment = &self.segments[index];
        let glyph = match &segment.mapping {
            Mapping::Delta(delta) => {
                // The addition wraps at 16 bits, which the format relies on.
                let value = (code as i64 + i64::from(*delta)) as u32 & 0xFFFF;
                value as u16
            }
            Mapping::Indices(indices) => *indices.get((code - segment.start) as usize)?,
        };

        // Glyph zero is the "missing glyph" box, which is not a match.
        if glyph == 0 {
            None
        } else {
            Some(GlyphId(glyph))
        }
    }

    /// Reads the `cmap` table and decodes the most useful subtable in it.
    pub(crate) fn parse(data: &[u8], offset: usize) -> Result<Self, Error> {
        let mut reader = Reader::at(data, offset)?;
        let _version = reader.u16()?;
        let subtable_count = reader.u16()?;

        // Ranked by how much of Unicode each encoding can express.
        let mut best: Option<(u8, usize, bool)> = None;

        for _ in 0..subtable_count {
            let platform = reader.u16()?;
            let encoding = reader.u16()?;
            let subtable_offset = offset + reader.u32()? as usize;

            let (rank, symbol) = match (platform, encoding) {
                // Windows, full Unicode.
                (3, 10) => (5, false),
                // Unicode platform, beyond the Basic Multilingual Plane.
                (0, 4 | 6) => (4, false),
                // Windows, Basic Multilingual Plane only.
                (3, 1) => (3, false),
                (0, _) => (2, false),
                // A symbol font, usable but only as a last resort.
                (3, 0) => (1, true),
                _ => continue,
            };

            if best.is_none_or(|(best_rank, _, _)| rank > best_rank) {
                best = Some((rank, subtable_offset, symbol));
            }
        }

        let Some((_, subtable_offset, symbol_encoded)) = best else {
            return Ok(Self::empty());
        };

        let mut segments = parse_subtable(data, subtable_offset)?;
        segments.sort_by_key(|segment| segment.start);

        Ok(Self { segments, symbol_encoded })
    }
}

fn parse_subtable(data: &[u8], offset: usize) -> Result<Vec<Segment>, Error> {
    let mut reader = Reader::at(data, offset)?;
    match reader.u16()? {
        0 => parse_format0(data, offset),
        4 => parse_format4(data, offset),
        6 => parse_format6(data, offset),
        12 => parse_format12(data, offset),
        // A format nobody uses any more. Better an empty mapping, and a visible
        // fallback to another font, than a wrong one.
        _ => Ok(Vec::new()),
    }
}

/// Format 0: a flat table of 256 bytes, one glyph per character code.
fn parse_format0(data: &[u8], offset: usize) -> Result<Vec<Segment>, Error> {
    let mut reader = Reader::at(data, offset + 6)?;
    let indices: Vec<u16> =
        (0..256).map(|_| reader.u8().map(u16::from)).collect::<Result<_, _>>()?;

    Ok(vec![Segment { start: 0, end: 255, mapping: Mapping::Indices(indices) }])
}

/// Format 4: segmented coverage of the Basic Multilingual Plane. The commonest
/// format by far, and the fiddliest.
fn parse_format4(data: &[u8], offset: usize) -> Result<Vec<Segment>, Error> {
    let mut reader = Reader::at(data, offset + 6)?;
    let segment_count = usize::from(reader.u16()?) / 2;
    if segment_count == 0 {
        return Ok(Vec::new());
    }
    reader.skip(6)?; // searchRange, entrySelector, rangeShift

    let end_codes_at = reader.position();
    let start_codes_at = end_codes_at + segment_count * 2 + 2; // + reservedPad
    let deltas_at = start_codes_at + segment_count * 2;
    let range_offsets_at = deltas_at + segment_count * 2;

    let mut segments = Vec::with_capacity(segment_count);

    for index in 0..segment_count {
        let end = u32::from(Reader::at(data, end_codes_at + index * 2)?.u16()?);
        let start = u32::from(Reader::at(data, start_codes_at + index * 2)?.u16()?);
        let delta = Reader::at(data, deltas_at + index * 2)?.i16()?;
        let range_offset_field = range_offsets_at + index * 2;
        let range_offset = Reader::at(data, range_offset_field)?.u16()?;

        // The final segment is a sentinel covering 0xFFFF and maps nothing.
        if start > end {
            continue;
        }

        if range_offset == 0 {
            segments.push(Segment { start, end, mapping: Mapping::Delta(i32::from(delta)) });
            continue;
        }

        // The offset is measured from the position of the field itself, which is
        // what makes this format awkward to read.
        let mut indices = Vec::with_capacity((end - start + 1) as usize);
        for code in start..=end {
            let address =
                range_offset_field + usize::from(range_offset) + (code - start) as usize * 2;
            let glyph = match Reader::at(data, address) {
                Ok(mut entry) => entry.u16().unwrap_or(0),
                Err(_) => 0,
            };
            // Zero means no glyph, and the delta must not be applied to it.
            let mapped = if glyph == 0 {
                0
            } else {
                ((i32::from(glyph) + i32::from(delta)) & 0xFFFF) as u16
            };
            indices.push(mapped);
        }
        segments.push(Segment { start, end, mapping: Mapping::Indices(indices) });
    }

    Ok(segments)
}

/// Format 6: a single contiguous run of character codes.
fn parse_format6(data: &[u8], offset: usize) -> Result<Vec<Segment>, Error> {
    let mut reader = Reader::at(data, offset + 6)?;
    let first = u32::from(reader.u16()?);
    let count = usize::from(reader.u16()?);
    if count == 0 {
        return Ok(Vec::new());
    }

    let indices: Vec<u16> = (0..count).map(|_| reader.u16()).collect::<Result<_, _>>()?;

    Ok(vec![Segment {
        start: first,
        end: first + count as u32 - 1,
        mapping: Mapping::Indices(indices),
    }])
}

/// Format 12: grouped coverage of the whole of Unicode. What a font needs for
/// emoji and for the rarer CJK planes.
fn parse_format12(data: &[u8], offset: usize) -> Result<Vec<Segment>, Error> {
    let mut reader = Reader::at(data, offset + 12)?;
    let group_count = reader.u32()? as usize;

    // Each group is 12 bytes; a count that could not fit is malformed.
    if group_count.saturating_mul(12) > reader.remaining() {
        return Err(Error::MalformedTable("cmap"));
    }

    let mut segments = Vec::with_capacity(group_count);
    for _ in 0..group_count {
        let start = reader.u32()?;
        let end = reader.u32()?;
        let start_glyph = reader.u32()?;
        if start > end {
            continue;
        }
        segments.push(Segment {
            start,
            end,
            mapping: Mapping::Delta(start_glyph as i32 - start as i32),
        });
    }

    Ok(segments)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a `cmap` with one format 4 subtable covering "A".."Z".
    fn format4_font() -> Vec<u8> {
        let mut out = Vec::new();
        // cmap header: version, one subtable, platform 3 encoding 1 at offset 12.
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&1u16.to_be_bytes());
        out.extend_from_slice(&3u16.to_be_bytes());
        out.extend_from_slice(&1u16.to_be_bytes());
        out.extend_from_slice(&12u32.to_be_bytes());

        // Two segments: "A".."Z" mapped by a delta, and the 0xFFFF sentinel.
        let segment_count: u16 = 2;
        out.extend_from_slice(&4u16.to_be_bytes()); // format
        out.extend_from_slice(&32u16.to_be_bytes()); // length
        out.extend_from_slice(&0u16.to_be_bytes()); // language
        out.extend_from_slice(&(segment_count * 2).to_be_bytes());
        out.extend_from_slice(&4u16.to_be_bytes()); // searchRange
        out.extend_from_slice(&1u16.to_be_bytes()); // entrySelector
        out.extend_from_slice(&0u16.to_be_bytes()); // rangeShift
        out.extend_from_slice(&0x005Au16.to_be_bytes()); // endCode "Z"
        out.extend_from_slice(&0xFFFFu16.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes()); // reservedPad
        out.extend_from_slice(&0x0041u16.to_be_bytes()); // startCode "A"
        out.extend_from_slice(&0xFFFFu16.to_be_bytes());
        // Glyph 1 for "A": 0x41 + delta = 1, so delta = 1 - 0x41.
        out.extend_from_slice(&((1i32 - 0x41) as i16).to_be_bytes());
        out.extend_from_slice(&1u16.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes()); // idRangeOffset
        out.extend_from_slice(&0u16.to_be_bytes());

        out
    }

    #[test]
    fn reads_a_format4_subtable() {
        let data = format4_font();
        let map = CharacterMap::parse(&data, 0).unwrap();

        assert_eq!(map.glyph_for('A'), Some(GlyphId(1)));
        assert_eq!(map.glyph_for('B'), Some(GlyphId(2)));
        assert_eq!(map.glyph_for('Z'), Some(GlyphId(26)));
        // Outside every segment.
        assert_eq!(map.glyph_for('a'), None);
        assert_eq!(map.glyph_for('0'), None);
    }

    #[test]
    fn an_empty_map_is_usable() {
        let map = CharacterMap::empty();
        assert_eq!(map.glyph_for('A'), None);
        assert_eq!(map.segment_count(), 0);
    }

    #[test]
    fn a_truncated_table_is_an_error_not_a_panic() {
        let data = format4_font();
        for cut in 0..data.len() {
            // Any outcome is fine as long as it is not a panic.
            let _ = CharacterMap::parse(&data[..cut], 0);
        }
    }

    #[test]
    fn format12_covers_beyond_the_basic_plane() {
        let mut out = Vec::new();
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&1u16.to_be_bytes());
        out.extend_from_slice(&3u16.to_be_bytes());
        out.extend_from_slice(&10u16.to_be_bytes());
        out.extend_from_slice(&12u32.to_be_bytes());

        out.extend_from_slice(&12u16.to_be_bytes()); // format
        out.extend_from_slice(&0u16.to_be_bytes()); // reserved
        out.extend_from_slice(&28u32.to_be_bytes()); // length
        out.extend_from_slice(&0u32.to_be_bytes()); // language
        out.extend_from_slice(&1u32.to_be_bytes()); // one group
        out.extend_from_slice(&0x1F600u32.to_be_bytes());
        out.extend_from_slice(&0x1F60Fu32.to_be_bytes());
        out.extend_from_slice(&100u32.to_be_bytes());

        let map = CharacterMap::parse(&out, 0).unwrap();
        assert_eq!(map.glyph_for('\u{1F600}'), Some(GlyphId(100)));
        assert_eq!(map.glyph_for('\u{1F60F}'), Some(GlyphId(115)));
        assert_eq!(map.glyph_for('\u{1F610}'), None);
    }
}
