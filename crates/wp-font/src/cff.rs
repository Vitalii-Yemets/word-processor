//! Glyph outlines from a PostScript-flavoured font: `CFF` and `CFF2`.
//!
//! # Two ways of drawing the same letters
//!
//! A font file says which glyphs it has and how wide they are in tables that
//! are the same whatever the outlines look like. The outlines themselves come
//! in two kinds. TrueType keeps them in `glyf`, as quadratic curves over a
//! grid of points. PostScript keeps them in `CFF`, as cubic curves in a little
//! stack language — one program per glyph, run to produce the shape.
//!
//! Neither is a niche. Every font Adobe ever made is the second kind, so are
//! the system fonts of macOS, so are most of the fonts a designer buys, and so
//! is every `.otf` file whose signature reads `OTTO`. A word processor that
//! reads only the first kind draws nothing at all for a good many documents.
//!
//! # What a charstring is
//!
//! A program in a stack language of about forty operators. Numbers are pushed;
//! an operator takes what is on the stack and draws with it. The awkward part
//! is that the operators are relative — every one of them moves the pen by a
//! delta rather than to a place — and that several of them take any number of
//! arguments, alternating between horizontal and vertical as they go. So the
//! same operator draws one curve or five depending on how much was pushed
//! before it.
//!
//! Two more things make it not simply an interpreter: the width of the glyph
//! may be prefixed to the first operator's arguments, so the first operator
//! has to count what it was given and decide; and a charstring may call
//! subroutines, which are whole charstrings shared between glyphs and
//! numbered from the middle of their list outwards.
//!
//! # What is here
//!
//! Enough to draw: the INDEX and DICT structures the table is built from, the
//! private dictionaries a glyph's subroutines live in, both kinds of glyph
//! lookup — plain and CID-keyed, where the glyph decides which dictionary it
//! belongs to — and the charstring interpreter, including the flex operators
//! (a curve so flat the format has a special way of saying so) and `seac`, the
//! old way of writing an accented letter as two glyphs.
//!
//! `CFF2`, the version made for variable fonts, is the same language with the
//! header and the dictionaries rearranged. It is read here at its default
//! instance: the `blend` operator keeps the values it is given and drops the
//! deltas, which is what the font says before any axis is moved. Moving one is
//! a separate item.

use crate::glyf::{Outline, PathCommand, Point};
use crate::read::Reader;
use crate::{Bounds, Error, GlyphId};

/// How deep one charstring may call another.
const MAX_DEPTH: u8 = 10;

/// The most numbers a charstring may leave on the stack. The format says
/// forty-eight; more than that is a malformed font rather than a deep one.
const MAX_STACK: usize = 48;

/// A CFF table, read far enough to draw any glyph in it.
#[derive(Clone, Debug)]
pub(crate) struct Cff<'a> {
    charstrings: Index<'a>,
    global: Index<'a>,
    /// The subroutines and the dictionary a glyph belongs to. A plain font has
    /// one for all of them; a CID-keyed font has one per glyph, chosen by the
    /// `FDSelect` table.
    private: Vec<Private<'a>>,
    select: Option<FontSelect<'a>>,
    /// Which SID each glyph's name is, which only `seac` needs.
    charset: Option<Charset<'a>>,
    /// The scale a font applies to its own design grid, where it is not the
    /// usual one. Written as the two diagonal entries.
    matrix: Option<(f64, f64)>,
    /// The deltas an axis of a variable font spends, and the regions they
    /// belong to. A `blend` says how many values it is blending but not how
    /// many deltas follow each, so without this the stack cannot even be
    /// unwound.
    store: Option<crate::vary::Store<'a>>,
}

/// One private dictionary: the local subroutines it names.
#[derive(Clone, Copy, Debug, Default)]
struct Private<'a> {
    local: Option<Index<'a>>,
}

impl<'a> Cff<'a> {
    /// Reads a `CFF` table.
    pub(crate) fn parse(data: &'a [u8]) -> Result<Self, Error> {
        let mut reader = Reader::new(data);
        reader.skip(2)?; // major and minor version
        let header_size = reader.u8()? as usize;
        reader.skip(1)?; // the size of an offset in the (unused) absolute form

        // Name, Top DICT, String, Global Subrs, one after another.
        let names = Index::parse(data, header_size, false)?;
        let tops = Index::parse(data, names.end, false)?;
        let strings = Index::parse(data, tops.end, false)?;
        let global = Index::parse(data, strings.end, false)?;

        let top = Dict::parse(tops.get(0).ok_or(Error::MalformedTable("CFF"))?)?;
        let charstrings_at = top.offset(KEY_CHARSTRINGS)?;
        let charstrings = Index::parse(data, charstrings_at, false)?;

        // A CID-keyed font says so by carrying a registry and ordering, and
        // keeps one private dictionary per group of glyphs rather than one for
        // the font.
        let (private, select) = if top.get(KEY_ROS).is_some() {
            Self::cid_dictionaries(data, &top, charstrings.count)?
        } else {
            (vec![Self::private_at(data, &top)?], None)
        };

        let charset = top
            .get(KEY_CHARSET)
            .and_then(|values| values.first().copied())
            .filter(|at| *at > 2.0)
            .map(|at| Charset::parse(data, at as usize, charstrings.count))
            .transpose()?;

        Ok(Self {
            charstrings,
            global,
            private,
            select,
            charset,
            matrix: top.matrix(),
            store: None,
        })
    }

    /// Reads a `CFF2` table, which is the same language with a different
    /// preamble: no names, no strings, and the top dictionary's length written
    /// in the header rather than kept in an INDEX.
    pub(crate) fn parse2(data: &'a [u8]) -> Result<Self, Error> {
        let mut reader = Reader::new(data);
        reader.skip(2)?; // major and minor version
        let header_size = reader.u8()? as usize;
        let top_length = reader.u16()? as usize;

        let top_end = header_size.checked_add(top_length).ok_or(Error::OutOfBounds)?;
        let top = Dict::parse(data.get(header_size..top_end).ok_or(Error::OutOfBounds)?)?;
        let global = Index::parse(data, top_end, true)?;

        let charstrings = Index::parse(data, top.offset(KEY_CHARSTRINGS)?, true)?;
        let (private, select) = Self::cid_dictionaries(data, &top, charstrings.count)?;
        let store = match top.get(KEY_VSTORE).and_then(|values| values.first().copied()) {
            // A CFF2 writes the store with its own length in front of it.
            Some(at) if at > 0.0 => Some(crate::vary::Store::parse(data, at as usize + 2)?),
            _ => None,
        };

        Ok(Self {
            charstrings,
            global,
            private,
            select,
            charset: None,
            matrix: top.matrix(),
            store,
        })
    }

    /// The private dictionary a top or font dictionary points at, and the local
    /// subroutines inside it.
    fn private_at(data: &'a [u8], dict: &Dict) -> Result<Private<'a>, Error> {
        let Some(values) = dict.get(KEY_PRIVATE) else { return Ok(Private::default()) };
        let [size, at] = values[..] else { return Ok(Private::default()) };
        if size < 0.0 || at < 0.0 {
            return Ok(Private::default());
        }

        let (at, size) = (at as usize, size as usize);
        let end = at.checked_add(size).ok_or(Error::OutOfBounds)?;
        let private = Dict::parse(data.get(at..end).ok_or(Error::OutOfBounds)?)?;

        // The subroutine index is written relative to the dictionary that
        // names it, which is the one place in the table that is not absolute.
        let local = match private.get(KEY_SUBRS).and_then(|values| values.first().copied()) {
            Some(relative) if relative > 0.0 => {
                let wide = data.len() > 4 && data[0] == 2;
                Some(Index::parse(data, at + relative as usize, wide)?)
            }
            _ => None,
        };
        Ok(Private { local })
    }

    /// The dictionaries of a font where each glyph says which it belongs to.
    fn cid_dictionaries(
        data: &'a [u8],
        top: &Dict,
        glyphs: u32,
    ) -> Result<(Vec<Private<'a>>, Option<FontSelect<'a>>), Error> {
        let fonts = Index::parse(data, top.offset(KEY_FDARRAY)?, data.first() == Some(&2))?;
        let mut private = Vec::with_capacity(fonts.count as usize);
        for index in 0..fonts.count {
            let entry = fonts.get(index).ok_or(Error::MalformedTable("CFF"))?;
            private.push(Self::private_at(data, &Dict::parse(entry)?)?);
        }

        let select = match top.get(KEY_FDSELECT).and_then(|values| values.first().copied()) {
            Some(at) if at > 0.0 => Some(FontSelect::parse(data, at as usize, glyphs)?),
            // One dictionary and no table saying which: every glyph uses it.
            _ => None,
        };
        Ok((private, select))
    }

    /// The outline of one glyph.
    ///
    /// `None` where the glyph draws nothing, which is what a space is.
    pub(crate) fn outline(
        &self,
        glyph: GlyphId,
        coordinates: &[f32],
    ) -> Result<Option<Outline>, Error> {
        let mut pen = Pen::default();
        self.run(glyph, coordinates, &mut pen)?;
        pen.finish();

        if pen.commands.is_empty() {
            return Ok(None);
        }
        let bounds = pen.bounds();
        let mut outline = Outline { commands: pen.commands, bounds };
        if let Some((x, y)) = self.matrix {
            scale(&mut outline, x, y);
        }
        Ok(Some(outline))
    }

    /// Runs one glyph's charstring into a pen.
    fn run(&self, glyph: GlyphId, coordinates: &[f32], pen: &mut Pen) -> Result<(), Error> {
        let Some(code) = self.charstrings.get(u32::from(glyph.0)) else {
            return Ok(());
        };
        let index = self.select.as_ref().map_or(0, |select| select.of(u32::from(glyph.0)));
        let private = self.private.get(index).copied().unwrap_or_default();

        let mut state = Machine {
            cff: self,
            coordinates,
            local: private.local,
            stack: Vec::with_capacity(MAX_STACK),
            stems: 0,
            width_seen: false,
            depth: 0,
            blend: self.blend_vector(0, coordinates),
        };
        state.execute(code, pen)
    }

    /// How much of each region of one set of variations applies at a setting
    /// of the axes. Empty for a font with no variations, which is every `CFF`
    /// of the older kind.
    fn blend_vector(&self, set: usize, coordinates: &[f32]) -> Vec<f32> {
        match &self.store {
            Some(store) => store.scalars(set, coordinates),
            None => Vec::new(),
        }
    }

    /// The glyph whose name is a given standard string, which is how `seac`
    /// names the two halves of an accented letter.
    fn glyph_named(&self, sid: u16) -> Option<GlyphId> {
        let charset = self.charset.as_ref()?;
        charset.glyph_for(sid)
    }
}

/// Multiplies an outline by a font's own scale.
///
/// Almost every font draws on a grid of a thousand units and says so in the
/// same breath; the few that do not say it with a matrix, and a reader that
/// ignores it draws the glyph at the wrong size.
fn scale(outline: &mut Outline, x: f64, y: f64) {
    let at = |point: Point| Point::new(point.x * x as f32, point.y * y as f32);
    for command in &mut outline.commands {
        *command = match *command {
            PathCommand::MoveTo(point) => PathCommand::MoveTo(at(point)),
            PathCommand::LineTo(point) => PathCommand::LineTo(at(point)),
            PathCommand::QuadTo(control, point) => PathCommand::QuadTo(at(control), at(point)),
            PathCommand::CubicTo(first, second, point) => {
                PathCommand::CubicTo(at(first), at(second), at(point))
            }
            PathCommand::Close => PathCommand::Close,
        };
    }
    outline.bounds = Bounds {
        min_x: (f32::from(outline.bounds.min_x) * x as f32) as i16,
        min_y: (f32::from(outline.bounds.min_y) * y as f32) as i16,
        max_x: (f32::from(outline.bounds.max_x) * x as f32) as i16,
        max_y: (f32::from(outline.bounds.max_y) * y as f32) as i16,
    };
}

// ---------------------------------------------------------------------------
// The structures a CFF table is built from
// ---------------------------------------------------------------------------

/// An INDEX: a list of byte strings, written as a count, an array of offsets,
/// and the strings one after another.
#[derive(Clone, Copy, Debug)]
struct Index<'a> {
    data: &'a [u8],
    count: u32,
    offset_size: usize,
    offsets_at: usize,
    /// One before the first byte of the data, because the offsets are written
    /// from one rather than from nought.
    base: usize,
    /// One past the last byte of the whole INDEX, which is where the next
    /// thing in the table begins.
    end: usize,
}

impl<'a> Index<'a> {
    /// Reads one. `wide` is what tells CFF2's four-byte count from CFF's two.
    fn parse(data: &'a [u8], at: usize, wide: bool) -> Result<Self, Error> {
        let mut reader = Reader::at(data, at)?;
        let count = if wide { reader.u32()? } else { u32::from(reader.u16()?) };
        if count == 0 {
            let end = reader.position();
            return Ok(Self { data, count, offset_size: 1, offsets_at: end, base: end, end });
        }

        let offset_size = reader.u8()? as usize;
        if !(1..=4).contains(&offset_size) {
            return Err(Error::MalformedTable("CFF"));
        }
        let offsets_at = reader.position();
        let count = count as usize;
        let base = offsets_at
            .checked_add((count + 1) * offset_size)
            .and_then(|after| after.checked_sub(1))
            .ok_or(Error::OutOfBounds)?;
        let last = offset(data, offsets_at + count * offset_size, offset_size)?;
        let end = base.checked_add(last).ok_or(Error::OutOfBounds)?;
        if end > data.len() {
            return Err(Error::OutOfBounds);
        }

        Ok(Self { data, count: count as u32, offset_size, offsets_at, base, end })
    }

    /// One entry, by number.
    fn get(&self, index: u32) -> Option<&'a [u8]> {
        if index >= self.count {
            return None;
        }
        let index = index as usize;
        let from =
            offset(self.data, self.offsets_at + index * self.offset_size, self.offset_size).ok()?;
        let to =
            offset(self.data, self.offsets_at + (index + 1) * self.offset_size, self.offset_size)
                .ok()?;
        self.data.get(self.base.checked_add(from)?..self.base.checked_add(to)?)
    }

    /// What a subroutine number is counted from. The format numbers them from
    /// the middle of the list outwards, so that the commonest ones are reached
    /// by a single small number.
    fn bias(&self) -> i32 {
        if self.count < 1240 {
            107
        } else if self.count < 33900 {
            1131
        } else {
            32768
        }
    }
}

/// One offset out of an INDEX's array, which is one to four bytes wide.
fn offset(data: &[u8], at: usize, size: usize) -> Result<usize, Error> {
    let end = at.checked_add(size).ok_or(Error::OutOfBounds)?;
    let bytes = data.get(at..end).ok_or(Error::OutOfBounds)?;
    Ok(bytes.iter().fold(0usize, |value, byte| (value << 8) | *byte as usize))
}

/// The keys of a dictionary that this reader asks about. A two-byte key is
/// written with 12 in the high byte, exactly as the format writes it.
const KEY_CHARSET: u16 = 15;
const KEY_CHARSTRINGS: u16 = 17;
const KEY_PRIVATE: u16 = 18;
const KEY_SUBRS: u16 = 19;
const KEY_MATRIX: u16 = 0x0C07;
const KEY_VSTORE: u16 = 24;
const KEY_ROS: u16 = 0x0C1E;
const KEY_FDARRAY: u16 = 0x0C24;
const KEY_FDSELECT: u16 = 0x0C25;

/// A DICT: numbers followed by the key they belong to, over and over.
///
/// Backwards from every other format, and deliberately: a reader that does not
/// know a key can throw its numbers away and carry on.
#[derive(Clone, Debug, Default)]
struct Dict {
    entries: Vec<(u16, Vec<f64>)>,
}

impl Dict {
    fn parse(data: &[u8]) -> Result<Self, Error> {
        let mut entries = Vec::new();
        let mut operands: Vec<f64> = Vec::new();
        let mut reader = Reader::new(data);

        while reader.remaining() > 0 {
            let first = reader.u8()?;
            match first {
                // The operators, which take whatever has been pushed. CFF's own
                // stop at twenty-one; CFF2 added three above them, for the
                // variations and how deep a charstring may push.
                0..=27 => {
                    let key = if first == 12 {
                        0x0C00 | u16::from(reader.u8()?)
                    } else {
                        u16::from(first)
                    };
                    entries.push((key, core::mem::take(&mut operands)));
                }
                28 => operands.push(f64::from(reader.i16()?)),
                29 => operands.push(f64::from(reader.u32()? as i32)),
                30 => operands.push(real(&mut reader)?),
                32..=246 => operands.push(f64::from(first) - 139.0),
                247..=250 => {
                    let low = f64::from(reader.u8()?);
                    operands.push((f64::from(first) - 247.0) * 256.0 + low + 108.0);
                }
                251..=254 => {
                    let low = f64::from(reader.u8()?);
                    operands.push(-(f64::from(first) - 251.0) * 256.0 - low - 108.0);
                }
                _ => return Err(Error::MalformedTable("CFF")),
            }
            if operands.len() > MAX_STACK {
                return Err(Error::MalformedTable("CFF"));
            }
        }
        Ok(Self { entries })
    }

    fn get(&self, key: u16) -> Option<&[f64]> {
        self.entries.iter().find(|(found, _)| *found == key).map(|(_, values)| values.as_slice())
    }

    /// A key whose value is one offset into the table.
    fn offset(&self, key: u16) -> Result<usize, Error> {
        let value = self
            .get(key)
            .and_then(|values| values.first().copied())
            .ok_or(Error::MalformedTable("CFF"))?;
        if value < 0.0 {
            return Err(Error::MalformedTable("CFF"));
        }
        Ok(value as usize)
    }

    /// The font's own scale, where it is not the one everything assumes.
    fn matrix(&self) -> Option<(f64, f64)> {
        let values = self.get(KEY_MATRIX)?;
        let [x, _, _, y, _, _] = values[..] else { return None };
        // The usual matrix is a thousandth, and the glyph is then drawn in the
        // units the rest of the font is measured in. Anything else is a scale
        // this has to apply.
        let (x, y) = (x * 1000.0, y * 1000.0);
        if (x - 1.0).abs() < 1e-6 && (y - 1.0).abs() < 1e-6 {
            None
        } else {
            Some((x, y))
        }
    }
}

/// A real number, written one decimal digit to a nibble.
fn real(reader: &mut Reader<'_>) -> Result<f64, Error> {
    let mut text = String::new();
    'outer: loop {
        let byte = reader.u8()?;
        for nibble in [byte >> 4, byte & 0x0F] {
            match nibble {
                0..=9 => text.push((b'0' + nibble) as char),
                0x0A => text.push('.'),
                0x0B => text.push('E'),
                0x0C => text.push_str("E-"),
                0x0E => text.push('-'),
                0x0F => break 'outer,
                _ => {}
            }
        }
        if text.len() > 64 {
            return Err(Error::MalformedTable("CFF"));
        }
    }
    text.parse().map_err(|_| Error::MalformedTable("CFF"))
}

/// Which private dictionary each glyph of a CID-keyed font belongs to.
#[derive(Clone, Copy, Debug)]
struct FontSelect<'a> {
    data: &'a [u8],
    at: usize,
    format: u8,
    ranges: u16,
    glyphs: u32,
}

impl<'a> FontSelect<'a> {
    fn parse(data: &'a [u8], at: usize, glyphs: u32) -> Result<Self, Error> {
        let mut reader = Reader::at(data, at)?;
        let format = reader.u8()?;
        let ranges = if format == 3 { reader.u16()? } else { 0 };
        Ok(Self { data, at, format, ranges, glyphs })
    }

    /// The dictionary one glyph uses.
    fn of(&self, glyph: u32) -> usize {
        match self.format {
            // One byte per glyph, in order.
            0 => self.data.get(self.at + 1 + glyph as usize).map_or(0, |value| *value as usize),
            // Ranges: the first glyph of each, and a sentinel at the end.
            3 => {
                if glyph >= self.glyphs {
                    return 0;
                }
                let mut answer = 0;
                for index in 0..self.ranges as usize {
                    let at = self.at + 3 + index * 3;
                    let Ok(first) = offset(self.data, at, 2) else { return answer };
                    let Some(dictionary) = self.data.get(at + 2) else { return answer };
                    if glyph >= first as u32 {
                        answer = *dictionary as usize;
                    } else {
                        break;
                    }
                }
                answer
            }
            _ => 0,
        }
    }
}

/// Which name each glyph has, as a number into the list of strings.
///
/// Only one thing here needs it — the old way of writing an accented letter,
/// which names the letter and the accent rather than pointing at them.
#[derive(Clone, Copy, Debug)]
struct Charset<'a> {
    data: &'a [u8],
    at: usize,
    format: u8,
    glyphs: u32,
}

impl<'a> Charset<'a> {
    fn parse(data: &'a [u8], at: usize, glyphs: u32) -> Result<Self, Error> {
        let mut reader = Reader::at(data, at)?;
        let format = reader.u8()?;
        Ok(Self { data, at, format, glyphs })
    }

    /// The glyph whose name is a given string, searched for rather than looked
    /// up: the table runs the other way, and this is asked for at most twice
    /// per accented letter.
    fn glyph_for(&self, sid: u16) -> Option<GlyphId> {
        // Glyph nought is always ".notdef" and is not in the table.
        match self.format {
            0 => {
                for glyph in 1..self.glyphs {
                    let at = self.at + 1 + (glyph as usize - 1) * 2;
                    let found = offset(self.data, at, 2).ok()? as u16;
                    if found == sid {
                        return Some(GlyphId(glyph as u16));
                    }
                }
                None
            }
            // Ranges of consecutive strings: a first name and how many follow.
            1 | 2 => {
                let step = if self.format == 1 { 3 } else { 4 };
                let mut glyph = 1u32;
                let mut at = self.at + 1;
                while glyph < self.glyphs {
                    let first = offset(self.data, at, 2).ok()? as u16;
                    let left = offset(self.data, at + 2, step - 2).ok()? as u32;
                    if sid >= first && u32::from(sid - first) <= left {
                        return Some(GlyphId((glyph + u32::from(sid - first)) as u16));
                    }
                    glyph += left + 1;
                    at += step;
                }
                None
            }
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

/// Where the pen is and what it has drawn.
#[derive(Clone, Debug, Default)]
struct Pen {
    commands: Vec<PathCommand>,
    x: f32,
    y: f32,
    open: bool,
    /// Everything a contour has passed through, which is the only way to know
    /// how big a CFF glyph is: unlike `glyf`, it does not say.
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
    seen: bool,
}

impl Pen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.finish();
        self.x = x;
        self.y = y;
        self.commands.push(PathCommand::MoveTo(Point::new(x, y)));
        self.open = true;
        self.reach(x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.x = x;
        self.y = y;
        self.commands.push(PathCommand::LineTo(Point::new(x, y)));
        self.reach(x, y);
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.x = x;
        self.y = y;
        self.commands.push(PathCommand::CubicTo(
            Point::new(x1, y1),
            Point::new(x2, y2),
            Point::new(x, y),
        ));
        self.reach(x1, y1);
        self.reach(x2, y2);
        self.reach(x, y);
    }

    /// Closes whatever contour is open, which is what a move or the end of the
    /// charstring does.
    fn finish(&mut self) {
        if self.open {
            self.commands.push(PathCommand::Close);
            self.open = false;
        }
    }

    fn reach(&mut self, x: f32, y: f32) {
        if !self.seen {
            self.seen = true;
            self.min_x = x;
            self.max_x = x;
            self.min_y = y;
            self.max_y = y;
            return;
        }
        self.min_x = self.min_x.min(x);
        self.max_x = self.max_x.max(x);
        self.min_y = self.min_y.min(y);
        self.max_y = self.max_y.max(y);
    }

    fn bounds(&self) -> Bounds {
        Bounds {
            min_x: self.min_x.floor() as i16,
            min_y: self.min_y.floor() as i16,
            max_x: self.max_x.ceil() as i16,
            max_y: self.max_y.ceil() as i16,
        }
    }
}

/// The interpreter: a stack, the subroutines within reach, and how many stem
/// hints have been declared, which is the only way to know how long a hint
/// mask is.
struct Machine<'a, 'f> {
    cff: &'a Cff<'f>,
    coordinates: &'a [f32],
    local: Option<Index<'f>>,
    stack: Vec<f32>,
    stems: usize,
    width_seen: bool,
    depth: u8,
    /// How much of each region applies, for the set of variations the
    /// charstring is currently using. A charstring may change which set that
    /// is as it goes.
    blend: Vec<f32>,
}

impl Machine<'_, '_> {
    /// Runs one charstring, following whatever it calls.
    fn execute(&mut self, code: &[u8], pen: &mut Pen) -> Result<(), Error> {
        if self.depth > MAX_DEPTH {
            return Err(Error::RecursiveGlyph);
        }
        let mut reader = Reader::new(code);

        while reader.remaining() > 0 {
            let operator = reader.u8()?;
            match operator {
                // The numbers.
                28 => self.push(f32::from(reader.i16()?))?,
                32..=246 => self.push(f32::from(operator) - 139.0)?,
                247..=250 => {
                    let low = f32::from(reader.u8()?);
                    self.push((f32::from(operator) - 247.0) * 256.0 + low + 108.0)?;
                }
                251..=254 => {
                    let low = f32::from(reader.u8()?);
                    self.push(-(f32::from(operator) - 251.0) * 256.0 - low - 108.0)?;
                }
                // A number with sixteen bits either side of the point.
                255 => self.push(reader.u32()? as i32 as f32 / 65536.0)?,

                // The stem hints. Nothing is drawn for them, but they have to
                // be counted: the mask that follows says one bit per stem, and
                // without the count the bytes cannot be skipped.
                1 | 3 | 18 | 23 => self.stems(),
                19 | 20 => {
                    self.stems();
                    reader.skip(self.stems.div_ceil(8))?;
                }

                // Moving the pen, which begins a new contour.
                21 => {
                    self.take_width(2);
                    let (dx, dy) = self.last_two();
                    pen.move_to(pen.x + dx, pen.y + dy);
                    self.stack.clear();
                }
                22 => {
                    self.take_width(1);
                    let dx = self.last_one();
                    pen.move_to(pen.x + dx, pen.y);
                    self.stack.clear();
                }
                4 => {
                    self.take_width(1);
                    let dy = self.last_one();
                    pen.move_to(pen.x, pen.y + dy);
                    self.stack.clear();
                }

                // Straight lines.
                5 => {
                    for pair in self.stack.chunks_exact(2) {
                        pen.line_to(pen.x + pair[0], pen.y + pair[1]);
                    }
                    self.stack.clear();
                }
                6 | 7 => {
                    let mut horizontal = operator == 6;
                    for step in core::mem::take(&mut self.stack) {
                        if horizontal {
                            pen.line_to(pen.x + step, pen.y);
                        } else {
                            pen.line_to(pen.x, pen.y + step);
                        }
                        horizontal = !horizontal;
                    }
                }

                // Curves, of which there are five spellings, each saving the
                // bytes of whichever deltas are nought.
                8 => {
                    for six in self.stack.chunks_exact(6) {
                        relative_curve(pen, six);
                    }
                    self.stack.clear();
                }
                24 => {
                    let taken = core::mem::take(&mut self.stack);
                    let curves = (taken.len().saturating_sub(2)) / 6;
                    for six in taken.chunks_exact(6).take(curves) {
                        relative_curve(pen, six);
                    }
                    if let [dx, dy] = taken[curves * 6..] {
                        pen.line_to(pen.x + dx, pen.y + dy);
                    }
                }
                25 => {
                    let taken = core::mem::take(&mut self.stack);
                    let lines = (taken.len().saturating_sub(6)) / 2;
                    for pair in taken.chunks_exact(2).take(lines) {
                        pen.line_to(pen.x + pair[0], pen.y + pair[1]);
                    }
                    if taken.len() >= lines * 2 + 6 {
                        relative_curve(pen, &taken[lines * 2..lines * 2 + 6]);
                    }
                }
                26 | 27 => self.axis_curves(pen, operator == 27),
                30 | 31 => self.alternating_curves(pen, operator == 31),

                // Calling another charstring. The number is counted from the
                // middle of the list, which is what the bias undoes.
                10 | 29 => {
                    let which = self.stack.pop().unwrap_or(0.0);
                    let index = if operator == 10 { self.local } else { Some(self.cff.global) };
                    let Some(index) = index else { continue };
                    let number = which as i32 + index.bias();
                    if number < 0 {
                        continue;
                    }
                    let Some(code) = index.get(number as u32) else { continue };
                    self.depth += 1;
                    self.execute(code, pen)?;
                    self.depth -= 1;
                }
                11 => return Ok(()),

                // The end, which may also be an accented letter written as the
                // letter and the accent.
                14 => {
                    self.take_width(0);
                    if self.stack.len() >= 4 {
                        self.accented(pen)?;
                    }
                    pen.finish();
                    return Ok(());
                }

                12 => {
                    let second = reader.u8()?;
                    self.escaped(second, pen);
                }

                // CFF2's two: which set of deltas to use, and the deltas
                // themselves.
                15 => {
                    let set = self.stack.pop().unwrap_or(0.0).max(0.0) as usize;
                    self.blend = self.cff.blend_vector(set, self.coordinates);
                    self.stack.clear();
                }
                16 => self.blend(),

                _ => self.stack.clear(),
            }
        }
        Ok(())
    }

    /// The two-byte operators: the flex family, which draws a pair of curves so
    /// nearly flat that the format has a shorter way of saying it.
    fn escaped(&mut self, operator: u8, pen: &mut Pen) {
        let taken = core::mem::take(&mut self.stack);
        match operator {
            // hflex: a flex whose ends are level, so the vertical deltas of
            // three of the four control points are nought.
            34 if taken.len() >= 7 => {
                let y = pen.y;
                let one = pen.x + taken[0];
                let two = one + taken[1];
                let top = pen.y + taken[2];
                let three = two + taken[3];
                pen.curve_to(one, y, two, top, three, top);
                let four = three + taken[4];
                let five = four + taken[5];
                let six = five + taken[6];
                pen.curve_to(four, top, five, y, six, y);
            }
            // flex: the whole thing, two curves and a threshold nobody reads.
            35 if taken.len() >= 13 => {
                relative_curve(pen, &taken[0..6]);
                relative_curve(pen, &taken[6..12]);
            }
            // hflex1: the ends are level but the middle is not.
            36 if taken.len() >= 9 => {
                let y = pen.y;
                let one = pen.x + taken[0];
                let one_y = pen.y + taken[1];
                let two = one + taken[2];
                let two_y = one_y + taken[3];
                let three = two + taken[4];
                pen.curve_to(one, one_y, two, two_y, three, two_y);
                let four = three + taken[5];
                let five = four + taken[6];
                let five_y = two_y + taken[7];
                let six = five + taken[8];
                pen.curve_to(four, two_y, five, five_y, six, y);
            }
            // flex1: every delta but the last, which is whichever of the two
            // makes the curve come back to where it started.
            37 if taken.len() >= 11 => {
                let (from_x, from_y) = (pen.x, pen.y);
                let dx: f32 = taken[0] + taken[2] + taken[4] + taken[6] + taken[8];
                let dy: f32 = taken[1] + taken[3] + taken[5] + taken[7] + taken[9];
                relative_curve(pen, &taken[0..6]);

                let four = pen.x + taken[6];
                let four_y = pen.y + taken[7];
                let five = four + taken[8];
                let five_y = four_y + taken[9];
                let (six, six_y) = if dx.abs() > dy.abs() {
                    (five + taken[10], from_y)
                } else {
                    (from_x, five_y + taken[10])
                };
                pen.curve_to(four, four_y, five, five_y, six, six_y);
            }
            _ => {}
        }
    }

    /// An accented letter written the old way: the letter and the accent, each
    /// named rather than pointed at, and the accent moved into place.
    fn accented(&mut self, pen: &mut Pen) -> Result<(), Error> {
        let taken = core::mem::take(&mut self.stack);
        let [dx, dy, base, accent] = taken[taken.len() - 4..] else { return Ok(()) };
        let (Some(base), Some(accent)) = (standard_sid(base), standard_sid(accent)) else {
            return Ok(());
        };
        let (Some(base), Some(accent)) = (self.cff.glyph_named(base), self.cff.glyph_named(accent))
        else {
            return Ok(());
        };

        pen.finish();
        let mut letter = Pen::default();
        self.cff.run(base, self.coordinates, &mut letter)?;
        letter.finish();

        let mut mark = Pen::default();
        self.cff.run(accent, self.coordinates, &mut mark)?;
        mark.finish();
        shift(&mut mark.commands, dx, dy);

        pen.commands.extend(letter.commands);
        pen.commands.extend(mark.commands);
        pen.reach(letter.min_x, letter.min_y);
        pen.reach(letter.max_x, letter.max_y);
        pen.reach(mark.min_x + dx, mark.min_y + dy);
        pen.reach(mark.max_x + dx, mark.max_y + dy);
        Ok(())
    }

    /// `vvcurveto` and `hhcurveto`: a run of curves all going one way, with one
    /// step across at the start if the run does not begin square.
    fn axis_curves(&mut self, pen: &mut Pen, horizontal: bool) {
        let taken = core::mem::take(&mut self.stack);
        let (mut across, rest) =
            if taken.len() % 4 == 1 { (taken[0], &taken[1..]) } else { (0.0, &taken[..]) };

        for four in rest.chunks_exact(4) {
            if horizontal {
                let x1 = pen.x + four[0];
                let y1 = pen.y + across;
                let x2 = x1 + four[1];
                let y2 = y1 + four[2];
                pen.curve_to(x1, y1, x2, y2, x2 + four[3], y2);
            } else {
                let x1 = pen.x + across;
                let y1 = pen.y + four[0];
                let x2 = x1 + four[1];
                let y2 = y1 + four[2];
                pen.curve_to(x1, y1, x2, y2, x2, y2 + four[3]);
            }
            across = 0.0;
        }
    }

    /// `hvcurveto` and `vhcurveto`: curves that turn a corner each time, going
    /// from horizontal to vertical and back, with one last delta allowed at the
    /// end for a curve that does not finish square.
    fn alternating_curves(&mut self, pen: &mut Pen, mut horizontal: bool) {
        let taken = core::mem::take(&mut self.stack);
        let groups = taken.len() / 4;
        let extra = if taken.len() % 4 == 1 { taken[taken.len() - 1] } else { 0.0 };

        for (index, four) in taken.chunks_exact(4).take(groups).enumerate() {
            let last = index + 1 == groups;
            if horizontal {
                let x1 = pen.x + four[0];
                let y1 = pen.y;
                let x2 = x1 + four[1];
                let y2 = y1 + four[2];
                let y = y2 + four[3];
                let x = if last { x2 + extra } else { x2 };
                pen.curve_to(x1, y1, x2, y2, x, y);
            } else {
                let x1 = pen.x;
                let y1 = pen.y + four[0];
                let x2 = x1 + four[1];
                let y2 = y1 + four[2];
                let x = x2 + four[3];
                let y = if last { y2 + extra } else { y2 };
                pen.curve_to(x1, y1, x2, y2, x, y);
            }
            horizontal = !horizontal;
        }
    }

    /// CFF2's `blend`: so many values, then that many deltas for each of them,
    /// one per region of the variation set in use.
    ///
    /// Each value is moved by its own deltas, each spent in proportion to how
    /// much of its region applies; the deltas are then dropped and the values
    /// left. At the default instance nothing applies and the values stay where
    /// they were — but the deltas still have to be counted to be dropped, and
    /// whatever was pushed before the blend has to be left alone.
    fn blend(&mut self) {
        let count = self.stack.pop().unwrap_or(0.0);
        if count < 0.0 {
            self.stack.clear();
            return;
        }
        let count = count as usize;
        let regions = self.blend.len();
        let deltas = count.saturating_mul(regions);
        if deltas > self.stack.len() || count > self.stack.len() - deltas {
            self.stack.clear();
            return;
        }

        let first = self.stack.len() - deltas - count;
        for value in 0..count {
            let mut moved = self.stack[first + value];
            for (region, scalar) in self.blend.iter().enumerate() {
                moved += self.stack[first + count + value * regions + region] * scalar;
            }
            self.stack[first + value] = moved;
        }
        self.stack.truncate(first + count);
    }

    /// Counts a stem hint operator's stems, and takes the width if it is there.
    fn stems(&mut self) {
        if !self.width_seen && self.stack.len() % 2 == 1 {
            self.stack.remove(0);
        }
        self.width_seen = true;
        self.stems += self.stack.len() / 2;
        self.stack.clear();
    }

    /// The width, which is written in front of the first operator's arguments
    /// where it is not the font's usual one — so the only way to know it is
    /// there is to count what the operator was given.
    fn take_width(&mut self, wanted: usize) {
        if !self.width_seen && self.stack.len() > wanted {
            self.stack.remove(0);
        }
        self.width_seen = true;
    }

    fn last_one(&self) -> f32 {
        self.stack.last().copied().unwrap_or(0.0)
    }

    fn last_two(&self) -> (f32, f32) {
        match self.stack[..] {
            [.., x, y] => (x, y),
            _ => (0.0, 0.0),
        }
    }

    fn push(&mut self, value: f32) -> Result<(), Error> {
        if self.stack.len() >= MAX_STACK {
            return Err(Error::MalformedTable("CFF"));
        }
        self.stack.push(value);
        Ok(())
    }
}

/// One curve, written as three deltas from where the pen is.
fn relative_curve(pen: &mut Pen, six: &[f32]) {
    let x1 = pen.x + six[0];
    let y1 = pen.y + six[1];
    let x2 = x1 + six[2];
    let y2 = y1 + six[3];
    pen.curve_to(x1, y1, x2, y2, x2 + six[4], y2 + six[5]);
}

/// Moves a finished outline, which is what an accent needs.
fn shift(commands: &mut [PathCommand], dx: f32, dy: f32) {
    let at = |point: Point| Point::new(point.x + dx, point.y + dy);
    for command in commands {
        *command = match *command {
            PathCommand::MoveTo(point) => PathCommand::MoveTo(at(point)),
            PathCommand::LineTo(point) => PathCommand::LineTo(at(point)),
            PathCommand::QuadTo(control, point) => PathCommand::QuadTo(at(control), at(point)),
            PathCommand::CubicTo(first, second, point) => {
                PathCommand::CubicTo(at(first), at(second), at(point))
            }
            PathCommand::Close => PathCommand::Close,
        };
    }
}

/// The name a code stands for in the encoding every PostScript font starts
/// from, as a number into the list of standard strings.
///
/// Only the accented-letter operator asks: it names its two halves by code.
/// The printable range is in order and needs no table; the rest is the
/// punctuation and the accents, and it is a table because the codes are not.
fn standard_sid(code: f32) -> Option<u16> {
    let code = code as i32;
    if (32..=126).contains(&code) {
        return Some(code as u16 - 31);
    }
    let named: u16 = match code {
        161 => 96,  // exclamdown
        162 => 97,  // cent
        163 => 98,  // sterling
        164 => 99,  // fraction
        165 => 100, // yen
        166 => 101, // florin
        167 => 102, // section
        168 => 103, // currency
        169 => 104, // quotesingle
        170 => 105, // quotedblleft
        171 => 106, // guillemotleft
        172 => 107, // guilsinglleft
        173 => 108, // guilsinglright
        174 => 109, // fi
        175 => 110, // fl
        177 => 111, // endash
        178 => 112, // dagger
        179 => 113, // daggerdbl
        180 => 114, // periodcentered
        182 => 115, // paragraph
        183 => 116, // bullet
        184 => 117, // quotesinglbase
        185 => 118, // quotedblbase
        186 => 119, // quotedblright
        187 => 120, // guillemotright
        188 => 121, // ellipsis
        189 => 122, // perthousand
        191 => 123, // questiondown
        193 => 124, // grave
        194 => 125, // acute
        195 => 126, // circumflex
        196 => 127, // tilde
        197 => 128, // macron
        198 => 129, // breve
        199 => 130, // dotaccent
        200 => 131, // dieresis
        202 => 132, // ring
        203 => 133, // cedilla
        205 => 134, // hungarumlaut
        206 => 135, // ogonek
        207 => 136, // caron
        208 => 137, // emdash
        225 => 138, // AE
        227 => 139, // ordfeminine
        232 => 140, // Lslash
        233 => 141, // Oslash
        234 => 142, // OE
        235 => 143, // ordmasculine
        241 => 144, // ae
        245 => 145, // dotlessi
        248 => 146, // lslash
        249 => 147, // oslash
        250 => 148, // oe
        251 => 149, // germandbls
        _ => return None,
    };
    Some(named)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A number as a charstring writes it, for the small ones this needs.
    fn small(value: i32) -> Vec<u8> {
        match value {
            -107..=107 => vec![(value + 139) as u8],
            108..=1131 => {
                let value = value - 108;
                vec![247 + (value / 256) as u8, (value % 256) as u8]
            }
            -1131..=-108 => {
                let value = -value - 108;
                vec![251 + (value / 256) as u8, (value % 256) as u8]
            }
            _ => panic!("{value} is outside what this test writes"),
        }
    }

    /// An INDEX of the CFF2 kind: a four-byte count, one-byte offsets.
    fn index(entries: &[Vec<u8>]) -> Vec<u8> {
        let mut out = (entries.len() as u32).to_be_bytes().to_vec();
        if entries.is_empty() {
            return out;
        }
        out.push(1); // one byte to an offset
        let mut at = 1u8;
        out.push(at);
        for entry in entries {
            at += entry.len() as u8;
            out.push(at);
        }
        for entry in entries {
            out.extend_from_slice(entry);
        }
        out
    }

    /// A store of variations that says nothing except how many deltas there
    /// are, which is the one thing a charstring cannot do without.
    fn variation_store(regions: u16) -> Vec<u8> {
        let mut store: Vec<u8> = Vec::new();
        store.extend_from_slice(&1u16.to_be_bytes()); // format
        store.extend_from_slice(&12u32.to_be_bytes()); // where the regions are
        store.extend_from_slice(&1u16.to_be_bytes()); // one set of them
        let data_at = 12 + 4 + 6 * u32::from(regions);
        store.extend_from_slice(&data_at.to_be_bytes());

        // The regions themselves: one axis, and for each region the three
        // values that say where its influence starts, is whole, and ends. They
        // all reach the far end of the axis, so at the far end they apply in
        // full and at the near end not at all.
        store.extend_from_slice(&1u16.to_be_bytes());
        store.extend_from_slice(&regions.to_be_bytes());
        for _ in 0..regions {
            store.extend_from_slice(&0i16.to_be_bytes()); // starts at the default
            store.extend_from_slice(&0x4000i16.to_be_bytes()); // whole at the end
            store.extend_from_slice(&0x4000i16.to_be_bytes()); // and stops there
        }

        // And the set that names them.
        store.extend_from_slice(&0u16.to_be_bytes()); // no items
        store.extend_from_slice(&0u16.to_be_bytes()); // none of them short
        store.extend_from_slice(&regions.to_be_bytes());
        for region in 0..regions {
            store.extend_from_slice(&region.to_be_bytes());
        }

        let mut out = (store.len() as u16).to_be_bytes().to_vec();
        out.extend_from_slice(&store);
        out
    }

    /// A whole `CFF2` table holding one glyph drawn by the given charstring.
    fn table(charstring: Vec<u8>, regions: u16) -> Vec<u8> {
        let charstrings = index(&[Vec::new(), charstring]);
        // One font dictionary, which needs to say nothing for this.
        let fonts = index(&[Vec::new()]);
        let store = variation_store(regions);

        // The top dictionary is written twice: once to find out how long it is,
        // and once with the offsets that depend on that length.
        let build = |charstrings_at: u32, fonts_at: u32, store_at: u32| {
            let mut out = Vec::new();
            out.push(29);
            out.extend_from_slice(&charstrings_at.to_be_bytes());
            out.push(17);
            out.push(29);
            out.extend_from_slice(&fonts_at.to_be_bytes());
            out.extend_from_slice(&[12, 36]);
            out.push(29);
            out.extend_from_slice(&store_at.to_be_bytes());
            out.push(24);
            out
        };
        let length = build(0, 0, 0).len();

        let header = 5;
        let top_end = header + length;
        // The global subroutines, of which there are none.
        let charstrings_at = top_end + 4;
        let fonts_at = charstrings_at + charstrings.len();
        let store_at = fonts_at + fonts.len();

        let mut out = vec![2, 0, header as u8];
        out.extend_from_slice(&(length as u16).to_be_bytes());
        out.extend_from_slice(&build(charstrings_at as u32, fonts_at as u32, store_at as u32));
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(&charstrings);
        out.extend_from_slice(&fonts);
        out.extend_from_slice(&store);
        out
    }

    /// The four corners of whatever was drawn, in the order drawn.
    fn corners(outline: &Outline) -> Vec<(f32, f32)> {
        outline
            .commands
            .iter()
            .filter_map(|command| match command {
                PathCommand::MoveTo(point) | PathCommand::LineTo(point) => Some((point.x, point.y)),
                _ => None,
            })
            .collect()
    }

    /// `100 100 rmoveto`, then a square drawn from there.
    fn square() -> Vec<u8> {
        let mut code = Vec::new();
        code.extend(small(100));
        code.extend(small(100));
        code.push(21);
        for value in [200, 0, 0, 200, -200, 0] {
            code.extend(small(value));
        }
        code.push(5);
        code
    }

    #[test]
    fn a_cff2_table_draws_what_its_charstring_says() {
        let data = table(square(), 2);
        let table = Cff::parse2(&data).expect("a table that parses");
        let outline =
            table.outline(GlyphId(1), &[]).expect("a glyph that draws").expect("an outline");

        assert_eq!(
            corners(&outline),
            vec![(100.0, 100.0), (300.0, 100.0), (300.0, 300.0), (100.0, 300.0)]
        );
        assert_eq!(outline.bounds, Bounds { min_x: 100, min_y: 100, max_x: 300, max_y: 300 });
        assert!(matches!(outline.commands.last(), Some(PathCommand::Close)));
    }

    #[test]
    fn a_glyph_with_no_charstring_draws_nothing() {
        let data = table(square(), 2);
        let table = Cff::parse2(&data).unwrap();
        assert!(table.outline(GlyphId(0), &[]).unwrap().is_none());
        assert!(table.outline(GlyphId(7), &[]).unwrap().is_none());
    }

    #[test]
    fn blending_at_the_default_instance_keeps_the_values_and_drops_the_deltas() {
        // Which is the whole of what a variable font means here: the numbers
        // the font was drawn at, and a pile of deltas that are all nought
        // until an axis is moved. Counting them wrongly unwinds the stack
        // wrongly, and the glyph comes out as rubbish rather than as nothing —
        // which is why this is worth a test of its own.
        const REGIONS: u16 = 2;
        let mut code = Vec::new();
        code.extend(small(100));
        code.extend(small(100));
        code.push(21);

        let values = [200, 0, 0, 200, -200, 0];
        for value in values {
            code.extend(small(value));
        }
        for _ in 0..values.len() * REGIONS as usize {
            code.extend(small(0));
        }
        code.extend(small(values.len() as i32));
        code.push(16); // blend
        code.push(5); // rlineto

        let data = table(code, REGIONS);
        let table = Cff::parse2(&data).unwrap();
        let outline = table.outline(GlyphId(1), &[]).unwrap().expect("an outline");
        assert_eq!(
            corners(&outline),
            vec![(100.0, 100.0), (300.0, 100.0), (300.0, 300.0), (100.0, 300.0)]
        );
    }

    #[test]
    fn a_truncated_table_is_refused_rather_than_guessed_at() {
        let data = table(square(), 2);
        for length in 0..data.len() {
            // Whatever it makes of a piece of a table, it must not panic: a
            // font file is data from outside the program.
            if let Ok(table) = Cff::parse2(&data[..length]) {
                let _ = table.outline(GlyphId(1), &[]);
            }
        }
    }

    #[test]
    fn blending_away_from_the_default_moves_what_was_drawn() {
        // The other half of a variable font: an axis turned all the way spends
        // every delta in full. Here the square is drawn two hundred wide with
        // a delta of a hundred on each of its two regions, so at the far end of
        // the axis it comes out four hundred wide.
        const REGIONS: u16 = 2;
        let mut code = Vec::new();
        code.extend(small(100));
        code.extend(small(100));
        code.push(21);

        let values = [200, 0, 0, 200, -200, 0];
        for value in values {
            code.extend(small(value));
        }
        // Each value's deltas: one per region, and only the two that make the
        // square wider and taller are anything but nought.
        for value in values {
            for _ in 0..REGIONS {
                code.extend(small(if value == 200 { 100 } else { 0 }));
            }
        }
        code.extend(small(values.len() as i32));
        code.push(16); // blend
        code.push(5); // rlineto

        let data = table(code, REGIONS);
        let table = Cff::parse2(&data).unwrap();

        // Where the axis has not been moved, the square is the square.
        let drawn = table.outline(GlyphId(1), &[]).unwrap().expect("an outline");
        assert_eq!(drawn.bounds, Bounds { min_x: 100, min_y: 100, max_x: 300, max_y: 300 });

        // And at the far end of it, both regions apply in full.
        let far = table.outline(GlyphId(1), &[1.0]).unwrap().expect("an outline");
        assert_eq!(far.bounds, Bounds { min_x: 100, min_y: 100, max_x: 500, max_y: 500 });

        // Halfway along, half of each.
        let middle = table.outline(GlyphId(1), &[0.5]).unwrap().expect("an outline");
        assert_eq!(middle.bounds, Bounds { min_x: 100, min_y: 100, max_x: 400, max_y: 400 });
    }
}
