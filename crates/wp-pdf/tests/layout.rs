//! What a PDF's pages say beyond their lines of text, worked back out:
//! the header and footer every page repeats, footnotes, tables drawn with
//! rules across them only or with none, text drawn turned, and the order
//! of columns that end at different heights.
//!
//! The document is made here, saved as a `.docx`, and printed to PDF by
//! LibreOffice — its own layout, its own way of drawing all of these — and
//! the reader has to get each back from the pages alone.

use std::path::Path;
use std::process::Command;

use wp_docx::furniture::{Furniture, Preset};
use wp_docx::model::{
    Alignment, Block, Body, Border, BreakKind, Paragraph, Run, RunContent, Table, TableBorders,
    TableCell, TableRow, TextDirection,
};
use wp_docx::notes::Kind;
use wp_docx::sections::Start;
use wp_docx::{Document, TextPosition};

fn soffice(folder: &Path, file: &Path) {
    let output = Command::new("soffice")
        .arg(format!("-env:UserInstallation=file://{}/profile", folder.display()))
        .args(["--headless", "--convert-to", "pdf", "--outdir"])
        .arg(folder)
        .arg(file)
        .output()
        .unwrap_or_else(|error| {
            panic!("cannot run soffice: {error}\nthe build image should install libreoffice-writer-nogui")
        });
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}

const WORDS: [&str; 12] = [
    "harvest", "ledger", "meadow", "council", "orchard", "granary", "bridge", "market", "mill",
    "quarry", "chapel", "ferry",
];

/// A paragraph of body text long enough to take several lines.
fn prose(index: usize) -> String {
    let mut text = format!("Paragraph {index} of the report tells of the");
    for step in 0..30 {
        text.push(' ');
        text.push_str(WORDS[(index * 7 + step) % WORDS.len()]);
    }
    text.push('.');
    text
}

fn cell(text: &str) -> TableCell {
    TableCell::text(text)
}

fn rule() -> Option<Border> {
    Some(Border::line("single", 8, None))
}

/// The document: prose with a footnote over two pages and more, a table
/// with rules across it only, one with no rules, one with a cell turned,
/// and three columns of different lengths.
fn document() -> Document {
    let mut blocks = vec![Block::Paragraph(Paragraph::text(
        "The report opens with a remark that wants a note at its foot",
    ))];
    for index in 1..=14 {
        blocks.push(Block::Paragraph(Paragraph::text(&prose(index))));
    }
    let header_cells = ["Item", "Qty", "Price"].map(|text| TableCell {
        borders: TableBorders { bottom: rule(), ..TableBorders::default() },
        ..cell(text)
    });
    let row =
        |texts: [&str; 3]| TableRow { cells: texts.map(cell).to_vec(), ..TableRow::default() };
    blocks.push(Block::Table(Box::new(Table {
        rows: vec![
            TableRow { cells: header_cells.to_vec(), ..TableRow::default() },
            row(["Apples", "12", "3.60"]),
            row(["Pears", "4", "1.20"]),
            row(["Plums", "30", "9.00"]),
        ],
        grid: vec![2400, 1600, 1600],
        borders: TableBorders { top: rule(), bottom: rule(), ..TableBorders::default() },
        ..Table::default()
    })));
    blocks.push(Block::Paragraph(Paragraph::text("Between the two tables.")));
    blocks.push(Block::Table(Box::new(Table {
        rows: vec![
            row(["North", "17", "4.50"]),
            row(["South", "7", "2.25"]),
            row(["East", "25", "8.75"]),
        ],
        grid: vec![2400, 1600, 1600],
        borders: TableBorders::default(),
        ..Table::default()
    })));
    blocks.push(Block::Paragraph(Paragraph::text("Before the turned cell.")));
    blocks.push(Block::Table(Box::new(Table {
        rows: vec![TableRow {
            cells: vec![
                TableCell { direction: TextDirection::Up, ..cell("Upwards") },
                cell("Level text beside it."),
            ],
            height: Some(1600),
            height_exact: true,
            ..TableRow::default()
        }],
        grid: vec![900, 4000],
        borders: TableBorders::grid(),
        ..Table::default()
    })));
    // The columns: the first long, the second short, the third between,
    // each ended by a column break.
    let column = |name: &str, paragraphs: usize| -> Vec<Block> {
        (0..paragraphs)
            .map(|index| {
                let mut paragraph = Paragraph::text(&format!(
                    "Column {name} line {index} of the three column part."
                ));
                if index + 1 == paragraphs && name != "three" {
                    paragraph.runs.push(Run {
                        content: vec![RunContent::Break(BreakKind::Column)],
                        ..Run::text("")
                    });
                }
                Block::Paragraph(paragraph)
            })
            .collect()
    };
    let columns_start = blocks.len();
    blocks.extend(column("one", 7));
    blocks.extend(column("two", 2));
    blocks.extend(column("three", 4));
    let mut document = Document::create(&Body { blocks }).expect("a document");
    document.set_caret(TextPosition::new(0, document.paragraph_text(0).unwrap().len()));
    document.add_note(Kind::Footnote, "A note at the foot of the page.").expect("a note");
    document
        .set_furniture(
            Furniture::Header,
            Preset::Text,
            Alignment::Start,
            "Annual report of the society",
        )
        .expect("a header");
    document
        .set_furniture(Furniture::Footer, Preset::PageOfTotal, Alignment::Center, "")
        .expect("a footer");
    // The columns start where their first paragraph does: the paragraph
    // count, since each table's cells hold one paragraph each.
    let paragraph_of_columns = (0..document.paragraph_count())
        .find(|&index| {
            document.paragraph_text(index).is_some_and(|t| t.starts_with("Column one line 0"))
        })
        .expect("the columns' first paragraph");
    let _ = columns_start;
    document.set_caret(TextPosition::new(paragraph_of_columns, 0));
    // On a page of their own, so that their lengths are what is set here.
    assert!(document.insert_section_break(Start::NextPage));
    assert!(document.set_columns(3, 360));
    document
}

fn printed() -> Vec<u8> {
    let folder = std::env::temp_dir().join(format!("wp-pdf-layout-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&folder);
    let path = folder.join("report.docx");
    std::fs::write(&path, document().save().expect("saved")).unwrap();
    soffice(&folder, &path);
    let pdf = std::fs::read(folder.join("report.pdf")).expect("printed");
    let _ = std::fs::remove_dir_all(&folder);
    pdf
}

fn texts(body: &Body) -> Vec<String> {
    body.blocks.iter().map(Block::plain_text).collect()
}

#[test]
fn a_pdfs_layout_is_read_back() {
    let pdf = printed();
    let document = wp_pdf::open(&pdf).expect("opened");
    let body = document.body();
    let all = texts(&body).join("\n");

    // The header and footer, out of the text and into their own.
    assert!(!all.contains("Annual report"), "{all}");
    assert!(!all.contains(" of 3") && !all.contains(" of 4"), "{all}");
    let header = document.furniture(Furniture::Header).expect("a header");
    assert_eq!(texts(&header).join("|").trim(), "Annual report of the society");
    let footer = document.furniture(Furniture::Footer).expect("a footer");
    let fields: Vec<&str> = footer
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph(paragraph) => Some(paragraph),
            _ => None,
        })
        .flat_map(|paragraph| paragraph.runs.iter().filter_map(|run| run.field.as_deref()))
        .collect();
    assert!(fields.iter().any(|f| f.trim() == "PAGE"), "{fields:?} in {:?}", texts(&footer));
    assert!(fields.iter().any(|f| f.trim() == "NUMPAGES"), "{fields:?}");

    // The footnote, with its reference where it was.
    let notes = document.notes(Kind::Footnote);
    assert_eq!(notes.len(), 1, "{all}");
    assert_eq!(notes[0].text.trim(), "A note at the foot of the page.");
    assert!(!all.contains("A note at the foot"), "{all}");
    assert!(
        all.starts_with("The report opens with a remark that wants a note at its foot\n"),
        "{all}"
    );

    // The tables with no rules between their columns.
    let tables: Vec<&Table> = body
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Table(table) => Some(table.as_ref()),
            _ => None,
        })
        .collect();
    let rows_of = |table: &Table| -> Vec<Vec<String>> {
        table
            .rows
            .iter()
            .map(|row| {
                row.cells
                    .iter()
                    .map(|cell| cell.blocks.iter().map(Block::plain_text).collect::<String>())
                    .collect()
            })
            .collect()
    };
    let across = tables
        .iter()
        .find(|t| rows_of(t).first().is_some_and(|r| r.first().is_some_and(|c| c == "Item")))
        .unwrap_or_else(|| panic!("no ruled table: {all}"));
    assert_eq!(
        rows_of(across),
        vec![
            vec!["Item", "Qty", "Price"],
            vec!["Apples", "12", "3.60"],
            vec!["Pears", "4", "1.20"],
            vec!["Plums", "30", "9.00"]
        ]
    );
    assert!(across.borders.top.is_some() && across.borders.bottom.is_some());
    assert!(across.borders.inside_vertical.is_none());
    let unruled = tables
        .iter()
        .find(|t| rows_of(t).first().is_some_and(|r| r.first().is_some_and(|c| c == "North")))
        .unwrap_or_else(|| panic!("no unruled table: {all}"));
    assert_eq!(
        rows_of(unruled),
        vec![vec!["North", "17", "4.50"], vec!["South", "7", "2.25"], vec!["East", "25", "8.75"]]
    );
    assert!(unruled.borders.top.is_none());

    // The turned text, whole, and not a letter a line.
    let paragraphs = texts(&body);
    assert!(paragraphs.iter().any(|p| p == "Upwards"), "{all}");
    assert!(!paragraphs.iter().any(|p| p == "U" || p == "s"), "{all}");

    // The columns, read one after another.
    let at = |needle: &str| {
        paragraphs
            .iter()
            .position(|p| p.contains(needle))
            .unwrap_or_else(|| panic!("no {needle}: {all}"))
    };
    let (one, two, three) =
        (at("Column one line 6"), at("Column two line 0"), at("Column three line 0"));
    assert!(one < two && two < three, "{all}");
    assert!(at("Column one line 0") < one && at("Column two line 1") < three, "{all}");
}
