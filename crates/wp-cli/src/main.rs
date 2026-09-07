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

/// Like `println!`, but survives a closed pipe.
macro_rules! outln {
    () => { write_line("") };
    ($($argument:tt)*) => { write_line(&format!($($argument)*)) };
}

use wp_docx::model::{Alignment, Block, Body, Paragraph, Run, RunContent, RunProperties, Table, TableCell, TableRow};
use wp_docx::Document;

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let command = arguments.first().map(String::as_str);

    let result = match (command, arguments.len()) {
        (Some("new"), 2) => create(&arguments[1]),
        (Some("info"), 2) => info(&arguments[1]),
        (Some("text"), 2) => text(&arguments[1]),
        (Some("outline"), 2) => outline(&arguments[1]),
        (Some("roundtrip"), 3) => roundtrip(&arguments[1], &arguments[2]),
        (Some("replace"), 5) => {
            replace(&arguments[1], &arguments[2], &arguments[3], &arguments[4])
        }
        (Some("append"), 4) => append(&arguments[1], &arguments[2], &arguments[3]),
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

  new <file.docx>              create a demonstration document
  info <file.docx>             show the package structure
  text <file.docx>             print the document text
  outline <file.docx>          print the structure with formatting
  roundtrip <in> <out>         open and save, checking nothing changed

  replace <in> <out> <from> <to>   replace text, across run boundaries
  append <in> <out> <text>         add a paragraph at the end

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
    Document::open(&bytes).map_err(|error| format!("cannot open {path}: {error}"))
}

// --- Commands ---------------------------------------------------------------

/// Writes a document that exercises the features the model supports.
fn create(path: &str) -> Result<(), String> {
    let document = Document::create(&demonstration_body())
        .map_err(|error| format!("cannot build the document: {error}"))?;
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
            outln!("  {:<8} {:<12} {}", relationship.id, short_type(&relationship.kind), relationship.target);
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

/// Prints the structure, with the formatting of each run.
fn outline(path: &str) -> Result<(), String> {
    let document = open(path)?;
    let body = document.body();

    print_blocks(&body.blocks, 0);
    Ok(())
}

fn print_blocks(blocks: &[Block], depth: usize) {
    let indent = "  ".repeat(depth);

    for block in blocks {
        match block {
            Block::Paragraph(paragraph) => {
                let mut description = String::from("paragraph");
                if let Some(style) = &paragraph.style {
                    description.push_str(&format!(" style={style}"));
                }
                if let Some(alignment) = paragraph.alignment {
                    description.push_str(&format!(" align={}", alignment.to_attribute()));
                }
                if paragraph.right_to_left {
                    description.push_str(" rtl");
                }
                outln!("{indent}{description}");

                for run in &paragraph.runs {
                    let formatting = describe(&run.properties);
                    let text = run.plain_text();
                    if text.is_empty() && formatting.is_empty() {
                        continue;
                    }
                    outln!("{indent}  run{formatting}: {text:?}");
                }
            }
            Block::Table(table) => {
                let style = table.style.as_deref().unwrap_or("(none)");
                outln!("{indent}table style={style} rows={}", table.rows.len());
                for (row_index, row) in table.rows.iter().enumerate() {
                    outln!("{indent}  row {row_index}");
                    for (cell_index, cell) in row.cells.iter().enumerate() {
                        outln!("{indent}    cell {cell_index}");
                        print_blocks(&cell.blocks, depth + 3);
                    }
                }
            }
        }
    }
}

/// Summarizes character formatting for the outline.
fn describe(properties: &RunProperties) -> String {
    let mut parts: Vec<String> = Vec::new();
    if properties.bold {
        parts.push("bold".to_owned());
    }
    if properties.italic {
        parts.push("italic".to_owned());
    }
    if properties.underline {
        parts.push("underline".to_owned());
    }
    if properties.strike {
        parts.push("strike".to_owned());
    }
    if let Some(half_points) = properties.size_half_points {
        // The format stores half-points; people think in points.
        parts.push(format!("{}pt", half_points as f64 / 2.0));
    }
    if let Some(color) = &properties.color {
        parts.push(format!("#{color}"));
    }
    if let Some(font) = &properties.font {
        parts.push(font.clone());
    }
    if let Some(language) = &properties.language {
        parts.push(language.clone());
    }
    if properties.right_to_left {
        parts.push("rtl".to_owned());
    }

    if parts.is_empty() {
        String::new()
    } else {
        format!(" [{}]", parts.join(", "))
    }
}

/// Opens a document and saves it again, reporting whether anything changed.
///
/// This is the property the whole design rests on: a document that passes
/// through this program untouched must come out identical. If it does not, the
/// package layer is losing something.
fn roundtrip(input: &str, output: &str) -> Result<(), String> {
    let original = read(input)?;
    let document = Document::open(&original).map_err(|error| format!("cannot open {input}: {error}"))?;
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
                changed.push(format!("changed:  {} ({} -> {} bytes)", entry.name, entry.data.len(), data.len()));
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

    body.blocks.push(Block::Paragraph(Paragraph {
        style: Some("Title".to_owned()),
        alignment: Some(Alignment::Center),
        ..Paragraph::text("Word Processor")
    }));

    body.blocks.push(Block::Paragraph(Paragraph {
        alignment: Some(Alignment::Center),
        runs: vec![Run {
            properties: RunProperties {
                italic: true,
                color: Some("595959".to_owned()),
                ..RunProperties::default()
            },
            content: vec![RunContent::Text(
                "Written from scratch in Rust, with no third-party code".to_owned(),
            )],
        }],
        ..Paragraph::default()
    }));

    body.blocks.push(heading("Character formatting"));
    body.blocks.push(Block::Paragraph(Paragraph {
        runs: vec![
            Run::text("This paragraph mixes "),
            styled("bold", RunProperties { bold: true, ..RunProperties::default() }),
            Run::text(", "),
            styled("italic", RunProperties { italic: true, ..RunProperties::default() }),
            Run::text(", "),
            styled("underline", RunProperties { underline: true, ..RunProperties::default() }),
            Run::text(", "),
            styled("strikethrough", RunProperties { strike: true, ..RunProperties::default() }),
            Run::text(", "),
            styled(
                "colour",
                RunProperties { color: Some("C00000".to_owned()), ..RunProperties::default() },
            ),
            Run::text(" and "),
            styled(
                "size",
                RunProperties { size_half_points: Some(36), ..RunProperties::default() },
            ),
            Run::text(" in a single line."),
        ],
        ..Paragraph::default()
    }));

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
        body.blocks.push(Block::Paragraph(Paragraph {
            runs: vec![Run {
                properties: RunProperties {
                    language: Some(language.to_owned()),
                    ..RunProperties::default()
                },
                content: vec![RunContent::Text(format!("{language}  {sample}"))],
            }],
            ..Paragraph::default()
        }));
    }

    body.blocks.push(Block::Paragraph(Paragraph::text(
        "Right-to-left scripts need more than the right characters: the paragraph itself has a \
         direction, and the run must be marked, or the text lays out backwards.",
    )));

    for (language, sample) in [
        ("ar", "نص حكيم له سر قاطع وذو شأن عظيم مكتوب على ثوب أخضر."),
        ("he", "דג סקרן שט בים מאוכזב ולפתע מצא חברה."),
    ] {
        body.blocks.push(Block::Paragraph(Paragraph {
            right_to_left: true,
            alignment: Some(Alignment::Start),
            runs: vec![Run {
                properties: RunProperties {
                    right_to_left: true,
                    language: Some(language.to_owned()),
                    ..RunProperties::default()
                },
                content: vec![RunContent::Text(sample.to_owned())],
            }],
            ..Paragraph::default()
        }));
    }

    body.blocks.push(heading("Alignment"));
    for (alignment, label) in [
        (Alignment::Start, "Aligned to the start edge."),
        (Alignment::Center, "Centred."),
        (Alignment::End, "Aligned to the end edge."),
    ] {
        body.blocks.push(Block::Paragraph(Paragraph {
            alignment: Some(alignment),
            ..Paragraph::text(label)
        }));
    }

    body.blocks.push(heading("Tables"));
    body.blocks.push(Block::Table(Table {
        style: None,
        rows: vec![
            table_row(&["Layer", "What it does"], true),
            table_row(&["wp-deflate", "DEFLATE, zlib, CRC-32"], false),
            table_row(&["wp-zip", "ZIP archives, including Zip64"], false),
            table_row(&["wp-xml", "XML with namespaces"], false),
            table_row(&["wp-opc", "Parts, content types, relationships"], false),
            table_row(&["wp-docx", "The document body"], false),
        ],
    }));

    body
}

fn heading(text: &str) -> Block {
    Block::Paragraph(Paragraph {
        style: Some("Heading1".to_owned()),
        ..Paragraph::text(text)
    })
}

fn styled(text: &str, properties: RunProperties) -> Run {
    Run { properties, content: vec![RunContent::Text(text.to_owned())] }
}

fn table_row(cells: &[&str], header: bool) -> TableRow {
    TableRow {
        cells: cells
            .iter()
            .map(|text| TableCell {
                blocks: vec![Block::Paragraph(Paragraph {
                    runs: vec![Run {
                        properties: RunProperties { bold: header, ..RunProperties::default() },
                        content: vec![RunContent::Text((*text).to_owned())],
                    }],
                    ..Paragraph::default()
                })],
            })
            .collect(),
    }
}
