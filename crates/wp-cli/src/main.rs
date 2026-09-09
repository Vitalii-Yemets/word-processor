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

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let command = arguments.first().map(String::as_str);

    let result = match (command, arguments.len()) {
        (Some("new"), 2) => create(&arguments[1]),
        (Some("info"), 2) => info(&arguments[1]),
        (Some("text"), 2) => text(&arguments[1]),
        (Some("outline"), 2) => outline(&arguments[1]),
        (Some("roundtrip"), 3) => roundtrip(&arguments[1], &arguments[2]),
        (Some("replace"), 5) => replace(&arguments[1], &arguments[2], &arguments[3], &arguments[4]),
        (Some("append"), 4) => append(&arguments[1], &arguments[2], &arguments[3]),
        (Some("render"), 3) => render(&arguments[1], &arguments[2], "96"),
        (Some("render"), 4) => render(&arguments[1], &arguments[2], &arguments[3]),
        (Some("pdf"), 3) => pdf(&arguments[1], &arguments[2]),
        (Some("bench"), 1) => bench("100"),
        (Some("bench"), 2) => bench(&arguments[1]),
        (Some("fonts"), 1) => fonts(),
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

  render <in.docx> <prefix> [dpi]  draw the pages as PNG images
  pdf <in.docx> <out.pdf>         write the pages out as a PDF
  bench [pages]                   time what a person waits for
  fonts                            list the fonts found on this machine

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

    let start = Instant::now();
    let after = engine.layout_document(&document);
    let relayout = start.elapsed();

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

    outln!("pages: {} before the edit, {} after", laid.len(), after.len());
    outln!();
    outln!("{:<26} {:>10}", "WHAT", "TIME");
    outln!("{:<26} {:>10}", "opening the file", took(opening));
    outln!("{:<26} {:>10}", "laying it out", took(layout));
    outln!("{:<26} {:>10}", "typing one letter", took(typing));
    outln!("{:<26} {:>10}", "laying it out again", took(relayout));
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
            canvas.width(),
            canvas.height(),
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
