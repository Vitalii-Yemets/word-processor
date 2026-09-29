//! Packages written here by hand, for what LibreOffice does not write when it
//! converts a Word document — the forms of the format another program, or
//! LibreOffice with its own documents, uses: a header for left-hand pages
//! and one for the first page, a section of its own in two columns, a text
//! box in a frame, an ellipse, a frame anchored to the page, the fields
//! Word has names for, a table's header row and the height of a row, a
//! bookmark at a point, the settings, and a page numbered from a number of
//! its own. And a package this program writes with a character style in it,
//! read back.

use wp_docx::furniture::{Furniture, Which};
use wp_docx::model::{Block, Body, Paragraph, Run, RunContent};
use wp_docx::sections::Start;
use wp_docx::{Document, StyleDefinition, StyleKind};

const NAMESPACES: &str = r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" xmlns:xlink="http://www.w3.org/1999/xlink" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:meta="urn:oasis:names:tc:opendocument:xmlns:meta:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" xmlns:config="urn:oasis:names:tc:opendocument:xmlns:config:1.0" office:version="1.3""#;

fn content() -> String {
    format!(
        r##"<?xml version="1.0" encoding="UTF-8"?>
<office:document-content {NAMESPACES}>
<office:automatic-styles>
<style:style style:name="P1" style:family="paragraph" style:parent-style-name="Standard" style:master-page-name="Standard"><style:paragraph-properties style:page-number="5"/></style:style>
<style:style style:name="Sect1" style:family="section"><style:section-properties><style:columns fo:column-count="2" fo:column-gap="0.5in"/></style:section-properties></style:style>
<style:style style:name="fr1" style:family="graphic"><style:graphic-properties style:wrap="none" style:horizontal-pos="center" style:horizontal-rel="page" style:vertical-pos="from-top" style:vertical-rel="page" fo:background-color="#ffffcc" fo:border="0.5pt solid #000000"/></style:style>
<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#00ff00" draw:stroke="none" style:wrap="run-through" style:run-through="background"/></style:style>
<style:style style:name="T1.R1" style:family="table-row"><style:table-row-properties style:row-height="0.5in"/></style:style>
</office:automatic-styles>
<office:body><office:text>
<draw:frame draw:style-name="fr1" draw:name="Notice" text:anchor-type="page" text:anchor-page-number="1" svg:width="3in" svg:height="1in" svg:y="1in"><draw:text-box><text:p>On the page</text:p></draw:text-box></draw:frame>
<text:p text:style-name="P1">Numbered from five, <text:bookmark text:name="here"/>dated <text:date>5 March 2024</text:date>, titled <text:title>A Report</text:title>, see <text:bookmark-ref text:ref-name="here" text:reference-format="page">1</text:bookmark-ref> and figure <text:sequence text:name="Figure">1</text:sequence>.</text:p>
<text:p>An ellipse: <draw:ellipse draw:style-name="gr1" draw:name="Round" text:anchor-type="char" svg:width="1in" svg:height="0.5in" svg:x="0in" svg:y="0in"/> behind.</text:p>
<text:section text:style-name="Sect1" text:name="Two"><text:p>In two columns.</text:p><text:p>Still in two.</text:p></text:section>
<text:p>Back in one.</text:p>
<table:table table:name="T1"><table:table-column table:number-columns-repeated="2"/>
<table:table-header-rows><table:table-row table:style-name="T1.R1"><table:table-cell><text:p>Name</text:p></table:table-cell><table:table-cell><text:p>Value</text:p></table:table-cell></table:table-row></table:table-header-rows>
<table:table-row><table:table-cell><text:p>One</text:p></table:table-cell><table:table-cell><text:p>1</text:p></table:table-cell></table:table-row>
</table:table>
<text:p>The end.</text:p>
</office:text></office:body></office:document-content>"##
    )
}

fn styles() -> String {
    format!(
        r##"<?xml version="1.0" encoding="UTF-8"?>
<office:document-styles {NAMESPACES}>
<office:styles><style:style style:name="Standard" style:family="paragraph"/></office:styles>
<office:automatic-styles><style:page-layout style:name="pm1"><style:page-layout-properties fo:page-width="8.5in" fo:page-height="11in" fo:margin-top="0.5in" fo:margin-bottom="0.5in" fo:margin-left="1in" fo:margin-right="1in" style:num-format="i"/><style:header-style><style:header-footer-properties fo:min-height="0.5in"/></style:header-style></style:page-layout></office:automatic-styles>
<office:master-styles><style:master-page style:name="Standard" style:page-layout-name="pm1">
<style:header><text:p>Right head</text:p></style:header>
<style:header-left><text:p>Left head</text:p></style:header-left>
<style:header-first><text:p>First head</text:p></style:header-first>
</style:master-page></office:master-styles>
</office:document-styles>"##
    )
}

const SETTINGS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<office:document-settings xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:config="urn:oasis:names:tc:opendocument:xmlns:config:1.0"><office:settings>
<config:config-item-set config:name="ooo:view-settings"><config:config-item-map-indexed config:name="Views"><config:config-item-map-entry><config:config-item config:name="ZoomFactor" config:type="short">120</config:config-item></config:config-item-map-entry></config:config-item-map-indexed></config:config-item-set>
<config:config-item-set config:name="ooo:configuration-settings"><config:config-item config:name="LoadReadonly" config:type="boolean">true</config:config-item></config:config-item-set>
</office:settings></office:document-settings>"#;

/// The parts as a package.
fn package() -> Vec<u8> {
    let mut zip = wp_zip::ZipWriter::new();
    zip.add_stored("mimetype", wp_odt::MIME_TYPE.as_bytes()).expect("mimetype");
    zip.add("content.xml", content().as_bytes()).expect("content");
    zip.add("styles.xml", styles().as_bytes()).expect("styles");
    zip.add("settings.xml", SETTINGS.as_bytes()).expect("settings");
    zip.finish().expect("a package")
}

#[test]
fn the_forms_libreoffice_does_not_write_from_word_are_read() {
    let reading = wp_odt::read(&content(), Some(&styles()), None, Some(SETTINGS)).expect("read");

    // The page: its own numbers from five, in small Roman numerals, a header
    // for the left-hand pages and one for the first.
    let first = &reading.sections[0];
    let numbering = first.page.numbering.expect("numbered");
    assert_eq!(numbering.start, Some(5));
    assert_eq!(numbering.format, wp_docx::sections::NumberFormat::LowerRoman);
    assert!(first.page.title_page);
    assert_eq!(first.page.margins[0], 720 + 720, "the header's height is in the top margin");
    let furniture = |which: Which| {
        first
            .furniture
            .iter()
            .find(|found| found.kind == Furniture::Header && found.which == which)
            .map(|found| found.body.plain_text())
    };
    assert_eq!(furniture(Which::Default).as_deref(), Some("Right head"));
    assert_eq!(furniture(Which::Even).as_deref(), Some("Left head"));
    assert_eq!(furniture(Which::First).as_deref(), Some("First head"));
    assert!(reading.facing_pages);

    // The section of its own in two columns, and the one in one after it.
    assert_eq!(reading.sections.len(), 3, "{:?}", reading.sections);
    assert_eq!(reading.sections[1].start, Start::Continuous);
    assert_eq!(reading.sections[1].page.columns, 2);
    assert_eq!(reading.sections[2].page.columns, 1);
    assert_eq!(reading.sections[0].last_paragraph, Some(1));
    assert_eq!(reading.sections[1].last_paragraph, Some(3));

    // The fields.
    let Block::Paragraph(fields) = &reading.body.blocks[0] else { panic!() };
    let field = |text: &str| {
        fields
            .runs
            .iter()
            .find(|run| run.plain_text() == text)
            .and_then(|run| run.field.clone())
            .unwrap_or_else(|| panic!("no field over {text:?}: {:?}", fields.runs))
    };
    assert_eq!(field("5 March 2024"), "DATE");
    assert_eq!(field("A Report"), "TITLE");
    assert_eq!(field("1"), "PAGEREF here \\h");
    assert_eq!(reading.bookmarks.len(), 1);
    assert_eq!(reading.bookmarks[0].start, reading.bookmarks[0].end);

    // The text box on the page, ridden by the first paragraph, and the
    // ellipse behind the text.
    let shapes: Vec<_> = reading
        .body
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph(paragraph) => Some(paragraph),
            Block::Table(_) => None,
        })
        .flat_map(|paragraph| paragraph.runs.iter())
        .flat_map(|run| run.content.iter())
        .filter_map(|content| match content {
            RunContent::Shape(shape) => Some(shape),
            _ => None,
        })
        .collect();
    let notice = shapes.iter().find(|shape| shape.name == "Notice").expect("the text box");
    assert_eq!(notice.text[0].plain_text(), "On the page");
    let anchor = notice.anchor.as_ref().expect("it floats");
    assert_eq!(anchor.horizontal_from, wp_docx::anchor::Relative::Page);
    assert_eq!(anchor.horizontal, wp_docx::anchor::Placement::Aligned("center".to_owned()));
    assert_eq!(anchor.wrap, wp_docx::anchor::Wrap::TopAndBottom);
    let round = shapes.iter().find(|shape| shape.name == "Round").expect("the ellipse");
    assert_eq!(round.preset, "ellipse");
    assert!(round.anchor.as_ref().is_some_and(|anchor| anchor.behind_text));
    assert!(round.outline.is_none());

    // The table's header row and the row's height.
    let table = reading
        .body
        .blocks
        .iter()
        .find_map(|block| match block {
            Block::Table(table) => Some(table),
            Block::Paragraph(_) => None,
        })
        .expect("the table");
    assert!(table.rows[0].is_header);
    assert!(!table.rows[1].is_header);
    assert_eq!((table.rows[0].height, table.rows[0].height_exact), (Some(720), true));

    // How it was last looked at.
    assert_eq!(reading.zoom, Some(120));
    assert!(reading.read_only);

    // And as a document.
    let document = wp_odt::open(&package()).expect("opened");
    assert_eq!(document.zoom_percent(), Some(120));
    assert!(document.write_protection().is_some(), "it does not ask to be read only");
    assert_eq!(document.sections().len(), 3);
}

#[test]
fn a_character_style_is_written_as_one_and_read_back() {
    let mut styled = Run::text("styled");
    styled.properties.style = Some("StrongRed".to_owned());
    let body = Body {
        blocks: vec![Block::Paragraph(Paragraph::from_runs(vec![
            Run::text("Plain and "),
            styled,
            Run::text(" words."),
        ]))],
    };
    let mut document = Document::create(&body).expect("a document");
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

    let package = wp_odt::save(&document).expect("saved");
    let again = wp_odt::open(&package).expect("read back");
    let back = again.body();
    let Block::Paragraph(paragraph) = &back.blocks[0] else { panic!() };
    let run = paragraph.runs.iter().find(|run| run.plain_text() == "styled").expect("the run");
    assert_eq!(run.properties.style.as_deref(), Some("StrongRed"), "{:?}", paragraph.runs);
    let style = again.styles().get("StrongRed").cloned().expect("the style");
    assert_eq!(style.kind, StyleKind::Character);
    assert_eq!(style.run.bold, Some(true));
    assert_eq!(style.run.color.as_deref(), Some("C00000"));
}
