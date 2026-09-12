//! The values behind a shape's yellow handles: `a:avLst`.
//!
//! What a shape looks like is not settled by its preset alone. A rounded
//! rectangle whose corner was dragged square is a rectangle, and a document
//! that says so says it here. These tests are about reading that, keeping it,
//! and writing it back as it came.

use wp_docx::shapes::Shape;
use wp_docx::Document;

/// A document with one shape in it, built by hand so that reading it is what is
/// being tested rather than writing it.
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

/// One rounded rectangle, with whatever list of adjustments is given.
fn rounded_with(values: &str) -> String {
    format!(
        r#"<w:drawing><wp:inline><wp:extent cx="914400" cy="457200"/>
<wp:docPr id="1" name="Box"/>
<a:graphic><a:graphicData uri="http://schemas.microsoft.com/office/word/2010/wordprocessingShape">
<wps:wsp><wps:cNvPr id="2" name="Box"/><wps:spPr>
<a:xfrm><a:off x="0" y="0"/><a:ext cx="914400" cy="457200"/></a:xfrm>
<a:prstGeom prst="roundRect"><a:avLst>{values}</a:avLst></a:prstGeom>
<a:solidFill><a:srgbClr val="4472C4"/></a:solidFill>
</wps:spPr></wps:wsp>
</a:graphicData></a:graphic></wp:inline></w:drawing>"#
    )
}

#[test]
fn a_handle_a_document_moved_is_read() {
    let drawing = rounded_with(r#"<a:gd name="adj" fmla="val 40000"/>"#);
    let document = Document::open(&document_with(&drawing)).expect("a readable document");
    let shape = document.shapes().into_iter().next().expect("the shape");

    assert_eq!(shape.preset, "roundRect");
    assert_eq!(shape.adjusts, vec![("adj".to_owned(), 40_000)]);
    assert_eq!(shape.adjust("adj"), Some(40_000));
}

#[test]
fn a_shape_with_an_empty_list_has_no_adjustments() {
    // Which is not the same as having them at zero: a rounded rectangle with no
    // `adj` has round corners and one with `adj` at zero has square ones.
    let document = Document::open(&document_with(&rounded_with(""))).expect("a document");
    let shape = document.shapes().into_iter().next().expect("the shape");
    assert!(shape.adjusts.is_empty());
    assert_eq!(shape.adjust("adj"), None);
}

#[test]
fn the_same_handle_is_found_under_either_of_its_names() {
    // A shape with one handle is written `adj` by some programs and `adj1` by
    // others, and it is the same handle.
    let drawing = rounded_with(r#"<a:gd name="adj1" fmla="val 12500"/>"#);
    let document = Document::open(&document_with(&drawing)).expect("a document");
    let shape = document.shapes().into_iter().next().expect("the shape");
    assert_eq!(shape.adjust("adj"), Some(12_500));
    assert_eq!(shape.adjust("adj1"), Some(12_500));
}

#[test]
fn a_formula_that_is_not_a_value_is_left_alone() {
    // `val 25000` is the only formula an adjustment ever uses. One saying
    // anything else is one this program cannot work out, and a shape drawn at
    // the wrong proportion because a formula was misread is worse than one
    // drawn at the proportion the format falls back on.
    let drawing = rounded_with(r#"<a:gd name="adj" fmla="*/ 100 w ss"/>"#);
    let document = Document::open(&document_with(&drawing)).expect("a document");
    let shape = document.shapes().into_iter().next().expect("the shape");
    assert!(shape.adjusts.is_empty(), "a formula nobody can read was read anyway");
}

#[test]
fn several_handles_are_read_in_the_order_they_were_written() {
    let drawing =
        rounded_with(r#"<a:gd name="adj1" fmla="val 50000"/><a:gd name="adj2" fmla="val 25000"/>"#);
    let document = Document::open(&document_with(&drawing)).expect("a document");
    let shape = document.shapes().into_iter().next().expect("the shape");
    assert_eq!(shape.adjusts, vec![("adj1".to_owned(), 50_000), ("adj2".to_owned(), 25_000)]);
}

#[test]
fn saving_a_document_leaves_a_handle_nobody_moved_exactly_as_it_was() {
    // The whole point of keeping the value rather than the shape it makes: a
    // document saved by this program is a document Word opens as the same
    // drawing, down to the number.
    let drawing = rounded_with(r#"<a:gd name="adj" fmla="val 40000"/>"#);
    let document = Document::open(&document_with(&drawing)).expect("a document");
    let saved = document.save().expect("a saved document");
    let package = wp_opc::Package::open(&saved).expect("a package");
    let xml = String::from_utf8(package.part("word/document.xml").expect("the part").to_vec())
        .expect("text");
    assert!(xml.contains(r#"<a:gd name="adj" fmla="val 40000"/>"#), "{xml}");
}

#[test]
fn a_shape_this_program_writes_carries_its_handles() {
    let shape = Shape {
        preset: "roundRect".to_owned(),
        adjusts: vec![("adj".to_owned(), 30_000)],
        width_emu: 914_400,
        height_emu: 457_200,
        ..Shape::default()
    };
    let mut document = Document::open(&document_with("<w:t>words</w:t>")).expect("a document");
    document.insert_shape(&shape);

    let read = document.shapes().into_iter().next().expect("the shape back");
    assert_eq!(read.adjusts, vec![("adj".to_owned(), 30_000)]);
}
