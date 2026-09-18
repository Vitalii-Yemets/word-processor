//! Every macro in the corpus, read and written back out.
//!
//! # Why this is a command
//!
//! For the same reason the corpus itself is one: the documents cannot be
//! committed — see [`crate::corpus`] — so the only machine that can run this
//! is a machine with real macros on it. What the repository holds is the
//! parser and its own tests; what this adds is the question those tests
//! cannot ask, which is whether the parser survives what people actually
//! wrote.
//!
//! # The two questions
//!
//! **Did it parse?** A module that produced no complaints was understood line
//! by line. One that complained was not, and the line it complained about is
//! printed, because that line is the next piece of work.
//!
//! **Did it come back?** The tree holds every word and every space, so
//! writing it out again must give back the module byte for byte. That is the
//! stronger question of the two: a parser can understand a line and still
//! lose the tab in front of it, and a macro written back with the tabs
//! missing is a macro somebody else's editor will show as a mess.

use std::path::{Path, PathBuf};

use wp_vba::tree::Complaint;

/// How one document's macros went.
#[derive(Clone, Debug, Default)]
pub struct Read {
    pub path: PathBuf,
    /// How many modules the project holds.
    pub modules: usize,
    /// How many of them were understood, line by line.
    pub understood: usize,
    /// And how many came back out byte for byte.
    pub whole: usize,
    /// The first few lines nobody could make sense of, with the module they
    /// are in.
    pub complaints: Vec<String>,
    /// Or why the project could not be read at all.
    pub failed: Option<String>,
}

/// How many complaints are worth printing from one document.
const SHOWN: usize = 3;

/// Reads every module of every document in a directory.
#[must_use]
pub fn run(corpus: &Path) -> Vec<Read> {
    crate::corpus::documents(corpus).into_iter().filter_map(|path| examine(&path)).collect()
}

/// One document, or nothing at all if it carries no macros.
#[must_use]
pub fn examine(path: &Path) -> Option<Read> {
    let bytes = std::fs::read(path).ok()?;
    let document = crate::open_bytes(path, &bytes).ok()?;
    let project = document.package().part("word/vbaProject.bin")?;

    let mut read = Read { path: path.to_owned(), ..Read::default() };
    let project = match wp_vba::Project::open(project) {
        Ok(project) => project,
        Err(error) => {
            read.failed = Some(error.to_string());
            return Some(read);
        }
    };

    for module in &project.modules {
        read.modules += 1;
        let (tree, complaints) = wp_vba::parse::parse(&module.source);
        if complaints.is_empty() {
            read.understood += 1;
        } else if read.complaints.len() < SHOWN {
            read.complaints.extend(
                complaints
                    .iter()
                    .take(SHOWN - read.complaints.len())
                    .map(|complaint| said(&module.name, complaint)),
            );
        }
        if tree.written() == module.source {
            read.whole += 1;
        } else if read.complaints.len() < SHOWN {
            read.complaints.push(format!("{}: did not come back as it went in", module.name));
        }
    }
    Some(read)
}

/// One complaint, with the module it is about.
fn said(module: &str, complaint: &Complaint) -> String {
    format!("{module} line {}: {}", complaint.line, complaint.said)
}

/// The totals over every document that carries macros.
#[must_use]
pub fn total(reports: &[Read]) -> (usize, usize, usize) {
    reports.iter().fold((0, 0, 0), |(modules, understood, whole), read| {
        (modules + read.modules, understood + read.understood, whole + read.whole)
    })
}

/// The report, as lines to print.
#[must_use]
pub fn lines(corpus: &Path, reports: &[Read]) -> Vec<String> {
    if reports.is_empty() {
        return vec![
            format!("No document in {} carries Visual Basic.", corpus.display()),
            String::new(),
            "Macro-enabled documents are .docm and .dotm, and an ordinary .docx".to_owned(),
            "cannot hold macros at all. Put some in the corpus — see its README —".to_owned(),
            "and this reads every module of every one of them.".to_owned(),
        ];
    }

    let widest = reports.iter().map(|read| name_of(corpus, read).len()).max().unwrap_or(0).min(44);
    let mut out = vec![
        format!(
            "{} — {} carrying Visual Basic",
            corpus.display(),
            plural(reports.len(), "document")
        ),
        String::new(),
    ];
    for read in reports {
        let name = name_of(corpus, read);
        if let Some(why) = &read.failed {
            out.push(format!("  {name:<widest$}  the project would not open: {why}"));
            continue;
        }
        out.push(format!(
            "  {name:<widest$}  {}, {} read, {} came back the same",
            plural(read.modules, "module"),
            read.understood,
            read.whole
        ));
        for complaint in &read.complaints {
            out.push(format!("      {complaint}"));
        }
    }

    let (modules, understood, whole) = total(reports);
    out.push(String::new());
    out.push(format!(
        "{}, {}: {understood} read, {whole} came back the same",
        plural(reports.len(), "document"),
        plural(modules, "module")
    ));
    out
}

fn name_of(corpus: &Path, read: &Read) -> String {
    read.path.strip_prefix(corpus).unwrap_or(&read.path).to_string_lossy().into_owned()
}

/// A count and the thing counted, in the right number.
fn plural(count: usize, thing: &str) -> String {
    format!("{count} {thing}{}", if count == 1 { "" } else { "s" })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A macro-enabled document carrying the modules given.
    fn document_with(modules: &[(&str, &str)]) -> Vec<u8> {
        let mut body = wp_docx::model::Body::default();
        body.blocks.push(wp_docx::model::Block::Paragraph(wp_docx::model::Paragraph::text("one")));
        let mut document = wp_docx::Document::create(&body).expect("a document");
        document.set_kind(wp_docx::kinds::Kind::MacroEnabledDocument);

        let bytes = document.save().expect("saving");
        let mut package = wp_opc::Package::open(&bytes).expect("a package");
        package.add_part(
            "word/vbaProject.bin",
            "application/vnd.ms-office.vbaProject",
            wp_vba::example(modules),
        );
        let mut relationships = package.relationships("word/document.xml").expect("relationships");
        relationships.add(
            "http://schemas.microsoft.com/office/2006/relationships/vbaProject",
            "vbaProject.bin",
            wp_opc::TargetMode::Internal,
        );
        package.set_relationships(&relationships).expect("writing them");
        package.save().expect("saving the package")
    }

    fn scratch(name: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!("wp-vba-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a directory to work in");
        directory
    }

    #[test]
    fn a_document_carrying_macros_has_every_module_read_and_written_back() {
        let corpus = scratch("read");
        std::fs::write(
            corpus.join("one.docm"),
            document_with(&[
                ("Module1", "Public Sub Hello()\r\n    MsgBox \"Hello\"\r\nEnd Sub\r\n"),
                ("Module2", "Private Function Twice(ByVal n As Long) As Long\r\n    Twice = n * 2\r\nEnd Function\r\n"),
            ]),
        )
        .expect("writing it");

        let reports = run(&corpus);
        assert_eq!(reports.len(), 1);
        assert_eq!(total(&reports), (2, 2, 2));
        assert!(reports[0].complaints.is_empty(), "{:?}", reports[0].complaints);

        let said = lines(&corpus, &reports).join("\n");
        assert!(said.contains("2 modules, 2 read, 2 came back the same"), "{said}");
        let _ = std::fs::remove_dir_all(&corpus);
    }

    #[test]
    fn a_module_that_does_not_parse_is_named_with_its_line() {
        // The point of running this over other people's macros: the line it
        // could not read is the next piece of work, and a report that only
        // counted would not say where to start.
        let corpus = scratch("broken");
        std::fs::write(
            corpus.join("one.docm"),
            document_with(&[("Module1", "Sub A()\r\n    ]] nonsense\r\nEnd Sub\r\n")]),
        )
        .expect("writing it");

        let reports = run(&corpus);
        assert_eq!(total(&reports), (1, 0, 1), "it did not come back unchanged");
        assert_eq!(reports[0].complaints.len(), 1);
        assert!(reports[0].complaints[0].contains("Module1 line 2"), "{:?}", reports[0].complaints);
        let _ = std::fs::remove_dir_all(&corpus);
    }

    #[test]
    fn a_document_with_no_macros_in_it_is_not_in_the_report() {
        // Most of anybody's corpus is .docx, and a list of documents that
        // carry nothing would bury the ones that do.
        let corpus = scratch("plain");
        let mut body = wp_docx::model::Body::default();
        body.blocks.push(wp_docx::model::Block::Paragraph(wp_docx::model::Paragraph::text("one")));
        let bytes = wp_docx::Document::create(&body).expect("a document").save().expect("saving");
        std::fs::write(corpus.join("plain.docx"), bytes).expect("writing it");

        let reports = run(&corpus);
        assert!(reports.is_empty());
        let said = lines(&corpus, &reports).join("\n");
        assert!(said.contains("carries Visual Basic"), "{said}");
        assert!(said.contains(".docm"), "it did not say what does: {said}");
        let _ = std::fs::remove_dir_all(&corpus);
    }
}
