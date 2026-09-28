//! The rules for English.

use crate::{Piece, Rule};

/// The pronouns that take a verb in the singular.
const HE_SHE_IT: &[&str] = &["he", "she", "it"];
/// And those that take it in the plural.
const THEY_WE_YOU: &[&str] = &["they", "we", "you"];
/// A verb with "not" folded into it.
const NEGATIVE_VERBS: &[&str] = &[
    "don't",
    "doesn't",
    "didn't",
    "can't",
    "cannot",
    "couldn't",
    "won't",
    "wouldn't",
    "shouldn't",
    "isn't",
    "aren't",
    "wasn't",
    "weren't",
    "haven't",
    "hasn't",
    "hadn't",
    "ain't",
];
/// The words that make a negative a double one.
const NEGATIVE_WORDS: &[&str] = &["no", "nothing", "nobody", "nowhere", "never", "none", "neither"];
/// Verbs a "have" gets misheard after.
const MODALS: &[&str] = &["could", "should", "would", "might", "must", "may"];

pub(crate) const RULES: &[Rule] = &[
    // --- Article use -------------------------------------------------------
    Rule {
        pattern: &[Piece::Word("a"), Piece::VowelSound],
        category: "Article use",
        replacement: Some("an $2"),
        example: "I ate a apple.",
    },
    Rule {
        pattern: &[Piece::Word("an"), Piece::ConsonantSound],
        category: "Article use",
        replacement: Some("a $2"),
        example: "They live in an house.",
    },
    // --- Verb form ---------------------------------------------------------
    // Before "could of" and its kin, which would otherwise speak first and
    // leave "went" where "gone" belongs.
    Rule {
        pattern: &[Piece::Word("should"), Piece::Word("of"), Piece::Word("went")],
        category: "Verb form",
        replacement: Some("should have gone"),
        example: "We should of went home.",
    },
    Rule {
        pattern: &[Piece::AnyOf(MODALS), Piece::Word("of")],
        category: "Verb form",
        replacement: Some("$1 have"),
        example: "I could of gone.",
    },
    Rule {
        pattern: &[Piece::AnyOf(HE_SHE_IT), Piece::Word("don't")],
        category: "Subject-verb agreement",
        replacement: Some("$1 doesn't"),
        example: "He don't care.",
    },
    Rule {
        pattern: &[Piece::AnyOf(HE_SHE_IT), Piece::Word("have")],
        category: "Subject-verb agreement",
        replacement: Some("$1 has"),
        example: "She have a car.",
    },
    Rule {
        pattern: &[Piece::AnyOf(HE_SHE_IT), Piece::Word("were")],
        category: "Subject-verb agreement",
        replacement: Some("$1 was"),
        example: "It were late.",
    },
    Rule {
        pattern: &[Piece::AnyOf(THEY_WE_YOU), Piece::Word("was")],
        category: "Subject-verb agreement",
        replacement: Some("$1 were"),
        example: "They was there.",
    },
    Rule {
        pattern: &[Piece::AnyOf(THEY_WE_YOU), Piece::Word("is")],
        category: "Subject-verb agreement",
        replacement: Some("$1 are"),
        example: "We is ready.",
    },
    Rule {
        pattern: &[Piece::AnyOf(THEY_WE_YOU), Piece::Word("has")],
        category: "Subject-verb agreement",
        replacement: Some("$1 have"),
        example: "They has left.",
    },
    Rule {
        pattern: &[Piece::AnyOf(THEY_WE_YOU), Piece::Word("doesn't")],
        category: "Subject-verb agreement",
        replacement: Some("$1 don't"),
        example: "You doesn't know.",
    },
    Rule {
        pattern: &[Piece::Word("i"), Piece::Word("is")],
        category: "Subject-verb agreement",
        replacement: Some("I am"),
        example: "Then I is late.",
    },
    // --- Double negation ---------------------------------------------------
    Rule {
        pattern: &[Piece::AnyOf(NEGATIVE_VERBS), Piece::AnyOf(NEGATIVE_WORDS)],
        category: "Double negation",
        replacement: None,
        example: "It isn't nothing.",
    },
    Rule {
        pattern: &[Piece::AnyOf(NEGATIVE_VERBS), Piece::Any, Piece::AnyOf(NEGATIVE_WORDS)],
        category: "Double negation",
        replacement: None,
        example: "I don't know nothing.",
    },
    // --- Commonly confused words -------------------------------------------
    Rule {
        pattern: &[Piece::Word("alot")],
        category: "Commonly confused words",
        replacement: Some("a lot"),
        example: "There were alot of them.",
    },
    Rule {
        pattern: &[Piece::Word("their"), Piece::AnyOf(&["is", "are", "was", "were"])],
        category: "Commonly confused words",
        replacement: Some("there $2"),
        example: "Their is a way.",
    },
    Rule {
        pattern: &[Piece::Word("your"), Piece::Word("welcome")],
        category: "Commonly confused words",
        replacement: Some("you're welcome"),
        example: "Your welcome.",
    },
    Rule {
        pattern: &[
            Piece::AnyOf(&[
                "better", "worse", "more", "less", "rather", "other", "bigger", "smaller", "fewer",
                "greater", "higher", "lower", "later", "earlier", "faster", "slower", "longer",
                "shorter", "larger",
            ]),
            Piece::Word("then"),
        ],
        category: "Commonly confused words",
        replacement: Some("$1 than"),
        example: "It is better then that.",
    },
    Rule {
        pattern: &[Piece::Word("irregardless")],
        category: "Word choice",
        replacement: Some("regardless"),
        example: "Irregardless of that, we went.",
    },
    Rule {
        pattern: &[
            Piece::Word("for"),
            Piece::Word("all"),
            Piece::Word("intensive"),
            Piece::Word("purposes"),
        ],
        category: "Commonly confused words",
        replacement: Some("for all intents and purposes"),
        example: "It is finished for all intensive purposes.",
    },
    Rule {
        pattern: &[Piece::Word("could"), Piece::Word("care"), Piece::Word("less")],
        category: "Commonly confused words",
        replacement: Some("couldn't care less"),
        example: "I could care less.",
    },
    // --- Capitalization ----------------------------------------------------
    Rule {
        pattern: &[Piece::Word("i")],
        category: "Capitalization",
        replacement: Some("I"),
        example: "And then i left.",
    },
];
