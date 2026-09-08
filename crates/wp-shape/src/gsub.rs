//! Reading the glyph substitution table, to the OpenType specification.
//!
//! # What a substitution table is
//!
//! Three lists that point at each other. The script list says which scripts the
//! font supports; each script names features, such as "the initial form" or
//! "ligatures"; each feature names lookups; and a lookup is the rule that
//! actually replaces one glyph with another. Asking a font for the initial form
//! of a letter means walking that chain and running whatever it arrives at.
//!
//! # What is read here
//!
//! Single substitution (one glyph for one), multiple (one for several),
//! alternate (a choice, of which the first is taken) and ligature (several for
//! one). Those four carry Arabic joining and Latin ligatures between them,
//! which is what makes the difference between text that is right and text that
//! is wrong.
//!
//! Contextual and chaining-contextual lookups are read far enough to be
//! skipped rather than misapplied. They refine what the four above do; leaving
//! them out costs some polish and no correctness.

use wp_font::GlyphId;

/// A parsed substitution table, ready to be asked for features.
#[derive(Clone, Debug)]
pub struct Substitutions<'a> {
    data: &'a [u8],
    scripts: usize,
    features: usize,
    lookups: usize,
}

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    let bytes = data.get(at..at + 2)?;
    Some(u16::from_be_bytes([bytes[0], bytes[1]]))
}

fn tag_at(data: &[u8], at: usize) -> Option<[u8; 4]> {
    let bytes = data.get(at..at + 4)?;
    Some([bytes[0], bytes[1], bytes[2], bytes[3]])
}

impl<'a> Substitutions<'a> {
    /// Reads the header of a `GSUB` table.
    #[must_use]
    pub fn parse(data: &'a [u8]) -> Option<Self> {
        // Major version one; the minor version only adds an optional list this
        // does not use.
        if u16_at(data, 0)? != 1 {
            return None;
        }
        Some(Self {
            scripts: usize::from(u16_at(data, 4)?),
            features: usize::from(u16_at(data, 6)?),
            lookups: usize::from(u16_at(data, 8)?),
            data,
        })
    }

    /// The lookups a feature uses, for a given script.
    ///
    /// The default language system is used: a font that distinguishes Urdu from
    /// Arabic offers both, and picking the default gives the letters everyone
    /// agrees on rather than nothing.
    #[must_use]
    pub fn lookups_for(&self, script: &[u8; 4], feature: &[u8; 4]) -> Vec<usize> {
        let Some(language) = self.default_language(script) else {
            return Vec::new();
        };

        let count = usize::from(u16_at(self.data, language + 4).unwrap_or(0));
        let mut found = Vec::new();

        for index in 0..count {
            let Some(feature_index) = u16_at(self.data, language + 6 + index * 2) else {
                break;
            };
            let entry = self.features + 2 + usize::from(feature_index) * 6;
            if tag_at(self.data, entry) != Some(*feature) {
                continue;
            }

            let Some(offset) = u16_at(self.data, entry + 4) else { continue };
            let table = self.features + usize::from(offset);
            let lookup_count = usize::from(u16_at(self.data, table + 2).unwrap_or(0));
            for lookup in 0..lookup_count {
                if let Some(index) = u16_at(self.data, table + 4 + lookup * 2) {
                    found.push(usize::from(index));
                }
            }
        }

        found
    }

    /// The default language system of a script, if the font has that script.
    fn default_language(&self, script: &[u8; 4]) -> Option<usize> {
        let count = usize::from(u16_at(self.data, self.scripts)?);
        for index in 0..count {
            let entry = self.scripts + 2 + index * 6;
            if tag_at(self.data, entry)? != *script {
                continue;
            }
            let table = self.scripts + usize::from(u16_at(self.data, entry + 4)?);
            let default = u16_at(self.data, table)?;
            if default == 0 {
                return None;
            }
            return Some(table + usize::from(default));
        }
        None
    }

    /// Where a lookup's table begins, and what type it is.
    fn lookup(&self, index: usize) -> Option<(u16, Vec<usize>)> {
        let count = usize::from(u16_at(self.data, self.lookups)?);
        if index >= count {
            return None;
        }
        let table = self.lookups + usize::from(u16_at(self.data, self.lookups + 2 + index * 2)?);
        let kind = u16_at(self.data, table)?;

        let subtable_count = usize::from(u16_at(self.data, table + 4)?);
        let mut subtables = Vec::with_capacity(subtable_count);
        for at in 0..subtable_count {
            if let Some(offset) = u16_at(self.data, table + 6 + at * 2) {
                subtables.push(table + usize::from(offset));
            }
        }

        Some((kind, subtables))
    }

    /// Applies one lookup to a run of glyphs, in place.
    ///
    /// Returns whether anything changed, so a caller can tell a feature that
    /// did something from one that did not.
    pub fn apply(
        &self,
        index: usize,
        glyphs: &mut Vec<GlyphId>,
        clusters: &mut Vec<usize>,
    ) -> bool {
        let Some((kind, subtables)) = self.lookup(index) else {
            return false;
        };

        match kind {
            1 => subtables.iter().any(|at| self.apply_single(*at, glyphs)),
            2 | 3 => subtables.iter().any(|at| self.apply_one_to_many(kind, *at, glyphs, clusters)),
            4 => subtables.iter().any(|at| self.apply_ligature(*at, glyphs, clusters)),
            // Contextual and chaining lookups refine the rest; an extension
            // lookup points at another table. Skipping them loses polish, not
            // correctness — applying them wrongly would lose both.
            _ => false,
        }
    }

    /// One glyph replaced by one other.
    fn apply_single(&self, at: usize, glyphs: &mut [GlyphId]) -> bool {
        let Some(format) = u16_at(self.data, at) else { return false };
        let Some(coverage) = u16_at(self.data, at + 2) else { return false };
        let coverage = at + usize::from(coverage);
        let mut changed = false;

        for glyph in glyphs.iter_mut() {
            let Some(index) = self.covered(coverage, *glyph) else { continue };
            let replacement = match format {
                // A single number added to every covered glyph.
                1 => u16_at(self.data, at + 4).map(|delta| glyph.0.wrapping_add(delta)),
                // One replacement listed per covered glyph.
                2 => u16_at(self.data, at + 6 + index * 2),
                _ => None,
            };
            if let Some(replacement) = replacement {
                *glyph = GlyphId(replacement);
                changed = true;
            }
        }

        changed
    }

    /// One glyph replaced by several, or by the first of a choice.
    fn apply_one_to_many(
        &self,
        kind: u16,
        at: usize,
        glyphs: &mut Vec<GlyphId>,
        clusters: &mut Vec<usize>,
    ) -> bool {
        if u16_at(self.data, at) != Some(1) {
            return false;
        }
        let Some(coverage) = u16_at(self.data, at + 2) else { return false };
        let coverage = at + usize::from(coverage);
        let mut changed = false;

        let mut index = 0usize;
        while index < glyphs.len() {
            let Some(covered) = self.covered(coverage, glyphs[index]) else {
                index += 1;
                continue;
            };
            let Some(offset) = u16_at(self.data, at + 6 + covered * 2) else {
                index += 1;
                continue;
            };
            let sequence = at + usize::from(offset);
            let count = usize::from(u16_at(self.data, sequence).unwrap_or(0));

            if kind == 3 {
                // A choice of alternates. Without a way for the user to pick
                // one, the first is the font designer's own default.
                if count > 0 {
                    if let Some(replacement) = u16_at(self.data, sequence + 2) {
                        glyphs[index] = GlyphId(replacement);
                        changed = true;
                    }
                }
                index += 1;
                continue;
            }

            let mut replacements = Vec::with_capacity(count);
            for at_index in 0..count {
                if let Some(glyph) = u16_at(self.data, sequence + 2 + at_index * 2) {
                    replacements.push(GlyphId(glyph));
                }
            }
            if replacements.is_empty() {
                index += 1;
                continue;
            }

            // Every glyph the one glyph became belongs to the same character.
            let cluster = clusters[index];
            let added = replacements.len();
            glyphs.splice(index..=index, replacements);
            clusters.splice(index..=index, std::iter::repeat_n(cluster, added));
            index += added;
            changed = true;
        }

        changed
    }

    /// Several glyphs replaced by one.
    fn apply_ligature(
        &self,
        at: usize,
        glyphs: &mut Vec<GlyphId>,
        clusters: &mut Vec<usize>,
    ) -> bool {
        if u16_at(self.data, at) != Some(1) {
            return false;
        }
        let Some(coverage) = u16_at(self.data, at + 2) else { return false };
        let coverage = at + usize::from(coverage);
        let mut changed = false;

        let mut index = 0usize;
        while index < glyphs.len() {
            let Some(covered) = self.covered(coverage, glyphs[index]) else {
                index += 1;
                continue;
            };
            let Some(offset) = u16_at(self.data, at + 6 + covered * 2) else {
                index += 1;
                continue;
            };
            let set = at + usize::from(offset);
            let count = usize::from(u16_at(self.data, set).unwrap_or(0));

            let mut replaced = false;
            for entry in 0..count {
                let Some(ligature_offset) = u16_at(self.data, set + 2 + entry * 2) else {
                    continue;
                };
                let ligature = set + usize::from(ligature_offset);
                let Some(glyph) = u16_at(self.data, ligature) else { continue };
                let components = usize::from(u16_at(self.data, ligature + 2).unwrap_or(0));
                if components == 0 || index + components > glyphs.len() {
                    continue;
                }

                // The first component is the glyph already matched; the rest
                // are listed and have to follow it exactly.
                let matches = (1..components).all(|step| {
                    u16_at(self.data, ligature + 2 + step * 2)
                        .is_some_and(|wanted| glyphs[index + step].0 == wanted)
                });
                if !matches {
                    continue;
                }

                let cluster = clusters[index];
                glyphs.splice(index..index + components, [GlyphId(glyph)]);
                clusters.splice(index..index + components, [cluster]);
                replaced = true;
                changed = true;
                break;
            }

            index += 1;
            let _ = replaced;
        }

        changed
    }

    /// Where a glyph sits in a coverage table, if it is in one at all.
    ///
    /// Coverage is how every lookup says which glyphs it applies to, and the
    /// index it gives back is how the lookup finds the matching entry in its
    /// own list.
    fn covered(&self, at: usize, glyph: GlyphId) -> Option<usize> {
        match u16_at(self.data, at)? {
            // A sorted list of the glyphs themselves.
            1 => {
                let count = usize::from(u16_at(self.data, at + 2)?);
                let mut low = 0usize;
                let mut high = count;
                while low < high {
                    let middle = (low + high) / 2;
                    let found = u16_at(self.data, at + 4 + middle * 2)?;
                    match found.cmp(&glyph.0) {
                        core::cmp::Ordering::Less => low = middle + 1,
                        core::cmp::Ordering::Greater => high = middle,
                        core::cmp::Ordering::Equal => return Some(middle),
                    }
                }
                None
            }
            // Ranges, each carrying the index its first glyph stands at.
            2 => {
                let count = usize::from(u16_at(self.data, at + 2)?);
                for index in 0..count {
                    let entry = at + 4 + index * 6;
                    let first = u16_at(self.data, entry)?;
                    let last = u16_at(self.data, entry + 2)?;
                    if glyph.0 >= first && glyph.0 <= last {
                        let start = u16_at(self.data, entry + 4)?;
                        return Some(usize::from(start) + usize::from(glyph.0 - first));
                    }
                }
                None
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds the smallest table that says one thing: for the Arabic script,
    /// the initial form feature runs one single-substitution lookup.
    fn build_table() -> Vec<u8> {
        // Laid out by hand so the offsets are visible and checkable.
        let mut data = vec![0u8; 10];
        data[0..2].copy_from_slice(&1u16.to_be_bytes()); // major version
        data[2..4].copy_from_slice(&0u16.to_be_bytes()); // minor version

        // The script list runs to twenty bytes, the feature list to fourteen;
        // each list follows the one before it.
        let scripts = 10usize;
        let features = scripts + 20;
        let lookups = features + 14;

        data[4..6].copy_from_slice(&(scripts as u16).to_be_bytes());
        data[6..8].copy_from_slice(&(features as u16).to_be_bytes());
        data[8..10].copy_from_slice(&(lookups as u16).to_be_bytes());

        // Script list: one script, "arab", whose table is six bytes along.
        data.extend_from_slice(&1u16.to_be_bytes());
        data.extend_from_slice(b"arab");
        data.extend_from_slice(&8u16.to_be_bytes());
        // Script table: a default language system four bytes along, no others.
        data.extend_from_slice(&4u16.to_be_bytes());
        data.extend_from_slice(&0u16.to_be_bytes());
        // Language system: no required feature, one feature, index zero.
        data.extend_from_slice(&0u16.to_be_bytes());
        data.extend_from_slice(&0xFFFFu16.to_be_bytes());
        data.extend_from_slice(&1u16.to_be_bytes());
        data.extend_from_slice(&0u16.to_be_bytes());

        // Feature list: one feature, "init", whose table is eight bytes along.
        data.extend_from_slice(&1u16.to_be_bytes());
        data.extend_from_slice(b"init");
        data.extend_from_slice(&8u16.to_be_bytes());
        // Feature table: no parameters, one lookup, index zero.
        data.extend_from_slice(&0u16.to_be_bytes());
        data.extend_from_slice(&1u16.to_be_bytes());
        data.extend_from_slice(&0u16.to_be_bytes());

        // Lookup list: one lookup, whose table is four bytes along.
        data.extend_from_slice(&1u16.to_be_bytes());
        data.extend_from_slice(&4u16.to_be_bytes());
        // Lookup: type one, no flags, one subtable eight bytes along.
        data.extend_from_slice(&1u16.to_be_bytes());
        data.extend_from_slice(&0u16.to_be_bytes());
        data.extend_from_slice(&1u16.to_be_bytes());
        data.extend_from_slice(&8u16.to_be_bytes());
        // Subtable: format one, coverage six bytes along, add one hundred.
        data.extend_from_slice(&1u16.to_be_bytes());
        data.extend_from_slice(&6u16.to_be_bytes());
        data.extend_from_slice(&100u16.to_be_bytes());
        // Coverage: format one, one glyph, glyph five.
        data.extend_from_slice(&1u16.to_be_bytes());
        data.extend_from_slice(&1u16.to_be_bytes());
        data.extend_from_slice(&5u16.to_be_bytes());

        data
    }

    #[test]
    fn a_table_that_is_not_one_is_refused() {
        assert!(Substitutions::parse(&[]).is_none());
        assert!(Substitutions::parse(&[0, 9, 0, 0, 0, 0, 0, 0, 0, 0]).is_none());
    }

    #[test]
    fn a_feature_is_found_through_its_script() {
        let data = build_table();
        let table = Substitutions::parse(&data).expect("a readable table");

        assert_eq!(table.lookups_for(b"arab", b"init"), vec![0]);
    }

    #[test]
    fn a_script_the_font_does_not_have_offers_nothing() {
        let data = build_table();
        let table = Substitutions::parse(&data).expect("a readable table");

        assert!(table.lookups_for(b"latn", b"init").is_empty());
        assert!(table.lookups_for(b"arab", b"liga").is_empty());
    }

    #[test]
    fn a_single_substitution_replaces_the_glyph_it_covers() {
        let data = build_table();
        let table = Substitutions::parse(&data).expect("a readable table");

        let mut glyphs = vec![GlyphId(4), GlyphId(5), GlyphId(6)];
        let mut clusters = vec![0, 1, 2];
        assert!(table.apply(0, &mut glyphs, &mut clusters));

        assert_eq!(glyphs, vec![GlyphId(4), GlyphId(105), GlyphId(6)], "only glyph five moves");
        assert_eq!(clusters, vec![0, 1, 2], "and the characters they came from do not");
    }

    #[test]
    fn a_lookup_that_covers_nothing_present_changes_nothing() {
        let data = build_table();
        let table = Substitutions::parse(&data).expect("a readable table");

        let mut glyphs = vec![GlyphId(1), GlyphId(2)];
        let mut clusters = vec![0, 1];
        assert!(!table.apply(0, &mut glyphs, &mut clusters));
        assert_eq!(glyphs, vec![GlyphId(1), GlyphId(2)]);
    }

    #[test]
    fn a_lookup_that_does_not_exist_is_refused_rather_than_read_past() {
        let data = build_table();
        let table = Substitutions::parse(&data).expect("a readable table");

        let mut glyphs = vec![GlyphId(5)];
        let mut clusters = vec![0];
        assert!(!table.apply(99, &mut glyphs, &mut clusters));
    }

    #[test]
    fn a_truncated_table_is_read_as_far_as_it_goes_and_no_further() {
        let data = build_table();
        for length in 10..data.len() {
            // Whatever it does, it must not panic: a font file is data from
            // outside, and half the fonts in the world are slightly wrong.
            if let Some(table) = Substitutions::parse(&data[..length]) {
                let mut glyphs = vec![GlyphId(5)];
                let mut clusters = vec![0];
                let _ = table.lookups_for(b"arab", b"init");
                let _ = table.apply(0, &mut glyphs, &mut clusters);
            }
        }
    }
}
