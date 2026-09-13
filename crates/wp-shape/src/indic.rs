//! Devanagari: the syllable, and the order its pieces are drawn in.
//!
//! # Why this is not a matter of substitution
//!
//! Every other script this program shapes is drawn in the order it is stored.
//! Devanagari is not. The vowel sign of "ki" — कि — is stored after the
//! consonant and drawn before it; the "r" of a cluster that begins with र्
//! is stored first and drawn last, as a hook over the end of the syllable. No
//! substitution table can express that, because a substitution replaces glyphs
//! where they stand. The text has to be rearranged before the font is asked
//! anything.
//!
//! So the work here is: find where one syllable ends and the next begins, find
//! which consonant of it is the one the others hang off, and put the pieces in
//! the order they are drawn. What the font does afterwards — half forms,
//! conjuncts, the hook itself — is ordinary substitution, asked for in the
//! order the format lays down.
//!
//! # What a syllable is
//!
//! A run of consonants joined by the halant — the mark that says "no vowel
//! here" — ending in a consonant that keeps its vowel, followed by whatever
//! vowel signs and dots belong to it. An independent vowel stands as a
//! syllable of its own. Anything else — a digit, a stop, a space — is not part
//! of one.
//!
//! # What is here and what is not
//!
//! Devanagari. The other nine scripts written this way share the shape of the
//! rules and differ in their details — where the hook goes, which consonants
//! take a form below the line, whether a vowel sign is written in two pieces
//! around the consonant — and each needs its own reading of the same tables.
//! Named in the roadmap.

use wp_font::{Font, GlyphId};

use crate::gsub::Substitutions;

/// What a character is, as far as a syllable is concerned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    /// A consonant, which is what a syllable is built from.
    Consonant,
    /// The one consonant with rules of its own: at the front of a cluster it
    /// is drawn as a hook over the end of it, and at the back as a tail under
    /// the middle.
    Ra,
    /// A vowel written as a letter rather than as a sign on a consonant.
    Vowel,
    /// A vowel sign, which hangs on the consonant before it — and, when it is
    /// one of the two that are written to the left, is drawn before it.
    Matra(Position),
    /// The dot that changes a consonant into another consonant.
    Nukta,
    /// The mark that says the consonant it follows keeps no vowel, which is
    /// what joins two consonants into one cluster.
    Halant,
    /// The dots and hooks that belong to the whole syllable: the anusvara, the
    /// visarga, the accents.
    Modifier(Position),
    /// A zero-width joiner or non-joiner, which is how somebody asks for the
    /// joined or the unjoined form of what would otherwise be either.
    Joiner,
    NonJoiner,
    /// Something a sign may hang on that is not a consonant: the avagraha, the
    /// syllable Om, the dotted circle shown where a sign has nothing to hang
    /// on.
    Placeholder,
    /// A digit, a stop, a space: not part of a syllable at all.
    Other,
}

/// Where a sign is drawn with respect to the consonant it belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Position {
    /// Written to the left of the consonant, however far to the right it is
    /// stored. This is the whole reason the order has to be changed.
    Before,
    Above,
    Below,
    After,
}

/// What a character is in Devanagari.
///
/// The block is 0900 to 097F, and the four that matter from outside it are the
/// two joiners, the dotted circle, and nothing else.
#[must_use]
pub fn category_of(character: char) -> Category {
    let value = character as u32;
    match value {
        0x200C => Category::NonJoiner,
        0x200D => Category::Joiner,
        0x25CC => Category::Placeholder,

        // The signs that belong to the whole syllable: the candrabindu and
        // the anusvara above, the visarga after.
        0x0900..=0x0902 => Category::Modifier(Position::Above),
        0x0903 => Category::Modifier(Position::After),

        // The independent vowels, written as letters.
        0x0904..=0x0914 | 0x0960 | 0x0961 | 0x0972..=0x0977 => Category::Vowel,

        // The consonants. Ra is one of them and has rules of its own.
        0x0930 | 0x0931 | 0x095D => Category::Ra,
        0x0915..=0x0939 | 0x0958..=0x095F | 0x0979..=0x097F => Category::Consonant,

        0x093A => Category::Matra(Position::Above),
        0x093B => Category::Matra(Position::After),
        0x093C => Category::Nukta,
        // The avagraha and Om: not consonants, but a sign may hang on them.
        0x093D | 0x0950 => Category::Placeholder,
        0x093E => Category::Matra(Position::After),
        // The two that are stored after the consonant and written before it.
        0x093F => Category::Matra(Position::Before),
        0x0940 => Category::Matra(Position::After),
        0x0941..=0x0944 => Category::Matra(Position::Below),
        0x0945..=0x0948 => Category::Matra(Position::Above),
        0x0949..=0x094C => Category::Matra(Position::After),
        0x094D => Category::Halant,
        0x094E => Category::Matra(Position::Before),
        0x094F => Category::Matra(Position::After),

        // The Vedic accents and the two signs written under the line.
        0x0951 | 0x0953 | 0x0954 => Category::Modifier(Position::Above),
        0x0952 | 0x0955..=0x0957 => Category::Modifier(Position::Below),
        0x0962 | 0x0963 => Category::Matra(Position::Below),

        _ => Category::Other,
    }
}

/// One syllable: where it starts, where it ends, and which of its consonants
/// the rest of it hangs on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Syllable {
    /// Indices into the run of characters, the end being one past the last.
    pub start: usize,
    pub end: usize,
    /// Which character is the base consonant, if the syllable has one.
    pub base: Option<usize>,
    /// Whether it begins with a Ra and a halant, which is drawn as a hook over
    /// the end of the syllable rather than as a letter at the front of it.
    pub reph: bool,
}

impl Syllable {
    /// How many characters it holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.end - self.start
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.start >= self.end
    }
}

/// Splits a run of characters into syllables.
///
/// Everything is in one syllable or another: a digit or a stop is a syllable
/// of one character with no base, which keeps the rest of the machinery from
/// having to ask whether it is looking at Devanagari at all.
#[must_use]
pub fn syllables(characters: &[char]) -> Vec<Syllable> {
    let mut out = Vec::new();
    let mut at = 0usize;

    while at < characters.len() {
        let start = at;
        match category_of(characters[at]) {
            Category::Consonant | Category::Ra | Category::Placeholder => {
                at = consonants(characters, at);
                at = tail(characters, at);
            }
            Category::Vowel => {
                at += 1;
                at = tail(characters, at);
            }
            // A sign with nothing before it to hang on, or anything that is
            // not Devanagari at all: one character, and no base.
            _ => at += 1,
        }
        let mut syllable = Syllable { start, end: at, base: None, reph: false };
        settle(characters, &mut syllable);
        out.push(syllable);
    }

    out
}

/// Walks the run of consonants joined by halants, ending after the last
/// consonant of it.
fn consonants(characters: &[char], from: usize) -> usize {
    let mut at = from;
    loop {
        // A consonant, then whatever may hang on it before the halant.
        at += 1;
        if matches!(characters.get(at).copied().map(category_of), Some(Category::Nukta)) {
            at += 1;
        }
        if matches!(
            characters.get(at).copied().map(category_of),
            Some(Category::Joiner | Category::NonJoiner)
        ) {
            at += 1;
        }

        // A halant joins this consonant to whatever follows. Without one the
        // consonant keeps its vowel and the run of them is over.
        if !matches!(characters.get(at).copied().map(category_of), Some(Category::Halant)) {
            return at;
        }
        let halant = at;
        at += 1;
        if matches!(
            characters.get(at).copied().map(category_of),
            Some(Category::Joiner | Category::NonJoiner)
        ) {
            at += 1;
        }

        // A halant at the end of the syllable belongs to it — it is how a
        // word ending in a bare consonant is written — but it joins nothing.
        if !matches!(
            characters.get(at).copied().map(category_of),
            Some(Category::Consonant | Category::Ra | Category::Placeholder)
        ) {
            return halant + 1;
        }
    }
}

/// Everything that may follow the last consonant: the vowel signs, then the
/// dots that belong to the whole syllable.
fn tail(characters: &[char], from: usize) -> usize {
    let mut at = from;
    while matches!(
        characters.get(at).copied().map(category_of),
        Some(Category::Matra(_) | Category::Nukta)
    ) {
        at += 1;
    }
    while matches!(characters.get(at).copied().map(category_of), Some(Category::Modifier(_))) {
        at += 1;
    }
    at
}

/// Works out which consonant a syllable hangs on, and whether it begins with
/// the hook.
fn settle(characters: &[char], syllable: &mut Syllable) {
    let is_letter = |at: usize| {
        matches!(
            category_of(characters[at]),
            Category::Consonant | Category::Ra | Category::Vowel | Category::Placeholder
        )
    };

    // A syllable that begins with Ra and a halant, and has something after
    // them, begins with the hook: that Ra is not a letter of the syllable, it
    // is a mark drawn over the end of it.
    let mut first = syllable.start;
    if syllable.len() > 2
        && category_of(characters[syllable.start]) == Category::Ra
        && category_of(characters[syllable.start + 1]) == Category::Halant
        // Somebody who wrote a joiner after the halant asked for the letter
        // rather than the hook, and is to be given it.
        && !matches!(
            characters.get(syllable.start + 2).copied().map(category_of),
            Some(Category::Joiner)
        )
        && (syllable.start + 2..syllable.end).any(is_letter)
    {
        syllable.reph = true;
        first = syllable.start + 2;
    }

    // The base is the last letter of the syllable — the one that keeps its
    // vowel. A Ra at the end of a cluster is drawn as a tail under the letter
    // before it rather than as a letter of its own, so it is not the base
    // while there is anything else it could be.
    let mut base = None;
    for at in first..syllable.end {
        if !is_letter(at) {
            continue;
        }
        let tail_ra = category_of(characters[at]) == Category::Ra
            && at > first
            && category_of(characters[at - 1]) == Category::Halant;
        if tail_ra && base.is_some() {
            continue;
        }
        base = Some(at);
    }
    syllable.base = base;
}

/// The order the characters of a syllable are drawn in.
///
/// Gives back indices into the run of characters. Three things move: the vowel
/// signs written to the left go to the front, the hook goes to the end, and
/// everything else stays where it was.
#[must_use]
pub fn drawn_order(characters: &[char], syllable: &Syllable) -> Vec<usize> {
    let mut before = Vec::new();
    let mut middle = Vec::new();

    for (at, character) in characters.iter().enumerate().take(syllable.end).skip(syllable.start) {
        // The hook is dealt with at the end, and the halant that made it goes
        // with it: the two are one mark.
        if syllable.reph && (at == syllable.start || at == syllable.start + 1) {
            continue;
        }
        match category_of(*character) {
            Category::Matra(Position::Before) => before.push(at),
            _ => middle.push(at),
        }
    }

    if syllable.reph {
        // After the letter the syllable hangs on and whatever is written under
        // it, and before the signs written to the right of it: the hook sits
        // over the end of the syllable, and a vowel sign written to the right
        // is further right still.
        let at = middle
            .iter()
            .position(|index| {
                matches!(
                    category_of(characters[*index]),
                    Category::Matra(Position::After) | Category::Modifier(_)
                )
            })
            .unwrap_or(middle.len());
        middle.insert(at, syllable.start);
        middle.insert(at + 1, syllable.start + 1);
    }

    before.extend(middle);
    before
}

/// The two names Devanagari goes by in a font.
///
/// The scripts that reorder were given new tags in 2005, when the rules were
/// settled; a font written since then carries the new one and an older font
/// carries the old. A font may carry both, and then the new one is what its
/// designer meant.
pub const TAGS: [[u8; 4]; 2] = [*b"dev2", *b"deva"];

/// Which of the two names this font knows the script by.
#[must_use]
pub fn tag_in(table: &Substitutions<'_>) -> [u8; 4] {
    if table.has_script(&TAGS[0]) {
        TAGS[0]
    } else {
        TAGS[1]
    }
}

/// Turns Devanagari into glyphs: the syllable, the forms, and the order.
///
/// The order the format lays down, and the order matters. The forms that make
/// a cluster one shape are asked for first, while the pieces are still where
/// they were written; then the pieces are put in the order they are drawn;
/// then the forms that depend on that order.
#[must_use]
pub fn shape(
    font: &Font<'_>,
    table: &Substitutions<'_>,
    script: &[u8; 4],
    text: &str,
) -> (Vec<GlyphId>, Vec<usize>) {
    let characters: Vec<char> = text.chars().collect();
    // Where each character begins, which is what a glyph carries so that a
    // caret can be put back between two of them.
    let mut offsets = Vec::with_capacity(characters.len() + 1);
    let mut at = 0usize;
    for character in &characters {
        offsets.push(at);
        at += character.len_utf8();
    }
    offsets.push(at);

    let mut glyphs = Vec::with_capacity(characters.len());
    let mut clusters = Vec::with_capacity(characters.len());
    for syllable in syllables(&characters) {
        let (piece, places) = one(font, table, script, &characters, &offsets, &syllable);
        glyphs.extend(piece);
        clusters.extend(places);
    }
    (glyphs, clusters)
}

/// One syllable, from characters to the glyphs that draw it.
fn one(
    font: &Font<'_>,
    table: &Substitutions<'_>,
    script: &[u8; 4],
    characters: &[char],
    offsets: &[usize],
    syllable: &Syllable,
) -> (Vec<GlyphId>, Vec<usize>) {
    let mut glyphs: Vec<GlyphId> = (syllable.start..syllable.end)
        .map(|at| font.glyph_for(characters[at]).unwrap_or(GlyphId(0)))
        .collect();
    let mut clusters: Vec<usize> = (syllable.start..syllable.end).map(|at| offsets[at]).collect();
    if glyphs.is_empty() {
        return (glyphs, clusters);
    }

    // The dot that makes another consonant, and the conjuncts a font spells as
    // one letter. Both are about what the letters are rather than where they
    // go, so both come first.
    for feature in [b"nukt", b"akhn"] {
        whole(table, script, feature, &mut glyphs, &mut clusters);
    }

    // The hook: the Ra and the halant at the front become one mark. Asked for
    // at the front of the syllable and nowhere else, or every consonant of it
    // followed by a halant would become a hook.
    if syllable.reph {
        for lookup in table.lookups_for(script, b"rphf") {
            table.apply_at_start(lookup, &mut glyphs, &mut clusters);
        }
    }

    // The half forms, which are the consonants before the one the syllable
    // hangs on; and the forms written under and after it, which are the ones
    // following it. Each is asked of its own part of the syllable, because a
    // font will spell a half form out of any consonant and a halant and would
    // make one where a form below the line belongs.
    let base = syllable.base.map(|at| offsets[at]);
    if let Some(base) = base {
        let split = clusters.iter().position(|cluster| *cluster >= base).unwrap_or(0);
        part(table, script, b"half", &mut glyphs, &mut clusters, 0..split);
        for feature in [b"blwf", b"pstf"] {
            let from = clusters.iter().position(|cluster| *cluster >= base).unwrap_or(0);
            let end = glyphs.len();
            part(table, script, feature, &mut glyphs, &mut clusters, from..end);
        }
    }
    for feature in [b"vatu", b"cjct"] {
        whole(table, script, feature, &mut glyphs, &mut clusters);
    }

    // And now the order they are drawn in. A glyph belongs where the character
    // it came from belongs: a form made of two characters carries the first of
    // them, which is the one whose place decides.
    let order = drawn_order(characters, syllable);
    let mut rank = vec![usize::MAX; characters.len() + 1];
    for (place, at) in order.iter().enumerate() {
        rank[*at] = place;
    }
    let place_of = |cluster: usize| -> usize {
        let at = offsets.iter().position(|offset| *offset == cluster).unwrap_or(0);
        rank.get(at).copied().unwrap_or(usize::MAX)
    };
    let mut together: Vec<(GlyphId, usize)> =
        glyphs.iter().copied().zip(clusters.iter().copied()).collect();
    // Stable, so that two glyphs standing for the same character keep the
    // order they were made in.
    together.sort_by_key(|(_, cluster)| place_of(*cluster));
    glyphs = together.iter().map(|(glyph, _)| *glyph).collect();
    clusters = together.iter().map(|(_, cluster)| *cluster).collect();

    // What the font does once everything is where it belongs: the forms drawn
    // before, above, below and after the letter, and the rule for a cluster
    // that ends in a bare halant.
    for feature in [b"pres", b"abvs", b"blws", b"psts", b"haln", b"calt", b"clig"] {
        whole(table, script, feature, &mut glyphs, &mut clusters);
    }

    (glyphs, clusters)
}

/// Applies a feature to the whole of a syllable.
fn whole(
    table: &Substitutions<'_>,
    script: &[u8; 4],
    feature: &[u8; 4],
    glyphs: &mut Vec<GlyphId>,
    clusters: &mut Vec<usize>,
) {
    for lookup in table.lookups_for(script, feature) {
        table.apply(lookup, glyphs, clusters);
    }
}

/// Applies a feature to one stretch of a syllable, leaving the rest alone.
fn part(
    table: &Substitutions<'_>,
    script: &[u8; 4],
    feature: &[u8; 4],
    glyphs: &mut Vec<GlyphId>,
    clusters: &mut Vec<usize>,
    range: core::ops::Range<usize>,
) {
    if range.start >= range.end || range.end > glyphs.len() {
        return;
    }
    let lookups = table.lookups_for(script, feature);
    if lookups.is_empty() {
        return;
    }

    let mut piece: Vec<GlyphId> = glyphs[range.clone()].to_vec();
    let mut places: Vec<usize> = clusters[range.clone()].to_vec();
    for lookup in lookups {
        table.apply(lookup, &mut piece, &mut places);
    }
    glyphs.splice(range.clone(), piece);
    clusters.splice(range, places);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The letters used below, so the tests read as the words they are.
    const KA: char = '\u{0915}';
    const KHA: char = '\u{0916}';
    const GA: char = '\u{0917}';
    const RA: char = '\u{0930}';
    const SA: char = '\u{0938}';
    const HALANT: char = '\u{094D}';
    const I: char = '\u{093F}'; // written to the left
    const AA: char = '\u{093E}'; // written to the right
    const U: char = '\u{0941}'; // written below
    const ANUSVARA: char = '\u{0902}';
    const NUKTA: char = '\u{093C}';
    const A: char = '\u{0905}'; // the independent vowel
    const JOINER: char = '\u{200D}';

    fn split(text: &str) -> Vec<Syllable> {
        let characters: Vec<char> = text.chars().collect();
        syllables(&characters)
    }

    fn order(text: &str) -> String {
        let characters: Vec<char> = text.chars().collect();
        let mut out = String::new();
        for syllable in syllables(&characters) {
            for at in drawn_order(&characters, &syllable) {
                out.push(characters[at]);
            }
        }
        out
    }

    #[test]
    fn a_consonant_and_its_vowel_sign_are_one_syllable() {
        let text: String = [KA, I].iter().collect();
        let found = split(&text);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].len(), 2);
        assert_eq!(found[0].base, Some(0));
    }

    #[test]
    fn consonants_joined_by_halants_are_one_syllable() {
        // क्ख्ग — three consonants, two halants, one syllable, and the last
        // of them is what the rest hangs on.
        let text: String = [KA, HALANT, KHA, HALANT, GA].iter().collect();
        let found = split(&text);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].base, Some(4));
    }

    #[test]
    fn a_consonant_without_a_halant_begins_a_new_syllable() {
        let text: String = [KA, KHA].iter().collect();
        let found = split(&text);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].base, Some(0));
        assert_eq!(found[1].base, Some(1));
    }

    #[test]
    fn a_halant_at_the_end_belongs_to_the_syllable_it_ends() {
        // How a word ending in a bare consonant is written.
        let text: String = [KA, HALANT].iter().collect();
        let found = split(&text);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].len(), 2);
    }

    #[test]
    fn an_independent_vowel_is_a_syllable_of_its_own() {
        let text: String = [A, ANUSVARA, KA].iter().collect();
        let found = split(&text);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].len(), 2, "the dot belongs to the vowel");
        assert_eq!(found[1].base, Some(2));
    }

    #[test]
    fn a_digit_or_a_stop_is_a_syllable_with_nothing_to_hang_on() {
        let found = split("\u{0967} ");
        assert_eq!(found.len(), 2);
        assert!(found.iter().all(|syllable| syllable.base.is_none()));
    }

    #[test]
    fn the_vowel_sign_written_to_the_left_is_drawn_first() {
        // कि is stored consonant-then-sign and drawn sign-then-consonant.
        // This is the whole reason any of this exists.
        let text: String = [KA, I].iter().collect();
        let drawn: String = [I, KA].iter().collect();
        assert_eq!(order(&text), drawn);
    }

    #[test]
    fn it_is_drawn_before_the_whole_cluster_and_not_only_before_the_base() {
        // क्कि: the sign goes to the front of the lot, not between them.
        let text: String = [KA, HALANT, KA, I].iter().collect();
        let drawn: String = [I, KA, HALANT, KA].iter().collect();
        assert_eq!(order(&text), drawn);
    }

    #[test]
    fn the_signs_written_elsewhere_stay_where_they_are() {
        for sign in [AA, U, ANUSVARA] {
            let text: String = [KA, sign].iter().collect();
            assert_eq!(order(&text), text, "a sign that is not written to the left moved");
        }
    }

    #[test]
    fn a_syllable_beginning_with_ra_and_a_halant_draws_it_last() {
        // र्क: the r is stored first and drawn as a hook over the end.
        let text: String = [RA, HALANT, KA].iter().collect();
        let drawn: String = [KA, RA, HALANT].iter().collect();
        assert_eq!(order(&text), drawn);

        let found = split(&text);
        assert!(found[0].reph);
        assert_eq!(found[0].base, Some(2), "the r is not what the syllable hangs on");
    }

    #[test]
    fn the_hook_goes_before_a_vowel_sign_written_to_the_right() {
        // र्का: the hook sits over the consonant, and the sign is further
        // right than either.
        let text: String = [RA, HALANT, KA, AA].iter().collect();
        let drawn: String = [KA, RA, HALANT, AA].iter().collect();
        assert_eq!(order(&text), drawn);
    }

    #[test]
    fn both_moves_happen_at_once() {
        // र्कि: the sign to the front, the hook to the end.
        let text: String = [RA, HALANT, KA, I].iter().collect();
        let drawn: String = [I, KA, RA, HALANT].iter().collect();
        assert_eq!(order(&text), drawn);
    }

    #[test]
    fn a_ra_that_is_the_whole_syllable_is_a_letter_and_not_a_hook() {
        let text: String = [RA, AA].iter().collect();
        assert_eq!(order(&text), text);
        assert!(!split(&text)[0].reph);
    }

    #[test]
    fn a_joiner_after_the_halant_asks_for_the_letter_and_gets_it() {
        // What somebody writes when they want र् drawn as a letter with a
        // halant rather than as the hook.
        let text: String = [RA, HALANT, JOINER, KA].iter().collect();
        assert_eq!(order(&text), text);
        assert!(!split(&text)[0].reph);
    }

    #[test]
    fn a_ra_at_the_end_of_a_cluster_is_not_what_the_syllable_hangs_on() {
        // क्र: the r is drawn as a tail under the k, and the k is the base.
        let text: String = [KA, HALANT, RA].iter().collect();
        let found = split(&text);
        assert_eq!(found[0].base, Some(0));
        assert!(!found[0].reph);
    }

    #[test]
    fn a_nukta_belongs_to_the_consonant_before_it() {
        let text: String = [KA, NUKTA, HALANT, SA].iter().collect();
        let found = split(&text);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].base, Some(3));
    }

    #[test]
    fn a_word_of_several_syllables_is_split_where_the_vowels_are() {
        // सकल — three syllables, each a consonant with its own vowel.
        let text: String = [SA, KA, '\u{0932}'].iter().collect();
        assert_eq!(split(&text).len(), 3);
        assert_eq!(order(&text), text, "nothing here is drawn out of order");
    }

    #[test]
    fn every_character_comes_out_exactly_once() {
        // Whatever the rules do, nothing may be dropped or drawn twice: what
        // is drawn is what was written.
        for text in [
            vec![RA, HALANT, KA, I, ANUSVARA],
            vec![KA, HALANT, KHA, HALANT, GA, AA],
            vec![A, KA, NUKTA, U],
            vec![RA, HALANT, RA, HALANT, KA],
        ] {
            let written: String = text.iter().collect();
            let drawn = order(&written);
            let mut one: Vec<char> = written.chars().collect();
            let mut other: Vec<char> = drawn.chars().collect();
            one.sort_unstable();
            other.sort_unstable();
            assert_eq!(one, other, "{written:?} came out as {drawn:?}");
        }
    }
}
