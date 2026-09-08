//! Reading the pictures a document embeds.
//!
//! The documents here are built part by part rather than through
//! `Document::create`, which writes no drawings: a picture is a part of the
//! package plus a relationship plus a tree of DrawingML, and reading all three
//! back is the thing worth testing.

use wp_docx::model::{Block, RunContent};
use wp_docx::Document;
use wp_opc::{Package, Relationships, TargetMode};

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const RELATIONSHIP_IMAGE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";

/// Builds a document whose one paragraph holds a drawing.
fn document_with_picture(drawing: &str, picture_bytes: &[u8]) -> Vec<u8> {
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="{W}"
 xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"
 xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"
 xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"
 xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture">
<w:body><w:p><w:r><w:t>before</w:t>{drawing}<w:t>after</w:t></w:r></w:p></w:body>
</w:document>"#
    );

    let mut package = Package::empty();
    package.add_part("word/document.xml", wp_opc::MAIN_DOCUMENT_CONTENT_TYPE, xml.into_bytes());
    package.add_part("word/media/image1.png", "image/png", picture_bytes.to_vec());

    let mut root = Relationships::new("");
    root.add(wp_opc::OFFICE_DOCUMENT_RELATIONSHIP, "word/document.xml", TargetMode::Internal);
    package.set_relationships(&root).expect("the root relationships");

    let mut document = Relationships::new("word/document.xml");
    document.add(RELATIONSHIP_IMAGE, "media/image1.png", TargetMode::Internal);
    package.set_relationships(&document).expect("the document relationships");

    package.save().expect("a saved package")
}

/// A drawing the way Word writes one, at a given size in English Metric Units.
fn drawing(relationship: &str, width: i64, height: i64) -> String {
    format!(
        r#"<w:drawing><wp:inline distT="0" distB="0" distL="0" distR="0">
<wp:extent cx="{width}" cy="{height}"/>
<wp:docPr id="1" name="Picture 1" descr="a gradient"/>
<a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture">
<pic:pic><pic:blipFill><a:blip r:embed="{relationship}"/></pic:blipFill></pic:pic>
</a:graphicData></a:graphic>
</wp:inline></w:drawing>"#
    )
}

fn fixture() -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("wp-image")
        .join("tests")
        .join("fixtures")
        .join("gradient.png");
    std::fs::read(path).expect("the picture fixture")
}

/// The one picture of the document, as the model reports it.
fn picture_of(document: &Document) -> wp_docx::model::Picture {
    let body = document.body();
    let Block::Paragraph(paragraph) = &body.blocks[0] else { panic!("not a paragraph") };
    paragraph
        .runs
        .iter()
        .flat_map(|run| run.content.iter())
        .find_map(|piece| match piece {
            RunContent::Picture(picture) => Some(picture.clone()),
            _ => None,
        })
        .expect("a picture in the paragraph")
}

#[test]
fn a_drawing_is_read_as_a_picture() {
    // 914400 English Metric Units to the inch, so this is one inch square.
    let bytes = document_with_picture(&drawing("rId1", 914_400, 914_400), &fixture());
    let document = Document::open(&bytes).expect("a readable document");

    let picture = picture_of(&document);
    assert_eq!(picture.relationship, "rId1");
    assert_eq!(picture.width_emu, 914_400);
    assert!((picture.width_points() - 72.0).abs() < 0.01, "one inch is seventy-two points");
    assert!((picture.height_points() - 72.0).abs() < 0.01);
}

#[test]
fn a_picture_carries_the_words_that_describe_it() {
    let bytes = document_with_picture(&drawing("rId1", 100, 100), &fixture());
    let document = Document::open(&bytes).expect("a readable document");

    assert_eq!(picture_of(&document).description.as_deref(), Some("a gradient"));
}

#[test]
fn the_bytes_behind_a_picture_can_be_reached() {
    let fixture = fixture();
    let bytes = document_with_picture(&drawing("rId1", 100, 100), &fixture);
    let document = Document::open(&bytes).expect("a readable document");

    let found = document.embedded_part("rId1").expect("the picture part");
    assert_eq!(found, fixture.as_slice(), "the very bytes that went in");
}

#[test]
fn those_bytes_decode_to_the_picture_they_are() {
    let bytes = document_with_picture(&drawing("rId1", 100, 100), &fixture());
    let document = Document::open(&bytes).expect("a readable document");

    let found = document.embedded_part("rId1").expect("the picture part");
    let image = wp_image::decode(found).expect("a decodable picture");
    assert_eq!((image.width, image.height), (32, 32));
}

#[test]
fn a_picture_takes_one_place_in_the_text() {
    // So the caret can stand either side of it, the way it can beside a tab.
    let bytes = document_with_picture(&drawing("rId1", 100, 100), &fixture());
    let document = Document::open(&bytes).expect("a readable document");

    assert_eq!(document.plain_text(), "beforeafter", "a picture reads as nothing");
    assert_eq!(
        document.paragraph_text(0).as_deref(),
        Some("before\u{1}after"),
        "but takes one place in the text the caret moves through"
    );
}

#[test]
fn a_drawing_pointing_at_nothing_is_ignored_rather_than_breaking_the_document() {
    let bytes = document_with_picture("<w:drawing><wp:inline/></w:drawing>", &fixture());
    let document = Document::open(&bytes).expect("a readable document");

    let body = document.body();
    let Block::Paragraph(paragraph) = &body.blocks[0] else { panic!() };
    let pictures = paragraph
        .runs
        .iter()
        .flat_map(|run| run.content.iter())
        .filter(|piece| matches!(piece, RunContent::Picture(_)))
        .count();

    assert_eq!(pictures, 0, "a drawing with no picture in it is not a picture");
    assert_eq!(document.plain_text(), "beforeafter", "and the text is still there");
}

#[test]
fn a_picture_a_document_does_not_carry_reads_as_missing() {
    let bytes = document_with_picture(&drawing("rId99", 100, 100), &fixture());
    let document = Document::open(&bytes).expect("a readable document");

    assert_eq!(picture_of(&document).relationship, "rId99");
    assert_eq!(document.embedded_part("rId99"), None);
}

#[test]
fn a_document_with_a_picture_still_saves_unchanged() {
    // The picture is carried through in its own element rather than rebuilt,
    // so the file has to come back exactly as it went in.
    let original = document_with_picture(&drawing("rId1", 914_400, 914_400), &fixture());
    let document = Document::open(&original).expect("a readable document");

    assert_eq!(document.save().expect("a saved document"), original);
}

#[test]
fn editing_beside_a_picture_leaves_the_picture_alone() {
    let original = document_with_picture(&drawing("rId1", 914_400, 914_400), &fixture());
    let mut document = Document::open(&original).expect("a readable document");

    assert_eq!(document.replace_text("before", "instead"), 1);
    let saved = document.save().expect("a saved document");

    let reopened = Document::open(&saved).expect("a readable document");
    assert_eq!(reopened.plain_text(), "insteadafter");
    assert_eq!(picture_of(&reopened).relationship, "rId1");
    assert_eq!(reopened.embedded_part("rId1").map(<[u8]>::len), Some(fixture().len()));
}

#[test]
#[ignore = "writes a file for looking at by hand"]
fn write_a_document_with_a_picture() {
    let bytes = document_with_picture(&drawing("rId1", 1_828_800, 1_828_800), &fixture());
    std::fs::write("/work/dist/picture.docx", bytes).unwrap();
}
