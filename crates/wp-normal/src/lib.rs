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
//! The letters people actually type: the Latin alphabets of Europe, Greek and
//! Cyrillic. Vietnamese, the Indic scripts, the Hebrew and Arabic points and
//! the Hangul syllables have canonical decompositions too and are not here yet;
//! text in them is left exactly as it was, which is the one safe thing to do
//! with text this table does not know.
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
                if let Some(joined) = table::composed(out[at], character) {
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

/// How a character is written when it is pulled apart, if it can be.
#[must_use]
pub fn decomposed(character: char) -> Option<(char, char)> {
    table::decomposed(character)
}

/// The character a letter and a mark make together, if there is one.
#[must_use]
pub fn composed(base: char, mark: char) -> Option<char> {
    table::composed(base, mark)
}

/// Pulls one character apart, and its parts in turn: `ǻ` is `å` and an acute,
/// and `å` is `a` and a ring.
fn expand(character: char, out: &mut Vec<char>) {
    match table::decomposed(character) {
        Some((base, mark)) => {
            expand(base, out);
            out.push(mark);
        }
        None => out.push(character),
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
    match character as u32 {
        0x0334..=0x0338 => 1,
        0x0316..=0x0319 | 0x031C..=0x0320 | 0x0323..=0x0326 | 0x0329..=0x0333 => 220,
        0x0339..=0x033C | 0x0347..=0x0349 | 0x034D..=0x034E | 0x0353..=0x0356 => 220,
        0x0359..=0x035A => 220,
        0x0321..=0x0322 | 0x0327..=0x0328 => 202,
        0x031B => 216,
        0x0315 | 0x031A | 0x0358 => 232,
        0x035C | 0x035F | 0x0362 => 233,
        0x035D..=0x035E | 0x0360..=0x0361 => 234,
        0x0345 => 240,
        // Everything else in the combining blocks is drawn above the letter,
        // which is where most marks go.
        0x0300..=0x036F | 0x0483..=0x0487 | 0x0591..=0x05BD => 230,
        _ => 0,
    }
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
        for (composed_char, _, _) in super::table::ENTRIES {
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
        for (composed_char, base, mark) in super::table::ENTRIES {
            let mut lower = composed_char.to_lowercase();
            let (Some(single), None) = (lower.next(), lower.next()) else { continue };
            if single == *composed_char {
                continue;
            }
            let Some((lower_base, lower_mark)) = super::table::decomposed(single) else {
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
}
