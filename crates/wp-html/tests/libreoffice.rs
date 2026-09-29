//! A web page written by another program, read here.
//!
//! A document with one of everything is made here, saved as a `.docx`, and
//! handed to LibreOffice — which is in the build image — to be written as a
//! web page. That page is then opened by this crate, and what LibreOffice
//! carries into a page is looked for in what came out: its notes, its
//! header, its tables with their merges, lines and colours and the table
//! inside one, its bordered paragraph, its right-to-left one and its page.

use std::process::{Command, Output};
use std::sync::Mutex;

use wp_docx::anchor::Anchor;
use wp_docx::colour::Colour;
use wp_docx::fills::Fill;
use wp_docx::furniture::{Furniture, Which};
use wp_docx::model::{
    Alignment, Block, Body, Border, Paragraph, ParagraphBorders, Revision, RevisionKind, Run,
    Table, TableBorders, TableCell, TableRow,
};
use wp_docx::notes::Kind;
use wp_docx::sections::Start;
use wp_docx::shapes::Shape;
use wp_docx::{Document, StyleDefinition, StyleKind, TextPosition};

/// LibreOffice takes turns with itself.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn run(command: &mut Command) -> std::io::Result<Output> {
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    command.output()
}

const ARABIC: &str =
    "\u{645}\u{631}\u{62D}\u{628}\u{627} \u{628}\u{627}\u{644}\u{639}\u{627}\u{644}\u{645}";

fn cell(text: &str) -> TableCell {
    TableCell::text(text)
}

/// The table: a cell across two columns, one down two rows, one shaded and
/// ruled of its own, and one holding a table of its own.
fn table() -> Table {
    let inner = Table {
        rows: vec![TableRow { cells: vec![cell("In one"), cell("In two")], ..TableRow::default() }],
        grid: vec![1200, 1200],
        borders: TableBorders::grid(),
        ..Table::default()
    };
    let wide = TableCell { width: Some(4000), span: 2, ..cell("Wide") };
    let tall = cell("Tall");
    let shaded = TableCell {
        shading: Some("D9E2F3".to_owned()),
        borders: TableBorders {
            top: Some(Border::line("double", 6, Some("FF0000"))),
            bottom: Some(Border::line("double", 6, Some("FF0000"))),
            ..TableBorders::default()
        },
        ..cell("Shaded")
    };
    let holder = TableCell {
        blocks: vec![Block::Table(Box::new(inner)), Block::Paragraph(Paragraph::text(""))],
        ..TableCell::default()
    };
    let under = TableCell { merged_upwards: true, ..TableCell::default() };
    Table {
        rows: vec![
            TableRow { cells: vec![wide, cell("Corner")], ..TableRow::default() },
            TableRow { cells: vec![tall, shaded, holder], ..TableRow::default() },
            TableRow { cells: vec![under, cell("Left"), cell("Right")], ..TableRow::default() },
        ],
        grid: vec![2000, 2000, 3000],
        borders: TableBorders::grid(),
        style: Some("TableGrid".to_owned()),
        ..Table::default()
    }
}

fn revised(text: &str, kind: RevisionKind) -> Run {
    Run {
        revision: Some(Revision {
            kind,
            author: "Kim Smith".to_owned(),
            date: "2024-03-05T10:30:00Z".to_owned(),
            id: 1,
        }),
        ..Run::text(text)
    }
}

fn paragraph_of(document: &Document, text: &str) -> usize {
    (0..document.paragraph_count())
        .find(|index| document.paragraph_text(*index).is_some_and(|found| found == text))
        .unwrap_or_else(|| panic!("no paragraph {text:?}"))
}

fn select(document: &mut Document, paragraph: usize, word: &str) {
    let text = document.paragraph_text(paragraph).expect("the paragraph");
    let start = text.find(word).expect("the word");
    document.set_caret(TextPosition::new(paragraph, start));
    document.extend_selection_to(TextPosition::new(paragraph, start + word.len()));
}

/// A document with one of everything.
fn document() -> Document {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("Chapter one").with_style("Heading1")));
    body.blocks.push(Block::Paragraph(Paragraph::text("A sentence with a note and an end.")));
    body.blocks.push(Block::Paragraph(Paragraph::text("Some marked and noted text.")));
    body.blocks.push(Block::Paragraph(Paragraph::from_runs(vec![
        Run::text("Kept "),
        revised("added ", RevisionKind::Inserted),
        revised("removed ", RevisionKind::Deleted),
        Run::text("end."),
    ])));
    body.blocks.push(Block::Paragraph(Paragraph::from_runs(vec![
        Run::text("Pages: "),
        Run { field: Some("NUMPAGES".to_owned()), ..Run::text("1") },
    ])));
    let mut boxed = Paragraph::text("A boxed and shaded paragraph.");
    let line = || Some(Border::line("single", 12, Some("0000FF")));
    boxed.properties.borders =
        ParagraphBorders { top: line(), start: line(), bottom: line(), end: line(), between: None };
    boxed.properties.shading = Some("FFFF00".to_owned());
    body.blocks.push(Block::Paragraph(boxed));
    let mut arabic = Paragraph::from_runs(vec![Run::text(ARABIC)]);
    arabic.properties.right_to_left = Some(true);
    arabic.runs[0].properties.right_to_left = Some(true);
    body.blocks.push(Block::Paragraph(arabic));
    let mut styled = Run::text("styled");
    styled.properties.style = Some("StrongRed".to_owned());
    body.blocks.push(Block::Paragraph(Paragraph::from_runs(vec![
        Run::text("Plain and "),
        styled,
        Run::text(" words."),
    ])));
    body.blocks.push(Block::Paragraph(Paragraph::text("A quoted block.").with_style("QuoteBlock")));
    body.blocks.push(Block::Table(Box::new(table())));
    body.blocks.push(Block::Paragraph(Paragraph::text("After the table.")));
    body.blocks.push(Block::Paragraph(Paragraph::text("A drawing: ")));
    body.blocks.push(Block::Paragraph(Paragraph::text("Section two")));

    let mut document = Document::create(&body).expect("a document");

    // The styles the text names.
    let mut quote = StyleDefinition {
        id: "QuoteBlock".to_owned(),
        name: "Quote Block".to_owned(),
        based_on: Some("Normal".to_owned()),
        next: None,
        paragraph: Default::default(),
        run: Default::default(),
    };
    quote.paragraph.indent_start = Some(720);
    quote.run.italic = Some(true);
    assert!(document.set_style_of_kind(&quote, StyleKind::Paragraph));
    let mut strong = StyleDefinition {
        id: "StrongRed".to_owned(),
        name: "Strong Red".to_owned(),
        based_on: None,
        next: None,
        paragraph: Default::default(),
        run: Default::default(),
    };
    strong.run.bold = Some(true);
    strong.run.color = Some("C00000".to_owned());
    assert!(document.set_style_of_kind(&strong, StyleKind::Character));
    let grid = StyleDefinition {
        id: "TableGrid".to_owned(),
        name: "Table Grid".to_owned(),
        based_on: None,
        next: None,
        paragraph: Default::default(),
        run: Default::default(),
    };
    assert!(document.set_table_style_definition(&grid, &TableBorders::grid()));

    // A footnote after "note" and an endnote after "end".
    let notes = paragraph_of(&document, "A sentence with a note and an end.");
    let text = document.paragraph_text(notes).expect("text");
    let after_note = text.find("note").expect("note") + 4;
    document.set_caret(TextPosition::new(notes, after_note));
    document.add_note(Kind::Footnote, "The footnote says this.").expect("a footnote");
    let text = document.paragraph_text(notes).expect("text");
    let after_end = text.find("end").expect("end") + 3;
    document.set_caret(TextPosition::new(notes, after_end));
    document.add_note(Kind::Endnote, "The endnote says that.").expect("an endnote");

    // A bookmark over "marked" and a comment over "noted".
    let marked = paragraph_of(&document, "Some marked and noted text.");
    select(&mut document, marked, "marked");
    assert!(document.add_bookmark("marked_place"));
    select(&mut document, marked, "noted");
    document
        .add_comment("A comment on it.", "Kim Smith", "2024-03-05T10:30:00Z")
        .expect("a comment");
    document.clear_selection();

    // A floating ellipse and a text box.
    let drawing = paragraph_of(&document, "A drawing: ");
    document.set_caret(TextPosition::new(drawing, "A drawing: ".len()));
    let mut ellipse = Shape::preset("ellipse", 72.0, 36.0).floating(Anchor::default());
    ellipse.fill = Fill::Solid(Colour::rgb("FF0000"));
    ellipse.outline = Some(Colour::rgb("000000"));
    assert!(document.insert_shape(&ellipse));
    let text_box = Shape::text_box(144.0, 72.0, "Boxed words").floating(Anchor::default());
    assert!(document.insert_shape(&text_box));

    // Two sections: the second on its side, in two columns, with a header of
    // its own; the first with margins of its own, a header, a footer with the
    // page number, and a first page different from the rest.
    let second = paragraph_of(&document, "Section two");
    assert!(document.end_section_at(second - 1, Start::NextPage));
    document.set_caret(TextPosition::new(second, 0));
    document.set_landscape(true);
    document.set_columns(2, 720);
    let head = |text: &str| Body { blocks: vec![Block::Paragraph(Paragraph::text(text))] };
    document
        .set_furniture_body(Furniture::Header, Which::Default, &head("Second head"))
        .expect("header");
    document.set_caret(TextPosition::new(0, 0));
    document.set_page_margins(1000, 1100, 1200, 1300);
    document
        .set_furniture_body(Furniture::Header, Which::Default, &head("Running head"))
        .expect("header");
    let footer = Body {
        blocks: vec![Block::Paragraph(Paragraph::from_runs(vec![
            Run::text("Page "),
            Run { field: Some("PAGE".to_owned()), ..Run::text("1") },
        ]))],
    };
    document.set_furniture_body(Furniture::Footer, Which::Default, &footer).expect("footer");
    document.set_different_first_page(true);
    document
        .set_furniture_body(Furniture::Header, Which::First, &head("First page head"))
        .expect("header");
    document
}

/// The document as a `.docx`, converted to RTF by LibreOffice.
/// The document as a web page by LibreOffice, opened here with its
/// pictures beside it.
fn through_libreoffice(docx: &[u8]) -> Document {
    // A folder of its own for each call: two tests converting at once must
    // not clear each other's away.
    static CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let call = CALLS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let folder = std::env::temp_dir().join(format!("wp-html-read-{}-{call}", std::process::id()));
    let _ = std::fs::create_dir_all(&folder);
    let source = folder.join("written.docx");
    std::fs::write(&source, docx).expect("the file written");
    let output = run(Command::new("soffice")
        .arg(format!("-env:UserInstallation=file://{}/profile", folder.display()))
        .args(["--headless", "--convert-to", "html", "--outdir"])
        .arg(&folder)
        .arg(&source))
    .unwrap_or_else(|error| {
        panic!(
            "cannot run soffice: {error}\nthe build image should install libreoffice-writer-nogui"
        )
    });
    assert!(output.status.success(), "soffice failed: {}", String::from_utf8_lossy(&output.stderr));
    let page = folder.join("written.html");
    let bytes = std::fs::read(&page).unwrap_or_else(|error| {
        panic!(
            "LibreOffice made nothing of the file ({error}): {}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    if let Ok(dump) = std::env::var("WP_HTML_DUMP") {
        std::fs::write(format!("{dump}/lo.html"), &bytes).expect("dumped");
    }
    let document = wp_html::open_html(&bytes, Some(&page)).expect("opened");
    let _ = std::fs::remove_dir_all(&folder);
    document
}

fn paragraph<'a>(body: &'a Body, text: &str) -> &'a Paragraph {
    body.paragraphs()
        .into_iter()
        .find(|paragraph| paragraph.plain_text() == text)
        .unwrap_or_else(|| panic!("no paragraph {text:?} in {:?}", body.plain_text()))
}

#[test]
fn libreoffice_web_pages_are_read_with_their_notes_header_and_page() {
    let document = through_libreoffice(&document().save().expect("saved"));

    let footnotes = document.notes(Kind::Footnote);
    assert_eq!(footnotes.len(), 1, "{footnotes:?}");
    assert_eq!(footnotes[0].text, "The footnote says this.");
    let endnotes = document.notes(Kind::Endnote);
    assert_eq!(endnotes.len(), 1, "{endnotes:?}");
    assert_eq!(endnotes[0].text, "The endnote says that.");
    let notes = paragraph_of(&document, "A sentence with a note\u{2} and an end\u{2}.");
    assert_eq!(footnotes[0].mark, Some(TextPosition::new(notes, "A sentence with a note".len())));

    assert!(document.bookmark("marked_place").is_some(), "{:?}", document.bookmarks());

    let header =
        document.furniture_of_page(Furniture::Header, 0, Which::Default).expect("a header");
    assert!(header.plain_text().contains("head"), "{header:?}");

    let (width, height) = document.page_size();
    assert!((width - 11_906).abs() < 20 && (height - 16_838).abs() < 20, "{width} x {height}");
}

#[test]
fn libreoffice_web_pages_keep_their_boxes_directions_and_tables() {
    let document = through_libreoffice(&document().save().expect("saved"));
    let body = document.body();

    let boxed = paragraph(&body, "A boxed and shaded paragraph.");
    assert_eq!(boxed.properties.shading.as_deref(), Some("FFFF00"));
    let top = boxed.properties.borders.top.as_ref().expect("a line above");
    assert_eq!((top.style.as_str(), top.color.as_deref()), ("single", Some("0000FF")));

    let arabic = paragraph(&body, ARABIC);
    assert_eq!(arabic.properties.right_to_left, Some(true));
    assert_eq!(arabic.properties.alignment, Some(Alignment::Start));

    let table = body
        .blocks
        .iter()
        .find_map(|block| match block {
            Block::Table(table) => Some(table),
            Block::Paragraph(_) => None,
        })
        .expect("the table");
    assert_eq!(table.rows.len(), 3);
    assert_eq!(table.rows[0].cells[0].span, 2);
    assert!(table.rows[2].cells[0].merged_upwards, "the cell under Tall is not merged into it");
    let shaded = &table.rows[1].cells[1];
    assert_eq!(shaded.shading.as_deref(), Some("D9E2F3"));
    let line = shaded.borders.top.as_ref().expect("its own top line");
    assert_eq!((line.style.as_str(), line.color.as_deref()), ("double", Some("FF0000")));
    let holder = &table.rows[1].cells[2].blocks;
    let inner = holder
        .iter()
        .find_map(|block| match block {
            Block::Table(table) => Some(table),
            Block::Paragraph(_) => None,
        })
        .unwrap_or_else(|| panic!("no table in the cell: {holder:?}"));
    assert_eq!(inner.rows[0].cells[1].blocks[0].plain_text(), "In two");
}
