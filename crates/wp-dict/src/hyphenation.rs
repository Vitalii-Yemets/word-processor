//! Where a word may be broken: Liang's patterns, read from the machine's own.
//!
//! # The algorithm
//!
//! Liang's, from 1983, which every typesetting program since TeX has used. A
//! language is described by a few thousand patterns — `hy3ph`, `.ad4der` —
//! each a run of letters with digits between them. A word is looked at with
//! a dot at each end, every pattern that occurs anywhere in it is found, and
//! at every position between two letters the largest digit any pattern put
//! there wins. An odd digit is a place the word may break; an even one is a
//! place it may not, which is how a pattern that allows a break is overruled
//! by a longer one that knows better. Nothing is invented: a word no pattern
//! mentions gets no digits and is never broken.
//!
//! # The files
//!
//! The patterns for a language are data, with their own licence, exactly as a
//! typeface or a spelling dictionary is — so none are shipped with this
//! program and none are made up here: a set invented here would break words
//! where no dictionary of the language says they break, which is worse than
//! not breaking them at all. What is read is whatever the machine already
//! has, in the files LibreOffice and TeX keep them in — `hyph_en_US.dic` —
//! which name their encoding on their first line and their patterns one to
//! a line after it, with the two words that say how many letters must be
//! left at each end.
//!
//! A German file has two levels, the first for the seams of compound words
//! and the second for everything else, separated by `NEXTLEVEL`; both are
//! read, and a place either allows is a place the word may break.

use std::path::{Path, PathBuf};

use crate::Error;

/// The patterns of one language, ready to be asked about a word.
#[derive(Clone, Debug, Default)]
pub struct Patterns {
    /// Each level's patterns, as the automaton that matches them.
    levels: Vec<Level>,
    /// How many letters must be left before the first break and after the
    /// last: the file's `LEFTHYPHENMIN` and `RIGHTHYPHENMIN`, or two and
    /// two, which is what the library falls back on for a file that names
    /// neither.
    pub left_min: usize,
    pub right_min: usize,
    /// The same at the seams of a compound word, for a file with two levels:
    /// `COMPOUNDLEFTHYPHENMIN` and `COMPOUNDRIGHTHYPHENMIN`, or the plain
    /// minimums when the file names none.
    pub compound_left_min: Option<usize>,
    pub compound_right_min: Option<usize>,
}

/// One level of patterns, as the machine that matches them.
///
/// # Why a machine and not a search
///
/// Liang's algorithm as TeX runs it applies every pattern that occurs
/// anywhere in the word. The library the pattern files on a machine were
/// written for — `libhyphen`, which LibreOffice hyphenates with — does not:
/// it walks the word through a trie of the patterns, falling back to the
/// longest suffix it knows when a letter leads nowhere, and applies only the
/// pattern that is exactly the state it is in. A shorter pattern that ends
/// at the same letter as a longer one is not applied; nor is one that is a
/// suffix of a longer pattern's beginning. The files were tuned to that,
/// and a German file read Liang's way breaks a word into single letters
/// where the library breaks it where the language does. So this is the
/// library's machine, checked against the library, and not TeX's search.
#[derive(Clone, Debug, Default)]
struct Level {
    nodes: Vec<Node>,
}

/// One state of the machine: a prefix of some pattern.
#[derive(Clone, Debug, Default)]
struct Node {
    /// The next state for each letter that leads on from here.
    next: Vec<(char, usize)>,
    /// The digits of the pattern that is exactly this state, if one is.
    digits: Option<Vec<u8>>,
    /// The state for the longest proper suffix of this one that is a state.
    fallback: usize,
    /// How many letters this state stands for.
    depth: usize,
}

impl Level {
    fn new() -> Self {
        Self { nodes: vec![Node::default()] }
    }

    fn is_empty(&self) -> bool {
        self.nodes.len() <= 1
    }

    /// Puts one pattern in. A later pattern of the same letters replaces an
    /// earlier one, which is what the library does with its table.
    fn insert(&mut self, letters: &[char], digits: Vec<u8>) {
        let mut at = 0usize;
        for (depth, letter) in letters.iter().enumerate() {
            let found =
                self.nodes[at].next.iter().find(|(known, _)| known == letter).map(|(_, to)| *to);
            at = match found {
                Some(to) => to,
                None => {
                    let to = self.nodes.len();
                    self.nodes.push(Node { depth: depth + 1, ..Node::default() });
                    self.nodes[at].next.push((*letter, to));
                    to
                }
            };
        }
        self.nodes[at].digits = Some(digits);
    }

    /// Works out where every state falls back to, once every pattern is in:
    /// breadth first, so a state's fallback is settled before the states
    /// under it ask for it.
    fn settle(&mut self) {
        let mut queue: std::collections::VecDeque<usize> = std::collections::VecDeque::new();
        for (_, to) in self.nodes[0].next.clone() {
            self.nodes[to].fallback = 0;
            queue.push_back(to);
        }
        while let Some(at) = queue.pop_front() {
            for (letter, to) in self.nodes[at].next.clone() {
                let mut back = self.nodes[at].fallback;
                let fallback = loop {
                    if let Some((_, found)) =
                        self.nodes[back].next.iter().find(|(known, _)| *known == letter)
                    {
                        if *found != to {
                            break *found;
                        }
                    }
                    if back == 0 {
                        break 0;
                    }
                    back = self.nodes[back].fallback;
                };
                self.nodes[to].fallback = fallback;
                queue.push_back(to);
            }
        }
    }

    /// The largest digit any applied pattern puts before each character of
    /// the dotted word, and after its last.
    fn digits_along(&self, dotted: &[char]) -> Vec<u8> {
        let mut best = vec![0u8; dotted.len() + 1];
        let mut state = 0usize;
        for (index, letter) in dotted.iter().enumerate() {
            loop {
                if let Some((_, to)) =
                    self.nodes[state].next.iter().find(|(known, _)| known == letter)
                {
                    state = *to;
                    break;
                }
                if state == 0 {
                    break;
                }
                state = self.nodes[state].fallback;
            }
            let node = &self.nodes[state];
            if let Some(digits) = &node.digits {
                // The pattern's letters end at this one; its first digit
                // stands before its first letter.
                let start = index + 1 - node.depth;
                for (offset, digit) in digits.iter().enumerate() {
                    if let Some(slot) = best.get_mut(start + offset) {
                        *slot = (*slot).max(*digit);
                    }
                }
            }
        }
        best
    }
}

impl Patterns {
    /// Reads the patterns out of a file's bytes.
    pub fn read(bytes: &[u8]) -> Result<Self, Error> {
        let text = decode(bytes)?;
        let mut patterns = Self { left_min: 2, right_min: 2, ..Self::default() };
        let mut level = Level::new();
        let mut lines = text.lines();
        // The first line is the encoding, already read.
        lines.next();
        for line in lines {
            let line = line.trim();
            if line.is_empty() || line.starts_with('%') || line.starts_with('#') {
                continue;
            }
            if let Some(rest) = line.strip_prefix("LEFTHYPHENMIN") {
                patterns.left_min = rest.trim().parse().unwrap_or(2);
                continue;
            }
            if let Some(rest) = line.strip_prefix("RIGHTHYPHENMIN") {
                patterns.right_min = rest.trim().parse().unwrap_or(2);
                continue;
            }
            if let Some(rest) = line.strip_prefix("COMPOUNDLEFTHYPHENMIN") {
                patterns.compound_left_min = rest.trim().parse().ok();
                continue;
            }
            if let Some(rest) = line.strip_prefix("COMPOUNDRIGHTHYPHENMIN") {
                patterns.compound_right_min = rest.trim().parse().ok();
                continue;
            }
            if line == "NEXTLEVEL" {
                level.settle();
                patterns.levels.push(std::mem::replace(&mut level, Level::new()));
                continue;
            }
            // The other words the format knows — the characters no hyphen
            // may follow — and the patterns that replace letters at the
            // break, which are more than a place to break and are not read:
            // a word they apply to is broken where the plain patterns allow
            // and nowhere else.
            if line.starts_with(char::is_uppercase) || line.contains('/') {
                continue;
            }
            let (letters, digits) = split_pattern(line);
            if letters.is_empty() {
                continue;
            }
            level.insert(&letters, digits);
        }
        if !level.is_empty() {
            level.settle();
            patterns.levels.push(level);
        }
        if patterns.levels.is_empty() {
            return Err(Error::Malformed("no patterns".to_owned()));
        }
        Ok(patterns)
    }

    /// Whether there is anything here at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.levels.iter().all(Level::is_empty)
    }

    /// Where a word may be broken: the byte offsets into it, in order, at
    /// each of which a hyphen may be put and the rest carried to the next
    /// line. The word is taken as it is, letters only; anything else in it
    /// and it is not a word the patterns can speak for, and nothing comes
    /// back.
    ///
    /// With two levels, the first finds the seams of a compound word and
    /// the second is asked about each part on its own, with the word's ends
    /// at the part's ends: that is what keeps a pattern from reaching across
    /// a seam and breaking one word where two meet.
    #[must_use]
    pub fn breaks(&self, word: &str) -> Vec<usize> {
        let letters: Vec<char> = word.chars().collect();
        if letters.len() < self.left_min + self.right_min
            || letters.iter().any(|character| !character.is_alphabetic())
        {
            return Vec::new();
        }
        // The word as the patterns see it: in small letters. Lowering a
        // letter can lengthen it (an İ is two characters small), and then
        // the offsets would not be the word's own: such a word is left
        // unbroken rather than broken in the wrong place.
        let small: Vec<char> =
            letters.iter().flat_map(|character| character.to_lowercase()).collect();
        if small.len() != letters.len() {
            return Vec::new();
        }
        let Some(last) = self.levels.len().checked_sub(1) else { return Vec::new() };

        // The seams, after this many letters, where the first of two levels
        // says one word meets another.
        let mut seams: Vec<usize> = Vec::new();
        if last > 0 {
            seams = self.level_breaks(0, &small, self.left_min, self.right_min);
        }
        // Then each part on its own, or the whole word when it is one part,
        // with the compound minimums at the seams and the word's at its ends.
        let mut after: Vec<usize> = seams.clone();
        let mut start = 0usize;
        for (number, end) in seams.iter().copied().chain([small.len()]).enumerate() {
            let left = if number == 0 {
                self.left_min
            } else {
                self.compound_left_min.unwrap_or(self.left_min)
            };
            let right = if end == small.len() {
                self.right_min
            } else {
                self.compound_right_min.unwrap_or(self.right_min)
            };
            after.extend(
                self.level_breaks(last, &small[start..end], left, right)
                    .into_iter()
                    .map(|at| start + at),
            );
            start = end;
        }
        after.sort_unstable();
        after.dedup();

        // A break after that many letters is at that many letters' bytes.
        let mut bytes = Vec::with_capacity(letters.len() + 1);
        let mut byte = 0usize;
        for character in &letters {
            byte += character.len_utf8();
            bytes.push(byte);
        }
        after.into_iter().filter_map(|count| bytes.get(count.checked_sub(1)?).copied()).collect()
    }

    /// The places one level of patterns allows in a run of small letters,
    /// each as the count of letters before it, with the minimums kept at
    /// both ends.
    fn level_breaks(&self, level: usize, small: &[char], left: usize, right: usize) -> Vec<usize> {
        let Some(patterns) = self.levels.get(level) else { return Vec::new() };
        if small.len() < left + right {
            return Vec::new();
        }
        // With a dot at each end, so that a pattern may say it is about the
        // start or the end.
        let mut dotted: Vec<char> = Vec::with_capacity(small.len() + 2);
        dotted.push('.');
        dotted.extend_from_slice(small);
        dotted.push('.');
        let best = patterns.digits_along(&dotted);
        (1..small.len())
            .filter(|after| *after >= left && small.len() - after >= right)
            .filter(|after| best[after + 1] % 2 == 1)
            .collect()
    }

    /// The word with a soft hyphen at every place it may break, for reading
    /// what the patterns say.
    #[must_use]
    pub fn marked(&self, word: &str) -> String {
        let mut out = String::new();
        let mut last = 0usize;
        for at in self.breaks(word) {
            out.push_str(&word[last..at]);
            out.push('-');
            last = at;
        }
        out.push_str(&word[last..]);
        out
    }
}

/// A pattern as its letters and the digits between them.
fn split_pattern(line: &str) -> (Vec<char>, Vec<u8>) {
    let mut letters = Vec::new();
    let mut digits = vec![0u8];
    for character in line.chars() {
        if let Some(digit) = character.to_digit(10) {
            if let Some(last) = digits.last_mut() {
                *last = digit as u8;
            }
        } else {
            letters.push(character);
            digits.push(0);
        }
    }
    (letters, digits)
}

/// The file as text, by the encoding its first line names.
fn decode(bytes: &[u8]) -> Result<String, Error> {
    let first_line_end = bytes.iter().position(|byte| *byte == b'\n').unwrap_or(bytes.len());
    let named = String::from_utf8_lossy(&bytes[..first_line_end]).trim().to_ascii_lowercase();
    match named.as_str() {
        "utf-8" | "utf8" => Ok(String::from_utf8_lossy(bytes).into_owned()),
        "iso8859-1" | "iso-8859-1" | "latin1" => {
            Ok(bytes.iter().map(|byte| *byte as char).collect())
        }
        other => Err(Error::UnknownEncoding(other.to_owned())),
    }
}

/// Every set of patterns installed on this machine, by the language it is
/// for: `en_US`, `de_DE`.
#[must_use]
pub fn installed() -> Vec<(String, PathBuf)> {
    let mut found: Vec<(String, PathBuf)> = Vec::new();
    for directory in search_paths() {
        let Ok(entries) = std::fs::read_dir(&directory) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|value| value.to_str()) else { continue };
            let Some(language) =
                name.strip_prefix("hyph_").and_then(|rest| rest.strip_suffix(".dic"))
            else {
                continue;
            };
            if found.iter().any(|(held, _)| held == language) {
                continue;
            }
            found.push((language.to_owned(), path));
        }
    }
    found.sort_by(|one, other| one.0.cmp(&other.0));
    found
}

/// The patterns for a language, if the machine has them: the exact language
/// and region first — `en-US` — then the language with any region, then
/// the language alone.
#[must_use]
pub fn for_language(tag: &str) -> Option<Patterns> {
    let path = path_for_language(tag)?;
    let bytes = std::fs::read(path).ok()?;
    Patterns::read(&bytes).ok()
}

/// Where the patterns for a language are, if the machine has them.
#[must_use]
pub fn path_for_language(tag: &str) -> Option<PathBuf> {
    let wanted = tag.replace('-', "_");
    let language = wanted.split('_').next().unwrap_or_default().to_ascii_lowercase();
    if language.is_empty() {
        return None;
    }
    let installed = installed();
    installed
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(&wanted))
        .or_else(|| {
            installed.iter().find(|(name, _)| {
                name.split('_').next().is_some_and(|first| first.eq_ignore_ascii_case(&language))
            })
        })
        .map(|(_, path)| path.clone())
}

/// Where patterns are kept, by operating system: beside the program, and
/// where LibreOffice's are.
fn search_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(beside) = std::env::current_exe() {
        if let Some(directory) = beside.parent() {
            paths.push(directory.join("dictionaries"));
        }
    }
    if cfg!(windows) {
        if let Ok(data) = std::env::var("APPDATA") {
            paths.push(PathBuf::from(data).join("hyphen"));
        }
    } else {
        paths.push(PathBuf::from("/usr/share/hyphen"));
    }
    paths
}

/// Reads the patterns at a path.
pub fn read_file(path: &Path) -> Result<Patterns, Error> {
    let bytes = std::fs::read(path).map_err(|error| Error::NotReadable(error.to_string()))?;
    Patterns::read(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A handful of English patterns, written out here so that what is
    /// being tested is the algorithm and not somebody else's file.
    const SMALL: &str = "\
UTF-8
% a comment
LEFTHYPHENMIN 2
RIGHTHYPHENMIN 3
hy3ph
he2n
hena4
hen5at
1na
n2at
1tio
2io
o2n
1gram
";

    fn small() -> Patterns {
        Patterns::read(SMALL.as_bytes()).expect("patterns")
    }

    #[test]
    fn a_pattern_is_split_into_its_letters_and_digits() {
        let (letters, digits) = split_pattern(".ad4der");
        assert_eq!(letters, vec!['.', 'a', 'd', 'd', 'e', 'r']);
        assert_eq!(digits, vec![0, 0, 0, 4, 0, 0, 0]);
        let (letters, digits) = split_pattern("hy3ph");
        assert_eq!(letters, vec!['h', 'y', 'p', 'h']);
        assert_eq!(digits, vec![0, 0, 3, 0, 0]);
    }

    #[test]
    fn hyphenation_breaks_where_the_odd_digits_are() {
        // Liang's own example: hy-phen-ation.
        let patterns = small();
        assert_eq!(patterns.marked("hyphenation"), "hy-phen-ation");
        assert_eq!(patterns.breaks("hyphenation"), vec![2, 6]);
        // The same in capitals, and the offsets are the word's own.
        assert_eq!(patterns.marked("Hyphenation"), "Hy-phen-ation");
    }

    #[test]
    fn the_ends_of_a_word_are_left_alone() {
        let patterns = small();
        // "gram" alone could break after g by the 1gram pattern, but two
        // letters must be left at the front and three at the back.
        assert_eq!(patterns.breaks("gram"), Vec::<usize>::new());
        assert_eq!(patterns.marked("program"), "pro-gram");
    }

    #[test]
    fn anything_that_is_not_letters_is_not_broken() {
        let patterns = small();
        assert!(patterns.breaks("hyphen-ation").is_empty());
        assert!(patterns.breaks("hyphen4").is_empty());
        assert!(patterns.breaks("").is_empty());
    }

    #[test]
    fn the_encoding_on_the_first_line_is_followed() {
        let latin = b"ISO8859-1\nLEFTHYPHENMIN 1\nRIGHTHYPHENMIN 1\n\xe4b1c\n";
        let patterns = Patterns::read(latin).expect("patterns");
        assert_eq!(patterns.marked("\u{E4}bc"), "\u{E4}b-c");
        assert!(Patterns::read(b"KOI8-R\na1b\n").is_err());
        assert!(Patterns::read(b"UTF-8\n% nothing\n").is_err());
    }

    #[test]
    fn a_second_level_adds_its_own_places() {
        let two = "\
UTF-8
LEFTHYPHENMIN 1
RIGHTHYPHENMIN 1
ab1cd
NEXTLEVEL
c1d
";
        let patterns = Patterns::read(two.as_bytes()).expect("patterns");
        assert_eq!(patterns.marked("abcd"), "ab-c-d");
    }

    #[test]
    fn the_language_and_its_region_are_looked_for_in_turn() {
        // Only the naming is tested here: what is installed depends on the
        // machine, and a test cannot rely on it.
        let none = path_for_language("");
        assert!(none.is_none());
    }
}
