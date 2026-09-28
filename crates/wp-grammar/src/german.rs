//! The rules for German.
//!
//! As narrow as the mistakes: "seid" is a verb in "ihr seid dem Ziel nahe"
//! and a mistake in "seid Jahren", and "ein Paar" is right before shoes and
//! wrong before days, so each rule says which words make it one.

use crate::{Piece, Rule};

/// What "seit" goes before and "seid", the verb, hardly ever does: spans of
/// time. Not "heute" or "gestern", which "ihr seid" goes before as often.
const SINCE: &[&str] = &[
    "jahren",
    "jahrzehnten",
    "monaten",
    "wochen",
    "tagen",
    "stunden",
    "minuten",
    "langem",
    "kurzem",
    "jeher",
    "ewigkeiten",
    "wann",
];

/// The comparatives that take "als" and not "wie". Not "mehr", "früher" or
/// "lieber", which go before "wie" in "nicht mehr wie früher" and are right.
const COMPARATIVES: &[&str] = &[
    "besser",
    "schlechter",
    "größer",
    "kleiner",
    "weniger",
    "älter",
    "jünger",
    "schneller",
    "langsamer",
    "höher",
    "tiefer",
    "länger",
    "kürzer",
    "stärker",
    "schwächer",
    "teurer",
    "billiger",
    "schöner",
    "einfacher",
    "schwieriger",
    "leichter",
    "wichtiger",
    "anders",
];

/// A superlative of a word that has none: "einzig" is already the only one.
const EINZIGSTE: &[(&str, &str)] = &[
    ("einzigste", "einzige"),
    ("einzigster", "einziger"),
    ("einzigstes", "einziges"),
    ("einzigsten", "einzigen"),
    ("einzigstem", "einzigem"),
];

/// What "irgend" is written together with.
const IRGEND: &[&str] = &[
    "etwas", "jemand", "jemanden", "jemandem", "ein", "eine", "einer", "einen", "einem", "eines",
    "welche", "welcher", "welches", "welchen", "welchem", "wer", "was", "wo", "wohin", "woher",
    "wie", "wann", "wieso", "womit", "wozu",
];

/// What "ein paar" — a few — goes before, where "ein Paar" is a pair and
/// wrong: "ein paar Tage", but "ein Paar Schuhe".
const A_FEW: &[&str] = &[
    "sekunden", "minuten", "stunden", "tage", "tagen", "wochen", "monate", "monaten", "jahre",
    "jahren", "mal", "leute", "leuten", "euro", "worte", "wörter", "zeilen", "seiten", "fragen",
];

pub(crate) const RULES: &[Rule] = &[
    // --- Commonly confused words -------------------------------------------
    Rule {
        pattern: &[Piece::Word("seid"), Piece::AnyOf(SINCE)],
        category: "Commonly confused words",
        replacement: Some("seit $2"),
        example: "Wir warten seid Jahren darauf.",
    },
    Rule {
        pattern: &[Piece::Word("wieder"), Piece::Word("willen")],
        category: "Commonly confused words",
        replacement: Some("wider $2"),
        example: "Er tat es wieder Willen.",
    },
    Rule {
        pattern: &[Piece::Word("wieder"), Piece::Word("besseres"), Piece::Word("wissen")],
        category: "Commonly confused words",
        replacement: Some("wider $2 $3"),
        example: "Sie schwieg wieder besseres Wissen.",
    },
    // --- Comparisons -------------------------------------------------------
    Rule {
        pattern: &[Piece::AnyOf(COMPARATIVES), Piece::Word("wie")],
        category: "Comparisons",
        replacement: Some("$1 als"),
        example: "Das ist besser wie nichts.",
    },
    Rule {
        pattern: &[Piece::Swap(EINZIGSTE)],
        category: "Comparisons",
        replacement: Some("%1"),
        example: "Das war die einzigste Möglichkeit.",
    },
    // --- Words split -------------------------------------------------------
    Rule {
        pattern: &[
            Piece::AnyOf(&["der", "die", "das", "den", "dem", "des"]),
            Piece::AnyOf(&["selbe", "selben"]),
        ],
        category: "Words split",
        replacement: Some("$1%2"),
        example: "Wir haben die selbe Idee.",
    },
    Rule {
        pattern: &[Piece::Word("nichts"), Piece::Word("desto"), Piece::Word("trotz")],
        category: "Words split",
        replacement: Some("nichtsdestotrotz"),
        example: "Es regnete, nichts desto trotz gingen wir.",
    },
    Rule {
        pattern: &[Piece::Word("nichts"), Piece::Word("desto"), Piece::Word("weniger")],
        category: "Words split",
        replacement: Some("nichtsdestoweniger"),
        example: "Es war spät, nichts desto weniger blieben wir.",
    },
    Rule {
        pattern: &[Piece::Word("irgend"), Piece::AnyOf(IRGEND)],
        category: "Words split",
        replacement: Some("$1%2"),
        example: "Hat irgend jemand angerufen?",
    },
    // --- Capitalization ----------------------------------------------------
    Rule {
        pattern: &[Piece::Word("ein"), Piece::Word("paar"), Piece::AnyOf(A_FEW)],
        category: "Capitalization",
        replacement: Some("$1 paar $3"),
        example: "Wir bleiben noch ein Paar Tage.",
    },
];
