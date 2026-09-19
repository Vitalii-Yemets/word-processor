//! The scripts of India and Sri Lanka: the syllable, and the order its pieces
//! are drawn in.
//!
//! # Why this is not a matter of substitution
//!
//! Every other script this program shapes is drawn in the order it is stored.
//! These are not. The vowel sign of "ki" — कि — is stored after the consonant
//! and drawn before it; the "r" of a cluster that begins with र् is stored
//! first and drawn last, as a hook over the end of the syllable. No
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
//! # Ten scripts, one shape of rule
//!
//! Devanagari, Bengali, Gurmukhi, Gujarati, Oriya, Tamil, Telugu, Kannada,
//! Malayalam and Sinhala are written on the same plan and differ in every
//! detail of it. Each has its own block of characters to be told apart —
//! which is a consonant, which a vowel sign and where that sign is drawn —
//! and each answers three questions its own way. *Which consonant does the
//! syllable hang on?* The last one in the northern scripts, where the
//! earlier ones take half forms; the first one in Telugu, Kannada and Oriya,
//! where the later ones are written small beneath it. *Where does the hook
//! go?* Over the end of the syllable in Devanagari, before the signs written
//! to the right; after everything in the south; only when a joiner asks for
//! it in Telugu and Sinhala; and in Malayalam it is a character of its own,
//! written first and drawn after the letter. *Which vowel signs are written
//! in two pieces?* Bengali's o is its e drawn before the consonant and its aa
//! drawn after, and the font knows the two pieces and not the whole: those
//! are split before anything else is asked.
//!
//! Each piece of a syllable is given a place in a fixed sequence — the signs
//! before the letter, the letters before the base, the base, the forms
//! beneath it, the signs above and below, the hook where the script puts it,
//! the letters after, the signs after, the dots that belong to the whole —
//! and the pieces are sorted into that sequence. That is the whole of the
//! reordering, for every script alike.

use wp_font::{Font, GlyphId};

use crate::gsub::Substitutions;

/// The scripts written this way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Script {
    Devanagari,
    Bengali,
    Gurmukhi,
    Gujarati,
    Oriya,
    Tamil,
    Telugu,
    Kannada,
    Malayalam,
    Sinhala,
}

impl Script {
    /// Every one of them.
    pub const ALL: [Self; 10] = [
        Self::Devanagari,
        Self::Bengali,
        Self::Gurmukhi,
        Self::Gujarati,
        Self::Oriya,
        Self::Tamil,
        Self::Telugu,
        Self::Kannada,
        Self::Malayalam,
        Self::Sinhala,
    ];

    /// The script a character belongs to, by its block, if it belongs to one
    /// of these.
    #[must_use]
    pub fn of(character: char) -> Option<Self> {
        Some(match character as u32 {
            0x0900..=0x097F => Self::Devanagari,
            0x0980..=0x09FF => Self::Bengali,
            0x0A00..=0x0A7F => Self::Gurmukhi,
            0x0A80..=0x0AFF => Self::Gujarati,
            0x0B00..=0x0B7F => Self::Oriya,
            0x0B80..=0x0BFF => Self::Tamil,
            0x0C00..=0x0C7F => Self::Telugu,
            0x0C80..=0x0CFF => Self::Kannada,
            0x0D00..=0x0D7F => Self::Malayalam,
            0x0D80..=0x0DFF => Self::Sinhala,
            _ => return None,
        })
    }

    /// The script a run of text is in: the first character that belongs to
    /// one of these decides.
    #[must_use]
    pub fn of_text(text: &str) -> Option<Self> {
        text.chars().find_map(Self::of)
    }

    /// The two names the script goes by in a font.
    ///
    /// The scripts that reorder were given new tags in 2005, when the rules
    /// were settled; a font written since then carries the new one and an
    /// older font carries the old. A font may carry both, and then the new
    /// one is what its designer meant. Sinhala was never given a new one.
    #[must_use]
    pub fn tags(self) -> [[u8; 4]; 2] {
        match self {
            Self::Devanagari => [*b"dev2", *b"deva"],
            Self::Bengali => [*b"bng2", *b"beng"],
            Self::Gurmukhi => [*b"gur2", *b"guru"],
            Self::Gujarati => [*b"gjr2", *b"gujr"],
            Self::Oriya => [*b"ory2", *b"orya"],
            Self::Tamil => [*b"tml2", *b"taml"],
            Self::Telugu => [*b"tel2", *b"telu"],
            Self::Kannada => [*b"knd2", *b"knda"],
            Self::Malayalam => [*b"mlm2", *b"mlym"],
            Self::Sinhala => [*b"sinh", *b"sinh"],
        }
    }

    /// The script one of those tags names.
    #[must_use]
    pub fn of_tag(tag: &[u8; 4]) -> Option<Self> {
        Self::ALL.into_iter().find(|script| script.tags().contains(tag))
    }

    /// Which of the two names a font knows the script by.
    #[must_use]
    pub fn tag_in(self, table: &Substitutions<'_>) -> [u8; 4] {
        let [new, old] = self.tags();
        if table.has_script(&new) {
            new
        } else {
            old
        }
    }

    /// Whether the syllable hangs on its first consonant, the rest being
    /// written small beneath it — or, in Sinhala, joined to it only when a
    /// joiner asks — rather than on its last.
    fn base_first(self) -> bool {
        matches!(self, Self::Telugu | Self::Kannada | Self::Oriya | Self::Sinhala)
    }

    /// Whether the hook is made only when a joiner asks for it, as it is in
    /// the two scripts where Ra and a halant at the front of a syllable are
    /// otherwise a letter like any other.
    fn reph_explicit(self) -> bool {
        matches!(self, Self::Telugu | Self::Sinhala)
    }

    /// Where the hook is drawn, as a place in the sequence of a syllable.
    fn reph_place(self) -> Place {
        match self {
            Self::Devanagari | Self::Gujarati => Place::BeforePost,
            Self::Bengali => Place::AfterSub,
            Self::Gurmukhi => Place::BeforeSub,
            Self::Oriya | Self::Malayalam => Place::AfterMain,
            Self::Tamil | Self::Telugu | Self::Kannada | Self::Sinhala => Place::AfterPost,
        }
    }

    /// Whether a consonant after the base is written in a form of its own
    /// beneath or after the base, so that it is never the base while there is
    /// anything before it to be: Ra's tail in the north, and the few others
    /// that are written that way.
    fn takes_form_after_base(self, character: char) -> bool {
        let value = character as u32;
        match self {
            Self::Devanagari | Self::Gujarati => matches!(category_of(character), Category::Ra),
            // Ra-phala beneath and ya-phala after.
            Self::Bengali => matches!(value, 0x09B0 | 0x09AF),
            // The three written beneath: ra, va and ha.
            Self::Gurmukhi => matches!(value, 0x0A30 | 0x0A35 | 0x0A39),
            // Ya, va, la and ra, all written after or before rather than as
            // half of a conjunct.
            Self::Malayalam => matches!(value, 0x0D2F | 0x0D35 | 0x0D32 | 0x0D30),
            _ => false,
        }
    }

    /// The pieces a vowel sign written in two parts is made of, for the few
    /// that are: what the font knows, since it draws the two parts and not
    /// the whole. Unicode's own decompositions.
    #[must_use]
    pub fn split(character: char) -> Option<&'static [char]> {
        Some(match character as u32 {
            0x09CB => &['\u{09C7}', '\u{09BE}'],
            0x09CC => &['\u{09C7}', '\u{09D7}'],
            0x0B48 => &['\u{0B47}', '\u{0B56}'],
            0x0B4B => &['\u{0B47}', '\u{0B3E}'],
            0x0B4C => &['\u{0B47}', '\u{0B57}'],
            0x0BCA => &['\u{0BC6}', '\u{0BBE}'],
            0x0BCB => &['\u{0BC7}', '\u{0BBE}'],
            0x0BCC => &['\u{0BC6}', '\u{0BD7}'],
            0x0C48 => &['\u{0C46}', '\u{0C56}'],
            0x0CC0 => &['\u{0CBF}', '\u{0CD5}'],
            0x0CC7 => &['\u{0CC6}', '\u{0CD5}'],
            0x0CC8 => &['\u{0CC6}', '\u{0CD6}'],
            0x0CCA => &['\u{0CC6}', '\u{0CC2}'],
            0x0CCB => &['\u{0CC6}', '\u{0CC2}', '\u{0CD5}'],
            0x0D4A => &['\u{0D46}', '\u{0D3E}'],
            0x0D4B => &['\u{0D47}', '\u{0D3E}'],
            0x0D4C => &['\u{0D46}', '\u{0D57}'],
            0x0DDA => &['\u{0DD9}', '\u{0DCA}'],
            0x0DDC => &['\u{0DD9}', '\u{0DCF}'],
            0x0DDD => &['\u{0DD9}', '\u{0DCF}', '\u{0DCA}'],
            0x0DDE => &['\u{0DD9}', '\u{0DDF}'],
            _ => return None,
        })
    }
}

/// What a character is, as far as a syllable is concerned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    /// A consonant, which is what a syllable is built from.
    Consonant,
    /// The one consonant with rules of its own: at the front of a cluster it
    /// is drawn as a hook over the end of it, and at the back as a tail under
    /// the middle.
    Ra,
    /// Malayalam's hook written as a character of its own, at the front of
    /// the syllable and drawn after the letter.
    Repha,
    /// A vowel written as a letter rather than as a sign on a consonant.
    Vowel,
    /// A vowel sign, which hangs on the consonant before it — and, when it is
    /// one of those written to the left, is drawn before it.
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

/// What a character is, in whichever of the scripts it belongs to.
///
/// The three that matter from outside the blocks are the two joiners and the
/// dotted circle. Each block is a hundred and twenty-eight characters told
/// apart by hand, the same way for every script.
#[must_use]
pub fn category_of(character: char) -> Category {
    use Category::{Consonant, Halant, Matra, Modifier, Nukta, Other, Placeholder, Ra, Vowel};
    use Position::{After, Before, Below};
    let above = Modifier(Position::Above);
    let over = Matra(Position::Above);
    let value = character as u32;
    match value {
        0x200C => Category::NonJoiner,
        0x200D => Category::Joiner,
        0x25CC => Placeholder,

        // --- Devanagari ---------------------------------------------------
        0x0900..=0x0902 => above,
        0x0903 => Modifier(After),
        0x0904..=0x0914 | 0x0960 | 0x0961 | 0x0972..=0x0977 => Vowel,
        0x0930 | 0x0931 | 0x095D => Ra,
        0x0915..=0x0939 | 0x0958..=0x095F | 0x0979..=0x097F => Consonant,
        0x093A => over,
        0x093B => Matra(After),
        0x093C => Nukta,
        0x093D | 0x0950 => Placeholder,
        0x093E => Matra(After),
        0x093F => Matra(Before),
        0x0940 => Matra(After),
        0x0941..=0x0944 => Matra(Below),
        0x0945..=0x0948 => over,
        0x0949..=0x094C => Matra(After),
        0x094D => Halant,
        0x094E => Matra(Before),
        0x094F => Matra(After),
        0x0951 | 0x0953 | 0x0954 => above,
        0x0952 | 0x0955..=0x0957 => Modifier(Below),
        0x0962 | 0x0963 => Matra(Below),

        // --- Bengali ------------------------------------------------------
        0x0980 | 0x09BD | 0x09FC => Placeholder,
        0x0981 | 0x09FE => above,
        0x0982 | 0x0983 => Modifier(After),
        0x0985..=0x098C | 0x098F | 0x0990 | 0x0993 | 0x0994 | 0x09E0 | 0x09E1 => Vowel,
        0x09B0 | 0x09F0 => Ra,
        0x0995..=0x09A8 | 0x09AA..=0x09AF | 0x09B2 | 0x09B6..=0x09B9 => Consonant,
        0x09CE | 0x09DC | 0x09DD | 0x09DF | 0x09F1 => Consonant,
        0x09BC => Nukta,
        0x09BE | 0x09C0 | 0x09D7 => Matra(After),
        0x09BF | 0x09C7 | 0x09C8 => Matra(Before),
        0x09C1..=0x09C4 | 0x09E2 | 0x09E3 => Matra(Below),
        0x09CB | 0x09CC => Matra(After),
        0x09CD => Halant,

        // --- Gurmukhi -----------------------------------------------------
        0x0A01 | 0x0A02 | 0x0A70 | 0x0A71 => above,
        0x0A03 => Modifier(After),
        0x0A05..=0x0A0A | 0x0A0F | 0x0A10 | 0x0A13 | 0x0A14 => Vowel,
        0x0A30 => Ra,
        0x0A15..=0x0A28 | 0x0A2A..=0x0A2F | 0x0A32 | 0x0A33 | 0x0A35 | 0x0A36 => Consonant,
        0x0A38 | 0x0A39 | 0x0A59..=0x0A5C | 0x0A5E | 0x0A72 | 0x0A73 => Consonant,
        0x0A3C => Nukta,
        0x0A3E | 0x0A40 => Matra(After),
        0x0A3F => Matra(Before),
        0x0A41 | 0x0A42 | 0x0A75 => Matra(Below),
        0x0A47 | 0x0A48 | 0x0A4B | 0x0A4C => over,
        0x0A4D => Halant,
        0x0A51 => Modifier(Below),

        // --- Gujarati -----------------------------------------------------
        0x0A81 | 0x0A82 | 0x0AFA..=0x0AFF => above,
        0x0A83 => Modifier(After),
        0x0A85..=0x0A8D | 0x0A8F..=0x0A91 | 0x0A93 | 0x0A94 | 0x0AE0 | 0x0AE1 => Vowel,
        0x0AB0 => Ra,
        0x0A95..=0x0AA8 | 0x0AAA..=0x0AAF | 0x0AB2 | 0x0AB3 | 0x0AB5..=0x0AB9 | 0x0AF9 => Consonant,
        0x0ABC => Nukta,
        0x0ABD | 0x0AD0 => Placeholder,
        0x0ABE | 0x0AC0 | 0x0AC9 | 0x0ACB | 0x0ACC => Matra(After),
        0x0ABF => Matra(Before),
        0x0AC1..=0x0AC4 | 0x0AE2 | 0x0AE3 => Matra(Below),
        0x0AC5 | 0x0AC7 | 0x0AC8 => over,
        0x0ACD => Halant,

        // --- Oriya --------------------------------------------------------
        0x0B01 => above,
        0x0B02 | 0x0B03 => Modifier(After),
        0x0B05..=0x0B0C | 0x0B0F | 0x0B10 | 0x0B13 | 0x0B14 | 0x0B60 | 0x0B61 => Vowel,
        0x0B30 => Ra,
        0x0B15..=0x0B28 | 0x0B2A..=0x0B2F | 0x0B32 | 0x0B33 | 0x0B35..=0x0B39 => Consonant,
        0x0B5C | 0x0B5D | 0x0B5F | 0x0B71 => Consonant,
        0x0B3C => Nukta,
        0x0B3D => Placeholder,
        0x0B3E | 0x0B40 | 0x0B57 => Matra(After),
        0x0B3F | 0x0B55 | 0x0B56 => over,
        0x0B41..=0x0B44 | 0x0B62 | 0x0B63 => Matra(Below),
        0x0B47 => Matra(Before),
        0x0B48 | 0x0B4B | 0x0B4C => Matra(After),
        0x0B4D => Halant,

        // --- Tamil --------------------------------------------------------
        0x0B82 => above,
        // The aytham stands as a letter of its own.
        0x0B83 | 0x0B85..=0x0B8A | 0x0B8E..=0x0B90 | 0x0B92..=0x0B94 => Vowel,
        0x0BB0 => Ra,
        0x0B95 | 0x0B99 | 0x0B9A | 0x0B9C | 0x0B9E | 0x0B9F | 0x0BA3 | 0x0BA4 => Consonant,
        0x0BA8..=0x0BAA | 0x0BAE..=0x0BAF | 0x0BB1..=0x0BB9 => Consonant,
        0x0BBE..=0x0BC2 | 0x0BD7 => Matra(After),
        0x0BC6..=0x0BC8 => Matra(Before),
        0x0BCA..=0x0BCC => Matra(After),
        0x0BCD => Halant,
        0x0BD0 => Placeholder,

        // --- Telugu -------------------------------------------------------
        0x0C00 | 0x0C01 | 0x0C04 => above,
        0x0C02 | 0x0C03 => Modifier(After),
        0x0C05..=0x0C0C | 0x0C0E..=0x0C10 | 0x0C12..=0x0C14 | 0x0C60 | 0x0C61 => Vowel,
        0x0C30 => Ra,
        0x0C15..=0x0C28 | 0x0C2A..=0x0C2F | 0x0C31..=0x0C39 | 0x0C58..=0x0C5A | 0x0C5D => Consonant,
        0x0C3C => Nukta,
        0x0C3D => Placeholder,
        0x0C3E..=0x0C40 | 0x0C46 | 0x0C47 | 0x0C4A..=0x0C4C | 0x0C55 => over,
        0x0C48 => over,
        0x0C41..=0x0C44 => Matra(After),
        0x0C56 | 0x0C62 | 0x0C63 => Matra(Below),
        0x0C4D => Halant,

        // --- Kannada ------------------------------------------------------
        0x0C80 | 0x0CBD => Placeholder,
        0x0C81 => above,
        0x0C82 | 0x0C83 | 0x0CF3 => Modifier(After),
        0x0C85..=0x0C8C | 0x0C8E..=0x0C90 | 0x0C92..=0x0C94 | 0x0CE0 | 0x0CE1 => Vowel,
        0x0CB0 => Ra,
        0x0C95..=0x0CA8 | 0x0CAA..=0x0CAF | 0x0CB1..=0x0CB3 | 0x0CB5..=0x0CB9 => Consonant,
        0x0CDD | 0x0CDE | 0x0CF1 | 0x0CF2 => Consonant,
        0x0CBC => Nukta,
        0x0CBE | 0x0CC1..=0x0CC4 | 0x0CD5 | 0x0CD6 => Matra(After),
        0x0CBF | 0x0CC6 | 0x0CCC => over,
        0x0CC0 | 0x0CC7 | 0x0CC8 | 0x0CCA | 0x0CCB => Matra(After),
        0x0CE2 | 0x0CE3 => Matra(Below),
        0x0CCD => Halant,

        // --- Malayalam ----------------------------------------------------
        0x0D00 | 0x0D01 | 0x0D04 => above,
        0x0D02 | 0x0D03 => Modifier(After),
        0x0D05..=0x0D0C | 0x0D0E..=0x0D10 | 0x0D12..=0x0D14 | 0x0D60 | 0x0D61 => Vowel,
        0x0D30 => Ra,
        0x0D15..=0x0D2F | 0x0D31..=0x0D3A | 0x0D54..=0x0D56 | 0x0D5F | 0x0D7A..=0x0D7F => Consonant,
        0x0D3B | 0x0D3C | 0x0D4D => Halant,
        0x0D3D | 0x0D4F => Placeholder,
        0x0D3E..=0x0D44 | 0x0D57 => Matra(After),
        0x0D46..=0x0D48 => Matra(Before),
        0x0D4A..=0x0D4C => Matra(After),
        0x0D4E => Category::Repha,
        0x0D62 | 0x0D63 => Matra(Below),

        // --- Sinhala ------------------------------------------------------
        0x0D81 => above,
        0x0D82 | 0x0D83 => Modifier(After),
        0x0D85..=0x0D96 => Vowel,
        0x0DBB => Ra,
        0x0D9A..=0x0DB1 | 0x0DB3..=0x0DBA | 0x0DBD | 0x0DC0..=0x0DC6 => Consonant,
        0x0DCA => Halant,
        0x0DCF..=0x0DD1 | 0x0DD8 | 0x0DDF | 0x0DF2 | 0x0DF3 => Matra(After),
        0x0DD2 | 0x0DD3 => over,
        0x0DD4 | 0x0DD6 => Matra(Below),
        0x0DD9 | 0x0DDB => Matra(Before),
        0x0DDA | 0x0DDC..=0x0DDE => Matra(After),

        _ => Other,
    }
}

/// One character of a run as the syllable rules see it: what it is, and
/// which character of the text it came from — several pieces come from one
/// character when a vowel sign is written in two parts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Piece {
    pub character: char,
    pub category: Category,
    pub origin: usize,
}

/// The pieces of a run of text: every character, with the vowel signs that
/// are written in two parts split into them.
#[must_use]
pub fn pieces_of(characters: &[char]) -> Vec<Piece> {
    let mut out = Vec::with_capacity(characters.len());
    for (origin, character) in characters.iter().enumerate() {
        match Script::split(*character) {
            Some(parts) => {
                for part in parts {
                    // A halant that is half of a vowel sign is a sign, not a
                    // halant: Sinhala writes two of its vowels that way.
                    let category = match category_of(*part) {
                        Category::Halant => Category::Matra(Position::After),
                        other => other,
                    };
                    out.push(Piece { character: *part, category, origin });
                }
            }
            None => {
                out.push(Piece { character: *character, category: category_of(*character), origin })
            }
        }
    }
    out
}

/// One syllable: where it starts, where it ends, and which of its consonants
/// the rest of it hangs on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Syllable {
    /// Indices into the run of pieces, the end being one past the last.
    pub start: usize,
    pub end: usize,
    /// Which piece is the base consonant, if the syllable has one.
    pub base: Option<usize>,
    /// Whether it begins with a Ra and a halant that are drawn as a hook over
    /// the end of the syllable rather than as a letter at the front of it.
    pub reph: bool,
}

impl Syllable {
    /// How many pieces it holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.end - self.start
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.start >= self.end
    }
}

/// Splits a run of pieces into syllables.
///
/// Everything is in one syllable or another: a digit or a stop is a syllable
/// of one piece with no base, which keeps the rest of the machinery from
/// having to ask whether it is looking at one of these scripts at all.
#[must_use]
pub fn syllables(script: Script, pieces: &[Piece]) -> Vec<Syllable> {
    let mut out = Vec::new();
    let mut at = 0usize;

    while at < pieces.len() {
        let start = at;
        match pieces[at].category {
            Category::Consonant | Category::Ra | Category::Placeholder => {
                at = consonants(script, pieces, at);
                at = tail(pieces, at);
            }
            // Malayalam's hook: a character at the front, and then the
            // syllable it belongs to.
            Category::Repha
                if matches!(
                    pieces.get(at + 1).map(|piece| piece.category),
                    Some(Category::Consonant | Category::Ra | Category::Placeholder)
                ) =>
            {
                at = consonants(script, pieces, at + 1);
                at = tail(pieces, at);
            }
            Category::Vowel => {
                at += 1;
                at = tail(pieces, at);
            }
            // A sign with nothing before it to hang on, or anything that is
            // not one of these scripts at all: one piece, and no base.
            _ => at += 1,
        }
        let mut syllable = Syllable { start, end: at, base: None, reph: false };
        settle(script, pieces, &mut syllable);
        out.push(syllable);
    }

    out
}

fn category_at(pieces: &[Piece], at: usize) -> Option<Category> {
    pieces.get(at).map(|piece| piece.category)
}

/// Walks the run of consonants joined by halants, ending after the last
/// consonant of it.
fn consonants(script: Script, pieces: &[Piece], from: usize) -> usize {
    let mut at = from;
    loop {
        // A consonant, then whatever may hang on it before the halant.
        at += 1;
        if category_at(pieces, at) == Some(Category::Nukta) {
            at += 1;
        }
        if matches!(category_at(pieces, at), Some(Category::Joiner | Category::NonJoiner)) {
            at += 1;
        }

        // A halant joins this consonant to whatever follows. Without one the
        // consonant keeps its vowel and the run of them is over.
        if category_at(pieces, at) != Some(Category::Halant) {
            return at;
        }
        let halant = at;
        at += 1;
        let joined =
            matches!(category_at(pieces, at), Some(Category::Joiner | Category::NonJoiner));
        if joined {
            at += 1;
        }

        // A halant at the end of the syllable belongs to it — it is how a
        // word ending in a bare consonant is written — but it joins nothing.
        // In Sinhala it joins nothing unless a joiner says so: a consonant
        // with the al-lakuna is a letter complete in itself, and two letters
        // touch only when they are asked to.
        let continues = matches!(
            category_at(pieces, at),
            Some(Category::Consonant | Category::Ra | Category::Placeholder)
        ) && (script != Script::Sinhala || joined);
        if !continues {
            return halant + 1;
        }
    }
}

/// Everything that may follow the last consonant: the vowel signs, then the
/// dots that belong to the whole syllable.
fn tail(pieces: &[Piece], from: usize) -> usize {
    let mut at = from;
    while matches!(category_at(pieces, at), Some(Category::Matra(_) | Category::Nukta)) {
        at += 1;
    }
    while matches!(category_at(pieces, at), Some(Category::Modifier(_))) {
        at += 1;
    }
    at
}

fn is_letter(piece: &Piece) -> bool {
    matches!(
        piece.category,
        Category::Consonant | Category::Ra | Category::Vowel | Category::Placeholder
    )
}

/// Works out which consonant a syllable hangs on, and whether it begins with
/// the hook.
fn settle(script: Script, pieces: &[Piece], syllable: &mut Syllable) {
    let mut first = syllable.start;

    // Malayalam's hook is a character of its own at the front; the letters
    // begin after it.
    if pieces[syllable.start].category == Category::Repha && syllable.len() > 1 {
        first = syllable.start + 1;
    }

    // A syllable that begins with Ra and a halant, and has something after
    // them, begins with the hook: that Ra is not a letter of the syllable, it
    // is a mark drawn over the end of it. In Telugu and Sinhala only when a
    // joiner follows the halant and asks for it; elsewhere a joiner there
    // asks for the letter instead.
    if syllable.len() > 2
        && pieces[syllable.start].category == Category::Ra
        && pieces[syllable.start + 1].category == Category::Halant
    {
        let joiner = category_at(pieces, syllable.start + 2) == Some(Category::Joiner);
        let asked = if script.reph_explicit() { joiner } else { !joiner };
        let rest = syllable.start + if joiner { 3 } else { 2 };
        if asked && (rest..syllable.end).any(|at| is_letter(&pieces[at])) {
            syllable.reph = true;
            first = rest;
        }
    }

    // The base: the first letter where the rest are written small beneath
    // it, else the last letter — the one that keeps its vowel — passing over
    // a consonant that is drawn as a tail or a sign of the letter before it
    // rather than as a letter of its own.
    let letters: Vec<usize> = (first..syllable.end).filter(|at| is_letter(&pieces[*at])).collect();
    syllable.base = if script.base_first() {
        letters.first().copied()
    } else {
        let mut base = None;
        for at in letters {
            let after_halant = at > first && pieces[at - 1].category == Category::Halant;
            if after_halant && base.is_some() && script.takes_form_after_base(pieces[at].character)
            {
                continue;
            }
            base = Some(at);
        }
        base
    };
}

/// A place in the sequence a syllable is drawn in. The order of the variants
/// is the order on the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Place {
    /// The vowel signs written to the left of everything.
    PreMatra,
    /// A consonant drawn before the base as a sign — Malayalam's ra.
    PreForm,
    /// The consonants before the base: the half forms.
    PreConsonant,
    Base,
    AfterMain,
    BeforeSub,
    /// The consonants written beneath the base.
    BelowConsonant,
    /// The signs written above and below.
    AfterSub,
    BeforePost,
    /// The consonants written after the base.
    PostConsonant,
    /// The signs written to the right.
    AfterPost,
    /// The dots and hooks that belong to the whole syllable.
    Modifier,
}

/// The order the pieces of a syllable are drawn in.
///
/// Gives back indices into the run of pieces. Every piece is given a place
/// in the sequence and the pieces are sorted into it, keeping the order they
/// were written in where two share a place. `pre_forms` names the pieces a
/// font has said it draws before the base.
#[must_use]
pub fn drawn_order(
    script: Script,
    pieces: &[Piece],
    syllable: &Syllable,
    pre_forms: &[usize],
) -> Vec<usize> {
    let base = syllable.base;
    let reph_place = script.reph_place();
    let mut placed: Vec<(Place, u8, usize)> = Vec::with_capacity(syllable.len());

    // The place of a consonant: before the base, the base, or after it —
    // beneath it in the scripts that stack, or as a tail for Ra, or after.
    let consonant_place = |at: usize| -> Place {
        match base {
            Some(base) if at < base => Place::PreConsonant,
            Some(base) if at == base => Place::Base,
            Some(_) => {
                if script.base_first() || pieces[at].category == Category::Ra {
                    Place::BelowConsonant
                } else {
                    Place::PostConsonant
                }
            }
            None => Place::Base,
        }
    };

    let mut at = syllable.start;
    while at < syllable.end {
        let piece = &pieces[at];
        // The hook: the Ra and the halant that made it are one mark, drawn
        // where the script draws its hook.
        if syllable.reph && at == syllable.start {
            placed.push((reph_place, 1, at));
            placed.push((reph_place, 1, at + 1));
            at += 2;
            continue;
        }
        if pre_forms.contains(&at) {
            placed.push((Place::PreForm, 0, at));
            at += 1;
            continue;
        }
        let place = match piece.category {
            Category::Matra(Position::Before) => Place::PreMatra,
            Category::Matra(Position::Above | Position::Below) => Place::AfterSub,
            Category::Matra(Position::After) => Place::AfterPost,
            Category::Modifier(_) => Place::Modifier,
            Category::Repha => reph_place,
            Category::Consonant | Category::Ra | Category::Vowel | Category::Placeholder => {
                consonant_place(at)
            }
            // A halant goes with the consonant it joins to the one after it,
            // and a halant that joins nothing stays with the letter before.
            Category::Halant => {
                let next = (at + 1..syllable.end).find(|next| {
                    !matches!(pieces[*next].category, Category::Joiner | Category::NonJoiner)
                });
                match next {
                    Some(next) if is_letter(&pieces[next]) && !pre_forms.contains(&next) => {
                        consonant_place(next)
                    }
                    Some(next) if pre_forms.contains(&next) => Place::PreForm,
                    _ => {
                        let (place, sub, _) =
                            placed.last().copied().unwrap_or((Place::Base, 0, at));
                        placed.push((place, sub, at));
                        at += 1;
                        continue;
                    }
                }
            }
            // A nukta or a joiner belongs to whatever came before it, hook
            // and all.
            Category::Nukta | Category::Joiner | Category::NonJoiner | Category::Other => {
                let (place, sub, _) = placed.last().copied().unwrap_or((Place::Base, 0, at));
                placed.push((place, sub, at));
                at += 1;
                continue;
            }
        };
        let sub = if piece.category == Category::Repha { 1 } else { 0 };
        placed.push((place, sub, at));
        at += 1;
    }

    // Stable, so that two pieces in the same place keep the order they were
    // written in: the halant before the consonant it joins, the nukta after
    // its letter.
    placed.sort_by_key(|(place, sub, _)| (*place, *sub));
    placed.into_iter().map(|(_, _, at)| at).collect()
}

/// The two names Devanagari goes by in a font, kept for what asks for them.
pub const TAGS: [[u8; 4]; 2] = [*b"dev2", *b"deva"];

/// Which of the two names this font knows Devanagari by.
#[must_use]
pub fn tag_in(table: &Substitutions<'_>) -> [u8; 4] {
    Script::Devanagari.tag_in(table)
}

/// Turns text in one of these scripts into glyphs: the syllable, the forms,
/// and the order.
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
    let which =
        Script::of_tag(script).or_else(|| Script::of_text(text)).unwrap_or(Script::Devanagari);
    // Where each character begins, which is what a glyph carries so that a
    // caret can be put back between two of them.
    let mut offsets = Vec::with_capacity(characters.len() + 1);
    let mut at = 0usize;
    for character in &characters {
        offsets.push(at);
        at += character.len_utf8();
    }
    offsets.push(at);
    let pieces = pieces_of(&characters);

    let mut glyphs = Vec::with_capacity(pieces.len());
    let mut clusters = Vec::with_capacity(pieces.len());
    for syllable in syllables(which, &pieces) {
        let (piece, places) = one(font, table, script, which, &pieces, &offsets, &syllable);
        glyphs.extend(piece);
        clusters.extend(places);
    }
    (glyphs, clusters)
}

/// One syllable, from pieces to the glyphs that draw it.
fn one(
    font: &Font<'_>,
    table: &Substitutions<'_>,
    script: &[u8; 4],
    which: Script,
    pieces: &[Piece],
    offsets: &[usize],
    syllable: &Syllable,
) -> (Vec<GlyphId>, Vec<usize>) {
    let mut glyphs: Vec<GlyphId> = (syllable.start..syllable.end)
        .map(|at| font.glyph_for(pieces[at].character).unwrap_or(GlyphId(0)))
        .collect();
    let mut clusters: Vec<usize> =
        (syllable.start..syllable.end).map(|at| offsets[pieces[at].origin]).collect();
    if glyphs.is_empty() {
        return (glyphs, clusters);
    }
    // A hook is a hook only when the font can make one: a font with no rule
    // for it draws the Ra as the letter it is.
    let mut syllable = *syllable;
    if syllable.reph && table.lookups_for(script, b"rphf").is_empty() {
        syllable.reph = false;
        settle_without_reph(which, pieces, &mut syllable);
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
    // hangs on; and the forms written before, under and after it, which are
    // the ones following it. Each is asked of its own part of the syllable,
    // because a font will spell a half form out of any consonant and a halant
    // and would make one where a form below the line belongs.
    let mut pre_forms: Vec<usize> = Vec::new();
    let base = syllable.base.map(|at| offsets[pieces[at].origin]);
    if let Some(base) = base {
        let split = clusters.iter().position(|cluster| *cluster >= base).unwrap_or(0);
        part(table, script, b"half", &mut glyphs, &mut clusters, 0..split);
        for feature in [b"pref", b"blwf", b"pstf"] {
            let from = clusters.iter().position(|cluster| *cluster >= base).unwrap_or(0);
            let end = glyphs.len();
            let before: Vec<GlyphId> = glyphs[from..end].to_vec();
            part(table, script, feature, &mut glyphs, &mut clusters, from..end);
            // A consonant the font draws before the base — Malayalam's ra —
            // is the one whose glyph the pre-base form changed.
            if feature == b"pref" {
                for (index, glyph) in glyphs[from..].iter().enumerate() {
                    if before.get(index).is_some_and(|was| was != glyph) {
                        let cluster = clusters[from + index];
                        pre_forms.extend((syllable.start..syllable.end).filter(|at| {
                            offsets[pieces[*at].origin] == cluster
                                && matches!(
                                    pieces[*at].category,
                                    Category::Ra | Category::Consonant
                                )
                        }));
                    }
                }
            }
        }
    }
    for feature in [b"vatu", b"cjct"] {
        whole(table, script, feature, &mut glyphs, &mut clusters);
    }

    // And now the order they are drawn in. A glyph belongs where the character
    // it came from belongs: a form made of two characters carries the first of
    // them, which is the one whose place decides.
    let order = drawn_order(which, pieces, &syllable, &pre_forms);
    let mut rank = vec![usize::MAX; offsets.len()];
    for (place, at) in order.iter().enumerate() {
        let origin = pieces[*at].origin;
        // The first piece of a character is what places the character.
        if rank[origin] == usize::MAX {
            rank[origin] = place;
        }
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

/// Settles a syllable again once its hook has turned out to be a letter.
fn settle_without_reph(script: Script, pieces: &[Piece], syllable: &mut Syllable) {
    let letters: Vec<usize> =
        (syllable.start..syllable.end).filter(|at| is_letter(&pieces[*at])).collect();
    syllable.base =
        if script.base_first() { letters.first().copied() } else { letters.last().copied() };
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

    /// The Devanagari letters used below, so the tests read as the words they
    /// are.
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
        let script = Script::of_text(text).unwrap_or(Script::Devanagari);
        syllables(script, &pieces_of(&characters))
    }

    /// The pieces of a text in the order they are drawn, as a string.
    fn order(text: &str) -> String {
        let characters: Vec<char> = text.chars().collect();
        let script = Script::of_text(text).unwrap_or(Script::Devanagari);
        let pieces = pieces_of(&characters);
        let mut out = String::new();
        for syllable in syllables(script, &pieces) {
            for at in drawn_order(script, &pieces, &syllable, &[]) {
                out.push(pieces[at].character);
            }
        }
        out
    }

    fn text(characters: &[char]) -> String {
        characters.iter().collect()
    }

    #[test]
    fn a_consonant_and_its_vowel_sign_are_one_syllable() {
        let found = split(&text(&[KA, I]));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].len(), 2);
        assert_eq!(found[0].base, Some(0));
    }

    #[test]
    fn consonants_joined_by_halants_are_one_syllable() {
        // क्ख्ग — three consonants, two halants, one syllable, and the last
        // of them is what the rest hangs on.
        let found = split(&text(&[KA, HALANT, KHA, HALANT, GA]));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].base, Some(4));
    }

    #[test]
    fn a_consonant_without_a_halant_begins_a_new_syllable() {
        let found = split(&text(&[KA, KHA]));
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].base, Some(0));
        assert_eq!(found[1].base, Some(1));
    }

    #[test]
    fn a_halant_at_the_end_belongs_to_the_syllable_it_ends() {
        // How a word ending in a bare consonant is written.
        let found = split(&text(&[KA, HALANT]));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].len(), 2);
    }

    #[test]
    fn an_independent_vowel_is_a_syllable_of_its_own() {
        let found = split(&text(&[A, ANUSVARA, KA]));
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
        assert_eq!(order(&text(&[KA, I])), text(&[I, KA]));
    }

    #[test]
    fn it_is_drawn_before_the_whole_cluster_and_not_only_before_the_base() {
        // क्कि: the sign goes to the front of the lot, not between them.
        assert_eq!(order(&text(&[KA, HALANT, KA, I])), text(&[I, KA, HALANT, KA]));
    }

    #[test]
    fn the_signs_written_elsewhere_stay_where_they_are() {
        for sign in [AA, U, ANUSVARA] {
            let written = text(&[KA, sign]);
            assert_eq!(order(&written), written, "a sign that is not written to the left moved");
        }
    }

    #[test]
    fn a_syllable_beginning_with_ra_and_a_halant_draws_it_last() {
        // र्क: the r is stored first and drawn as a hook over the end.
        let written = text(&[RA, HALANT, KA]);
        assert_eq!(order(&written), text(&[KA, RA, HALANT]));
        let found = split(&written);
        assert!(found[0].reph);
        assert_eq!(found[0].base, Some(2), "the r is not what the syllable hangs on");
    }

    #[test]
    fn the_hook_goes_before_a_vowel_sign_written_to_the_right() {
        // र्का: the hook sits over the consonant, and the sign is further
        // right than either.
        assert_eq!(order(&text(&[RA, HALANT, KA, AA])), text(&[KA, RA, HALANT, AA]));
    }

    #[test]
    fn both_moves_happen_at_once() {
        // र्कि: the sign to the front, the hook to the end.
        assert_eq!(order(&text(&[RA, HALANT, KA, I])), text(&[I, KA, RA, HALANT]));
    }

    #[test]
    fn a_ra_that_is_the_whole_syllable_is_a_letter_and_not_a_hook() {
        let written = text(&[RA, AA]);
        assert_eq!(order(&written), written);
        assert!(!split(&written)[0].reph);
    }

    #[test]
    fn a_joiner_after_the_halant_asks_for_the_letter_and_gets_it() {
        // What somebody writes when they want र् drawn as a letter with a
        // halant rather than as the hook.
        let written = text(&[RA, HALANT, JOINER, KA]);
        assert_eq!(order(&written), written);
        assert!(!split(&written)[0].reph);
    }

    #[test]
    fn a_ra_at_the_end_of_a_cluster_is_not_what_the_syllable_hangs_on() {
        // क्र: the r is drawn as a tail under the k, and the k is the base.
        let found = split(&text(&[KA, HALANT, RA]));
        assert_eq!(found[0].base, Some(0));
        assert!(!found[0].reph);
    }

    #[test]
    fn a_nukta_belongs_to_the_consonant_before_it() {
        let found = split(&text(&[KA, NUKTA, HALANT, SA]));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].base, Some(3));
    }

    #[test]
    fn a_word_of_several_syllables_is_split_where_the_vowels_are() {
        // सकल — three syllables, each a consonant with its own vowel.
        let written = text(&[SA, KA, '\u{0932}']);
        assert_eq!(split(&written).len(), 3);
        assert_eq!(order(&written), written, "nothing here is drawn out of order");
    }

    #[test]
    fn every_character_comes_out_exactly_once() {
        // Whatever the rules do, nothing may be dropped or drawn twice: what
        // is drawn is what was written.
        for written in [
            text(&[RA, HALANT, KA, I, ANUSVARA]),
            text(&[KA, HALANT, KHA, HALANT, GA, AA]),
            text(&[A, KA, NUKTA, U]),
            text(&[RA, HALANT, RA, HALANT, KA]),
            // Bengali, Tamil, Kannada and Sinhala, with their split vowels.
            "\u{0995}\u{09CB}\u{09B0}\u{09CD}\u{0995}".to_owned(),
            "\u{0B95}\u{0BCA}".to_owned(),
            "\u{0C95}\u{0CCD}\u{0C95}\u{0CCB}".to_owned(),
            "\u{0D9A}\u{0DDD}".to_owned(),
        ] {
            let drawn = order(&written);
            let mut one: Vec<char> = pieces_of(&written.chars().collect::<Vec<_>>())
                .iter()
                .map(|piece| piece.character)
                .collect();
            let mut other: Vec<char> = drawn.chars().collect();
            one.sort_unstable();
            other.sort_unstable();
            assert_eq!(one, other, "{written:?} came out as {drawn:?}");
        }
    }

    // --- the other scripts --------------------------------------------------

    #[test]
    fn every_block_has_its_consonants_vowels_signs_and_halant_told_apart() {
        let halants =
            [0x094D, 0x09CD, 0x0A4D, 0x0ACD, 0x0B4D, 0x0BCD, 0x0C4D, 0x0CCD, 0x0D4D, 0x0DCA];
        let ras = [0x0930, 0x09B0, 0x0A30, 0x0AB0, 0x0B30, 0x0BB0, 0x0C30, 0x0CB0, 0x0D30, 0x0DBB];
        let firsts =
            [0x0915, 0x0995, 0x0A15, 0x0A95, 0x0B15, 0x0B95, 0x0C15, 0x0C95, 0x0D15, 0x0D9A];
        for (index, script) in Script::ALL.iter().enumerate() {
            let halant = char::from_u32(halants[index]).unwrap();
            assert_eq!(category_of(halant), Category::Halant, "{script:?}");
            assert_eq!(Script::of(halant), Some(*script));
            assert_eq!(
                category_of(char::from_u32(ras[index]).unwrap()),
                Category::Ra,
                "{script:?}"
            );
            assert_eq!(
                category_of(char::from_u32(firsts[index]).unwrap()),
                Category::Consonant,
                "{script:?}"
            );
            // Every script has a vowel sign written to the left of the
            // consonant except Telugu and Kannada, whose vowel signs all sit
            // above or after.
            let block = match script {
                Script::Devanagari => 0x0900,
                Script::Bengali => 0x0980,
                Script::Gurmukhi => 0x0A00,
                Script::Gujarati => 0x0A80,
                Script::Oriya => 0x0B00,
                Script::Tamil => 0x0B80,
                Script::Telugu => 0x0C00,
                Script::Kannada => 0x0C80,
                Script::Malayalam => 0x0D00,
                Script::Sinhala => 0x0D80,
            };
            let before = (block..block + 0x80)
                .filter_map(char::from_u32)
                .any(|c| category_of(c) == Category::Matra(Position::Before));
            assert_eq!(
                before,
                !matches!(script, Script::Telugu | Script::Kannada),
                "{script:?} and its signs written to the left"
            );
            let vowels = (block..block + 0x80)
                .filter_map(char::from_u32)
                .filter(|c| category_of(*c) == Category::Vowel)
                .count();
            assert!(vowels >= 10, "{script:?} has {vowels} independent vowels");
        }
    }

    #[test]
    fn a_vowel_sign_written_in_two_parts_is_split_and_the_first_part_goes_first() {
        // Bengali কো: the o is its e before the consonant and its aa after.
        let bengali = "\u{0995}\u{09CB}";
        let pieces = pieces_of(&bengali.chars().collect::<Vec<_>>());
        assert_eq!(pieces.len(), 3);
        assert_eq!(pieces[1].category, Category::Matra(Position::Before));
        assert_eq!(pieces[2].category, Category::Matra(Position::After));
        assert_eq!(pieces[1].origin, 1, "both parts come from the one character");
        assert_eq!(pieces[2].origin, 1);
        assert_eq!(order(bengali), "\u{09C7}\u{0995}\u{09BE}");
        // Tamil கொ the same way, and Malayalam കോ.
        assert_eq!(order("\u{0B95}\u{0BCA}"), "\u{0BC6}\u{0B95}\u{0BBE}");
        assert_eq!(order("\u{0D15}\u{0D4B}"), "\u{0D47}\u{0D15}\u{0D3E}");
        // Sinhala කේ: the second part is the al-lakuna, which is a sign here
        // and not a halant, so the syllable does not end at it.
        let sinhala = "\u{0D9A}\u{0DDA}\u{0D9A}";
        let found = split(sinhala);
        assert_eq!(found.len(), 2);
        assert_eq!(order(sinhala), "\u{0DD9}\u{0D9A}\u{0DCA}\u{0D9A}");
    }

    #[test]
    fn telugu_and_kannada_hang_on_the_first_consonant_and_stack_the_rest_beneath() {
        // Kannada ಕ್ಕಿ: the first ka is the base, the second is written small
        // beneath it, and the i sits above the base — after it in the sequence.
        let kannada = "\u{0C95}\u{0CCD}\u{0C95}\u{0CBF}";
        let found = split(kannada);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].base, Some(0));
        assert_eq!(order(kannada), kannada);
        // Telugu కు likewise, and a Telugu Ra at the front is a letter, not
        // a hook, unless a joiner asks.
        let telugu = "\u{0C30}\u{0C4D}\u{0C15}";
        assert!(!split(telugu)[0].reph);
        assert_eq!(split(telugu)[0].base, Some(0));
        let asked = "\u{0C30}\u{0C4D}\u{200D}\u{0C15}";
        assert!(split(asked)[0].reph);
        assert_eq!(order(asked), "\u{0C15}\u{0C30}\u{0C4D}\u{200D}");
    }

    #[test]
    fn the_hook_is_drawn_where_each_script_draws_it() {
        // Bengali র্কা: after the below signs, before the aa.
        assert_eq!(order("\u{09B0}\u{09CD}\u{0995}\u{09BE}"), "\u{0995}\u{09B0}\u{09CD}\u{09BE}");
        // Oriya ର୍କା: right after the base, before anything else.
        assert_eq!(order("\u{0B30}\u{0B4D}\u{0B15}\u{0B3E}"), "\u{0B15}\u{0B30}\u{0B4D}\u{0B3E}");
        // Kannada ರ್ಕಾ: after the aa, before the dots.
        assert_eq!(
            order("\u{0CB0}\u{0CCD}\u{0C95}\u{0CBE}\u{0C82}"),
            "\u{0C95}\u{0CBE}\u{0CB0}\u{0CCD}\u{0C82}"
        );
        // Malayalam's hook is a character of its own, written first and
        // drawn after the letter: ൎക.
        assert_eq!(order("\u{0D4E}\u{0D15}\u{0D3E}"), "\u{0D15}\u{0D4E}\u{0D3E}");
        let found = split("\u{0D4E}\u{0D15}\u{0D3E}");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].base, Some(1));
    }

    #[test]
    fn a_consonant_written_after_the_base_is_passed_over_when_the_base_is_sought() {
        // Bengali ক্য: the ya-phala is a sign after the ka, so ka is the base.
        assert_eq!(split("\u{0995}\u{09CD}\u{09AF}")[0].base, Some(0));
        // Malayalam ക്യ likewise; and ക്ക is a conjunct hung on its last.
        assert_eq!(split("\u{0D15}\u{0D4D}\u{0D2F}")[0].base, Some(0));
        assert_eq!(split("\u{0D15}\u{0D4D}\u{0D15}")[0].base, Some(2));
        // Gurmukhi ਕ੍ਵ: the va beneath.
        assert_eq!(split("\u{0A15}\u{0A4D}\u{0A35}")[0].base, Some(0));
    }

    #[test]
    fn in_sinhala_a_halant_ends_the_letter_unless_a_joiner_joins_the_next() {
        let apart = "\u{0D9A}\u{0DCA}\u{0DC2}";
        assert_eq!(split(apart).len(), 2, "two letters, the first with its al-lakuna");
        let joined = "\u{0D9A}\u{0DCA}\u{200D}\u{0DC2}";
        assert_eq!(split(joined).len(), 1);
        assert_eq!(split(joined)[0].base, Some(0));
        // And the e written before goes before the whole of a joined pair.
        assert_eq!(
            order("\u{0D9A}\u{0DCA}\u{200D}\u{0DC2}\u{0DD9}"),
            "\u{0DD9}\u{0D9A}\u{0DCA}\u{200D}\u{0DC2}"
        );
    }

    #[test]
    fn a_pre_base_form_the_font_made_is_drawn_before_the_base() {
        // Malayalam ക്ര: the font says it draws the ra before the ka, and so
        // the ra and its halant go before the ka, after any sign written to
        // the left.
        let characters: Vec<char> = "\u{0D15}\u{0D4D}\u{0D30}\u{0D46}".chars().collect();
        let pieces = pieces_of(&characters);
        let found = syllables(Script::Malayalam, &pieces);
        let drawn: String = drawn_order(Script::Malayalam, &pieces, &found[0], &[2])
            .into_iter()
            .map(|at| pieces[at].character)
            .collect();
        assert_eq!(drawn, "\u{0D46}\u{0D4D}\u{0D30}\u{0D15}");
    }

    #[test]
    fn every_script_answers_to_both_its_tags() {
        for script in Script::ALL {
            for tag in script.tags() {
                assert_eq!(Script::of_tag(&tag), Some(script));
            }
        }
        assert_eq!(Script::of_tag(b"latn"), None);
    }
}
