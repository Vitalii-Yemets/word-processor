//! Turns the Unicode Character Database into the tables this program searches.
//!
//! # Why a generator
//!
//! The tables for bidirectionality, line breaking, segmentation and
//! normalization were written by hand, from the standard, one range at a time.
//! A hand-written table is a subset: it holds the characters somebody thought
//! of, and everything else falls to a default that is right for Latin and
//! wrong for whatever was forgotten. A character in a script nobody added is
//! then laid out as though it were English.
//!
//! So the tables are generated instead, from the database itself, and the
//! result is committed. Committing it is the point: the program depends on no
//! files at build time and on no crates at all, and the tables can be read and
//! reviewed like any other source.
//!
//! # Where the database comes from
//!
//! Perl carries the whole of it, already parsed into files of ranges, and the
//! build image carries Perl. Nothing is downloaded and nothing is installed.
//! The format is Perl's own: a heredoc of tab-separated ranges, and a `missing`
//! line saying what everything absent from the file maps to.
//!
//! # Running it
//!
//! `docker compose run --rm dev bash tools/generate-unicode-tables.sh`

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let mut arguments = std::env::args().skip(1);
    let ucd = PathBuf::from(
        arguments.next().unwrap_or_else(|| "/usr/share/perl/5.36.0/unicore".to_string()),
    );
    let root = PathBuf::from(arguments.next().unwrap_or_else(|| ".".to_string()));

    let version = fs::read_to_string(ucd.join("version"))
        .expect("the database says which version it is")
        .trim()
        .to_string();
    println!("Unicode {version}, from {}", ucd.display());

    bidi(&ucd, &root, &version);
    breaking(&ucd, &root, &version);
    segmentation(&ucd, &root, &version);
    normalization(&ucd, &root, &version);
}

// ---------------------------------------------------------------------------
// Reading what Perl wrote
// ---------------------------------------------------------------------------

/// The body of one of Perl's tables: the lines between the heredoc markers.
fn body(path: &Path) -> Vec<String> {
    let text =
        fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let mut lines = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        if !inside {
            inside = line.starts_with("return <<");
            continue;
        }
        if line == "END" {
            break;
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        lines.push(line.to_string());
    }
    assert!(!lines.is_empty(), "{} held nothing", path.display());
    lines
}

/// A map: every line is a range of code points and the value they map to.
fn map(path: &Path) -> Vec<(u32, u32, String)> {
    let mut entries: Vec<(u32, u32, String)> = body(path)
        .iter()
        .map(|line| {
            let mut fields = line.splitn(3, '\t');
            let first = hex(fields.next().expect("a first code point"));
            let second = fields.next().unwrap_or("");
            let last = if second.is_empty() { first } else { hex(second) };
            (first, last, fields.next().unwrap_or("").to_string())
        })
        .collect();
    entries.sort_by_key(|entry| entry.0);
    entries
}

/// What a map gives every character it does not list.
fn absent(path: &Path) -> String {
    let text =
        fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    for line in text.lines() {
        if let Some((_, rest)) = line.split_once("{'missing'} = ") {
            let value = rest.trim_start().trim_start_matches('\'');
            let end = value.find('\'').unwrap_or(value.len());
            return value[..end].to_string();
        }
    }
    panic!("{} does not say what it leaves out", path.display())
}

/// A set: Perl writes one as the boundaries at which membership changes.
fn set(path: &Path) -> Vec<(u32, u32)> {
    let numbers: Vec<u32> = body(path)
        .iter()
        .filter(|line| !line.starts_with('V'))
        .map(|line| line.trim().parse().expect("a code point"))
        .collect();

    let mut ranges = Vec::new();
    let mut index = 0;
    while index < numbers.len() {
        let first = numbers[index];
        let last = numbers.get(index + 1).map_or(0x10FFFF, |after| after - 1);
        ranges.push((first, last));
        index += 2;
    }
    ranges
}

fn hex(text: &str) -> u32 {
    u32::from_str_radix(text.trim(), 16).unwrap_or_else(|_| panic!("not a code point: {text:?}"))
}

// ---------------------------------------------------------------------------
// Turning it into runs
// ---------------------------------------------------------------------------

/// Every code point's value, as the runs it falls into.
///
/// A run is written as where it begins, and it lasts until the next one: what a
/// character is is the value of the last run beginning at or before it. That is
/// half the size of writing both ends, and it is also the reason the table
/// covers every character rather than most of them — there is no gap left for a
/// character to fall into.
fn runs(mapped: &[(u32, u32, String)], default: &str) -> Vec<(u32, String)> {
    let mut out: Vec<(u32, String)> = Vec::new();
    let mut next = 0u32;

    for (first, last, value) in mapped {
        assert!(*first >= next, "the database overlaps itself at {first:X}");
        if *first > next && out.last().map_or(true, |(_, held)| held != default) {
            out.push((next, default.to_string()));
        }
        if out.last().map_or(true, |(_, held)| held != value) {
            out.push((*first, value.clone()));
        }
        next = last + 1;
    }

    if next <= 0x10FFFF && out.last().map_or(true, |(_, held)| held != default) {
        out.push((next, default.to_string()));
    }
    assert_eq!(out[0].0, 0, "the runs do not begin at the first character");
    out
}

/// Renames each value, splitting a range wherever the new name depends on which
/// characters the range holds.
fn renamed(
    mapped: &[(u32, u32, String)],
    name: &dyn Fn(&str, u32) -> String,
) -> Vec<(u32, u32, String)> {
    let mut out = Vec::new();
    for (first, last, value) in mapped {
        let mut at = *first;
        while at <= *last {
            let called = name(value, at);
            let mut end = at;
            while end < *last && name(value, end + 1) == called {
                end += 1;
            }
            out.push((at, end, called));
            at = end + 1;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Writing Rust
// ---------------------------------------------------------------------------

fn preamble(doc: &str, version: &str) -> String {
    format!(
        "{doc}//!\n\
         //! Generated from the Unicode Character Database, version {version}, by\n\
         //! `tools/generate-unicode-tables.sh`. Do not edit this file: the generator\n\
         //! is the source, and anything written here by hand is lost the next time it\n\
         //! runs.\n\n"
    )
}

/// A table of runs, each value named as a variant of one type.
fn table_of_runs(doc: &str, name: &str, kind: &str, runs: &[(u32, String)]) -> String {
    let mut text = format!("{doc}static {name}: &[(u32, {kind})] = &[\n");
    for (first, value) in runs {
        let _ = writeln!(text, "    (0x{first:04X}, {kind}::{value}),");
    }
    text.push_str("];\n\n");
    text
}

/// The search that reads one: the last run beginning at or before the
/// character.
fn search_of_runs(doc: &str, function: &str, name: &str, kind: &str) -> String {
    format!(
        "{doc}pub(crate) fn {function}(character: char) -> {kind} {{\n    \
         let code = character as u32;\n    \
         let after = {name}.partition_point(|(first, _)| *first <= code);\n    \
         {name}[after - 1].1\n\
         }}\n\n"
    )
}

/// The one thing a generated table has to be checked for: that it is in order
/// and begins at the first character, which is what the searches above take for
/// granted and what makes "every character" true.
fn order_check(names: &[&str]) -> String {
    let checks: Vec<String> = names
        .iter()
        .map(|name| {
            format!("{name}[0].0 == 0 && {name}.windows(2).all(|pair| pair[0].0 < pair[1].0)")
        })
        .collect();
    format!(
        "/// Whether the tables are in order and leave no character out, which is\n\
         /// what the searches above take for granted.\n\
         #[cfg(test)]\n\
         pub(crate) fn in_order() -> bool {{\n    {}\n}}\n\n",
        checks.join("\n        && ")
    )
}

/// A table of pairs of code points.
fn table_of_pairs(doc: &str, name: &str, visibility: &str, pairs: &[(u32, u32)]) -> String {
    let mut text = format!("{doc}{visibility}static {name}: &[(u32, u32)] = &[\n");
    for (left, right) in pairs {
        let _ = writeln!(text, "    (0x{left:04X}, 0x{right:04X}),");
    }
    text.push_str("];\n\n");
    text
}

fn write_out(path: PathBuf, text: &str) {
    fs::write(&path, text).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    println!("  {:<38} {:>6} lines", path.display().to_string(), text.lines().count());
}

// ---------------------------------------------------------------------------
// The bidirectional algorithm
// ---------------------------------------------------------------------------

fn bidi(ucd: &Path, root: &Path, version: &str) {
    let file = ucd.join("To/Bc.pl");
    let named = renamed(&map(&file), &|value, _| {
        const KNOWN: &[&str] = &[
            "L", "R", "AL", "EN", "ES", "ET", "AN", "CS", "NSM", "BN", "B", "S", "WS", "ON", "LRE",
            "RLE", "LRO", "RLO", "PDF", "LRI", "RLI", "FSI", "PDI",
        ];
        assert!(KNOWN.contains(&value), "unknown bidirectional class {value}");
        value.to_string()
    });
    let classes = runs(&named, &absent(&file));

    // The mirrors. Unicode lists both ends of every pair, so the table is
    // already symmetrical and one search answers either direction.
    let mut mirrors: Vec<(u32, u32)> = Vec::new();
    for (first, last, value) in map(&ucd.join("To/Bmg.pl")) {
        assert_eq!(first, last, "a range of mirrored characters at {first:X}");
        mirrors.push((first, hex(&value)));
    }
    mirrors.sort();

    // And which end of which pair a bracket is, which rule N0 needs.
    let sides: BTreeMap<u32, String> = map(&ucd.join("To/Bpt.pl"))
        .into_iter()
        .flat_map(|(first, last, value)| (first..=last).map(move |code| (code, value.clone())))
        .collect();
    let paired: BTreeMap<u32, u32> = map(&ucd.join("To/Bpb.pl"))
        .into_iter()
        .flat_map(|(first, last, value)| (first..=last).map(move |code| (code, hex(&value))))
        .collect();
    let brackets: Vec<(u32, u32)> = sides
        .iter()
        .filter(|(_, side)| *side == "o")
        .map(|(open, _)| (*open, paired[open]))
        .collect();

    let mut text = preamble(
        "//! Which bidirectional class each character belongs to, which characters\n\
         //! are drawn the other way round in right-to-left text, and which of those\n\
         //! are the two ends of a bracket.\n",
        version,
    );
    text.push_str("use crate::class::Class;\n\n");
    text.push_str(&table_of_runs("/// Where each run of one class begins.\n", "CLASSES", "Class", &classes));
    text.push_str(&search_of_runs("/// The class of one character.\n", "class_of", "CLASSES", "Class"));
    text.push_str(&table_of_pairs(
        "/// Every character drawn as another one where the text reads right to\n\
         /// left, and the one it is drawn as. Unicode lists both ends of every\n\
         /// pair, so this answers the question either way round.\n",
        "MIRRORS",
        "",
        &mirrors,
    ));
    text.push_str(
        "/// The character drawn in place of this one in right-to-left text.\n\
         pub(crate) fn mirror_of(code: u32) -> Option<u32> {\n    \
         MIRRORS.binary_search_by_key(&code, |(from, _)| *from).ok().map(|at| MIRRORS[at].1)\n\
         }\n\n",
    );
    text.push_str(&table_of_pairs(
        "/// The brackets, each written as the pair it belongs to: the one that\n\
         /// opens and the one that closes. Only the brackets — `<` is drawn\n\
         /// mirrored but opens nothing.\n",
        "BRACKETS",
        "pub(crate) ",
        &brackets,
    ));

    text.push_str(&order_check(&["CLASSES"]));
    write_out(root.join("crates/wp-bidi/src/tables.rs"), &text);
}

// ---------------------------------------------------------------------------
// Line breaking
// ---------------------------------------------------------------------------

/// The classes of [UAX #14], every one of them by the standard's own name.
///
/// Nothing is folded: the standard's pair table is written against these,
/// and the rules in `wp-break` are the standard's rules. The one name changed
/// is the database's word for a character it has not assigned, which the
/// standard calls XX.
///
/// [UAX #14]: https://www.unicode.org/reports/tr14/
fn breaking_class(value: &str) -> String {
    const KNOWN: &[&str] = &[
        "BK", "CR", "LF", "CM", "NL", "SG", "WJ", "ZW", "GL", "SP", "ZWJ", "B2", "BA", "BB", "HY",
        "CB", "CL", "CP", "EX", "IN", "NS", "OP", "QU", "IS", "NU", "PO", "PR", "SY", "AI", "AL",
        "CJ", "EB", "EM", "H2", "H3", "HL", "ID", "JL", "JV", "JT", "RI", "SA", "XX",
    ];
    let name = if value == "Unknown" { "XX" } else { value };
    assert!(KNOWN.contains(&name), "unknown line break class {value}");
    name.to_string()
}

/// Whether a character is East Asian wide, full-width or half-width, which
/// is the one thing LB30 asks about a bracket.
fn width_class(value: &str) -> String {
    match value {
        "W" | "F" | "H" => "Wide".to_string(),
        "Na" | "A" | "N" | "Neutral" => "Narrow".to_string(),
        _ => panic!("unknown East Asian width {value}"),
    }
}

/// The characters of a map that carry one value and fall inside a set, as
/// ranges: what LB1 and LB30b ask about, worked out once here rather than
/// from two tables at run time.
fn within(mapped: &[(u32, u32, String)], value: &str, set: &[(u32, u32)]) -> Vec<(u32, u32)> {
    let mut out: Vec<(u32, u32)> = Vec::new();
    for (first, last, held) in mapped {
        if held != value {
            continue;
        }
        for (from, to) in set {
            let (low, high) = ((*first).max(*from), (*last).min(*to));
            if low > high {
                continue;
            }
            match out.last_mut() {
                Some((_, end)) if *end + 1 == low => *end = high,
                _ => out.push((low, high)),
            }
        }
    }
    out.sort_unstable();
    out
}

fn breaking(ucd: &Path, root: &Path, version: &str) {
    let file = ucd.join("To/Lb.pl");
    let mapped = map(&file);
    let named = renamed(&mapped, &|value, _| breaking_class(value));
    let classes = runs(&named, &breaking_class(&absent(&file)));

    let widths_file = ucd.join("To/Ea.pl");
    let widths = renamed(&map(&widths_file), &|value, _| width_class(value));
    let widths = runs(&widths, &width_class(&absent(&widths_file)));

    // LB1: the South East Asian letters that are marks become CM, and the
    // rest AL. Which are marks is the general category.
    let mut marks = set(&ucd.join("lib/Gc/Mn.pl"));
    marks.extend(set(&ucd.join("lib/Gc/Mc.pl")));
    marks.sort_unstable();
    let complex_marks = within(&mapped, "SA", &marks);

    // LB30b: a pictograph the database has reserved room for but not yet
    // assigned takes a skin tone the way an assigned one does.
    let unassigned = set(&ucd.join("lib/Gc/Cn.pl"));
    let pictographs: Vec<(u32, u32, String)> = set(&ucd.join("lib/ExtPict/Y.pl"))
        .into_iter()
        .map(|(first, last)| (first, last, "Y".to_string()))
        .collect();
    let reserved = within(&pictographs, "Y", &unassigned);

    let mut text = preamble(
        "//! What kind of thing each character is, for the rules that say where a\n\
         //! line may be broken: the classes of [UAX #14], by the standard's own\n\
         //! names, and the two sets its rules ask about beside them.\n\
         //!\n\
         //! [UAX #14]: https://www.unicode.org/reports/tr14/\n",
        version,
    );
    text.push_str("use crate::{Class, Width};\n\n");
    text.push_str(&table_of_runs("/// Where each run of one class begins.\n", "CLASSES", "Class", &classes));
    text.push_str(&search_of_runs("/// The class of one character.\n", "class_of", "CLASSES", "Class"));
    text.push_str(&table_of_runs(
        "/// Where each run of one East Asian width begins, wide or not.\n",
        "WIDTHS",
        "Width",
        &widths,
    ));
    text.push_str(&search_of_runs(
        "/// Whether one character is East Asian wide, full-width or half-width.\n",
        "width_of",
        "WIDTHS",
        "Width",
    ));
    text.push_str(&table_of_pairs(
        "/// The letters of the South East Asian scripts that are marks — a vowel\n\
         /// written above its consonant, a tone mark — which LB1 reads as CM.\n",
        "COMPLEX_MARKS",
        "pub(crate) ",
        &complex_marks,
    ));
    text.push_str(&table_of_pairs(
        "/// The code points reserved for pictographs and not yet assigned, which\n\
         /// LB30b keeps a skin tone with.\n",
        "RESERVED_PICTOGRAPHS",
        "pub(crate) ",
        &reserved,
    ));
    text.push_str(
        "/// Whether a code point falls in one of a table's ranges.\n\
         pub(crate) fn within(table: &[(u32, u32)], code: u32) -> bool {\n    \
         let after = table.partition_point(|(first, _)| *first <= code);\n    \
         after > 0 && table[after - 1].1 >= code\n\
         }\n\n",
    );

    text.push_str(&order_check(&["CLASSES", "WIDTHS"]));
    write_out(root.join("crates/wp-break/src/tables.rs"), &text);
}

// ---------------------------------------------------------------------------
// Segmentation
// ---------------------------------------------------------------------------

/// The emoji are a value of neither property, and both sets of rules ask about
/// them, so Perl writes them as a value of their own wherever the property
/// would otherwise say "anything else". That is exactly the overlay these rules
/// want, and it is taken as it stands.
fn grapheme_class(value: &str) -> String {
    let name = match value {
        "Other" => "Other",
        "ExtPict_XX" => "Pictographic",
        "CR" => "CarriageReturn",
        "LF" => "LineFeed",
        "Control" => "Control",
        "Extend" => "Extend",
        "ZWJ" => "Joiner",
        "Regional_Indicator" => "Regional",
        "Prepend" => "Prepend",
        "SpacingMark" => "SpacingMark",
        "L" => "Leading",
        "V" => "Vowel",
        "T" => "Trailing",
        "LV" => "LeadingVowel",
        "LVT" => "LeadingVowelTrailing",
        _ => panic!("unknown grapheme cluster class {value}"),
    };
    name.to_string()
}

/// The same for the word break property, with one thing taken back.
///
/// Perl tailors the property: where the standard says which spaces hold a run
/// of whitespace together, Perl substitutes its own idea of horizontal
/// whitespace, which is a tab and two no-break spaces wider. The standard's own
/// set is in the database as well, and `space` is it — a tailoring of somebody
/// else's is not what this program wants to agree with.
///
/// The five emoji that are also letters — the information source, the circled
/// M, the squared A and B — keep the letter, which is what the standard says
/// they are. Nothing is lost by it: the one rule that asks about an emoji
/// there asks about what follows a zero width joiner, and a joiner between two
/// letters is read over in any case.
fn word_class(value: &str, code: u32, space: &[(u32, u32)]) -> String {
    let name = match value {
        "Other" => "Other",
        "ExtPict_XX" => "Pictographic",
        "CR" => "CarriageReturn",
        "LF" => "LineFeed",
        "Newline" => "Newline",
        // A mark and a character that draws nothing are both read over as
        // though they were not there, which is the one rule either takes part
        // in.
        "Extend" | "Format" => "Ignored",
        "ZWJ" => "Joiner",
        "Regional_Indicator" => "Regional",
        "Perl_Tailored_HSpace" => {
            if space.iter().any(|(first, last)| code >= *first && code <= *last) {
                "Space"
            } else {
                "Other"
            }
        }
        "ALetter" | "ExtPict_LE" => "Letter",
        "Hebrew_Letter" => "Hebrew",
        "Katakana" => "Katakana",
        "Numeric" => "Numeric",
        "Single_Quote" => "SingleQuote",
        "Double_Quote" => "DoubleQuote",
        "MidLetter" => "MidLetter",
        "MidNum" => "MidNumber",
        "MidNumLet" => "MidBoth",
        "ExtendNumLet" => "Connector",
        _ => panic!("unknown word break class {value}"),
    };
    name.to_string()
}

fn segmentation(ucd: &Path, root: &Path, version: &str) {
    let grapheme_file = ucd.join("To/GCB.pl");
    let named = renamed(&map(&grapheme_file), &|value, _| grapheme_class(value));
    let graphemes = runs(&named, &grapheme_class(&absent(&grapheme_file)));

    let space = set(&ucd.join("lib/WB/WSegSpac.pl"));
    let word_file = ucd.join("To/WB.pl");
    let named = renamed(&map(&word_file), &|value, code| word_class(value, code, &space));
    let words = runs(&named, &word_class(&absent(&word_file), 0, &space));

    let mut text = preamble(
        "//! What kind of thing each character is, for the rules that say where one\n\
         //! character ends and the next begins, and one word and the next.\n\
         //!\n\
         //! Two properties of [UAX #29], with the emoji laid over both: neither\n\
         //! property names them, and both sets of rules ask about them.\n\
         //!\n\
         //! [UAX #29]: https://www.unicode.org/reports/tr29/\n",
        version,
    );
    text.push_str("use crate::grapheme::Class as Grapheme;\nuse crate::word::Class as Word;\n\n");
    text.push_str(&table_of_runs(
        "/// Where each run of one grapheme cluster class begins.\n",
        "GRAPHEMES",
        "Grapheme",
        &graphemes,
    ));
    text.push_str(&search_of_runs(
        "/// The grapheme cluster class of one character.\n",
        "grapheme_class_of",
        "GRAPHEMES",
        "Grapheme",
    ));
    text.push_str(&table_of_runs(
        "/// Where each run of one word break class begins.\n",
        "WORDS",
        "Word",
        &words,
    ));
    text.push_str(&search_of_runs(
        "/// The word break class of one character.\n",
        "word_class_of",
        "WORDS",
        "Word",
    ));

    // And one property that is not a boundary at all, kept here because this is
    // where the character tables live: whether a character is drawn as a
    // coloured picture by default rather than as a letter.
    let emoji = set(&ucd.join("lib/EPres/Y.pl"));
    text.push_str(&table_of_pairs(
        "/// The characters a reader expects to see as a coloured picture rather\n\
         /// than as a letter. A rocket is one; a bare heart is not, and is drawn\n\
         /// in the colour of the text until the writer asks otherwise.\n",
        "EMOJI",
        "",
        &emoji,
    ));
    text.push_str(
        "/// Whether a character is drawn as a picture by default.\n\
         pub(crate) fn drawn_as_emoji(character: char) -> bool {\n    \
         let code = character as u32;\n    \
         let after = EMOJI.partition_point(|(first, _)| *first <= code);\n    \
         after > 0 && EMOJI[after - 1].1 >= code\n\
         }\n\n",
    );

    text.push_str(&order_check(&["GRAPHEMES", "WORDS"]));
    write_out(root.join("crates/wp-segment/src/tables.rs"), &text);
}

// ---------------------------------------------------------------------------
// Normalization
// ---------------------------------------------------------------------------

fn normalization(ucd: &Path, root: &Path, version: &str) {
    let file = ucd.join("CombiningClass.pl");
    let combining = runs(&map(&file), &absent(&file));

    // The canonical decompositions: the ones Unicode says are the same text
    // written differently, as against the compatibility ones, which are not.
    let mut pairs: Vec<(u32, u32, u32)> = Vec::new();
    let mut singles: Vec<(u32, u32)> = Vec::new();
    for (first, last, value) in map(&ucd.join("Decomposition.pl")) {
        if value.starts_with('<') {
            continue;
        }
        let parts: Vec<u32> = value.split_whitespace().map(hex).collect();
        match parts.as_slice() {
            // A range of them is real: the compatibility ideographs hold the
            // same character twice over, and both are the same unified one.
            [only] => singles.extend((first..=last).map(|made| (made, *only))),
            [base, mark] => {
                assert_eq!(first, last, "a range of two-part decompositions at {first:X}");
                pairs.push((first, *base, *mark));
            }
            _ => panic!("a canonical decomposition of {} characters at {first:X}", parts.len()),
        }
    }

    // Putting them back together is not simply the reverse. Unicode keeps a
    // list of the characters that must never be made again, because making one
    // would change what the text says, or would undo a decision a later version
    // of the standard took back.
    let excluded = set(&ucd.join("lib/CompEx/Y.pl"));
    let mut composable: Vec<(u32, u32, u32)> = pairs
        .iter()
        .filter(|(made, _, _)| !excluded.iter().any(|(first, last)| made >= first && made <= last))
        .map(|(made, base, mark)| (*base, *mark, *made))
        .collect();
    composable.sort();

    let mut text = preamble(
        "//! Which letter and which mark each accented character is made of, which\n\
         //! of those may be put back together, and where each mark is drawn.\n\
         //!\n\
         //! Only the canonical decompositions: the ones Unicode says are the same\n\
         //! text written two ways. The compatibility ones — a superscript two\n\
         //! written as a two, a ligature written as its letters — say something\n\
         //! different about the text and are not here.\n",
        version,
    );

    let mut classes = format!(
        "/// Where each run of one combining class begins: the number says where a\n\
         /// mark is drawn, and nought means the character is not a mark at all.\n\
         static COMBINING: &[(u32, u8)] = &[\n"
    );
    for (first, value) in &combining {
        let _ = writeln!(classes, "    (0x{first:04X}, {value}),");
    }
    classes.push_str("];\n\n");
    text.push_str(&classes);
    text.push_str(
        "/// Where a mark is drawn, as the number the standard gives it.\n\
         pub(crate) fn combining_class(character: char) -> u8 {\n    \
         let code = character as u32;\n    \
         let after = COMBINING.partition_point(|(first, _)| *first <= code);\n    \
         COMBINING[after - 1].1\n\
         }\n\n",
    );

    let _ = write!(
        text,
        "/// The characters written as a letter with a mark drawn on it, in the\n\
         /// order a search wants them.\n\
         pub(crate) static PAIRS: &[(char, char, char)] = &[\n"
    );
    for (made, base, mark) in &pairs {
        let _ = writeln!(
            text,
            "    ('\\u{{{made:04X}}}', '\\u{{{base:04X}}}', '\\u{{{mark:04X}}}'),"
        );
    }
    text.push_str("];\n\n");

    let _ = write!(
        text,
        "/// The characters that are simply another character: the angstrom sign is\n\
         /// an A with a ring, the ohm sign is an omega.\n\
         pub(crate) static SINGLES: &[(char, char)] = &[\n"
    );
    for (made, only) in &singles {
        let _ = writeln!(text, "    ('\\u{{{made:04X}}}', '\\u{{{only:04X}}}'),");
    }
    text.push_str("];\n\n");

    let _ = write!(
        text,
        "/// A letter, a mark, and the character the two make together. The pairs\n\
         /// Unicode forbids putting back together are not here.\n\
         pub(crate) static COMPOSABLE: &[(char, char, char)] = &[\n"
    );
    for (base, mark, made) in &composable {
        let _ = writeln!(
            text,
            "    ('\\u{{{base:04X}}}', '\\u{{{mark:04X}}}', '\\u{{{made:04X}}}'),"
        );
    }
    text.push_str("];\n");

    text.push_str(&order_check(&["COMBINING"]));
    write_out(root.join("crates/wp-normal/src/table.rs"), &text);
}
