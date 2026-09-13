//! Where a line of text may be broken.
//!
//! # Why this is not "break at the spaces"
//!
//! Because half the world writes without them. Chinese, Japanese and Korean put
//! no spaces between words, and a line may be broken between almost any two
//! characters — but not any two: a line may not begin with a full stop or a
//! closing bracket, and may not end with an opening one. Japanese typesetting
//! has a name for those rules, *kinsoku shori*, and a reader sees a broken one
//! immediately.
//!
//! And even in English, breaking only at spaces is wrong. A hyphenated word
//! breaks after its hyphen, a non-breaking space is a space that must not be
//! broken at, and a line may not begin with a comma however long the word
//! before it.
//!
//! # What is here
//!
//! The part of [UAX #14] that decides these cases: each character is given a
//! class, and the classes either side of a possible break say whether it is
//! allowed. Which class each character has is generated from the character
//! database, so every character there is has one.
//!
//! There are fewer classes here than the standard names. Its pair table is
//! forty classes square and settles a great many cases that never arise in a
//! document; the classes here are the ones that do arise, and each of the
//! standard's is folded into the nearest of them by a list in
//! `tools/unicode/generate.rs` — which is therefore where to read what the
//! folding costs.
//!
//! Hyphenation — breaking *inside* a word, at a place the language allows — is
//! a different problem needing pattern data per language, and is not here.
//!
//! [UAX #14]: https://www.unicode.org/reports/tr14/
//!
//! # Example
//!
//! ```
//! // A line may be broken after a hyphen, but never before a full stop.
//! assert!(wp_break::may_break('-', 'k'));
//! assert!(!wp_break::may_break('d', '.'));
//! ```

#![forbid(unsafe_code)]

mod tables;

/// What a character does to a break beside it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    /// A space: a break is allowed after a run of them.
    Space,
    /// A line feed or a paragraph separator, which breaks whatever is beside
    /// it.
    Mandatory,
    /// An opening bracket or quote: nothing may be broken away from it.
    Open,
    /// A closing bracket, and the punctuation that clings to what precedes it —
    /// a full stop, a comma, an exclamation mark.
    Close,
    /// A quotation mark, which could be either and is treated as neither.
    Quote,
    /// Glue: a non-breaking space or hyphen, which forbids a break on both
    /// sides.
    Glue,
    /// A character that may not begin a line, though it may end one: the small
    /// kana, the sound marks, the ellipsis.
    NonStarter,
    /// Thai and Lao: written without spaces between the words, and broken by
    /// rules of their own. See [`starts_syllable`].
    Complex,
    /// The optional hyphen: a place inside a word where the writer says a line
    /// may be broken. It is drawn only if the line is broken there, and is
    /// otherwise nothing at all.
    SoftHyphen,
    /// A hyphen, which a line may be broken after.
    Hyphen,
    /// Something a break is allowed after: an en dash, a slash, an ideographic
    /// space.
    BreakAfter,
    /// Something a break is allowed before.
    BreakBefore,
    /// An ideograph or a kana, which may be broken between.
    Ideograph,
    /// A digit.
    Numeric,
    /// A sign that goes before a number, such as a currency sign.
    Prefix,
    /// A sign that goes after one, such as a per cent sign.
    Postfix,
    /// A letter.
    Alphabetic,
}

/// The class of one character.
///
/// The table behind this is generated from the character database and covers
/// every code point there is, so there is no character it does not know. The
/// one thing decided here rather than there is the optional hyphen: the
/// standard puts it with the ordinary hyphens, and this program has to tell it
/// apart from them because it is drawn only where a line is broken.
#[must_use]
pub fn class_of(character: char) -> Class {
    if character == '\u{00AD}' {
        return Class::SoftHyphen;
    }
    tables::class_of(character)
}

/// Whether a line may be broken between two characters.
///
/// The rules are applied in the standard's order, and the first that speaks
/// decides. Anything the rules say nothing about is a break opportunity only
/// between two things that are not letters of the same word — which is what
/// the last two rules say.
#[must_use]
pub fn may_break(before: char, after: char) -> bool {
    let (left, right) = (class_of(before), class_of(after));

    // LB4 and LB5: a line feed breaks, and nothing may be broken away from it.
    if left == Class::Mandatory || right == Class::Mandatory {
        return left == Class::Mandatory;
    }

    // LB7: never before a space; a run of spaces goes with the line it ends.
    if right == Class::Space {
        return false;
    }

    // LB12 and LB12a: glue forbids a break on both sides. That is what makes a
    // non-breaking space non-breaking.
    if left == Class::Glue || right == Class::Glue {
        return false;
    }

    // LB18: after a space a break is always allowed.
    if left == Class::Space {
        return true;
    }

    // LB13 and LB16: never before closing punctuation or a non-starter, which
    // is the rule that keeps a full stop off the beginning of a line and a
    // small kana with the syllable it belongs to.
    if matches!(right, Class::Close | Class::NonStarter) {
        return false;
    }

    // LB14: never after an opening bracket.
    if left == Class::Open {
        return false;
    }

    // LB19: a quotation mark could open or close, so nothing is broken either
    // side of it.
    if left == Class::Quote || right == Class::Quote {
        return false;
    }

    // LB25: a number holds together with the signs around it. This is why
    // "$1,500" and "20%" are never broken up.
    if matches!(left, Class::Numeric | Class::Prefix)
        && matches!(right, Class::Numeric | Class::Postfix)
    {
        return false;
    }
    if left == Class::Numeric && right == Class::Alphabetic {
        return false;
    }

    // LB6 as it applies to the optional hyphen: the writer put it there to say
    // a line may be broken inside this word, so it may — after it and never
    // before it, the same as any other hyphen. What makes it different is what
    // is drawn, which is the layout's business rather than this one's.
    if right == Class::SoftHyphen {
        return false;
    }
    if left == Class::SoftHyphen {
        return true;
    }

    // LB21: a break is allowed after a hyphen and before a break-before, and
    // never before a hyphen — "well-known" breaks after the hyphen, never in
    // front of it.
    if right == Class::Hyphen {
        return false;
    }
    if left == Class::Hyphen {
        // Except between two digits, where the hyphen is a minus sign or part
        // of a number and breaking would read as arithmetic.
        return right != Class::Numeric;
    }
    if left == Class::BreakAfter || right == Class::BreakBefore {
        return true;
    }

    // LB8a and LB23: an ideograph may be broken from anything, and anything
    // from an ideograph, which is what makes text without spaces wrap at all.
    if left == Class::Ideograph || right == Class::Ideograph {
        return true;
    }

    // LB28a, the standard's "complex context": Thai and Lao are written
    // without spaces between the words, and where one word ends is a matter
    // for a dictionary. What can be known without one is where a syllable
    // begins, and a break is allowed there — never inside a syllable, and
    // never between a vowel written before its consonant and that consonant.
    // See the note on [`starts_syllable`].
    if left == Class::Complex && right == Class::Complex {
        return starts_syllable(after) && !leads_a_syllable(before);
    }
    // Against anything else the standard resolves these to ordinary letters,
    // so a Thai word is not broken away from the Latin one it is joined to.
    if left == Class::Complex || right == Class::Complex {
        return false;
    }

    // LB28 and LB29: two letters, or a letter and a digit, are the inside of a
    // word and are never broken.
    false
}

/// Whether a character may begin a syllable of Thai or Lao.
///
/// # Why a syllable and not a word
///
/// Because a word cannot be found without a dictionary. Thai is written with
/// no spaces inside a sentence, and which of several readings of a run of
/// letters is the intended one is a question about the language rather than
/// about the letters: the standard says so outright, and Word ships a
/// dictionary to answer it.
///
/// This program has none, and inventing one is not a thing a program may do.
/// So what is offered is the next best true thing: a break wherever a syllable
/// begins. Every word boundary is a syllable boundary, so no break is missed;
/// some syllable boundaries are inside a word, so some breaks are offered that
/// a Thai reader would not choose. A line broken inside a word reads badly; a
/// line that cannot be broken at all runs off the page. Named in the roadmap.
#[must_use]
pub fn starts_syllable(character: char) -> bool {
    !clings_to_what_precedes(character)
}

/// Whether a character is one of the vowels written to the left of the
/// consonant it belongs to.
///
/// They are stored in the order they are drawn — unlike the scripts that
/// reorder — so nothing has to move. But the consonant after one belongs with
/// it, and a line broken between the two would put a vowel at the end of one
/// line and its consonant at the start of the next.
#[must_use]
pub fn leads_a_syllable(character: char) -> bool {
    matches!(character as u32, 0x0E40..=0x0E44 | 0x0EC0..=0x0EC4)
}

/// Whether a character hangs on the one before it: the tone marks, the vowels
/// written above and below, and the two that are written after.
fn clings_to_what_precedes(character: char) -> bool {
    matches!(
        character as u32,
        // Thai. The vowels written above and below a consonant are the
        // obvious ones — but so are the vowels written *after* it: ะ and า
        // take room of their own on the line and are still part of the
        // syllable, and a line beginning with one reads as badly as a line
        // beginning with a tone mark. The repetition mark and the abbreviation
        // mark follow a word and go with it.
        0x0E2F..=0x0E3A | 0x0E45..=0x0E4E
        // Lao, which is written the same way.
        | 0x0EAF..=0x0EBC | 0x0EC6..=0x0ECD
    )
}

/// Every place in a line where it may be broken, as byte offsets.
///
/// The offsets are the starts of the characters a break would put on the next
/// line. Neither end of the text is one: a break there would move nothing.
#[must_use]
pub fn opportunities(text: &str) -> Vec<usize> {
    let mut out = Vec::new();
    let mut previous: Option<(usize, char)> = None;
    for (offset, character) in text.char_indices() {
        if let Some((_, before)) = previous {
            if may_break(before, character) {
                out.push(offset);
            }
        }
        previous = Some((offset, character));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_word_is_never_broken_inside() {
        assert!(!may_break('e', 'l'));
        assert!(!may_break('a', '1'));
    }

    #[test]
    fn a_line_breaks_after_a_space_and_not_before_one() {
        assert!(may_break(' ', 'w'));
        assert!(!may_break('o', ' '));
        // Two spaces stay together with the line they end.
        assert!(!may_break(' ', ' '));
    }

    #[test]
    fn a_non_breaking_space_does_not_break() {
        assert!(!may_break('\u{00A0}', 'w'));
        assert!(!may_break('o', '\u{00A0}'));
    }

    #[test]
    fn a_hyphenated_word_breaks_after_the_hyphen() {
        assert!(may_break('-', 'k'));
        assert!(!may_break('l', '-'), "a line may not begin with a hyphen");
    }

    #[test]
    fn a_line_never_begins_with_the_punctuation_that_ends_a_sentence() {
        for mark in ['.', ',', ';', ':', '!', '?', ')', ']', '}'] {
            assert!(!may_break('d', mark), "a line could begin with {mark}");
        }
    }

    #[test]
    fn a_line_never_ends_with_an_opening_bracket() {
        for bracket in ['(', '[', '{'] {
            assert!(!may_break(bracket, 'a'), "a line could end with {bracket}");
        }
    }

    #[test]
    fn a_number_holds_together_with_its_signs() {
        assert!(!may_break('$', '1'));
        assert!(!may_break('5', '%'));
        assert!(!may_break('1', '5'));
        assert!(!may_break('2', '0'));
    }

    #[test]
    fn text_without_spaces_breaks_between_its_characters() {
        // Japanese: a line may be broken between two ideographs.
        assert!(may_break('日', '本'));
        assert!(may_break('あ', 'い'));
    }

    #[test]
    fn japanese_punctuation_never_begins_a_line() {
        assert!(!may_break('本', '。'), "a line could begin with a full stop");
        assert!(!may_break('本', '、'), "a line could begin with a comma");
        assert!(!may_break('本', '」'), "a line could begin with a closing bracket");
        assert!(!may_break('「', '本'), "a line could end with an opening bracket");
    }

    #[test]
    fn a_small_kana_stays_with_the_syllable_before_it() {
        assert!(!may_break('き', 'ょ'), "the small kana was left to begin a line");
        assert!(!may_break('ラ', 'ー'), "the sound mark was left to begin a line");
    }

    #[test]
    fn the_places_a_line_may_break_are_found_in_order() {
        let text = "one two three";
        assert_eq!(opportunities(text), vec![4, 8]);
    }

    #[test]
    fn a_line_break_in_the_text_is_a_break_wherever_it_falls() {
        assert!(may_break('\n', 'a'));
        assert!(!may_break('a', '\n'), "nothing is broken away from the break itself");
    }
}

#[cfg(test)]
mod thai {
    use super::*;

    // A sentence of Thai: "I can eat glass" — ฉันกินกระจกได้, which is the
    // sentence every script is tested with.
    const KO: char = '\u{0E01}'; // the consonant k
    const NO: char = '\u{0E19}'; // the consonant n
    const CHO: char = '\u{0E09}'; // the consonant ch
    const SARA_A: char = '\u{0E31}'; // the vowel written above
    const SARA_AM: char = '\u{0E33}'; // the vowel written after
    const SARA_I: char = '\u{0E34}'; // the vowel written above
    const SARA_E: char = '\u{0E40}'; // the vowel written before
    const MAI_EK: char = '\u{0E48}'; // a tone mark

    #[test]
    fn a_line_of_thai_may_be_broken_where_a_syllable_begins() {
        // Which is what makes it wrap at all: written without spaces, a line
        // of Thai that could only be broken at a space could not be broken.
        assert!(may_break(NO, KO), "a consonant beginning a syllable is a break");
        assert!(may_break(NO, SARA_E), "so is a vowel written before its consonant");
    }

    #[test]
    fn a_syllable_is_never_broken_into() {
        assert!(!may_break(KO, SARA_A), "a vowel was taken off its consonant");
        assert!(!may_break(KO, SARA_I), "a vowel was taken off its consonant");
        assert!(!may_break(KO, MAI_EK), "a tone mark was taken off its syllable");
        assert!(!may_break(KO, SARA_AM), "a vowel written after was taken off it");
        assert!(!may_break(SARA_A, MAI_EK), "a tone mark was taken off a vowel");
    }

    #[test]
    fn a_vowel_written_before_its_consonant_keeps_it() {
        // เก is stored in the order it is drawn, so nothing moves — but the
        // two are one syllable, and a break between them would leave a vowel
        // hanging at the end of a line.
        assert!(!may_break(SARA_E, KO));
        assert!(leads_a_syllable(SARA_E));
    }

    #[test]
    fn thai_is_not_broken_from_the_latin_beside_it() {
        // The standard resolves these to ordinary letters against anything
        // that is not one of them, so a Thai word joined to a Latin one is
        // one word.
        assert!(!may_break(KO, 'a'));
        assert!(!may_break('a', KO));
    }

    #[test]
    fn a_space_still_breaks_and_a_full_stop_still_clings() {
        assert!(may_break(' ', KO), "a space is a space in any script");
        assert!(!may_break(KO, '.'), "a full stop began a line");
    }

    #[test]
    fn a_run_of_thai_offers_a_break_before_every_consonant() {
        // ฉันกิน is two words, and this offers three breaks: before each of
        // the three consonants that are not carrying a vowel of their own.
        // The one before the น that *ends* the first word is a break no Thai
        // reader would choose — and telling it from the one that begins the
        // second word is exactly what needs the dictionary this program has
        // not got. Every real break is offered; some that are not real are
        // offered too, and that is the trade, written down.
        let text: String = [CHO, SARA_A, NO, KO, SARA_I, NO].iter().collect();
        let found = opportunities(&text);
        assert_eq!(found, vec![6, 9, 15], "{found:?}");
    }

    #[test]
    fn lao_is_read_the_same_way() {
        let (ko, sara_i, ko_lao) = ('\u{0E81}', '\u{0EB4}', '\u{0E81}');
        assert!(!may_break(ko, sara_i));
        assert!(may_break(sara_i, ko_lao));
        assert!(leads_a_syllable('\u{0EC0}'));
    }
    #[test]
    fn a_vowel_written_after_its_consonant_keeps_it_too() {
        // ะ and า are written to the right of the consonant and take room of
        // their own, which makes them look like letters — and they are not:
        // a line beginning with one is a line beginning in the middle of a
        // syllable.
        for after in ['\u{0E30}', '\u{0E32}', '\u{0E33}'] {
            assert!(!may_break(KO, after), "{after:?} was left to begin a line");
        }
        // And the Lao ones.
        for after in ['\u{0EB0}', '\u{0EB2}', '\u{0EB3}'] {
            assert!(!may_break('\u{0E81}', after), "{after:?} was left to begin a line");
        }
    }
}

#[cfg(test)]
mod hyphens {
    use super::*;

    const SOFT: char = '\u{00AD}';
    const HARD: char = '\u{2011}';

    #[test]
    fn a_line_may_be_broken_after_an_optional_hyphen() {
        // Which is the whole of what it is for: the writer marking a place
        // inside a word where a break would be all right.
        assert!(may_break(SOFT, 'd'));
        assert!(!may_break('n', SOFT), "a line ended before the hyphen rather than after it");
    }

    #[test]
    fn a_non_breaking_hyphen_is_not_a_place_to_break() {
        // The other half of the pair: a hyphen that is part of the word and
        // must not end a line, which is what a telephone number needs.
        assert!(!may_break(HARD, '5'));
        assert!(!may_break('5', HARD));
    }

    #[test]
    fn an_ordinary_hyphen_still_breaks_after_itself() {
        assert!(may_break('-', 'k'));
        assert!(!may_break('l', '-'));
    }

    #[test]
    fn the_places_in_a_hyphenated_word_are_where_the_hyphens_are() {
        let text = format!("hy{SOFT}phen{SOFT}ation");
        // After each optional hyphen, and nowhere else inside the word.
        assert_eq!(opportunities(&text), vec![4, 10]);
    }
}

/// What the generated table added: the scripts and the planes nobody listed by
/// hand, which used to fall through to "a letter" and so were never broken.
#[cfg(test)]
mod coverage {
    use super::*;

    #[test]
    fn every_character_there_is_has_a_class() {
        for code in 0..=0x10FFFFu32 {
            if let Some(character) = char::from_u32(code) {
                let _ = class_of(character);
            }
        }
        assert!(tables::in_order(), "the table is out of order");
    }

    #[test]
    fn ideographs_outside_the_first_plane_are_still_ideographs() {
        // Extension B and the rest live above U+FFFF. A hand-written table
        // that stopped at the common ranges made a page of them one
        // unbreakable word.
        assert_eq!(class_of('\u{20000}'), Class::Ideograph);
        assert!(may_break('\u{20000}', '\u{20001}'));
    }

    #[test]
    fn the_syllabaries_are_broken_between_syllables() {
        // Yi and Hangul are both written without spaces, and both wrap.
        assert!(may_break('\u{A000}', '\u{A001}'), "Yi");
        assert!(may_break('\u{AC00}', '\u{AC01}'), "Hangul");
    }

    #[test]
    fn a_hangul_syllable_spelled_in_jamo_is_not_broken_apart() {
        // The consonants and vowels of one syllable are one syllable; a line
        // broken between them reads as nonsense.
        assert!(!may_break('\u{1100}', '\u{1161}'));
        assert!(!may_break('\u{1161}', '\u{11A8}'));
    }

    #[test]
    fn a_mark_never_begins_a_line() {
        // The standard gives a combining mark the class of the letter it is
        // drawn on; here it is simply a letter, which comes to the same thing
        // at a break.
        assert!(!may_break('a', '\u{0301}'));
        assert!(!may_break('\u{0915}', '\u{093F}'), "a Devanagari vowel sign");
    }

    #[test]
    fn tibetan_breaks_at_its_own_mark() {
        // The tsheg is what separates Tibetan syllables, and the standard says
        // a line may be broken after it.
        assert_eq!(class_of('\u{0F0B}'), Class::BreakAfter);
        assert!(may_break('\u{0F0B}', '\u{0F40}'));
    }
}
