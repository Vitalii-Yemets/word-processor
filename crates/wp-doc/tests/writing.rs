//! Tests of the Word 97-2003 files this program writes.
//!
//! Reading one back with this program's own reader proves only that the two
//! halves agree. What proves the file is a `.doc` is another program opening
//! it: Word, where there is one, and here LibreOffice, which is in the build
//! image — it reads what was written and converts it to a `.docx`, and the
//! `.docx` is what these tests look at.

use std::process::{Command, Output};
use std::sync::Mutex;

use wp_docx::model::{
    Alignment, Block, Body, NumberingReference, Paragraph, Run, RunContent, Table, TableCell,
    TableRow,
};
use wp_docx::{Document, TextPosition};

/// LibreOffice takes turns with itself.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn run(command: &mut Command) -> std::io::Result<Output> {
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    command.output()
}

/// A small red picture, as a PNG.
fn picture() -> Vec<u8> {
    let mut canvas = wp_raster::Canvas::new(24, 16);
    canvas.fill_rect(0, 0, 24, 16, wp_raster::Color::rgb(0xCC, 0x22, 0x22));
    wp_raster::encode_png(&canvas)
}

/// A document with one of everything the writer writes.
fn document() -> Document {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("Chapter one").with_style("Heading1")));

    let mut runs = Vec::new();
    let mut styled = |text: &str, change: &dyn Fn(&mut Run)| {
        let mut run = Run::text(text);
        change(&mut run);
        runs.push(run);
    };
    styled("Bold", &|run| run.properties.bold = Some(true));
    styled(" and ", &|_| {});
    styled("italic", &|run| run.properties.italic = Some(true));
    styled(" and ", &|_| {});
    styled("red", &|run| run.properties.color = Some("FF0000".to_owned()));
    styled(" and ", &|_| {});
    styled("big", &|run| run.properties.size_half_points = Some(32));
    styled(" and ", &|_| {});
    styled("Courier", &|run| run.properties.font = Some("Courier New".to_owned()));
    let mut paragraph = Paragraph::from_runs(runs);
    paragraph.runs.push(Run { content: vec![RunContent::Tab], ..Run::text("") });
    paragraph.runs.push(Run::text("end."));
    paragraph.properties.alignment = Some(Alignment::Center);
    paragraph.properties.indent_start = Some(720);
    paragraph.properties.space_after = Some(240);
    body.blocks.push(Block::Paragraph(paragraph));

    for (text, id) in [
        ("Milk", wp_docx::BULLET_LIST),
        ("Bread", wp_docx::BULLET_LIST),
        ("First", wp_docx::NUMBERED_LIST),
        ("Second", wp_docx::NUMBERED_LIST),
    ] {
        let mut item = Paragraph::text(text);
        item.properties.numbering = Some(NumberingReference { id, level: 0 });
        body.blocks.push(Block::Paragraph(item));
    }

    let cell = |text: &str| TableCell {
        blocks: vec![Block::Paragraph(Paragraph::text(text))],
        ..TableCell::default()
    };
    let rows = vec![
        TableRow { cells: vec![cell("Item"), cell("Qty")], ..TableRow::default() },
        TableRow { cells: vec![cell("Apples"), cell("12")], ..TableRow::default() },
    ];
    body.blocks.push(Block::Table(Box::new(Table {
        rows,
        grid: vec![3000, 1500],
        ..Table::default()
    })));

    body.blocks.push(Block::Paragraph(Paragraph::text("See the site for more: ")));
    body.blocks.push(Block::Paragraph(Paragraph::text(
        "\u{41F}\u{440}\u{438}\u{432}\u{435}\u{442}, \u{43C}\u{438}\u{440} \u{2014} caf\u{E9}.",
    )));

    let bytes = Document::create(&body).expect("a document").save().expect("saved");
    let mut document = Document::open(&bytes).expect("reopened");
    // A link over "site", and a picture at the end of the same paragraph.
    let link_paragraph = 10;
    let text = document.paragraph_text(link_paragraph).expect("the paragraph");
    let site = text.find("site").expect("site");
    document.set_caret(TextPosition::new(link_paragraph, site));
    document.extend_selection_to(TextPosition::new(link_paragraph, site + 4));
    assert!(document.add_hyperlink("https://example.com/", "site"));
    document.set_caret(TextPosition::new(link_paragraph, text.len()));
    assert!(document.insert_picture(&picture(), "png", 914_400, 609_600).expect("put in"));
    document.set_page_margins(1000, 1100, 1200, 1300);
    document
}

/// The document written as a `.doc`, read by LibreOffice, and given back as
/// the `.docx` LibreOffice made of it.
fn through_libreoffice(doc: &[u8]) -> Document {
    // A folder of its own for each call: two tests converting at once must
    // not clear each other's away.
    static CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let call = CALLS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let folder = std::env::temp_dir().join(format!("wp-doc-write-{}-{call}", std::process::id()));
    let _ = std::fs::create_dir_all(&folder);
    let source = folder.join("written.doc");
    std::fs::write(&source, doc).expect("the file written");
    let output = run(Command::new("soffice")
        .arg(format!("-env:UserInstallation=file://{}/profile", folder.display()))
        .args(["--headless", "--convert-to", "docx:MS Word 2007 XML", "--outdir"])
        .arg(&folder)
        .arg(&source))
    .unwrap_or_else(|error| {
        panic!(
            "cannot run soffice: {error}\nthe build image should install libreoffice-writer-nogui"
        )
    });
    assert!(output.status.success(), "soffice failed: {}", String::from_utf8_lossy(&output.stderr));
    let converted = std::fs::read(folder.join("written.docx")).unwrap_or_else(|error| {
        panic!(
            "LibreOffice made nothing of the file ({error}): {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    let _ = std::fs::remove_dir_all(&folder);
    Document::open(&converted).expect("LibreOffice's docx opens")
}

#[test]
fn a_written_document_is_a_compound_file_with_the_word_streams() {
    let bytes = wp_doc::save(&document());
    let file = wp_ole::CompoundFile::open(bytes).expect("a compound file");
    let word = file.stream("WordDocument").expect("a WordDocument stream");
    assert_eq!(&word[..4], &[0xEC, 0xA5, 0xC1, 0x00], "Word 97's block");
    assert!(file.stream("1Table").is_some());
    assert!(file.stream("Data").is_some(), "the picture's stream");
    assert!(wp_doc::looks_binary(&file_bytes()), "the program does not know it for one");
}

fn file_bytes() -> Vec<u8> {
    wp_doc::save(&document())
}

#[test]
fn this_programs_own_reader_reads_it_back() {
    let reading = wp_doc::read(&file_bytes()).expect("read back");
    let text = reading.body.plain_text();
    for wanted in
        ["Chapter one", "Bold and italic and red and big and Courier\tend.", "Milk", "Apples"]
    {
        assert!(text.contains(wanted), "{wanted:?} not in {text:?}");
    }
    assert_eq!(reading.links.len(), 1, "{:?}", reading.links);
    assert_eq!(reading.links[0].address, "https://example.com/");
    assert_eq!(reading.pictures.len(), 1);
    assert!(reading.pictures[0].bytes.starts_with(b"\x89PNG"));
}

#[test]
fn libreoffice_reads_the_text_and_its_formatting() {
    let read = through_libreoffice(&file_bytes());
    let body = read.body();
    let paragraphs = body.paragraphs();
    let texts: Vec<String> = paragraphs.iter().map(|paragraph| paragraph.plain_text()).collect();

    let heading = paragraphs.iter().find(|p| p.plain_text() == "Chapter one").expect("the heading");
    assert_eq!(heading.properties.style.as_deref(), Some("Heading1"), "{texts:?}");

    let formatted = paragraphs
        .iter()
        .find(|p| p.plain_text().starts_with("Bold and"))
        .unwrap_or_else(|| panic!("{texts:?}"));
    assert_eq!(formatted.plain_text(), "Bold and italic and red and big and Courier\tend.");
    assert_eq!(formatted.properties.alignment, Some(Alignment::Center));
    assert_eq!(formatted.properties.indent_start, Some(720));
    let run = |text: &str| {
        formatted
            .runs
            .iter()
            .find(|run| run.plain_text() == text)
            .unwrap_or_else(|| panic!("no run {text:?}: {:?}", formatted.runs))
    };
    assert_eq!(run("Bold").properties.bold, Some(true));
    assert_eq!(run("italic").properties.italic, Some(true));
    assert_eq!(run("red").properties.color.as_deref(), Some("FF0000"));
    assert_eq!(run("big").properties.size_half_points, Some(32));
    assert_eq!(run("Courier").properties.font.as_deref(), Some("Courier New"));

    let unicode = paragraphs.iter().any(|p| p.plain_text() == "\u{41F}\u{440}\u{438}\u{432}\u{435}\u{442}, \u{43C}\u{438}\u{440} \u{2014} caf\u{E9}.");
    assert!(unicode, "{texts:?}");
}

#[test]
fn libreoffice_reads_the_lists_the_table_the_link_the_picture_and_the_page() {
    let read = through_libreoffice(&file_bytes());
    let body = read.body();
    let paragraphs = body.paragraphs();

    // The two lists: LibreOffice gives each its own numbering, and what
    // matters is that one is bullets and one counts.
    let list_of = |text: &str| {
        paragraphs
            .iter()
            .find(|p| p.plain_text() == text)
            .and_then(|p| p.properties.numbering)
            .unwrap_or_else(|| panic!("{text} is in no list"))
    };
    assert_eq!(list_of("Milk").id, list_of("Bread").id);
    assert_eq!(list_of("First").id, list_of("Second").id);
    assert_ne!(list_of("Milk").id, list_of("First").id);
    let format_of = |text: &str| {
        let reference = list_of(text);
        read.numbering().level(reference.id, reference.level).map(|level| level.format.clone())
    };
    assert_eq!(format_of("Milk"), Some(wp_docx::numbering::NumberFormat::Bullet));
    assert_eq!(format_of("First"), Some(wp_docx::numbering::NumberFormat::Decimal));

    // The table, its cells, and their widths.
    let table = body
        .blocks
        .iter()
        .find_map(|block| match block {
            Block::Table(table) => Some(table),
            Block::Paragraph(_) => None,
        })
        .expect("the table");
    assert_eq!(table.rows.len(), 2);
    assert_eq!(table.rows[1].cells.len(), 2);
    assert_eq!(table.rows[1].cells[0].blocks[0].plain_text(), "Apples");
    assert_eq!(table.grid.len(), 2);
    assert!(
        (table.grid[0] - 3000).abs() < 20 && (table.grid[1] - 1500).abs() < 20,
        "{:?}",
        table.grid
    );

    // The link over its word.
    let links = read.hyperlinks();
    assert_eq!(links.len(), 1, "{links:?}");
    assert_eq!(links[0].text, "site");
    assert!(matches!(&links[0].destination, wp_docx::links::Destination::Address(address)
        if address == "https://example.com/"));

    // The picture, its bytes, and its size.
    let picture = paragraphs
        .iter()
        .flat_map(|p| p.runs.iter())
        .flat_map(|run| run.content.iter())
        .find_map(|content| match content {
            RunContent::Picture(picture) => Some(picture.clone()),
            _ => None,
        })
        .expect("the picture");
    let bytes = read.embedded_part(&picture.relationship).expect("its bytes");
    assert!(bytes.starts_with(b"\x89PNG"), "not the PNG that went in");
    assert!((picture.width_emu - 914_400).abs() < 20_000, "{}", picture.width_emu);
    assert!((picture.height_emu - 609_600).abs() < 20_000, "{}", picture.height_emu);

    // And the page.
    assert_eq!(read.page_margins(), (1000, 1100, 1200, 1300));
}
