//! The two ways of writing an accented letter, made one.
//!
//! # Why this exists
//!
//! `é` is either one character or two. A French keyboard on Windows produces
//! U+00E9, one character that means "e with an acute accent". A Mac, and a good
//! deal of text on the web, produces `e` followed by U+0301, a mark that draws
//! an acute accent on whatever precedes it. The two are the *same text*: they
//! are drawn identically and Unicode says they are equivalent. They are not the
//! same bytes.
//!
//! So a search for `café` misses the café that was written the other way, a
//! word appears twice in an index, and a document that was written on two
//! machines sorts wrongly. Normalization is the fix: put both into the same
//! form before comparing them.
//!
//! [UAX #15] defines four forms; the two here are the canonical ones — NFD,
//! everything pulled apart into a letter and its marks, and NFC, everything put
//! back together. NFC is what Windows produces and what a `.docx` written by
//! Word holds, so it is what this program writes; NFD is the form that makes
//! comparing easy, because there is only one way to write anything in it.
//!
//! [UAX #15]: https://www.unicode.org/reports/tr15/
//!
//! # What is covered
//!
//! Every character Unicode gives a canonical decomposition, generated from the
//! character database — the Latin alphabets of Europe, Greek and Cyrillic,
//! Vietnamese with its two marks on one letter, the Indic scripts, the Hebrew
//! and Arabic points, and the Hangul syllables, which are not a table at all
//! but arithmetic.
//!
//! Not the compatibility decompositions. A superscript two is not a two and a
//! ligature is not the letters it is drawn from: pulling those apart changes
//! what the text says, and nothing here asks for it.
//!
//! # Example
//!
//! ```
//! // The same word, written the two ways, is one word after this.
//! let composed = "café";
//! let decomposed = "cafe\u{0301}";
//! assert_ne!(composed, decomposed);
//! assert_eq!(wp_normal::compose(decomposed), composed);
//! assert_eq!(wp_normal::decompose(composed), decomposed);
//! ```

#![forbid(unsafe_code)]

mod table;

/// The text with every accented letter pulled apart into a letter and its
/// marks, and the marks put in the order the standard fixes (NFD).
///
/// The order matters: `ệ` can be written with the dot below before the
/// circumflex or after it, and only one of the two is the normal form.
#[must_use]
pub fn decompose(text: &str) -> String {
    let mut out: Vec<char> = Vec::with_capacity(text.len());
    for character in text.chars() {
        expand(character, &mut out);
    }
    order(&mut out);
    out.into_iter().collect()
}

/// The text with every letter and its marks put back together where Unicode
/// has a single character for the pair (NFC).
///
/// This is what Word writes, and so what this program writes.
#[must_use]
pub fn compose(text: &str) -> String {
    let pulled: Vec<char> = decompose(text).chars().collect();
    let mut out: Vec<char> = Vec::with_capacity(pulled.len());
    // The last character that stands on its own, which is the one a mark may
    // join onto, and the class of whatever came after it.
    let mut starter: Option<usize> = None;
    let mut last_class = 0u8;

    for character in pulled {
        let class = combining_class(character);
        if let Some(at) = starter {
            // A mark may only join the starter if nothing between them stands
            // in the way: another mark of the same class or a higher one is
            // drawn in between, and joining across it would move it.
            let blocked = last_class != 0 && last_class >= class;
            if !blocked {
                if let Some(joined) = composed(out[at], character) {
                    out[at] = joined;
                    continue;
                }
            }
        }

        out.push(character);
        if class == 0 {
            starter = Some(out.len() - 1);
            last_class = 0;
        } else {
            last_class = class;
        }
    }

    out.into_iter().collect()
}

/// Whether the text holds any mark that draws on the character before it.
///
/// Text without them has nothing to put together and can be left alone, which
/// is true of nearly every document written on Windows. Text with them may
/// still be as together as it can get — not every letter and mark has a single
/// character — so this is a quick "nothing to do here" rather than a full
/// answer.
#[must_use]
pub fn has_marks(text: &str) -> bool {
    text.chars().any(|character| combining_class(character) != 0)
}

/// How a character is written when it is pulled apart, if it can be: the
/// character it starts from, and the mark drawn on it where there is one.
///
/// A few hundred characters are simply another character rather than a letter
/// and a mark — the angstrom sign is an A with a ring, the ohm sign an omega,
/// and the compatibility ideographs are the unified ideographs they duplicate.
/// Those come back with no mark.
#[must_use]
pub fn decomposed(character: char) -> Option<(char, Option<char>)> {
    if let Some(parts) = hangul::decomposed(character) {
        return Some(parts);
    }
    if let Ok(at) = table::PAIRS.binary_search_by_key(&character, |(made, _, _)| *made) {
        let (_, base, mark) = table::PAIRS[at];
        return Some((base, Some(mark)));
    }
    table::SINGLES
        .binary_search_by_key(&character, |(made, _)| *made)
        .ok()
        .map(|at| (table::SINGLES[at].1, None))
}

/// The character a letter and a mark make together, if there is one.
#[must_use]
pub fn composed(base: char, mark: char) -> Option<char> {
    if let Some(joined) = hangul::composed(base, mark) {
        return Some(joined);
    }
    table::COMPOSABLE
        .binary_search_by_key(&(base, mark), |(first, second, _)| (*first, *second))
        .ok()
        .map(|at| table::COMPOSABLE[at].2)
}

/// Pulls one character apart, and its parts in turn: `ǻ` is `å` and an acute,
/// and `å` is `a` and a ring.
fn expand(character: char, out: &mut Vec<char>) {
    match decomposed(character) {
        Some((base, mark)) => {
            expand(base, out);
            if let Some(mark) = mark {
                expand(mark, out);
            }
        }
        None => out.push(character),
    }
}

/// Korean, which is not in the table because it does not need to be.
///
/// A Hangul syllable is a leading consonant, a vowel and sometimes a trailing
/// consonant, and the character for the syllable is worked out from the three
/// by arithmetic. Unicode does the same and leaves all eleven thousand of them
/// out of its decomposition table; this does too, and the table stays a table
/// of the characters that really are exceptions.
mod hangul {
    const FIRST_SYLLABLE: u32 = 0xAC00;
    const FIRST_LEADING: u32 = 0x1100;
    const FIRST_VOWEL: u32 = 0x1161;
    /// One before the first trailing consonant: a syllable with none is
    /// counted as having trailing consonant nought.
    const BEFORE_TRAILING: u32 = 0x11A7;
    const LEADING: u32 = 19;
    const VOWELS: u32 = 21;
    const TRAILING: u32 = 28;
    const SYLLABLES: u32 = LEADING * VOWELS * TRAILING;

    /// A syllable as the syllable without its trailing consonant and that
    /// consonant, or as its leading consonant and its vowel.
    pub(super) fn decomposed(character: char) -> Option<(char, Option<char>)> {
        let code = character as u32;
        let index = code.checked_sub(FIRST_SYLLABLE).filter(|at| *at < SYLLABLES)?;

        let trailing = index % TRAILING;
        if trailing != 0 {
            let without = char::from_u32(code - trailing)?;
            return Some((without, char::from_u32(BEFORE_TRAILING + trailing)));
        }
        let leading = char::from_u32(FIRST_LEADING + index / (VOWELS * TRAILING))?;
        let vowel = char::from_u32(FIRST_VOWEL + (index % (VOWELS * TRAILING)) / TRAILING)?;
        Some((leading, Some(vowel)))
    }

    /// And the other way round.
    pub(super) fn composed(base: char, mark: char) -> Option<char> {
        let (base, mark) = (base as u32, mark as u32);

        if let (Some(leading), Some(vowel)) =
            (base.checked_sub(FIRST_LEADING), mark.checked_sub(FIRST_VOWEL))
        {
            if leading < LEADING && vowel < VOWELS {
                return char::from_u32(FIRST_SYLLABLE + (leading * VOWELS + vowel) * TRAILING);
            }
        }

        let index = base.checked_sub(FIRST_SYLLABLE).filter(|at| *at < SYLLABLES)?;
        let trailing = mark.checked_sub(BEFORE_TRAILING)?;
        if index % TRAILING == 0 && (1..TRAILING).contains(&trailing) {
            return char::from_u32(base + trailing);
        }
        None
    }
}

/// Puts each run of marks into the order the standard fixes, which is by the
/// place they are drawn: what goes under the letter before what goes over it.
///
/// The sort has to be stable, and that is not a detail: two marks drawn in the
/// same place keep the order they were typed in, because that order is what
/// says which is drawn nearer the letter.
fn order(characters: &mut [char]) {
    let mut start = 0;
    while start < characters.len() {
        if combining_class(characters[start]) == 0 {
            start += 1;
            continue;
        }
        let mut end = start;
        while end < characters.len() && combining_class(characters[end]) != 0 {
            end += 1;
        }
        characters[start..end].sort_by_key(|character| combining_class(*character));
        start = end;
    }
}

/// Where a mark is drawn, as the number the standard gives it.
///
/// Zero means it is not a mark at all but a character that stands on its own.
#[must_use]
pub fn combining_class(character: char) -> u8 {
    table::combining_class(character)
}
#[cfg(test)]
mod tests {
    use super::{combining_class, compose, decompose, has_marks};

    #[test]
    fn an_accented_letter_comes_apart_and_goes_back_together() {
        assert_eq!(decompose("é"), "e\u{0301}");
        assert_eq!(compose("e\u{0301}"), "é");
    }

    #[test]
    fn text_with_nothing_to_do_is_left_exactly_as_it_was() {
        for text in ["hello", "", "1234", "שלום", "日本語"] {
            assert_eq!(decompose(text), text);
            assert_eq!(compose(text), text);
        }
    }

    #[test]
    fn a_letter_with_two_marks_comes_apart_in_the_fixed_order() {
        // The ring is drawn on the letter and the acute above it, so the ring
        // comes first however the two were typed.
        assert_eq!(decompose("\u{01FB}"), "a\u{030A}\u{0301}");
        assert_eq!(compose("a\u{030A}\u{0301}"), "\u{01FB}");
        // Typed the other way round they are two marks in the same place, and
        // the order they were typed in is what says which is drawn nearer the
        // letter. So they stay as they were, and only the first one joins the
        // letter: there is no single character for "a with an acute and a ring
        // above it".
        assert_eq!(compose("a\u{0301}\u{030A}"), "\u{00E1}\u{030A}");
    }

    #[test]
    fn what_has_no_marks_at_all_needs_nothing_done_to_it() {
        assert!(!has_marks("café"), "the accent is part of the letter here");
        assert!(has_marks("cafe\u{0301}"), "the accent is a mark of its own here");
        assert!(!has_marks(""));
    }

    #[test]
    fn a_mark_under_the_letter_is_ordered_before_one_over_it() {
        // Cedilla is drawn below, acute above: below comes first.
        let typed = "c\u{0301}\u{0327}";
        assert_eq!(decompose(typed), "c\u{0327}\u{0301}");
    }

    #[test]
    fn every_letter_in_the_table_survives_the_round_trip() {
        for (_, _, composed_char) in super::table::COMPOSABLE {
            let text = composed_char.to_string();
            let apart = decompose(&text);
            assert_ne!(apart, text, "{composed_char} did not come apart");
            assert_eq!(compose(&apart), text, "{composed_char} did not go back together");
        }
    }

    #[test]
    fn the_table_agrees_with_itself_about_case() {
        // If "É" is "E" and an acute, then "é" must be "e" and an acute: the
        // one is the other in lower case, mark and all. A test rather than a
        // reading, because a table this long is not read carefully by anybody.
        for (composed_char, base, mark) in super::table::PAIRS {
            let mut lower = composed_char.to_lowercase();
            let (Some(single), None) = (lower.next(), lower.next()) else { continue };
            if single == *composed_char {
                continue;
            }
            let Some((lower_base, Some(lower_mark))) = super::decomposed(single) else {
                panic!(
                    "{composed_char} comes apart but {single}, which is its lower case, does not"
                )
            };
            assert_eq!(lower_mark, *mark, "{composed_char} and {single} carry different marks");

            let mut expected = base.to_lowercase();
            if let (Some(one), None) = (expected.next(), expected.next()) {
                assert_eq!(
                    lower_base, one,
                    "{composed_char} and {single} sit on different letters"
                );
            }
        }
    }

    #[test]
    fn a_mark_knows_where_it_is_drawn() {
        assert_eq!(combining_class('\u{0301}'), 230, "an acute is drawn above");
        assert_eq!(combining_class('\u{0327}'), 202, "a cedilla is drawn below");
        assert_eq!(combining_class('a'), 0, "a letter is not a mark");
    }

    #[test]
    fn every_character_there_is_has_a_combining_class() {
        for code in 0..=0x10FFFFu32 {
            if let Some(character) = char::from_u32(code) {
                let _ = combining_class(character);
            }
        }
        assert!(super::table::in_order(), "the table is out of order");
    }

    #[test]
    fn the_scripts_the_hand_written_table_left_out_come_apart_too() {
        // Vietnamese, which stacks two marks on one letter; the Hebrew points;
        // and the Arabic ones. None of them was in the table written by hand,
        // so a search for a Vietnamese word missed it if it had been typed the
        // other way.
        // The dot below is drawn nearer the letter than the circumflex, so it
        // is written first however it was typed.
        assert_eq!(decompose("\u{1EC7}"), "e\u{0323}\u{0302}", "Vietnamese");
        assert_eq!(decompose("\u{FB2E}"), "\u{05D0}\u{05B7}", "Hebrew with a point");
        assert_eq!(decompose("\u{0622}"), "\u{0627}\u{0653}", "Arabic with a madda");
        assert_eq!(compose("e\u{0302}\u{0323}"), "\u{1EC7}", "typed in the other order");
    }

    #[test]
    fn a_hangul_syllable_comes_apart_into_its_letters() {
        // Eleven thousand syllables that are in no table: the character is
        // worked out from the three letters by arithmetic, and Unicode leaves
        // them out of its own table for the same reason.
        assert_eq!(decompose("\u{D55C}"), "\u{1112}\u{1161}\u{11AB}", "한");
        assert_eq!(compose("\u{1112}\u{1161}\u{11AB}"), "\u{D55C}");
        // And one without a trailing consonant.
        assert_eq!(decompose("\u{AC00}"), "\u{1100}\u{1161}");
        assert_eq!(compose("\u{1100}\u{1161}"), "\u{AC00}");
    }

    #[test]
    fn a_character_that_is_simply_another_character_becomes_it() {
        // The angstrom sign was encoded twice over, and Unicode says the two
        // are the same character. Text holding one and text holding the other
        // have to compare equal.
        assert_eq!(compose("\u{212B}"), "\u{00C5}", "the angstrom sign is an A with a ring");
        assert_eq!(compose("\u{2126}"), "\u{03A9}", "the ohm sign is an omega");
        assert_eq!(decompose("\u{212B}"), "A\u{030A}");
    }

    #[test]
    fn what_unicode_forbids_putting_back_together_stays_apart() {
        // Some characters come apart and may never be made again: the standard
        // keeps a list of them, because making one back would change what the
        // text says or would undo a decision a later version took back.
        let apart = decompose("\u{0344}");
        assert_eq!(apart, "\u{0308}\u{0301}");
        assert_eq!(compose(&apart), apart, "a forbidden character was made again");
    }
}
