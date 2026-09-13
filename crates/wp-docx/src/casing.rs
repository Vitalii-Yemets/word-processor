//! Turning letters into their capitals and back, in the language they are in.
//!
//! # Why the language comes into it
//!
//! Because the answer is not the same everywhere. The capital of "i" is "I" in
//! English and "İ" in Turkish, where the dotted and dotless i are two different
//! letters: a Turkish word put into capitals the English way says a different
//! word. The lower case of "I" is "i" in English and "ı" in Turkish, for the
//! same reason.
//!
//! Greek drops its accents in capitals — άνθρωπος is ΑΝΘΡΩΠΟΣ and not
//! ΆΝΘΡΩΠΟΣ — which is a rule about the writing rather than about the letters.
//! Lithuanian keeps the dot on an i that carries an accent, where every other
//! language drops it.
//!
//! And two rules are not about any language in particular: the capital of ß is
//! SS, which is one letter becoming two, and a final ς is written σ anywhere
//! but at the end of a word. Both of those the standard library already knows,
//! and they are left to it.
//!
//! # What a tailoring is
//!
//! The document says what language each run is in — `w:lang` — and that tag
//! chooses between the handful of rules above. Everything with no rule of its
//! own gets the default, which is the standard's.

/// Which set of rules a piece of text is read by.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tailoring {
    /// What the standard says, which is right for almost every language.
    #[default]
    Default,
    /// Turkish and Azerbaijani, where the dotted and the dotless i are two
    /// letters rather than two shapes of one.
    Turkic,
    /// Lithuanian, which keeps the dot on an i under an accent.
    Lithuanian,
    /// Greek, which drops its accents in capitals.
    Greek,
}

impl Tailoring {
    /// The rules a language tag asks for.
    ///
    /// The tag is what the document carries: `tr-TR`, `az-Latn-AZ`, `el-GR`.
    /// Only the language itself decides — a Turkish document is Turkish in
    /// every country that writes it.
    #[must_use]
    pub fn of(tag: &str) -> Self {
        let language = tag.split(['-', '_']).next().unwrap_or_default().to_ascii_lowercase();
        match language.as_str() {
            "tr" | "az" => Self::Turkic,
            "lt" => Self::Lithuanian,
            "el" => Self::Greek,
            _ => Self::Default,
        }
    }
}

/// The dot written over a letter, which several of these rules move about.
const DOT_ABOVE: char = '\u{0307}';

/// Everything in capitals.
#[must_use]
pub fn upper(text: &str, tailoring: Tailoring) -> String {
    match tailoring {
        Tailoring::Turkic => {
            text.chars().flat_map(|character| turkic_upper(character).into_iter()).collect()
        }
        // The dot over an i or a j is dropped when the letter is capitalised,
        // and Lithuanian is the language that put it there.
        Tailoring::Lithuanian => {
            let mut out = String::with_capacity(text.len());
            let mut after_i = false;
            for character in text.chars() {
                if after_i && character == DOT_ABOVE {
                    after_i = false;
                    continue;
                }
                after_i = matches!(character, 'i' | 'j' | '\u{012F}');
                out.extend(character.to_uppercase());
            }
            out
        }
        Tailoring::Greek => {
            let mut out = String::with_capacity(text.len());
            for character in text.chars() {
                match greek_capital(character) {
                    Some(capital) => out.push(capital),
                    // The accents Greek drops in capitals, written as marks of
                    // their own. The dialytika is not one of them: it says two
                    // vowels are read apart and is kept.
                    None if matches!(
                        character,
                        '\u{0300}' | '\u{0301}' | '\u{0342}' | '\u{0345}'
                    ) => {}
                    None => out.extend(character.to_uppercase()),
                }
            }
            out
        }
        Tailoring::Default => text.to_uppercase(),
    }
}

/// And everything in small letters.
#[must_use]
pub fn lower(text: &str, tailoring: Tailoring) -> String {
    match tailoring {
        Tailoring::Turkic => {
            let mut out = String::with_capacity(text.len());
            let mut characters = text.chars().peekable();
            while let Some(character) = characters.next() {
                match character {
                    // The dotted capital is the dotted small letter.
                    '\u{0130}' => out.push('i'),
                    // The undotted capital is the undotted small letter —
                    // unless the dot is written after it as a mark of its own,
                    // which is how "İ" is spelt the long way.
                    'I' => {
                        if characters.peek() == Some(&DOT_ABOVE) {
                            characters.next();
                            out.push('i');
                        } else {
                            out.push('\u{0131}');
                        }
                    }
                    _ => out.extend(character.to_lowercase()),
                }
            }
            out
        }
        // An i under an accent keeps its dot, so the dot is written back in as
        // a mark of its own before the accent.
        Tailoring::Lithuanian => {
            let mut out = String::with_capacity(text.len());
            let mut characters = text.chars().peekable();
            while let Some(character) = characters.next() {
                let dotted = matches!(character, 'I' | 'J' | '\u{012E}');
                out.extend(character.to_lowercase());
                if dotted && characters.peek().is_some_and(|next| is_accent_above(*next)) {
                    out.push(DOT_ABOVE);
                }
            }
            out
        }
        // Greek needs nothing of its own going this way: the rule about the
        // final sigma is the standard's, and the standard library keeps it.
        Tailoring::Default | Tailoring::Greek => text.to_lowercase(),
    }
}

/// The capitals of one character, for a caller that works a letter at a time.
///
/// Which is what drawing does: `w:caps` draws a capital without changing the
/// text, and it draws one letter of the run at a time. A letter whose capital
/// is two letters gives two.
#[must_use]
pub fn upper_char(character: char, tailoring: Tailoring) -> Vec<char> {
    match tailoring {
        Tailoring::Turkic => turkic_upper(character),
        Tailoring::Greek => match greek_capital(character) {
            Some(capital) => vec![capital],
            None if matches!(character, '\u{0300}' | '\u{0301}' | '\u{0342}' | '\u{0345}') => {
                Vec::new()
            }
            None => character.to_uppercase().collect(),
        },
        // The dot over an i is dropped in capitals, and a caller working one
        // letter at a time cannot see what came before it: the dot on its own
        // is what is dropped.
        Tailoring::Lithuanian if character == DOT_ABOVE => Vec::new(),
        Tailoring::Lithuanian | Tailoring::Default => character.to_uppercase().collect(),
    }
}

/// The capital of a letter in Turkish: the dotted i keeps its dot and the
/// dotless one stays dotless.
fn turkic_upper(character: char) -> Vec<char> {
    match character {
        'i' => vec!['\u{0130}'],
        '\u{0131}' => vec!['I'],
        _ => character.to_uppercase().collect(),
    }
}

/// Whether a mark is one of the accents written above a letter.
fn is_accent_above(character: char) -> bool {
    matches!(character as u32, 0x0300..=0x0314 | 0x033D..=0x0344 | 0x0346..=0x034A)
}

/// The capital of a Greek letter that carries an accent, which in capitals it
/// does not.
///
/// Written out rather than worked out: the accented letters are a closed list,
/// and the rule — drop the accent, keep the dialytika — is a rule about how
/// Greek is written rather than anything the letters themselves say.
fn greek_capital(character: char) -> Option<char> {
    Some(match character {
        '\u{03AC}' | '\u{0386}' => '\u{0391}', // alpha
        '\u{03AD}' | '\u{0388}' => '\u{0395}', // epsilon
        '\u{03AE}' | '\u{0389}' => '\u{0397}', // eta
        '\u{03AF}' | '\u{038A}' => '\u{0399}', // iota
        '\u{03CC}' | '\u{038C}' => '\u{039F}', // omicron
        '\u{03CD}' | '\u{038E}' => '\u{03A5}', // upsilon
        '\u{03CE}' | '\u{038F}' => '\u{03A9}', // omega
        // The two that carry an accent and a dialytika: the accent goes and
        // the dialytika stays.
        '\u{0390}' | '\u{03CA}' => '\u{03AA}', // iota with dialytika
        '\u{03B0}' | '\u{03CB}' => '\u{03AB}', // upsilon with dialytika
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tag_chooses_the_rules_its_language_is_written_by() {
        assert_eq!(Tailoring::of("tr-TR"), Tailoring::Turkic);
        assert_eq!(Tailoring::of("az-Latn-AZ"), Tailoring::Turkic);
        assert_eq!(Tailoring::of("lt-LT"), Tailoring::Lithuanian);
        assert_eq!(Tailoring::of("el-GR"), Tailoring::Greek);
        assert_eq!(Tailoring::of("en-GB"), Tailoring::Default);
        assert_eq!(Tailoring::of(""), Tailoring::Default);
    }

    #[test]
    fn the_capital_of_i_depends_on_the_language() {
        // The one everybody knows: a Turkish i keeps its dot, and an English
        // one does not.
        assert_eq!(upper("i", Tailoring::Turkic), "\u{0130}");
        assert_eq!(upper("i", Tailoring::Default), "I");
        assert_eq!(upper("\u{0131}", Tailoring::Turkic), "I");
    }

    #[test]
    fn and_so_does_the_small_letter_of_i() {
        assert_eq!(lower("I", Tailoring::Turkic), "\u{0131}");
        assert_eq!(lower("I", Tailoring::Default), "i");
        assert_eq!(lower("\u{0130}", Tailoring::Turkic), "i");
    }

    #[test]
    fn a_turkish_word_keeps_its_meaning_in_capitals() {
        // "istanbul" is a place; "ISTANBUL" is not how it is written there.
        assert_eq!(upper("istanbul", Tailoring::Turkic), "\u{0130}STANBUL");
        assert_eq!(lower("ISTANBUL", Tailoring::Turkic), "\u{0131}stanbul");
    }

    #[test]
    fn a_dot_written_as_a_mark_of_its_own_is_the_same_letter() {
        // "I" with a dot over it is how "İ" is spelt the long way, and both
        // become the dotted small letter.
        let spelt_out = format!("I{DOT_ABOVE}");
        assert_eq!(lower(&spelt_out, Tailoring::Turkic), "i");
    }

    #[test]
    fn the_capital_of_the_sharp_s_is_two_letters() {
        // Which is the standard's rule and not any language's, so it holds
        // whatever the document says it is written in.
        for tailoring in [Tailoring::Default, Tailoring::Turkic, Tailoring::Greek] {
            assert_eq!(upper("stra\u{00DF}e", tailoring), "STRASSE");
        }
    }

    #[test]
    fn a_sigma_at_the_end_of_a_word_is_written_the_way_it_is_written_there() {
        // The final sigma: ΟΔΟΣ is οδός, with the last sigma written ς.
        assert_eq!(
            lower("\u{039F}\u{0394}\u{039F}\u{03A3}", Tailoring::Greek),
            "\u{03BF}\u{03B4}\u{03BF}\u{03C2}"
        );
        // And in the middle of a word it is the ordinary one.
        assert_eq!(lower("\u{03A3}\u{03A9}", Tailoring::Greek), "\u{03C3}\u{03C9}");
    }

    #[test]
    fn greek_drops_its_accents_in_capitals() {
        // άνθρωπος is ΑΝΘΡΩΠΟΣ: the accent belongs to the small letters.
        let word = "\u{03AC}\u{03BD}\u{03B8}\u{03C1}\u{03C9}\u{03C0}\u{03BF}\u{03C2}";
        assert_eq!(
            upper(word, Tailoring::Greek),
            "\u{0391}\u{039D}\u{0398}\u{03A1}\u{03A9}\u{03A0}\u{039F}\u{03A3}"
        );
        // And an accent written as a mark of its own goes the same way.
        assert_eq!(upper("\u{03B1}\u{0301}", Tailoring::Greek), "\u{0391}");
    }

    #[test]
    fn the_dialytika_is_not_an_accent_and_stays() {
        // It says two vowels are read apart, which is as true in capitals.
        assert_eq!(upper("\u{03CA}", Tailoring::Greek), "\u{03AA}");
        assert_eq!(upper("\u{0390}", Tailoring::Greek), "\u{03AA}");
    }

    #[test]
    fn greek_in_another_language_is_left_as_the_standard_says() {
        // A Greek word in an English document: the standard's answer, accents
        // and all, because nothing said it was Greek.
        let word = "\u{03AC}";
        assert_eq!(upper(word, Tailoring::Default), "\u{0386}");
    }

    #[test]
    fn lithuanian_keeps_the_dot_on_an_i_under_an_accent() {
        // Everywhere else the dot goes when the accent arrives; here it does
        // not, and the dot is written as a mark of its own.
        let with_accent = "I\u{0301}";
        assert_eq!(lower(with_accent, Tailoring::Lithuanian), format!("i{DOT_ABOVE}\u{0301}"));
        // And in capitals the dot goes again.
        let spelt = format!("i{DOT_ABOVE}\u{0301}");
        assert_eq!(upper(&spelt, Tailoring::Lithuanian), "I\u{0301}");
    }

    #[test]
    fn a_letter_at_a_time_says_the_same_as_a_word_at_a_time() {
        // Which is what drawing needs: `w:caps` draws a capital one letter at
        // a time, and it must agree with what Change Case would write.
        for (text, tailoring) in [
            ("i", Tailoring::Turkic),
            ("\u{0131}", Tailoring::Turkic),
            ("\u{00DF}", Tailoring::Default),
            ("\u{03AC}", Tailoring::Greek),
            ("q", Tailoring::Default),
        ] {
            let character = text.chars().next().expect("a letter");
            let one_at_a_time: String = upper_char(character, tailoring).into_iter().collect();
            assert_eq!(one_at_a_time, upper(text, tailoring), "{text:?}");
        }
    }

    #[test]
    fn what_has_no_case_is_left_alone() {
        for tailoring in [Tailoring::Default, Tailoring::Turkic, Tailoring::Greek] {
            assert_eq!(upper("1 \u{6F22}, ", tailoring), "1 \u{6F22}, ");
            assert_eq!(lower("1 \u{6F22}, ", tailoring), "1 \u{6F22}, ");
        }
    }
}
