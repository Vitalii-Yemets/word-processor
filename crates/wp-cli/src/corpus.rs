//! Real documents, opened and saved again and compared with what went in.
//!
//! # Why the documents are not in the repository
//!
//! A Word file carries personal data inside itself: the author's name, how
//! long it was edited for, the comments and the tracked changes somebody
//! accepted without removing. A public repository is the wrong place for any
//! of that, so the corpus lives in a directory git ignores and every machine
//! has its own. That is also why this is a command and not a test: `cargo
//! test` on a fresh clone would have nothing to run, and a suite that passes
//! because it found no files is a suite that says nothing.
//!
//! What *is* tested here is the harness — that it finds documents and not
//! everything else in the directory, that a file it cannot open is reported
//! rather than quietly skipped, and that it can tell the three answers below
//! apart. The documents it is pointed at are the part that cannot be
//! committed; the machinery around them is ordinary code.
//!
//! # Three answers, not two
//!
//! "Byte for byte" is two questions wearing one coat. A document can come
//! back with every part identical and still not be the same file, because the
//! zip around the parts has an order, a compression choice and a timestamp
//! for each entry, and reproducing those is a different job from reproducing
//! the XML. So there is a third answer between yes and no:
//!
//! - **identical** — every byte came back;
//! - **repackaged** — every part came back and the wrapping differs;
//! - **differs** — something in the document itself changed, and it is named.
//!
//! A file in a format that has to be converted on the way in — an old `.doc`,
//! a `.rtf`, a web page — cannot be asked the byte question at all: what it
//! saves as is a different format from what it was. That it opened and could
//! be written back is what is reported for those, and nothing more is
//! claimed.
//!
//! # What is a failure and what is a measurement
//!
//! A document that will not open, or opens and will not save, is a bug: the
//! command says so and ends unhappily. A difference is not a bug. It is a
//! number that should go down over time, and a harness that failed on it
//! would be a harness nobody could run.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use wp_opc::Package;

/// The kinds the byte question can be asked of: what goes in is a package and
/// what comes out is the same package.
const PACKAGES: [&str; 4] = ["docx", "docm", "dotx", "dotm"];

/// And the kinds that are converted on the way in, where it cannot.
const CONVERTED: [&str; 8] = ["doc", "rtf", "odt", "htm", "html", "mht", "mhtml", "pdf"];

/// How deep into the corpus directory to look.
///
/// Deep enough for anybody's filing, shallow enough that a symbolic link
/// pointing at its own parent cannot keep this running until the disk fills.
const DEPTH: usize = 8;

/// One part that did not come back the way it went in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Difference {
    /// A part that went in and did not come out.
    Lost(String),
    /// One that came out without having gone in.
    Added(String),
    /// And one that came back as something else.
    Changed {
        /// The part's name inside the package.
        part: String,
        /// How big it was, and how big it came back.
        before: usize,
        after: usize,
    },
}

impl Difference {
    /// The part this is about.
    #[must_use]
    pub fn part(&self) -> &str {
        match self {
            Self::Lost(part) | Self::Added(part) | Self::Changed { part, .. } => part,
        }
    }

    /// The word for what happened to it.
    #[must_use]
    pub fn word(&self) -> &'static str {
        match self {
            Self::Lost(_) => "lost",
            Self::Added(_) => "added",
            Self::Changed { .. } => "changed",
        }
    }
}

/// What happened to one document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Every byte of the file came back.
    Identical,
    /// Every part came back; the zip around them is written differently.
    Repackaged {
        /// How many parts came through untouched.
        parts: usize,
        /// Whether they came back in a different order, which is the one
        /// container difference that can be named rather than guessed at.
        order: bool,
    },
    /// Something inside the document differs.
    Different {
        /// How many parts did come back untouched.
        same: usize,
        /// And what did not.
        differences: Vec<Difference>,
    },
    /// Read from a format that has to be converted, so the byte question
    /// cannot be asked. That it opened and saved is what is reported.
    Converted {
        /// How much text came out of it, as some evidence it read something
        /// rather than nothing.
        characters: usize,
    },
    /// It would not open, or would not save.
    Failed {
        /// Which of those two.
        stage: &'static str,
        /// And what the stage said.
        why: String,
    },
}

/// One document and what happened to it.
#[derive(Clone, Debug)]
pub struct Report {
    /// Where the document is.
    pub path: PathBuf,
    /// How big the file is.
    pub bytes: usize,
    /// And what came of opening and saving it.
    pub outcome: Outcome,
}

/// The whole corpus in numbers.
#[derive(Clone, Debug, Default)]
pub struct Summary {
    /// How many documents were looked at.
    pub documents: usize,
    pub identical: usize,
    pub repackaged: usize,
    pub different: usize,
    pub converted: usize,
    pub failed: usize,
    /// Which parts differ, and in how many documents: the number that says
    /// where the remaining work is.
    pub parts: BTreeMap<(&'static str, String), usize>,
}

/// Opens, saves and compares every document under a directory.
#[must_use]
pub fn survey(directory: &Path) -> Vec<Report> {
    documents(directory).iter().map(|path| examine(path)).collect()
}

/// Every document under a directory.
///
/// Sorted as a whole rather than folder by folder: a report read twice should
/// be the same report, and one where the documents filed in a folder come
/// before the ones beside it reads as an accident.
#[must_use]
pub fn documents(directory: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    walk(directory, 0, &mut found);
    found.sort();
    found
}

/// Every document under a directory, in a stable order.
fn walk(directory: &Path, depth: usize, found: &mut Vec<PathBuf>) {
    if depth > DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    let mut here: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
    here.sort();
    for path in here {
        let name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
        // Hidden files are the machine's business, and `~$name.docx` is the
        // lock file Word leaves behind while a document is open: a hundred
        // and sixty bytes saying who has it, which is not a document.
        if name.starts_with('.') || name.starts_with("~$") {
            continue;
        }
        // Word's own pages of these documents live in the corpus too, and
        // they are pictures rather than documents.
        if depth == 0 && name == crate::fidelity::REFERENCE {
            continue;
        }
        if path.is_dir() {
            walk(&path, depth + 1, found);
        } else if kind(&path).is_some() {
            found.push(path);
        }
    }
}

/// Whether a file is a document, and whether its bytes can be compared.
fn kind(path: &Path) -> Option<bool> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if PACKAGES.contains(&extension.as_str()) {
        Some(true)
    } else if CONVERTED.contains(&extension.as_str()) {
        Some(false)
    } else {
        None
    }
}

/// Opens one document, saves it and compares the two.
#[must_use]
pub fn examine(path: &Path) -> Report {
    let report = |bytes: usize, outcome: Outcome| Report { path: path.to_owned(), bytes, outcome };

    let original = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            return report(0, Outcome::Failed { stage: "read", why: error.to_string() });
        }
    };
    let size = original.len();

    let document = match crate::open_bytes(path, &original) {
        Ok(document) => document,
        Err(why) => return report(size, Outcome::Failed { stage: "open", why }),
    };
    let saved = match document.save() {
        Ok(saved) => saved,
        Err(error) => {
            return report(size, Outcome::Failed { stage: "save", why: error.to_string() });
        }
    };

    // A converted document saves as a different format from the one it came
    // from, so asking whether its bytes came back would be asking nonsense.
    if kind(path) == Some(false) {
        return report(
            size,
            Outcome::Converted { characters: document.plain_text().chars().count() },
        );
    }
    report(size, compare(&original, &saved))
}

/// What one package kept of another.
///
/// Directory entries are skipped: a `word/` with no data in it is part of how
/// the archive was written and not part of the document, so dropping one
/// belongs in the wrapping and not in the list of parts that changed.
#[must_use]
pub fn compare(before: &[u8], after: &[u8]) -> Outcome {
    if before == after {
        return Outcome::Identical;
    }
    let (Ok(before), Ok(after)) = (Package::open(before), Package::open(after)) else {
        return Outcome::Failed {
            stage: "compare",
            why: "one of the two is not a readable package".to_owned(),
        };
    };

    let mut differences = Vec::new();
    let mut same = 0usize;
    for entry in before.entries().iter().filter(|entry| !entry.is_directory()) {
        match after.part(&entry.name) {
            None => differences.push(Difference::Lost(entry.name.clone())),
            Some(data) if data != entry.data => differences.push(Difference::Changed {
                part: entry.name.clone(),
                before: entry.data.len(),
                after: data.len(),
            }),
            Some(_) => same += 1,
        }
    }
    for entry in after.entries().iter().filter(|entry| !entry.is_directory()) {
        if before.part(&entry.name).is_none() {
            differences.push(Difference::Added(entry.name.clone()));
        }
    }

    if differences.is_empty() {
        let names = |package: &Package| -> Vec<String> {
            package
                .entries()
                .iter()
                .filter(|entry| !entry.is_directory())
                .map(|entry| entry.name.to_ascii_lowercase())
                .collect()
        };
        Outcome::Repackaged { parts: same, order: names(&before) != names(&after) }
    } else {
        Outcome::Different { same, differences }
    }
}

/// The corpus in numbers.
#[must_use]
pub fn summarise(reports: &[Report]) -> Summary {
    let mut summary = Summary { documents: reports.len(), ..Summary::default() };
    for report in reports {
        match &report.outcome {
            Outcome::Identical => summary.identical += 1,
            Outcome::Repackaged { .. } => summary.repackaged += 1,
            Outcome::Converted { .. } => summary.converted += 1,
            Outcome::Failed { .. } => summary.failed += 1,
            Outcome::Different { differences, .. } => {
                summary.different += 1;
                for difference in differences {
                    *summary
                        .parts
                        .entry((difference.word(), difference.part().to_owned()))
                        .or_insert(0) += 1;
                }
            }
        }
    }
    summary
}

/// What to say about one document.
fn said(outcome: &Outcome) -> (&'static str, String) {
    match outcome {
        Outcome::Identical => ("identical", String::new()),
        Outcome::Repackaged { parts, order } => (
            "repackaged",
            format!(
                "all {parts} parts came back, {}",
                if *order { "in a different order" } else { "the zip around them did not" }
            ),
        ),
        Outcome::Different { same, differences } => {
            let mut named: Vec<String> = differences
                .iter()
                .take(3)
                .map(|difference| format!("{} {}", difference.word(), difference.part()))
                .collect();
            if differences.len() > named.len() {
                named.push(format!("and {} more", differences.len() - named.len()));
            }
            ("differs", format!("{same} parts kept, {}", named.join(", ")))
        }
        Outcome::Converted { characters } => {
            ("converted", format!("{characters} characters read, no bytes to compare"))
        }
        Outcome::Failed { stage, why } => ("failed", format!("{stage}: {why}")),
    }
}

/// The report, as lines to print.
#[must_use]
pub fn lines(directory: &Path, reports: &[Report]) -> Vec<String> {
    let mut out = Vec::new();
    if reports.is_empty() {
        out.push(format!("{} holds no documents.", directory.display()));
        out.push(String::new());
        out.push(
            "Put files Word itself wrote into it: .docx, .docm, .dotx and .dotm can be".into(),
        );
        out.push("compared byte for byte, and .doc, .rtf, .odt and saved web pages can at".into());
        out.push("least be opened. Nothing in that directory is committed or leaves this".into());
        out.push("machine — corpus/README.md says why.".into());
        return out;
    }

    let shown: Vec<(String, String, &Report)> = reports
        .iter()
        .map(|report| {
            let name = report
                .path
                .strip_prefix(directory)
                .unwrap_or(&report.path)
                .to_string_lossy()
                .into_owned();
            let (word, _) = said(&report.outcome);
            (word.to_owned(), name, report)
        })
        .collect();
    let width = shown.iter().map(|(_, name, _)| name.len()).max().unwrap_or(0).min(44);

    out.push(format!("{} — {} documents", directory.display(), reports.len()));
    out.push(String::new());
    for (word, name, report) in &shown {
        let (_, detail) = said(&report.outcome);
        out.push(
            format!(
                "  {word:<11} {name:<width$}  {:>9}  {detail}",
                crate::size(report.bytes),
                width = width
            )
            .trim_end()
            .to_owned(),
        );
    }

    let summary = summarise(reports);
    let mut counted = Vec::new();
    for (count, word) in [
        (summary.identical, "identical"),
        (summary.repackaged, "repackaged"),
        (summary.different, "differ"),
        (summary.converted, "converted"),
        (summary.failed, "failed"),
    ] {
        if count > 0 {
            counted.push(format!("{count} {word}"));
        }
    }
    out.push(String::new());
    out.push(format!("{} documents: {}", summary.documents, counted.join(", ")));

    if !summary.parts.is_empty() {
        out.push(String::new());
        out.push("what differs, across the corpus:".to_owned());
        let mut rows: Vec<((&str, String), usize)> =
            summary.parts.into_iter().map(|(key, count)| (key, count)).collect();
        // The part that differs in the most documents is the work worth doing
        // first, so it goes at the top.
        rows.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
        let widest = rows.iter().map(|((_, part), _)| part.len()).max().unwrap_or(0).min(44);
        for ((word, part), count) in rows {
            out.push(format!(
                "  {word:<8} {part:<widest$}  {count} document{}",
                if count == 1 { "" } else { "s" },
                widest = widest
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    use wp_docx::Document;

    /// A directory of this test's own, inside the container.
    fn scratch(name: &str) -> PathBuf {
        let directory =
            std::env::temp_dir().join(format!("wp-corpus-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a directory to work in");
        directory
    }

    /// A document this program wrote, which is the only kind the suite has.
    fn a_document() -> Vec<u8> {
        Document::create(&crate::demonstration_body())
            .expect("the demonstration document")
            .save()
            .expect("saving it")
    }

    #[test]
    fn a_document_that_comes_back_whole_is_reported_identical() {
        let directory = scratch("whole");
        std::fs::write(directory.join("one.docx"), a_document()).expect("writing it");

        let reports = survey(&directory);
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].outcome, Outcome::Identical, "a document did not come back whole");

        let summary = summarise(&reports);
        assert_eq!(summary.identical, 1);
        assert_eq!(summary.failed, 0);
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn everything_that_is_not_a_document_is_left_alone() {
        // A corpus directory is a person's own: it will have notes in it, and
        // a README, and the lock file Word leaves behind while a document is
        // open. None of those is a document, and one of them looks exactly
        // like one.
        let directory = scratch("mixed");
        std::fs::write(directory.join("one.docx"), a_document()).expect("writing it");
        std::fs::write(directory.join("README.md"), b"notes").expect("writing it");
        std::fs::write(directory.join("list.txt"), b"a list").expect("writing it");
        std::fs::write(directory.join("~$one.docx"), b"lock").expect("writing it");
        std::fs::write(directory.join(".hidden.docx"), b"hidden").expect("writing it");
        // And documents filed in folders are still documents.
        std::fs::create_dir_all(directory.join("letters")).expect("a folder");
        std::fs::write(directory.join("letters/two.docx"), a_document()).expect("writing it");

        let reports = survey(&directory);
        let mut names: Vec<String> = reports
            .iter()
            .map(|report| {
                report.path.file_name().unwrap_or_default().to_string_lossy().into_owned()
            })
            .collect();
        names.sort();
        assert_eq!(names, vec!["one.docx".to_owned(), "two.docx".to_owned()], "found {names:?}");
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_file_that_will_not_open_is_reported_rather_than_skipped() {
        // The whole point of running against real documents is finding the
        // ones this program chokes on. Passing over them quietly would leave
        // a corpus of files that happen to work.
        let directory = scratch("broken");
        std::fs::write(directory.join("broken.docx"), b"this is not a zip archive")
            .expect("writing it");

        let reports = survey(&directory);
        assert_eq!(reports.len(), 1);
        match &reports[0].outcome {
            Outcome::Failed { stage, why } => {
                assert_eq!(*stage, "open");
                assert!(!why.is_empty(), "it failed without saying why");
            }
            other => panic!("a file that is not a document was reported as {other:?}"),
        }
        assert_eq!(summarise(&reports).failed, 1);
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_converted_document_is_not_asked_the_byte_question() {
        // What a Rich Text file saves as is a package, and comparing that
        // with the text it came from would be comparing two formats.
        let directory = scratch("converted");
        std::fs::write(directory.join("note.rtf"), br"{\rtf1\ansi Hello from Word.}")
            .expect("writing it");

        let reports = survey(&directory);
        assert_eq!(reports.len(), 1);
        match &reports[0].outcome {
            Outcome::Converted { characters } => assert!(*characters > 0, "it read nothing"),
            other => panic!("a converted document was reported as {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// Two packages holding the named parts, in the order given.
    fn packaged(parts: &[(&str, &[u8])]) -> Vec<u8> {
        let mut package = Package::empty();
        package.set_part(
            "[Content_Types].xml",
            br#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"/>"#
                .to_vec(),
        );
        for (name, data) in parts {
            package.set_part(name, (*data).to_vec());
        }
        package.save().expect("writing the package")
    }

    #[test]
    fn parts_that_came_back_and_a_zip_that_did_not_are_different_answers() {
        // A document whose every part survived and whose archive is written
        // differently has not lost anything, and saying it "differs" would
        // send somebody looking for a change in the XML that is not there.
        let one = packaged(&[
            ("word/document.xml", b"<w:document/>"),
            ("word/styles.xml", b"<w:styles/>"),
        ]);
        let other = packaged(&[
            ("word/styles.xml", b"<w:styles/>"),
            ("word/document.xml", b"<w:document/>"),
        ]);
        assert_ne!(one, other, "the two packages came out identical");

        assert_eq!(compare(&one, &one), Outcome::Identical);
        match compare(&one, &other) {
            Outcome::Repackaged { parts, order } => {
                assert_eq!(parts, 3, "the parts were not all counted");
                assert!(order, "the order changed and was not noticed");
            }
            other => panic!("a repackaged document was reported as {other:?}"),
        }
    }

    #[test]
    fn a_part_that_changed_is_named_and_the_rest_are_counted() {
        let before = packaged(&[
            ("word/document.xml", b"<w:document/>"),
            ("word/styles.xml", b"<w:styles/>"),
        ]);
        let after = packaged(&[
            ("word/document.xml", b"<w:document><w:body/></w:document>"),
            ("word/people.xml", b"<w:people/>"),
        ]);

        match compare(&before, &after) {
            Outcome::Different { same, differences } => {
                assert_eq!(same, 1, "the content types stream should have come through");
                assert!(differences.contains(&Difference::Lost("word/styles.xml".to_owned())));
                assert!(differences.contains(&Difference::Added("word/people.xml".to_owned())));
                assert!(differences.iter().any(|difference| matches!(
                    difference,
                    Difference::Changed { part, .. } if part == "word/document.xml"
                )));
            }
            other => panic!("a changed document was reported as {other:?}"),
        }
    }

    #[test]
    fn an_empty_corpus_says_what_to_put_in_it() {
        // A fresh clone has no corpus, and a command that printed "0
        // documents" and stopped would look broken rather than empty.
        let directory = scratch("empty");
        let reports = survey(&directory);
        assert!(reports.is_empty());

        let said = lines(&directory, &reports).join("\n");
        assert!(said.contains("no documents"), "{said}");
        assert!(said.contains(".docx"), "it did not say what to put there: {said}");
        assert!(said.contains("README.md"), "it did not say where to read why: {said}");
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn the_report_counts_every_document_once_and_names_what_differs() {
        let reports = vec![
            Report { path: PathBuf::from("a.docx"), bytes: 100, outcome: Outcome::Identical },
            Report {
                path: PathBuf::from("b.docx"),
                bytes: 200,
                outcome: Outcome::Repackaged { parts: 12, order: false },
            },
            Report {
                path: PathBuf::from("c.docx"),
                bytes: 300,
                outcome: Outcome::Different {
                    same: 9,
                    differences: vec![Difference::Changed {
                        part: "word/document.xml".to_owned(),
                        before: 10,
                        after: 11,
                    }],
                },
            },
            Report {
                path: PathBuf::from("d.docx"),
                bytes: 400,
                outcome: Outcome::Different {
                    same: 4,
                    differences: vec![Difference::Changed {
                        part: "word/document.xml".to_owned(),
                        before: 10,
                        after: 12,
                    }],
                },
            },
            Report {
                path: PathBuf::from("e.rtf"),
                bytes: 500,
                outcome: Outcome::Converted { characters: 42 },
            },
            Report {
                path: PathBuf::from("f.docx"),
                bytes: 600,
                outcome: Outcome::Failed { stage: "open", why: "not a zip archive".to_owned() },
            },
        ];

        let summary = summarise(&reports);
        assert_eq!(summary.documents, 6);
        assert_eq!(
            summary.identical
                + summary.repackaged
                + summary.different
                + summary.converted
                + summary.failed,
            summary.documents,
            "a document was counted twice or not at all"
        );
        // The part that differs in the most documents is the work worth doing
        // first, and it is the number this whole command exists to produce.
        assert_eq!(summary.parts[&("changed", "word/document.xml".to_owned())], 2);

        let said = lines(Path::new("."), &reports).join("\n");
        assert!(said.contains("6 documents: 1 identical, 1 repackaged, 2 differ"), "{said}");
        assert!(said.contains("word/document.xml"), "{said}");
        assert!(said.contains("not a zip archive"), "a failure was not printed: {said}");
    }
}
