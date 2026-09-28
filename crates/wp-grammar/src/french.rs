//! The rules for French.
//!
//! Most of them are the elision: French writes "d’abord" and "j’ai", and a
//! "de" or a "je" left whole before a vowel is a mistake in every register.
//! The rest are the handful of pairs a French reader stops at.

use crate::{Piece, Rule};

/// The small words that lose their vowel before one, and what each becomes.
///
/// Not "si", which only does before "il" and "ils" and has a rule of its own;
/// not "ce", which becomes "cet" before a noun and "c’" only before "est" and
/// its tenses; not "la" and "le" before "une" and "un", which are right in "à
/// la une".
const ELIDING: &[(&str, &str)] = &[
    ("de", "d\u{2019}"),
    ("le", "l\u{2019}"),
    ("la", "l\u{2019}"),
    ("je", "j\u{2019}"),
    ("me", "m\u{2019}"),
    ("te", "t\u{2019}"),
    ("se", "s\u{2019}"),
    ("ne", "n\u{2019}"),
    ("que", "qu\u{2019}"),
    ("jusque", "jusqu\u{2019}"),
    ("lorsque", "lorsqu\u{2019}"),
    ("puisque", "puisqu\u{2019}"),
    ("quoique", "quoiqu\u{2019}"),
];

/// Whether a word begins with the vowel an elision is made before.
///
/// Not an h, which is silent in "l’homme" and sounded in "le héros", and
/// which no rule short of a dictionary can tell apart. Not the words that
/// begin with a vowel and take no elision — "le onze", "le oui" — nor "un"
/// and "une", which are right after "le" and "la" as often as not. Not a word
/// in capitals, which is a letter named or an initialism: "de A à Z".
fn begins_with_vowel(word: &str) -> bool {
    const NONE_BEFORE: &[&str] =
        &["onze", "onzième", "onzièmes", "oui", "ouis", "ouistiti", "ouistitis", "un", "une"];
    let lower = word.to_lowercase();
    if word.chars().all(|character| !character.is_lowercase()) || NONE_BEFORE.contains(&&*lower) {
        return false;
    }
    lower.chars().next().is_some_and(|first| "aeiouéèêëàâäîïôöûùüœæ".contains(first))
}

pub(crate) const RULES: &[Rule] = &[
    // --- Elision -----------------------------------------------------------
    Rule {
        pattern: &[Piece::Swap(ELIDING), Piece::Test(begins_with_vowel)],
        category: "Elision",
        replacement: Some("%1$2"),
        example: "Je ai faim.",
    },
    Rule {
        pattern: &[Piece::Word("si"), Piece::AnyOf(&["il", "ils"])],
        category: "Elision",
        replacement: Some("s\u{2019}$2"),
        example: "Je viendrai si il fait beau.",
    },
    Rule {
        pattern: &[Piece::Word("ce"), Piece::AnyOf(&["est", "était", "étaient"])],
        category: "Elision",
        replacement: Some("c\u{2019}$2"),
        example: "Ce est vrai.",
    },
    Rule {
        pattern: &[Piece::Word("quelque"), Piece::AnyOf(&["un", "une", "uns", "unes"])],
        category: "Elision",
        replacement: Some("quelqu\u{2019}$2"),
        example: "Il y a quelque un à la porte.",
    },
    // --- Commonly confused words -------------------------------------------
    Rule {
        pattern: &[
            Piece::Word("quand"),
            Piece::Word("à"),
            Piece::AnyOf(&[
                "moi", "toi", "lui", "elle", "nous", "vous", "eux", "elles", "cela", "ça",
            ]),
        ],
        category: "Commonly confused words",
        replacement: Some("quant à $3"),
        example: "Quand à moi, je reste.",
    },
    Rule {
        pattern: &[Piece::Word("sa"), Piece::Word("va")],
        category: "Commonly confused words",
        replacement: Some("ça va"),
        example: "Sa va bien.",
    },
    Rule {
        // "Quel que soit", "quelle que soit", "quels que soient": which of
        // them is for the noun that follows to say, so none is offered.
        pattern: &[Piece::Word("quelque"), Piece::AnyOf(&["soit", "soient"])],
        category: "Commonly confused words",
        replacement: None,
        example: "Quelque soit le temps, nous partirons.",
    },
    // --- Comparisons -------------------------------------------------------
    Rule {
        pattern: &[
            Piece::AnyOf(&["le", "la", "les"]),
            Piece::Word("plus"),
            Piece::AnyOf(&["pire", "pires", "meilleur", "meilleure", "meilleurs", "meilleures"]),
        ],
        category: "Comparisons",
        replacement: Some("$1 $3"),
        example: "C’est le plus meilleur gâteau.",
    },
    // --- Word choice -------------------------------------------------------
    Rule {
        pattern: &[
            Piece::Word("malgré"),
            Piece::AnyOf(&["que", "qu'il", "qu'ils", "qu'elle", "qu'elles", "qu'on"]),
        ],
        category: "Word choice",
        replacement: Some("bien $2"),
        example: "Il est sorti malgré qu’il pleuve.",
    },
    Rule {
        pattern: &[
            Piece::AnyOf(&[
                "pallier",
                "pallie",
                "pallies",
                "pallions",
                "palliez",
                "pallient",
                "palliait",
                "palliaient",
                "pallié",
                "palliera",
                "pallierait",
            ]),
            Piece::Word("à"),
        ],
        category: "Word choice",
        replacement: Some("$1"),
        example: "Il faut pallier à ce problème.",
    },
    Rule {
        pattern: &[Piece::Word("voire"), Piece::Word("même")],
        category: "Word choice",
        replacement: Some("voire"),
        example: "C’est difficile, voire même impossible.",
    },
    Rule {
        pattern: &[Piece::Word("au"), Piece::Word("jour"), Piece::Word("d'aujourd'hui")],
        category: "Word choice",
        replacement: Some("aujourd\u{2019}hui"),
        example: "Au jour d’aujourd’hui, tout a changé.",
    },
];
