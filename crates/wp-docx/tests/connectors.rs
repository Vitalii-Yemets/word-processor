//! What a connector carries: what is drawn at its ends, and what it is
//! fastened to.
//!
//! An arrowhead belongs to the line and not to the shape, which is why the same
//! connector is an arrow or is not depending on nothing but `a:headEnd` and
//! `a:tailEnd`, and why Word offers "Line", "Line Arrow" and "Line Arrow
//! Double" as three things to insert that all insert the same shape.
//!
//! What it is fastened to is the other half: `a:stCxn` and `a:endCxn` name a
//! shape and one of its connection points, and a connector that says them is a
//! connector that follows those shapes about.

use wp_docx::shapes::Shape;
use wp_docx::Document;

/// A document with one drawing in it, built by hand so that reading it is what
/// is being tested rather than writing it.
fn document_with(drawing: &str) -> Vec<u8> {
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
 xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"
 xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"
 xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"
 xmlns:wps="http://schemas.microsoft.com/office/word/2010/wordprocessingShape">
<w:body><w:p><w:r>{drawing}</w:r></w:p></w:body>
</w:document>"#
    );

    let mut package = wp_opc::Package::empty();
    package.add_part("word/document.xml", wp_opc::MAIN_DOCUMENT_CONTENT_TYPE, xml.into_bytes());
    let mut root = wp_opc::Relationships::new("");
    root.add(
        wp_opc::OFFICE_DOCUMENT_RELATIONSHIP,
        "word/document.xml",
        wp_opc::TargetMode::Internal,
    );
    package.set_relationships(&root).expect("the root relationships");
    package.save().expect("a saved package")
}

/// One connector with whatever line is given.
fn connector_with(line: &str) -> String {
    format!(
        r#"<w:drawing><wp:inline><wp:extent cx="914400" cy="457200"/>
<wp:docPr id="1" name="Connector"/>
<a:graphic><a:graphicData uri="http://schemas.microsoft.com/office/word/2010/wordprocessingShape">
<wps:wsp><wps:cNvPr id="2" name="Connector"/><wps:spPr>
<a:xfrm><a:off x="0" y="0"/><a:ext cx="914400" cy="457200"/></a:xfrm>
<a:prstGeom prst="straightConnector1"><a:avLst/></a:prstGeom>
{line}
</wps:spPr></wps:wsp>
</a:graphicData></a:graphic></wp:inline></w:drawing>"#
    )
}

#[test]
fn what_is_drawn_at_the_ends_of_a_line_is_read() {
    use wp_docx::lines::{EndKind, EndSize};

    let line = r#"<a:ln w="19050"><a:solidFill><a:srgbClr val="4472C4"/></a:solidFill>
<a:headEnd type="oval" w="sm" len="sm"/><a:tailEnd type="triangle" w="lg" len="lg"/></a:ln>"#;
    let document =
        Document::open(&document_with(&connector_with(line))).expect("a readable document");
    let shape = document.shapes().into_iter().next().expect("the connector");

    assert_eq!(shape.preset, "straightConnector1");
    assert_eq!(shape.head_end.kind, EndKind::Oval);
    assert_eq!(shape.head_end.width, EndSize::Small);
    assert_eq!(shape.tail_end.kind, EndKind::Triangle);
    assert_eq!(shape.tail_end.length, EndSize::Large);
}

#[test]
fn a_line_that_says_nothing_about_its_ends_has_none() {
    let line = r#"<a:ln w="19050"><a:solidFill><a:srgbClr val="4472C4"/></a:solidFill></a:ln>"#;
    let document = Document::open(&document_with(&connector_with(line))).expect("a document");
    let shape = document.shapes().into_iter().next().expect("the connector");
    assert!(shape.head_end.is_nothing() && shape.tail_end.is_nothing());
}

#[test]
fn a_connector_this_program_writes_carries_its_arrowhead() {
    use wp_docx::lines::{EndKind, LineEnd};

    let shape = Shape {
        preset: "bentConnector3".to_owned(),
        width_emu: 914_400,
        height_emu: 457_200,
        outline: Some("1F3864".to_owned()),
        outline_emu: 19_050,
        tail_end: LineEnd { kind: EndKind::Triangle, ..LineEnd::default() },
        ..Shape::default()
    };
    let mut document = Document::open(&document_with("<w:t>words</w:t>")).expect("a document");
    document.insert_shape(&shape);

    let read = document.shapes().into_iter().next().expect("the shape back");
    assert_eq!(read.tail_end.kind, EndKind::Triangle);
    assert!(read.head_end.is_nothing(), "an end nobody asked for was written");
}

/// A shape that floats at an offset from the margin, with the id a connector
/// would name it by.
fn box_at(id: u32, name: &str, across: i64, down: i64) -> Shape {
    use wp_docx::anchor::{Anchor, Placement, Wrap};

    let mut shape = Shape::preset("rect", 72.0, 36.0).floating(Anchor {
        wrap: Wrap::None,
        horizontal: Placement::Offset(across),
        vertical: Placement::Offset(down),
        ..Anchor::default()
    });
    shape.id = id;
    shape.name = name.to_owned();
    shape
}

#[test]
fn what_a_connector_is_fastened_to_is_read_and_written_back() {
    use wp_docx::joins::{Join, Joins};

    let drawing = r#"<w:drawing><wp:inline><wp:extent cx="914400" cy="457200"/>
<wp:docPr id="1" name="Connector"/>
<a:graphic><a:graphicData uri="http://schemas.microsoft.com/office/word/2010/wordprocessingShape">
<wps:wsp><wps:cNvPr id="5" name="Connector"/>
<wps:cNvCnPr><a:stCxn id="2" idx="3"/><a:endCxn id="4" idx="1"/></wps:cNvCnPr>
<wps:spPr><a:prstGeom prst="bentConnector3"><a:avLst/></a:prstGeom></wps:spPr>
</wps:wsp></a:graphicData></a:graphic></wp:inline></w:drawing>"#;
    let document = Document::open(&document_with(drawing)).expect("a readable document");
    let shape = document.shapes().into_iter().next().expect("the connector");

    assert_eq!(shape.id, 5, "the id a connector would be named by");
    assert_eq!(shape.joins.start, Some(Join { shape: 2, site: 3 }));
    assert_eq!(shape.joins.end, Some(Join { shape: 4, site: 1 }));
    assert!(shape.joins.holds(2) && shape.joins.holds(4));

    // And out again, through the writer this program uses for a new shape.
    let mut written = Document::open(&document_with("<w:t>words</w:t>")).expect("a document");
    written.insert_shape(&shape);
    let back = written.shapes().into_iter().next().expect("the shape back");
    assert_eq!(back.joins, Joins { start: shape.joins.start, end: shape.joins.end });
}

#[test]
fn the_file_follows_a_shape_that_moved() {
    // The screen follows a join at layout time. This is the other half: the box
    // the file gives the connector is put back where the two shapes are, so a
    // document saved here opens in Word with the connector where it was left.
    use wp_docx::anchor::Placement;
    use wp_docx::joins::{Join, Joins};
    use wp_docx::model::{Block, Body, Paragraph, Run, RunContent};

    let mut connector = box_at(3, "Connector", 4_572_000, 4_572_000);
    connector.preset = "straightConnector1".to_owned();
    connector.joins =
        Joins { start: Some(Join { shape: 1, site: 3 }), end: Some(Join { shape: 2, site: 1 }) };

    let runs: Vec<Run> =
        [box_at(1, "First", 0, 0), box_at(2, "Second", 1_828_800, 914_400), connector]
            .into_iter()
            .map(|shape| Run {
                properties: wp_docx::model::RunProperties::default(),
                content: vec![RunContent::Shape(shape)],
                field: None,
                revision: None,
                format_change: None,
            })
            .collect();
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::from_runs(runs)));
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    let mut document = Document::open(&bytes).expect("reopening");

    assert!(document.rejoin_connectors(), "nothing was put back");

    let connector = document
        .shapes()
        .into_iter()
        .find(|shape| shape.name == "Connector")
        .expect("the connector");
    let anchor = connector.anchor.expect("it floats");
    // The right-hand side of the first box: 72 points across and half of 36
    // down, in the format's own units.
    assert_eq!(anchor.horizontal, Placement::Offset(914_400));
    assert_eq!(anchor.vertical, Placement::Offset(228_600));
    // And as wide and as tall as the gap between the two points.
    assert_eq!(connector.width_emu, 1_828_800 - 914_400);
    assert_eq!(connector.height_emu, 914_400 + 228_600 - 228_600);

    // Asking again changes nothing: a connector already where it belongs is
    // not an edit, and a document that marked itself modified every time it was
    // looked at would never stop asking to be saved.
    assert!(!document.rejoin_connectors(), "it was put back twice");
}
