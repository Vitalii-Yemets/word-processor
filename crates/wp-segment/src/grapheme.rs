//! Where one character the reader sees ends and the next begins.
//!
//! A `char` in Rust is a Unicode code point, which is not what a person means
//! by a character. `é` may be one code point or two — `e` and an accent that
//! draws on top of it. A flag is two. A family emoji is seven, joined by an
//! invisible character whose whole job is to say "these draw as one".
//!
//! The caret must step over the thing a person sees, or Left arrow leaves it
//! between a letter and its accent and Backspace takes the skin tone off a
//! hand. What that thing is has a name — a *grapheme cluster* — and rules, in
//! [UAX #29], and the rules are here.
//!
//! [UAX #29]: https://www.unicode.org/reports/tr29/

/// What a character does to the boundary beside it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
    /// Anything that stands on its own: a letter, a digit, a full stop.
    Other,
    CarriageReturn,
    LineFeed,
    /// A character that draws nothing and joins nothing.
    Control,
    /// A mark that draws on the character before it: an accent, a vowel sign, a
    /// skin tone, a variation selector.
    Extend,
    /// The zero width joiner, which welds two pictures into one.
    Joiner,
    /// Half of a flag: flags are written as two letters from a private
    /// alphabet, and it is the pair that draws as a flag.
    Regional,
    /// A character that attaches to whatever follows it.
    Prepend,
    /// A vowel sign that takes room of its own but still belongs to its
    /// consonant, as the Indic scripts are full of.
    SpacingMark,
    /// Hangul: a leading consonant, a vowel, a trailing consonant, and the
    /// composed syllables of the two shapes.
    Leading,
    Vowel,
    Trailing,
    LeadingVowel,
    LeadingVowelTrailing,
    /// An emoji, which may be joined to another one.
    Pictographic,
}

/// Every place in the text where one cluster ends and the next begins.
///
/// The ends count: the first offset is always 0 and the last is always the
/// length, so the boundaries chop the text into clusters with nothing left
/// over.
#[must_use]
pub fn boundaries(text: &str) -> Vec<usize> {
    let mut out = vec![0];
    if text.is_empty() {
        return out;
    }

    let mut previous: Option<Class> = None;
    // How many regional indicators run up to here without a break, which is
    // what says whether the next one starts a flag or finishes one.
    let mut flag_run = 0usize;
    // Whether what has been read so far is an emoji followed by any number of
    // marks — the shape a joiner may hang another emoji from.
    let mut picture_chain = false;
    // Whether the character just read was that joiner.
    let mut joined_picture = false;

    for (offset, character) in text.char_indices() {
        let class = class_of(character);
        if let Some(before) = previous {
            if breaks(before, class, flag_run, joined_picture) {
                out.push(offset);
                flag_run = 0;
            }
        }

        flag_run = if class == Class::Regional { flag_run + 1 } else { 0 };
        joined_picture = class == Class::Joiner && picture_chain;
        picture_chain = match class {
            Class::Pictographic => true,
            Class::Extend => picture_chain,
            _ => false,
        };
        previous = Some(class);
    }

    out.push(text.len());
    out
}

/// Where the cluster after an offset begins.
#[must_use]
pub fn next(text: &str, offset: usize) -> usize {
    boundaries(text).into_iter().find(|at| *at > offset).unwrap_or(text.len())
}

/// Where the cluster before an offset begins.
#[must_use]
pub fn previous(text: &str, offset: usize) -> usize {
    boundaries(text).into_iter().rev().find(|at| *at < offset).unwrap_or(0)
}

/// Whether a character hangs from the one before it rather than standing on its
/// own: an accent, a vowel sign, a variation selector, a skin tone.
pub(crate) fn is_mark(character: char) -> bool {
    matches!(class_of(character), Class::Extend | Class::SpacingMark)
}

/// Whether a character is a picture — an emoji — which matters because two of
/// them joined together are one.
pub(crate) fn is_pictographic(character: char) -> bool {
    class_of(character) == Class::Pictographic
}

/// Whether a cluster ends between two characters.
///
/// The rules are the standard's, in its order, and the first that speaks
/// decides; the last one says that anything not spoken for is a boundary,
/// which is why unknown characters stand alone rather than sticking together.
fn breaks(before: Class, after: Class, flag_run: usize, joined_picture: bool) -> bool {
    use Class::{
        CarriageReturn, Control, Extend, Joiner, Leading, LeadingVowel, LeadingVowelTrailing,
        LineFeed, Pictographic, Prepend, Regional, SpacingMark, Trailing, Vowel,
    };

    match (before, after) {
        // GB3 to GB5: the line ending is one cluster, and nothing joins a
        // control character.
        (CarriageReturn, LineFeed) => false,
        (CarriageReturn | LineFeed | Control, _) | (_, CarriageReturn | LineFeed | Control) => true,
        // GB6 to GB8: a Hangul syllable spelled out in its parts is one
        // cluster, in the order the parts may appear.
        (Leading, Leading | Vowel | LeadingVowel | LeadingVowelTrailing) => false,
        (LeadingVowel | Vowel, Vowel | Trailing) => false,
        (LeadingVowelTrailing | Trailing, Trailing) => false,
        // GB9, GB9a and GB9b: a mark goes with what it draws on, and a prepend
        // with what follows it.
        (_, Extend | Joiner | SpacingMark) | (Prepend, _) => false,
        // GB11: emoji, marks, a joiner, and another emoji: one picture.
        (Joiner, Pictographic) => !joined_picture,
        // GB12 and GB13: two regional indicators make a flag, and a third
        // starts a new one rather than joining the first.
        (Regional, Regional) => flag_run % 2 == 0,
        // GB999.
        _ => true,
    }
}

/// The class of one character.
fn class_of(character: char) -> Class {
    let code = character as u32;
    match code {
        0x000D => return Class::CarriageReturn,
        0x000A => return Class::LineFeed,
        0x200D => return Class::Joiner,
        0x1F1E6..=0x1F1FF => return Class::Regional,
        // Hangul, composed: every twenty-eighth syllable has no trailing
        // consonant, and the rest do.
        0xAC00..=0xD7A3 => {
            return if (code - 0xAC00) % 28 == 0 {
                Class::LeadingVowel
            } else {
                Class::LeadingVowelTrailing
            }
        }
        _ => {}
    }

    for (first, last, class) in RANGES {
        if code >= *first && code <= *last {
            return *class;
        }
    }

    Class::Other
}

/// The characters that are not simply themselves.
///
/// Order matters: the first range that contains a character decides, so the
/// exceptions are written before the blocks they are carved out of.
const RANGES: &[(u32, u32, Class)] = &[
    // Control characters: everything that draws nothing and joins nothing.
    (0x0000, 0x0009, Class::Control),
    (0x000B, 0x000C, Class::Control),
    (0x000E, 0x001F, Class::Control),
    (0x007F, 0x009F, Class::Control),
    (0x00AD, 0x00AD, Class::Control),
    (0x061C, 0x061C, Class::Control),
    (0x180E, 0x180E, Class::Control),
    (0x200B, 0x200B, Class::Control),
    (0x200E, 0x200F, Class::Control),
    (0x2028, 0x202E, Class::Control),
    (0x2060, 0x206F, Class::Control),
    (0xFEFF, 0xFEFF, Class::Control),
    (0xFFF0, 0xFFFB, Class::Control),
    // The characters that attach to what follows them, which is a short list.
    (0x0600, 0x0605, Class::Prepend),
    (0x06DD, 0x06DD, Class::Prepend),
    (0x070F, 0x070F, Class::Prepend),
    (0x0890, 0x0891, Class::Prepend),
    (0x08E2, 0x08E2, Class::Prepend),
    (0x0D4E, 0x0D4E, Class::Prepend),
    (0x110BD, 0x110BD, Class::Prepend),
    (0x110CD, 0x110CD, Class::Prepend),
    // Marks that draw on the character before them.
    (0x0300, 0x036F, Class::Extend),
    (0x0483, 0x0489, Class::Extend),
    (0x0591, 0x05BD, Class::Extend),
    (0x05BF, 0x05BF, Class::Extend),
    (0x05C1, 0x05C2, Class::Extend),
    (0x05C4, 0x05C5, Class::Extend),
    (0x05C7, 0x05C7, Class::Extend),
    (0x0610, 0x061A, Class::Extend),
    (0x064B, 0x065F, Class::Extend),
    (0x0670, 0x0670, Class::Extend),
    (0x06D6, 0x06DC, Class::Extend),
    (0x06DF, 0x06E4, Class::Extend),
    (0x06E7, 0x06E8, Class::Extend),
    (0x06EA, 0x06ED, Class::Extend),
    (0x0711, 0x0711, Class::Extend),
    (0x0730, 0x074A, Class::Extend),
    (0x07A6, 0x07B0, Class::Extend),
    (0x07EB, 0x07F3, Class::Extend),
    (0x0816, 0x0819, Class::Extend),
    (0x081B, 0x0823, Class::Extend),
    (0x0825, 0x0827, Class::Extend),
    (0x0829, 0x082D, Class::Extend),
    (0x0898, 0x089F, Class::Extend),
    (0x08CA, 0x08E1, Class::Extend),
    (0x08E3, 0x0902, Class::Extend),
    (0x0903, 0x0903, Class::SpacingMark),
    (0x093A, 0x093A, Class::Extend),
    (0x093B, 0x093B, Class::SpacingMark),
    (0x093C, 0x093C, Class::Extend),
    (0x093E, 0x0940, Class::SpacingMark),
    (0x0941, 0x0948, Class::Extend),
    (0x0949, 0x094C, Class::SpacingMark),
    (0x094D, 0x094D, Class::Extend),
    (0x094E, 0x094F, Class::SpacingMark),
    (0x0951, 0x0957, Class::Extend),
    (0x0962, 0x0963, Class::Extend),
    (0x0981, 0x0981, Class::Extend),
    (0x0982, 0x0983, Class::SpacingMark),
    (0x09BC, 0x09BC, Class::Extend),
    (0x09BE, 0x09C0, Class::SpacingMark),
    (0x09C1, 0x09C4, Class::Extend),
    (0x09C7, 0x09CC, Class::SpacingMark),
    (0x09CD, 0x09CD, Class::Extend),
    (0x09D7, 0x09D7, Class::SpacingMark),
    (0x0A01, 0x0A02, Class::Extend),
    (0x0A03, 0x0A03, Class::SpacingMark),
    (0x0A3C, 0x0A51, Class::Extend),
    (0x0A70, 0x0A71, Class::Extend),
    (0x0A75, 0x0A82, Class::Extend),
    (0x0A83, 0x0A83, Class::SpacingMark),
    (0x0ABC, 0x0ABC, Class::Extend),
    (0x0ABE, 0x0AC0, Class::SpacingMark),
    (0x0AC1, 0x0AC8, Class::Extend),
    (0x0AC9, 0x0ACC, Class::SpacingMark),
    (0x0ACD, 0x0ACD, Class::Extend),
    (0x0AE2, 0x0AE3, Class::Extend),
    (0x0AFA, 0x0B02, Class::Extend),
    (0x0B03, 0x0B03, Class::SpacingMark),
    (0x0B3C, 0x0B3C, Class::Extend),
    (0x0B3E, 0x0B40, Class::SpacingMark),
    (0x0B41, 0x0B44, Class::Extend),
    (0x0B47, 0x0B4C, Class::SpacingMark),
    (0x0B4D, 0x0B56, Class::Extend),
    (0x0B57, 0x0B57, Class::SpacingMark),
    (0x0B62, 0x0B63, Class::Extend),
    (0x0B82, 0x0B82, Class::Extend),
    (0x0BBE, 0x0BBF, Class::SpacingMark),
    (0x0BC0, 0x0BC0, Class::Extend),
    (0x0BC1, 0x0BCC, Class::SpacingMark),
    (0x0BCD, 0x0BCD, Class::Extend),
    (0x0BD7, 0x0BD7, Class::SpacingMark),
    (0x0C00, 0x0C00, Class::Extend),
    (0x0C01, 0x0C03, Class::SpacingMark),
    (0x0C3C, 0x0C3C, Class::Extend),
    (0x0C3E, 0x0C40, Class::Extend),
    (0x0C41, 0x0C44, Class::SpacingMark),
    (0x0C46, 0x0C56, Class::Extend),
    (0x0C62, 0x0C63, Class::Extend),
    (0x0C81, 0x0C81, Class::Extend),
    (0x0C82, 0x0C83, Class::SpacingMark),
    (0x0CBC, 0x0CBC, Class::Extend),
    (0x0CBE, 0x0CBE, Class::SpacingMark),
    (0x0CBF, 0x0CBF, Class::Extend),
    (0x0CC0, 0x0CC4, Class::SpacingMark),
    (0x0CC6, 0x0CC6, Class::Extend),
    (0x0CC7, 0x0CCB, Class::SpacingMark),
    (0x0CCC, 0x0CCD, Class::Extend),
    (0x0CD5, 0x0CD6, Class::SpacingMark),
    (0x0CE2, 0x0CE3, Class::Extend),
    (0x0D00, 0x0D01, Class::Extend),
    (0x0D02, 0x0D03, Class::SpacingMark),
    (0x0D3B, 0x0D3C, Class::Extend),
    (0x0D3E, 0x0D40, Class::SpacingMark),
    (0x0D41, 0x0D44, Class::Extend),
    (0x0D46, 0x0D4C, Class::SpacingMark),
    (0x0D4D, 0x0D4D, Class::Extend),
    (0x0D57, 0x0D57, Class::SpacingMark),
    (0x0D62, 0x0D63, Class::Extend),
    (0x0D81, 0x0D81, Class::Extend),
    (0x0D82, 0x0D83, Class::SpacingMark),
    (0x0DCA, 0x0DCA, Class::Extend),
    (0x0DCF, 0x0DD1, Class::SpacingMark),
    (0x0DD2, 0x0DD6, Class::Extend),
    (0x0DD8, 0x0DDF, Class::SpacingMark),
    (0x0DF2, 0x0DF3, Class::SpacingMark),
    (0x0E31, 0x0E31, Class::Extend),
    (0x0E33, 0x0E33, Class::SpacingMark),
    (0x0E34, 0x0E3A, Class::Extend),
    (0x0E47, 0x0E4E, Class::Extend),
    (0x0EB1, 0x0EB1, Class::Extend),
    (0x0EB3, 0x0EB3, Class::SpacingMark),
    (0x0EB4, 0x0EBC, Class::Extend),
    (0x0EC8, 0x0ECE, Class::Extend),
    (0x0F18, 0x0F19, Class::Extend),
    (0x0F35, 0x0F35, Class::Extend),
    (0x0F37, 0x0F37, Class::Extend),
    (0x0F39, 0x0F39, Class::Extend),
    (0x0F71, 0x0F7E, Class::Extend),
    (0x0F80, 0x0F84, Class::Extend),
    (0x0F86, 0x0F87, Class::Extend),
    (0x0F8D, 0x0FBC, Class::Extend),
    (0x0FC6, 0x0FC6, Class::Extend),
    (0x102D, 0x1030, Class::Extend),
    (0x1032, 0x1037, Class::Extend),
    (0x1039, 0x103A, Class::Extend),
    (0x103D, 0x103E, Class::Extend),
    (0x1058, 0x1059, Class::Extend),
    (0x105E, 0x1060, Class::Extend),
    (0x1071, 0x1074, Class::Extend),
    (0x1082, 0x1082, Class::Extend),
    (0x1085, 0x1086, Class::Extend),
    (0x108D, 0x108D, Class::Extend),
    (0x109D, 0x109D, Class::Extend),
    (0x135D, 0x135F, Class::Extend),
    (0x1712, 0x1714, Class::Extend),
    (0x1732, 0x1733, Class::Extend),
    (0x1752, 0x1753, Class::Extend),
    (0x1772, 0x1773, Class::Extend),
    (0x17B4, 0x17B5, Class::Extend),
    (0x17B7, 0x17BD, Class::Extend),
    (0x17C6, 0x17C6, Class::Extend),
    (0x17C9, 0x17D3, Class::Extend),
    (0x17DD, 0x17DD, Class::Extend),
    (0x180B, 0x180D, Class::Extend),
    (0x1885, 0x1886, Class::Extend),
    (0x18A9, 0x18A9, Class::Extend),
    (0x1920, 0x1922, Class::Extend),
    (0x1927, 0x1928, Class::Extend),
    (0x1932, 0x1932, Class::Extend),
    (0x1939, 0x193B, Class::Extend),
    (0x1A17, 0x1A18, Class::Extend),
    (0x1A1B, 0x1A1B, Class::Extend),
    (0x1A56, 0x1A56, Class::Extend),
    (0x1A58, 0x1A60, Class::Extend),
    (0x1A62, 0x1A62, Class::Extend),
    (0x1A65, 0x1A6C, Class::Extend),
    (0x1A73, 0x1A7F, Class::Extend),
    (0x1AB0, 0x1AFF, Class::Extend),
    (0x1B00, 0x1B03, Class::Extend),
    (0x1B34, 0x1B3A, Class::Extend),
    (0x1B3C, 0x1B3C, Class::Extend),
    (0x1B42, 0x1B44, Class::Extend),
    (0x1B6B, 0x1B73, Class::Extend),
    (0x1B80, 0x1B81, Class::Extend),
    (0x1BA2, 0x1BA5, Class::Extend),
    (0x1BA8, 0x1BA9, Class::Extend),
    (0x1BAB, 0x1BAD, Class::Extend),
    (0x1BE6, 0x1BE6, Class::Extend),
    (0x1BE8, 0x1BE9, Class::Extend),
    (0x1BED, 0x1BED, Class::Extend),
    (0x1BEF, 0x1BF1, Class::Extend),
    (0x1C2C, 0x1C33, Class::Extend),
    (0x1C36, 0x1C37, Class::Extend),
    (0x1CD0, 0x1CD2, Class::Extend),
    (0x1CD4, 0x1CE0, Class::Extend),
    (0x1CE2, 0x1CE8, Class::Extend),
    (0x1CED, 0x1CED, Class::Extend),
    (0x1CF4, 0x1CF4, Class::Extend),
    (0x1CF8, 0x1CF9, Class::Extend),
    (0x1DC0, 0x1DFF, Class::Extend),
    (0x200C, 0x200C, Class::Extend),
    (0x20D0, 0x20F0, Class::Extend),
    (0x2CEF, 0x2CF1, Class::Extend),
    (0x2D7F, 0x2D7F, Class::Extend),
    (0x2DE0, 0x2DFF, Class::Extend),
    (0x302A, 0x302F, Class::Extend),
    (0x3099, 0x309A, Class::Extend),
    (0xA66F, 0xA672, Class::Extend),
    (0xA674, 0xA67D, Class::Extend),
    (0xA69E, 0xA69F, Class::Extend),
    (0xA6F0, 0xA6F1, Class::Extend),
    (0xA802, 0xA802, Class::Extend),
    (0xA806, 0xA806, Class::Extend),
    (0xA80B, 0xA80B, Class::Extend),
    (0xA825, 0xA826, Class::Extend),
    (0xA82C, 0xA82C, Class::Extend),
    (0xA8C4, 0xA8C5, Class::Extend),
    (0xA8E0, 0xA8F1, Class::Extend),
    (0xA8FF, 0xA8FF, Class::Extend),
    (0xA926, 0xA92D, Class::Extend),
    (0xA947, 0xA951, Class::Extend),
    (0xA980, 0xA982, Class::Extend),
    (0xA9B3, 0xA9B3, Class::Extend),
    (0xA9B6, 0xA9B9, Class::Extend),
    (0xA9BC, 0xA9BD, Class::Extend),
    (0xA9E5, 0xA9E5, Class::Extend),
    (0xAA29, 0xAA2E, Class::Extend),
    (0xAA31, 0xAA32, Class::Extend),
    (0xAA35, 0xAA36, Class::Extend),
    (0xAA43, 0xAA43, Class::Extend),
    (0xAA4C, 0xAA4C, Class::Extend),
    (0xAA7C, 0xAA7C, Class::Extend),
    (0xAAB0, 0xAAB0, Class::Extend),
    (0xAAB2, 0xAAB4, Class::Extend),
    (0xAAB7, 0xAAB8, Class::Extend),
    (0xAABE, 0xAABF, Class::Extend),
    (0xAAC1, 0xAAC1, Class::Extend),
    (0xAAEC, 0xAAED, Class::Extend),
    (0xAAF6, 0xAAF6, Class::Extend),
    (0xABE5, 0xABE5, Class::Extend),
    (0xABE8, 0xABE8, Class::Extend),
    (0xABED, 0xABED, Class::Extend),
    (0xFB1E, 0xFB1E, Class::Extend),
    (0xFE00, 0xFE0F, Class::Extend),
    (0xFE20, 0xFE2F, Class::Extend),
    (0x101FD, 0x101FD, Class::Extend),
    (0x102E0, 0x102E0, Class::Extend),
    (0x10376, 0x1037A, Class::Extend),
    (0x10A01, 0x10A0F, Class::Extend),
    (0x10A38, 0x10A3F, Class::Extend),
    (0x10AE5, 0x10AE6, Class::Extend),
    (0x11001, 0x11001, Class::Extend),
    (0x11038, 0x11046, Class::Extend),
    (0x1107F, 0x11081, Class::Extend),
    (0x110B3, 0x110B6, Class::Extend),
    (0x110B9, 0x110BA, Class::Extend),
    (0x11100, 0x11102, Class::Extend),
    (0x11127, 0x1112B, Class::Extend),
    (0x1112D, 0x11134, Class::Extend),
    (0x11180, 0x11181, Class::Extend),
    (0x111B6, 0x111BE, Class::Extend),
    (0x1122F, 0x11231, Class::Extend),
    (0x11234, 0x11234, Class::Extend),
    (0x11236, 0x11237, Class::Extend),
    (0x112DF, 0x112DF, Class::Extend),
    (0x112E3, 0x112EA, Class::Extend),
    (0x11300, 0x11301, Class::Extend),
    (0x1133B, 0x1133C, Class::Extend),
    (0x11340, 0x11340, Class::Extend),
    (0x11366, 0x11374, Class::Extend),
    (0x1D165, 0x1D169, Class::Extend),
    (0x1D16D, 0x1D172, Class::Extend),
    (0x1D17B, 0x1D182, Class::Extend),
    (0x1D185, 0x1D18B, Class::Extend),
    (0x1D1AA, 0x1D1AD, Class::Extend),
    (0x1D242, 0x1D244, Class::Extend),
    (0x1DA00, 0x1DA36, Class::Extend),
    (0x1DA3B, 0x1DA6C, Class::Extend),
    (0x1E8D0, 0x1E8D6, Class::Extend),
    (0x1F3FB, 0x1F3FF, Class::Extend),
    (0xE0020, 0xE007F, Class::Extend),
    (0xE0100, 0xE01EF, Class::Extend),
    // Hangul, spelled out in its parts.
    (0x1100, 0x115F, Class::Leading),
    (0x1160, 0x11A7, Class::Vowel),
    (0x11A8, 0x11FF, Class::Trailing),
    (0xA960, 0xA97C, Class::Leading),
    (0xD7B0, 0xD7C6, Class::Vowel),
    (0xD7CB, 0xD7FB, Class::Trailing),
    // The pictures, which may be joined to one another.
    (0x00A9, 0x00A9, Class::Pictographic),
    (0x00AE, 0x00AE, Class::Pictographic),
    (0x203C, 0x203C, Class::Pictographic),
    (0x2049, 0x2049, Class::Pictographic),
    (0x2122, 0x2122, Class::Pictographic),
    (0x2139, 0x2139, Class::Pictographic),
    (0x2194, 0x21AA, Class::Pictographic),
    (0x231A, 0x231B, Class::Pictographic),
    (0x2328, 0x2328, Class::Pictographic),
    (0x23CF, 0x23FA, Class::Pictographic),
    (0x24C2, 0x24C2, Class::Pictographic),
    (0x25AA, 0x25FE, Class::Pictographic),
    (0x2600, 0x27BF, Class::Pictographic),
    (0x2934, 0x2935, Class::Pictographic),
    (0x2B00, 0x2BFF, Class::Pictographic),
    (0x3030, 0x3030, Class::Pictographic),
    (0x303D, 0x303D, Class::Pictographic),
    (0x3297, 0x3297, Class::Pictographic),
    (0x3299, 0x3299, Class::Pictographic),
    (0x1F000, 0x1FAFF, Class::Pictographic),
    (0x1FC00, 0x1FFFD, Class::Pictographic),
];

#[cfg(test)]
mod tests {
    use super::{boundaries, next, previous};

    /// The text chopped into the clusters a reader would count.
    fn clusters(text: &str) -> Vec<&str> {
        boundaries(text).windows(2).map(|pair| &text[pair[0]..pair[1]]).collect()
    }

    #[test]
    fn plain_text_is_one_cluster_per_letter() {
        assert_eq!(clusters("abc"), ["a", "b", "c"]);
    }

    #[test]
    fn a_letter_and_its_accent_are_one_character() {
        // "e" and a combining acute: two code points, one character.
        assert_eq!(clusters("e\u{0301}f"), ["e\u{0301}", "f"]);
    }

    #[test]
    fn a_flag_is_one_character_and_two_flags_are_two() {
        let flags = "\u{1F1FA}\u{1F1E6}\u{1F1EC}\u{1F1E7}";
        assert_eq!(clusters(flags), ["\u{1F1FA}\u{1F1E6}", "\u{1F1EC}\u{1F1E7}"]);
    }

    #[test]
    fn an_odd_regional_indicator_stands_alone() {
        let text = "\u{1F1FA}\u{1F1E6}\u{1F1EC}";
        assert_eq!(clusters(text), ["\u{1F1FA}\u{1F1E6}", "\u{1F1EC}"]);
    }

    #[test]
    fn a_family_joined_by_zero_width_joiners_is_one_character() {
        let family = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}";
        assert_eq!(clusters(family).len(), 1, "the family came apart");
    }

    #[test]
    fn a_skin_tone_belongs_to_the_hand_it_colours() {
        let wave = "\u{1F44B}\u{1F3FD}";
        assert_eq!(clusters(wave).len(), 1);
    }

    #[test]
    fn a_line_ending_of_two_characters_is_one() {
        assert_eq!(clusters("a\r\nb"), ["a", "\r\n", "b"]);
    }

    #[test]
    fn a_hangul_syllable_spelled_in_parts_is_one_character() {
        // Leading consonant, vowel, trailing consonant.
        assert_eq!(clusters("\u{1112}\u{1161}\u{11AB}").len(), 1);
    }

    #[test]
    fn the_caret_steps_over_a_whole_character() {
        let text = "e\u{0301}f";
        assert_eq!(next(text, 0), 3, "the caret stopped between the letter and its accent");
        assert_eq!(previous(text, 3), 0);
        assert_eq!(next(text, 3), 4);
        assert_eq!(previous(text, 4), 3);
    }

    #[test]
    fn the_ends_are_boundaries_and_nothing_runs_past_them() {
        assert_eq!(boundaries(""), vec![0]);
        assert_eq!(next("abc", 3), 3);
        assert_eq!(previous("abc", 0), 0);
    }
}
