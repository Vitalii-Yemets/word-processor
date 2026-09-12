//! What is drawn at the ends of a line: `a:headEnd` and `a:tailEnd`.
//!
//! An arrowhead belongs to the line and not to the shape, which is why the same
//! connector is an arrow or is not depending on nothing but this, and why Word
//! offers "Line", "Line Arrow" and "Line Arrow Double" as three things to
//! insert that all insert the same shape.

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
