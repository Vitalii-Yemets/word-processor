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
//! [UAX #14], as the standard writes it. Each character has one of the
//! standard's forty-three classes, generated from the character database so
//! that every character there is has one; the rules LB1 to LB31 are applied in
//! the standard's order, against those classes, and the first that speaks
//! decides. Nothing is folded: where the standard tells a full stop from a
//! closing bracket, or an em dash from a slash, so does this.
//!
//! What is tailored, and the standard allows it: the South East Asian scripts,
//! which the standard hands to a dictionary and this program has none for.
//! Thai and Lao are broken where a syllable begins, which is where every
//! word begins and some places besides — see [`starts_syllable`].
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

/// The classes of [UAX #14], by the standard's own two-letter names, which
/// are what its pair table and its rules are written in.
///
/// [UAX #14]: https://www.unicode.org/reports/tr14/#Table1
#[allow(clippy::upper_case_acronyms)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Class {
    /// Mandatory break: the form feed, the line and paragraph separators.
    BK,
    /// Carriage return.
    CR,
    /// Line feed.
    LF,
    /// Combining mark: an accent, a vowel sign, a variation selector, which
    /// belongs to the character before it.
    CM,
    /// Next line.
    NL,
    /// A surrogate, which cannot appear in text and is read as a letter.
    SG,
    /// Word joiner: forbids a break either side.
    WJ,
    /// Zero width space: a break is allowed after it.
    ZW,
    /// Glue: the non-breaking space and its kind.
    GL,
    /// Space.
    SP,
    /// Zero width joiner, which holds two emoji together.
    ZWJ,
    /// Break opportunity before and after: the em dash.
    B2,
    /// Break after: the hyphens that are not the hyphen-minus, the ideographic
    /// space, the optional hyphen.
    BA,
    /// Break before.
    BB,
    /// The hyphen-minus.
    HY,
    /// Contingent break: the place an inline object sits.
    CB,
    /// Closing punctuation.
    CL,
    /// Closing parenthesis, which LB30 tells from the rest.
    CP,
    /// Exclamation and question marks, which cling to what precedes them.
    EX,
    /// Inseparable: the ellipsis, the leaders.
    IN,
    /// Non-starter: the small kana, the sound marks.
    NS,
    /// Opening punctuation.
    OP,
    /// A quotation mark, which could open or close.
    QU,
    /// Infix numeric separator: the comma and full stop inside a number.
    IS,
    /// Numeric.
    NU,
    /// Postfix numeric: a per cent sign, a degree.
    PO,
    /// Prefix numeric: a currency sign, a plus.
    PR,
    /// Symbols allowing break after: the solidus.
    SY,
    /// Ambiguous, which LB1 reads as a letter.
    AI,
    /// Alphabetic.
    AL,
    /// Conditional Japanese starter: the small kana, which LB1 reads as NS.
    CJ,
    /// An emoji base, which takes a skin tone.
    EB,
    /// An emoji modifier: the skin tone.
    EM,
    /// A Hangul syllable of two jamo, and of three.
    H2,
    H3,
    /// Hebrew letter.
    HL,
    /// Ideographic: the ideographs, the kana, the emoji.
    ID,
    /// The Hangul jamo: leading, vowel, trailing.
    JL,
    JV,
    JT,
    /// Regional indicator: half of a flag.
    RI,
    /// Complex context: the South East Asian scripts written without spaces,
    /// whose words need a dictionary.
    SA,
    /// Unknown, which LB1 reads as a letter.
    XX,
}

/// Whether a character is East Asian wide, full-width or half-width — the one
/// thing LB30 asks about a bracket, and the width in the standard's own terms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Width {
    Narrow,
    Wide,
}

/// The class of one character, as the character database gives it.
///
/// The table behind this is generated from the database and covers every code
/// point there is, so there is no character it does not know. What the rules
/// see is this after LB1 — see [`resolved_class`].
#[must_use]
pub fn class_of(character: char) -> Class {
    tables::class_of(character)
}

/// Whether a character is East Asian wide, full-width or half-width.
#[must_use]
pub fn width_of(character: char) -> Width {
    tables::width_of(character)
}

/// The class of one character after LB1: the ambiguous, the surrogates and
/// the unknown read as letters, the small kana as non-starters, and the South
/// East Asian letters as letters or, where they are marks, as marks.
#[must_use]
pub fn resolved_class(character: char) -> Class {
    match class_of(character) {
        Class::AI | Class::SG | Class::XX => Class::AL,
        Class::CJ => Class::NS,
        Class::SA => {
            if tables::within(tables::COMPLEX_MARKS, character as u32) {
                Class::CM
            } else {
                Class::AL
            }
        }
        other => other,
    }
}

/// Whether a line may be broken between two characters, with nothing else
/// known about the line.
///
/// The rules that look further than the pair — a run of spaces, a mark on
/// its letter, a pair of flags — are answered as though the two stood alone.
/// A line is asked about whole through [`opportunities`]; this is for the
/// place where two runs of text meet and only their edges are to hand.
#[must_use]
pub fn may_break(before: char, after: char) -> bool {
    let mut text = String::with_capacity(8);
    text.push(before);
    text.push(after);
    !opportunities(&text).is_empty()
}

/// Every place in a line where it may be broken, as byte offsets.
///
/// The offsets are the starts of the characters a break would put on the next
/// line. Neither end of the text is one: a break there would move nothing.
/// The rules are the standard's, LB2 to LB31, applied in its order at each
/// place between two characters, and the first that speaks decides.
#[must_use]
pub fn opportunities(text: &str) -> Vec<usize> {
    let characters: Vec<(usize, char)> = text.char_indices().collect();
    let line = Line::of(&characters);
    (1..characters.len()).filter(|at| line.breaks_before(*at)).map(|at| characters[at].0).collect()
}

/// A line of text with its classes worked out, and what each rule needs to
/// look back at.
struct Line {
    characters: Vec<char>,
    /// The class of each character after LB1.
    raw: Vec<Class>,
    /// The class each character stands for once LB9 and LB10 are applied: a
    /// mark or a joiner stands for the character it is attached to, and one
    /// attached to nothing stands for a letter.
    class: Vec<Class>,
    /// Which character each stands for: itself, or the base a mark is
    /// attached to.
    base: Vec<usize>,
    /// Whether each character is inside a number, with the character before
    /// it: what LB25 asks. See [`Self::mark_numbers`].
    in_number: Vec<bool>,
}

impl Line {
    fn of(characters: &[(usize, char)]) -> Self {
        let characters: Vec<char> = characters.iter().map(|(_, character)| *character).collect();
        let raw: Vec<Class> =
            characters.iter().map(|character| resolved_class(*character)).collect();
        let mut class = Vec::with_capacity(raw.len());
        let mut base = Vec::with_capacity(raw.len());
        for (at, own) in raw.iter().enumerate() {
            // LB9: a mark or a joiner is part of the character before it,
            // unless that character is one a mark cannot attach to. LB10: a
            // mark with nothing to attach to is a letter.
            let attached = matches!(own, Class::CM | Class::ZWJ)
                && at > 0
                && !matches!(
                    raw[at - 1],
                    Class::BK | Class::CR | Class::LF | Class::NL | Class::SP | Class::ZW
                );
            if attached {
                class.push(class[at - 1]);
                base.push(base[at - 1]);
            } else if matches!(own, Class::CM | Class::ZWJ) {
                class.push(Class::AL);
                base.push(at);
            } else {
                class.push(*own);
                base.push(at);
            }
        }
        let in_number = Self::mark_numbers(&class, &base);
        Self { characters, raw, class, base, in_number }
    }

    /// LB25, as the standard's own example of tailoring writes it: a number
    /// is `(PR | PO)? (OP | HY)? NU (NU | SY | IS)* (CL | CP)? (PR | PO)?`,
    /// and nothing inside one is broken. The pairs the rule lists by
    /// themselves would hold a per cent sign to any closing bracket and a
    /// full stop to any digit; the expression holds them only where there
    /// is a number for them to belong to, which is what the standard's own
    /// test data expects.
    ///
    /// Every character that could begin a number is tried, and a boundary
    /// inside any match is held — so "5% 3" and "1/2/3" are held throughout.
    fn mark_numbers(class: &[Class], base: &[usize]) -> Vec<bool> {
        let mut held = vec![false; class.len()];
        // The characters that stand for themselves, which is what the
        // expression is matched over: a mark inside a number is its base.
        let bases: Vec<usize> = (0..class.len()).filter(|at| base[*at] == *at).collect();
        let at = |index: usize| bases.get(index).map(|at| class[*at]);
        let sign = |class: Option<Class>| matches!(class, Some(Class::PR | Class::PO));
        for start in 0..bases.len() {
            let mut end = start;
            if sign(at(end)) {
                end += 1;
            }
            if matches!(at(end), Some(Class::OP | Class::HY)) {
                end += 1;
            }
            if at(end) != Some(Class::NU) {
                continue;
            }
            while matches!(at(end), Some(Class::NU | Class::SY | Class::IS)) {
                end += 1;
            }
            if matches!(at(end), Some(Class::CL | Class::CP)) {
                end += 1;
            }
            if sign(at(end)) {
                end += 1;
            }
            for index in start + 1..end {
                held[bases[index]] = true;
            }
        }
        held
    }

    /// The class standing before a run of spaces that ends at `at`, or the
    /// class before `at` when there is no run: what LB8 and LB14 to LB17 ask.
    fn before_spaces(&self, at: usize) -> Option<Class> {
        let mut index = at;
        while index > 0 && self.raw[index - 1] == Class::SP {
            index -= 1;
        }
        (index > 0).then(|| self.class[index - 1])
    }

    /// Whether a line may be broken before the character at `at`.
    fn breaks_before(&self, at: usize) -> bool {
        let (before, after) = (self.raw[at - 1], self.raw[at]);
        let (left, right) = (self.class[at - 1], self.class[at]);

        // LB4 and LB5: the breaks the text itself asks for, and nothing
        // broken between a carriage return and its line feed.
        if before == Class::BK {
            return true;
        }
        if before == Class::CR && after == Class::LF {
            return false;
        }
        if matches!(before, Class::CR | Class::LF | Class::NL) {
            return true;
        }
        // LB6: nothing is broken away from a mandatory break.
        if matches!(after, Class::BK | Class::CR | Class::LF | Class::NL) {
            return false;
        }
        // LB7: never before a space or a zero width space.
        if matches!(after, Class::SP | Class::ZW) {
            return false;
        }
        // LB8: a zero width space, and any spaces after it, is a break.
        if self.before_spaces(at) == Some(Class::ZW) {
            return true;
        }
        // LB8a: never after a zero width joiner.
        if before == Class::ZWJ {
            return false;
        }
        // LB9: a mark stays with what it is attached to.
        if self.base[at] != at {
            return false;
        }
        // LB11: nothing is broken away from a word joiner.
        if right == Class::WJ || left == Class::WJ {
            return false;
        }
        // LB12 and LB12a: glue holds, and holds to what precedes it unless
        // that is a space or a place to break after.
        if left == Class::GL {
            return false;
        }
        if right == Class::GL && !matches!(left, Class::SP | Class::BA | Class::HY) {
            return false;
        }
        // LB13: never before closing punctuation, or the marks that cling.
        if matches!(right, Class::CL | Class::CP | Class::EX | Class::IS | Class::SY) {
            return false;
        }
        // LB14 to LB17: what holds across a run of spaces — an opening
        // bracket to whatever follows, a quotation mark to an opening
        // bracket, a closing bracket to a non-starter, one half of a break-
        // both-ways to the other.
        let ahead = self.before_spaces(at);
        if ahead == Some(Class::OP) {
            return false;
        }
        if ahead == Some(Class::QU) && right == Class::OP {
            return false;
        }
        if matches!(ahead, Some(Class::CL | Class::CP)) && right == Class::NS {
            return false;
        }
        if ahead == Some(Class::B2) && right == Class::B2 {
            return false;
        }
        // LB18: after a space a break is allowed.
        if before == Class::SP {
            return true;
        }
        // LB19: a quotation mark could open or close, so nothing is broken
        // either side of it.
        if right == Class::QU || left == Class::QU {
            return false;
        }
        // LB20: an inline object may be broken from either side.
        if right == Class::CB || left == Class::CB {
            return true;
        }
        // LB21: never before a hyphen, a break-after or a non-starter, and
        // never after a break-before.
        if matches!(right, Class::BA | Class::HY | Class::NS) || left == Class::BB {
            return false;
        }
        // LB21a: a Hebrew letter holds the hyphen after it to what follows.
        if matches!(left, Class::HY | Class::BA) {
            let hyphen = self.base[at - 1];
            if hyphen > 0 && self.class[hyphen - 1] == Class::HL {
                return false;
            }
        }
        // LB21b: a solidus holds to a Hebrew letter after it.
        if left == Class::SY && right == Class::HL {
            return false;
        }
        // LB22: never before an ellipsis.
        if right == Class::IN {
            return false;
        }
        // LB23 and LB23a: letters hold to digits, and a prefix to an
        // ideograph or an emoji, and those to a postfix.
        let letter = |class: Class| matches!(class, Class::AL | Class::HL);
        let picture = |class: Class| matches!(class, Class::ID | Class::EB | Class::EM);
        if (letter(left) && right == Class::NU) || (left == Class::NU && letter(right)) {
            return false;
        }
        if (left == Class::PR && picture(right)) || (picture(left) && right == Class::PO) {
            return false;
        }
        // LB24: a prefix or a postfix holds to a letter either way round.
        let sign = |class: Class| matches!(class, Class::PR | Class::PO);
        if (sign(left) && letter(right)) || (letter(left) && sign(right)) {
            return false;
        }
        // LB25: a number holds together with the signs around it. See
        // [`Self::mark_numbers`].
        if self.in_number[at] {
            return false;
        }
        // LB26 and LB27: a Hangul syllable spelled in jamo holds together,
        // and holds to the signs round it.
        let jamo = |class: Class| {
            matches!(class, Class::JL | Class::JV | Class::JT | Class::H2 | Class::H3)
        };
        if left == Class::JL && matches!(right, Class::JL | Class::JV | Class::H2 | Class::H3) {
            return false;
        }
        if matches!(left, Class::JV | Class::H2) && matches!(right, Class::JV | Class::JT) {
            return false;
        }
        if matches!(left, Class::JT | Class::H3) && right == Class::JT {
            return false;
        }
        if (jamo(left) && right == Class::PO) || (left == Class::PR && jamo(right)) {
            return false;
        }
        // This program's tailoring of the standard's complex context: Thai
        // and Lao are broken where a syllable begins. See
        // [`starts_syllable`].
        if class_of(self.characters[at - 1]) == Class::SA
            && class_of(self.characters[at]) == Class::SA
        {
            return starts_syllable(self.characters[at])
                && !leads_a_syllable(self.characters[at - 1]);
        }
        // LB28 and LB29: two letters are the inside of a word, and so is a
        // full stop between a number and a letter.
        if letter(left) && letter(right) {
            return false;
        }
        if left == Class::IS && letter(right) {
            return false;
        }
        // LB30: a letter or a digit holds to an opening bracket after it,
        // and a closing bracket to one after it — unless the bracket is East
        // Asian wide, where the room round it is the bracket's own.
        let word = |class: Class| letter(class) || class == Class::NU;
        let narrow = |index: usize| width_of(self.characters[self.base[index]]) == Width::Narrow;
        if word(left) && right == Class::OP && narrow(at) {
            return false;
        }
        if left == Class::CP && narrow(at - 1) && word(right) {
            return false;
        }
        // LB30a: two regional indicators are one flag, and a flag may be
        // broken from the next — so a break is allowed only after an even
        // number of them.
        if left == Class::RI && right == Class::RI {
            let mut count = 0;
            let mut index = at;
            while index > 0 && self.class[index - 1] == Class::RI {
                count += 1;
                index = self.base[index - 1];
            }
            if count % 2 == 1 {
                return false;
            }
        }
        // LB30b: a skin tone stays with the emoji it colours — including one
        // the database has reserved room for but not yet named.
        if right == Class::EM
            && (left == Class::EB
                || tables::within(
                    tables::RESERVED_PICTOGRAPHS,
                    self.characters[self.base[at - 1]] as u32,
                ))
        {
            return false;
        }
        // LB31: everywhere else.
        true
    }
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

/// What the fold used to cost, each named in the roadmap and each now the
/// standard's own answer.
#[cfg(test)]
mod unfolded {
    use super::*;

    #[test]
    fn the_classes_are_the_standards_own() {
        assert_eq!(class_of('a'), Class::AL);
        assert_eq!(class_of('.'), Class::IS);
        assert_eq!(class_of('!'), Class::EX);
        assert_eq!(class_of(')'), Class::CP);
        assert_eq!(class_of('」'), Class::CL);
        assert_eq!(class_of('/'), Class::SY);
        assert_eq!(class_of('—'), Class::B2);
        assert_eq!(class_of('\u{00AD}'), Class::BA);
        assert_eq!(class_of('\u{200D}'), Class::ZWJ);
        assert_eq!(class_of('\u{1F1E6}'), Class::RI);
        assert_eq!(class_of('\u{1F3FB}'), Class::EM);
        assert_eq!(class_of('\u{1F466}'), Class::EB);
        assert_eq!(class_of('\u{05D0}'), Class::HL);
        assert_eq!(class_of('\u{0E01}'), Class::SA);
        assert_eq!(resolved_class('\u{0E01}'), Class::AL);
        assert_eq!(resolved_class('\u{0E31}'), Class::CM, "a Thai vowel above is a mark");
        assert_eq!(resolved_class('ょ'), Class::NS);
    }

    #[test]
    fn a_mark_takes_the_class_of_the_letter_it_is_drawn_on() {
        // LB9 whole: a mark on a letter is the letter, so a mark on the last
        // letter of a word before a space still lets the space break — and a
        // mark on an ideograph is an ideograph, and may be broken from the
        // next.
        assert_eq!(opportunities("e\u{0301} f"), vec![4]);
        assert_eq!(opportunities("日\u{0301}本"), vec![5]);
        // A mark with nothing to attach to is a letter of its own.
        assert!(!may_break('\u{0301}', 'a'));
        assert!(!may_break('a', '\u{0301}'));
    }

    #[test]
    fn a_skin_tone_stays_with_its_emoji_and_the_next_emoji_may_follow() {
        // LB30b as written: the tone holds to an emoji base, and not to
        // anything else.
        assert!(!may_break('\u{1F466}', '\u{1F3FB}'));
        assert!(may_break('\u{1F3FB}', '\u{1F466}'));
        assert!(may_break('日', '\u{1F3FB}'), "a tone after an ideograph is not held");
    }

    #[test]
    fn an_em_dash_may_be_broken_before_as_well_as_after() {
        assert!(may_break('a', '—'));
        assert!(may_break('—', 'a'));
        // But not between two of them, and not from a space.
        assert!(!may_break('—', '—'));
    }

    #[test]
    fn a_solidus_holds_between_two_digits() {
        // LB25: 1/2 is a fraction, and so is 1/2/3; a solidus with no number
        // before it begins nothing.
        assert!(opportunities("1/2").is_empty());
        assert!(opportunities("1/2/3").is_empty());
        assert!(may_break('/', '2'), "a solidus alone is not a number");
        assert!(may_break('/', 'b'), "a solidus between letters still breaks after");
    }

    #[test]
    fn a_sign_holds_to_a_number_and_to_nothing_else() {
        // LB25 as the standard's example tailors it: "$1,500.00" and "20%"
        // are numbers; a per cent sign after a closing bracket is not.
        assert!(opportunities("$1,500.00").is_empty());
        assert!(opportunities("20%").is_empty());
        assert!(opportunities("(1)%").is_empty());
        assert!(may_break('}', '%'));
        assert!(may_break('.', '3'), "a full stop with no number before it begins nothing");
        assert!(opportunities("5% 3").len() == 1);
    }

    #[test]
    fn a_hangul_syllable_spelled_in_jamo_holds_and_two_syllables_break() {
        // LB26: the jamo of one syllable hold together; between a trailing
        // jamo and the next leading one a line may be broken.
        assert!(!may_break('\u{1100}', '\u{1161}'));
        assert!(!may_break('\u{1161}', '\u{11A8}'));
        assert!(may_break('\u{11A8}', '\u{1100}'));
    }

    #[test]
    fn two_regional_indicators_are_one_flag() {
        let flag = "\u{1F1EC}\u{1F1E7}";
        assert!(opportunities(flag).is_empty(), "a flag was broken in half");
        let two = format!("{flag}{flag}");
        assert_eq!(opportunities(&two), vec![8], "two flags may be broken between");
    }

    #[test]
    fn a_hebrew_word_keeps_its_hyphen_to_what_follows() {
        // LB21a.
        assert!(opportunities("\u{05D0}-\u{05D1}").is_empty());
        assert_eq!(opportunities("a-b"), vec![2]);
    }

    #[test]
    fn a_narrow_bracket_holds_to_the_word_and_a_wide_one_does_not() {
        // LB30: "word(" is one piece; a full-width bracket after an
        // ideograph is its own.
        assert!(!may_break('d', '('));
        assert!(!may_break(')', 'w'));
        assert!(may_break('日', '（'));
        assert_eq!(width_of('（'), Width::Wide);
        assert_eq!(width_of('('), Width::Narrow);
    }

    #[test]
    fn a_run_of_spaces_is_looked_through() {
        // LB14: an opening bracket holds across the spaces after it.
        assert!(opportunities("(  a").is_empty());
        // LB16: a closing bracket holds a non-starter across spaces.
        assert!(opportunities(") ょ").is_empty());
        // LB8: a zero width space breaks after its spaces.
        assert_eq!(opportunities("a\u{200B}  b"), vec![6]);
    }

    #[test]
    fn a_zero_width_joiner_holds_two_emoji_together() {
        assert!(opportunities("\u{1F468}\u{200D}\u{1F469}").is_empty());
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
                let _ = width_of(character);
            }
        }
        assert!(tables::in_order(), "the table is out of order");
    }

    #[test]
    fn ideographs_outside_the_first_plane_are_still_ideographs() {
        // Extension B and the rest live above U+FFFF. A hand-written table
        // that stopped at the common ranges made a page of them one
        // unbreakable word.
        assert_eq!(class_of('\u{20000}'), Class::ID);
        assert!(may_break('\u{20000}', '\u{20001}'));
    }

    #[test]
    fn the_syllabaries_are_broken_between_syllables() {
        // Yi and Hangul are both written without spaces, and both wrap.
        assert!(may_break('\u{A000}', '\u{A001}'), "Yi");
        assert!(may_break('\u{AC00}', '\u{AC01}'), "Hangul");
    }

    #[test]
    fn a_mark_never_begins_a_line() {
        assert!(!may_break('a', '\u{0301}'));
        assert!(!may_break('\u{0915}', '\u{093F}'), "a Devanagari vowel sign");
    }

    #[test]
    fn tibetan_breaks_at_its_own_mark() {
        // The tsheg is what separates Tibetan syllables, and the standard says
        // a line may be broken after it.
        assert_eq!(class_of('\u{0F0B}'), Class::BA);
        assert!(may_break('\u{0F0B}', '\u{0F40}'));
    }
}
