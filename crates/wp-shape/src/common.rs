//! What the substitution and positioning tables have in common.
//!
//! Both are the same shape: a list of scripts, each naming features, each
//! naming lookups, and a list of lookups those names point into. One says what
//! a glyph becomes and the other says where it goes, and everything above that
//! — finding the script, following the feature, reaching the lookup, asking
//! whether a glyph is covered and which class it is in — is the same walk.
//!
//! So it is written once here. Two copies of this walk would be two things to
//! keep in step, and the day they drifted one table would be read with the
//! other's arithmetic.

use wp_font::GlyphId;

pub(crate) fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    let bytes = data.get(at..at + 2)?;
    Some(u16::from_be_bytes([bytes[0], bytes[1]]))
}

pub(crate) fn i16_at(data: &[u8], at: usize) -> Option<i16> {
    u16_at(data, at).map(|value| value as i16)
}

pub(crate) fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    let bytes = data.get(at..at + 4)?;
    Some(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

pub(crate) fn tag_at(data: &[u8], at: usize) -> Option<[u8; 4]> {
    let bytes = data.get(at..at + 4)?;
    Some([bytes[0], bytes[1], bytes[2], bytes[3]])
}

/// The three lists every one of these tables begins with.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Tables<'a> {
    pub data: &'a [u8],
    scripts: usize,
    features: usize,
    lookups: usize,
}

impl<'a> Tables<'a> {
    /// Reads the header, whichever of the two tables it belongs to.
    pub fn parse(data: &'a [u8]) -> Option<Self> {
        // Major version one; the minor version only adds an optional list
        // neither of these readers uses.
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
    /// The default language system is used: a font that distinguishes Urdu
    /// from Arabic offers both, and picking the default gives the letters
    /// everyone agrees on rather than nothing.
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

    /// Whether the font says anything at all under a script.
    ///
    /// Which matters where one script has two names: the scripts that reorder
    /// were given new tags in 2005, and a font may carry either. Asking under
    /// the wrong one finds nothing and draws the letters in the order they
    /// were typed.
    pub fn has_script(&self, script: &[u8; 4]) -> bool {
        self.default_language(script).is_some()
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

    /// What a lookup is: its kind, the flags that say what it passes over, and
    /// where each of its subtables begins.
    pub fn lookup(&self, index: usize) -> Option<(u16, u16, Vec<usize>)> {
        let count = usize::from(u16_at(self.data, self.lookups)?);
        if index >= count {
            return None;
        }
        let table = self.lookups + usize::from(u16_at(self.data, self.lookups + 2 + index * 2)?);
        let kind = u16_at(self.data, table)?;
        let flags = u16_at(self.data, table + 2)?;

        let subtable_count = usize::from(u16_at(self.data, table + 4)?);
        let mut subtables = Vec::with_capacity(subtable_count);
        for at in 0..subtable_count {
            if let Some(offset) = u16_at(self.data, table + 6 + at * 2) {
                subtables.push(table + usize::from(offset));
            }
        }

        Some((kind, flags, subtables))
    }

    /// Where a glyph sits in a coverage table, if it is in one at all.
    ///
    /// Coverage is how every lookup says which glyphs it applies to, and the
    /// index it gives back is how the lookup finds the matching entry in its
    /// own list.
    pub fn covered(&self, at: usize, glyph: GlyphId) -> Option<usize> {
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

    /// Which class a glyph is in, which is how a rule is written once for a
    /// whole set of them. Anything a definition does not name is class zero.
    pub fn class_of(&self, at: usize, glyph: GlyphId) -> u16 {
        class_in(self.data, at, glyph)
    }
}

/// The same question asked of a table that is not one of the two: `GDEF` keeps
/// class definitions of its own.
pub(crate) fn class_in(data: &[u8], at: usize, glyph: GlyphId) -> u16 {
    match u16_at(data, at) {
        // A run of glyphs starting at one, with a class each.
        Some(1) => {
            let Some(first) = u16_at(data, at + 2) else { return 0 };
            let Some(count) = u16_at(data, at + 4) else { return 0 };
            if glyph.0 < first || glyph.0 >= first.saturating_add(count) {
                return 0;
            }
            let index = usize::from(glyph.0 - first);
            u16_at(data, at + 6 + index * 2).unwrap_or(0)
        }
        // Ranges, each with the class its glyphs are in.
        Some(2) => {
            let Some(count) = u16_at(data, at + 2) else { return 0 };
            for index in 0..usize::from(count) {
                let entry = at + 4 + index * 6;
                let (Some(first), Some(last), Some(class)) =
                    (u16_at(data, entry), u16_at(data, entry + 2), u16_at(data, entry + 4))
                else {
                    return 0;
                };
                if glyph.0 >= first && glyph.0 <= last {
                    return class;
                }
            }
            0
        }
        _ => 0,
    }
}
