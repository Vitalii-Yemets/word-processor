//! Tests against a binary document written by somebody else.
//!
//! A file written by the reader's own author proves the author's
//! understanding and nothing else. LibreOffice is in the build image to
//! write the files these tests read: a page of everything a document holds,
//! converted to Word 97 by an implementation that is not this one.

use std::process::Command;

use wp_docx::model::{Alignment, Block, RunContent, Underline, VerticalAlignment};

const PAGE: &str = r##"<html><head><meta charset="utf-8"><title>Sample</title></head><body>
<h1>A heading</h1>
<p>Hello, <b>world</b> &#8212; caf&eacute; <i>italic</i> <u>under</u> <s>struck</s> <span style="color:#FF0000">red</span> <span style="font-size:14pt;font-family:'Times New Roman'">Times 14</span> <sup>sup</sup> <span style="background:#FFFF00">high</span>.</p>
<p style="text-align:center">Centred paragraph.</p>
<p style="text-align:justify;margin-left:1in;text-indent:0.5in">Indented and justified paragraph that goes on for a while so that it wraps onto more than one line of the page.</p>
<ul><li>Milk</li><li>Bread</li></ul>
<ol><li>First</li><li>Second</li></ol>
<table border="1"><tr><td>Item</td><td>Qty</td><td>Price</td></tr><tr><td>Apples</td><td>12</td><td>3.60</td></tr></table>
<p>A picture: <img src="data:image/png;base64,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" width="96" height="96"> and a <a href="https://example.com/">link</a> after it.</p>
<p>Привет, мир — Unicode text.</p>
</body></html>
"##;

/// LibreOffice, converting to Word 97 into a folder — with a profile of its
/// own in that folder, because two of it running at once share one
/// otherwise and the second refuses to start.
fn soffice(folder: &std::path::Path) -> Command {
    let mut command = Command::new("soffice");
    command
        .arg(format!("-env:UserInstallation=file://{}/profile", folder.display()))
        .args(["--headless", "--convert-to", "doc:MS Word 97", "--outdir"])
        .arg(folder);
    command
}

/// The page as a Word 97 document, written by LibreOffice.
fn converted() -> Vec<u8> {
    let folder = std::env::temp_dir().join(format!("wp-doc-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&folder);
    let page = folder.join("sample.html");
    std::fs::write(&page, PAGE).expect("the page written");
    let status = soffice(&folder).arg(&page).output().unwrap_or_else(|error| {
        panic!(
            "cannot run soffice: {error}\nthe build image should install libreoffice-writer-nogui"
        )
    });
    assert!(status.status.success(), "soffice failed: {}", String::from_utf8_lossy(&status.stderr));
    let bytes = std::fs::read(folder.join("sample.doc")).expect("the document converted");
    let _ = std::fs::remove_dir_all(&folder);
    bytes
}

fn document_and_reading() -> (wp_docx::Document, wp_doc::Reading) {
    let bytes = converted();
    let reading = wp_doc::read(&bytes).expect("read");
    let document = wp_doc::open(&bytes).expect("opened");
    (document, reading)
}

#[test]
fn a_real_document_reads_to_its_text_and_formatting() {
    let (document, reading) = document_and_reading();
    let blocks = &reading.body.blocks;

    let Block::Paragraph(heading) = &blocks[0] else { panic!("{blocks:?}") };
    assert_eq!(heading.plain_text(), "A heading");
    assert_eq!(heading.properties.style.as_deref(), Some("Heading1"));

    let Block::Paragraph(hello) = &blocks[1] else { panic!() };
    assert_eq!(
        hello.plain_text(),
        "Hello, world \u{2014} caf\u{e9} italic under struck red Times 14 sup high."
    );
    let run = |text: &str| {
        hello
            .runs
            .iter()
            .find(|run| run.plain_text() == text)
            .unwrap_or_else(|| panic!("no run {text:?}: {:?}", hello.runs))
    };
    assert_eq!(run("world").properties.bold, Some(true));
    assert_eq!(run("italic").properties.italic, Some(true));
    assert_eq!(run("under").properties.underline, Some(Underline::Single));
    assert_eq!(run("struck").properties.strike, Some(true));
    assert_eq!(run("red").properties.color.as_deref(), Some("FF0000"));
    assert_eq!(run("Times 14").properties.font.as_deref(), Some("Times New Roman"));
    assert_eq!(run("Times 14").properties.size_half_points, Some(28));
    // LibreOffice writes a superscript as text raised by so many points,
    // not as Word's superscript switch; either is the word up in the air.
    let sup = &run("sup").properties;
    assert!(
        sup.vertical_align == Some(VerticalAlignment::Superscript)
            || sup.position_half_points.is_some_and(|raised| raised > 0),
        "{sup:?}"
    );
    assert_eq!(run("high").properties.highlight.as_deref(), Some("yellow"));

    let Block::Paragraph(centred) = &blocks[2] else { panic!() };
    assert_eq!(centred.properties.alignment, Some(Alignment::Center));
    let Block::Paragraph(indented) = &blocks[3] else { panic!() };
    assert_eq!(indented.properties.alignment, Some(Alignment::Both));
    assert_eq!(indented.properties.indent_start, Some(1440));
    assert_eq!(indented.properties.indent_first_line, Some(720));

    let Block::Paragraph(milk) = &blocks[4] else { panic!() };
    assert_eq!(milk.plain_text(), "Milk");
    assert_eq!(milk.properties.numbering.map(|n| n.id), Some(wp_docx::BULLET_LIST));
    let Block::Paragraph(first) = &blocks[6] else { panic!() };
    assert_eq!(first.plain_text(), "First");
    assert_eq!(first.properties.numbering.map(|n| n.id), Some(wp_docx::NUMBERED_LIST));

    let Block::Table(table) = &blocks[8] else { panic!("no table: {:?}", blocks[8].plain_text()) };
    assert_eq!(table.rows.len(), 2);
    assert_eq!(table.rows[0].cells.len(), 3);
    assert_eq!(table.rows[1].cells[2].blocks[0].plain_text(), "3.60");
    assert!(
        table.rows[0].cells.iter().all(|cell| cell.width.is_some_and(|w| w > 0)),
        "{:?}",
        table.grid
    );

    let Block::Paragraph(picture) = &blocks[9] else { panic!() };
    assert_eq!(
        picture.plain_text(),
        format!("A picture: {} and a link after it.", wp_doc::PICTURE_MARK)
    );
    assert_eq!(reading.pictures.len(), 1);
    assert!(
        reading.pictures[0].bytes.starts_with(b"\x89PNG"),
        "the picture's bytes are not the PNG"
    );
    assert_eq!(reading.pictures[0].extension, "png");
    assert_eq!(reading.links.len(), 1);
    assert_eq!(reading.links[0].address, "https://example.com/");

    let Block::Paragraph(unicode) = &blocks[10] else { panic!() };
    assert_eq!(unicode.plain_text(), "Привет, мир \u{2014} Unicode text.");

    // And as a document: the picture in, the link over its word.
    let links = document.hyperlinks();
    assert_eq!(links.len(), 1, "{links:?}");
    assert_eq!(links[0].text, "link");
    let has_picture = document.body().paragraphs().iter().any(|paragraph| {
        paragraph
            .runs
            .iter()
            .any(|run| run.content.iter().any(|c| matches!(c, RunContent::Picture(_))))
    });
    assert!(has_picture, "the picture was not put in");
    let (width, height) = document.page_size();
    assert!(width > 10000 && height > width, "the page was not read: {width}x{height}");
}

/// A document this program wrote, taken through LibreOffice to Word 97 and
/// read back: the round trip a person makes when they send a file to
/// somebody with an old Word.
#[test]
fn a_document_of_this_programs_own_survives_the_old_format() {
    use wp_docx::model::{Body, NumberingReference, Paragraph, Run};

    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("The Title").with_style("Title")));
    body.blocks.push(Block::Paragraph(Paragraph::text("Chapter one").with_style("Heading1")));
    let mut bold = Run::text("Bold");
    bold.properties.bold = Some(true);
    let mut paragraph =
        Paragraph::from_runs(vec![bold, Run::text(" and plain, with a tab\tafter.")]);
    paragraph.properties.alignment = Some(Alignment::End);
    body.blocks.push(Block::Paragraph(paragraph));
    let mut item = Paragraph::text("A bullet");
    item.properties.numbering = Some(NumberingReference { id: wp_docx::BULLET_LIST, level: 0 });
    body.blocks.push(Block::Paragraph(item));
    let docx = wp_docx::Document::create(&body).expect("a document").save().expect("saved");

    let folder = std::env::temp_dir().join(format!("wp-doc-own-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&folder);
    let source = folder.join("own.docx");
    std::fs::write(&source, docx).unwrap();
    let output = soffice(&folder).arg(&source).output().expect("soffice runs");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let bytes = std::fs::read(folder.join("own.doc")).expect("converted");
    let _ = std::fs::remove_dir_all(&folder);

    let reading = wp_doc::read(&bytes).expect("read");
    let blocks = &reading.body.blocks;
    assert_eq!(
        reading.body.plain_text(),
        "The Title\nChapter one\nBold and plain, with a tab\tafter.\nA bullet"
    );
    let Block::Paragraph(title) = &blocks[0] else { panic!() };
    assert_eq!(title.properties.style.as_deref(), Some("Title"));
    let Block::Paragraph(heading) = &blocks[1] else { panic!() };
    assert_eq!(heading.properties.style.as_deref(), Some("Heading1"));
    let Block::Paragraph(paragraph) = &blocks[2] else { panic!() };
    assert_eq!(paragraph.properties.alignment, Some(Alignment::End));
    assert_eq!(paragraph.runs[0].properties.bold, Some(true));
    assert!(
        paragraph.runs.iter().any(|run| run.content.iter().any(|c| matches!(c, RunContent::Tab))),
        "the tab was lost"
    );
    let Block::Paragraph(item) = &blocks[3] else { panic!() };
    assert_eq!(item.properties.numbering.map(|n| n.id), Some(wp_docx::BULLET_LIST));
}
