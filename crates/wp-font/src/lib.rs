//! Reading TrueType and OpenType font files.
//!
//! A font file is a directory of tables. This crate reads the ones needed to put
//! text on a page: how big the design grid is, which glyph a character maps to,
//! how wide each glyph is, and the outline to fill.
//!
//! No font is shipped with this program. Typefaces are data with their own
//! licences — the ones Word uses belong to Microsoft — so the editor reads
//! whatever is installed on the system, which is what Word does too.
//!
//! # Scope
//!
//! Glyph outlines are read from both tables a font may keep them in: `glyf`,
//! which is what Arial, Times New Roman, Calibri, Cambria, Segoe UI and the
//! rest of the fonts a Windows machine ships with all use, and `CFF`, which is
//! what every font Adobe made uses, and most fonts a designer buys, and every
//! `.otf` file whose signature reads `OTTO`. Either comes back as the same
//! drawing commands, so nothing downstream has to know which kind of font it
//! was handed.
//!
//! # Safety of input
//!
//! A font file comes from outside the program and its internal offsets point
//! wherever they like. Every read is bounds-checked, and malformed input is
//! reported rather than causing a panic.

#![forbid(unsafe_code)]

mod cff;
mod cmap;
mod colour;
mod glyf;
mod name;
mod read;
mod vary;

pub use cmap::CharacterMap;
pub use colour::{Bitmap, Layer, Rgba};
pub use glyf::{Outline, PathCommand, Point};
pub use vary::{Axis, Instance};

use read::Reader;

/// A glyph's index within a font. Not a character: the mapping between the two
/// is what [`CharacterMap`] is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GlyphId(pub u16);

/// A rectangle in font design units.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Bounds {
    pub min_x: i16,
    pub min_y: i16,
    pub max_x: i16,
    pub max_y: i16,
}

/// Why a font could not be read.
///
/// English by design: developer diagnostics. Text shown to the user comes from
/// the localized presentation layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// A read went past the end of the data, or an offset pointed outside it.
    OutOfBounds,
    /// The file does not begin with a recognized font signature.
    NotAFont,
    /// A table the font cannot work without is missing.
    MissingTable(&'static str),
    /// A table is present but its contents make no sense.
    MalformedTable(&'static str),
    /// The font stores its outlines in a format this crate does not read.
    UnsupportedOutlineFormat,
    /// A collection was asked for a font index it does not contain.
    NoSuchFontInCollection,
    /// A composite glyph refers to itself, directly or through a ring.
    RecursiveGlyph,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::OutOfBounds => f.write_str("a read went outside the font data"),
            Self::NotAFont => f.write_str("not a recognized font file"),
            Self::MissingTable(tag) => write!(f, "the font has no {tag} table"),
            Self::MalformedTable(tag) => write!(f, "the {tag} table is malformed"),
            Self::UnsupportedOutlineFormat => {
                f.write_str("the font stores outlines in a format that is not supported")
            }
            Self::NoSuchFontInCollection => f.write_str("no font at that index in the collection"),
            Self::RecursiveGlyph => f.write_str("a composite glyph refers to itself"),
        }
    }
}

impl std::error::Error for Error {}

/// Where a table sits in the file.
#[derive(Clone, Copy, Debug)]
pub struct TableRange {
    pub offset: usize,
    pub length: usize,
}

/// Reads a `cmap` table on its own, without the rest of the font.
///
/// Choosing a font for a character means asking many faces whether they cover
/// it. Loading each whole file to ask is what turned showing one page into
/// hundreds of megabytes; the mapping alone is a few kilobytes.
pub fn character_map_from_table(table: &[u8]) -> Result<CharacterMap, Error> {
    CharacterMap::parse(table, 0)
}

/// Reads the axes and named instances of a variable font from its own tables.
///
/// The catalogue reads a few tables of each font file rather than the whole of
/// it, and a variable font is several faces in one file: this is how it finds
/// out which, without loading it.
#[must_use]
pub fn variations_from_tables(fvar: &[u8], name_table: &[u8]) -> (Vec<Axis>, Vec<Instance>) {
    let names = |id: u16| name::find(name_table, 0, id);
    let read = vary::Axes::parse(fvar, &names).unwrap_or_default();
    (read.axes, read.instances)
}

/// Reads the family name out of a `name` table on its own.
///
/// Cataloguing the fonts on a machine means reading a few hundred files for one
/// string each. Loading each one whole to get it costs hundreds of megabytes,
/// so the catalogue reads just this table and asks for the name directly.
#[must_use]
pub fn family_from_name_table(table: &[u8]) -> Option<String> {
    name::find(table, 0, name::FAMILY)
}

/// Reads the table directory of a font, given the file's first bytes.
///
/// `header` must hold at least the twelve-byte header and the directory that
/// follows it. Returns each table's tag and where it sits in the file, so a
/// caller can read only the ones it needs.
pub fn table_directory(header: &[u8]) -> Result<Vec<([u8; 4], TableRange)>, Error> {
    let mut reader = Reader::new(header);
    let version = reader.tag()?;
    if !matches!(
        version,
        [0x00, 0x01, 0x00, 0x00] | [b't', b'r', b'u', b'e'] | [b'O', b'T', b'T', b'O']
    ) {
        return Err(Error::NotAFont);
    }

    let count = reader.u16()?;
    reader.skip(6)?;

    let mut tables = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
        let tag = reader.tag()?;
        reader.skip(4)?;
        let offset = reader.u32()? as usize;
        let length = reader.u32()? as usize;
        tables.push((tag, TableRange { offset, length }));
    }

    Ok(tables)
}

/// Where each font in a collection begins, given the file's first bytes.
///
/// A file that is not a collection has one font, starting at zero.
pub fn collection_offsets(header: &[u8]) -> Result<Vec<usize>, Error> {
    let mut reader = Reader::new(header);
    if reader.tag()? != COLLECTION_TAG {
        return Ok(vec![0]);
    }

    reader.skip(4)?;
    let count = reader.u32()?.min(64);
    let mut offsets = Vec::with_capacity(count as usize);
    for _ in 0..count {
        offsets.push(reader.u32()? as usize);
    }
    Ok(offsets)
}

/// Vertical metrics, in font design units.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VerticalMetrics {
    /// How far above the baseline the tallest letters reach.
    pub ascender: i16,
    /// How far below it the descenders go. Negative.
    pub descender: i16,
    /// Extra space the designer asks for between lines.
    pub line_gap: i16,
}

impl VerticalMetrics {
    /// The natural distance between one baseline and the next.
    #[must_use]
    pub fn line_height(&self) -> i32 {
        i32::from(self.ascender) - i32::from(self.descender) + i32::from(self.line_gap)
    }
}

/// A parsed font file.
#[derive(Clone, Debug)]
pub struct Font<'a> {
    data: &'a [u8],
    /// Size of the design grid. Every measurement in the font is in these units.
    units_per_em: u16,
    /// Whether `loca` holds 16-bit or 32-bit offsets.
    long_loca: bool,
    vertical: VerticalMetrics,
    glyph_count: u16,
    number_of_h_metrics: u16,
    hmtx: Option<TableRange>,
    loca: Option<TableRange>,
    glyf: Option<TableRange>,
    /// PostScript outlines, and whether they are the newer version of the
    /// table. A font has one kind of outline or the other, never both.
    cff: Option<(TableRange, bool)>,
    /// The tables of a variable font: which axes it has, how they bend, and
    /// how the outlines and the widths move along them.
    fvar: Option<TableRange>,
    avar: Option<TableRange>,
    gvar: Option<TableRange>,
    hvar: Option<TableRange>,
    /// The tables of a font that draws in colour: the layers a glyph is made
    /// of and the palettes they are drawn in, or the pictures themselves.
    colr: Option<TableRange>,
    cpal: Option<TableRange>,
    cblc: Option<TableRange>,
    cbdt: Option<TableRange>,
    /// Where this font has been set on its axes, already turned into the -1 to
    /// 1 the deltas are written against. `None` means where it was drawn.
    variations: Option<Vec<f32>>,
    kern: Option<TableRange>,
    /// Glyph substitution and positioning, which shaping needs.
    gsub: Option<TableRange>,
    gpos: Option<TableRange>,
    gdef: Option<TableRange>,
    name: Option<TableRange>,
    os2: Option<TableRange>,
    character_map: CharacterMap,
}

/// Signature of a TrueType collection, which packs several fonts into one file.
const COLLECTION_TAG: [u8; 4] = *b"ttcf";

impl<'a> Font<'a> {
    /// Parses a font file. For a collection, the first font is used.
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        Self::parse_index(data, 0)
    }

    /// Parses one font out of a file, by index within a collection.
    pub fn parse_index(data: &'a [u8], index: u32) -> Result<Self, Error> {
        let mut reader = Reader::new(data);
        let signature = reader.tag()?;

        let directory_offset = if signature == COLLECTION_TAG {
            reader.skip(4)?; // version
            let count = reader.u32()?;
            if index >= count {
                return Err(Error::NoSuchFontInCollection);
            }
            reader.skip(index as usize * 4)?;
            reader.u32()? as usize
        } else {
            0
        };

        Self::parse_directory(data, directory_offset)
    }

    /// How many fonts a file contains. One, unless it is a collection.
    pub fn count(data: &[u8]) -> Result<u32, Error> {
        let mut reader = Reader::new(data);
        if reader.tag()? != COLLECTION_TAG {
            return Ok(1);
        }
        reader.skip(4)?;
        reader.u32()
    }

    fn parse_directory(data: &'a [u8], directory_offset: usize) -> Result<Self, Error> {
        let mut reader = Reader::at(data, directory_offset)?;
        let version = reader.tag()?;

        // 0x00010000 is TrueType, "true" is the old Apple form, "OTTO" means the
        // outlines are PostScript rather than quadratic. Which kind they are is
        // read off the tables rather than off this, because that is what says
        // where they are.
        if !matches!(
            version,
            [0x00, 0x01, 0x00, 0x00] | [b't', b'r', b'u', b'e'] | [b'O', b'T', b'T', b'O']
        ) {
            return Err(Error::NotAFont);
        }

        let table_count = reader.u16()?;
        reader.skip(6)?; // searchRange, entrySelector, rangeShift

        let mut head = None;
        let mut hhea = None;
        let mut maxp = None;
        let mut hmtx = None;
        let mut loca = None;
        let mut glyf = None;
        let mut cff = None;
        let mut fvar = None;
        let mut avar = None;
        let mut gvar = None;
        let mut hvar = None;
        let mut colr = None;
        let mut cpal = None;
        let mut cblc = None;
        let mut cbdt = None;
        let mut cmap = None;
        let mut kern = None;
        let mut gsub = None;
        let mut gpos = None;
        let mut gdef = None;
        let mut name = None;
        let mut os2 = None;

        for _ in 0..table_count {
            let tag = reader.tag()?;
            reader.skip(4)?; // checksum
            let offset = reader.u32()? as usize;
            let length = reader.u32()? as usize;

            // A table that claims to reach past the end of the file is ignored
            // rather than trusted; the font may still be usable without it.
            if offset > data.len() || offset.saturating_add(length) > data.len() {
                continue;
            }
            let range = TableRange { offset, length };

            match &tag {
                b"head" => head = Some(range),
                b"hhea" => hhea = Some(range),
                b"maxp" => maxp = Some(range),
                b"hmtx" => hmtx = Some(range),
                b"loca" => loca = Some(range),
                b"glyf" => glyf = Some(range),
                b"CFF " | b"CFF2" => cff = Some((range, tag == *b"CFF2")),
                b"fvar" => fvar = Some(range),
                b"avar" => avar = Some(range),
                b"gvar" => gvar = Some(range),
                b"HVAR" => hvar = Some(range),
                b"COLR" => colr = Some(range),
                b"CPAL" => cpal = Some(range),
                b"CBLC" => cblc = Some(range),
                b"CBDT" => cbdt = Some(range),
                b"cmap" => cmap = Some(range),
                b"kern" => kern = Some(range),
                b"GSUB" => gsub = Some(range),
                b"GPOS" => gpos = Some(range),
                b"GDEF" => gdef = Some(range),
                b"name" => name = Some(range),
                b"OS/2" => os2 = Some(range),
                _ => {}
            }
        }

        let head = head.ok_or(Error::MissingTable("head"))?;
        let hhea = hhea.ok_or(Error::MissingTable("hhea"))?;
        let maxp = maxp.ok_or(Error::MissingTable("maxp"))?;

        let mut head_reader = Reader::at(data, head.offset)?;
        head_reader.skip(18)?;
        let units_per_em = head_reader.u16()?;
        if units_per_em == 0 {
            return Err(Error::MalformedTable("head"));
        }
        head_reader.seek(head.offset + 50)?;
        let long_loca = head_reader.i16()? == 1;

        let mut hhea_reader = Reader::at(data, hhea.offset + 4)?;
        let vertical = VerticalMetrics {
            ascender: hhea_reader.i16()?,
            descender: hhea_reader.i16()?,
            line_gap: hhea_reader.i16()?,
        };
        hhea_reader.seek(hhea.offset + 34)?;
        let number_of_h_metrics = hhea_reader.u16()?;

        let mut maxp_reader = Reader::at(data, maxp.offset + 4)?;
        let glyph_count = maxp_reader.u16()?;

        let character_map = match cmap {
            Some(range) => CharacterMap::parse(data, range.offset)?,
            None => CharacterMap::empty(),
        };

        Ok(Self {
            data,
            units_per_em,
            long_loca,
            vertical,
            glyph_count,
            number_of_h_metrics,
            hmtx,
            loca,
            glyf,
            cff,
            fvar,
            avar,
            gvar,
            hvar,
            colr,
            cpal,
            cblc,
            cbdt,
            variations: None,
            kern,
            gsub,
            gpos,
            gdef,
            name,
            os2,
            character_map,
        })
    }

    /// Size of the design grid. Every measurement in the font is in these units,
    /// so a value is scaled to a point size by multiplying by
    /// `size / units_per_em`.
    #[must_use]
    pub fn units_per_em(&self) -> u16 {
        self.units_per_em
    }

    /// Converts a font unit measurement to pixels at a given size.
    #[must_use]
    pub fn scale(&self, value: f32, size_pixels: f32) -> f32 {
        value * size_pixels / f32::from(self.units_per_em)
    }

    #[must_use]
    pub fn vertical_metrics(&self) -> VerticalMetrics {
        self.vertical
    }

    #[must_use]
    pub fn glyph_count(&self) -> u16 {
        self.glyph_count
    }

    /// The character-to-glyph mapping.
    #[must_use]
    pub fn character_map(&self) -> &CharacterMap {
        &self.character_map
    }

    /// The glyph a character maps to, if the font has one.
    #[must_use]
    pub fn glyph_for(&self, character: char) -> Option<GlyphId> {
        self.character_map.glyph_for(character)
    }

    /// How far the pen moves after drawing a glyph, in font units.
    ///
    /// A variable font set away from where it was drawn moves this too: a
    /// heavier letter is a wider one, and text measured without that is text
    /// set at the wrong width.
    #[must_use]
    pub fn advance(&self, glyph: GlyphId) -> u16 {
        let width = f32::from(self.advance_checked(glyph).unwrap_or(0));
        (width + self.advance_delta(glyph)).max(0.0).round() as u16
    }

    /// How much wider or narrower this glyph is at this font's setting.
    fn advance_delta(&self, glyph: GlyphId) -> f32 {
        let Some(coordinates) = self.variations.as_ref() else { return 0.0 };
        let Some(table) = self.raw_table(self.hvar) else { return 0.0 };
        vary::Advances::parse(table).map_or(0.0, |advances| advances.delta(glyph, coordinates))
    }

    fn advance_checked(&self, glyph: GlyphId) -> Option<u16> {
        let hmtx = self.hmtx?;
        if self.number_of_h_metrics == 0 {
            return None;
        }

        // Only the first `number_of_h_metrics` glyphs have their own advance.
        // Every glyph after that repeats the last one, which is how a font with
        // many equal-width glyphs stays small.
        let index = glyph.0.min(self.number_of_h_metrics - 1);
        let offset = hmtx.offset + usize::from(index) * 4;
        Reader::at(self.data, offset).ok()?.u16().ok()
    }

    /// The axes this font can be set along, if it is a variable one.
    ///
    /// Empty for an ordinary font, which is one typeface and not a family of
    /// them.
    #[must_use]
    pub fn axes(&self) -> Vec<vary::Axis> {
        self.axes_and_instances().axes
    }

    /// The places on those axes the designer gave names to, which is what a
    /// font menu lists: "Thin", "SemiBold", "Condensed Black".
    #[must_use]
    pub fn instances(&self) -> Vec<vary::Instance> {
        self.axes_and_instances().instances
    }

    /// Whether the file is a family rather than a typeface.
    #[must_use]
    pub fn is_variable(&self) -> bool {
        self.fvar.is_some()
    }

    /// Whether this font has been set anywhere but where it was drawn.
    #[must_use]
    pub fn is_varied(&self) -> bool {
        self.variations.is_some()
    }

    /// The same font set to a place on its axes, given in the axes' own units
    /// and in their own order — which is the order [`Font::axes`] gives them
    /// and the order an instance's coordinates are in.
    ///
    /// An ordinary font comes back unchanged: there is nowhere to set it to.
    #[must_use]
    pub fn varied(&self, coordinates: &[f32]) -> Self {
        let axes = self.axes_and_instances();
        if axes.axes.is_empty() {
            return self.clone();
        }
        let normalized = axes.normalize(coordinates, self.raw_table(self.avar));
        // Where every axis is left at its default the font is the font, and
        // saying otherwise would cost every glyph a search through the deltas.
        let moved = normalized.iter().any(|value| *value != 0.0);
        Self { variations: moved.then_some(normalized), ..self.clone() }
    }

    /// The axes and instances, read afresh. Both are asked for once per font
    /// rather than per glyph, so the table is not kept parsed.
    fn axes_and_instances(&self) -> vary::Axes {
        let Some(table) = self.raw_table(self.fvar) else { return vary::Axes::default() };
        let names = |id: u16| self.name(id);
        vary::Axes::parse(table, &names).unwrap_or_default()
    }

    /// How far each piece of a composite glyph moves at this font's setting.
    ///
    /// A composite is drawn from other glyphs, and its own deltas move where
    /// each of them goes rather than any point inside them: as a letter grows
    /// heavier its accent moves up to clear it.
    pub(crate) fn component_deltas(&self, glyph: GlyphId, offsets: &[Point]) -> Option<Vec<Point>> {
        let coordinates = self.variations.as_ref()?;
        let table = self.raw_table(self.gvar)?;
        // No contours, and so nothing worked out for the pieces left alone:
        // that is what the format says for a composite.
        vary::Outlines::parse(table).ok()?.deltas(glyph, coordinates, offsets, &[])
    }

    /// How far each point of a glyph moves at this font's setting of the axes.
    ///
    /// `None` where nothing moves, which is every glyph of an ordinary font and
    /// most glyphs of a variable one at its default.
    pub(crate) fn point_deltas(
        &self,
        glyph: GlyphId,
        points: &[Point],
        ends: &[usize],
    ) -> Option<Vec<Point>> {
        let coordinates = self.variations.as_ref()?;
        let table = self.raw_table(self.gvar)?;
        vary::Outlines::parse(table).ok()?.deltas(glyph, coordinates, points, ends)
    }

    /// The layers a glyph is drawn from, where it is drawn from several.
    ///
    /// A colour emoji in a layered font is a dozen ordinary glyphs, each in its
    /// own colour out of the font's palette; a layer with no colour is drawn in
    /// whatever colour the text is. `None` for every glyph of an ordinary font
    /// and for the letters of a colour one.
    #[must_use]
    pub fn colour_layers(&self, glyph: GlyphId) -> Option<Vec<Layer>> {
        let colr = self.raw_table(self.colr)?;
        colour::layers_of(colr, self.raw_table(self.cpal), glyph)
    }

    /// The picture a glyph is kept as, in a font that keeps pictures.
    ///
    /// `pixels_per_em` is the size the text is being drawn at; the nearest
    /// size the font holds is given back, and it is the caller's to scale.
    #[must_use]
    pub fn bitmap(&self, glyph: GlyphId, pixels_per_em: u16) -> Option<Bitmap<'a>> {
        let locations = self.raw_table(self.cblc)?;
        let data = self.raw_table(self.cbdt)?;
        colour::bitmap_of(locations, data, glyph, pixels_per_em)
    }

    /// Whether this font draws in colours of its own rather than in the text's.
    #[must_use]
    pub fn has_colour(&self) -> bool {
        self.colr.is_some() || (self.cblc.is_some() && self.cbdt.is_some())
    }

    /// The outline of a glyph, in font units.
    ///
    /// The same commands whichever kind of outline the font keeps: a caller
    /// that draws them does not have to know, and a font of either kind draws.
    ///
    /// Returns `Ok(None)` for a glyph with no outline, such as a space.
    pub fn outline(&self, glyph: GlyphId) -> Result<Option<Outline>, Error> {
        if let Some((range, second)) = self.cff {
            let data = self
                .data
                .get(range.offset..range.offset + range.length)
                .ok_or(Error::OutOfBounds)?;
            let table = if second { cff::Cff::parse2(data)? } else { cff::Cff::parse(data)? };
            return table.outline(glyph, self.variations.as_deref().unwrap_or(&[]));
        }
        if self.glyf.is_none() || self.loca.is_none() {
            return Err(Error::UnsupportedOutlineFormat);
        }
        glyf::outline(self, glyph)
    }

    /// The bytes of one table, for a program that has to *write* a font rather
    /// than draw with one.
    ///
    /// Embedding a font in a PDF means building a smaller font out of the
    /// original's tables, and that needs them as they are.
    #[must_use]
    pub fn table(&self, tag: &[u8; 4]) -> Option<&'a [u8]> {
        let ranges = table_directory(self.data).ok()?;
        let (_, range) = ranges.iter().find(|(found, _)| found == tag)?;
        self.data.get(range.offset..range.offset + range.length)
    }

    /// The outline of one glyph exactly as it sits in `glyf`.
    ///
    /// Empty for a glyph with no outline, such as a space, which is what the
    /// format itself says by giving it no bytes.
    #[must_use]
    pub fn glyph_data(&self, glyph: GlyphId) -> &'a [u8] {
        let Ok(Some((start, end))) = self.glyph_range(glyph) else {
            return &[];
        };
        self.data.get(start..end).unwrap_or(&[])
    }

    /// The glyphs a composite glyph is built out of.
    ///
    /// A font that draws `é` as an `e` with an accent on it needs all three
    /// when it is cut down, or the letter comes out as an empty box.
    #[must_use]
    pub fn components(&self, glyph: GlyphId) -> Vec<GlyphId> {
        glyf::components(self, glyph)
    }

    /// Whether `loca` holds its offsets as words or as longs.
    #[must_use]
    pub fn has_long_loca(&self) -> bool {
        self.long_loca
    }

    /// How many glyphs have their own advance width in `hmtx`; the rest share
    /// the last one.
    #[must_use]
    pub fn horizontal_metrics_count(&self) -> u16 {
        self.number_of_h_metrics
    }

    /// The kerning adjustment between two glyphs, in font units.
    ///
    /// Only the old `kern` table is read. Modern fonts put kerning in `GPOS`
    /// instead, which needs the shaping engine and comes with it.
    #[must_use]
    pub fn kerning(&self, left: GlyphId, right: GlyphId) -> i16 {
        self.kerning_checked(left, right).unwrap_or(0)
    }

    fn kerning_checked(&self, left: GlyphId, right: GlyphId) -> Option<i16> {
        let kern = self.kern?;
        let mut reader = Reader::at(self.data, kern.offset).ok()?;

        let _version = reader.u16().ok()?;
        let subtable_count = reader.u16().ok()?;
        let wanted = (u32::from(left.0) << 16) | u32::from(right.0);

        for _ in 0..subtable_count {
            let start = reader.position();
            let _subtable_version = reader.u16().ok()?;
            let length = reader.u16().ok()? as usize;
            let coverage = reader.u16().ok()?;

            // Only horizontal format 0 subtables are read; the rest are rare and
            // skipping one loses kerning, not correctness.
            if coverage & 0x0F == 0 && coverage & 0x0001 != 0 {
                let pair_count = reader.u16().ok()?;
                reader.skip(6).ok()?; // searchRange, entrySelector, rangeShift

                // The pairs are sorted, so a binary search finds one directly.
                let pairs_start = reader.position();
                let mut low = 0usize;
                let mut high = usize::from(pair_count);
                while low < high {
                    let middle = (low + high) / 2;
                    let mut entry = Reader::at(self.data, pairs_start + middle * 6).ok()?;
                    let key = entry.u32().ok()?;
                    if key < wanted {
                        low = middle + 1;
                    } else if key > wanted {
                        high = middle;
                    } else {
                        return entry.i16().ok();
                    }
                }
            }

            reader.seek(start + length.max(6)).ok()?;
        }

        None
    }

    /// One string of the `name` table, by the number a table refers to it by.
    #[must_use]
    pub fn name(&self, id: u16) -> Option<String> {
        let range = self.name?;
        name::find(self.data, range.offset, id)
    }

    /// The family name, as the font declares it.
    #[must_use]
    pub fn family_name(&self) -> Option<String> {
        let range = self.name?;
        name::find(self.data, range.offset, name::FAMILY)
    }

    /// The full name, which usually includes the style.
    #[must_use]
    pub fn full_name(&self) -> Option<String> {
        let range = self.name?;
        name::find(self.data, range.offset, name::FULL)
    }

    /// Whether the font declares itself bold.
    #[must_use]
    pub fn is_bold(&self) -> bool {
        self.selection_flag(5).unwrap_or(false)
    }

    /// Whether the font declares itself italic.
    #[must_use]
    pub fn is_italic(&self) -> bool {
        self.selection_flag(0).unwrap_or(false)
    }

    /// Reads one bit of the `OS/2` selection flags.
    fn selection_flag(&self, bit: u16) -> Option<bool> {
        let os2 = self.os2?;
        let mut reader = Reader::at(self.data, os2.offset + 62).ok()?;
        let selection = reader.u16().ok()?;
        Some(selection & (1 << bit) != 0)
    }

    /// Weight on the usual 100..900 scale, where 400 is regular and 700 bold.
    #[must_use]
    pub fn weight(&self) -> u16 {
        self.weight_checked().unwrap_or(400)
    }

    fn weight_checked(&self) -> Option<u16> {
        let os2 = self.os2?;
        Reader::at(self.data, os2.offset + 4).ok()?.u16().ok()
    }

    /// The glyph substitution table, if the font has one.
    ///
    /// Substitution is what turns a sequence of characters into the glyphs that
    /// actually draw it: an Arabic letter into the form it takes beside its
    /// neighbours, a pair of letters into a ligature.
    #[must_use]
    pub fn substitution_table(&self) -> Option<&'a [u8]> {
        self.raw_table(self.gsub)
    }

    /// The glyph positioning table, if the font has one.
    #[must_use]
    pub fn positioning_table(&self) -> Option<&'a [u8]> {
        self.raw_table(self.gpos)
    }

    /// What the font says about its own glyphs, if it says anything.
    ///
    /// Which of them are letters and which are marks that sit on letters. The
    /// positioning rules cannot be followed without it: where an accent
    /// belongs is decided by finding the letter before it, and finding that
    /// means knowing what is a letter.
    #[must_use]
    pub fn definitions_table(&self) -> Option<&'a [u8]> {
        self.raw_table(self.gdef)
    }

    fn raw_table(&self, range: Option<TableRange>) -> Option<&'a [u8]> {
        let range = range?;
        self.data.get(range.offset..range.offset + range.length)
    }

    /// Whether outlines can be read from this font.
    #[must_use]
    pub fn has_outlines(&self) -> bool {
        self.cff.is_some() || (self.glyf.is_some() && self.loca.is_some())
    }

    /// Whether anything at all can be drawn with it.
    ///
    /// A font of colour bitmaps has no outlines and is still a font: every
    /// emoji on an Android machine is one.
    #[must_use]
    pub fn can_be_drawn_with(&self) -> bool {
        self.has_outlines() || self.has_colour()
    }

    /// Whether the outlines are PostScript ones, which matters to a program
    /// that has to hand the font on rather than draw with it: a PDF says which
    /// kind it is being given, and says it in a different key for each.
    #[must_use]
    pub fn has_postscript_outlines(&self) -> bool {
        self.cff.is_some()
    }

    // --- Used by the outline reader ---------------------------------------

    pub(crate) fn data(&self) -> &'a [u8] {
        self.data
    }

    /// Where a glyph's outline data starts and ends, from the `loca` table.
    pub(crate) fn glyph_range(&self, glyph: GlyphId) -> Result<Option<(usize, usize)>, Error> {
        let loca = self.loca.ok_or(Error::MissingTable("loca"))?;
        let glyf = self.glyf.ok_or(Error::MissingTable("glyf"))?;

        if glyph.0 >= self.glyph_count {
            return Ok(None);
        }

        let index = usize::from(glyph.0);
        let (start, end) = if self.long_loca {
            let mut reader = Reader::at(self.data, loca.offset + index * 4)?;
            let start = reader.u32()? as usize;
            let end = reader.u32()? as usize;
            (start, end)
        } else {
            // The short form stores halved offsets, which is why they double.
            let mut reader = Reader::at(self.data, loca.offset + index * 2)?;
            let start = usize::from(reader.u16()?) * 2;
            let end = usize::from(reader.u16()?) * 2;
            (start, end)
        };

        // Equal offsets mean the glyph has no outline at all, as a space does.
        if end <= start {
            return Ok(None);
        }
        if end > glyf.length {
            return Err(Error::MalformedTable("loca"));
        }

        Ok(Some((glyf.offset + start, glyf.offset + end)))
    }
}
