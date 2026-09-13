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
//! And the three that say *when* to run those four. A contextual lookup is a
//! rule about surroundings — this glyph, but only between those two — and it
//! does its work by naming other lookups and the places in the match to run
//! them at. A chaining one says the same thing with a before and an after. An
//! extension lookup is none of them: it is a lookup that points at another
//! table, which is how a font too big for sixteen-bit offsets is written, and
//! a reader that skips extensions skips most of what a large font says.
//!
//! Those three are not polish. A font that writes its ligatures inside a
//! chaining rule has no ligatures at all to a reader that cannot follow one,
//! and the scripts that reorder — see the roadmap — are written almost
//! entirely in them.

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

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    let bytes = data.get(at..at + 4)?;
    Some(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
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
        self.run(index, glyphs, clusters, 0, false)
    }

    /// How far one lookup may reach into another before this stops following.
    ///
    /// A font may point a contextual lookup at itself, directly or round a
    /// ring of them. Eight is deeper than any real font goes and shallow
    /// enough that a font which does go round for ever stops rather than
    /// taking the program with it.
    const DEPTH: usize = 8;

    /// Applies the lookup at an index, from inside another or on its own.
    ///
    /// `only_first` is what tells the two apart. A lookup run on its own works
    /// wherever it matches; a lookup named by a contextual rule works at the
    /// place that rule points at and nowhere else, or a rule about one letter
    /// would change every letter like it in the word.
    fn run(
        &self,
        index: usize,
        glyphs: &mut Vec<GlyphId>,
        clusters: &mut Vec<usize>,
        depth: usize,
        only_first: bool,
    ) -> bool {
        if depth > Self::DEPTH {
            return false;
        }
        let Some((kind, subtables)) = self.lookup(index) else {
            return false;
        };
        self.run_subtables(kind, &subtables, glyphs, clusters, depth, only_first)
    }

    /// The same, once the kind and the subtables are known.
    ///
    /// Apart because an extension lookup has to say what it really is before
    /// anything can be done with it, and what it really is arrives here.
    fn run_subtables(
        &self,
        kind: u16,
        subtables: &[usize],
        glyphs: &mut Vec<GlyphId>,
        clusters: &mut Vec<usize>,
        depth: usize,
        only_first: bool,
    ) -> bool {
        match kind {
            1 => subtables.iter().any(|at| self.apply_single(*at, glyphs, only_first)),
            2 | 3 => subtables
                .iter()
                .any(|at| self.apply_one_to_many(kind, *at, glyphs, clusters, only_first)),
            4 => subtables.iter().any(|at| self.apply_ligature(*at, glyphs, clusters, only_first)),
            5 => subtables
                .iter()
                .any(|at| self.apply_context(*at, glyphs, clusters, depth, only_first)),
            6 => subtables
                .iter()
                .any(|at| self.apply_chained(*at, glyphs, clusters, depth, only_first)),
            // A lookup that points at another table, so that a font too big
            // for sixteen-bit offsets can still say where its rules are. What
            // it points at is an ordinary subtable of the kind it names.
            7 => subtables.iter().any(|at| {
                let Some(real) = u16_at(self.data, at + 2) else { return false };
                let Some(offset) = u32_at(self.data, at + 4) else { return false };
                if real == 7 {
                    // An extension pointing at an extension is a font pointing
                    // at itself; the format forbids it and this stops rather
                    // than following it round.
                    return false;
                }
                let inner = at + offset as usize;
                self.run_subtables(real, &[inner], glyphs, clusters, depth, only_first)
            }),
            _ => false,
        }
    }

    /// One glyph replaced by one other.
    fn apply_single(&self, at: usize, glyphs: &mut [GlyphId], only_first: bool) -> bool {
        let Some(format) = u16_at(self.data, at) else { return false };
        let Some(coverage) = u16_at(self.data, at + 2) else { return false };
        let coverage = at + usize::from(coverage);
        let mut changed = false;

        let reach = if only_first { glyphs.len().min(1) } else { glyphs.len() };
        for glyph in glyphs.iter_mut().take(reach) {
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
        only_first: bool,
    ) -> bool {
        if u16_at(self.data, at) != Some(1) {
            return false;
        }
        let Some(coverage) = u16_at(self.data, at + 2) else { return false };
        let coverage = at + usize::from(coverage);
        let mut changed = false;

        let mut index = 0usize;
        while index < glyphs.len() {
            if only_first && index > 0 {
                break;
            }
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
        only_first: bool,
    ) -> bool {
        if u16_at(self.data, at) != Some(1) {
            return false;
        }
        let Some(coverage) = u16_at(self.data, at + 2) else { return false };
        let coverage = at + usize::from(coverage);
        let mut changed = false;

        let mut index = 0usize;
        while index < glyphs.len() {
            if only_first && index > 0 {
                break;
            }
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

    /// A rule about surroundings: this glyph, in this company, becomes that.
    ///
    /// Three ways of saying the same thing. By glyph, one rule per glyph that
    /// may begin a match; by class, so that a rule about every vowel is
    /// written once; and by coverage, one set per place in the match, which is
    /// the way a font says "any of these, then any of those".
    fn apply_context(
        &self,
        at: usize,
        glyphs: &mut Vec<GlyphId>,
        clusters: &mut Vec<usize>,
        depth: usize,
        only_first: bool,
    ) -> bool {
        let Some(format) = u16_at(self.data, at) else { return false };
        let mut changed = false;
        let mut index = 0usize;

        while index < glyphs.len() {
            if only_first && index > 0 {
                break;
            }
            let matched = match format {
                1 | 2 => self.context_rule(format, at, index, glyphs),
                3 => self.context_coverages(at, index, glyphs),
                _ => None,
            };
            let Some((length, records)) = matched else {
                index += 1;
                continue;
            };

            let before = glyphs.len();
            if self.apply_records(&records, index, glyphs, clusters, depth) {
                changed = true;
            }
            // Past what was matched, so a rule cannot match inside its own
            // answer. What the nested lookups did to the length is counted in.
            let grew = glyphs.len() as isize - before as isize;
            index += (length as isize + grew).max(1) as usize;
        }
        changed
    }

    /// The same with a before and an after: the chaining kind.
    fn apply_chained(
        &self,
        at: usize,
        glyphs: &mut Vec<GlyphId>,
        clusters: &mut Vec<usize>,
        depth: usize,
        only_first: bool,
    ) -> bool {
        let Some(format) = u16_at(self.data, at) else { return false };
        let mut changed = false;
        let mut index = 0usize;

        while index < glyphs.len() {
            if only_first && index > 0 {
                break;
            }
            let matched = match format {
                1 | 2 => self.chained_rule(format, at, index, glyphs),
                3 => self.chained_coverages(at, index, glyphs),
                _ => None,
            };
            let Some((length, records)) = matched else {
                index += 1;
                continue;
            };

            let before = glyphs.len();
            if self.apply_records(&records, index, glyphs, clusters, depth) {
                changed = true;
            }
            let grew = glyphs.len() as isize - before as isize;
            index += (length as isize + grew).max(1) as usize;
        }
        changed
    }

    /// A contextual rule written out glyph by glyph, or class by class.
    ///
    /// Gives back how many glyphs the rule matched and what it asks to be run
    /// on them.
    fn context_rule(
        &self,
        format: u16,
        at: usize,
        index: usize,
        glyphs: &[GlyphId],
    ) -> Option<(usize, Vec<(usize, usize)>)> {
        let coverage = at + usize::from(u16_at(self.data, at + 2)?);
        let covered = self.covered(coverage, glyphs[index])?;

        // By glyph, the rule set is chosen by where the first glyph sits in
        // the coverage; by class, by what class it is in.
        let (sets_at, chosen) = if format == 1 {
            (at + 6, covered)
        } else {
            let classes = at + usize::from(u16_at(self.data, at + 4)?);
            (at + 8, usize::from(self.class_of(classes, glyphs[index])))
        };
        let count = usize::from(u16_at(self.data, sets_at - 2)?);
        if chosen >= count {
            return None;
        }
        let offset = u16_at(self.data, sets_at + chosen * 2)?;
        if offset == 0 {
            return None;
        }
        let set = at + usize::from(offset);

        let rules = usize::from(u16_at(self.data, set)?);
        for rule in 0..rules {
            let Some(offset) = u16_at(self.data, set + 2 + rule * 2) else { continue };
            let rule = set + usize::from(offset);
            let Some(length) = u16_at(self.data, rule).map(usize::from) else { continue };
            let Some(records) = u16_at(self.data, rule + 2).map(usize::from) else { continue };
            if length == 0 || index + length > glyphs.len() {
                continue;
            }

            // The first glyph is the one already matched by the coverage, so
            // the sequence lists the rest.
            let follows = (1..length).all(|step| {
                let Some(wanted) = u16_at(self.data, rule + 4 + (step - 1) * 2) else {
                    return false;
                };
                if format == 1 {
                    glyphs[index + step].0 == wanted
                } else {
                    let classes = at + usize::from(u16_at(self.data, at + 4).unwrap_or(0));
                    self.class_of(classes, glyphs[index + step]) == wanted
                }
            });
            if !follows {
                continue;
            }
            let at_records = rule + 4 + (length - 1) * 2;
            return Some((length, self.records(at_records, records)));
        }
        None
    }

    /// A contextual rule written as one set of glyphs per place.
    fn context_coverages(
        &self,
        at: usize,
        index: usize,
        glyphs: &[GlyphId],
    ) -> Option<(usize, Vec<(usize, usize)>)> {
        let length = usize::from(u16_at(self.data, at + 2)?);
        let records = usize::from(u16_at(self.data, at + 4)?);
        if length == 0 || index + length > glyphs.len() {
            return None;
        }
        for step in 0..length {
            let coverage = at + usize::from(u16_at(self.data, at + 6 + step * 2)?);
            self.covered(coverage, glyphs[index + step])?;
        }
        Some((length, self.records(at + 6 + length * 2, records)))
    }

    /// A chaining rule written out glyph by glyph, or class by class.
    fn chained_rule(
        &self,
        format: u16,
        at: usize,
        index: usize,
        glyphs: &[GlyphId],
    ) -> Option<(usize, Vec<(usize, usize)>)> {
        let coverage = at + usize::from(u16_at(self.data, at + 2)?);
        let covered = self.covered(coverage, glyphs[index])?;

        // The three class definitions of the class-based kind: what comes
        // before, what is matched, and what comes after. Each may class the
        // same glyph differently, which is the point of having three.
        let (sets_at, chosen, backtrack_classes, input_classes, lookahead_classes) = if format == 1
        {
            (at + 6, covered, 0, 0, 0)
        } else {
            let backtrack = at + usize::from(u16_at(self.data, at + 4)?);
            let input = at + usize::from(u16_at(self.data, at + 6)?);
            let lookahead = at + usize::from(u16_at(self.data, at + 8)?);
            let chosen = usize::from(self.class_of(input, glyphs[index]));
            (at + 12, chosen, backtrack, input, lookahead)
        };
        let count = usize::from(u16_at(self.data, sets_at - 2)?);
        if chosen >= count {
            return None;
        }
        let offset = u16_at(self.data, sets_at + chosen * 2)?;
        if offset == 0 {
            return None;
        }
        let set = at + usize::from(offset);

        let rules = usize::from(u16_at(self.data, set)?);
        for rule in 0..rules {
            let Some(offset) = u16_at(self.data, set + 2 + rule * 2) else { continue };
            let rule = set + usize::from(offset);
            let matched = self.chained_rule_matches(
                format,
                rule,
                index,
                glyphs,
                (backtrack_classes, input_classes, lookahead_classes),
            );
            if let Some(found) = matched {
                return Some(found);
            }
        }
        None
    }

    /// Whether one chaining rule matches where it is being tried.
    fn chained_rule_matches(
        &self,
        format: u16,
        rule: usize,
        index: usize,
        glyphs: &[GlyphId],
        classes: (usize, usize, usize),
    ) -> Option<(usize, Vec<(usize, usize)>)> {
        let (backtrack_classes, input_classes, lookahead_classes) = classes;
        let same = |wanted: u16, glyph: GlyphId, class_at: usize| -> bool {
            if format == 1 {
                glyph.0 == wanted
            } else {
                self.class_of(class_at, glyph) == wanted
            }
        };

        // What comes before, written nearest first, which is why it is read
        // backwards from where the match begins.
        let backtrack = usize::from(u16_at(self.data, rule)?);
        if backtrack > index {
            return None;
        }
        for step in 0..backtrack {
            let wanted = u16_at(self.data, rule + 2 + step * 2)?;
            if !same(wanted, glyphs[index - 1 - step], backtrack_classes) {
                return None;
            }
        }

        let mut at = rule + 2 + backtrack * 2;
        let length = usize::from(u16_at(self.data, at)?);
        if length == 0 || index + length > glyphs.len() {
            return None;
        }
        for step in 1..length {
            let wanted = u16_at(self.data, at + 2 + (step - 1) * 2)?;
            if !same(wanted, glyphs[index + step], input_classes) {
                return None;
            }
        }

        at += 2 + (length - 1) * 2;
        let lookahead = usize::from(u16_at(self.data, at)?);
        if index + length + lookahead > glyphs.len() {
            return None;
        }
        for step in 0..lookahead {
            let wanted = u16_at(self.data, at + 2 + step * 2)?;
            if !same(wanted, glyphs[index + length + step], lookahead_classes) {
                return None;
            }
        }

        at += 2 + lookahead * 2;
        let records = usize::from(u16_at(self.data, at)?);
        Some((length, self.records(at + 2, records)))
    }

    /// A chaining rule written as one set of glyphs per place.
    fn chained_coverages(
        &self,
        at: usize,
        index: usize,
        glyphs: &[GlyphId],
    ) -> Option<(usize, Vec<(usize, usize)>)> {
        let backtrack = usize::from(u16_at(self.data, at + 2)?);
        if backtrack > index {
            return None;
        }
        for step in 0..backtrack {
            let coverage = at + usize::from(u16_at(self.data, at + 4 + step * 2)?);
            self.covered(coverage, glyphs[index - 1 - step])?;
        }

        let mut walk = at + 4 + backtrack * 2;
        let length = usize::from(u16_at(self.data, walk)?);
        if length == 0 || index + length > glyphs.len() {
            return None;
        }
        for step in 0..length {
            let coverage = at + usize::from(u16_at(self.data, walk + 2 + step * 2)?);
            self.covered(coverage, glyphs[index + step])?;
        }

        walk += 2 + length * 2;
        let lookahead = usize::from(u16_at(self.data, walk)?);
        if index + length + lookahead > glyphs.len() {
            return None;
        }
        for step in 0..lookahead {
            let coverage = at + usize::from(u16_at(self.data, walk + 2 + step * 2)?);
            self.covered(coverage, glyphs[index + length + step])?;
        }

        walk += 2 + lookahead * 2;
        let records = usize::from(u16_at(self.data, walk)?);
        Some((length, self.records(walk + 2, records)))
    }

    /// What a rule asks to be run, and where: a place in the match, and a
    /// lookup.
    fn records(&self, at: usize, count: usize) -> Vec<(usize, usize)> {
        let mut out = Vec::with_capacity(count);
        for index in 0..count {
            let Some(place) = u16_at(self.data, at + index * 4) else { break };
            let Some(lookup) = u16_at(self.data, at + index * 4 + 2) else { break };
            out.push((usize::from(place), usize::from(lookup)));
        }
        out
    }

    /// Runs what a rule asked for, at the places it asked for.
    ///
    /// A nested lookup may make the run longer or shorter — a ligature is two
    /// glyphs becoming one — and the places the rule named were counted before
    /// any of that happened. So what each one did to the length is carried
    /// along and added to the places still to come.
    fn apply_records(
        &self,
        records: &[(usize, usize)],
        index: usize,
        glyphs: &mut Vec<GlyphId>,
        clusters: &mut Vec<usize>,
        depth: usize,
    ) -> bool {
        let mut changed = false;
        let mut shift = 0isize;

        for (place, lookup) in records {
            let at = index as isize + *place as isize + shift;
            if at < 0 || at as usize >= glyphs.len() {
                continue;
            }
            let at = at as usize;

            // The lookup is given what follows the place it was sent to, and
            // told to work at the front of it and nowhere else.
            let mut tail: Vec<GlyphId> = glyphs[at..].to_vec();
            let mut tail_clusters: Vec<usize> = clusters[at..].to_vec();
            let before = tail.len();
            if self.run(*lookup, &mut tail, &mut tail_clusters, depth + 1, true) {
                glyphs.splice(at.., tail);
                clusters.splice(at.., tail_clusters);
                changed = true;
                shift += glyphs.len() as isize - (at + before) as isize;
            }
        }
        changed
    }

    /// Which class a glyph is in, which is how a rule is written once for a
    /// whole set of them. Anything a definition does not name is class zero.
    fn class_of(&self, at: usize, glyph: GlyphId) -> u16 {
        match u16_at(self.data, at) {
            // A run of glyphs starting at one, with a class each.
            Some(1) => {
                let Some(first) = u16_at(self.data, at + 2) else { return 0 };
                let Some(count) = u16_at(self.data, at + 4) else { return 0 };
                if glyph.0 < first || glyph.0 >= first + count {
                    return 0;
                }
                let index = usize::from(glyph.0 - first);
                u16_at(self.data, at + 6 + index * 2).unwrap_or(0)
            }
            // Ranges, each with the class its glyphs are in.
            Some(2) => {
                let Some(count) = u16_at(self.data, at + 2) else { return 0 };
                for index in 0..usize::from(count) {
                    let entry = at + 4 + index * 6;
                    let (Some(first), Some(last), Some(class)) = (
                        u16_at(self.data, entry),
                        u16_at(self.data, entry + 2),
                        u16_at(self.data, entry + 4),
                    ) else {
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

/// Tables built by hand, because no font to hand is written the way each of
/// these rules needs to be seen in isolation.
///
/// Everything below assembles real table bytes rather than standing in for
/// them: what is tested is the reading of the format, so anything short of the
/// format would test the wrong thing.
#[cfg(test)]
mod contextual {
    use super::*;

    fn be(value: u16) -> [u8; 2] {
        value.to_be_bytes()
    }

    /// A coverage table listing the glyphs given, in order.
    fn coverage(glyphs: &[u16]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&be(1));
        out.extend_from_slice(&be(glyphs.len() as u16));
        for glyph in glyphs {
            out.extend_from_slice(&be(*glyph));
        }
        out
    }

    /// A class definition of the ranges given: first, last, class.
    fn classes(ranges: &[(u16, u16, u16)]) -> Vec<u8> {
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

    /// One glyph for one other, named pair by pair.
    fn single(pairs: &[(u16, u16)]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&be(2)); // one replacement listed per glyph
        out.extend_from_slice(&be(6)); // the coverage follows the header
        out.extend_from_slice(&be(pairs.len() as u16));
        for (_, to) in pairs {
            out.extend_from_slice(&be(*to));
        }
        let from: Vec<u16> = pairs.iter().map(|(from, _)| *from).collect();
        out.extend_from_slice(&coverage(&from));
        // The coverage offset above assumed it followed the header; correct it
        // now that the substitutes are counted in.
        let at = 6 + pairs.len() * 2;
        out[2..4].copy_from_slice(&be(at as u16));
        out
    }

    /// Two glyphs becoming one.
    fn ligature(first: u16, second: u16, into: u16) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&be(1));
        out.extend_from_slice(&be(8)); // coverage, after the set below
        out.extend_from_slice(&be(1)); // one set
        out.extend_from_slice(&be(8)); // the set follows the header
                                       // The set: one ligature, which follows it.
        out.extend_from_slice(&be(1));
        out.extend_from_slice(&be(4));
        // The ligature: what it becomes, how many glyphs, and the rest of them.
        out.extend_from_slice(&be(into));
        out.extend_from_slice(&be(2));
        out.extend_from_slice(&be(second));
        let at = out.len();
        out.extend_from_slice(&coverage(&[first]));
        out[2..4].copy_from_slice(&be(at as u16));
        out
    }

    /// A whole substitution table: one script, one feature, and the lookups
    /// given, in order, all of them named by that feature.
    fn table(lookups: &[(u16, Vec<u8>)]) -> Vec<u8> {
        let scripts = 10usize;
        let script_list = 20usize;
        let features = scripts + script_list;
        let feature_list = 2 + 6 + 4 + 2;
        let lookup_list = features + feature_list;

        let mut data = Vec::new();
        data.extend_from_slice(&be(1));
        data.extend_from_slice(&be(0));
        data.extend_from_slice(&be(scripts as u16));
        data.extend_from_slice(&be(features as u16));
        data.extend_from_slice(&be(lookup_list as u16));

        // The script list: one script, whose table is six bytes along, with a
        // default language system naming the one feature.
        data.extend_from_slice(&be(1));
        data.extend_from_slice(b"latn");
        data.extend_from_slice(&be(8));
        data.extend_from_slice(&be(4));
        data.extend_from_slice(&be(0));
        data.extend_from_slice(&be(0));
        data.extend_from_slice(&be(0xFFFF));
        data.extend_from_slice(&be(1));
        data.extend_from_slice(&be(0));

        // The feature list: one feature, naming every lookup.
        data.extend_from_slice(&be(1));
        data.extend_from_slice(b"test");
        data.extend_from_slice(&be(8));
        data.extend_from_slice(&be(0));
        // Only the first: a lookup a rule names is in the list but is not
        // named by the feature, or it would work on its own as well.
        data.extend_from_slice(&be(1));
        data.extend_from_slice(&be(0));

        // The lookup list: the offsets first, then the tables they point at.
        data.extend_from_slice(&be(lookups.len() as u16));
        let offsets_at = data.len();
        data.extend_from_slice(&vec![0u8; lookups.len() * 2]);
        for (index, (kind, subtable)) in lookups.iter().enumerate() {
            let table_at = data.len() - lookup_list;
            data[offsets_at + index * 2..offsets_at + index * 2 + 2]
                .copy_from_slice(&be(table_at as u16));
            data.extend_from_slice(&be(*kind));
            data.extend_from_slice(&be(0)); // no flags
            data.extend_from_slice(&be(1)); // one subtable, which follows
            data.extend_from_slice(&be(8));
            data.extend_from_slice(subtable);
        }
        data
    }

    /// Runs the feature over a run of glyphs and says what came out.
    fn run(data: &[u8], glyphs: &[u16]) -> Vec<u16> {
        let table = Substitutions::parse(data).expect("the table should parse");
        let mut ids: Vec<GlyphId> = glyphs.iter().map(|glyph| GlyphId(*glyph)).collect();
        let mut clusters: Vec<usize> = (0..glyphs.len()).collect();
        for lookup in table.lookups_for(b"latn", b"test") {
            table.apply(lookup, &mut ids, &mut clusters);
        }
        ids.into_iter().map(|glyph| glyph.0).collect()
    }

    /// A chaining rule written as one set of glyphs per place: what comes
    /// before, what is matched, what comes after, and what to run.
    fn chained_by_coverage(
        backtrack: &[&[u16]],
        input: &[&[u16]],
        lookahead: &[&[u16]],
        records: &[(u16, u16)],
    ) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&be(3));
        // Where each offset has to be written once its table has been laid
        // down, kept as it is left empty: the offsets of the three groups are
        // not one run of numbers, they have the counts in among them.
        let mut slots: Vec<(usize, Vec<u8>)> = Vec::new();
        for group in [backtrack, input, lookahead] {
            out.extend_from_slice(&be(group.len() as u16));
            for glyphs in group {
                slots.push((out.len(), coverage(glyphs)));
                out.extend_from_slice(&be(0));
            }
        }
        out.extend_from_slice(&be(records.len() as u16));
        for (place, lookup) in records {
            out.extend_from_slice(&be(*place));
            out.extend_from_slice(&be(*lookup));
        }

        for (slot, set) in slots {
            let at = out.len();
            out.extend_from_slice(&set);
            out[slot..slot + 2].copy_from_slice(&be(at as u16));
        }
        out
    }

    #[test]
    fn a_rule_about_surroundings_works_only_in_them() {
        // Glyph 20 becomes 99, but only with 10 before it and 30 after it.
        let data = table(&[
            (6, chained_by_coverage(&[&[10]], &[&[20]], &[&[30]], &[(0, 1)])),
            (1, single(&[(20, 99)])),
        ]);
        assert_eq!(run(&data, &[10, 20, 30]), vec![10, 99, 30], "the rule did not fire");
        assert_eq!(run(&data, &[11, 20, 30]), vec![11, 20, 30], "what comes before was ignored");
        assert_eq!(run(&data, &[10, 20, 31]), vec![10, 20, 31], "what comes after was ignored");
    }

    #[test]
    fn a_rule_fires_where_it_matches_and_not_elsewhere() {
        // The same glyph twice, in context once.
        let data = table(&[
            (6, chained_by_coverage(&[&[10]], &[&[20]], &[], &[(0, 1)])),
            (1, single(&[(20, 99)])),
        ]);
        assert_eq!(run(&data, &[20, 10, 20]), vec![20, 10, 99]);
    }

    #[test]
    fn a_lookup_a_rule_names_works_at_the_place_named_and_nowhere_else() {
        // The lookup would change both glyphs if it were let loose; the rule
        // sends it to the second one.
        let data = table(&[
            (6, chained_by_coverage(&[], &[&[20], &[20]], &[], &[(1, 1)])),
            (1, single(&[(20, 99)])),
        ]);
        assert_eq!(run(&data, &[20, 20]), vec![20, 99]);
    }

    #[test]
    fn a_rule_may_name_a_lookup_that_makes_the_run_shorter() {
        // Two glyphs become one inside the match, and what follows is not
        // disturbed by the places moving.
        let data = table(&[
            (6, chained_by_coverage(&[&[5]], &[&[20], &[21]], &[], &[(0, 1)])),
            (4, ligature(20, 21, 99)),
        ]);
        assert_eq!(run(&data, &[5, 20, 21, 7]), vec![5, 99, 7]);
    }

    #[test]
    fn a_lookup_behind_an_extension_is_still_a_lookup() {
        // What a font too big for sixteen-bit offsets writes. The extension
        // says what kind the real table is and where it is; here it sits
        // directly after the eight bytes of the extension itself.
        let inner = single(&[(20, 99)]);
        let mut extension = Vec::new();
        extension.extend_from_slice(&be(1));
        extension.extend_from_slice(&be(1)); // what it really is: single
        extension.extend_from_slice(&8u32.to_be_bytes());
        extension.extend_from_slice(&inner);

        let data = table(&[(7, extension)]);
        assert_eq!(run(&data, &[20, 21]), vec![99, 21]);
    }

    #[test]
    fn an_extension_pointing_at_an_extension_is_not_followed() {
        // The format forbids it, and a font that writes one is a font that
        // could send a reader round for ever.
        let mut inner = Vec::new();
        inner.extend_from_slice(&be(1));
        inner.extend_from_slice(&be(7));
        inner.extend_from_slice(&0u32.to_be_bytes());

        let data = table(&[(7, inner)]);
        assert_eq!(run(&data, &[20]), vec![20]);
    }

    #[test]
    fn a_rule_written_by_class_says_the_same_thing_about_a_whole_set() {
        // Class one is every letter of a set; the rule is written once.
        let mut subtable = Vec::new();
        subtable.extend_from_slice(&be(2)); // by class
        subtable.extend_from_slice(&be(0)); // coverage, filled in below
        subtable.extend_from_slice(&be(0)); // the class definition, likewise
        subtable.extend_from_slice(&be(2)); // two class sets: nothing, and one
        subtable.extend_from_slice(&be(0));
        subtable.extend_from_slice(&be(0));

        // The set for class one: one rule, two glyphs long, running the second
        // lookup at the first place.
        let set_at = subtable.len();
        subtable.extend_from_slice(&be(1));
        subtable.extend_from_slice(&be(4));
        subtable.extend_from_slice(&be(2)); // the match is two glyphs
        subtable.extend_from_slice(&be(1)); // one thing to run
        subtable.extend_from_slice(&be(1)); // the second of them is class one
        subtable.extend_from_slice(&be(0));
        subtable.extend_from_slice(&be(1));
        subtable[10..12].copy_from_slice(&be(set_at as u16));

        let coverage_at = subtable.len();
        subtable.extend_from_slice(&coverage(&[20, 21]));
        subtable[2..4].copy_from_slice(&be(coverage_at as u16));

        let classes_at = subtable.len();
        subtable.extend_from_slice(&classes(&[(20, 21, 1)]));
        subtable[4..6].copy_from_slice(&be(classes_at as u16));

        let data = table(&[(5, subtable), (1, single(&[(20, 99), (21, 98)]))]);
        // Two of the class in a row: the first is changed.
        assert_eq!(run(&data, &[20, 21]), vec![99, 21]);
        assert_eq!(run(&data, &[21, 20]), vec![98, 20]);
        // One on its own is not two in a row.
        assert_eq!(run(&data, &[20, 7]), vec![20, 7]);
    }
}
