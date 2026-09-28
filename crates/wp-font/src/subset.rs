//! A `CFF` table cut down to the glyphs a document uses.
//!
//! # Why this is not what cutting a TrueType font is
//!
//! A TrueType glyph is a list of points, and one that is not wanted is cut by
//! writing nothing where it was. A `CFF` glyph is a program, and the programs
//! share their pieces: a stroke two hundred letters have in common is written
//! once, as a subroutine, and called by number from each of them. So the
//! glyphs that are kept cannot be copied and the rest dropped — what they call
//! has to come with them, and what nobody calls is most of the table.
//!
//! # How it is cut
//!
//! Every glyph asked for is run, the way it would be drawn, and every
//! subroutine it reaches is noted — and so is the place in the charstring
//! where the number that named it was written. An accented letter written the
//! old way names its letter and its accent rather than calling them, and those
//! two glyphs are run and kept as well.
//!
//! The subroutines nobody reached are dropped, the rest are numbered again
//! from nought, and every call is rewritten with the new number — counted, as
//! the format counts it, from the middle of the new list, whose bias is not
//! the old one's.
//!
//! The glyphs keep the numbers the page names them by. In a font whose
//! glyphs are named, that is their place in the font, so none may move: a
//! glyph not asked for is left as a charstring that draws nothing, and the
//! glyphs after the last one wanted are not written at all. In a CID-keyed
//! font the page names a glyph by its CID, and the font's own table says
//! which glyph each CID is — so the glyphs kept are written one after
//! another and the table gives each the CID that is its place in the whole
//! font. That is the difference between a quarter of a megabyte and twenty
//! kilobytes for a line of Japanese, whose glyphs are spread across sixty-five
//! thousand.
//!
//! A call is rewritten only where the number it calls was written as a
//! number immediately in front of it, in the same charstring, and every run
//! through that place called the same subroutine — which is how every
//! subroutinizer writes one. A font where that is not so keeps every
//! subroutine at its own number and has the ones nobody calls emptied, which
//! is larger and still right; a font whose charstrings cannot be followed at
//! all is not cut, and goes in whole.
//!
//! # What changes besides
//!
//! A font whose glyphs are split between several dictionaries — every
//! Chinese, Japanese and Korean one — keeps them all, each with its own
//! subroutines cut the same way. The CID the page names a glyph by is its
//! place in the whole font, which is what the PDF writer has always written;
//! the whole font's own CIDs are not, unless its CIDs and its places happen
//! to agree. The strings, the glyph names and the dictionary entries this
//! does not rewrite are copied as they were written, number for number.

use std::collections::{BTreeMap, BTreeSet};

use crate::cff::{self, Cff, Index};
use crate::read::Reader;
use crate::Error;

/// A PostScript font cut down, and what was done to cut it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CutDown {
    /// The new font: a whole OpenType file, the cut `CFF` table in it and the
    /// tables a reader needs to open it cut to the same number of glyphs.
    pub font: Vec<u8>,
    /// The cut `CFF` table on its own, which is the same table.
    ///
    /// What a PDF should carry for a CID-keyed font. The format lets either
    /// go in, and says a reader finds a glyph by its CID through the font's
    /// own table either way; but FreeType does that only for a bare table,
    /// and treats the CID as the glyph's place inside an OpenType file — and
    /// Poppler leaves it to FreeType. Acrobat has always written the bare
    /// table.
    pub table: Vec<u8>,
    /// Whether its glyphs are named by CID.
    pub cid_keyed: bool,
    /// Which glyph of the whole font each glyph of the cut one is, in order.
    ///
    /// For a font whose glyphs are named, every glyph up to the last one kept,
    /// so that none moves: a page names such a font's glyphs by their place.
    /// For a CID-keyed font, only the glyphs kept: a page names those by CID,
    /// the cut font's table of CIDs gives each glyph the number of its place
    /// in the whole font, and so it can stand anywhere.
    pub glyphs: Vec<u16>,
    /// Every glyph whose outline was kept: the ones asked for, the empty
    /// first glyph every font has, and the letters and accents the accented
    /// letters among them are made of.
    pub kept: BTreeSet<u16>,
    /// What became of the subroutines.
    pub subroutines: Subroutines,
}

/// What became of a cut font's subroutines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Subroutines {
    /// The ones the kept glyphs call were kept, numbered again, and every
    /// call rewritten to the new number. The rest are gone.
    Renumbered,
    /// Every one keeps its number, and the ones nobody calls are emptied:
    /// for a font where a call could not be rewritten safely.
    Emptied,
}

/// Cuts a `CFF` table down to the glyphs given.
///
/// `None` for a table this cannot cut — `CFF2`, charstrings of the old
/// Type 1 kind, or charstrings it cannot follow — which the caller then
/// embeds whole, as it did before any of this existed.
pub(crate) fn cut(whole: &crate::Font<'_>, data: &[u8], asked: &BTreeSet<u16>) -> Option<CutDown> {
    let Table { table, glyphs, kept, subroutines, cid_keyed } = cut_table(data, asked)?;
    let font = font_file(whole, table.clone(), &glyphs)?;
    Some(CutDown { font, table, cid_keyed, glyphs, kept, subroutines })
}

/// A cut `CFF` table, before it is put into a font file.
struct Table {
    table: Vec<u8>,
    glyphs: Vec<u16>,
    kept: BTreeSet<u16>,
    subroutines: Subroutines,
    cid_keyed: bool,
}

/// Cuts the table itself.
fn cut_table(data: &[u8], asked: &BTreeSet<u16>) -> Option<Table> {
    let font = Cff::parse(data).ok()?;
    let layout = Layout::read(data)?;
    let total = font.charstrings.count;
    if total == 0 || total > u32::from(u16::MAX) + 1 {
        return None;
    }

    // Which glyphs are kept: the ones asked for, the first, and whatever an
    // accented letter among them is built from — run until nothing new turns
    // up, since the letter an accent sits on may itself be built that way.
    let mut kept: BTreeSet<u16> =
        asked.iter().copied().filter(|glyph| u32::from(*glyph) < total).collect();
    kept.insert(0);
    let mut trace = Trace::new(&font);
    let mut waiting: Vec<u16> = kept.iter().copied().collect();
    let mut run = BTreeSet::new();
    while let Some(glyph) = waiting.pop() {
        if !run.insert(glyph) {
            continue;
        }
        for part in trace.glyph(glyph).ok()? {
            if kept.insert(part) {
                waiting.push(part);
            }
        }
    }
    // Which glyphs the cut font has, in order: every one up to the last kept
    // where a page names glyphs by their place, only the kept ones where it
    // names them by CID.
    let glyphs: Vec<u16> = if layout.cid {
        kept.iter().copied().collect()
    } else {
        (0..=kept.last().copied().unwrap_or(0)).collect()
    };

    // The new numbers, and whether every call can be written with one.
    let numbering = Numbering::of(&trace);
    let subroutines =
        if trace.can_renumber(&numbering) { Subroutines::Renumbered } else { Subroutines::Emptied };

    let rewrite = |body: Body, code: &[u8]| match subroutines {
        Subroutines::Renumbered => trace.rewritten(body, code, &numbering),
        Subroutines::Emptied => code.to_vec(),
    };

    // The glyphs, every one of them up to the last kept, so that none moves.
    let mut charstrings = Vec::with_capacity(glyphs.len());
    for glyph in glyphs.iter().copied() {
        let code = font.charstrings.get(u32::from(glyph)).unwrap_or(&[]);
        charstrings.push(if kept.contains(&glyph) {
            rewrite(Body::Glyph(glyph), code)
        } else {
            vec![ENDCHAR]
        });
    }

    // The subroutines: the ones reached, in the order they were numbered,
    // or all of them with the ones not reached left saying nothing.
    let global = match subroutines {
        Subroutines::Renumbered => numbering
            .global
            .iter()
            .map(|index| rewrite(Body::Global(*index), font.global.get(*index).unwrap_or(&[])))
            .collect(),
        Subroutines::Emptied => {
            emptied(&font.global, |index| trace.used.contains(&Subr::Global(index)))
        }
    };
    let dictionaries = font.private.len().max(1);
    let mut locals: Vec<Vec<Vec<u8>>> = Vec::with_capacity(dictionaries);
    for dictionary in 0..dictionaries {
        let Some(index) = font.private.get(dictionary).and_then(|private| private.local) else {
            locals.push(Vec::new());
            continue;
        };
        locals.push(match subroutines {
            Subroutines::Renumbered => numbering
                .local
                .get(&dictionary)
                .map(|kept| {
                    kept.iter()
                        .map(|number| {
                            rewrite(
                                Body::Local(dictionary, *number),
                                index.get(*number).unwrap_or(&[]),
                            )
                        })
                        .collect()
                })
                .unwrap_or_default(),
            Subroutines::Emptied => {
                emptied(&index, |number| trace.used.contains(&Subr::Local(dictionary, number)))
            }
        });
    }

    let table = layout.write(&font, &glyphs, &charstrings, &global, &locals)?;
    Some(Table { table, glyphs, kept, subroutines, cid_keyed: layout.cid })
}

/// Puts a cut `CFF` table into a font file of its own, with the tables a
/// reader needs to open it cut to the same number of glyphs.
///
/// Which tables those are is what FreeType will not open a font without —
/// the header, the horizontal header and metrics, the glyph count — and the
/// two small ones that say how the font is to be used and what it is called
/// in PostScript terms. The character map is left out, as it is from a cut
/// TrueType font: a PDF names glyphs by number, and the map of a font for
/// Chinese is larger than the glyphs a page uses.
fn font_file(whole: &crate::Font<'_>, table: Vec<u8>, glyphs: &[u16]) -> Option<Vec<u8>> {
    let glyph_count = u16::try_from(glyphs.len()).ok()?;
    let head = whole.table(b"head")?;
    let hhea = whole.table(b"hhea")?;
    let hmtx = whole.table(b"hmtx")?;
    let maxp = whole.table(b"maxp")?;
    if head.len() < 54 || hhea.len() < 36 || maxp.len() < 6 {
        return None;
    }
    let count = glyph_count.to_be_bytes();

    // The header's checksum is of the whole file, and is worked out once
    // there is one.
    let mut head = head.to_vec();
    head[8..12].copy_from_slice(&[0; 4]);

    // Every glyph given its own width, so the count of them is the count of
    // glyphs.
    let metrics = usize::from(u16::from_be_bytes([hhea[34], hhea[35]])).max(1);
    let mut hhea = hhea.to_vec();
    hhea[34..36].copy_from_slice(&count);
    let two = |at: usize| hmtx.get(at..at + 2).map_or([0, 0], |bytes| [bytes[0], bytes[1]]);
    let mut widths = Vec::with_capacity(glyphs.len() * 4);
    for glyph in glyphs.iter().map(|glyph| usize::from(*glyph)) {
        widths.extend_from_slice(&two(glyph.min(metrics - 1) * 4));
        let bearing =
            if glyph < metrics { glyph * 4 + 2 } else { metrics * 4 + (glyph - metrics) * 2 };
        widths.extend_from_slice(&two(bearing));
    }

    let mut maxp = maxp.to_vec();
    maxp[4..6].copy_from_slice(&count);

    let mut tables: Vec<([u8; 4], Vec<u8>)> = vec![(*b"CFF ", table)];
    if let Some(os2) = whole.table(b"OS/2") {
        tables.push((*b"OS/2", os2.to_vec()));
    }
    tables.push((*b"head", head));
    tables.push((*b"hhea", hhea));
    tables.push((*b"hmtx", widths));
    tables.push((*b"maxp", maxp));
    // The PostScript table without glyph names, which a CFF font keeps in
    // its own charset: version 3, and the first thirty-two bytes of it.
    if let Some(post) = whole.table(b"post").filter(|post| post.len() >= 32) {
        let mut post = post[..32].to_vec();
        post[..4].copy_from_slice(&0x0003_0000u32.to_be_bytes());
        tables.push((*b"post", post));
    }
    Some(crate::write::assemble(*b"OTTO", &tables))
}

/// The one-byte charstring that ends a glyph having drawn nothing.
const ENDCHAR: u8 = 14;

/// The one-byte subroutine that returns having done nothing.
const RETURN: u8 = 11;

/// An INDEX of subroutines with every one keeping its place, and the ones
/// nobody calls left as a bare return.
fn emptied(index: &Index<'_>, used: impl Fn(u32) -> bool) -> Vec<Vec<u8>> {
    (0..index.count)
        .map(|number| {
            if used(number) {
                index.get(number).unwrap_or(&[RETURN]).to_vec()
            } else {
                vec![RETURN]
            }
        })
        .collect()
}

/// What a subroutine number is counted from, for a list that long: the
/// format numbers them from the middle outwards so that the commonest are
/// reached by the shortest numbers.
fn bias(count: usize) -> i32 {
    if count < 1240 {
        107
    } else if count < 33900 {
        1131
    } else {
        32768
    }
}

// ---------------------------------------------------------------------------
// Following the charstrings
// ---------------------------------------------------------------------------

/// One charstring, by where it lives: a glyph's own, a global subroutine,
/// or a local one of one of the private dictionaries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Body {
    Glyph(u16),
    Global(u32),
    Local(usize, u32),
}

/// One subroutine, by the list it is in and its place in that list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Subr {
    Global(u32),
    Local(usize, u32),
}

/// A number on the stack, and where it was written if it was written as a
/// number rather than worked out.
#[derive(Clone, Copy, Debug)]
struct Value {
    number: f64,
    written: Option<(Body, usize, usize)>,
}

/// A place a subroutine is called from.
#[derive(Clone, Debug, Default)]
struct Site {
    /// The bytes of the number the call takes, where that number was
    /// written immediately in front of it.
    operand: Option<(usize, usize)>,
    /// Every subroutine a run through here called.
    calls: BTreeSet<Subr>,
    /// Whether a run through here called a number the font has no
    /// subroutine for, which the call cannot be rewritten to say.
    stray: bool,
}

/// How a charstring came to an end.
enum Flow {
    /// A subroutine returned, or ran out, and the caller carries on.
    Returned,
    /// The glyph is finished, however deep in its subroutines.
    Ended,
}

/// A charstring this cannot follow: the arithmetic operators, a stack that
/// overflows, a table that ends in the middle of a number.
#[derive(Debug)]
struct Fault;

impl From<Error> for Fault {
    fn from(_: Error) -> Self {
        Self
    }
}

/// The glyphs run so far: every call they made and from where, and every
/// hint mask they skipped and how long it was.
struct Trace<'c, 'a> {
    font: &'c Cff<'a>,
    sites: BTreeMap<Body, BTreeMap<usize, Site>>,
    masks: BTreeMap<(Body, usize), usize>,
    used: BTreeSet<Subr>,
    /// Whether every charstring read the same way each time it ran. A mask's
    /// length is the number of stems declared before it, which a subroutine
    /// shared between glyphs could in principle be run with two of.
    steady: bool,

    // The glyph being run.
    stack: Vec<Value>,
    stems: usize,
    width_seen: bool,
    dictionary: usize,
    parts: Vec<u16>,
}

impl<'c, 'a> Trace<'c, 'a> {
    fn new(font: &'c Cff<'a>) -> Self {
        Self {
            font,
            sites: BTreeMap::new(),
            masks: BTreeMap::new(),
            used: BTreeSet::new(),
            steady: true,
            stack: Vec::with_capacity(cff::MAX_STACK),
            stems: 0,
            width_seen: false,
            dictionary: 0,
            parts: Vec::new(),
        }
    }

    /// Runs one glyph, and says which glyphs it is made of when it is an
    /// accented letter written the old way.
    fn glyph(&mut self, glyph: u16) -> Result<Vec<u16>, Fault> {
        self.stack.clear();
        self.stems = 0;
        self.width_seen = false;
        self.parts.clear();
        self.dictionary = self.font.select.as_ref().map_or(0, |select| select.of(u32::from(glyph)));
        let Some(code) = self.font.charstrings.get(u32::from(glyph)) else {
            return Ok(Vec::new());
        };
        self.run(Body::Glyph(glyph), code, 0)?;
        Ok(core::mem::take(&mut self.parts))
    }

    /// Runs one charstring, following whatever it calls.
    fn run(&mut self, body: Body, code: &[u8], depth: u8) -> Result<Flow, Fault> {
        if depth > cff::MAX_DEPTH {
            return Err(Fault);
        }
        let mut reader = Reader::new(code);
        while reader.remaining() > 0 {
            let at = reader.position();
            let operator = reader.u8()?;
            match operator {
                28 => {
                    let number = f64::from(reader.i16()?);
                    self.push(number, body, at, reader.position())?;
                }
                32..=246 => self.push(f64::from(operator) - 139.0, body, at, at + 1)?,
                247..=250 => {
                    let low = f64::from(reader.u8()?);
                    let number = (f64::from(operator) - 247.0) * 256.0 + low + 108.0;
                    self.push(number, body, at, at + 2)?;
                }
                251..=254 => {
                    let low = f64::from(reader.u8()?);
                    let number = -(f64::from(operator) - 251.0) * 256.0 - low - 108.0;
                    self.push(number, body, at, at + 2)?;
                }
                255 => {
                    let number = f64::from(reader.u32()? as i32) / 65536.0;
                    self.push(number, body, at, at + 5)?;
                }

                // The hints, counted because a mask is as long as they are
                // many.
                1 | 3 | 18 | 23 => self.stems(),
                19 | 20 => {
                    self.stems();
                    let length = self.stems.div_ceil(8);
                    if self.masks.insert((body, at), length).is_some_and(|was| was != length) {
                        self.steady = false;
                    }
                    reader.skip(length)?;
                }

                // Drawing, which is nothing here but a cleared stack.
                4..=8 | 21 | 22 | 24..=27 | 30 | 31 => {
                    self.width_seen = true;
                    self.stack.clear();
                }

                10 | 29 => {
                    if let Flow::Ended = self.call(body, at, operator == 29, depth)? {
                        return Ok(Flow::Ended);
                    }
                }
                11 => return Ok(Flow::Returned),
                14 => {
                    self.end();
                    return Ok(Flow::Ended);
                }
                12 => match reader.u8()? {
                    // The flex family draws; the old dotsection says nothing.
                    0 | 34..=37 => self.stack.clear(),
                    // The arithmetic and the storage: a number worked out
                    // could be the number of a subroutine, and this does not
                    // work numbers out.
                    _ => return Err(Fault),
                },
                _ => return Err(Fault),
            }
        }
        Ok(Flow::Returned)
    }

    /// A call to a subroutine: noted where it was made and what it named,
    /// and then run.
    fn call(&mut self, body: Body, at: usize, global: bool, depth: u8) -> Result<Flow, Fault> {
        let value = self.stack.pop().ok_or(Fault)?;
        let operand = value
            .written
            .filter(|(written, _, end)| *written == body && *end == at)
            .map(|(_, start, end)| (start, end));

        let dictionary = self.dictionary;
        let index = if global {
            Some(self.font.global)
        } else {
            self.font.private.get(dictionary).and_then(|private| private.local)
        };
        let number = index.and_then(|index| {
            let whole = value.number.fract() == 0.0;
            let number = value.number as i64 + i64::from(index.bias());
            (whole && number >= 0 && number < i64::from(index.count)).then_some(number as u32)
        });

        let site = self.sites.entry(body).or_default().entry(at).or_default();
        if !site.calls.is_empty() || site.stray {
            if site.operand != operand {
                self.steady = false;
            }
        } else {
            site.operand = operand;
        }
        let (Some(index), Some(number)) = (index, number) else {
            site.stray = true;
            return Ok(Flow::Returned);
        };
        let (subr, called) = if global {
            (Subr::Global(number), Body::Global(number))
        } else {
            (Subr::Local(dictionary, number), Body::Local(dictionary, number))
        };
        site.calls.insert(subr);
        self.used.insert(subr);
        let code = index.get(number).ok_or(Fault)?;
        self.run(called, code, depth + 1)
    }

    /// The end of the glyph, which is an accented letter where it is given
    /// the four numbers that say so.
    fn end(&mut self) {
        let mut taken = core::mem::take(&mut self.stack);
        // A width in front: one number where there should be none, or five
        // where there should be four — which is how every reader tells.
        if !self.width_seen && (taken.len() == 1 || taken.len() == 5) {
            taken.remove(0);
        }
        self.width_seen = true;
        let [.., _, _, base, accent] = taken[..] else { return };
        let charset = self.font.charset.as_ref();
        for code in [base.number, accent.number] {
            let glyph = cff::standard_sid(code as f32)
                .and_then(|sid| charset.and_then(|charset| charset.glyph_for(sid)));
            if let Some(glyph) = glyph {
                self.parts.push(glyph.0);
            }
        }
    }

    /// Counts a hint operator's stems, and takes the width if it is there.
    fn stems(&mut self) {
        if !self.width_seen && self.stack.len() % 2 == 1 {
            self.stack.remove(0);
        }
        self.width_seen = true;
        self.stems += self.stack.len() / 2;
        self.stack.clear();
    }

    fn push(&mut self, number: f64, body: Body, start: usize, end: usize) -> Result<(), Fault> {
        if self.stack.len() >= cff::MAX_STACK {
            return Err(Fault);
        }
        self.stack.push(Value { number, written: Some((body, start, end)) });
        Ok(())
    }

    /// Whether every call can be written with the new numbers: its number
    /// was written in front of it, it named a subroutine the font has, and
    /// every run through it lands on the same new number.
    fn can_renumber(&self, numbering: &Numbering) -> bool {
        self.steady
            && self.sites.values().flat_map(BTreeMap::values).all(|site| {
                let mut numbers = site.calls.iter().map(|subr| numbering.operand(*subr));
                let first = numbers.next();
                site.operand.is_some()
                    && !site.stray
                    && first.is_some_and(|first| first.is_some() && numbers.all(|n| n == first))
            })
    }

    /// A charstring with every call in it given its new number.
    fn rewritten(&self, body: Body, code: &[u8], numbering: &Numbering) -> Vec<u8> {
        let Some(sites) = self.sites.get(&body) else { return code.to_vec() };
        let mut out = Vec::with_capacity(code.len());
        let mut copied = 0;
        for site in sites.values() {
            let (Some((start, end)), Some(subr)) = (site.operand, site.calls.first()) else {
                continue;
            };
            let Some(number) = numbering.operand(*subr) else { continue };
            out.extend_from_slice(&code[copied..start]);
            out.extend(charstring_number(number));
            copied = end;
        }
        out.extend_from_slice(&code[copied..]);
        out
    }
}

/// The new numbers: the subroutines that were reached, in the order of their
/// old numbers, in each list.
struct Numbering {
    global: Vec<u32>,
    global_at: BTreeMap<u32, usize>,
    local: BTreeMap<usize, Vec<u32>>,
    local_at: BTreeMap<(usize, u32), usize>,
}

impl Numbering {
    fn of(trace: &Trace<'_, '_>) -> Self {
        let mut global = Vec::new();
        let mut local: BTreeMap<usize, Vec<u32>> = BTreeMap::new();
        for subr in &trace.used {
            match *subr {
                Subr::Global(number) => global.push(number),
                Subr::Local(dictionary, number) => {
                    local.entry(dictionary).or_default().push(number)
                }
            }
        }
        let global_at = global.iter().enumerate().map(|(at, number)| (*number, at)).collect();
        let local_at = local
            .iter()
            .flat_map(|(dictionary, numbers)| {
                numbers.iter().enumerate().map(move |(at, number)| ((*dictionary, *number), at))
            })
            .collect();
        Self { global, global_at, local, local_at }
    }

    /// The number a call to this subroutine is written with now: its new
    /// place, counted from the bias of the new list it is in.
    fn operand(&self, subr: Subr) -> Option<i32> {
        match subr {
            Subr::Global(number) => {
                let at = *self.global_at.get(&number)?;
                Some(at as i32 - bias(self.global.len()))
            }
            Subr::Local(dictionary, number) => {
                let at = *self.local_at.get(&(dictionary, number))?;
                let count = self.local.get(&dictionary).map_or(0, Vec::len);
                Some(at as i32 - bias(count))
            }
        }
    }
}

/// A number as a charstring writes it, in as few bytes as it can take.
fn charstring_number(value: i32) -> Vec<u8> {
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
        _ => {
            let [high, low] = (value as i16).to_be_bytes();
            vec![28, high, low]
        }
    }
}

// ---------------------------------------------------------------------------
// Writing the table
// ---------------------------------------------------------------------------

/// One entry of a DICT: its key, its numbers, and the bytes it was written
/// in, which are what an entry this does not rewrite is copied as.
#[derive(Clone, Debug)]
struct Entry<'a> {
    key: u16,
    values: Vec<f64>,
    raw: &'a [u8],
}

/// The keys this writes itself and never copies.
const KEY_ENCODING: u16 = 16;
const KEY_UNIQUE_ID: u16 = 13;
const KEY_XUID: u16 = 14;
const KEY_CHARSTRING_TYPE: u16 = 0x0C06;
const KEY_CID_COUNT: u16 = 0x0C22;
const KEY_UID_BASE: u16 = 0x0C23;

/// The parts of the table that are copied rather than worked out.
struct Layout<'a> {
    name: &'a [u8],
    strings: &'a [u8],
    top: Vec<Entry<'a>>,
    /// The private dictionary of a font with one.
    private: Option<Vec<Entry<'a>>>,
    /// The font dictionaries of a CID-keyed font, each with its private one.
    fonts: Vec<(Vec<Entry<'a>>, Option<Vec<Entry<'a>>>)>,
    cid: bool,
}

impl<'a> Layout<'a> {
    fn read(data: &'a [u8]) -> Option<Self> {
        let header = usize::from(*data.get(2)?);
        let names = Index::parse(data, header, false).ok()?;
        let tops = Index::parse(data, names.end, false).ok()?;
        let strings = Index::parse(data, tops.end, false).ok()?;
        let top = entries(tops.get(0)?)?;

        // Only the charstrings of the kind every CFF writes: the older kind
        // is a different language.
        if find(&top, KEY_CHARSTRING_TYPE).is_some_and(|values| values.first() != Some(&2.0)) {
            return None;
        }
        let cid = find(&top, cff::KEY_ROS).is_some();

        let private = if cid { None } else { private_of(data, &top) };
        let mut fonts = Vec::new();
        if cid {
            let at = *find(&top, cff::KEY_FDARRAY)?.first()?;
            let array = Index::parse(data, at as usize, false).ok()?;
            for number in 0..array.count {
                let dictionary = entries(array.get(number)?)?;
                let private = private_of(data, &dictionary);
                fonts.push((dictionary, private));
            }
        }

        Some(Self {
            name: names.get(0)?,
            strings: data.get(tops.end..strings.end)?,
            top,
            private,
            fonts,
            cid,
        })
    }

    /// The whole table, with the glyphs and subroutines given.
    fn write(
        &self,
        font: &Cff<'_>,
        glyphs: &[u16],
        charstrings: &[Vec<u8>],
        global: &[Vec<u8>],
        locals: &[Vec<Vec<u8>>],
    ) -> Option<Vec<u8>> {
        let name = index(&[self.name.to_vec()]);
        let global = index(global);
        let charstrings = index(charstrings);

        // The glyphs' names, or for a CID-keyed font their CIDs, which are
        // made their places. A font whose names are one of the three lists
        // the format predefines says so by number, and still does.
        let charset_value =
            find(&self.top, cff::KEY_CHARSET).and_then(|values| values.first().copied());
        let predefined = !self.cid && charset_value.is_none_or(|value| value <= 2.0);
        let glyph_count = u16::try_from(glyphs.len()).ok()?;
        let charset = if self.cid {
            charset(glyphs.get(1..).unwrap_or_default().to_vec())
        } else if predefined {
            Vec::new()
        } else {
            charset(sids(font.charset.as_ref()?, glyph_count)?)
        };
        let select = if self.cid { font_select(font, glyphs) } else { Vec::new() };
        // The CIDs run up to the highest place a kept glyph had.
        let cid_count = glyphs.last().map_or(1, |last| usize::from(*last) + 1);

        // The private dictionaries, each followed by its subroutines.
        let privates: Vec<Vec<u8>> = if self.cid {
            self.fonts
                .iter()
                .enumerate()
                .map(|(at, (_, private))| {
                    with_subroutines(private.as_deref(), locals.get(at).map_or(&[], Vec::as_slice))
                })
                .collect()
        } else {
            vec![with_subroutines(
                self.private.as_deref(),
                locals.first().map_or(&[], Vec::as_slice),
            )]
        };
        let private_lengths: Vec<(usize, usize)> = self
            .privates_dictionary_lengths(locals)
            .into_iter()
            .zip(&privates)
            .map(|(dictionary, whole)| (dictionary, whole.len()))
            .collect();

        // Every offset is written in five bytes, so that nothing's length
        // depends on where anything else is and one pass finds every place.
        let places = |start: usize| {
            let charset_at = start;
            let select_at = charset_at + charset.len();
            let charstrings_at = select_at + select.len();
            let after_charstrings = charstrings_at + charstrings.len();
            (charset_at, select_at, charstrings_at, after_charstrings)
        };
        let fonts_length = |privates_at: usize| {
            let mut at = privates_at;
            let mut dictionaries = Vec::new();
            for (number, (dictionary, _)) in self.fonts.iter().enumerate() {
                let (size, whole) = private_lengths.get(number).copied().unwrap_or((0, 0));
                dictionaries.push(font_dictionary(
                    dictionary,
                    size,
                    at,
                    self.fonts[number].1.is_some(),
                ));
                at += whole;
            }
            index(&dictionaries)
        };

        let top_length = self.top_dictionary(0, 0, 0, 0, 0, 0, cid_count, predefined).len();
        let top_index_length = index(&[vec![0; top_length]]).len();
        let start = 4 + name.len() + top_index_length + self.strings.len() + global.len();
        let (charset_at, select_at, charstrings_at, after_charstrings) = places(start);
        let fonts_at = after_charstrings;
        let fonts_index_length = if self.cid { fonts_length(0).len() } else { 0 };
        let privates_at = fonts_at + fonts_index_length;
        let fonts = if self.cid { fonts_length(privates_at) } else { Vec::new() };

        let (private_size, _) = private_lengths.first().copied().unwrap_or((0, 0));
        let top = self.top_dictionary(
            charset_at,
            charstrings_at,
            if self.cid { 0 } else { private_size },
            if self.cid { 0 } else { privates_at },
            fonts_at,
            select_at,
            cid_count,
            predefined,
        );
        debug_assert_eq!(top.len(), top_length);

        let mut out = vec![1, 0, 4, 4];
        out.extend_from_slice(&name);
        out.extend_from_slice(&index(&[top]));
        out.extend_from_slice(self.strings);
        out.extend_from_slice(&global);
        out.extend_from_slice(&charset);
        out.extend_from_slice(&select);
        out.extend_from_slice(&charstrings);
        out.extend_from_slice(&fonts);
        for private in &privates {
            out.extend_from_slice(private);
        }
        Some(out)
    }

    /// How long each private dictionary is, without the subroutines after
    /// it: what the dictionary that points at it gives as its size.
    fn privates_dictionary_lengths(&self, locals: &[Vec<Vec<u8>>]) -> Vec<usize> {
        let one = |private: Option<&[Entry<'_>]>, at: usize| {
            private_dictionary(private, !locals.get(at).is_none_or(Vec::is_empty)).len()
        };
        if self.cid {
            self.fonts
                .iter()
                .enumerate()
                .map(|(at, (_, private))| one(private.as_deref(), at))
                .collect()
        } else {
            vec![one(self.private.as_deref(), 0)]
        }
    }

    /// The top dictionary, with the places given. Everything it says that is
    /// not a place in the table is copied as it was written; the unique
    /// numbers are left out, because a cut font is not the font they name.
    #[allow(clippy::too_many_arguments)]
    fn top_dictionary(
        &self,
        charset_at: usize,
        charstrings_at: usize,
        private_size: usize,
        private_at: usize,
        fonts_at: usize,
        select_at: usize,
        cid_count: usize,
        predefined: bool,
    ) -> Vec<u8> {
        let mut out = Vec::new();
        for entry in &self.top {
            let custom_encoding =
                entry.key == KEY_ENCODING && entry.values.first().is_some_and(|value| *value > 1.0);
            let written_here = matches!(
                entry.key,
                cff::KEY_CHARSTRINGS
                    | cff::KEY_PRIVATE
                    | cff::KEY_FDARRAY
                    | cff::KEY_FDSELECT
                    | KEY_CID_COUNT
                    | KEY_UNIQUE_ID
                    | KEY_XUID
                    | KEY_UID_BASE
            ) || (entry.key == cff::KEY_CHARSET && !predefined);
            if written_here || custom_encoding {
                continue;
            }
            out.extend_from_slice(entry.raw);
        }
        if !predefined || self.cid {
            out.extend(dict_entry(cff::KEY_CHARSET, &[charset_at]));
        }
        out.extend(dict_entry(cff::KEY_CHARSTRINGS, &[charstrings_at]));
        if self.cid {
            out.extend(dict_entry(cff::KEY_FDARRAY, &[fonts_at]));
            out.extend(dict_entry(cff::KEY_FDSELECT, &[select_at]));
            out.extend(dict_entry(KEY_CID_COUNT, &[cid_count]));
        } else if self.private.is_some() {
            out.extend(dict_entry(cff::KEY_PRIVATE, &[private_size, private_at]));
        }
        out
    }
}

/// A font dictionary of a CID-keyed font, pointing at its private one.
fn font_dictionary(entries: &[Entry<'_>], size: usize, at: usize, has_private: bool) -> Vec<u8> {
    let mut out = Vec::new();
    for entry in entries.iter().filter(|entry| entry.key != cff::KEY_PRIVATE) {
        out.extend_from_slice(entry.raw);
    }
    if has_private {
        out.extend(dict_entry(cff::KEY_PRIVATE, &[size, at]));
    }
    out
}

/// A private dictionary, pointing at subroutines that follow it straight
/// away when it has any.
fn private_dictionary(entries: Option<&[Entry<'_>]>, has_subroutines: bool) -> Vec<u8> {
    let Some(entries) = entries else { return Vec::new() };
    let mut out = Vec::new();
    for entry in entries.iter().filter(|entry| entry.key != cff::KEY_SUBRS) {
        out.extend_from_slice(entry.raw);
    }
    if has_subroutines {
        // Where they are is counted from the dictionary itself, and they
        // are straight after it: so the offset is the dictionary's length,
        // this entry included.
        let length = out.len() + dict_entry(cff::KEY_SUBRS, &[0]).len();
        out.extend(dict_entry(cff::KEY_SUBRS, &[length]));
    }
    out
}

/// A private dictionary and the subroutines after it, as they are written.
fn with_subroutines(entries: Option<&[Entry<'_>]>, subroutines: &[Vec<u8>]) -> Vec<u8> {
    if entries.is_none() {
        return Vec::new();
    }
    let mut out = private_dictionary(entries, !subroutines.is_empty());
    if !subroutines.is_empty() {
        out.extend(index(subroutines));
    }
    out
}

/// Which private dictionary each glyph uses, as ranges of glyphs.
fn font_select(font: &Cff<'_>, glyphs: &[u16]) -> Vec<u8> {
    let mut ranges: Vec<(u16, u8)> = Vec::new();
    for (at, glyph) in glyphs.iter().enumerate() {
        let dictionary =
            font.select.as_ref().map_or(0, |select| select.of(u32::from(*glyph))).min(255) as u8;
        if ranges.last().is_none_or(|(_, last)| *last != dictionary) {
            ranges.push((at as u16, dictionary));
        }
    }
    let glyph_count = glyphs.len() as u16;
    let mut out = vec![3];
    out.extend_from_slice(&(ranges.len() as u16).to_be_bytes());
    for (first, dictionary) in ranges {
        out.extend_from_slice(&first.to_be_bytes());
        out.push(dictionary);
    }
    out.extend_from_slice(&glyph_count.to_be_bytes());
    out
}

/// The names — or the CIDs — of every glyph after the first, as runs of
/// consecutive numbers.
fn charset(names: Vec<u16>) -> Vec<u8> {
    let mut out = vec![2];
    let mut at = 0;
    while at < names.len() {
        let first = names[at];
        let mut more = 0usize;
        while at + more + 1 < names.len()
            && more < usize::from(u16::MAX)
            && usize::from(names[at + more + 1]) == usize::from(first) + more + 1
        {
            more += 1;
        }
        out.extend_from_slice(&first.to_be_bytes());
        out.extend_from_slice(&(more as u16).to_be_bytes());
        at += more + 1;
    }
    out
}

/// The name of each glyph after the first, read out of a font's charset.
fn sids(charset: &cff::Charset<'_>, glyph_count: u16) -> Option<Vec<u16>> {
    let wanted = usize::from(glyph_count).saturating_sub(1);
    let mut out = Vec::with_capacity(wanted);
    let read = |at: usize, size: usize| cff::offset(charset.data, at, size).ok();
    match charset.format {
        0 => {
            for glyph in 0..wanted {
                out.push(read(charset.at + 1 + glyph * 2, 2)? as u16);
            }
        }
        1 | 2 => {
            let step = if charset.format == 1 { 3 } else { 4 };
            let mut at = charset.at + 1;
            while out.len() < wanted {
                let first = read(at, 2)?;
                let more = read(at + 2, step - 2)?;
                for name in first..=first + more {
                    if out.len() == wanted {
                        break;
                    }
                    out.push(u16::try_from(name).ok()?);
                }
                at += step;
            }
        }
        _ => return None,
    }
    Some(out)
}

/// The private dictionary a top or font dictionary points at, read into
/// entries.
fn private_of<'a>(data: &'a [u8], dictionary: &[Entry<'a>]) -> Option<Vec<Entry<'a>>> {
    let values = find(dictionary, cff::KEY_PRIVATE)?;
    let [size, at] = values[..] else { return None };
    if size < 0.0 || at < 0.0 {
        return None;
    }
    let (size, at) = (size as usize, at as usize);
    entries(data.get(at..at.checked_add(size)?)?)
}

fn find<'e>(entries: &'e [Entry<'_>], key: u16) -> Option<&'e [f64]> {
    entries.iter().find(|entry| entry.key == key).map(|entry| entry.values.as_slice())
}

/// A DICT read into entries, each with the bytes it was written in.
fn entries(data: &[u8]) -> Option<Vec<Entry<'_>>> {
    let mut out = Vec::new();
    let mut values = Vec::new();
    let mut start = 0;
    let mut reader = Reader::new(data);
    while reader.remaining() > 0 {
        let first = reader.u8().ok()?;
        match first {
            0..=27 => {
                let key = if first == 12 {
                    0x0C00 | u16::from(reader.u8().ok()?)
                } else {
                    u16::from(first)
                };
                let end = reader.position();
                out.push(Entry {
                    key,
                    values: core::mem::take(&mut values),
                    raw: &data[start..end],
                });
                start = end;
            }
            28 => values.push(f64::from(reader.i16().ok()?)),
            29 => values.push(f64::from(reader.u32().ok()? as i32)),
            30 => values.push(cff::real(&mut reader).ok()?),
            32..=246 => values.push(f64::from(first) - 139.0),
            247..=250 => {
                let low = f64::from(reader.u8().ok()?);
                values.push((f64::from(first) - 247.0) * 256.0 + low + 108.0);
            }
            251..=254 => {
                let low = f64::from(reader.u8().ok()?);
                values.push(-(f64::from(first) - 251.0) * 256.0 - low - 108.0);
            }
            _ => return None,
        }
    }
    Some(out)
}

/// One DICT entry of offsets and counts, each written in the five-byte form
/// so that its length does not depend on its value.
fn dict_entry(key: u16, values: &[usize]) -> Vec<u8> {
    let mut out = Vec::with_capacity(values.len() * 5 + 2);
    for value in values {
        out.push(29);
        out.extend_from_slice(&(*value as i32).to_be_bytes());
    }
    if key >= 0x0C00 {
        out.push(12);
        out.push((key & 0xFF) as u8);
    } else {
        out.push(key as u8);
    }
    out
}

/// An INDEX, with offsets as narrow as the data allows.
fn index(entries: &[Vec<u8>]) -> Vec<u8> {
    if entries.is_empty() {
        return vec![0, 0];
    }
    let total: usize = entries.iter().map(Vec::len).sum();
    let last = total + 1;
    let size = if last <= 0xFF {
        1
    } else if last <= 0xFFFF {
        2
    } else if last <= 0xFF_FFFF {
        3
    } else {
        4
    };
    let mut out = Vec::with_capacity(3 + (entries.len() + 1) * size + total);
    out.extend_from_slice(&(entries.len() as u16).to_be_bytes());
    out.push(size as u8);
    let mut at = 1usize;
    let write = |out: &mut Vec<u8>, value: usize| {
        out.extend_from_slice(&(value as u32).to_be_bytes()[4 - size..]);
    };
    write(&mut out, at);
    for entry in entries {
        at += entry.len();
        write(&mut out, at);
    }
    for entry in entries {
        out.extend_from_slice(entry);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::glyf::Outline;
    use crate::GlyphId;

    /// A number as a charstring writes it.
    fn n(value: i32) -> Vec<u8> {
        charstring_number(value)
    }

    /// A charstring put together from pieces.
    fn code(pieces: &[&[u8]]) -> Vec<u8> {
        pieces.concat()
    }

    /// `x y rmoveto`, the start of every glyph here.
    fn start(x: i32, y: i32) -> Vec<u8> {
        code(&[&n(x), &n(y), &[21]])
    }

    /// A subroutine drawing one line and returning.
    fn line(dx: i32, dy: i32) -> Vec<u8> {
        code(&[&n(dx), &n(dy), &[5, RETURN]])
    }

    /// A call to a subroutine of a list this long, by its place in the list.
    fn call(number: i32, count: usize, global: bool) -> Vec<u8> {
        code(&[&n(number - bias(count)), &[if global { 29 } else { 10 }]])
    }

    /// A name-keyed table: its glyphs, its global and local subroutines, and
    /// the name of each glyph after the first.
    fn plain(
        charstrings: &[Vec<u8>],
        global: &[Vec<u8>],
        local: &[Vec<u8>],
        names: &[u16],
    ) -> Vec<u8> {
        let name = index(&[b"Test".to_vec()]);
        let strings = index(&[]);
        let global = index(global);
        let mut charset = vec![0u8];
        for sid in names {
            charset.extend_from_slice(&sid.to_be_bytes());
        }
        let charstrings = index(charstrings);
        let mut private = private_dictionary(Some(&[]), !local.is_empty());
        let private_size = private.len();
        if !local.is_empty() {
            private.extend(index(local));
        }
        let top = |charset_at: usize, charstrings_at: usize, private_at: usize| {
            let mut out = dict_entry(cff::KEY_CHARSET, &[charset_at]);
            out.extend(dict_entry(cff::KEY_CHARSTRINGS, &[charstrings_at]));
            out.extend(dict_entry(cff::KEY_PRIVATE, &[private_size, private_at]));
            out
        };
        let top_length = index(&[top(0, 0, 0)]).len();
        let charset_at = 4 + name.len() + top_length + strings.len() + global.len();
        let charstrings_at = charset_at + charset.len();
        let private_at = charstrings_at + charstrings.len();
        let mut out = vec![1, 0, 4, 4];
        for part in [
            name,
            index(&[top(charset_at, charstrings_at, private_at)]),
            strings,
            global,
            charset,
            charstrings,
            private,
        ] {
            out.extend(part);
        }
        out
    }

    /// A CID-keyed table: its glyphs, the CID of each after the first, which
    /// of the two dictionaries each glyph uses, the global subroutines, and
    /// the local ones of each dictionary.
    fn keyed(
        charstrings: &[Vec<u8>],
        cids: &[u16],
        dictionaries: &[u8],
        global: &[Vec<u8>],
        locals: [&[Vec<u8>]; 2],
    ) -> Vec<u8> {
        let name = index(&[b"Keyed".to_vec()]);
        // The registry and the ordering, which are the first two strings.
        let strings = index(&[b"Adobe".to_vec(), b"Identity".to_vec()]);
        let global = index(global);
        let mut charset = vec![0u8];
        for cid in cids {
            charset.extend_from_slice(&cid.to_be_bytes());
        }
        let mut select = vec![0u8];
        select.extend_from_slice(dictionaries);
        let charstrings = index(charstrings);
        let privates: Vec<(usize, Vec<u8>)> = locals
            .iter()
            .map(|local| {
                let mut private = private_dictionary(Some(&[]), !local.is_empty());
                let size = private.len();
                if !local.is_empty() {
                    private.extend(index(local));
                }
                (size, private)
            })
            .collect();
        let fonts = |at: usize| {
            let mut at = at;
            let mut dictionaries = Vec::new();
            for (size, whole) in &privates {
                dictionaries.push(dict_entry(cff::KEY_PRIVATE, &[*size, at]));
                at += whole.len();
            }
            index(&dictionaries)
        };
        let top = |charset_at: usize, select_at: usize, charstrings_at: usize, fonts_at: usize| {
            let mut out = dict_entry(cff::KEY_ROS, &[391, 392, 0]);
            out.extend(dict_entry(cff::KEY_CHARSET, &[charset_at]));
            out.extend(dict_entry(cff::KEY_FDSELECT, &[select_at]));
            out.extend(dict_entry(cff::KEY_CHARSTRINGS, &[charstrings_at]));
            out.extend(dict_entry(cff::KEY_FDARRAY, &[fonts_at]));
            out
        };
        let top_length = index(&[top(0, 0, 0, 0)]).len();
        let charset_at = 4 + name.len() + top_length + strings.len() + global.len();
        let select_at = charset_at + charset.len();
        let charstrings_at = select_at + select.len();
        let fonts_at = charstrings_at + charstrings.len();
        let privates_at = fonts_at + fonts(0).len();
        let mut out = vec![1, 0, 4, 4];
        for part in [
            name,
            index(&[top(charset_at, select_at, charstrings_at, fonts_at)]),
            strings,
            global,
            charset,
            select,
            charstrings,
            fonts(privates_at),
        ] {
            out.extend(part);
        }
        for (_, private) in privates {
            out.extend(private);
        }
        out
    }

    fn outline(data: &[u8], glyph: u16) -> Option<Outline> {
        Cff::parse(data).expect("a table").outline(GlyphId(glyph), &[]).expect("a glyph")
    }

    fn set(glyphs: &[u16]) -> BTreeSet<u16> {
        glyphs.iter().copied().collect()
    }

    /// A glyph that reaches past three local subroutines it does not use to
    /// call the fourth, and calls the second of two global ones as well.
    fn reaching() -> Vec<u8> {
        let local = [line(10, 0), line(20, 0), line(30, 0), line(100, 0)];
        let global = [line(0, 7), line(0, 100)];
        let glyph = code(&[
            &start(50, 50),
            &call(3, 4, false),
            &call(1, 2, true),
            &n(-100),
            &n(0),
            &[5, ENDCHAR],
        ]);
        let other = code(&[&start(0, 0), &call(0, 4, false), &[ENDCHAR]]);
        plain(&[vec![ENDCHAR], other, glyph], &global, &local, &[34, 35])
    }

    #[test]
    fn only_the_subroutines_reached_are_kept_and_the_calls_say_their_new_numbers() {
        let data = reaching();
        let cut = cut_table(&data, &set(&[2])).expect("a table");
        assert_eq!(cut.subroutines, Subroutines::Renumbered);
        assert_eq!(cut.glyphs, vec![0, 1, 2]);

        let table = Cff::parse(&cut.table).expect("the cut table reads");
        assert_eq!(table.global.count, 1, "the global one nobody calls is gone");
        let locals = table.private[0].local.expect("locals");
        assert_eq!(locals.count, 1);
        // The one kept is the one that was called: it is first in its list
        // now, and the call says so.
        assert_eq!(locals.get(0), Some(&line(100, 0)[..]));
        assert_eq!(outline(&cut.table, 2), outline(&data, 2));
        // The glyph not asked for is still there, drawing nothing.
        assert_eq!(outline(&cut.table, 1), None);
    }

    #[test]
    fn an_accented_letter_brings_its_letter_and_its_accent() {
        // Glyph 1 is A and glyph 2 the acute, by their standard names; glyph 3
        // is written as nothing but the two of them, with no width and no
        // hints in front — four numbers and endchar.
        let letter = code(&[&start(0, 0), &n(100), &n(100), &[5, ENDCHAR]]);
        let accent = code(&[&start(40, 0), &n(10), &n(20), &[5, ENDCHAR]]);
        let accented = code(&[&n(0), &n(300), &n(65), &n(194), &[ENDCHAR]]);
        let data = plain(&[vec![ENDCHAR], letter, accent, accented], &[], &[], &[34, 125, 203]);

        // Drawn, it is both — which it was not while the first of the four
        // numbers was taken for a width.
        let drawn = outline(&data, 3).expect("the accented letter draws");
        assert!(drawn.bounds.max_y >= 300, "the accent is not over the letter: {:?}", drawn.bounds);

        let cut = cut_table(&data, &set(&[3])).expect("a table");
        assert_eq!(cut.kept, set(&[0, 1, 2, 3]));
        assert_eq!(outline(&cut.table, 3), outline(&data, 3));
    }

    #[test]
    fn a_call_whose_number_was_not_written_in_front_of_it_keeps_every_number() {
        // The first call's number is written in front of it; the second's is
        // the number under it on the stack, pushed before the first call — a
        // subroutinizer never writes that, and this does not rewrite it.
        let local = [line(0, 5), vec![RETURN], line(9, 9)];
        let glyph = code(&[&start(10, 10), &n(-107), &n(-106), &[10, 10, ENDCHAR]]);
        let data = plain(&[vec![ENDCHAR], glyph], &[], &local, &[34]);
        let cut = cut_table(&data, &set(&[1])).expect("a table");
        assert_eq!(cut.subroutines, Subroutines::Emptied);

        let table = Cff::parse(&cut.table).unwrap();
        let locals = table.private[0].local.expect("locals");
        assert_eq!(locals.count, 3, "every subroutine keeps its place");
        assert_eq!(locals.get(2), Some(&[RETURN][..]), "the one nobody calls is emptied");
        assert_eq!(outline(&cut.table, 1), outline(&data, 1));
    }

    #[test]
    fn a_cid_keyed_font_keeps_only_its_glyphs_and_names_each_by_its_old_place() {
        // Five glyphs whose CIDs are not their places, in two dictionaries,
        // each with local subroutines, and one global subroutine that calls
        // a local one of whichever dictionary the glyph is in.
        let first = [line(1, 0), line(100, 0)];
        let second = [line(0, 3), line(0, 100)];
        let global = [code(&[&call(1, 2, false), &[RETURN]])];
        let glyph = |x: i32| code(&[&start(x, 0), &call(0, 1, true), &[ENDCHAR]]);
        let charstrings = [vec![ENDCHAR], glyph(1), glyph(2), glyph(3), glyph(4)];
        let data =
            keyed(&charstrings, &[10, 20, 30, 40], &[0, 0, 0, 1, 1], &global, [&first, &second]);

        let cut = cut_table(&data, &set(&[1, 3])).expect("a table");
        assert_eq!(cut.subroutines, Subroutines::Renumbered);
        assert_eq!(cut.glyphs, vec![0, 1, 3]);

        let table = Cff::parse(&cut.table).expect("the cut table reads");
        assert!(table.cid);
        assert_eq!(table.charstrings.count, 3);
        let charset = table.charset.expect("a table of CIDs");
        // The page names the glyph that was fourth by 3, and finds it third.
        assert_eq!(charset.glyph_for(1), Some(GlyphId(1)));
        assert_eq!(charset.glyph_for(3), Some(GlyphId(2)));
        assert_eq!(charset.glyph_for(2), None);
        let select = table.select.expect("which dictionary each glyph is in");
        assert_eq!((select.of(1), select.of(2)), (0, 1));
        // Each dictionary keeps the one local subroutine it was reached for.
        assert_eq!(table.private[0].local.unwrap().count, 1);
        assert_eq!(table.private[1].local.unwrap().count, 1);
        assert_eq!(outline(&cut.table, 1), outline(&data, 1));
        assert_eq!(outline(&cut.table, 2), outline(&data, 3));
    }

    #[test]
    fn a_global_call_that_would_mean_two_new_numbers_keeps_every_number() {
        // The global subroutine calls local subroutine one of whichever
        // dictionary the glyph is in. The first dictionary's glyph also calls
        // its local nought, so there its one stays second; in the second
        // dictionary it would become first. One call cannot say both.
        let first = [line(1, 0), line(100, 0)];
        let second = [line(0, 3), line(0, 100)];
        let global = [code(&[&call(1, 2, false), &[RETURN]])];
        let one = code(&[&start(1, 0), &call(0, 2, false), &call(0, 1, true), &[ENDCHAR]]);
        let two = code(&[&start(2, 0), &call(0, 1, true), &[ENDCHAR]]);
        let data =
            keyed(&[vec![ENDCHAR], one, two], &[1, 2], &[0, 0, 1], &global, [&first, &second]);

        let cut = cut_table(&data, &set(&[1, 2])).expect("a table");
        assert_eq!(cut.subroutines, Subroutines::Emptied);
        assert_eq!(outline(&cut.table, 1), outline(&data, 1));
        assert_eq!(outline(&cut.table, 2), outline(&data, 2));
    }

    #[test]
    fn charstrings_of_the_older_kind_are_not_cut() {
        // The same table with a top dictionary that says Type 1 charstrings,
        // put in front of what it said: the cut gives up on the key before it
        // reads anything the key moved.
        let data = reaching();
        let header = 4 + index(&[b"Test".to_vec()]).len();
        let tops = Index::parse(&data, header, false).unwrap();
        let mut top = dict_entry(KEY_CHARSTRING_TYPE, &[1]);
        top.extend_from_slice(tops.get(0).unwrap());
        let mut rebuilt = data[..header].to_vec();
        rebuilt.extend(index(&[top]));
        rebuilt.extend_from_slice(&data[tops.end..]);
        assert!(cut_table(&rebuilt, &set(&[2])).is_none());
    }

    #[test]
    fn a_truncated_table_is_refused_rather_than_guessed_at() {
        let data = reaching();
        for length in 0..data.len() {
            let _ = cut_table(&data[..length], &set(&[2]));
        }
    }
}
