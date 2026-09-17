//! The standard's own answers, run against the text engine.
//!
//! # Why this is a command and not a test
//!
//! The suites are six files the Unicode Consortium publishes, and nothing in
//! the build image carries them: Perl's character database is character data,
//! and these are test data. The build downloads nothing and installs nothing,
//! which is the rule the whole project is built under, so the files come from
//! whoever wants to run them and live in a directory git ignores — the same
//! answer the corpus gets in [`crate::corpus`], for the same reason applied to
//! different data. A suite whose file is not there is reported missing, not
//! passed and not failed.
//!
//! What the repository does carry is everything else: the parsers, the
//! runners and tests of both, written against fixtures of a few lines each
//! that the tests type out themselves. So the harness is proven without the
//! data, and the data proves the engine.
//!
//! # What each suite asks
//!
//! - `BidiTest.txt` — the bidirectional algorithm over sequences of character
//!   *classes* rather than characters, each class standing for itself. Every
//!   line is run once for each paragraph direction it names.
//! - `BidiCharacterTest.txt` — the same algorithm over real text, with the
//!   resolved paragraph level, every character's level, and the order they are
//!   drawn in.
//! - `GraphemeBreakTest.txt` — where one character ends and the next begins.
//! - `WordBreakTest.txt` — where a word does.
//! - `LineBreakTest.txt` — where a line may be broken.
//! - `NormalizationTest.txt` — NFC and NFD, both ways round. The NFKC and
//!   NFKD columns are read past: compatibility normalization changes what the
//!   text says and this program does not do it.
//!
//! # A number, not a gate
//!
//! The command reports how many cases each suite ran and how many passed, and
//! writes the totals to `unicode/conformance.log` so they can be seen to
//! move. It does not fail on a failing case. Line breaking here keeps
//! seventeen classes where the standard has about forty, and that is written
//! down in the roadmap rather than discovered again by a red build every
//! morning; what is wanted from this is the number and the first few lines
//! that produce it.

use std::path::{Path, PathBuf};

/// The file every run leaves its totals in.
pub const HISTORY: &str = "conformance.log";

/// How many failing lines are worth printing from one suite.
const SHOWN: usize = 3;

/// One of the standard's test files.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Suite {
    Bidi,
    BidiCharacter,
    GraphemeBreak,
    WordBreak,
    LineBreak,
    Normalization,
}

impl Suite {
    /// All of them, in the order they are reported.
    pub const ALL: [Self; 6] = [
        Self::Bidi,
        Self::BidiCharacter,
        Self::GraphemeBreak,
        Self::WordBreak,
        Self::LineBreak,
        Self::Normalization,
    ];

    /// What the file is called, which is what it is called everywhere.
    #[must_use]
    pub fn file(self) -> &'static str {
        match self {
            Self::Bidi => "BidiTest.txt",
            Self::BidiCharacter => "BidiCharacterTest.txt",
            Self::GraphemeBreak => "GraphemeBreakTest.txt",
            Self::WordBreak => "WordBreakTest.txt",
            Self::LineBreak => "LineBreakTest.txt",
            Self::Normalization => "NormalizationTest.txt",
        }
    }

    /// Which part of the engine it holds to account.
    #[must_use]
    pub fn about(self) -> &'static str {
        match self {
            Self::Bidi | Self::BidiCharacter => "the bidirectional algorithm",
            Self::GraphemeBreak => "where one character ends and the next begins",
            Self::WordBreak => "where a word ends",
            Self::LineBreak => "where a line may be broken",
            Self::Normalization => "NFC and NFD",
        }
    }
}

/// How one suite went.
#[derive(Clone, Debug, Default)]
pub struct Outcome {
    /// How many cases the file held that this program knows how to run.
    pub ran: usize,
    /// And how many of them came out as the standard says.
    pub passed: usize,
    /// The first few that did not, as lines worth printing.
    pub failures: Vec<String>,
    /// Lines that could not be read as a case at all, which is a hole in the
    /// parser rather than a failure of the engine and is counted apart.
    pub unread: usize,
}

impl Outcome {
    /// Records one case.
    fn case(&mut self, line: usize, passed: bool, said: impl FnOnce() -> String) {
        self.ran += 1;
        if passed {
            self.passed += 1;
        } else if self.failures.len() < SHOWN {
            self.failures.push(format!("line {line}: {}", said()));
        }
    }

    /// How many passed, out of a hundred.
    #[must_use]
    pub fn percent(&self) -> f32 {
        if self.ran == 0 {
            return 100.0;
        }
        #[allow(clippy::cast_precision_loss)]
        let share = self.passed as f32 / self.ran as f32;
        share * 100.0
    }
}

/// What came of one suite.
#[derive(Clone, Debug)]
pub enum Report {
    /// Nobody has put the file there. Not a pass and not a failure.
    Missing,
    /// It was read and run.
    Ran(Outcome),
    /// It is there and could not be read.
    Unreadable(String),
}

/// Runs whichever suites are in a directory.
#[must_use]
pub fn run(directory: &Path) -> Vec<(Suite, Report)> {
    Suite::ALL
        .into_iter()
        .map(|suite| {
            let path: PathBuf = directory.join(suite.file());
            let report = match std::fs::read_to_string(&path) {
                Ok(text) => Report::Ran(run_text(suite, &text)),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Report::Missing,
                Err(error) => Report::Unreadable(error.to_string()),
            };
            (suite, report)
        })
        .collect()
}

/// Runs one suite over the text of its file.
#[must_use]
pub fn run_text(suite: Suite, text: &str) -> Outcome {
    match suite {
        Suite::Bidi => bidi_classes(text),
        Suite::BidiCharacter => bidi_characters(text),
        Suite::GraphemeBreak | Suite::WordBreak | Suite::LineBreak => breaking(suite, text),
        Suite::Normalization => normalization(text),
    }
}

/// A line with its comment taken off, and nothing else done to it.
fn body(line: &str) -> &str {
    line.split('#').next().unwrap_or("").trim()
}

/// Hexadecimal code points separated by spaces, as text.
fn code_points(field: &str) -> Option<String> {
    let mut out = String::new();
    for word in field.split_whitespace() {
        out.push(char::from_u32(u32::from_str_radix(word, 16).ok()?)?);
    }
    Some(out)
}

// --- Where things may be broken ---------------------------------------------

/// The text of a break test and every offset a break is expected at.
///
/// The file writes a line as its characters with `÷` where a break belongs
/// and `×` where one does not, so the offsets fall out of building the text:
/// a `÷` belongs wherever the text has got to.
fn expected_breaks(line: &str) -> Option<(String, Vec<usize>)> {
    let mut text = String::new();
    let mut breaks = Vec::new();
    for token in body(line).split_whitespace() {
        match token {
            "\u{00F7}" => breaks.push(text.len()),
            "\u{00D7}" => {}
            hex => text.push(char::from_u32(u32::from_str_radix(hex, 16).ok()?)?),
        }
    }
    if text.is_empty() {
        return None;
    }
    Some((text, breaks))
}

fn breaking(suite: Suite, text: &str) -> Outcome {
    let mut outcome = Outcome::default();
    for (number, line) in text.lines().enumerate() {
        if body(line).is_empty() {
            continue;
        }
        let Some((text, expected)) = expected_breaks(line) else {
            outcome.unread += 1;
            continue;
        };
        let (ours, wanted) = match suite {
            Suite::GraphemeBreak => (wp_segment::character_boundaries(&text), expected),
            Suite::WordBreak => (wp_segment::word_boundaries(&text), expected),
            // A break at either end of a line moves nothing, so the engine
            // does not offer one and the file's outermost marks are dropped
            // before the two are compared.
            _ => (
                wp_break::opportunities(&text),
                expected.into_iter().filter(|at| *at != 0 && *at != text.len()).collect(),
            ),
        };
        outcome.case(number + 1, ours == wanted, || {
            format!("breaks at {ours:?}, the standard says {wanted:?}")
        });
    }
    outcome
}

// --- NFC and NFD ------------------------------------------------------------

fn normalization(text: &str) -> Outcome {
    let mut outcome = Outcome::default();
    for (number, line) in text.lines().enumerate() {
        let body = body(line);
        if body.is_empty() || body.starts_with('@') {
            continue;
        }
        let fields: Vec<&str> = body.split(';').collect();
        if fields.len() < 5 {
            outcome.unread += 1;
            continue;
        }
        let Some(source) = code_points(fields[0]) else {
            outcome.unread += 1;
            continue;
        };
        let (Some(composed), Some(decomposed)) = (code_points(fields[1]), code_points(fields[2]))
        else {
            outcome.unread += 1;
            continue;
        };

        // The file's own rule: the composed form is what NFC makes of the
        // source, of itself and of the decomposed form, and likewise for NFD.
        // Running all three catches a normalizer that only works one way.
        for from in [&source, &composed, &decomposed] {
            let ours = wp_normal::compose(from);
            outcome.case(number + 1, ours == composed, || {
                format!(
                    "NFC({}) came out {}, wanted {}",
                    named(from),
                    named(&ours),
                    named(&composed)
                )
            });
            let ours = wp_normal::decompose(from);
            outcome.case(number + 1, ours == decomposed, || {
                format!(
                    "NFD({}) came out {}, wanted {}",
                    named(from),
                    named(&ours),
                    named(&decomposed)
                )
            });
        }
    }
    outcome
}

/// Text written as its code points, which is how these files read.
fn named(text: &str) -> String {
    text.chars().map(|character| format!("{:04X}", character as u32)).collect::<Vec<_>>().join(" ")
}

// --- The bidirectional algorithm --------------------------------------------

/// A character standing for each of the standard's bidirectional classes.
///
/// `BidiTest.txt` is written in classes rather than characters, and an engine
/// that works on text needs one character per class to run it. Any character
/// of the class will do, since the algorithm sees nothing else about it.
fn standing_for(class: &str) -> Option<char> {
    Some(match class {
        "L" => 'A',
        "R" => '\u{05D0}',
        "AL" => '\u{0627}',
        "EN" => '0',
        "ES" => '+',
        "ET" => '#',
        "AN" => '\u{0660}',
        "CS" => ',',
        "NSM" => '\u{0300}',
        "BN" => '\u{00AD}',
        "B" => '\u{2029}',
        "S" => '\u{0009}',
        "WS" => ' ',
        "ON" => '!',
        "LRE" => '\u{202A}',
        "RLE" => '\u{202B}',
        "PDF" => '\u{202C}',
        "LRO" => '\u{202D}',
        "RLO" => '\u{202E}',
        "LRI" => '\u{2066}',
        "RLI" => '\u{2067}',
        "FSI" => '\u{2068}',
        "PDI" => '\u{2069}',
        _ => return None,
    })
}

/// Whether the levels and the visual order come out as the file says.
///
/// The file writes `x` for a character the algorithm removes, and this engine
/// removes nothing, so those positions are read past on both sides rather
/// than counted wrong.
fn bidi_case(
    text: &str,
    direction: wp_bidi::Direction,
    levels: &str,
    reorder: &str,
) -> Result<(), String> {
    let ours = wp_bidi::character_levels(text, direction);
    let wanted: Vec<&str> = levels.split_whitespace().collect();
    if wanted.len() != ours.len() {
        return Err(format!("{} levels, the standard gives {}", ours.len(), wanted.len()));
    }
    for (index, (level, want)) in ours.iter().zip(&wanted).enumerate() {
        if *want == "x" {
            continue;
        }
        if want.parse::<u8>() != Ok(*level) {
            return Err(format!("level {level} at {index}, the standard says {want}"));
        }
    }

    let removed: Vec<bool> = wanted.iter().map(|want| *want == "x").collect();
    let ours: Vec<usize> = wp_bidi::visual_order(text, direction)
        .into_iter()
        .filter(|index| !removed.get(*index).copied().unwrap_or(false))
        .collect();
    let wanted: Vec<usize> =
        reorder.split_whitespace().filter_map(|index| index.parse().ok()).collect();
    if ours != wanted {
        return Err(format!("drawn in the order {ours:?}, the standard says {wanted:?}"));
    }
    Ok(())
}

/// `BidiTest.txt`: sequences of classes, under a `@Levels` and a `@Reorder`.
fn bidi_classes(text: &str) -> Outcome {
    let mut outcome = Outcome::default();
    let (mut levels, mut reorder) = (String::new(), String::new());
    for (number, line) in text.lines().enumerate() {
        let body = body(line);
        if body.is_empty() {
            continue;
        }
        if let Some(rest) = body.strip_prefix("@Levels:") {
            levels = rest.trim().to_owned();
            continue;
        }
        if let Some(rest) = body.strip_prefix("@Reorder:") {
            reorder = rest.trim().to_owned();
            continue;
        }
        let Some((classes, wanted)) = body.split_once(';') else {
            outcome.unread += 1;
            continue;
        };
        let (Some(text), Ok(directions)) = (
            classes.split_whitespace().map(standing_for).collect::<Option<String>>(),
            wanted.trim().parse::<u8>(),
        ) else {
            outcome.unread += 1;
            continue;
        };

        // The number after the semicolon says which paragraph directions the
        // line is to be run under: one for the direction taken from the text
        // itself, two for left to right, four for right to left.
        for (bit, direction) in [
            (1, wp_bidi::Direction::from_text(&text)),
            (2, wp_bidi::Direction::LeftToRight),
            (4, wp_bidi::Direction::RightToLeft),
        ] {
            if directions & bit == 0 {
                continue;
            }
            let said = bidi_case(&text, direction, &levels, &reorder);
            outcome.case(number + 1, said.is_ok(), || {
                format!("{classes} under {direction:?}: {}", said.unwrap_err())
            });
        }
    }
    outcome
}

/// `BidiCharacterTest.txt`: real text, a paragraph direction, and the answers.
fn bidi_characters(text: &str) -> Outcome {
    let mut outcome = Outcome::default();
    for (number, line) in text.lines().enumerate() {
        let body = body(line);
        if body.is_empty() {
            continue;
        }
        let fields: Vec<&str> = body.split(';').collect();
        if fields.len() < 5 {
            outcome.unread += 1;
            continue;
        }
        let Some(text) = code_points(fields[0]) else {
            outcome.unread += 1;
            continue;
        };
        let direction = match fields[1] {
            "0" => wp_bidi::Direction::LeftToRight,
            "1" => wp_bidi::Direction::RightToLeft,
            "2" => wp_bidi::Direction::from_text(&text),
            _ => {
                outcome.unread += 1;
                continue;
            }
        };

        // The paragraph's own level is the first answer, and the one every
        // other answer depends on.
        let ours = u8::from(direction.is_right_to_left());
        let wanted = fields[2].trim().parse::<u8>().ok();
        if wanted != Some(ours) {
            outcome.case(number + 1, false, || {
                format!("paragraph level {ours}, the standard says {}", fields[2].trim())
            });
            continue;
        }

        let said = bidi_case(&text, direction, fields[3], fields[4]);
        outcome
            .case(number + 1, said.is_ok(), || format!("{}: {}", named(&text), said.unwrap_err()));
    }
    outcome
}

// --- Saying how it went -----------------------------------------------------

/// The totals over the suites that ran.
#[must_use]
pub fn total(reports: &[(Suite, Report)]) -> (usize, usize, usize) {
    let (mut suites, mut ran, mut passed) = (0, 0, 0);
    for (_, report) in reports {
        if let Report::Ran(outcome) = report {
            suites += 1;
            ran += outcome.ran;
            passed += outcome.passed;
        }
    }
    (suites, ran, passed)
}

/// How many of everything passed, out of a hundred.
#[must_use]
pub fn percent(ran: usize, passed: usize) -> f32 {
    if ran == 0 {
        return 100.0;
    }
    #[allow(clippy::cast_precision_loss)]
    let share = passed as f32 / ran as f32;
    share * 100.0
}

/// The line this run leaves behind in the history.
#[must_use]
pub fn history_line(reports: &[(Suite, Report)], stamp: &str, commit: &str) -> String {
    let (suites, ran, passed) = total(reports);
    format!(
        "{stamp}  {commit:<8}  {}  {}  passed {:.2}%",
        plural(suites, "suite"),
        plural(ran, "case"),
        percent(ran, passed)
    )
}

/// The score a history line records.
#[must_use]
pub fn score_in(line: &str) -> Option<f32> {
    line.split("passed ").nth(1)?.trim().trim_end_matches('%').parse().ok()
}

/// A count and the thing counted, in the right number.
fn plural(count: usize, thing: &str) -> String {
    format!("{count} {thing}{}", if count == 1 { "" } else { "s" })
}

/// The report, as lines to print.
#[must_use]
pub fn lines(directory: &Path, reports: &[(Suite, Report)], before: Option<&str>) -> Vec<String> {
    let mut out = vec![
        format!("{} — {}", directory.display(), plural(reports.len(), "suite")),
        String::new(),
    ];
    let widest = reports.iter().map(|(suite, _)| suite.file().len()).max().unwrap_or(0);

    for (suite, report) in reports {
        let file = suite.file();
        match report {
            Report::Missing => {
                out.push(format!("  {file:<widest$}  missing — {}", suite.about()));
            }
            Report::Unreadable(why) => {
                out.push(format!("  {file:<widest$}  unreadable: {why}"));
            }
            Report::Ran(outcome) => {
                let mut said = format!(
                    "  {file:<widest$}  {}, {} passed ({:.2}%)",
                    plural(outcome.ran, "case"),
                    outcome.passed,
                    outcome.percent()
                );
                if outcome.unread > 0 {
                    said.push_str(&format!(", {} lines not understood", outcome.unread));
                }
                out.push(said);
                for failure in &outcome.failures {
                    out.push(format!("      {failure}"));
                }
            }
        }
    }

    let (suites, ran, passed) = total(reports);
    out.push(String::new());
    if suites == 0 {
        out.push(format!("None of the six files is in {}.", directory.display()));
        out.push(String::new());
        out.push("They are published by the Unicode Consortium, one directory per".to_owned());
        out.push("version, and are not downloaded by anything here — see the README".to_owned());
        out.push("beside this directory for which files and where from.".to_owned());
        return out;
    }

    out.push(format!(
        "{}, {}: {passed} passed ({:.2}%)",
        plural(suites, "suite"),
        plural(ran, "case"),
        percent(ran, passed)
    ));
    if let Some(line) = before {
        if let Some(was) = score_in(line) {
            let when = line.split_whitespace().next().unwrap_or("the run before");
            out.push(format!(
                "since {when}: {:+.2} points, from {was:.2}%",
                percent(ran, passed) - was
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_suite_whose_file_is_not_there_is_missing_and_not_a_failure() {
        // Six missing files are the ordinary state of a fresh clone, and a
        // command that called that nought per cent would be lying about the
        // engine.
        let directory =
            std::env::temp_dir().join(format!("wp-conformance-{}-bare", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a directory to work in");

        let reports = run(&directory);
        assert_eq!(reports.len(), 6);
        assert!(reports.iter().all(|(_, report)| matches!(report, Report::Missing)));
        assert_eq!(total(&reports), (0, 0, 0));

        let said = lines(&directory, &reports, None).join("\n");
        assert!(said.contains("missing"), "{said}");
        assert!(said.contains("Unicode Consortium"), "{said}");
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn where_one_character_ends_and_the_next_begins() {
        // The file's own first lines, and one more: a letter, a letter with a
        // mark on it, and a flag, which is two characters and one grapheme.
        let outcome = run_text(
            Suite::GraphemeBreak,
            "# a comment ÷ × 0000\n\
             \u{00F7} 0061 \u{00F7} 0062 \u{00F7}\t# ÷ [0.2] LATIN SMALL LETTER A\n\
             \u{00F7} 0061 \u{00D7} 0301 \u{00F7}\n\
             \u{00F7} 1F1E6 \u{00D7} 1F1E7 \u{00F7}\n",
        );
        assert_eq!(outcome.ran, 3);
        assert_eq!(outcome.passed, 3, "{:?}", outcome.failures);
        assert_eq!(outcome.unread, 0);
    }

    #[test]
    fn where_a_word_ends_and_where_a_line_may_be_broken() {
        let words = run_text(
            Suite::WordBreak,
            "\u{00F7} 0061 \u{00D7} 0062 \u{00F7} 0020 \u{00F7} 0063 \u{00F7}\n",
        );
        assert_eq!((words.ran, words.passed), (1, 1), "{:?}", words.failures);

        // The outermost marks are dropped: a break at either end of a line
        // moves nothing, and the engine does not offer one.
        let lines = run_text(
            Suite::LineBreak,
            "\u{00D7} 0061 \u{00D7} 0062 \u{00D7} 0020 \u{00F7} 0063 \u{00F7}\n",
        );
        assert_eq!((lines.ran, lines.passed), (1, 1), "{:?}", lines.failures);
    }

    #[test]
    fn a_case_the_engine_gets_wrong_is_counted_and_shown() {
        // The harness has to be able to fail, or a hundred per cent means
        // nothing. Nobody breaks between a letter and the next letter.
        let outcome = run_text(Suite::GraphemeBreak, "\u{00F7} 0061 \u{00D7} 0062 \u{00F7}\n");
        assert_eq!((outcome.ran, outcome.passed), (1, 0));
        assert_eq!(outcome.failures.len(), 1);
        assert!(outcome.failures[0].starts_with("line 1: "), "{:?}", outcome.failures);
        assert!(outcome.failures[0].contains("the standard says"), "{:?}", outcome.failures);
        assert!((outcome.percent() - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn a_line_that_cannot_be_read_is_counted_apart_from_a_line_that_failed() {
        // A hole in the parser is this program's fault and not the engine's,
        // and rolling the two together would hide whichever is smaller.
        let outcome = run_text(Suite::Normalization, "@Part0\nnonsense\n0041;0041\n");
        assert_eq!(outcome.ran, 0);
        assert_eq!(outcome.unread, 2);
    }

    #[test]
    fn nfc_and_nfd_both_ways_round() {
        // The file's own shape: source, NFC, NFD, and the two compatibility
        // columns this program reads past. Each row is run six ways.
        let outcome = run_text(
            Suite::Normalization,
            "# comment\n\
             @Part1\n\
             1E0A;1E0A;0044 0307;1E0A;0044 0307; # LATIN CAPITAL LETTER D WITH DOT ABOVE\n\
             00C5;00C5;0041 030A;00C5;0041 030A;\n",
        );
        assert_eq!(outcome.ran, 12);
        assert_eq!(outcome.passed, 12, "{:?}", outcome.failures);
    }

    #[test]
    fn the_bidirectional_algorithm_over_real_text() {
        // Latin, then Hebrew, in a left-to-right paragraph: the Hebrew is
        // drawn right to left inside it.
        let outcome = run_text(
            Suite::BidiCharacter,
            "# comment\n0061 0062;0;0;0 0;0 1\n05D0 05D1;0;0;1 1;1 0\n",
        );
        assert_eq!(outcome.ran, 2);
        assert_eq!(outcome.passed, 2, "{:?}", outcome.failures);
    }

    #[test]
    fn the_bidirectional_algorithm_over_classes() {
        // `BidiTest.txt` is written in classes, so each one stands for itself
        // and the levels above apply until the next heading.
        let outcome = run_text(
            Suite::Bidi,
            "# comment\n@Levels:\t0 0\n@Reorder:\t0 1\nL L; 2\n@Levels:\t1 1\n@Reorder:\t1 0\nR R; 4\n",
        );
        assert_eq!(outcome.ran, 2);
        assert_eq!(outcome.passed, 2, "{:?}", outcome.failures);
        assert_eq!(outcome.unread, 0);
    }

    #[test]
    fn a_class_this_program_has_no_character_for_is_not_silently_passed() {
        let outcome = run_text(Suite::Bidi, "@Levels:\t0\n@Reorder:\t0\nNONSENSE; 2\n");
        assert_eq!((outcome.ran, outcome.unread), (0, 1));
    }

    #[test]
    fn the_history_line_says_when_what_and_how_much() {
        let reports = vec![
            (
                Suite::GraphemeBreak,
                Report::Ran(Outcome { ran: 100, passed: 99, failures: Vec::new(), unread: 0 }),
            ),
            (Suite::LineBreak, Report::Missing),
            (
                Suite::Normalization,
                Report::Ran(Outcome { ran: 100, passed: 91, failures: Vec::new(), unread: 0 }),
            ),
        ];
        assert_eq!(total(&reports), (2, 200, 190));

        let line = history_line(&reports, "2026-09-17T10:00:00Z", "abc1234");
        assert!(line.contains("2 suites"), "{line}");
        assert!(line.contains("200 cases"), "{line}");
        assert!(line.contains("passed 95.00%"), "{line}");
        assert_eq!(score_in(&line), Some(95.0));

        let earlier = line.replace("passed 95.00%", "passed 90.00%");
        let said = lines(Path::new("unicode"), &reports, Some(&earlier)).join("\n");
        assert!(said.contains("+5.00 points"), "{said}");
        assert!(said.contains("missing"), "a missing suite was not named: {said}");
    }
}
