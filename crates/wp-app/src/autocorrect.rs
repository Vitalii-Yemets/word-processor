//! Correcting what was typed, as it is typed.
//!
//! # One mechanism, several tables
//!
//! Almost everything Word's AutoCorrect does is the same thing: watch the word
//! that was just finished, and if it is one of a known set, put something else
//! in its place. The replacement list is the obvious case — "teh" becomes
//! "the" — and so are the capital letters: a word typed as "TWo" is a word
//! whose first two letters are capitals, and the rule replaces it with "Two".
//!
//! What is not a word replacement is a character replacement: a straight quote
//! becomes a curly one the instant it is typed, because which way it curls
//! depends on what is in front of it and that is known at once. A hyphen
//! between two words becomes a dash, and "1st" becomes "1ˢᵗ".
//!
//! # Why the list is ours and not Word's
//!
//! Word ships about nine hundred replacements in a file of its own. That list
//! is Microsoft's; this one is written here, and is the handful of misspellings
//! that are worth catching without being wrong about anybody's name. Anyone can
//! add to it, and what they add is kept in the settings file beside the program
//! — it is about the person, not about the document.

use std::collections::{BTreeMap, BTreeSet};

/// The days, which Word capitalises whatever they are typed as.
const DAYS: &[&str] = &[
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
    "january",
    "february",
    "march",
    "april",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
];

/// The replacements a new settings file starts with.
///
/// Common misspellings of common words, and nothing whose "correction" could be
/// somebody's name or a word in another language: a program that quietly
/// rewrote a surname would be worse than one that corrected nothing.
const USUAL: &[(&str, &str)] = &[
    ("teh", "the"),
    ("adn", "and"),
    ("taht", "that"),
    ("thier", "their"),
    ("recieve", "receive"),
    ("seperate", "separate"),
    ("definately", "definitely"),
    ("occured", "occurred"),
    ("untill", "until"),
    ("wich", "which"),
    ("becuase", "because"),
    ("tommorow", "tomorrow"),
    ("alot", "a lot"),
    ("dont", "don't"),
    ("cant", "can't"),
    ("wont", "won't"),
    ("isnt", "isn't"),
    ("wasnt", "wasn't"),
    ("didnt", "didn't"),
    ("youre", "you're"),
    ("theyre", "they're"),
    ("its a", "it's a"),
];

/// The names Math AutoCorrect starts with: `\alpha` for α and the rest.
///
/// Word's list is the names the linear format of its equation editor takes,
/// which are TeX's: the Greek alphabet, with `\epsilon`, `\phi`, `\theta`,
/// `\rho`, `\sigma` and `\pi` each beside a `\var` twin, the relations, the
/// operators, the arrows, the big operators and the odd letters mathematics
/// borrows. Not the whole of Word's list, which is some hundreds long and
/// runs to the accents and the double-struck alphabets; these are the ones a
/// person writing mathematics reaches for, and any other can be added.
const MATH_USUAL: &[(&str, &str)] = &[
    // The Greek alphabet.
    ("\\alpha", "\u{3B1}"),
    ("\\beta", "\u{3B2}"),
    ("\\gamma", "\u{3B3}"),
    ("\\delta", "\u{3B4}"),
    ("\\epsilon", "\u{3F5}"),
    ("\\varepsilon", "\u{3B5}"),
    ("\\zeta", "\u{3B6}"),
    ("\\eta", "\u{3B7}"),
    ("\\theta", "\u{3B8}"),
    ("\\vartheta", "\u{3D1}"),
    ("\\iota", "\u{3B9}"),
    ("\\kappa", "\u{3BA}"),
    ("\\lambda", "\u{3BB}"),
    ("\\mu", "\u{3BC}"),
    ("\\nu", "\u{3BD}"),
    ("\\xi", "\u{3BE}"),
    ("\\pi", "\u{3C0}"),
    ("\\varpi", "\u{3D6}"),
    ("\\rho", "\u{3C1}"),
    ("\\varrho", "\u{3F1}"),
    ("\\sigma", "\u{3C3}"),
    ("\\varsigma", "\u{3C2}"),
    ("\\tau", "\u{3C4}"),
    ("\\upsilon", "\u{3C5}"),
    ("\\phi", "\u{3D5}"),
    ("\\varphi", "\u{3C6}"),
    ("\\chi", "\u{3C7}"),
    ("\\psi", "\u{3C8}"),
    ("\\omega", "\u{3C9}"),
    ("\\Alpha", "\u{391}"),
    ("\\Beta", "\u{392}"),
    ("\\Gamma", "\u{393}"),
    ("\\Delta", "\u{394}"),
    ("\\Epsilon", "\u{395}"),
    ("\\Zeta", "\u{396}"),
    ("\\Eta", "\u{397}"),
    ("\\Theta", "\u{398}"),
    ("\\Iota", "\u{399}"),
    ("\\Kappa", "\u{39A}"),
    ("\\Lambda", "\u{39B}"),
    ("\\Mu", "\u{39C}"),
    ("\\Nu", "\u{39D}"),
    ("\\Xi", "\u{39E}"),
    ("\\Pi", "\u{3A0}"),
    ("\\Rho", "\u{3A1}"),
    ("\\Sigma", "\u{3A3}"),
    ("\\Tau", "\u{3A4}"),
    ("\\Upsilon", "\u{3A5}"),
    ("\\Phi", "\u{3A6}"),
    ("\\Chi", "\u{3A7}"),
    ("\\Psi", "\u{3A8}"),
    ("\\Omega", "\u{3A9}"),
    // The operators.
    ("\\times", "\u{D7}"),
    ("\\div", "\u{F7}"),
    ("\\pm", "\u{B1}"),
    ("\\mp", "\u{2213}"),
    ("\\cdot", "\u{22C5}"),
    ("\\ast", "\u{2217}"),
    ("\\star", "\u{22C6}"),
    ("\\circ", "\u{2218}"),
    ("\\bullet", "\u{2219}"),
    ("\\oplus", "\u{2295}"),
    ("\\ominus", "\u{2296}"),
    ("\\otimes", "\u{2297}"),
    ("\\oslash", "\u{2298}"),
    ("\\odot", "\u{2299}"),
    ("\\cup", "\u{222A}"),
    ("\\cap", "\u{2229}"),
    ("\\wedge", "\u{2227}"),
    ("\\vee", "\u{2228}"),
    ("\\neg", "\u{AC}"),
    // The relations.
    ("\\le", "\u{2264}"),
    ("\\leq", "\u{2264}"),
    ("\\ge", "\u{2265}"),
    ("\\geq", "\u{2265}"),
    ("\\ne", "\u{2260}"),
    ("\\neq", "\u{2260}"),
    ("\\approx", "\u{2248}"),
    ("\\equiv", "\u{2261}"),
    ("\\sim", "\u{223C}"),
    ("\\simeq", "\u{2243}"),
    ("\\cong", "\u{2245}"),
    ("\\propto", "\u{221D}"),
    ("\\ll", "\u{226A}"),
    ("\\gg", "\u{226B}"),
    ("\\subset", "\u{2282}"),
    ("\\supset", "\u{2283}"),
    ("\\subseteq", "\u{2286}"),
    ("\\supseteq", "\u{2287}"),
    ("\\in", "\u{2208}"),
    ("\\ni", "\u{220B}"),
    ("\\perp", "\u{22A5}"),
    ("\\parallel", "\u{2225}"),
    // The arrows.
    ("\\rightarrow", "\u{2192}"),
    ("\\to", "\u{2192}"),
    ("\\leftarrow", "\u{2190}"),
    ("\\uparrow", "\u{2191}"),
    ("\\downarrow", "\u{2193}"),
    ("\\leftrightarrow", "\u{2194}"),
    ("\\updownarrow", "\u{2195}"),
    ("\\Rightarrow", "\u{21D2}"),
    ("\\Leftarrow", "\u{21D0}"),
    ("\\Leftrightarrow", "\u{21D4}"),
    ("\\mapsto", "\u{21A6}"),
    // The big operators, and the roots.
    ("\\sum", "\u{2211}"),
    ("\\prod", "\u{220F}"),
    ("\\coprod", "\u{2210}"),
    ("\\int", "\u{222B}"),
    ("\\iint", "\u{222C}"),
    ("\\iiint", "\u{222D}"),
    ("\\oint", "\u{222E}"),
    ("\\sqrt", "\u{221A}"),
    ("\\cbrt", "\u{221B}"),
    ("\\qdrt", "\u{221C}"),
    // The rest of what mathematics writes with.
    ("\\infty", "\u{221E}"),
    ("\\partial", "\u{2202}"),
    ("\\nabla", "\u{2207}"),
    ("\\forall", "\u{2200}"),
    ("\\exists", "\u{2203}"),
    ("\\emptyset", "\u{2205}"),
    ("\\therefore", "\u{2234}"),
    ("\\because", "\u{2235}"),
    ("\\angle", "\u{2220}"),
    ("\\degree", "\u{B0}"),
    ("\\degc", "\u{2103}"),
    ("\\degf", "\u{2109}"),
    ("\\prime", "\u{2032}"),
    ("\\hbar", "\u{210F}"),
    ("\\ell", "\u{2113}"),
    ("\\Re", "\u{211C}"),
    ("\\Im", "\u{2111}"),
    ("\\aleph", "\u{2135}"),
    ("\\wp", "\u{2118}"),
    ("\\dots", "\u{2026}"),
    ("\\ldots", "\u{2026}"),
    ("\\cdots", "\u{22EF}"),
    ("\\vdots", "\u{22EE}"),
    ("\\ddots", "\u{22F1}"),
    ("\\langle", "\u{27E8}"),
    ("\\rangle", "\u{27E9}"),
    ("\\lfloor", "\u{230A}"),
    ("\\rfloor", "\u{230B}"),
    ("\\lceil", "\u{2308}"),
    ("\\rceil", "\u{2309}"),
    ("\\Vert", "\u{2016}"),
];

/// The abbreviations a full stop does not end a sentence after.
///
/// Without these, "see e.g. the table" becomes "see e.g. The table", because a
/// full stop and a space is what the end of a sentence looks like. Word keeps
/// the same list, under Exceptions ▸ First Letter, and lets it be added to.
const ABBREVIATIONS: &[&str] = &[
    "al.", "apr.", "assn.", "aug.", "co.", "corp.", "dec.", "dept.", "dr.", "e.g.", "eq.", "etc.",
    "feb.", "fig.", "figs.", "i.e.", "inc.", "jan.", "jr.", "jul.", "jun.", "ltd.", "mar.", "mr.",
    "mrs.", "ms.", "mt.", "no.", "nov.", "oct.", "p.", "pp.", "prof.", "sept.", "sr.", "st.",
    "vol.", "vs.",
];

/// The words whose first two capitals are meant.
///
/// "CDs" is not a finger left on the shift key. Word keeps this list too, under
/// Exceptions ▸ INitial CAps. A word of three capitals or more is left alone by
/// the rule itself and needs no exception.
const TWO_CAPITALS: &[&str] = &["CDs", "GBs", "IDs", "KBs", "MBs", "PCs", "TVs"];

/// The fractions that have a character of their own.
const FRACTIONS: &[(&str, char)] =
    &[("1/2", '½'), ("1/4", '¼'), ("3/4", '¾'), ("1/3", '⅓'), ("2/3", '⅔')];

/// Which corrections are made.
///
/// Word's tabs, less the two that are about things this program does not
/// have: Math AutoCorrect (which waits for the equation editor) and Actions
/// (which offers to look a name up in an address book).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutoCorrect {
    /// What is typed, and what goes in its place.
    pub replacements: BTreeMap<String, String>,
    /// The abbreviations a sentence does not begin after. Word's Exceptions ▸
    /// First Letter.
    pub first_letter: BTreeSet<String>,
    /// The words allowed to begin with two capitals. Word's Exceptions ▸
    /// INitial CAps.
    pub initial_caps: BTreeSet<String>,

    // --- The AutoCorrect tab ------------------------------------------------
    /// TWo initial capitals become one.
    pub two_initials: bool,
    /// The first letter of a sentence is capitalised.
    pub sentence_case: bool,
    /// The days of the week and the months are capitalised.
    pub day_names: bool,
    /// A word typed with the shift key and the caps lock both on — `tHE` — is
    /// turned back the right way up.
    pub caps_lock: bool,
    /// Whether the replacement list is used at all.
    pub replace_text: bool,

    // --- The AutoFormat As You Type tab -------------------------------------
    /// Straight quotes become curly ones, curling the way the text around them
    /// asks for.
    pub curly_quotes: bool,
    /// `1st` becomes `1ˢᵗ`.
    pub ordinals: bool,
    /// `1/2` becomes `½`.
    pub fractions: bool,
    /// A hyphen between two words becomes a dash.
    pub dashes: bool,
    /// A line begun with `- ` or `1. ` becomes a list.
    pub automatic_lists: bool,
    /// `*bold*` and `_italic_` become bold and italic, and lose their marks.
    pub bold_italic: bool,
    /// An address typed in — `www.example.com`, `https://…` — becomes a link.
    pub hyperlinks: bool,
    /// Three hyphens on a line of their own become a line under the paragraph
    /// above.
    pub border_lines: bool,
    /// `+---+---+` and Enter become a table, one column for each stretch of
    /// hyphens.
    pub tables: bool,
    /// A short line with no full stop, and Enter twice, becomes a heading.
    /// Word ships it switched off, and so does this.
    pub headings: bool,

    // --- The Exceptions dialog ------------------------------------------
    /// A word a person undoes the capitalising of straight after goes on the
    /// First Letter list, so it is not capitalised after again.
    pub add_first_letter_exceptions: bool,
    /// And the same for a word whose two capitals were undone.
    pub add_initial_caps_exceptions: bool,

    /// The AutoFormat tab: what reformatting a whole document at once
    /// changes, which Word keeps apart from what happens as one types.
    pub reformat: Reformat,

    // --- The Math AutoCorrect tab --------------------------------------------
    /// What is typed — `\alpha` — and the character that goes in its place.
    pub math: BTreeMap<String, String>,
    /// Whether the list is used at all.
    pub math_replace: bool,
    /// Whether it is used in the text as well as in an equation. Word ships
    /// it switched off: `\alpha` in a paragraph about LaTeX is meant.
    pub math_outside: bool,

    /// The Actions tab: what is recognised in the text for the right-click
    /// menu to offer something for.
    pub actions: crate::actions::Recognisers,
}

/// Word's AutoFormat tab: what the AutoFormat command changes when it goes
/// over a whole document. The same rules as the ones applied as one types,
/// switched on and off apart from them — a person may want quotes curled in a
/// pasted text file and not as they type, or the other way round.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reformat {
    /// A short line with a blank line after it becomes a heading.
    pub headings: bool,
    /// A paragraph begun `1. ` becomes a numbered list. Word's "List styles".
    pub numbered_lists: bool,
    /// A paragraph begun `- ` or `* ` becomes a bulleted list.
    pub bulleted_lists: bool,
    pub curly_quotes: bool,
    pub ordinals: bool,
    pub fractions: bool,
    pub dashes: bool,
    pub bold_italic: bool,
    pub hyperlinks: bool,
    /// A paragraph that already has a style of its own keeps it: no heading
    /// and no list is made of it.
    pub keep_styles: bool,
}

impl Default for Reformat {
    /// Everything on, which is how Word's tab arrives.
    fn default() -> Self {
        Self {
            headings: true,
            numbered_lists: true,
            bulleted_lists: true,
            curly_quotes: true,
            ordinals: true,
            fractions: true,
            dashes: true,
            bold_italic: true,
            hyperlinks: true,
            keep_styles: true,
        }
    }
}

/// One thing AutoFormat changes in a paragraph's words, found with the rest
/// of them before any is made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fix {
    /// These bytes become this text: a quote curled, a dash, an ordinal, a
    /// fraction.
    Text { start: usize, end: usize, putting: String },
    /// `*bold*` or `_italic_`: the marks go and what was between them takes
    /// the formatting.
    Emphasis(Emphasis),
    /// An address becomes a link to itself.
    Link { start: usize, end: usize },
}

impl Fix {
    /// The bytes of the paragraph it covers.
    #[must_use]
    pub fn span(&self) -> (usize, usize) {
        match self {
            Self::Text { start, end, .. } | Self::Link { start, end } => (*start, *end),
            Self::Emphasis(emphasis) => (emphasis.open, emphasis.close + 1),
        }
    }
}

impl Default for AutoCorrect {
    /// Everything on but the headings, which is how Word arrives.
    fn default() -> Self {
        Self {
            replacements: USUAL
                .iter()
                .map(|(what, with)| ((*what).to_owned(), (*with).to_owned()))
                .collect(),
            first_letter: ABBREVIATIONS.iter().map(|word| (*word).to_owned()).collect(),
            initial_caps: TWO_CAPITALS.iter().map(|word| (*word).to_owned()).collect(),
            two_initials: true,
            sentence_case: true,
            day_names: true,
            caps_lock: true,
            replace_text: true,
            curly_quotes: true,
            ordinals: true,
            fractions: true,
            dashes: true,
            automatic_lists: true,
            bold_italic: true,
            hyperlinks: true,
            border_lines: true,
            tables: true,
            headings: false,
            add_first_letter_exceptions: true,
            add_initial_caps_exceptions: true,
            reformat: Reformat::default(),
            math: MATH_USUAL
                .iter()
                .map(|(what, with)| ((*what).to_owned(), (*with).to_owned()))
                .collect(),
            math_replace: true,
            math_outside: false,
            actions: crate::actions::Recognisers::default(),
        }
    }
}

/// Which rule made a correction, which is what the little box under it names
/// and what "stop correcting this" has to know to stop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// An entry of the replacement list.
    Replacement,
    DayName,
    TwoInitials,
    CapsLock,
    SentenceCase,
    Ordinal,
    Fraction,
    Dash,
    /// A line begun with a marker made into a list.
    List,
    /// `*bold*` or `_italic_` given the formatting and losing the marks.
    Emphasis,
    /// An address made into a link.
    Hyperlink,
    /// Three hyphens on a line of their own made into a border.
    BorderLine,
    /// `+---+---+` made into a table.
    Table,
    /// A short line entered twice made into a heading.
    Heading,
    /// A name on the Math AutoCorrect list made into its character.
    Math,
}

impl Kind {
    /// Every rule, for the list of what the box can say.
    pub const ALL: [Self; 15] = [
        Self::Replacement,
        Self::DayName,
        Self::TwoInitials,
        Self::CapsLock,
        Self::SentenceCase,
        Self::Ordinal,
        Self::Fraction,
        Self::Dash,
        Self::List,
        Self::Emphasis,
        Self::Hyperlink,
        Self::BorderLine,
        Self::Table,
        Self::Heading,
        Self::Math,
    ];

    /// What the box under the correction offers to undo, in Word's words.
    #[must_use]
    pub fn undo_label(self) -> &'static str {
        match self {
            Self::Replacement | Self::Ordinal | Self::Fraction | Self::Dash | Self::Math => {
                "Undo Automatic Corrections"
            }
            Self::DayName | Self::TwoInitials | Self::CapsLock | Self::SentenceCase => {
                "Undo Automatic Capitalization"
            }
            Self::List => "Undo Automatic Numbering",
            Self::Emphasis => "Undo Automatic Formatting",
            Self::Hyperlink => "Undo Hyperlink",
            Self::BorderLine => "Undo Border Line",
            Self::Table => "Undo Automatic Table",
            Self::Heading => "Undo Automatic Heading Style",
        }
    }

    /// What the box offers to stop doing, in Word's words, given the word
    /// that was corrected.
    #[must_use]
    pub fn stop_label(self, word: &str) -> String {
        match self {
            Self::Replacement | Self::Math => {
                format!("Stop Automatically Correcting \u{201C}{word}\u{201D}")
            }
            Self::DayName => "Stop Capitalizing Names of Days".to_owned(),
            Self::TwoInitials => format!("Stop Correcting \u{201C}{word}\u{201D}"),
            Self::CapsLock => "Stop Correcting Accidental Use of Caps Lock".to_owned(),
            Self::SentenceCase => "Stop Auto-capitalizing First Letter of Sentences".to_owned(),
            Self::Ordinal => "Stop Superscripting Ordinals".to_owned(),
            Self::Fraction => "Stop Replacing Fractions".to_owned(),
            Self::Dash => "Stop Replacing Hyphens with Dashes".to_owned(),
            Self::List => "Stop Automatically Creating Lists".to_owned(),
            Self::Emphasis => "Stop Automatically Formatting Bold and Italic".to_owned(),
            Self::Hyperlink => "Stop Automatically Creating Hyperlinks".to_owned(),
            Self::BorderLine => "Stop Automatically Creating Border Lines".to_owned(),
            Self::Table => "Stop Automatically Creating Tables".to_owned(),
            Self::Heading => "Stop Automatically Applying Heading Styles".to_owned(),
        }
    }

    /// Whether what the box offers to stop doing names the word it was done
    /// to, and so cannot be put in a list of messages ahead of time.
    #[must_use]
    pub fn names_the_word(self) -> bool {
        matches!(self, Self::Replacement | Self::TwoInitials | Self::Math)
    }
}

/// What a correction does to the text before the caret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Correction {
    /// How many characters before the caret to take away.
    pub taking: usize,
    /// What to put in their place.
    pub putting: String,
    /// Which rule asked for it.
    pub kind: Kind,
}

impl AutoCorrect {
    /// The correction for the word just finished, if there is one.
    ///
    /// `before` is the text of the paragraph up to the caret, and the word is
    /// whatever follows the last space in it. The boundary character — the
    /// space or the full stop that finished the word — is not part of it and is
    /// typed afterwards.
    #[must_use]
    pub fn on_word(&self, before: &str) -> Option<Correction> {
        let word = last_word(before);
        if word.is_empty() {
            return None;
        }

        // The list first, because a person who put a word in it meant it to
        // win over any rule.
        if self.replace_text {
            if let Some(with) = self.replacements.get(&word.to_lowercase()) {
                return Some(Correction {
                    taking: word.chars().count(),
                    putting: matched_case(word, with),
                    kind: Kind::Replacement,
                });
            }
        }

        if self.day_names && DAYS.contains(&word.to_lowercase().as_str()) {
            let capitalised = capitalise(word);
            if capitalised != word {
                return Some(Correction {
                    taking: word.chars().count(),
                    putting: capitalised,
                    kind: Kind::DayName,
                });
            }
        }

        // TWo initial capitals, which is what a finger left on the shift key
        // makes. Not applied to a word that is all capitals: that is an
        // abbreviation, and Word leaves those alone too.
        if self.two_initials && !self.initial_caps.contains(word) {
            let letters: Vec<char> = word.chars().collect();
            let two_capitals = letters.len() > 2
                && letters[0].is_uppercase()
                && letters[1].is_uppercase()
                && letters[2..].iter().any(|letter| letter.is_lowercase())
                && letters[2..].iter().all(|letter| !letter.is_uppercase());
            if two_capitals {
                let mut fixed = String::new();
                fixed.extend(letters[0].to_uppercase());
                fixed.extend(letters[1].to_lowercase());
                fixed.extend(letters[2..].iter());
                return Some(Correction {
                    taking: letters.len(),
                    putting: fixed,
                    kind: Kind::TwoInitials,
                });
            }
        }

        // The caps lock left on with the shift key held: `tHE` for `The`.
        if self.caps_lock {
            let letters: Vec<char> = word.chars().collect();
            let inverted = letters.len() > 1
                && letters[0].is_lowercase()
                && letters[1..].iter().all(|letter| letter.is_uppercase());
            if inverted {
                return Some(Correction {
                    taking: letters.len(),
                    putting: capitalise(&word.to_lowercase()),
                    kind: Kind::CapsLock,
                });
            }
        }

        if self.sentence_case {
            if let Some(fixed) = sentence_capital(before, word, &self.first_letter) {
                return Some(Correction {
                    taking: word.chars().count(),
                    putting: fixed,
                    kind: Kind::SentenceCase,
                });
            }
        }

        if self.ordinals {
            if let Some(putting) = ordinal(word) {
                return Some(Correction {
                    taking: word.chars().count(),
                    putting,
                    kind: Kind::Ordinal,
                });
            }
        }

        if self.fractions {
            if let Some((_, mark)) = FRACTIONS.iter().find(|(text, _)| *text == word) {
                return Some(Correction {
                    taking: word.chars().count(),
                    putting: mark.to_string(),
                    kind: Kind::Fraction,
                });
            }
        }
        None
    }

    /// What a typed character should be instead, if it should be something
    /// else.
    ///
    /// The quotes, which curl by what is in front of them, and the hyphen
    /// between two words, which becomes a dash. Both are decided the moment the
    /// character is typed rather than at the end of the word, because both are
    /// about the character and not about the word.
    #[must_use]
    pub fn on_character(&self, before: &str, typed: char) -> Option<char> {
        let previous = before.chars().last();
        match typed {
            '"' if self.curly_quotes => {
                Some(if opens_a_quote(previous) { '\u{201C}' } else { '\u{201D}' })
            }
            '\'' if self.curly_quotes => {
                Some(if opens_a_quote(previous) { '\u{2018}' } else { '\u{2019}' })
            }
            _ => None,
        }
    }

    /// Whether a hyphen surrounded by words should become a dash.
    ///
    /// Word turns `a - b` into `a – b` when the space after the hyphen is
    /// typed, which is the moment it can tell a dash from a hyphenated word.
    #[must_use]
    pub fn dash_before(&self, before: &str) -> Option<Correction> {
        if !self.dashes {
            return None;
        }
        // The shape being looked for is "word space hyphen", the space after
        // which has just been typed.
        let mut letters = before.chars().rev();
        if letters.next() != Some('-') {
            return None;
        }
        if letters.next() != Some(' ') {
            return None;
        }
        letters.next().filter(|letter| !letter.is_whitespace())?;
        Some(Correction { taking: 1, putting: "\u{2013}".to_owned(), kind: Kind::Dash })
    }
}

/// `*bold*` or `_italic_` typed and finished: where the marks are, and which
/// formatting they asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Emphasis {
    /// Byte offsets of the opening and closing marks within `before`.
    pub open: usize,
    pub close: usize,
    pub bold: bool,
}

impl AutoCorrect {
    /// Whether the word just finished closed a `*bold*` or an `_italic_`.
    ///
    /// The closing mark is the last character of `before`, and the opening one
    /// is the nearest earlier one that begins a word — so that a lone asterisk
    /// in the middle of a sentence three lines back does not turn everything
    /// since into bold. Word's rule, and Word's limits: no spaces at the
    /// inside of either mark, and something between them.
    #[must_use]
    pub fn emphasis(&self, before: &str) -> Option<Emphasis> {
        if !self.bold_italic {
            return None;
        }
        let closing = before.chars().last()?;
        let bold = match closing {
            '*' => true,
            '_' => false,
            _ => return None,
        };
        let close = before.len() - closing.len_utf8();
        // The character before the closing mark is part of the word: "a *" is
        // not the end of anything.
        let inner_end = before[..close].chars().last()?;
        if inner_end.is_whitespace() || inner_end == closing {
            return None;
        }

        // The opening mark: the same character at the start of a word, with
        // no other of the same kind in between.
        let open = before[..close].rfind(closing)?;
        let begins_a_word = open == 0
            || before[..open].chars().last().is_some_and(|c| c.is_whitespace() || c == '(');
        if !begins_a_word {
            return None;
        }
        let inner_start = before[open + closing.len_utf8()..close].chars().next()?;
        if inner_start.is_whitespace() {
            return None;
        }
        Some(Emphasis { open, close, bold })
    }

    /// Whether a word just finished is an address the reader would expect to
    /// be a link: something with a scheme in front, or a `www.` in front, or
    /// an address at somebody.
    #[must_use]
    pub fn is_address(&self, word: &str) -> bool {
        if !self.hyperlinks || word.len() < 4 {
            return false;
        }
        let lower = word.to_ascii_lowercase();
        let schemed = ["http://", "https://", "ftp://", "mailto:", "file://"]
            .iter()
            .any(|scheme| lower.starts_with(scheme) && lower.len() > scheme.len());
        let bare = lower.starts_with("www.") && lower[4..].contains('.');
        let mail = lower.split_once('@').is_some_and(|(name, host)| {
            !name.is_empty() && host.contains('.') && !host.ends_with('.') && !host.contains('@')
        });
        schemed || bare || mail
    }

    /// The line a paragraph of three or more of one character becomes when
    /// it is ended: Word's six, as the style name the format uses and the
    /// width in eighths of a point.
    ///
    /// Hyphens make a plain line, underscores a heavier one, equals signs a
    /// double one, asterisks a dotted one, tildes a wavy one and pound signs a
    /// triple one with a thick centre — which is the list Word's help gives.
    #[must_use]
    pub fn border_line(&self, paragraph: &str) -> Option<(&'static str, u32)> {
        if !self.border_lines {
            return None;
        }
        let line = paragraph.trim();
        let mut characters = line.chars();
        let first = characters.next()?;
        if line.chars().count() < 3 || !characters.all(|c| c == first) {
            return None;
        }
        Some(match first {
            '-' => ("single", 6),
            '_' => ("single", 12),
            '=' => ("double", 6),
            '*' => ("dotted", 6),
            '~' => ("wave", 6),
            '#' => ("thinThickThinSmallGap", 24),
            _ => return None,
        })
    }

    /// The character a name typed just before this point stands for, where
    /// the Math AutoCorrect list has it: `\alpha` finished becomes α.
    ///
    /// `in_math` says whether the typing is in an equation, where the list is
    /// always used; in the text it is used only where the tab says so. The
    /// name is the last backslash and the letters after it, and it is finished
    /// by whatever was typed after it that is not a letter.
    #[must_use]
    pub fn math_word(&self, before: &str, in_math: bool) -> Option<Correction> {
        if !self.math_replace || !(in_math || self.math_outside) {
            return None;
        }
        let slash = before.rfind('\\')?;
        let name = &before[slash..];
        if name.len() < 2 || !name[1..].chars().all(char::is_alphabetic) {
            return None;
        }
        let with = self.math.get(name)?;
        Some(Correction { taking: name.chars().count(), putting: with.clone(), kind: Kind::Math })
    }

    /// Where the columns of a table typed as `+---+---+` begin and end: the
    /// byte offset of every plus sign, left to right.
    ///
    /// Word's rule: the line begins and ends with a plus sign, and between
    /// each two there is a run of hyphens and nothing else. One stretch makes
    /// a table of one column; the widths are where the plus signs fall on the
    /// line, which only the layout knows.
    #[must_use]
    pub fn table_columns(&self, paragraph: &str) -> Option<Vec<usize>> {
        if !self.tables {
            return None;
        }
        let line = paragraph.trim_end();
        if !line.starts_with('+') || !line.ends_with('+') || line.len() < 3 {
            return None;
        }
        let edges: Vec<usize> =
            line.char_indices().filter(|(_, c)| *c == '+').map(|(at, _)| at).collect();
        let hyphens_between = edges.windows(2).all(|pair| {
            let inside = &line[pair[0] + 1..pair[1]];
            !inside.is_empty() && inside.chars().all(|c| c == '-')
        });
        let nothing_else = line.chars().all(|c| c == '+' || c == '-');
        (edges.len() >= 2 && hyphens_between && nothing_else).then_some(edges)
    }

    /// The heading a line becomes when Enter is pressed twice after it: its
    /// level, and how many tabs it began with — which set the level and go.
    ///
    /// Word's rule, as far as its text goes: a line that begins with a capital
    /// and does not end with punctuation is a heading, Heading 1 as it stands
    /// and one level down for each tab in front of it. That it is one line
    /// long is the other half of the rule, and is the layout's to say.
    #[must_use]
    pub fn heading_level(&self, paragraph: &str) -> Option<(u8, usize)> {
        if !self.headings {
            return None;
        }
        let tabs = paragraph.chars().take_while(|c| *c == '\t').count();
        let text = paragraph[tabs..].trim_end();
        let first = text.chars().next()?;
        let last = text.chars().last()?;
        if tabs > 8 || !first.is_uppercase() || text.contains('\t') {
            return None;
        }
        if matches!(last, '.' | ',' | ';' | ':' | '!' | '?' | '\u{2026}') {
            return None;
        }
        Some((tabs as u8 + 1, tabs))
    }

    /// The rules AutoFormat goes over a document with: the AutoFormat tab's
    /// switches in place of the ones for typing, and none of what only makes
    /// sense of a word as it is typed — the replacement list, the capitals —
    /// which Word's AutoFormat does not do either.
    #[must_use]
    pub fn for_reformatting(&self) -> Self {
        let tab = &self.reformat;
        Self {
            replace_text: false,
            two_initials: false,
            sentence_case: false,
            day_names: false,
            caps_lock: false,
            curly_quotes: tab.curly_quotes,
            ordinals: tab.ordinals,
            fractions: tab.fractions,
            dashes: tab.dashes,
            bold_italic: tab.bold_italic,
            hyperlinks: tab.hyperlinks,
            automatic_lists: tab.numbered_lists || tab.bulleted_lists,
            border_lines: false,
            tables: false,
            headings: tab.headings,
            ..self.clone()
        }
    }

    /// Every change these rules make to a paragraph's words, found at once.
    ///
    /// The paragraph is read as if it were being typed: at every character
    /// that could be a quote, the quote rule is asked, and at every place a
    /// word ends — the end of the paragraph included — the rules for a
    /// finished word are, with what came before as what was typed so far.
    /// Where two would change the same characters the first found wins, as
    /// the first would have when typing.
    #[must_use]
    pub fn fixes_in(&self, text: &str) -> Vec<Fix> {
        let mut out: Vec<Fix> = Vec::new();
        let mut keep = |fix: Fix| {
            let (start, end) = fix.span();
            let clear = out.iter().all(|other| {
                let (from, to) = other.span();
                end <= from || to <= start
            });
            if clear && start < end {
                out.push(fix);
            }
        };

        let places = text.char_indices().map(|(at, c)| (at, Some(c))).chain([(text.len(), None)]);
        for (at, character) in places {
            let before = &text[..at];
            if let Some(character) = character {
                if let Some(curled) = self.on_character(before, character) {
                    if curled != character {
                        keep(Fix::Text {
                            start: at,
                            end: at + character.len_utf8(),
                            putting: curled.to_string(),
                        });
                    }
                }
                if !ends_a_word(character) {
                    continue;
                }
                if character == ' ' {
                    if let Some(dash) = self.dash_before(before) {
                        keep(Fix::Text { start: at - 1, end: at, putting: dash.putting });
                    }
                }
            }
            if let Some(emphasis) = self.emphasis(before) {
                keep(Fix::Emphasis(emphasis));
                continue;
            }
            let word = before.rsplit(char::is_whitespace).next().unwrap_or_default();
            if self.is_address(word) {
                keep(Fix::Link { start: before.len() - word.len(), end: before.len() });
                continue;
            }
            if let Some(correction) = self.on_word(before) {
                let start = before
                    .char_indices()
                    .rev()
                    .nth(correction.taking.saturating_sub(1))
                    .map_or(0, |(at, _)| at);
                keep(Fix::Text { start, end: before.len(), putting: correction.putting });
            }
        }
        out.sort_by_key(|fix| fix.span().0);
        out
    }

    /// Stops the rule that made a correction, which is what the box under
    /// the correction offers.
    ///
    /// For a replacement that means taking the pair off the list; for two
    /// initial capitals it means putting the word on the exceptions list,
    /// because the rule is right for every other word; for the rest it means
    /// the switch.
    pub fn stop(&mut self, kind: Kind, original: &str) {
        match kind {
            Kind::Replacement => {
                self.replacements.remove(&original.to_lowercase());
            }
            Kind::TwoInitials => {
                self.initial_caps.insert(original.to_owned());
            }
            Kind::DayName => self.day_names = false,
            Kind::CapsLock => self.caps_lock = false,
            Kind::SentenceCase => self.sentence_case = false,
            Kind::Ordinal => self.ordinals = false,
            Kind::Fraction => self.fractions = false,
            Kind::Dash => self.dashes = false,
            Kind::List => self.automatic_lists = false,
            Kind::Emphasis => self.bold_italic = false,
            Kind::Hyperlink => self.hyperlinks = false,
            Kind::BorderLine => self.border_lines = false,
            Kind::Table => self.tables = false,
            Kind::Heading => self.headings = false,
            Kind::Math => {
                self.math.remove(original);
            }
        }
    }

    /// Learns from a correction being undone.
    ///
    /// Word's "Automatically add words to list": a capital undone after an
    /// abbreviation puts the abbreviation on the First Letter list, and two
    /// initial capitals undone put the word on the INitial CAps list — so that
    /// a person who undoes it once is not asked to undo it every time. `ahead`
    /// is what came before the word. Returns whether anything was learnt.
    pub fn learn_from_undo(&mut self, kind: Kind, original: &str, ahead: &str) -> bool {
        match kind {
            Kind::SentenceCase if self.add_first_letter_exceptions => {
                // Only after an abbreviation: the first word of a paragraph
                // undone teaches nothing about any word.
                let abbreviation = last_word(ahead.trim_end());
                if abbreviation.len() > 1 && abbreviation.ends_with('.') {
                    self.first_letter.insert(abbreviation.to_lowercase())
                } else {
                    false
                }
            }
            Kind::TwoInitials if self.add_initial_caps_exceptions => {
                self.initial_caps.insert(original.to_owned())
            }
            _ => false,
        }
    }

    /// Where a numbered list typed by hand begins: "1." makes one, and so does
    /// "7." — at seven, which is what a person who typed seven meant.
    #[must_use]
    pub fn list_start(&self, marker: &str) -> Option<i32> {
        if !self.automatic_lists {
            return None;
        }
        let number = marker.strip_suffix('.').or_else(|| marker.strip_suffix(')'))?;
        let start: i32 = number.parse().ok()?;
        (start >= 1 && number.len() <= 3).then_some(start)
    }
}

/// Whether a character ends a word.
///
/// Word's list: a space, and the punctuation that closes a sentence or a
/// clause. A hyphen does not, because a hyphenated word is one word.
#[must_use]
pub fn ends_a_word(character: char) -> bool {
    character.is_whitespace()
        || matches!(character, '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}' | '"' | '\'')
}

/// The word before the caret: everything after the last space.
#[must_use]
fn last_word(before: &str) -> &str {
    let start = before
        .char_indices()
        .rev()
        .find(|(_, letter)| letter.is_whitespace() || *letter == '\u{a0}')
        .map_or(0, |(at, letter)| at + letter.len_utf8());
    &before[start..]
}

/// Whether a quote typed after this character opens rather than closes.
fn opens_a_quote(previous: Option<char>) -> bool {
    match previous {
        // Nothing before it, or a space, or an opening bracket: it opens.
        None => true,
        Some(letter) => letter.is_whitespace() || matches!(letter, '(' | '[' | '{' | '\u{201C}'),
    }
}

/// The same word with its first letter a capital.
fn capitalise(word: &str) -> String {
    let mut letters = word.chars();
    match letters.next() {
        Some(first) => first.to_uppercase().chain(letters).collect(),
        None => String::new(),
    }
}

/// A replacement written the way the word it replaces was.
///
/// A person who types "TEH" in a heading of capitals means "THE", and one who
/// starts a sentence with "Teh" means "The". Only those two cases: anything
/// else goes in as the list has it, because the list may be a person's own
/// shorthand for something that is written a particular way.
fn matched_case(word: &str, with: &str) -> String {
    let letters: Vec<char> = word.chars().collect();
    if letters.len() > 1 && letters.iter().all(|letter| letter.is_uppercase()) {
        return with.to_uppercase();
    }
    if letters.first().is_some_and(|first| first.is_uppercase()) {
        return capitalise(with);
    }
    with.to_owned()
}

/// The first letter of a sentence, capitalised.
///
/// A sentence starts at the beginning of a paragraph or after a full stop, a
/// question mark or an exclamation mark. Nothing is done to a word that is
/// already capitalised, nor to one that follows an abbreviation: `exceptions`
/// is the list of full stops that end a word rather than a sentence.
fn sentence_capital(before: &str, word: &str, exceptions: &BTreeSet<String>) -> Option<String> {
    let first = word.chars().next()?;
    if !first.is_lowercase() {
        return None;
    }

    // What comes before the word, less the space that separates them. A word is
    // whatever follows the last space, so this either ends in a space or is the
    // whole of an empty beginning.
    let ahead = before[..before.len() - word.len()].trim_end();
    if ahead.is_empty() {
        // The first word of the paragraph.
        return Some(capitalise(word));
    }
    if !ahead.ends_with(['.', '?', '!']) {
        return None;
    }
    // A full stop that ends an abbreviation does not end a sentence.
    if exceptions.contains(&last_word(ahead).to_lowercase()) {
        return None;
    }
    Some(capitalise(word))
}

/// `1st` and its kin, with the ending raised.
///
/// The raised letters are real characters rather than a superscript run,
/// because a correction that changed the formatting of what follows it would
/// keep raising everything typed after it.
fn ordinal(word: &str) -> Option<String> {
    let digits: String = word.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    let ending = &word[digits.len()..];
    let raised = match ending.to_lowercase().as_str() {
        "st" => "\u{02E2}\u{1D57}",
        "nd" => "\u{207F}\u{1D48}",
        "rd" => "\u{02B3}\u{1D48}",
        "th" => "\u{1D57}\u{02B0}",
        _ => return None,
    };
    // Only where the number agrees with the ending: "2st" is a typing mistake,
    // not an ordinal, and raising it would make the mistake look deliberate.
    let number: u32 = digits.parse().ok()?;
    let wanted = match (number % 100, number % 10) {
        (11..=13, _) => "th",
        (_, 1) => "st",
        (_, 2) => "nd",
        (_, 3) => "rd",
        _ => "th",
    };
    if ending.to_lowercase() != wanted {
        return None;
    }
    Some(format!("{digits}{raised}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules() -> AutoCorrect {
        AutoCorrect::default()
    }

    #[test]
    fn the_word_before_the_caret_is_what_is_looked_at() {
        assert_eq!(last_word("one two teh"), "teh");
        assert_eq!(last_word("teh"), "teh");
        assert_eq!(last_word("one "), "");
    }

    #[test]
    fn a_word_on_the_list_is_replaced() {
        let correction = rules().on_word("I think teh").expect("a correction");
        assert_eq!(correction.taking, 3);
        assert_eq!(correction.putting, "the");
    }

    #[test]
    fn a_replacement_is_written_the_way_the_word_was() {
        assert_eq!(rules().on_word("Teh").expect("a correction").putting, "The");
        assert_eq!(rules().on_word("say TEH").expect("a correction").putting, "THE");
    }

    #[test]
    fn nothing_on_the_list_is_left_alone() {
        // Mid-sentence, because the first word of one gets a capital whatever
        // else is or is not done to it.
        assert_eq!(rules().on_word("an ordinary"), None);
    }

    #[test]
    fn two_initial_capitals_become_one() {
        let correction = rules().on_word("TWo").expect("a correction");
        assert_eq!(correction.putting, "Two");
    }

    #[test]
    fn a_word_of_capitals_is_an_abbreviation_and_is_left_alone() {
        // "BBC" is not a finger left on the shift key.
        assert_eq!(rules().on_word("BBC"), None);
        assert_eq!(rules().on_word("PDF"), None);
    }

    #[test]
    fn a_day_is_capitalised() {
        assert_eq!(rules().on_word("monday").expect("a correction").putting, "Monday");
        // And one already capitalised is not corrected to itself.
        assert_eq!(rules().on_word("Monday"), None);
    }

    #[test]
    fn a_word_typed_with_the_caps_lock_on_is_turned_back_up() {
        assert_eq!(rules().on_word("tHE").expect("a correction").putting, "The");
    }

    #[test]
    fn the_first_word_of_a_sentence_gets_a_capital() {
        assert_eq!(rules().on_word("hello").expect("a correction").putting, "Hello");
        assert_eq!(rules().on_word("One. two").expect("a correction").putting, "Two");
    }

    #[test]
    fn a_decimal_point_does_not_start_a_sentence() {
        assert_eq!(rules().on_word("it is 3.14"), None);
        assert_eq!(rules().on_word("it is 3.14 and"), None);
    }

    #[test]
    fn an_abbreviation_does_not_start_a_sentence() {
        assert_eq!(rules().on_word("see e.g. the"), None);
        assert_eq!(rules().on_word("ask Mr. smith"), None);
        // But a full stop that is not on the list does end one.
        assert_eq!(rules().on_word("it ended. then").expect("a correction").putting, "Then");
    }

    #[test]
    fn a_word_allowed_two_capitals_keeps_them() {
        assert_eq!(rules().on_word("two CDs"), None);
        // And one that is not on the list is still corrected.
        assert_eq!(rules().on_word("TWo").expect("a correction").putting, "Two");
    }

    #[test]
    fn a_quote_curls_the_way_what_is_in_front_of_it_asks() {
        let rules = rules();
        assert_eq!(rules.on_character("", '"'), Some('\u{201C}'));
        assert_eq!(rules.on_character("he said ", '"'), Some('\u{201C}'));
        assert_eq!(rules.on_character("he said \u{201C}yes", '"'), Some('\u{201D}'));
        assert_eq!(rules.on_character("it", '\''), Some('\u{2019}'));
    }

    #[test]
    fn a_hyphen_between_words_becomes_a_dash() {
        let correction = rules().dash_before("one -").expect("a dash");
        assert_eq!(correction.taking, 1);
        assert_eq!(correction.putting, "\u{2013}");

        // A hyphenated word is not a dash, and neither is a hyphen at the
        // start of a line.
        assert_eq!(rules().dash_before("well-"), None);
        assert_eq!(rules().dash_before(" -"), None);
    }

    #[test]
    fn an_ordinal_is_raised_only_where_the_ending_fits_the_number() {
        assert!(rules().on_word("1st").is_some());
        assert!(rules().on_word("22nd").is_some());
        assert!(rules().on_word("11th").is_some());
        // The ones that are typing mistakes rather than ordinals.
        assert_eq!(rules().on_word("2st"), None);
        assert_eq!(rules().on_word("11st"), None);
    }

    #[test]
    fn a_fraction_becomes_the_character_for_it() {
        assert_eq!(rules().on_word("1/2").expect("a fraction").putting, "½");
    }

    #[test]
    fn every_rule_can_be_switched_off() {
        let off = AutoCorrect {
            two_initials: false,
            sentence_case: false,
            day_names: false,
            caps_lock: false,
            replace_text: false,
            curly_quotes: false,
            ordinals: false,
            fractions: false,
            dashes: false,
            automatic_lists: false,
            ..AutoCorrect::default()
        };
        assert_eq!(off.on_word("teh"), None);
        assert_eq!(off.on_word("TWo"), None);
        assert_eq!(off.on_word("monday"), None);
        assert_eq!(off.on_word("tHE"), None);
        assert_eq!(off.on_word("hello"), None);
        assert_eq!(off.on_word("1st"), None);
        assert_eq!(off.on_word("1/2"), None);
        assert_eq!(off.on_character("", '"'), None);
        assert_eq!(off.dash_before("one -"), None);
    }

    #[test]
    fn a_line_of_one_character_becomes_a_line_under_the_paragraph_above() {
        let rules = AutoCorrect::default();
        assert_eq!(rules.border_line("---"), Some(("single", 6)));
        assert_eq!(rules.border_line("-----"), Some(("single", 6)));
        assert_eq!(rules.border_line("___"), Some(("single", 12)));
        assert_eq!(rules.border_line("==="), Some(("double", 6)));
        assert_eq!(rules.border_line("***"), Some(("dotted", 6)));
        assert_eq!(rules.border_line("~~~"), Some(("wave", 6)));
        assert_eq!(rules.border_line("###"), Some(("thinThickThinSmallGap", 24)));
        assert_eq!(rules.border_line("--"), None, "two is a dash, not a line");
        assert_eq!(rules.border_line("-=-"), None);
        assert_eq!(rules.border_line("--- and"), None);
        let off = AutoCorrect { border_lines: false, ..AutoCorrect::default() };
        assert_eq!(off.border_line("---"), None);
    }

    #[test]
    fn a_whole_paragraph_is_read_as_if_it_were_typed() {
        let rules = rules().for_reformatting();
        let text = "He said \"it's the 21st\" - on *time* at www.example.com, 1/2 done";
        let fixes = rules.fixes_in(text);
        let texts: Vec<(&str, &str)> = fixes
            .iter()
            .filter_map(|fix| match fix {
                Fix::Text { start, end, putting } => Some((&text[*start..*end], putting.as_str())),
                _ => None,
            })
            .collect();
        assert_eq!(
            texts,
            vec![
                ("\"", "\u{201C}"),
                ("'", "\u{2019}"),
                ("21st", "21\u{02E2}\u{1D57}"),
                ("\"", "\u{201D}"),
                ("-", "\u{2013}"),
                ("1/2", "½"),
            ]
        );
        assert!(fixes.iter().any(|fix| matches!(fix, Fix::Emphasis(emphasis) if emphasis.bold)));
        assert!(fixes.iter().any(|fix| matches!(fix, Fix::Link { start, end }
            if &text[*start..*end] == "www.example.com")));
        // Nothing overlaps anything else.
        for pair in fixes.windows(2) {
            assert!(pair[0].span().1 <= pair[1].span().0, "{fixes:?}");
        }
    }

    #[test]
    fn reformatting_leaves_what_is_only_for_typing_alone() {
        // The replacement list and the capitals are about a word as it is
        // typed; a document gone over whole keeps its "teh" and its "monday".
        let reformatting = rules().for_reformatting();
        assert_eq!(reformatting.fixes_in("teh monday. and then"), vec![]);
        // And each of the tab's switches turns its rule off there alone.
        let mut quiet = rules();
        quiet.reformat.curly_quotes = false;
        assert_eq!(quiet.for_reformatting().fixes_in("\"a\""), vec![]);
        assert!(quiet.curly_quotes, "the typing rule is its own");
    }

    #[test]
    fn a_math_name_is_its_character_in_an_equation() {
        let rules = rules();
        let greek = |before: &str| rules.math_word(before, true).map(|fix| fix.putting);
        assert_eq!(greek("x = \\alpha"), Some("\u{3B1}".to_owned()));
        assert_eq!(greek("\\Delta"), Some("\u{394}".to_owned()), "the capital is its own name");
        assert_eq!(greek("\\varepsilon"), Some("\u{3B5}".to_owned()));
        assert_eq!(greek("a \\le b \\to"), Some("\u{2192}".to_owned()));
        assert_eq!(rules.math_word("\\infty", true).map(|fix| fix.taking), Some(6));
        for not_one in ["alpha", "\\", "\\nosuchname", "\\alpha2", "a\\b c"] {
            assert_eq!(greek(not_one), None, "{not_one:?}");
        }
    }

    #[test]
    fn a_math_name_in_the_text_is_left_alone_unless_the_tab_says_otherwise() {
        let mut rules = rules();
        assert_eq!(rules.math_word("\\alpha", false), None, "Word ships it off");
        rules.math_outside = true;
        assert!(rules.math_word("\\alpha", false).is_some());
        rules.math_replace = false;
        assert_eq!(rules.math_word("\\alpha", true), None, "the list switched off");
    }

    #[test]
    fn stopping_a_math_correction_takes_the_name_off_the_list() {
        let mut rules = rules();
        rules.stop(Kind::Math, "\\alpha");
        assert_eq!(rules.math_word("\\alpha", true), None);
        assert!(rules.math_word("\\beta", true).is_some());
    }

    #[test]
    fn plus_signs_and_hyphens_are_a_table() {
        let rules = rules();
        assert_eq!(rules.table_columns("+---+------+"), Some(vec![0, 4, 11]));
        assert_eq!(rules.table_columns("+--+"), Some(vec![0, 3]), "one column");
        assert_eq!(rules.table_columns("+--+ "), Some(vec![0, 3]), "a space after it is no matter");
        for not_one in ["+", "++", "+--", "--+", "+-- -+", "+--++", " +--+", "+==+", "a+--+"] {
            assert_eq!(rules.table_columns(not_one), None, "{not_one:?}");
        }
        let mut off = rules.clone();
        off.tables = false;
        assert_eq!(off.table_columns("+---+"), None);
    }

    #[test]
    fn a_short_line_with_no_full_stop_is_a_heading_where_that_is_switched_on() {
        let mut rules = rules();
        assert_eq!(rules.heading_level("Introduction"), None, "Word ships it off");
        rules.headings = true;
        assert_eq!(rules.heading_level("Introduction"), Some((1, 0)));
        assert_eq!(rules.heading_level("\tWhat came before"), Some((2, 1)));
        assert_eq!(rules.heading_level("\t\tSmaller still"), Some((3, 2)));
        assert_eq!(rules.heading_level("Введение"), Some((1, 0)), "a capital in any alphabet");
        for not_one in ["introduction", "It was late.", "Why?", "Note:", "", "\t", "Two\tparts"] {
            assert_eq!(rules.heading_level(not_one), None, "{not_one:?}");
        }
    }

    #[test]
    fn stars_round_a_word_mean_bold_and_underscores_mean_italic() {
        let rules = AutoCorrect::default();
        assert_eq!(rules.emphasis("*bold*"), Some(Emphasis { open: 0, close: 5, bold: true }));
        assert_eq!(
            rules.emphasis("very _italic words_"),
            Some(Emphasis { open: 5, close: 18, bold: false })
        );
        assert_eq!(rules.emphasis("a * b*"), None, "a space inside the opening mark");
        assert_eq!(rules.emphasis("2*3*"), None, "the opening mark is inside a word");
        assert_eq!(rules.emphasis("**"), None, "nothing between the marks");
        assert_eq!(rules.emphasis("bold"), None);
        let off = AutoCorrect { bold_italic: false, ..AutoCorrect::default() };
        assert_eq!(off.emphasis("*bold*"), None);
    }

    #[test]
    fn an_address_is_known_by_its_shape() {
        let rules = AutoCorrect::default();
        assert!(rules.is_address("https://example.com/page"));
        assert!(rules.is_address("www.example.com"));
        assert!(rules.is_address("someone@example.com"));
        assert!(rules.is_address("mailto:someone@example.com"));
        assert!(!rules.is_address("example.com"), "a bare name is a name");
        assert!(!rules.is_address("www"));
        assert!(!rules.is_address("@example.com"));
        assert!(!rules.is_address("someone@example"));
        let off = AutoCorrect { hyperlinks: false, ..AutoCorrect::default() };
        assert!(!off.is_address("https://example.com"));
    }

    #[test]
    fn a_list_typed_by_hand_begins_where_the_typing_did() {
        let rules = AutoCorrect::default();
        assert_eq!(rules.list_start("1."), Some(1));
        assert_eq!(rules.list_start("7."), Some(7));
        assert_eq!(rules.list_start("12)"), Some(12));
        assert_eq!(rules.list_start("0."), None);
        assert_eq!(rules.list_start("2024."), None, "a year is not a list");
        assert_eq!(rules.list_start("a."), None);
    }

    #[test]
    fn stopping_a_correction_stops_the_rule_that_made_it() {
        let mut rules = AutoCorrect::default();
        // After a word, so that the first word of a sentence does not get its
        // capital and muddy what is being asked.
        assert!(rules.on_word("a teh").is_some());
        rules.stop(Kind::Replacement, "teh");
        assert_eq!(rules.on_word("a teh"), None, "the pair is still on the list");
        assert!(rules.on_word("a adn").is_some(), "the other pairs went with it");

        rules.stop(Kind::TwoInitials, "TWo");
        assert_eq!(rules.on_word("a TWo"), None);
        assert!(rules.on_word("a THree").is_some(), "the rule itself was switched off");

        rules.stop(Kind::SentenceCase, "hello");
        assert!(!rules.sentence_case);
        rules.stop(Kind::List, "1.");
        assert!(!rules.automatic_lists);
        rules.stop(Kind::BorderLine, "---");
        assert!(!rules.border_lines);
    }

    #[test]
    fn an_undone_capital_after_an_abbreviation_teaches_the_abbreviation() {
        let mut rules = AutoCorrect::default();
        assert!(rules.on_word("Sent Wed. hello").is_some(), "Wed. is not on the list yet");
        assert!(rules.learn_from_undo(Kind::SentenceCase, "hello", "Sent Wed. "));
        assert!(rules.first_letter.contains("wed."));
        assert_eq!(rules.on_word("Sent Wed. hello"), None, "the list was not consulted");

        // The first word of a paragraph teaches nothing.
        assert!(!rules.learn_from_undo(Kind::SentenceCase, "hello", ""));
        // And nothing is learnt when the list does not add to itself.
        rules.add_initial_caps_exceptions = false;
        assert!(!rules.learn_from_undo(Kind::TwoInitials, "QUeue", ""));
        rules.add_initial_caps_exceptions = true;
        assert!(rules.learn_from_undo(Kind::TwoInitials, "QUeue", ""));
        assert_eq!(rules.on_word("a QUeue"), None);
    }
}
