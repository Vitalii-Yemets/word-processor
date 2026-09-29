//! A command line front end for the document stack.
//!
//! There is no window yet — that is a later stage — so this is how the work so
//! far is exercised and demonstrated. Every command runs the same code the
//! editor will: the package layer, the XML layer and the document model.

#![forbid(unsafe_code)]

use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;

/// Writes a line to standard output.
///
/// A broken pipe is not a failure: it means whatever was reading — a pager, or
/// `head` — has gone away, and the right response is to stop quietly. The
/// standard `println!` panics instead, which turns an ordinary `wp outline file
/// | head` into a crash report.
fn write_line(text: &str) {
    let mut out = io::stdout().lock();
    if let Err(error) = writeln!(out, "{text}") {
        if error.kind() == io::ErrorKind::BrokenPipe {
            std::process::exit(0);
        }
        eprintln!("error: cannot write to standard output: {error}");
        std::process::exit(1);
    }
}

/// How much memory this program is actually using, in bytes.
///
/// Asked of the operating system rather than counted here: a counting allocator
/// would mean `unsafe`, and this program keeps all of that in one crate. Linux
/// only, because that is where the benchmark runs; elsewhere it says nothing
/// rather than guessing.
fn memory_in_use() -> Option<usize> {
    let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
    // The second number is the resident set, in pages.
    let pages: usize = statm.split_whitespace().nth(1)?.parse().ok()?;
    Some(pages * 4096)
}

/// A number of bytes, written the way a person reads one.
fn size(bytes: usize) -> String {
    #[allow(clippy::cast_precision_loss)]
    let value = bytes as f64;
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.2} GB", value / 1024.0 / 1024.0 / 1024.0)
    } else if bytes >= 1024 * 1024 {
        format!("{:.1} MB", value / 1024.0 / 1024.0)
    } else if bytes >= 1024 {
        format!("{:.1} kB", value / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

/// Like `println!`, but survives a closed pipe.
macro_rules! outln {
    () => { write_line("") };
    ($($argument:tt)*) => { write_line(&format!($($argument)*)) };
}

use std::time::Instant;

use wp_docx::model::{
    Alignment, Block, Body, Paragraph, ResolvedRunProperties, Run, Table, TableBorders, TableCell,
    TableRow,
};
use wp_docx::Document;

mod conformance;
mod corpus;
mod fidelity;
mod vba;

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let command = arguments.first().map(String::as_str);

    let result = match (command, arguments.len()) {
        (Some("new"), 2) => create(&arguments[1]),
        (Some("info"), 2) => info(&arguments[1]),
        (Some("text"), 2) => text(&arguments[1]),
        (Some("outline"), 2) => outline(&arguments[1]),
        (Some("roundtrip"), 3) => roundtrip(&arguments[1], &arguments[2]),
        (Some("convert"), 3) => convert(&arguments[1], &arguments[2], false),
        (Some("convert"), 4) if arguments[3] == "--filtered" => {
            convert(&arguments[1], &arguments[2], true)
        }
        (Some("corpus"), 1) => corpus_command("corpus"),
        (Some("corpus"), 2) => corpus_command(&arguments[1]),
        (Some("fidelity"), 1) => fidelity_command("corpus"),
        (Some("fidelity"), 2) => fidelity_command(&arguments[1]),
        (Some("conformance"), 1) => conformance_command("unicode"),
        (Some("conformance"), 2) => conformance_command(&arguments[1]),
        (Some("vba"), 1) => vba_command("corpus"),
        (Some("vba"), 2) => vba_command(&arguments[1]),
        (Some("replace"), 5) => replace(&arguments[1], &arguments[2], &arguments[3], &arguments[4]),
        (Some("append"), 4) => append(&arguments[1], &arguments[2], &arguments[3]),
        (Some("render"), 3) => render(&arguments[1], &arguments[2], "96"),
        (Some("render"), 4) => render(&arguments[1], &arguments[2], &arguments[3]),
        (Some("pdf"), 3) => pdf(&arguments[1], &arguments[2]),
        (Some("bench"), 1) => bench("100"),
        (Some("bench"), 2) => bench(&arguments[1]),
        (Some("fonts"), 1) => fonts(),
        (Some("signatures"), 2) => signatures(&arguments[1]),
        (Some("sign"), 5) => sign(&arguments[1], &arguments[2], &arguments[3], &arguments[4], ""),
        (Some("sign"), 6) => {
            sign(&arguments[1], &arguments[2], &arguments[3], &arguments[4], &arguments[5])
        }
        _ => {
            print_usage();
            // Started with no arguments at all, most likely by double-clicking
            // it in a file manager. Exiting immediately would close the console
            // before anything could be read, so the program looks broken when it
            // is only telling you how to use it.
            if arguments.is_empty() {
                wait_for_enter();
            }
            return ExitCode::from(2);
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn print_usage() {
    eprintln!(
        "\
Usage: wp <command>

  new <file.docx>              create a demonstration document; .docm, .dotx
                               and .dotm make the other three kinds
  info <file.docx>             show the package structure
  text <file.docx>             print the document text
  outline <file.docx>          print the structure with formatting
  roundtrip <in> <out>         open and save, checking nothing changed
  convert <in> <out> [--filtered]
                               open anything that opens, and write what the
                               out file's extension says: .doc, .rtf, .odt,
                               .htm (its pictures in a folder beside it, or
                               Word's Web Page, Filtered with --filtered),
                               .mht, or a Word package
  corpus [directory]           open, save and compare every document in a
                               directory of real files (default: corpus/)
  fidelity [directory]         draw every document in it and score the pages
                               against Word's own, kept in reference/
  conformance [directory]      run the Unicode test suites kept in it against
                               the text engine (default: unicode/)
  vba [directory]              read every macro of every document in it, and
                               write each module back out to check it survived

  replace <in> <out> <from> <to>   replace text, across run boundaries
  append <in> <out> <text>         add a paragraph at the end

  render <in.docx> <prefix> [dpi]  draw the pages as PNG images
  pdf <in.docx> <out.pdf>         write the pages out as a PDF
  bench [pages]                   time what a person waits for
  fonts                            list the fonts found on this machine

  signatures <file.docx>           show the signatures and whether they hold
  sign <in> <out> <cert.der> <key.der> [why]
                                   sign the document with a certificate and
                                   its key, both written as DER

The editing commands report which parts of the package changed, so it is
visible that everything else was carried through untouched.
"
    );
}

/// Holds the console open until the reader presses Enter.
///
/// Only used when the program was started with no arguments, so a terminal user
/// typing the bare command sees the same help and presses Enter once.
fn wait_for_enter() {
    eprintln!();
    eprint!("Press Enter to close.");
    let _ = io::stderr().flush();
    let mut discard = String::new();
    let _ = io::stdin().read_line(&mut discard);
}

/// Reads a file, turning any failure into a message worth printing.
fn read(path: &str) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|error| format!("cannot read {path}: {error}"))
}

fn write(path: &str, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(path, bytes).map_err(|error| format!("cannot write {path}: {error}"))
}

fn open(path: &str) -> Result<Document, String> {
    let bytes = read(path)?;
    open_bytes(Path::new(path), &bytes).map_err(|why| format!("cannot open {path}: {why}"))
}

/// Opens bytes as whatever the file name says they are.
///
/// Separate from reading the file because the corpus harness reads its own —
/// it needs the bytes afterwards to compare against — and because what went
/// wrong is more useful than a sentence with the path already glued onto it.
fn open_bytes(path: &Path, bytes: &[u8]) -> Result<Document, String> {
    // A Rich Text file, a web page, an old Word document, an OpenDocument
    // package and a PDF are read as what they are; everything else is a
    // Word package.
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    // Each reader has its own kind of complaint, and what a caller wants is
    // the sentence rather than the type.
    let said = |error: &dyn std::fmt::Display| error.to_string();
    match extension.as_str() {
        "doc" => wp_doc::open(bytes).map_err(|error| said(&error)),
        "odt" => wp_odt::open(bytes).map_err(|error| said(&error)),
        "pdf" => wp_pdf::open(bytes).map_err(|error| said(&error)),
        "rtf" => wp_rtf::open(bytes).map_err(|error| said(&error)),
        "htm" | "html" => wp_html::open_html(bytes, Some(path)).map_err(|error| said(&error)),
        "mht" | "mhtml" => wp_html::open_mht(bytes).map_err(|error| said(&error)),
        _ => Document::open(bytes).map_err(|error| said(&error)),
    }
}

// --- Commands ---------------------------------------------------------------

/// Writes a document that exercises the features the model supports.
fn create(path: &str) -> Result<(), String> {
    let mut document = Document::create(&demonstration_body())
        .map_err(|error| format!("cannot build the document: {error}"))?;
    // The extension says which of the four kinds to write.
    if let Some(kind) = Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .and_then(wp_docx::kinds::Kind::of_extension)
    {
        document.set_kind(kind);
    }
    let bytes = document.save().map_err(|error| format!("cannot save: {error}"))?;

    write(path, &bytes)?;

    let name = Path::new(path).file_name().and_then(|name| name.to_str()).unwrap_or(path);
    outln!("wrote {name} ({} bytes)", bytes.len());
    outln!("open it in Word or LibreOffice to check the result");
    Ok(())
}

/// Prints what the package contains.
fn info(path: &str) -> Result<(), String> {
    let document = open(path)?;
    let package = document.package();

    outln!("main document part: {}", document.main_part());
    outln!(
        "kind: {}{}",
        document.kind().label(),
        if document.has_macros() { ", with macros" } else { "" }
    );
    outln!();
    outln!("{:<44} {:>10}  {}", "PART", "BYTES", "CONTENT TYPE");

    for entry in package.entries() {
        if entry.is_directory() {
            continue;
        }
        // The content types stream is not a part and has no type of its own;
        // saying "undeclared" would read as a fault in the document.
        let description = if entry.name.eq_ignore_ascii_case(wp_opc::CONTENT_TYPES_PART) {
            "(package stream)".to_owned()
        } else {
            match package.content_type(&entry.name) {
                Some(content_type) => short_type(content_type),
                None => "(undeclared)".to_owned(),
            }
        };
        outln!("{:<44} {:>10}  {}", entry.name, entry.data.len(), description);
    }

    let relationships = package
        .relationships("")
        .map_err(|error| format!("cannot read the package relationships: {error}"))?;
    if !relationships.all().is_empty() {
        outln!();
        outln!("package relationships:");
        for relationship in relationships.all() {
            outln!(
                "  {:<8} {:<12} {}",
                relationship.id,
                short_type(&relationship.kind),
                relationship.target
            );
        }
    }

    let problems = package.validate();
    if problems.is_empty() {
        outln!();
        outln!("package is consistent");
    } else {
        outln!();
        outln!("{} problem(s) found:", problems.len());
        for problem in problems {
            outln!("  {problem}");
        }
    }

    Ok(())
}

/// Prints the document's text.
fn text(path: &str) -> Result<(), String> {
    let document = open(path)?;
    let text = document.plain_text();
    outln!("{text}");
    Ok(())
}

/// Prints the structure with the formatting each run actually has.
///
/// The formatting shown is the *resolved* one — what the text really looks like
/// once the style chain and the document defaults have been applied. Printing
/// only what each run states directly would show almost nothing, since a heading
/// is bold because its style says so and not because the run does.
fn outline(path: &str) -> Result<(), String> {
    let document = open(path)?;
    let body = document.body();

    print_blocks(&document, &body.blocks, 0);
    Ok(())
}

fn print_blocks(document: &Document, blocks: &[Block], depth: usize) {
    let indent = "  ".repeat(depth);

    for block in blocks {
        match block {
            Block::Paragraph(paragraph) => {
                let resolved = document.resolve_paragraph(paragraph);

                let mut description = String::from("paragraph");
                if let Some(style) = paragraph.style() {
                    description.push_str(&format!(" style={style}"));
                }
                description.push_str(&format!(" align={}", resolved.alignment.to_attribute()));
                if resolved.right_to_left {
                    description.push_str(" rtl");
                }
                if let Some(level) = resolved.outline_level {
                    description.push_str(&format!(" outline={level}"));
                }
                if resolved.space_before != 0 || resolved.space_after != 0 {
                    description.push_str(&format!(
                        " spacing={}/{}",
                        resolved.space_before, resolved.space_after
                    ));
                }
                outln!("{indent}{description}");

                for run in &paragraph.runs {
                    let text = run.plain_text();
                    if text.is_empty() {
                        continue;
                    }
                    let formatting = describe(&document.resolve_run(paragraph, run));
                    outln!("{indent}  run [{formatting}]: {text:?}");
                }
            }
            Block::Table(table) => {
                let style = table.style.as_deref().unwrap_or("(none)");
                outln!("{indent}table style={style} rows={}", table.rows.len());
                for (row_index, row) in table.rows.iter().enumerate() {
                    outln!("{indent}  row {row_index}");
                    for (cell_index, cell) in row.cells.iter().enumerate() {
                        outln!("{indent}    cell {cell_index}");
                        print_blocks(document, &cell.blocks, depth + 3);
                    }
                }
            }
        }
    }
}

/// Summarizes the formatting a run actually has.
fn describe(properties: &ResolvedRunProperties) -> String {
    let mut parts: Vec<String> = Vec::new();

    // The format stores half-points; people think in points.
    parts.push(format!("{}pt", properties.size_points()));
    if let Some(font) = &properties.font {
        parts.push(font.clone());
    }
    if properties.bold {
        parts.push("bold".to_owned());
    }
    if properties.italic {
        parts.push("italic".to_owned());
    }
    if properties.underline.is_visible() {
        parts.push(format!("underline:{}", properties.underline.to_attribute()));
    }
    if properties.strike {
        parts.push("strike".to_owned());
    }
    if let Some(color) = &properties.color {
        parts.push(format!("#{color}"));
    }
    if let Some(language) = &properties.language {
        parts.push(language.clone());
    }
    if properties.right_to_left {
        parts.push("rtl".to_owned());
    }

    parts.join(", ")
}

/// Opens a document and saves it again, reporting whether anything changed.
///
/// This is the property the whole design rests on: a document that passes
/// through this program untouched must come out identical. If it does not, the
/// package layer is losing something.
fn roundtrip(input: &str, output: &str) -> Result<(), String> {
    let original = read(input)?;
    let document =
        Document::open(&original).map_err(|error| format!("cannot open {input}: {error}"))?;
    let saved = document.save().map_err(|error| format!("cannot save: {error}"))?;

    write(output, &saved)?;

    if saved == original {
        outln!("identical: {} bytes in, {} bytes out", original.len(), saved.len());
        Ok(())
    } else {
        outln!("DIFFERENT: {} bytes in, {} bytes out", original.len(), saved.len());
        report_differences(&original, &saved);
        Err("the document changed on save".to_owned())
    }
}

/// Opens a document of any kind this program reads and writes it as the kind
/// the output's name says.
fn convert(input: &str, output: &str, filtered: bool) -> Result<(), String> {
    let document = open(input)?;
    let extension = Path::new(output)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let name = Path::new(output).file_name().and_then(|name| name.to_str()).unwrap_or(output);
    let bytes = match extension.as_str() {
        // A web page, with its pictures in the folder Word names after it.
        "htm" | "html" => {
            let page = if filtered {
                wp_html::write_filtered(&document, name, None)
            } else {
                wp_html::write(&document, name, None)
            };
            let folder = Path::new(output).parent().map(Path::to_path_buf).unwrap_or_default();
            for picture in &page.pictures {
                let target = folder.join(&picture.name);
                if let Some(parent) = target.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                std::fs::write(&target, &picture.bytes)
                    .map_err(|error| format!("cannot write {}: {error}", target.display()))?;
            }
            page.html.into_bytes()
        }
        "mht" | "mhtml" => wp_html::write_mht(&document, name, None),
        "doc" => wp_doc::save(&document),
        "rtf" => wp_rtf::write(&document),
        "odt" => wp_odt::save(&document).map_err(|error| format!("cannot save: {error}"))?,
        _ => {
            let mut document = document;
            if let Some(kind) = wp_docx::kinds::Kind::of_extension(&extension) {
                document.set_kind(kind);
            }
            document.save().map_err(|error| format!("cannot save: {error}"))?
        }
    };
    write(output, &bytes)?;
    outln!("{input} written as {output}: {} bytes", bytes.len());
    Ok(())
}

/// Opens, saves and compares every document in a directory of real files.
///
/// Ends unhappily only when a document would not open or would not save,
/// which is a bug. A document that comes back changed is a measurement, and a
/// harness that failed on those would be one nobody could run.
fn corpus_command(directory: &str) -> Result<(), String> {
    let directory = Path::new(directory);
    let reports = corpus::survey(directory);
    for line in corpus::lines(directory, &reports) {
        outln!("{line}");
    }

    let failed = corpus::summarise(&reports).failed;
    if failed == 0 {
        Ok(())
    } else {
        Err(format!("{failed} document(s) could not be opened or saved"))
    }
}

/// Draws every document in a corpus and scores its pages against Word's own.
///
/// The score is written to the corpus's own history file so that it can be
/// seen to move, and the report says which way it went since the run before.
/// Only a run that measured something is written down: a run that found no
/// reference pages is not a point on the graph.
fn fidelity_command(directory: &str) -> Result<(), String> {
    let corpus = Path::new(directory);
    let verdicts = fidelity::run(corpus);

    let history = corpus.join(fidelity::HISTORY);
    let before = std::fs::read_to_string(&history).unwrap_or_default();
    let last = before.lines().last().map(str::to_owned);
    for line in fidelity::lines(corpus, &verdicts, last.as_deref()) {
        outln!("{line}");
    }

    let (_, _, documents, _) = fidelity::total(&verdicts);
    if documents > 0 {
        let line = fidelity::history_line(&verdicts, &now(), &fidelity::commit(Path::new(".")));
        let mut text = before;
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&line);
        text.push('\n');
        write(&history.to_string_lossy(), text.as_bytes())?;
        outln!();
        outln!("{}", history.display());
    }

    let failed = verdicts
        .iter()
        .filter(|verdict| matches!(verdict.judgement, fidelity::Judgement::Failed(_)))
        .count();
    if failed == 0 {
        Ok(())
    } else {
        Err(format!("{failed} document(s) could not be drawn"))
    }
}

/// Reads every macro of every document in a corpus.
///
/// Ends unhappily when a module would not parse or did not come back as it
/// went in: both of those are this program's fault and not the document's,
/// which is the difference between this and the corpus round trip.
fn vba_command(directory: &str) -> Result<(), String> {
    let corpus = Path::new(directory);
    let reports = vba::run(corpus);
    for line in vba::lines(corpus, &reports) {
        outln!("{line}");
    }

    let (modules, understood, whole) = vba::total(&reports);
    if understood == modules && whole == modules {
        return Ok(());
    }
    Err(format!(
        "of {modules} module(s), {} were not read and {} did not come back as they went in",
        modules - understood,
        modules - whole
    ))
}

/// Runs whichever of the Unicode conformance suites are in a directory.
///
/// Reports rather than gates: a failing case is a gap that is written down in
/// the roadmap, and the number is what this exists to produce. It ends
/// unhappily only when a file is there and cannot be read at all.
fn conformance_command(directory: &str) -> Result<(), String> {
    let where_they_are = Path::new(directory);
    let reports = conformance::run(where_they_are);

    let history = where_they_are.join(conformance::HISTORY);
    let before = std::fs::read_to_string(&history).unwrap_or_default();
    let last = before.lines().last().map(str::to_owned);
    for line in conformance::lines(where_they_are, &reports, last.as_deref()) {
        outln!("{line}");
    }

    let (suites, _, _) = conformance::total(&reports);
    if suites > 0 {
        let line = conformance::history_line(&reports, &now(), &fidelity::commit(Path::new(".")));
        let mut text = before;
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&line);
        text.push('\n');
        write(&history.to_string_lossy(), text.as_bytes())?;
        outln!();
        outln!("{}", history.display());
    }

    let unreadable = reports
        .iter()
        .filter(|(_, report)| matches!(report, conformance::Report::Unreadable(_)))
        .count();
    if unreadable == 0 {
        Ok(())
    } else {
        Err(format!("{unreadable} of the suites could not be read"))
    }
}

/// Replaces text throughout a document and reports what that touched.
fn replace(input: &str, output: &str, from: &str, to: &str) -> Result<(), String> {
    let original = read(input)?;
    let mut document =
        Document::open(&original).map_err(|error| format!("cannot open {input}: {error}"))?;

    let count = document.replace_text(from, to);
    let saved = document.save().map_err(|error| format!("cannot save: {error}"))?;
    write(output, &saved)?;

    outln!("replaced {count} occurrence(s) of {from:?} with {to:?}");
    outln!();
    report_part_changes(&original, &saved);
    Ok(())
}

/// Adds a paragraph at the end of a document.
fn append(input: &str, output: &str, text: &str) -> Result<(), String> {
    let original = read(input)?;
    let mut document =
        Document::open(&original).map_err(|error| format!("cannot open {input}: {error}"))?;

    if !document.append_paragraph(&Paragraph::text(text)) {
        return Err("the document has no body to append to".to_owned());
    }
    let saved = document.save().map_err(|error| format!("cannot save: {error}"))?;
    write(output, &saved)?;

    outln!("appended a paragraph");
    outln!();
    report_part_changes(&original, &saved);
    Ok(())
}

/// Draws a document's pages as images.
///
/// There is no window yet, so this is how the rendering stack can be looked at:
/// the same layout and drawing code a window will use, writing to a file
/// Times what a person waits for, on a document that is not a toy.
///
/// Four numbers, because they are the four waits: opening a file, laying it
/// out, typing one letter into the middle of it, and saving it. The one that
/// matters most is the third — it happens on every keystroke, and a person
/// notices a tenth of a second.
fn bench(pages: &str) -> Result<(), String> {
    let wanted: usize = pages.parse().map_err(|_| format!("not a number of pages: {pages}"))?;
    let library = wp_layout::FontLibrary::scan_system();
    if library.is_empty() {
        return Err("no usable fonts were found on this machine".to_owned());
    }

    // A document of about the size asked for: a page of A4 holds some fifty
    // lines, and these paragraphs are four lines each.
    let paragraphs = wanted * 12;
    let mut body = Body::default();
    for number in 0..paragraphs {
        if number % 12 == 0 {
            body.blocks.push(Block::Paragraph(
                Paragraph::text(&format!("Chapter {}", number / 12 + 1)).with_style("Heading1"),
            ));
        }
        body.blocks.push(Block::Paragraph(Paragraph::text(
            "Some words that go on for long enough to fill several lines of a page, so that \
             the line breaking has something to decide and the paragraph is the size of a \
             paragraph somebody would really write in a document of this length.",
        )));
    }

    let built = Document::create(&body).map_err(|error| format!("cannot build: {error}"))?;
    let bytes = built.save().map_err(|error| format!("cannot save: {error}"))?;
    outln!("document: {paragraphs} paragraphs, {} bytes on disk", bytes.len());

    let start = Instant::now();
    let mut document = Document::open(&bytes).map_err(|error| format!("cannot open: {error}"))?;
    let opening = start.elapsed();

    let mut engine = wp_layout::LayoutEngine::new(&library);
    let start = Instant::now();
    let laid = engine.layout_document(&document);
    let layout = start.elapsed();

    // A letter typed into the middle of the document, and the pages worked out
    // again — which is what happens on every keystroke.
    let middle = document.paragraph_count() / 2;
    document.set_caret(wp_docx::TextPosition::new(middle, 0));
    let start = Instant::now();
    document.insert_text(wp_docx::TextPosition::new(middle, 0), "x");
    let typing = start.elapsed();

    let pages_before = laid.len();
    let start = Instant::now();
    let after = engine.layout_document_again(
        &document,
        wp_layout::PageMetrics::from_document(&document),
        laid,
    );
    let relayout = start.elapsed();
    let placed = engine.blocks_placed();

    // And a paragraph break, which is the other thing a person types: it
    // moves everything after it down a line, so the pages after it cannot
    // be kept, and the layout runs to the end of the document. Word does the
    // same, in the background.
    let metrics = wp_layout::PageMetrics::from_document(&document);
    document.set_caret(wp_docx::TextPosition::new(middle, 1));
    document.type_text("\n");
    let start = Instant::now();
    let after = engine.layout_document_again(&document, metrics, after);
    let breaking = start.elapsed();
    let placed_after_break = engine.blocks_placed();

    // What a hundred letters cost, which is what a person types in half a
    // minute — and what the undo history has to hold afterwards. Typed the way
    // a person types: on from where the last letter went, with spaces between
    // the words, because a space is where undo breaks a step.
    let before_typing = memory_in_use();
    let start = Instant::now();
    document.set_caret(wp_docx::TextPosition::new(middle, 0));
    for index in 0..100 {
        document.type_text(if index % 6 == 5 { " " } else { "x" });
    }
    let hundred = start.elapsed();
    let after_typing = memory_in_use();

    let start = Instant::now();
    let saved = document.save().map_err(|error| format!("cannot save: {error}"))?;
    let saving = start.elapsed();

    outln!("pages: {pages_before} before the edit, {} after", after.len());
    outln!();
    outln!("{:<26} {:>10}", "WHAT", "TIME");
    outln!("{:<26} {:>10}", "opening the file", took(opening));
    outln!("{:<26} {:>10}", "laying it out", took(layout));
    outln!("{:<26} {:>10}", "typing one letter", took(typing));
    outln!("{:<26} {:>10}", "laying it out again", took(relayout));
    outln!("{:<26} {:>10}", "  blocks placed", format!("{placed} of {paragraphs}"));
    outln!("{:<26} {:>10}", "after a paragraph break", took(breaking));
    outln!("{:<26} {:>10}", "  blocks placed", format!("{placed_after_break} of {paragraphs}"));
    outln!("{:<26} {:>10}", "saving it", took(saving));
    outln!("{:<26} {:>10}", "typing a hundred letters", took(hundred));
    outln!("{:<26} {:>10}", "  undo steps kept", document.undo_depth().to_string());
    if let (Some(before), Some(after)) = (before_typing, after_typing) {
        outln!("{:<26} {:>10}", "  which cost, in memory", size(after.saturating_sub(before)));
    }
    if let Some(now) = memory_in_use() {
        outln!("{:<26} {:>10}", "memory in use", size(now));
    }
    outln!();
    outln!("bytes written: {}", saved.len());

    // The one that decides whether the program is usable on a document this
    // size, said out loud rather than left to be worked out from the table.
    let keystroke = typing + relayout;
    outln!();
    outln!("a keystroke costs {} on {wanted} pages", took(keystroke));
    Ok(())
}

/// A duration written the way a person reads one.
fn took(duration: std::time::Duration) -> String {
    let millis = duration.as_secs_f64() * 1000.0;
    if millis >= 1000.0 {
        format!("{:.2} s", millis / 1000.0)
    } else if millis >= 1.0 {
        format!("{millis:.1} ms")
    } else {
        format!("{:.0} us", millis * 1000.0)
    }
}

/// Writes a document out as a PDF.
///
/// The pages are laid out for paper rather than for a screen — seventy-two dots
/// to the inch, which is one dot to the point, the unit a PDF measures in.
fn pdf(input: &str, output: &str) -> Result<(), String> {
    let document = open(input)?;

    let library = wp_layout::FontLibrary::scan_system();
    if library.is_empty() {
        return Err("no usable fonts were found on this machine".to_owned());
    }

    let mut engine = wp_layout::LayoutEngine::for_device(&library, wp_layout::Device::paper());
    let pages = engine.layout_document(&document);
    outln!("pages laid out: {}", pages.len());

    let title = std::path::Path::new(input)
        .file_stem()
        .map_or_else(|| input.to_owned(), |stem| stem.to_string_lossy().into_owned());
    let bytes = wp_pdf::write(&pages, &library, &title);
    write(output, &bytes)?;

    let glyphs: usize = pages.iter().map(|page| page.glyphs.len()).sum();
    outln!("{output}  {} pages, {glyphs} glyphs, {} bytes", pages.len(), bytes.len());
    Ok(())
}

/// instead of to the screen.
fn render(input: &str, prefix: &str, dpi: &str) -> Result<(), String> {
    let dpi: f32 = dpi.parse().map_err(|_| format!("not a resolution: {dpi}"))?;
    let document = open(input)?;

    let library = wp_layout::FontLibrary::scan_system();
    if library.is_empty() {
        return Err("no usable fonts were found on this machine".to_owned());
    }
    outln!("fonts available: {} faces", library.faces().len());

    let mut engine = wp_layout::LayoutEngine::new(&library).with_dpi(dpi);
    let pages = engine.layout_document(&document);
    outln!("pages laid out: {}", pages.len());

    let mut renderer = wp_layout::Renderer::new(&library);
    for (number, page) in pages.iter().enumerate() {
        let canvas = renderer.render(page, wp_raster::Color::WHITE);
        let image = wp_raster::encode_png(&canvas);
        let path = format!("{prefix}-{:02}.png", number + 1);
        write(&path, &image)?;
        outln!(
            "  {path}  {}x{} pixels, {} glyphs, {} bytes",
            canvas.pixel_width(),
            canvas.pixel_height(),
            page.glyphs.len(),
            image.len()
        );
    }
    outln!("distinct glyphs drawn: {}", renderer.cached_glyphs());

    Ok(())
}

/// Lists the font families this machine has.
fn fonts() -> Result<(), String> {
    let library = wp_layout::FontLibrary::scan_system();
    if library.is_empty() {
        return Err("no usable fonts were found on this machine".to_owned());
    }

    outln!("{} faces in {} families", library.faces().len(), library.families().len());
    outln!();
    for family in library.families() {
        let styles: Vec<&str> = library
            .faces()
            .iter()
            .filter(|face| face.family == family)
            .map(|face| match (face.bold, face.italic) {
                (false, false) => "regular",
                (true, false) => "bold",
                (false, true) => "italic",
                (true, true) => "bold italic",
            })
            .collect();
        outln!("{family}  [{}]", styles.join(", "));
    }

    Ok(())
}

/// Shows which parts of the package an edit touched.
///
/// This is the point of the exercise: exactly one part should differ, and every
/// other part — including anything this program does not understand — should
/// come out byte for byte as it went in.
fn report_part_changes(original: &[u8], saved: &[u8]) {
    let (Ok(before), Ok(after)) = (wp_opc::Package::open(original), wp_opc::Package::open(saved))
    else {
        outln!("(one of the two is not a readable package)");
        return;
    };

    let mut unchanged = 0usize;
    let mut changed = Vec::new();

    for entry in before.entries() {
        match after.part(&entry.name) {
            None => changed.push(format!("lost:     {}", entry.name)),
            Some(data) if data != entry.data => {
                changed.push(format!(
                    "changed:  {} ({} -> {} bytes)",
                    entry.name,
                    entry.data.len(),
                    data.len()
                ));
            }
            Some(_) => unchanged += 1,
        }
    }
    for entry in after.entries() {
        if before.part(&entry.name).is_none() {
            changed.push(format!("added:    {}", entry.name));
        }
    }

    outln!("parts unchanged: {unchanged}");
    if changed.is_empty() {
        outln!("no part changed");
    } else {
        for line in changed {
            outln!("{line}");
        }
    }
}

/// Says which parts differ, which is far more useful than a byte offset.
fn report_differences(original: &[u8], saved: &[u8]) {
    let (Ok(before), Ok(after)) = (wp_opc::Package::open(original), wp_opc::Package::open(saved))
    else {
        outln!("  (one of the two is not a readable package)");
        return;
    };

    for entry in before.entries() {
        match after.part(&entry.name) {
            None => outln!("  lost: {}", entry.name),
            Some(data) if data != entry.data => outln!("  changed: {}", entry.name),
            Some(_) => {}
        }
    }
    for entry in after.entries() {
        if before.part(&entry.name).is_none() {
            outln!("  added: {}", entry.name);
        }
    }
}

/// Shortens the long URIs that content and relationship types use.
fn short_type(uri: &str) -> String {
    uri.rsplit(['/', '.']).next().unwrap_or(uri).to_owned()
}

// --- The demonstration document ---------------------------------------------

/// Builds a document covering what the model can express.
fn demonstration_body() -> Body {
    let mut body = Body::default();

    body.blocks.push(Block::Paragraph(
        Paragraph::text("Word Processor").with_style("Title").with_alignment(Alignment::Center),
    ));

    body.blocks.push(Block::Paragraph(
        Paragraph::from_runs(vec![Run::text(
            "Written from scratch in Rust, with no third-party code",
        )
        .italic()
        .colored("595959")])
        .with_alignment(Alignment::Center),
    ));

    body.blocks.push(heading("Character formatting"));
    body.blocks.push(Block::Paragraph(Paragraph::from_runs(vec![
        Run::text("This paragraph mixes "),
        Run::text("bold").bold(),
        Run::text(", "),
        Run::text("italic").italic(),
        Run::text(", "),
        Run::text("underline").underlined(),
        Run::text(", "),
        Run::text("strikethrough").struck_through(),
        Run::text(", "),
        Run::text("colour").colored("C00000"),
        Run::text(" and "),
        Run::text("size").sized(18.0),
        Run::text(" in a single line."),
    ])));

    body.blocks.push(heading("Writing systems"));
    body.blocks.push(Block::Paragraph(Paragraph::text(
        "Support for the world's scripts is designed in from the start, not added at the end.",
    )));

    for (language, sample) in [
        ("en-GB", "The quick brown fox jumps over the lazy dog."),
        ("ru-RU", "Съешь же ещё этих мягких французских булок."),
        ("el-GR", "Ταχίστη αλώπηξ βαφής ψημένη γη."),
        ("hi-IN", "वह क्षमा और साहस का प्रतीक है।"),
        ("th-TH", "เป็นมนุษย์สุดประเสริฐเลิศคุณค่า"),
        ("zh-CN", "永和九年，歲在癸丑，暮春之初。"),
        ("ja-JP", "いろはにほへと ちりぬるを"),
        ("ko-KR", "다람쥐 헌 쳇바퀴에 타고파."),
    ] {
        body.blocks.push(Block::Paragraph(Paragraph::from_runs(vec![Run::text(&format!(
            "{language}  {sample}"
        ))
        .in_language(language)])));
    }

    body.blocks.push(Block::Paragraph(Paragraph::text(
        "Right-to-left scripts need more than the right characters: the paragraph itself has a \
         direction, and the run must be marked, or the text lays out backwards.",
    )));

    for (language, sample) in [
        ("ar", "نص حكيم له سر قاطع وذو شأن عظيم مكتوب على ثوب أخضر."),
        ("he", "דג סקרן שט בים מאוכזב ולפתע מצא חברה."),
    ] {
        body.blocks.push(Block::Paragraph(
            Paragraph::from_runs(vec![Run::text(sample).right_to_left().in_language(language)])
                .right_to_left()
                .with_alignment(Alignment::Start),
        ));
    }

    body.blocks.push(heading("Alignment"));
    for (alignment, label) in [
        (Alignment::Start, "Aligned to the start edge."),
        (Alignment::Center, "Centred."),
        (Alignment::End, "Aligned to the end edge."),
    ] {
        body.blocks.push(Block::Paragraph(Paragraph::text(label).with_alignment(alignment)));
    }

    body.blocks.push(heading("Tables"));
    body.blocks.push(Block::Table(Box::new(
        Table::from_rows(vec![
            table_row(&["Layer", "What it does"], true),
            table_row(&["wp-deflate", "DEFLATE, zlib, CRC-32"], false),
            table_row(&["wp-zip", "ZIP archives, including Zip64"], false),
            table_row(&["wp-xml", "XML with namespaces"], false),
            table_row(&["wp-opc", "Parts, content types, relationships"], false),
            table_row(&["wp-docx", "The document body and styles"], false),
        ])
        .with_grid(vec![2400, 5600])
        .with_borders(TableBorders::grid()),
    )));

    body
}

fn heading(text: &str) -> Block {
    Block::Paragraph(Paragraph::text(text).with_style("Heading1"))
}

fn table_row(cells: &[&str], header: bool) -> TableRow {
    TableRow::from_cells(
        cells
            .iter()
            .map(|text| {
                let run = if header { Run::text(text).bold() } else { Run::text(text) };
                TableCell::from_blocks(vec![Block::Paragraph(Paragraph::from_runs(vec![run]))])
            })
            .collect(),
    )
}

/// What the signatures on a document say, and whether they hold.
fn signatures(path: &str) -> Result<(), String> {
    let document = open(path)?;
    let signatures = document.signatures();
    if signatures.is_empty() {
        println!("{path} is not signed");
        return Ok(());
    }
    for signature in signatures {
        println!("{}", signature.part);
        println!("  signed by   {}", signature.certificate.subject);
        println!("  issued by   {}", signature.certificate.issuer);
        println!(
            "  good from   {} to {}",
            signature.certificate.not_before, signature.certificate.not_after
        );
        println!("  signed at   {}", signature.signed_at);
        if !signature.reason.is_empty() {
            println!("  because     {}", signature.reason);
        }
        println!("  covers      {} parts", signature.parts.len());
        println!("  standing    {}", signature.standing.label());
    }
    Ok(())
}

/// Signs a document with a certificate and its key.
///
/// Both are given as files because there is nowhere else to get them from:
/// this program does not read the certificate store the operating system
/// keeps, which is the next thing to write and is named in the roadmap.
fn sign(path: &str, out: &str, certificate: &str, key: &str, why: &str) -> Result<(), String> {
    let document = open(path)?;
    let certificate = std::fs::read(certificate)
        .map_err(|error| format!("cannot read {certificate}: {error}"))?;
    let key_bytes = std::fs::read(key).map_err(|error| format!("cannot read {key}: {error}"))?;
    let private = wp_asn1::private_key(&key_bytes)
        .ok_or_else(|| format!("{key} is not a key this program reads"))?;

    // Whoever it is, said plainly, so that signing does not quietly go
    // through with a certificate the person did not mean.
    if let Some(read) = wp_asn1::Certificate::read(&certificate) {
        println!("signing as {}", read.subject);
    } else {
        return Err(String::from("that is not a certificate this program reads"));
    }

    let signer = wp_sign::Signer {
        certificate,
        // The command line takes one certificate and no chain; the program
        // itself carries what its store has. See wp-app's certificates.
        chain: Vec::new(),
        key: Box::new(wp_rsa::PrivateKey::new(&private.modulus, &private.exponent)),
        reason: why.to_owned(),
        at: now(),
        // Signed about the document rather than about one of its signature
        // lines: the command line has no way to pick a line and no person
        // in front of it to pick one.
        line: String::new(),
    };
    let bytes = document.save_signed(&signer).map_err(|error| error.to_string())?;
    std::fs::write(out, bytes).map_err(|error| format!("cannot write {out}: {error}"))?;
    println!("wrote {out}");
    Ok(())
}

/// This moment, as a signature writes one.
fn now() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or_default();
    let (days, rest) = (seconds / 86_400, seconds % 86_400);
    let (year, month, day) = civil_from_days(days as i64);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// The calendar arithmetic, which is the same everywhere and is written out
/// rather than asked of a library this program does not have.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let of_era = shifted.rem_euclid(146_097);
    let year_of_era = (of_era - of_era / 1460 + of_era / 36_524 - of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let of_year = of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * of_year + 2) / 153;
    let day = (of_year - (153 * month_prime + 2) / 5 + 1) as u32;
    let month = if month_prime < 10 { month_prime + 3 } else { month_prime - 9 } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}
