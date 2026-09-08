//! Which Arabic letters join to their neighbours, and which do not.
//!
//! # Why this decides everything about Arabic
//!
//! Arabic is written joined. A letter takes one of four forms depending on
//! whether it has a neighbour to join on each side: isolated, at the start of a
//! join, in the middle, or at the end. The characters in a document never say
//! which form is meant — there is one character per letter, and choosing the
//! form is the reader's job, and therefore the program's.
//!
//! A few letters join only on the right (`Right`), so the letter after them
//! starts a new join even in the middle of a word. Marks and vowel signs join
//! nothing and are transparent: a letter looks straight through them to find
//! its real neighbour, which is why a word with vowel marks joins exactly as it
//! would without them.
//!
//! # Where the data comes from
//!
//! `ArabicShaping.txt` of the Unicode Character Database, reduced to the ranges
//! this needs. Written out as ranges rather than a generated table: the ranges
//! are what the data actually says, and a list of thirty of them is smaller and
//! far easier to check than a thousand entries.

/// How a character joins to what is beside it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Joining {
    /// Joins on both sides: most Arabic letters.
    Dual,
    /// Joins only to the letter before it, so the next one starts afresh.
    Right,
    /// Joins to neither side, and stops a join running through it.
    NonJoining,
    /// Joins nothing and is looked straight through: marks and vowel signs.
    Transparent,
    /// Continues a join without being a letter, as the tatweel does.
    Causing,
}

/// Which form a letter takes, once its neighbours are known.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Form {
    /// No join on either side.
    Isolated,
    /// Joined to what follows.
    Initial,
    /// Joined on both sides.
    Medial,
    /// Joined to what precedes.
    Final,
}

impl Form {
    /// The OpenType feature that selects this form.
    #[must_use]
    pub fn feature(self) -> &'static [u8; 4] {
        match self {
            Self::Isolated => b"isol",
            Self::Initial => b"init",
            Self::Medial => b"medi",
            Self::Final => b"fina",
        }
    }
}

/// Ranges of characters that join on both sides.
///
/// The Arabic letters proper, plus the extended sets used for Persian, Urdu,
/// Sindhi, Kashmiri and the African languages written in Arabic script.
const DUAL: &[(u32, u32)] = &[
    (0x0620, 0x0620),
    (0x0626, 0x0626),
    (0x0628, 0x0628),
    (0x062A, 0x062E),
    (0x0633, 0x063F),
    (0x0641, 0x0647),
    (0x0649, 0x064A),
    (0x066E, 0x066F),
    (0x0678, 0x0687),
    (0x069A, 0x06BF),
    (0x06C1, 0x06C2),
    (0x06CC, 0x06CC),
    (0x06CE, 0x06CE),
    (0x06D0, 0x06D1),
    (0x06FA, 0x06FC),
    (0x06FF, 0x06FF),
    // Syriac and the extended Arabic blocks join the same way.
    (0x0712, 0x0714),
    (0x071A, 0x071D),
    (0x071F, 0x0727),
    (0x0729, 0x0729),
    (0x072B, 0x072B),
    (0x072D, 0x072E),
    (0x074E, 0x0758),
    (0x075C, 0x076A),
    (0x076D, 0x0770),
    (0x0772, 0x0772),
    (0x0775, 0x0777),
    (0x077A, 0x077F),
    (0x08A0, 0x08A9),
    (0x08AF, 0x08B0),
    (0x08B3, 0x08B4),
];

/// Ranges of characters that join only to what comes before them.
const RIGHT: &[(u32, u32)] = &[
    (0x0622, 0x0625),
    (0x0627, 0x0627),
    (0x0629, 0x0629),
    (0x062F, 0x0632),
    (0x0640, 0x0640),
    (0x0648, 0x0648),
    (0x0671, 0x0673),
    (0x0675, 0x0677),
    (0x0688, 0x0699),
    (0x06C0, 0x06C0),
    (0x06C3, 0x06CB),
    (0x06CD, 0x06CD),
    (0x06CF, 0x06CF),
    (0x06D2, 0x06D3),
    (0x06D5, 0x06D5),
    (0x06EE, 0x06EF),
    (0x0710, 0x0710),
    (0x0715, 0x0719),
    (0x071E, 0x071E),
    (0x0728, 0x0728),
    (0x072A, 0x072A),
    (0x072C, 0x072C),
    (0x072F, 0x072F),
    (0x074D, 0x074D),
    (0x0759, 0x075B),
    (0x076B, 0x076C),
    (0x0771, 0x0771),
    (0x0773, 0x0774),
    (0x0778, 0x0779),
    (0x08AA, 0x08AE),
    (0x08B1, 0x08B2),
];

/// Ranges of marks, which join nothing and are looked through.
///
/// The combining marks generally, not only the Arabic ones: a combining acute
/// between two Arabic letters must not break their join either.
const TRANSPARENT: &[(u32, u32)] = &[
    (0x0300, 0x036F),
    (0x0483, 0x0489),
    (0x0591, 0x05BD),
    (0x05BF, 0x05BF),
    (0x05C1, 0x05C2),
    (0x05C4, 0x05C5),
    (0x05C7, 0x05C7),
    (0x0610, 0x061A),
    (0x064B, 0x065F),
    (0x0670, 0x0670),
    (0x06D6, 0x06DC),
    (0x06DF, 0x06E4),
    (0x06E7, 0x06E8),
    (0x06EA, 0x06ED),
    (0x0730, 0x074A),
    (0x07EB, 0x07F3),
    (0x0816, 0x0819),
    (0x081B, 0x0823),
    (0x0825, 0x0827),
    (0x0829, 0x082D),
    (0x0859, 0x085B),
    (0x08D3, 0x08E1),
    (0x08E3, 0x0902),
    (0x200B, 0x200F),
    (0x2060, 0x2064),
    (0xFE00, 0xFE0F),
    (0xFE20, 0xFE2F),
];

fn in_ranges(ranges: &[(u32, u32)], value: u32) -> bool {
    ranges.iter().any(|(first, last)| value >= *first && value <= *last)
}

/// How a character joins to what is beside it.
#[must_use]
pub fn joining_of(character: char) -> Joining {
    let value = character as u32;

    // The tatweel is a stretch of line: it joins on both sides and is a letter
    // of no shape at all, which is what makes it the way Arabic is stretched.
    if value == 0x0640 {
        return Joining::Causing;
    }
    if in_ranges(TRANSPARENT, value) {
        return Joining::Transparent;
    }
    if in_ranges(DUAL, value) {
        return Joining::Dual;
    }
    if in_ranges(RIGHT, value) {
        return Joining::Right;
    }
    Joining::NonJoining
}

/// Whether a character belongs to a script written joined.
#[must_use]
pub fn is_joining_script(character: char) -> bool {
    matches!(
        character as u32,
        0x0600..=0x06FF   // Arabic
        | 0x0700..=0x074F // Syriac
        | 0x0750..=0x077F // Arabic Supplement
        | 0x08A0..=0x08FF // Arabic Extended-A
        | 0xFB50..=0xFDFF // Arabic Presentation Forms-A
        | 0xFE70..=0xFEFF // Arabic Presentation Forms-B
    )
}

/// Works out the form every character of a run takes.
///
/// A character's form depends on whether it can join leftwards and rightwards,
/// which depends in turn on its neighbours — looking through any marks between
/// them, because a mark is not a neighbour.
#[must_use]
pub fn forms(text: &[char]) -> Vec<Form> {
    let joinings: Vec<Joining> = text.iter().map(|character| joining_of(*character)).collect();
    let mut forms = vec![Form::Isolated; text.len()];

    for index in 0..text.len() {
        if joinings[index] == Joining::Transparent {
            // A mark takes no form of its own: it is drawn over its letter.
            continue;
        }

        // The nearest neighbour on each side that is not a mark.
        let before = (0..index)
            .rev()
            .find(|at| joinings[*at] != Joining::Transparent)
            .map(|at| joinings[at]);
        let after = (index + 1..text.len())
            .find(|at| joinings[*at] != Joining::Transparent)
            .map(|at| joinings[at]);

        // A letter can be joined to from the left if it joins at all, and can
        // join onwards only if it joins on both sides. A space joins nothing
        // in either direction, which is what breaks a word into words.
        let receives = matches!(joinings[index], Joining::Dual | Joining::Right | Joining::Causing);
        let joins_forward = matches!(joinings[index], Joining::Dual | Joining::Causing);

        let joined_before = receives && matches!(before, Some(Joining::Dual | Joining::Causing));
        let joined_after = joins_forward
            && matches!(after, Some(Joining::Dual | Joining::Right | Joining::Causing));

        forms[index] = match (joined_before, joined_after) {
            (true, true) => Form::Medial,
            (true, false) => Form::Final,
            (false, true) => Form::Initial,
            (false, false) => Form::Isolated,
        };
    }

    forms
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shapes(text: &str) -> Vec<Form> {
        forms(&text.chars().collect::<Vec<char>>())
    }

    #[test]
    fn a_letter_on_its_own_is_isolated() {
        // Beh, which joins on both sides but has nothing to join to.
        assert_eq!(shapes("\u{0628}"), vec![Form::Isolated]);
    }

    #[test]
    fn a_word_of_joining_letters_runs_start_middle_end() {
        // Beh, beh, beh: the first opens the join, the last closes it.
        assert_eq!(
            shapes("\u{0628}\u{0628}\u{0628}"),
            vec![Form::Initial, Form::Medial, Form::Final]
        );
    }

    #[test]
    fn a_letter_that_joins_only_rightwards_ends_the_join() {
        // Beh, alef, beh. Alef joins to what precedes it but not to what
        // follows, so the beh after it starts a new join.
        assert_eq!(
            shapes("\u{0628}\u{0627}\u{0628}"),
            vec![Form::Initial, Form::Final, Form::Isolated]
        );
    }

    #[test]
    fn a_mark_between_two_letters_does_not_break_the_join() {
        // Beh, fatha, beh: the mark is looked straight through.
        let result = shapes("\u{0628}\u{064E}\u{0628}");
        assert_eq!(result[0], Form::Initial);
        assert_eq!(result[2], Form::Final, "the join reaches across the mark");
    }

    #[test]
    fn a_space_breaks_a_join() {
        assert_eq!(
            shapes("\u{0628} \u{0628}"),
            vec![Form::Isolated, Form::Isolated, Form::Isolated]
        );
    }

    #[test]
    fn the_tatweel_carries_a_join_through_itself() {
        // Beh, tatweel, beh — the way Arabic is stretched for justification.
        assert_eq!(
            shapes("\u{0628}\u{0640}\u{0628}"),
            vec![Form::Initial, Form::Medial, Form::Final]
        );
    }

    #[test]
    fn latin_letters_join_nothing() {
        assert_eq!(joining_of('a'), Joining::NonJoining);
        assert_eq!(shapes("abc"), vec![Form::Isolated; 3]);
    }

    #[test]
    fn the_joining_kinds_are_what_the_data_says() {
        assert_eq!(joining_of('\u{0628}'), Joining::Dual, "beh joins both sides");
        assert_eq!(joining_of('\u{0627}'), Joining::Right, "alef joins rightwards only");
        assert_eq!(joining_of('\u{064E}'), Joining::Transparent, "fatha is a mark");
        assert_eq!(joining_of('\u{0640}'), Joining::Causing, "tatweel carries a join");
    }

    #[test]
    fn each_form_names_the_feature_that_selects_it() {
        assert_eq!(Form::Isolated.feature(), b"isol");
        assert_eq!(Form::Initial.feature(), b"init");
        assert_eq!(Form::Medial.feature(), b"medi");
        assert_eq!(Form::Final.feature(), b"fina");
    }

    #[test]
    fn arabic_is_recognised_as_a_joining_script() {
        assert!(is_joining_script('\u{0628}'));
        assert!(is_joining_script('\u{0710}'), "Syriac joins too");
        assert!(!is_joining_script('a'));
        assert!(!is_joining_script('\u{05D0}'), "Hebrew does not join");
    }

    #[test]
    fn a_real_word_takes_the_forms_a_reader_would_expect() {
        // "بسم" — beh, seen, meem. All three join both ways, so the middle one
        // is medial and the outer two open and close the join.
        assert_eq!(
            shapes("\u{0628}\u{0633}\u{0645}"),
            vec![Form::Initial, Form::Medial, Form::Final]
        );
    }
}
