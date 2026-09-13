//! Reading the glyph positioning table, to the OpenType specification.
//!
//! # What positioning is for
//!
//! Substitution says which glyph; this says where it goes. Two things depend
//! on it and neither is decoration.
//!
//! The first is kerning. Every font written this century keeps it here, in a
//! pair-positioning lookup, and the old `kern` table it replaced is usually
//! absent — so a reader that skips this one sets everything unkerned however
//! much trouble the designer took.
//!
//! The second is where a mark goes. A combining accent is drawn without moving
//! the pen, so left to itself it lands at the right-hand edge of whatever came
//! before it. Where it actually belongs is said here, as a pair of anchors: a
//! point on the letter and a point on the mark, which are to be brought
//! together. Without that a document in any language that writes accents
//! separately is drawn with the accents beside the letters rather than on
//! them.
//!
//! # What is read
//!
//! Single and pair adjustment, the two kinds of mark attachment — onto a
//! letter and onto another mark — the one that attaches to a piece of a
//! ligature, and the extension lookups that large fonts hide the rest behind.
//! Cursive attachment and the contextual kinds are not read yet; see the
//! roadmap.
//!
//! # The unit
//!
//! Font design units, the same as the advances. What the layout does with them
//! is the layout's arithmetic, which already divides by the units per em.

use wp_font::GlyphId;

use crate::common::{i16_at, u16_at, u32_at, Tables};
use crate::gdef::{Definitions, Kind};

/// Where a glyph goes, beside where the advances would have put it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Placement {
    /// In font design units, positive to the right and upwards.
    pub x_offset: i32,
    pub y_offset: i32,
    /// What to add to the glyph's own width before the pen moves on.
    pub x_advance: i32,
}

impl Placement {
    /// Whether this says anything at all.
    #[must_use]
    pub fn is_nothing(&self) -> bool {
        *self == Self::default()
    }
}

/// A run being positioned: what the glyphs are, how wide each is, and where
/// each has been moved to so far.
#[derive(Debug)]
pub struct Run<'a> {
    pub glyphs: &'a [GlyphId],
    /// Each glyph's own width in font units, before any adjustment.
    pub advances: &'a [i32],
    pub placements: &'a mut [Placement],
}

impl Run<'_> {
    /// How far the pen travels from the start of one glyph to the start of
    /// another, counting what positioning has already added.
    fn distance(&self, from: usize, to: usize) -> i32 {
        (from..to)
            .map(|at| {
                self.advances.get(at).copied().unwrap_or(0)
                    + self.placements.get(at).map_or(0, |placement| placement.x_advance)
            })
            .sum()
    }
}

/// A parsed positioning table, ready to be asked for features.
#[derive(Clone, Copy, Debug)]
pub struct Positions<'a> {
    tables: Tables<'a>,
}

impl<'a> Positions<'a> {
    /// Reads the header of a `GPOS` table.
    #[must_use]
    pub fn parse(data: &'a [u8]) -> Option<Self> {
        Tables::parse(data).map(|tables| Self { tables })
    }

    /// The lookups a feature uses, for a given script.
    #[must_use]
    pub fn lookups_for(&self, script: &[u8; 4], feature: &[u8; 4]) -> Vec<usize> {
        self.tables.lookups_for(script, feature)
    }

    /// Whether the font says anything under a script at all.
    #[must_use]
    pub fn has_script(&self, script: &[u8; 4]) -> bool {
        self.tables.has_script(script)
    }

    /// Applies one lookup to a run, in place.
    pub fn apply(&self, index: usize, run: &mut Run<'_>, definitions: Option<&Definitions<'_>>) {
        let Some((kind, flags, subtables)) = self.tables.lookup(index) else {
            return;
        };
        let skip = Skip::of(flags, definitions);
        for at in subtables {
            self.apply_subtable(kind, at, run, &skip, definitions);
        }
    }

    fn apply_subtable(
        &self,
        kind: u16,
        at: usize,
        run: &mut Run<'_>,
        skip: &Skip<'_>,
        definitions: Option<&Definitions<'_>>,
    ) {
        // The definitions travel with the call because an extension lookup
        // has to hand them on to whatever it points at.
        let _ = &definitions;
        match kind {
            1 => self.apply_single(at, run, skip),
            2 => self.apply_pair(at, run, skip),
            4..=6 => self.apply_attachment(kind, at, run, skip),
            // A lookup that points at another table, the same as in the
            // substitution table: what it names is an ordinary subtable.
            9 => {
                let Some(real) = u16_at(self.tables.data, at + 2) else { return };
                let Some(offset) = u32_at(self.tables.data, at + 4) else { return };
                if real == 9 {
                    return;
                }
                self.apply_subtable(real, at + offset as usize, run, skip, definitions);
            }
            // Cursive attachment, and the contextual kinds, which have their
            // own copies of the rules the substitution table carries. Named in
            // the roadmap.
            _ => {}
        }
    }

    /// One glyph moved, wherever it appears.
    fn apply_single(&self, at: usize, run: &mut Run<'_>, skip: &Skip<'_>) {
        let data = self.tables.data;
        let Some(format) = u16_at(data, at) else { return };
        let Some(coverage) = u16_at(data, at + 2) else { return };
        let coverage = at + usize::from(coverage);
        let Some(kinds) = u16_at(data, at + 4) else { return };

        for index in 0..run.glyphs.len() {
            if skip.passes_over(run.glyphs[index]) {
                continue;
            }
            let Some(covered) = self.tables.covered(coverage, run.glyphs[index]) else {
                continue;
            };
            let value = match format {
                // One value for everything the coverage names.
                1 => at + 6,
                // One value each, in coverage order.
                2 => at + 8 + covered * size_of_value(kinds),
                _ => continue,
            };
            let Some(placement) = run.placements.get_mut(index) else { continue };
            add_value(data, value, kinds, placement);
        }
    }

    /// Two glyphs moved with respect to each other, which is kerning.
    fn apply_pair(&self, at: usize, run: &mut Run<'_>, skip: &Skip<'_>) {
        let data = self.tables.data;
        let Some(format) = u16_at(data, at) else { return };
        let Some(coverage) = u16_at(data, at + 2) else { return };
        let coverage = at + usize::from(coverage);
        let (Some(first_kinds), Some(second_kinds)) = (u16_at(data, at + 4), u16_at(data, at + 6))
        else {
            return;
        };

        let mut index = 0usize;
        while index < run.glyphs.len() {
            let Some(next) = skip.next_after(run, index) else { break };
            let Some(covered) = self.tables.covered(coverage, run.glyphs[index]) else {
                index = next;
                continue;
            };

            let values = match format {
                1 => self.pair_by_glyph(at, covered, run.glyphs[next], first_kinds, second_kinds),
                2 => self.pair_by_class(
                    at,
                    run.glyphs[index],
                    run.glyphs[next],
                    first_kinds,
                    second_kinds,
                ),
                _ => None,
            };
            let Some((first_at, second_at)) = values else {
                index = next;
                continue;
            };

            if let Some(placement) = run.placements.get_mut(index) {
                add_value(data, first_at, first_kinds, placement);
            }
            if second_kinds != 0 {
                if let Some(placement) = run.placements.get_mut(next) {
                    add_value(data, second_at, second_kinds, placement);
                }
            }
            // The second glyph of a pair may begin the next one, which is what
            // makes a run of three letters kern twice.
            index = next;
        }
    }

    /// A pair written out glyph by glyph.
    fn pair_by_glyph(
        &self,
        at: usize,
        covered: usize,
        second: GlyphId,
        first_kinds: u16,
        second_kinds: u16,
    ) -> Option<(usize, usize)> {
        let data = self.tables.data;
        let sets = usize::from(u16_at(data, at + 8)?);
        if covered >= sets {
            return None;
        }
        let set = at + usize::from(u16_at(data, at + 10 + covered * 2)?);
        let count = usize::from(u16_at(data, set)?);
        let step = 2 + size_of_value(first_kinds) + size_of_value(second_kinds);

        for index in 0..count {
            let entry = set + 2 + index * step;
            if u16_at(data, entry)? != second.0 {
                continue;
            }
            return Some((entry + 2, entry + 2 + size_of_value(first_kinds)));
        }
        None
    }

    /// A pair written as one rule for every pair of classes, which is how a
    /// font of a few thousand kerning pairs is written in a few hundred bytes.
    fn pair_by_class(
        &self,
        at: usize,
        first: GlyphId,
        second: GlyphId,
        first_kinds: u16,
        second_kinds: u16,
    ) -> Option<(usize, usize)> {
        let data = self.tables.data;
        let first_classes = at + usize::from(u16_at(data, at + 8)?);
        let second_classes = at + usize::from(u16_at(data, at + 10)?);
        let first_count = usize::from(u16_at(data, at + 12)?);
        let second_count = usize::from(u16_at(data, at + 14)?);

        let first_class = usize::from(self.tables.class_of(first_classes, first));
        let second_class = usize::from(self.tables.class_of(second_classes, second));
        if first_class >= first_count || second_class >= second_count {
            return None;
        }

        let step = size_of_value(first_kinds) + size_of_value(second_kinds);
        let entry = at + 16 + (first_class * second_count + second_class) * step;
        Some((entry, entry + size_of_value(first_kinds)))
    }

    /// A mark brought onto whatever it belongs to.
    ///
    /// Three lookups in one, because they differ only in what the mark is
    /// being attached to: a letter, a piece of a ligature, or another mark.
    fn apply_attachment(&self, kind: u16, at: usize, run: &mut Run<'_>, skip: &Skip<'_>) {
        let data = self.tables.data;
        if u16_at(data, at) != Some(1) {
            return;
        }
        let Some(marks_coverage) = u16_at(data, at + 2) else { return };
        let Some(bases_coverage) = u16_at(data, at + 4) else { return };
        let marks_coverage = at + usize::from(marks_coverage);
        let bases_coverage = at + usize::from(bases_coverage);
        let Some(classes) = u16_at(data, at + 6) else { return };
        let classes = usize::from(classes);
        let Some(marks) = u16_at(data, at + 8) else { return };
        let Some(bases) = u16_at(data, at + 10) else { return };
        let marks = at + usize::from(marks);
        let bases = at + usize::from(bases);

        for index in 0..run.glyphs.len() {
            let Some(mark) = self.tables.covered(marks_coverage, run.glyphs[index]) else {
                continue;
            };
            // What it attaches to: for a mark onto a mark, the glyph before
            // it whatever that is; otherwise the last one that is not a mark.
            let Some(onto) =
                (if kind == 6 { index.checked_sub(1) } else { skip.base_before(run, index) })
            else {
                continue;
            };
            let Some(base) = self.tables.covered(bases_coverage, run.glyphs[onto]) else {
                continue;
            };

            // The mark's own anchor, and which of the font's groups of marks
            // it belongs to: a letter carries one anchor per group.
            let Some(class) = u16_at(data, marks + 2 + mark * 4) else { continue };
            let Some(mark_anchor) = u16_at(data, marks + 4 + mark * 4) else { continue };
            let mark_anchor = marks + usize::from(mark_anchor);
            if usize::from(class) >= classes {
                continue;
            }

            // Where on the base that group of marks goes. A ligature keeps one
            // set of anchors per piece of it; with nothing saying which piece
            // this mark belongs to, the first is the closest true answer.
            let base_anchor = if kind == 5 {
                let Some(offset) = u16_at(data, bases + 2 + base * 2) else { continue };
                let piece = bases + usize::from(offset);
                let Some(anchor) = u16_at(data, piece + usize::from(class) * 2) else { continue };
                piece + usize::from(anchor)
            } else {
                let Some(anchor) =
                    u16_at(data, bases + 2 + (base * classes + usize::from(class)) * 2)
                else {
                    continue;
                };
                if anchor == 0 {
                    continue;
                }
                bases + usize::from(anchor)
            };

            let (Some(mark_point), Some(base_point)) =
                (anchor_at(data, mark_anchor), anchor_at(data, base_anchor))
            else {
                continue;
            };

            // The mark is drawn where the pen has already travelled to, so
            // what is left to do is take back that travel and then put the
            // mark's own point on the base's.
            let travelled = run.distance(onto, index);
            let base_moved = run.placements.get(onto).copied().unwrap_or_default();
            if let Some(placement) = run.placements.get_mut(index) {
                placement.x_offset = base_moved.x_offset + base_point.0 - mark_point.0 - travelled;
                placement.y_offset = base_moved.y_offset + base_point.1 - mark_point.1;
            }
        }
    }
}

/// What a lookup passes over while it matches.
struct Skip<'a> {
    marks: bool,
    bases: bool,
    ligatures: bool,
    /// The one group of marks a lookup keeps when it passes over the rest.
    only_group: u16,
    definitions: Option<&'a Definitions<'a>>,
}

impl<'a> Skip<'a> {
    fn of(flags: u16, definitions: Option<&'a Definitions<'a>>) -> Self {
        Self {
            bases: flags & 0x0002 != 0,
            ligatures: flags & 0x0004 != 0,
            marks: flags & 0x0008 != 0,
            only_group: flags >> 8,
            definitions,
        }
    }

    /// Whether a glyph is one this lookup does not look at.
    fn passes_over(&self, glyph: GlyphId) -> bool {
        let Some(definitions) = self.definitions else { return false };
        let kind = definitions.kind(glyph);
        if self.marks && kind == Kind::Mark {
            return true;
        }
        if self.bases && kind == Kind::Base {
            return true;
        }
        if self.ligatures && kind == Kind::Ligature {
            return true;
        }
        // A lookup that keeps one group of marks passes over every other mark.
        if self.only_group != 0 && kind == Kind::Mark {
            return definitions.mark_class(glyph) != self.only_group;
        }
        false
    }

    /// The next glyph this lookup looks at, after one.
    fn next_after(&self, run: &Run<'_>, index: usize) -> Option<usize> {
        ((index + 1)..run.glyphs.len()).find(|at| !self.passes_over(run.glyphs[*at]))
    }

    /// The letter a mark belongs to: the nearest thing before it that is not
    /// itself a mark.
    ///
    /// Without the font's own definitions the only evidence is the width. A
    /// mark is drawn without moving the pen, so a glyph of no width is taken
    /// for one — a guess, and the reason the definitions are worth reading.
    fn base_before(&self, run: &Run<'_>, index: usize) -> Option<usize> {
        (0..index).rev().find(|at| match self.definitions {
            Some(definitions) => definitions.kind(run.glyphs[*at]) != Kind::Mark,
            None => run.advances.get(*at).copied().unwrap_or(0) != 0,
        })
    }
}

/// How many bytes a value record takes, which is one per thing it says.
fn size_of_value(kinds: u16) -> usize {
    usize::from(kinds.count_ones() as u16) * 2
}

/// Adds what a value record says to where a glyph goes.
///
/// The four that move the glyph are read; the four that point at tables of
/// per-size adjustments are counted and passed over, because what they hold is
/// a correction for a screen of a given number of dots to the inch and this
/// program draws at whatever size it is asked for.
fn add_value(data: &[u8], at: usize, kinds: u16, placement: &mut Placement) {
    let mut walk = at;
    let mut next = move || {
        let value = i16_at(data, walk).unwrap_or(0);
        walk += 2;
        i32::from(value)
    };

    if kinds & 0x0001 != 0 {
        placement.x_offset += next();
    }
    if kinds & 0x0002 != 0 {
        placement.y_offset += next();
    }
    if kinds & 0x0004 != 0 {
        placement.x_advance += next();
    }
    if kinds & 0x0008 != 0 {
        // The vertical advance, which matters only to text set downwards.
        let _ = next();
    }
}

/// A point on a glyph, in font units.
///
/// Three ways of writing one: a point, a point that also names an outline
/// point to snap to, and a point with per-size corrections. All three begin
/// with the coordinates, which is the whole of what is read.
fn anchor_at(data: &[u8], at: usize) -> Option<(i32, i32)> {
    let format = u16_at(data, at)?;
    if format == 0 || format > 3 {
        return None;
    }
    Some((i32::from(i16_at(data, at + 2)?), i32::from(i16_at(data, at + 4)?)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn be(value: u16) -> [u8; 2] {
        value.to_be_bytes()
    }

    fn signed(value: i16) -> [u8; 2] {
        value.to_be_bytes()
    }

    fn coverage(glyphs: &[u16]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&be(1));
        out.extend_from_slice(&be(glyphs.len() as u16));
        for glyph in glyphs {
            out.extend_from_slice(&be(*glyph));
        }
        out
    }

    /// A whole positioning table: one script, one feature, one lookup.
    fn table(kind: u16, flags: u16, subtable: &[u8]) -> Vec<u8> {
        let scripts = 10usize;
        let features = scripts + 20;
        let lookups = features + 14;

        let mut data = Vec::new();
        data.extend_from_slice(&be(1));
        data.extend_from_slice(&be(0));
        data.extend_from_slice(&be(scripts as u16));
        data.extend_from_slice(&be(features as u16));
        data.extend_from_slice(&be(lookups as u16));

        data.extend_from_slice(&be(1));
        data.extend_from_slice(b"latn");
        data.extend_from_slice(&be(8));
        data.extend_from_slice(&be(4));
        data.extend_from_slice(&be(0));
        data.extend_from_slice(&be(0));
        data.extend_from_slice(&be(0xFFFF));
        data.extend_from_slice(&be(1));
        data.extend_from_slice(&be(0));

        data.extend_from_slice(&be(1));
        data.extend_from_slice(b"kern");
        data.extend_from_slice(&be(8));
        data.extend_from_slice(&be(0));
        data.extend_from_slice(&be(1));
        data.extend_from_slice(&be(0));

        data.extend_from_slice(&be(1));
        data.extend_from_slice(&be(4));
        data.extend_from_slice(&be(kind));
        data.extend_from_slice(&be(flags));
        data.extend_from_slice(&be(1));
        data.extend_from_slice(&be(8));
        data.extend_from_slice(subtable);
        data
    }

    /// Runs the one lookup over a run of glyphs, each as wide as it says.
    fn run(data: &[u8], glyphs: &[(u16, i32)], definitions: Option<&[u8]>) -> Vec<Placement> {
        let table = Positions::parse(data).expect("the table should parse");
        let ids: Vec<GlyphId> = glyphs.iter().map(|(glyph, _)| GlyphId(*glyph)).collect();
        let advances: Vec<i32> = glyphs.iter().map(|(_, advance)| *advance).collect();
        let mut placements = vec![Placement::default(); ids.len()];
        let mut run = Run { glyphs: &ids, advances: &advances, placements: &mut placements };
        let definitions = definitions.and_then(Definitions::parse);
        for lookup in table.lookups_for(b"latn", b"kern") {
            table.apply(lookup, &mut run, definitions.as_ref());
        }
        placements
    }

    #[test]
    fn one_glyph_moved_wherever_it_appears() {
        let mut subtable = Vec::new();
        subtable.extend_from_slice(&be(1)); // one value for everything covered
        subtable.extend_from_slice(&be(10)); // the coverage, after the value
        subtable.extend_from_slice(&be(0x0005)); // an x placement and an x advance
        subtable.extend_from_slice(&signed(-20));
        subtable.extend_from_slice(&signed(-40));
        subtable.extend_from_slice(&coverage(&[7]));

        let data = table(1, 0, &subtable);
        let placements = run(&data, &[(7, 500), (8, 500), (7, 500)], None);
        assert_eq!(placements[0], Placement { x_offset: -20, y_offset: 0, x_advance: -40 });
        assert_eq!(placements[1], Placement::default(), "a glyph nobody named was moved");
        assert_eq!(placements[2].x_advance, -40);
    }

    /// A pair written out glyph by glyph: the first glyph's set lists what may
    /// follow it and what to do about each.
    fn pair_by_glyph(first: u16, second: u16, by: i16) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&be(1));
        out.extend_from_slice(&be(0)); // the coverage, filled in below
        out.extend_from_slice(&be(0x0004)); // the first glyph gets an advance
        out.extend_from_slice(&be(0)); // the second gets nothing
        out.extend_from_slice(&be(1)); // one set
        out.extend_from_slice(&be(12)); // which follows the header

        out.extend_from_slice(&be(1)); // one pair in it
        out.extend_from_slice(&be(second));
        out.extend_from_slice(&signed(by));

        let at = out.len();
        out.extend_from_slice(&coverage(&[first]));
        out[2..4].copy_from_slice(&be(at as u16));
        out
    }

    #[test]
    fn two_glyphs_set_closer_together_than_their_widths() {
        // Which is the whole of kerning: the pair says how much to take out.
        let data = table(2, 0, &pair_by_glyph(10, 11, -150));
        let placements = run(&data, &[(10, 600), (11, 600)], None);
        assert_eq!(placements[0].x_advance, -150);
        assert_eq!(placements[1], Placement::default());

        // The other way round is a different pair, and this font does not have
        // it.
        let placements = run(&data, &[(11, 600), (10, 600)], None);
        assert_eq!(placements[0].x_advance, 0);
    }

    #[test]
    fn a_run_of_three_kerns_twice() {
        let data = table(2, 0, &pair_by_glyph(10, 10, -100));
        let placements = run(&data, &[(10, 600), (10, 600), (10, 600)], None);
        assert_eq!(placements[0].x_advance, -100);
        assert_eq!(placements[1].x_advance, -100);
        assert_eq!(placements[2].x_advance, 0, "there is nothing after the last one");
    }

    /// A class definition of the ranges given: first, last, class.
    fn class_ranges(ranges: &[(u16, u16, u16)]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&be(2));
        out.extend_from_slice(&be(ranges.len() as u16));
        for (first, last, class) in ranges {
            out.extend_from_slice(&be(*first));
            out.extend_from_slice(&be(*last));
            out.extend_from_slice(&be(*class));
        }
        out
    }

    #[test]
    fn a_pair_written_by_class_kerns_the_whole_set_at_once() {
        // How a font of four thousand kerning pairs is written in a few
        // hundred bytes, and how every font that kerns much writes it: two
        // sets of classes and a value for each pair of them.
        let mut out = Vec::new();
        out.extend_from_slice(&be(2)); // by class
        out.extend_from_slice(&be(0)); // the coverage, filled in below
        out.extend_from_slice(&be(0x0004)); // the first glyph gets an advance
        out.extend_from_slice(&be(0)); // the second gets nothing
        out.extend_from_slice(&be(0)); // the first set of classes
        out.extend_from_slice(&be(0)); // the second
        out.extend_from_slice(&be(3)); // three classes on the left
        out.extend_from_slice(&be(2)); // two on the right

        // The values, a row per left class and a column per right class.
        // Everything is nothing except the left class one against the right
        // class one.
        for left in 0..3 {
            for right in 0..2 {
                let value: i16 = if left == 1 && right == 1 { -200 } else { 0 };
                out.extend_from_slice(&signed(value));
            }
        }

        let first_at = out.len();
        out.extend_from_slice(&class_ranges(&[(10, 10, 1), (11, 11, 2)]));
        let second_at = out.len();
        out.extend_from_slice(&class_ranges(&[(20, 20, 1)]));
        let coverage_at = out.len();
        out.extend_from_slice(&coverage(&[10, 11]));
        out[2..4].copy_from_slice(&be(coverage_at as u16));
        out[8..10].copy_from_slice(&be(first_at as u16));
        out[10..12].copy_from_slice(&be(second_at as u16));

        let data = table(2, 0, &out);
        assert_eq!(run(&data, &[(10, 600), (20, 600)], None)[0].x_advance, -200);
        // The same left glyph against something in no class at all.
        assert_eq!(run(&data, &[(10, 600), (21, 600)], None)[0].x_advance, 0);
        // And a left glyph in the other class, which this font says nothing
        // about.
        assert_eq!(run(&data, &[(11, 600), (20, 600)], None)[0].x_advance, 0);
    }

    /// The glyph classes of a font: 10 to 19 are letters, 20 to 29 are marks.
    fn definitions() -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&be(1));
        data.extend_from_slice(&be(0));
        data.extend_from_slice(&be(12));
        data.extend_from_slice(&be(0));
        data.extend_from_slice(&be(0));
        data.extend_from_slice(&be(0));
        data.extend_from_slice(&be(2));
        data.extend_from_slice(&be(2));
        for (first, last, class) in [(10u16, 19u16, 1u16), (20, 29, 3)] {
            data.extend_from_slice(&be(first));
            data.extend_from_slice(&be(last));
            data.extend_from_slice(&be(class));
        }
        data
    }

    #[test]
    fn a_lookup_that_passes_over_marks_kerns_across_one() {
        // What the flag is for: the letters either side of an accent are still
        // a pair, and a font that could not say so would set them apart.
        let data = table(2, 0x0008, &pair_by_glyph(10, 11, -150));
        let classes = definitions();
        let placements = run(&data, &[(10, 600), (20, 0), (11, 600)], Some(&classes));
        assert_eq!(placements[0].x_advance, -150, "the mark broke the pair");
    }

    /// A mark brought onto a letter: one anchor on each, to be brought
    /// together.
    fn mark_to_base(base: u16, mark: u16, base_x: i16, mark_x: i16) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&be(1));
        out.extend_from_slice(&be(0)); // the marks' coverage
        out.extend_from_slice(&be(0)); // the bases' coverage
        out.extend_from_slice(&be(1)); // one group of marks
        out.extend_from_slice(&be(0)); // the marks themselves
        out.extend_from_slice(&be(0)); // the bases

        let marks_at = out.len();
        out.extend_from_slice(&be(1)); // one mark
        out.extend_from_slice(&be(0)); // in group zero
        out.extend_from_slice(&be(6)); // whose anchor follows the list
        out.extend_from_slice(&be(1)); // an anchor of coordinates alone
        out.extend_from_slice(&signed(mark_x));
        out.extend_from_slice(&signed(0));

        let bases_at = out.len();
        out.extend_from_slice(&be(1)); // one base
        out.extend_from_slice(&be(4)); // whose anchor follows the list
        out.extend_from_slice(&be(1));
        out.extend_from_slice(&signed(base_x));
        out.extend_from_slice(&signed(500));

        let mark_coverage_at = out.len();
        out.extend_from_slice(&coverage(&[mark]));
        let base_coverage_at = out.len();
        out.extend_from_slice(&coverage(&[base]));

        out[2..4].copy_from_slice(&be(mark_coverage_at as u16));
        out[4..6].copy_from_slice(&be(base_coverage_at as u16));
        out[8..10].copy_from_slice(&be(marks_at as u16));
        out[10..12].copy_from_slice(&be(bases_at as u16));
        out
    }

    #[test]
    fn a_mark_is_brought_onto_the_letter_it_belongs_to() {
        // The letter is 600 wide and its anchor is at 300 — the middle of it.
        // The mark's own anchor is at its middle too, 50. The mark is drawn
        // after the letter, so the pen has already travelled 600: it has to
        // come back that far and then by the difference of the anchors.
        let data = table(4, 0, &mark_to_base(10, 20, 300, 50));
        let classes = definitions();
        let placements = run(&data, &[(10, 600), (20, 0)], Some(&classes));
        assert_eq!(placements[1].x_offset, 300 - 50 - 600);
        assert_eq!(placements[1].y_offset, 500);
    }

    #[test]
    fn a_mark_finds_its_letter_past_another_mark() {
        // Two accents on one letter: the second belongs to the letter as well,
        // and a reader that stopped at the first would hang it off the accent.
        let data = table(4, 0, &mark_to_base(10, 20, 300, 50));
        let classes = definitions();
        let placements = run(&data, &[(10, 600), (21, 0), (20, 0)], Some(&classes));
        assert_eq!(placements[2].x_offset, 300 - 50 - 600, "the second mark missed the letter");
    }

    #[test]
    fn a_mark_with_no_letter_before_it_is_left_where_it_is() {
        let data = table(4, 0, &mark_to_base(10, 20, 300, 50));
        let classes = definitions();
        let placements = run(&data, &[(20, 0)], Some(&classes));
        assert_eq!(placements[0], Placement::default());
    }

    #[test]
    fn a_mark_goes_onto_the_mark_before_it_when_that_is_what_the_lookup_says() {
        // The same table read as the other kind: what a mark attaches to is
        // then the glyph before it, whatever that is. A mark moves no pen, so
        // there is no travel to take back — the second mark sits where the
        // first one does, plus the difference between their anchors.
        let data = table(6, 0, &mark_to_base(21, 20, 300, 50));
        let classes = definitions();
        let placements = run(&data, &[(10, 600), (21, 0), (20, 0)], Some(&classes));
        assert_eq!(placements[2].x_offset, 300 - 50);
        // And the letter is not what it attached to: the mark before it is.
        let alone = run(&data, &[(10, 600), (20, 0)], Some(&classes));
        assert_eq!(alone[1], Placement::default(), "it attached to the letter instead");
    }

    #[test]
    fn a_lookup_behind_an_extension_is_still_a_lookup() {
        let inner = pair_by_glyph(10, 11, -150);
        let mut extension = Vec::new();
        extension.extend_from_slice(&be(1));
        extension.extend_from_slice(&be(2)); // what it really is: a pair
        extension.extend_from_slice(&8u32.to_be_bytes());
        extension.extend_from_slice(&inner);

        let data = table(9, 0, &extension);
        let placements = run(&data, &[(10, 600), (11, 600)], None);
        assert_eq!(placements[0].x_advance, -150);
    }

    #[test]
    fn an_extension_pointing_at_an_extension_is_not_followed() {
        let mut inner = Vec::new();
        inner.extend_from_slice(&be(1));
        inner.extend_from_slice(&be(9));
        inner.extend_from_slice(&0u32.to_be_bytes());

        let data = table(9, 0, &inner);
        assert_eq!(run(&data, &[(10, 600), (11, 600)], None)[0], Placement::default());
    }

    #[test]
    fn a_kind_this_does_not_read_leaves_the_run_alone() {
        // Cursive attachment, which is named in the roadmap: a reader that
        // half-applied it would draw worse than one that left it.
        let data = table(3, 0, &pair_by_glyph(10, 11, -150));
        assert!(run(&data, &[(10, 600), (11, 600)], None).iter().all(Placement::is_nothing));
    }
}
