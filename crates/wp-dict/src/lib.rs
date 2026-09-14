//! Reading the open dictionary formats: the word list and the affix rules.
//!
//! # Why a word list is not enough
//!
//! English has about fifty thousand words and about two hundred thousand forms
//! of them. Walk, walks, walked, walking; big, bigger, biggest; the plural of
//! every noun and the possessive of every plural. A spelling checker holding
//! only the list underlines half of what anybody writes, and a checker that
//! underlines correct words teaches people to ignore the underlining, which is
//! worse than no checker at all.
//!
//! So a real dictionary is two files. The word list says `walk/DSG` — the word,
//! and letters naming the rules it may take. The affix file says what each
//! letter means: rule `G` puts `ing` on the end of anything ending in a letter
//! that is not `e`. Fifty thousand entries and a hundred rules then cover the
//! two hundred thousand forms, and a language with real morphology — German,
//! Russian, Hungarian — is possible at all.
//!
//! The format is Hunspell's, which is what LibreOffice, Firefox, Chrome and
//! macOS all read, and what every free dictionary is published in. No
//! dictionary is shipped with this program: they are data with their own
//! licences, exactly as typefaces are, and this reads whatever the machine has
//! or whatever the reader is given.
//!
//! # How a word is checked
//!
//! Backwards. The word as typed is looked up first, and if that fails, every
//! rule that could have produced it is undone: take the ending off, put back
//! whatever the rule had stripped, and ask whether *that* is a word which is
//! allowed to take the rule. A word may have a prefix and a suffix at once, and
//! a suffix may itself carry the right to another suffix, so the undoing goes
//! two deep.
//!
//! # What is here and what is not
//!
//! The affixes, in full: stripping, conditions, cross products, the flags that
//! say a stem is not a word on its own, is forbidden, or must keep its case.
//! Compounding — the German habit of writing several words as one — as far as
//! the simple flags express it.
//!
//! Not the suggestions: what to offer in place of a word nobody knows is the
//! next item, and the data it needs (`TRY`, `REP`, `MAP`, `KEY`) is read and
//! kept here for it.

#![forbid(unsafe_code)]

pub mod thesaurus;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// A flag as the files write one: a letter, a pair of letters, or a number.
///
/// Kept as a number whichever way it was written, because what matters is only
/// that the word list and the affix file agree.
type Flag = u32;

/// Why a dictionary could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The affix file names an encoding this does not decode. Named rather
    /// than guessed at: a dictionary read in the wrong encoding is a dictionary
    /// of words nobody typed.
    UnknownEncoding(String),
    /// The files could not be read at all.
    NotReadable(String),
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnknownEncoding(name) => {
                write!(f, "the dictionary is written in {name}, which cannot be read here")
            }
            Self::NotReadable(why) => write!(f, "the dictionary could not be read: {why}"),
        }
    }
}

impl std::error::Error for Error {}

/// How the flags in a dictionary are written.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Flags {
    /// One character to a flag, which is what nearly every dictionary uses.
    #[default]
    Single,
    /// Two characters to a flag, for a language needing more than a few
    /// hundred rules.
    Double,
    /// Numbers separated by commas, for the same reason.
    Numeric,
}

/// One affix rule: what it takes off a word and what it puts on.
#[derive(Clone, Debug)]
struct Rule {
    /// What the rule strips from the stem before adding anything.
    strip: String,
    /// What it adds.
    add: String,
    /// Flags the added form itself carries, which is how one suffix allows
    /// another after it.
    carries: Vec<Flag>,
    /// What the stem must look like at the end the rule works on.
    condition: Vec<Match>,
}

/// One step of the little pattern an affix's condition is written in.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Match {
    /// `.`: anything at all.
    Any,
    One(char),
    /// `[abc]` and `[^abc]`.
    In(Vec<char>),
    NotIn(Vec<char>),
}

impl Match {
    fn matches(&self, character: char) -> bool {
        match self {
            Self::Any => true,
            Self::One(wanted) => character == *wanted,
            Self::In(set) => set.contains(&character),
            Self::NotIn(set) => !set.contains(&character),
        }
    }
}

/// Every rule sharing one flag, and whether they may be used with an affix at
/// the other end of the word.
#[derive(Clone, Debug, Default)]
struct Group {
    crossable: bool,
    rules: Vec<Rule>,
}

/// The words of a language and the rules for making their forms.
#[derive(Clone, Debug, Default)]
pub struct Dictionary {
    /// Every stem, and the flags saying which rules it may take.
    words: HashMap<String, Vec<Flag>>,
    prefixes: HashMap<Flag, Group>,
    suffixes: HashMap<Flag, Group>,
    /// The flags that change what a stem is rather than what may be added to
    /// it.
    needs_affix: Option<Flag>,
    forbidden: Option<Flag>,
    only_in_compound: Option<Flag>,
    keep_case: Option<Flag>,
    no_suggest: Option<Flag>,
    /// The flags that say a stem may be part of a word written as several
    /// words run together, which is how German works.
    compound: Option<Flag>,
    compound_begin: Option<Flag>,
    compound_middle: Option<Flag>,
    compound_end: Option<Flag>,
    compound_min: usize,
    /// What a suggester will want, read here because this is where the file is
    /// read. See the note at the top.
    pub try_letters: String,
    pub replacements: Vec<(String, String)>,
    kind: Flags,
    /// Characters thrown away before a word is looked up, which a few
    /// dictionaries use for a soft hyphen and the like.
    ignored: HashSet<char>,
    /// What the file says to rewrite before looking a word up. Every English
    /// dictionary uses it for one thing: the curly apostrophe a word processor
    /// types is the straight one the word list holds, and without this every
    /// "don't" anybody writes is underlined.
    rewrites: Vec<(String, String)>,
}

impl Dictionary {
    /// Reads a dictionary from the two files it is written in.
    ///
    /// `affix` is the `.aff` and `words` the `.dic`, as bytes: the affix file
    /// says which encoding both are in, so neither can be turned into text
    /// until it has been read.
    pub fn read(affix: &[u8], words: &[u8]) -> Result<Self, Error> {
        let (affix_text, encoding) = decode(affix)?;
        let mut dictionary = Self { compound_min: 3, ..Self::default() };
        let aliases = dictionary.read_affix(&affix_text);

        let words_text = match encoding {
            Encoding::Utf8 => String::from_utf8_lossy(words).into_owned(),
            Encoding::Latin1 => words.iter().map(|byte| *byte as char).collect(),
        };
        dictionary.read_words(&words_text, &aliases);
        Ok(dictionary)
    }

    /// Reads a plain list of words, one to a line, with no rules at all.
    ///
    /// What somebody's own list of names and jargon looks like, and what this
    /// program could do before it could read a real dictionary.
    #[must_use]
    pub fn from_list(text: &str) -> Self {
        let mut dictionary = Self { compound_min: 3, ..Self::default() };
        for line in text.lines() {
            let word = line.split('/').next().unwrap_or_default().trim();
            if word.is_empty() || word.chars().all(|character| character.is_ascii_digit()) {
                continue;
            }
            dictionary.words.entry(word.to_lowercase()).or_default();
        }
        dictionary
    }

    /// Whether the dictionary knows anything at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    /// How many stems it holds — not how many words it knows, which is a much
    /// larger number and is the whole point of the rules.
    #[must_use]
    pub fn stems(&self) -> usize {
        self.words.len()
    }

    /// Adds a word, as "Add to Dictionary" does.
    pub fn add(&mut self, word: &str) {
        self.words.entry(word.to_lowercase()).or_default();
        self.words.entry(word.to_owned()).or_default();
    }

    /// Whether a word is spelled the way the language spells it.
    #[must_use]
    pub fn spelled(&self, word: &str) -> bool {
        let mut word: String = if self.ignored.is_empty() {
            word.to_owned()
        } else {
            word.chars().filter(|character| !self.ignored.contains(character)).collect()
        };
        for (from, to) in &self.rewrites {
            if word.contains(from.as_str()) {
                word = word.replace(from.as_str(), to);
            }
        }
        if word.is_empty() {
            return true;
        }

        if self.known(&word, true) {
            return true;
        }

        // A word may be capitalised because it begins a sentence, or shouted in
        // capitals, and the dictionary holds it in neither form. Both are the
        // word — except where the dictionary says a stem keeps its own case,
        // which is how it knows that "iPhone" is not "IPHONE".
        let lower = word.to_lowercase();
        if lower != word && self.known(&lower, false) {
            return true;
        }
        if word.chars().all(|character| !character.is_lowercase()) {
            let title = capitalised(&lower);
            if title != word && self.known(&title, false) {
                return true;
            }
        }
        false
    }

    /// The word exactly as given, through every rule that could have made it.
    fn known(&self, word: &str, same_case: bool) -> bool {
        if self.forbidden_word(word) {
            return false;
        }
        if self.is_stem(word, same_case, false) {
            return true;
        }
        if self.through_affixes(word, same_case) {
            return true;
        }
        self.compounded(word, same_case, 0)
    }

    /// Whether the dictionary holds this word itself, rather than as a form of
    /// something else.
    fn is_stem(&self, word: &str, same_case: bool, in_compound: bool) -> bool {
        let Some(flags) = self.words.get(word) else { return false };
        if self.has(flags, self.needs_affix) || self.has(flags, self.forbidden) {
            return false;
        }
        if !in_compound && self.has(flags, self.only_in_compound) {
            return false;
        }
        if !same_case && self.has(flags, self.keep_case) {
            return false;
        }
        true
    }

    /// Whether a word is one the dictionary explicitly forbids — a spelling
    /// that a rule would otherwise make and that nobody writes.
    fn forbidden_word(&self, word: &str) -> bool {
        self.words.get(word).is_some_and(|flags| self.has(flags, self.forbidden))
    }

    fn has(&self, flags: &[Flag], wanted: Option<Flag>) -> bool {
        wanted.is_some_and(|flag| flags.contains(&flag))
    }

    /// Undoes the affixes: every rule that could have produced this word,
    /// tried backwards.
    fn through_affixes(&self, word: &str, same_case: bool) -> bool {
        // A suffix on its own, and then a second suffix the first one allowed.
        for (flag, group, stem) in self.strip_suffix(word) {
            if self.stem_takes(&stem, flag, same_case) {
                return true;
            }
            // A prefix as well, where both rules allow the other end to be
            // used: "unreadable" is "un" and "able" on "read".
            if group.crossable {
                for (prefix_flag, prefix_group, inner) in self.strip_prefix(&stem) {
                    if prefix_group.crossable
                        && self.stem_takes_both(&inner, prefix_flag, flag, same_case)
                    {
                        return true;
                    }
                }
            }
            // Or a second suffix, where the first carries its flag.
            for (second, _, inner) in self.strip_suffix(&stem) {
                if self.carries(flag, &stem, second) && self.stem_takes(&inner, second, same_case) {
                    return true;
                }
            }
        }

        for (flag, _, stem) in self.strip_prefix(word) {
            if self.stem_takes(&stem, flag, same_case) {
                return true;
            }
        }
        false
    }

    /// Every stem this word could be, with a suffix taken off it.
    fn strip_suffix<'a>(&'a self, word: &'a str) -> Vec<(Flag, &'a Group, String)> {
        let mut out = Vec::new();
        for (flag, group) in &self.suffixes {
            for rule in &group.rules {
                let Some(head) = word.strip_suffix(rule.add.as_str()) else { continue };
                if rule.add.is_empty() && rule.strip.is_empty() {
                    continue;
                }
                let stem = format!("{head}{}", rule.strip);
                if stem.is_empty() || !ends_with(&stem, &rule.condition) {
                    continue;
                }
                out.push((*flag, group, stem));
            }
        }
        out
    }

    /// The same at the other end.
    fn strip_prefix<'a>(&'a self, word: &'a str) -> Vec<(Flag, &'a Group, String)> {
        let mut out = Vec::new();
        for (flag, group) in &self.prefixes {
            for rule in &group.rules {
                let Some(tail) = word.strip_prefix(rule.add.as_str()) else { continue };
                if rule.add.is_empty() && rule.strip.is_empty() {
                    continue;
                }
                let stem = format!("{}{tail}", rule.strip);
                if stem.is_empty() || !starts_with(&stem, &rule.condition) {
                    continue;
                }
                out.push((*flag, group, stem));
            }
        }
        out
    }

    /// Whether a stem is a word that is allowed to take a given rule.
    fn stem_takes(&self, stem: &str, flag: Flag, same_case: bool) -> bool {
        let Some(flags) = self.words.get(stem) else { return false };
        if !flags.contains(&flag) || self.has(flags, self.forbidden) {
            return false;
        }
        if !same_case && self.has(flags, self.keep_case) {
            return false;
        }
        if self.has(flags, self.only_in_compound) {
            return false;
        }
        true
    }

    fn stem_takes_both(&self, stem: &str, one: Flag, other: Flag, same_case: bool) -> bool {
        let Some(flags) = self.words.get(stem) else { return false };
        flags.contains(&one)
            && flags.contains(&other)
            && !self.has(flags, self.forbidden)
            && (same_case || !self.has(flags, self.keep_case))
    }

    /// Whether the rule that made `form` from a stem itself allows a second
    /// rule after it.
    fn carries(&self, first: Flag, form: &str, second: Flag) -> bool {
        let Some(group) = self.suffixes.get(&first) else { return false };
        group
            .rules
            .iter()
            .any(|rule| form.ends_with(rule.add.as_str()) && rule.carries.contains(&second))
    }

    /// Whether a word is several words written as one, which is how German
    /// makes most of its long nouns.
    fn compounded(&self, word: &str, same_case: bool, depth: usize) -> bool {
        if self.compound.is_none() && self.compound_begin.is_none() && self.compound_end.is_none() {
            return false;
        }
        if depth > 3 {
            return false;
        }

        let letters: Vec<(usize, char)> = word.char_indices().collect();
        if letters.len() < self.compound_min * 2 {
            return false;
        }

        let last = letters.len().saturating_sub(self.compound_min.saturating_sub(1));
        for (at, _) in letters.iter().take(last).skip(self.compound_min) {
            let (head, tail) = word.split_at(*at);
            if !self.compound_part(head, same_case, depth == 0) {
                continue;
            }
            if self.compound_part(tail, same_case, false) && self.compound_end_ok(tail) {
                return true;
            }
            if self.compounded(tail, same_case, depth + 1) {
                return true;
            }
        }
        false
    }

    /// Whether one piece of a run-together word is allowed to be one.
    fn compound_part(&self, part: &str, same_case: bool, first: bool) -> bool {
        let word = if same_case { part.to_owned() } else { part.to_lowercase() };
        for candidate in [word.clone(), part.to_lowercase(), capitalised(&part.to_lowercase())] {
            let Some(flags) = self.words.get(&candidate) else { continue };
            if self.has(flags, self.forbidden) {
                continue;
            }
            let allowed = self.has(flags, self.compound)
                || self.has(flags, self.compound_middle)
                || (first && self.has(flags, self.compound_begin))
                || (!first && self.has(flags, self.compound_end));
            if allowed {
                return true;
            }
        }
        false
    }

    fn compound_end_ok(&self, part: &str) -> bool {
        if self.compound_end.is_none() {
            return true;
        }
        let lower = part.to_lowercase();
        [part.to_owned(), lower.clone(), capitalised(&lower)].iter().any(|candidate| {
            self.words.get(candidate).is_some_and(|flags| {
                self.has(flags, self.compound_end) || self.has(flags, self.compound)
            })
        })
    }

    /// What the writer probably meant by a word the dictionary does not know.
    ///
    /// # How the guesses are made
    ///
    /// Nearly every misspelling is one slip: two letters the wrong way round,
    /// one left out, one too many, or one struck for its neighbour on the
    /// keyboard. So every word one such slip away is tried, and the ones that
    /// are words are offered — in the order the slips happen, which is the
    /// order a reader wants them in. Before any of that come the pairs the
    /// dictionary itself lists as common mistakes, `ph` for `f` and the like,
    /// because those are the ones a keyboard does not explain; and after it a
    /// space, because "thequick" is two words with the space forgotten.
    ///
    /// The guesses keep the case of what was typed: a capitalised mistake gets
    /// capitalised corrections.
    #[must_use]
    pub fn suggest(&self, word: &str) -> Vec<String> {
        /// How many are offered. Word shows a handful, and the first is the
        /// one that gets taken.
        const MOST: usize = 8;
        let lower = word.to_lowercase();
        let mut found: Vec<String> = Vec::new();

        let offer = |candidate: String, found: &mut Vec<String>| {
            if found.len() >= MOST || candidate.is_empty() || candidate == lower {
                return;
            }
            if self.spelled(&candidate)
                && self.suggestable(&candidate)
                && !found.contains(&candidate)
            {
                found.push(candidate);
            }
        };

        // The word itself in another case: "london" for "London" is a slip of
        // the shift key, not of the spelling.
        if lower != word {
            offer(lower.clone(), &mut found);
        }
        offer(capitalised(&lower), &mut found);

        // The pairs the dictionary lists as things people get wrong.
        for (from, to) in &self.replacements {
            let mut at = 0;
            while let Some(found_at) = lower[at..].find(from.as_str()) {
                let start = at + found_at;
                let mut candidate = String::with_capacity(lower.len());
                candidate.push_str(&lower[..start]);
                candidate.push_str(to);
                candidate.push_str(&lower[start + from.len()..]);
                offer(candidate, &mut found);
                at = start + from.len();
            }
        }

        let letters: Vec<char> = lower.chars().collect();
        let joined = |pieces: &[&[char]]| -> String {
            pieces.iter().flat_map(|piece| piece.iter()).collect()
        };

        // Two letters the wrong way round.
        for at in 0..letters.len().saturating_sub(1) {
            let mut swapped = letters.clone();
            swapped.swap(at, at + 1);
            offer(swapped.iter().collect(), &mut found);
        }
        // One letter too many.
        for at in 0..letters.len() {
            offer(joined(&[&letters[..at], &letters[at + 1..]]), &mut found);
        }
        // One letter left out, and one struck for another — tried with every
        // letter the dictionary says its language uses, most common first. The
        // left-out letter before the wrong one, because that is the commoner
        // slip and because the first guess is the one that gets taken.
        // Only the small letters: the word is tried in lower case and given
        // back in the case it was typed, and a capital in the middle of a word
        // is never what anybody meant.
        let alphabet: Vec<char> = if self.try_letters.is_empty() {
            ('a'..='z').collect()
        } else {
            self.try_letters.chars().filter(|letter| !letter.is_uppercase()).collect()
        };
        for at in 0..=letters.len() {
            for letter in &alphabet {
                offer(joined(&[&letters[..at], &[*letter], &letters[at..]]), &mut found);
            }
        }
        for at in 0..letters.len() {
            for letter in &alphabet {
                if *letter == letters[at] {
                    continue;
                }
                offer(joined(&[&letters[..at], &[*letter], &letters[at + 1..]]), &mut found);
            }
        }

        // A space forgotten.
        for at in 1..letters.len() {
            let (head, tail): (String, String) =
                (letters[..at].iter().collect(), letters[at..].iter().collect());
            if head.chars().count() > 1
                && tail.chars().count() > 1
                && self.spelled(&head)
                && self.spelled(&tail)
                && found.len() < MOST
            {
                let candidate = format!("{head} {tail}");
                if !found.contains(&candidate) {
                    found.push(candidate);
                }
            }
        }

        // In the case the writer used.
        let shouted = word.chars().count() > 1 && word.chars().all(|c| !c.is_lowercase());
        let capital = word.chars().next().is_some_and(char::is_uppercase);
        found
            .into_iter()
            .map(|candidate| {
                if shouted {
                    candidate.to_uppercase()
                } else if capital {
                    capitalised(&candidate)
                } else {
                    candidate
                }
            })
            .collect()
    }

    /// Whether a word is one the dictionary would rather not offer as a
    /// suggestion — a rude word, or a spelling it knows but nobody means.
    #[must_use]
    pub fn suggestable(&self, word: &str) -> bool {
        !self.words.get(word).is_some_and(|flags| self.has(flags, self.no_suggest))
    }

    // -- Reading the files ---------------------------------------------------

    /// Reads the affix file, and gives back the table of flag aliases, which
    /// the word list needs.
    fn read_affix(&mut self, text: &str) -> Vec<Vec<Flag>> {
        let mut aliases: Vec<Vec<Flag>> = Vec::new();
        let mut lines = text.lines().peekable();

        while let Some(line) = lines.next() {
            let line = line.split('#').next().unwrap_or_default().trim();
            let mut fields = line.split_whitespace();
            let Some(key) = fields.next() else { continue };
            let rest: Vec<&str> = fields.collect();

            match key {
                "FLAG" => {
                    self.kind = match rest.first().copied() {
                        Some("long") => Flags::Double,
                        Some("num") => Flags::Numeric,
                        // "UTF-8" means a flag is one character, which is what
                        // the default already is once the file is text.
                        _ => Flags::Single,
                    }
                }
                "TRY" => self.try_letters = rest.first().copied().unwrap_or_default().to_owned(),
                "IGNORE" => {
                    self.ignored = rest.first().copied().unwrap_or_default().chars().collect();
                }
                "ICONV" => {
                    // The first line says how many follow, and has one field.
                    if rest.len() >= 2 {
                        self.rewrites.push((rest[0].to_owned(), rest[1].to_owned()));
                    }
                }
                "NEEDAFFIX" | "PSEUDOROOT" => self.needs_affix = self.one_flag(&rest),
                "FORBIDDENWORD" => self.forbidden = self.one_flag(&rest),
                "ONLYINCOMPOUND" => self.only_in_compound = self.one_flag(&rest),
                "KEEPCASE" => self.keep_case = self.one_flag(&rest),
                "NOSUGGEST" => self.no_suggest = self.one_flag(&rest),
                "COMPOUNDFLAG" => self.compound = self.one_flag(&rest),
                "COMPOUNDBEGIN" => self.compound_begin = self.one_flag(&rest),
                "COMPOUNDMIDDLE" => self.compound_middle = self.one_flag(&rest),
                "COMPOUNDEND" | "COMPOUNDLAST" => self.compound_end = self.one_flag(&rest),
                "COMPOUNDMIN" => {
                    if let Some(value) = rest.first().and_then(|text| text.parse().ok()) {
                        self.compound_min = value;
                    }
                }
                "REP" => {
                    // The first line says how many follow; the rest are pairs.
                    if rest.len() >= 2 {
                        self.replacements
                            .push((rest[0].replace('_', " "), rest[1].replace('_', " ")));
                    }
                }
                "AF" => {
                    if rest.len() == 1 && rest[0].parse::<usize>().is_ok() && aliases.is_empty() {
                        continue;
                    }
                    let flags = self.flags_of(rest.first().copied().unwrap_or_default(), &[]);
                    aliases.push(flags);
                }
                "PFX" | "SFX" => {
                    let group = self.read_group(key, &rest, &mut lines);
                    if let Some((flag, group)) = group {
                        let into =
                            if key == "PFX" { &mut self.prefixes } else { &mut self.suffixes };
                        into.entry(flag).or_default().merge(group);
                    }
                }
                _ => {}
            }
        }
        aliases
    }

    /// One group of affix rules: the header line, then as many rules as it
    /// says.
    fn read_group<'a>(
        &self,
        key: &str,
        header: &[&str],
        lines: &mut core::iter::Peekable<impl Iterator<Item = &'a str>>,
    ) -> Option<(Flag, Group)> {
        let flag = *self.flags_of(header.first().copied()?, &[]).first()?;
        let crossable = header.get(1).copied() == Some("Y");
        let count: usize = header.get(2)?.parse().ok()?;

        let mut group = Group { crossable, rules: Vec::new() };
        for _ in 0..count {
            let Some(line) = lines.peek() else { break };
            let line = line.split('#').next().unwrap_or_default().trim().to_owned();
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.first().copied() != Some(key) {
                break;
            }
            lines.next();
            if fields.len() < 4 {
                continue;
            }

            let strip = if fields[2] == "0" { String::new() } else { fields[2].to_owned() };
            // What is added may carry flags of its own after a slash, which is
            // how one suffix makes room for another.
            let (add, carries) = match fields[3].split_once('/') {
                Some((add, flags)) => (add, self.flags_of(flags, &[])),
                None => (fields[3], Vec::new()),
            };
            let add = if add == "0" { String::new() } else { add.to_owned() };
            let condition = fields.get(4).map_or_else(Vec::new, |text| condition_of(text));

            group.rules.push(Rule { strip, add, carries, condition });
        }
        Some((flag, group))
    }

    /// Reads the word list.
    fn read_words(&mut self, text: &str, aliases: &[Vec<Flag>]) {
        for (number, line) in text.lines().enumerate() {
            let line = line.trim_end();
            // The first line is the count of the rest, and is not a word.
            if number == 0 && line.trim().parse::<usize>().is_ok() {
                continue;
            }
            if line.is_empty() || line.starts_with('\t') || line.starts_with('/') {
                continue;
            }

            // A line may carry morphological fields after a space, which say
            // what part of speech the word is. Nothing here asks.
            let entry = line.split(['\t', ' ']).next().unwrap_or_default();
            if entry.is_empty() {
                continue;
            }

            // The slash is what separates the word from its flags, and a word
            // that really holds one escapes it.
            let (word, flags) = split_entry(entry);
            let flags = self.flags_of(flags, aliases);
            let word = word.replace("\\/", "/");
            if word.is_empty() {
                continue;
            }
            let held = self.words.entry(word).or_default();
            for flag in flags {
                if !held.contains(&flag) {
                    held.push(flag);
                }
            }
        }
    }

    fn one_flag(&self, rest: &[&str]) -> Option<Flag> {
        self.flags_of(rest.first().copied()?, &[]).first().copied()
    }

    /// The flags a piece of text names, in whichever way this dictionary writes
    /// them.
    fn flags_of(&self, text: &str, aliases: &[Vec<Flag>]) -> Vec<Flag> {
        if text.is_empty() {
            return Vec::new();
        }
        // An aliased dictionary writes a number standing for a whole set of
        // flags, which is how a large one stays small.
        if !aliases.is_empty() {
            if let Ok(index) = text.parse::<usize>() {
                return aliases.get(index.wrapping_sub(1)).cloned().unwrap_or_default();
            }
        }

        match self.kind {
            Flags::Single => text.chars().map(|character| character as Flag).collect(),
            Flags::Double => {
                let letters: Vec<char> = text.chars().collect();
                letters
                    .chunks(2)
                    .map(|pair| {
                        let high = pair[0] as Flag;
                        let low = pair.get(1).map_or(0, |character| *character as Flag);
                        (high << 16) | low
                    })
                    .collect()
            }
            Flags::Numeric => {
                text.split(',').filter_map(|piece| piece.trim().parse().ok()).collect()
            }
        }
    }
}

impl Group {
    fn merge(&mut self, other: Self) {
        self.crossable |= other.crossable;
        self.rules.extend(other.rules);
    }
}

/// Splits a word list entry into the word and its flags, at the slash that is
/// not escaped.
fn split_entry(entry: &str) -> (&str, &str) {
    let bytes = entry.as_bytes();
    for (at, byte) in bytes.iter().enumerate() {
        if *byte == b'/' && (at == 0 || bytes[at - 1] != b'\\') {
            return (&entry[..at], &entry[at + 1..]);
        }
    }
    (entry, "")
}

/// The little pattern an affix's condition is written in.
fn condition_of(text: &str) -> Vec<Match> {
    if text == "." {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut letters = text.chars().peekable();
    while let Some(character) = letters.next() {
        match character {
            '.' => out.push(Match::Any),
            '[' => {
                let negated = letters.peek() == Some(&'^');
                if negated {
                    letters.next();
                }
                let mut set = Vec::new();
                for inside in letters.by_ref() {
                    if inside == ']' {
                        break;
                    }
                    set.push(inside);
                }
                out.push(if negated { Match::NotIn(set) } else { Match::In(set) });
            }
            other => out.push(Match::One(other)),
        }
    }
    out
}

/// Whether a stem ends the way a suffix's condition says it must.
fn ends_with(stem: &str, condition: &[Match]) -> bool {
    if condition.is_empty() {
        return true;
    }
    let letters: Vec<char> = stem.chars().collect();
    if letters.len() < condition.len() {
        return false;
    }
    let tail = &letters[letters.len() - condition.len()..];
    condition.iter().zip(tail).all(|(step, character)| step.matches(*character))
}

/// And at the other end, for a prefix.
fn starts_with(stem: &str, condition: &[Match]) -> bool {
    if condition.is_empty() {
        return true;
    }
    let letters: Vec<char> = stem.chars().collect();
    if letters.len() < condition.len() {
        return false;
    }
    condition.iter().zip(&letters).all(|(step, character)| step.matches(*character))
}

/// A word with its first letter made a capital and the rest left alone.
fn capitalised(word: &str) -> String {
    let mut letters = word.chars();
    match letters.next() {
        Some(first) => first.to_uppercase().collect::<String>() + letters.as_str(),
        None => String::new(),
    }
}

/// The encodings a dictionary may be written in that this can read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Encoding {
    Utf8,
    Latin1,
}

/// Turns an affix file into text, and says what encoding the pair is in.
///
/// The file says so on its first line, and it has to: the same bytes mean
/// different letters in different encodings, and a dictionary read in the wrong
/// one is a dictionary of words nobody typed.
fn decode(affix: &[u8]) -> Result<(String, Encoding), Error> {
    let head = String::from_utf8_lossy(&affix[..affix.len().min(256)]).into_owned();
    let named = head
        .lines()
        .find_map(|line| line.strip_prefix("SET "))
        .map(|name| name.trim().to_ascii_lowercase());

    let encoding = match named.as_deref() {
        None | Some("utf-8") | Some("utf8") => Encoding::Utf8,
        Some("iso8859-1") | Some("iso-8859-1") | Some("latin1") => Encoding::Latin1,
        Some(other) => return Err(Error::UnknownEncoding(other.to_owned())),
    };

    let text = match encoding {
        Encoding::Utf8 => String::from_utf8_lossy(affix).into_owned(),
        Encoding::Latin1 => affix.iter().map(|byte| *byte as char).collect(),
    };
    Ok((text, encoding))
}

/// Every dictionary installed on this machine, by the language it is for.
///
/// No dictionary is shipped with this program — they are data with their own
/// licences, exactly as typefaces are — so what can be checked is whatever the
/// machine already has. On a Linux machine that is whatever was installed
/// beside LibreOffice; on Windows there is no common place, and a reader opens
/// one by hand.
#[must_use]
pub fn installed() -> Vec<(String, PathBuf)> {
    let mut found: Vec<(String, PathBuf)> = Vec::new();
    for directory in search_paths() {
        let Ok(entries) = std::fs::read_dir(&directory) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("dic") {
                continue;
            }
            if !path.with_extension("aff").exists() {
                continue;
            }
            let Some(name) = path.file_stem().and_then(|value| value.to_str()) else { continue };
            if found.iter().any(|(held, _)| held == name) {
                continue;
            }
            found.push((name.to_owned(), path));
        }
    }
    found.sort_by(|one, other| one.0.cmp(&other.0));
    found
}

/// Reads one dictionary given the path of its word list; the affix file is
/// beside it under the same name.
pub fn read_pair(words: &Path) -> Result<Dictionary, Error> {
    let affix = words.with_extension("aff");
    let affix_bytes =
        std::fs::read(&affix).map_err(|error| Error::NotReadable(error.to_string()))?;
    let word_bytes = std::fs::read(words).map_err(|error| Error::NotReadable(error.to_string()))?;
    Dictionary::read(&affix_bytes, &word_bytes)
}

/// Where dictionaries are kept, by operating system.
pub(crate) fn search_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(beside) = std::env::current_exe() {
        if let Some(directory) = beside.parent() {
            paths.push(directory.join("dictionaries"));
        }
    }
    if cfg!(windows) {
        if let Ok(data) = std::env::var("APPDATA") {
            paths.push(PathBuf::from(data).join("hunspell"));
        }
    } else {
        paths.push(PathBuf::from("/usr/share/hunspell"));
        paths.push(PathBuf::from("/usr/share/myspell"));
        paths.push(PathBuf::from("/usr/share/myspell/dicts"));
        paths.push(PathBuf::from("/usr/share/hunspell-dicts"));
    }
    paths
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny dictionary written out here, so that what is being tested is the
    /// reading and not somebody else's word list.
    fn small() -> Dictionary {
        let affix = "\
SET UTF-8
TRY esianrtolcdugmphbyfvkwzESIANRTOLCDUGMPHBYFVKWZ

PFX A Y 1
PFX A   0     un         .

SFX B Y 2
SFX B   0     ed         [^e]
SFX B   0     d          e

SFX C Y 1
SFX C   y     ies        [^aeiou]y
";
        let words = "\
4
walk/AB
love/B
try/C
house
";
        Dictionary::read(affix.as_bytes(), words.as_bytes()).expect("a dictionary")
    }

    #[test]
    fn a_word_in_the_list_is_a_word() {
        let dictionary = small();
        assert!(dictionary.spelled("walk"));
        assert!(dictionary.spelled("house"));
        assert!(!dictionary.spelled("hovse"));
    }

    #[test]
    fn a_suffix_makes_a_form_that_is_not_in_the_list() {
        // Which is the whole point: the list holds four words and the
        // dictionary knows nine.
        let dictionary = small();
        assert!(dictionary.spelled("walked"));
        assert!(dictionary.spelled("loved"), "the rule that strips nothing and adds a d");
        assert!(dictionary.spelled("tries"), "the rule that strips the y");
        assert!(!dictionary.spelled("houseed"), "house may not take that rule");
    }

    #[test]
    fn a_condition_decides_which_rule_applies() {
        // Both B rules add something to make a past tense, and which one
        // depends on the letter the word ends in. A reader that ignores the
        // condition accepts "loveed".
        let dictionary = small();
        assert!(!dictionary.spelled("loveed"));
        assert!(!dictionary.spelled("walkd"));
    }

    #[test]
    fn a_prefix_and_a_suffix_may_be_used_at_once() {
        let dictionary = small();
        assert!(dictionary.spelled("unwalk"));
        assert!(dictionary.spelled("unwalked"), "both ends of the same word");
        assert!(!dictionary.spelled("unlove"), "love does not take that prefix");
    }

    #[test]
    fn a_word_may_be_capitalised_or_shouted() {
        let dictionary = small();
        assert!(dictionary.spelled("Walk"), "at the start of a sentence");
        assert!(dictionary.spelled("WALKED"), "in capitals");
        assert!(!dictionary.spelled("WALKD"));
    }

    #[test]
    fn a_stem_that_needs_an_affix_is_not_a_word_on_its_own() {
        let affix = "SET UTF-8\nNEEDAFFIX X\nSFX B Y 1\nSFX B 0 ed .\n";
        let words = "1\nfoo/XB\n";
        let dictionary = Dictionary::read(affix.as_bytes(), words.as_bytes()).unwrap();
        assert!(!dictionary.spelled("foo"), "the stem is not a word");
        assert!(dictionary.spelled("fooed"), "a form of it is");
    }

    #[test]
    fn a_forbidden_word_is_not_a_word_however_it_was_made() {
        let affix = "SET UTF-8\nFORBIDDENWORD !\nSFX B Y 1\nSFX B 0 s .\n";
        let words = "2\ncat/B\ncats/!\n";
        let dictionary = Dictionary::read(affix.as_bytes(), words.as_bytes()).unwrap();
        assert!(dictionary.spelled("cat"));
        assert!(!dictionary.spelled("cats"), "a rule would make it, and the file forbids it");
    }

    #[test]
    fn flags_may_be_written_as_pairs_or_as_numbers() {
        let long = "SET UTF-8\nFLAG long\nSFX Aa Y 1\nSFX Aa 0 s .\n";
        let dictionary = Dictionary::read(long.as_bytes(), b"1\ncat/Aa\n").unwrap();
        assert!(dictionary.spelled("cats"));

        let numeric = "SET UTF-8\nFLAG num\nSFX 1001 Y 1\nSFX 1001 0 s .\n";
        let dictionary = Dictionary::read(numeric.as_bytes(), b"1\ncat/1001\n").unwrap();
        assert!(dictionary.spelled("cats"));
    }

    #[test]
    fn a_dictionary_in_an_encoding_this_cannot_read_says_so() {
        let affix = b"SET ISO8859-2\n";
        assert_eq!(
            Dictionary::read(affix, b"0\n").err(),
            Some(Error::UnknownEncoding("iso8859-2".to_owned()))
        );
    }

    #[test]
    fn a_plain_list_of_words_is_still_a_dictionary() {
        let dictionary = Dictionary::from_list("alpha\nbeta\n# a comment\ngamma/XY\n");
        assert!(dictionary.spelled("alpha"));
        assert!(dictionary.spelled("Gamma"));
        assert!(!dictionary.spelled("delta"));
    }

    #[test]
    fn words_run_together_are_a_word_where_the_language_says_so() {
        // German writes several nouns as one, and the file says which words
        // may take part.
        let affix = "SET UTF-8\nCOMPOUNDFLAG Z\nCOMPOUNDMIN 3\n";
        let words = "3\nhaus/Z\ntür/Z\nbaum\n";
        let dictionary = Dictionary::read(affix.as_bytes(), words.as_bytes()).unwrap();
        assert!(dictionary.spelled("haustür"));
        assert!(!dictionary.spelled("hausbaum"), "baum is not allowed to join one");
    }
}
