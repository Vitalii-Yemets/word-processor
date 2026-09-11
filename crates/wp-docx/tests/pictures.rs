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

// --- A picture that floats ---------------------------------------------------

/// The same drawing, anchored the way Word writes a floating one.
fn floating_drawing(relationship: &str, width: i64, height: i64) -> String {
    format!(
        r#"<w:drawing><wp:anchor distT="0" distB="0" distL="114300" distR="114300"
 simplePos="0" relativeHeight="251658250" behindDoc="1" locked="0" layoutInCell="1"
 allowOverlap="1">
<wp:simplePos x="0" y="0"/>
<wp:positionH relativeFrom="column"><wp:posOffset>457200</wp:posOffset></wp:positionH>
<wp:positionV relativeFrom="paragraph"><wp:posOffset>228600</wp:posOffset></wp:positionV>
<wp:extent cx="{width}" cy="{height}"/>
<wp:wrapSquare wrapText="bothSides"/>
<wp:docPr id="1" name="Picture 1" descr="a gradient"/>
<a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture">
<pic:pic><pic:blipFill><a:blip r:embed="{relationship}"/></pic:blipFill></pic:pic>
</a:graphicData></a:graphic>
</wp:anchor></w:drawing>"#
    )
}

/// A document whose one picture floats.
fn floating_document() -> Document {
    let bytes = document_with_picture(&floating_drawing("rId1", 914_400, 914_400), &fixture());
    Document::open(&bytes).expect("opening")
}

#[test]
fn a_floating_picture_is_read_as_floating() {
    let picture = picture_of(&floating_document());
    let anchor = picture.anchor.expect("an anchor");
    assert_eq!(anchor.wrap, wp_docx::anchor::Wrap::Square);
    assert!(anchor.behind_text);
    assert_eq!(anchor.depth, 251_658_250);
}

#[test]
fn a_picture_in_the_line_has_no_anchor() {
    let bytes = document_with_picture(&drawing("rId1", 914_400, 914_400), &fixture());
    let document = Document::open(&bytes).expect("opening");
    assert!(picture_of(&document).anchor.is_none());
}

#[test]
fn a_picture_can_be_made_to_float_and_put_back() {
    let bytes = document_with_picture(&drawing("rId1", 914_400, 914_400), &fixture());
    let mut document = Document::open(&bytes).expect("opening");
    // The caret beside the picture: "before" is six characters, and the
    // drawing is the seventh.
    document.set_caret(wp_docx::TextPosition::new(0, 7));
    assert!(document.drawing_here(), "the caret is not beside the picture");

    let anchor = wp_docx::anchor::Anchor {
        wrap: wp_docx::anchor::Wrap::Tight,
        ..wp_docx::anchor::Anchor::default()
    };
    assert!(document.set_anchor_here(Some(&anchor)));
    assert_eq!(picture_of(&document).anchor.map(|found| found.wrap), Some(anchor.wrap));

    assert!(document.set_anchor_here(None));
    assert!(picture_of(&document).anchor.is_none(), "the picture is still floating");
}

#[test]
fn floating_survives_being_saved_and_opened_again() {
    let bytes = document_with_picture(&drawing("rId1", 914_400, 914_400), &fixture());
    let mut document = Document::open(&bytes).expect("opening");
    document.set_caret(wp_docx::TextPosition::new(0, 7));

    let anchor = wp_docx::anchor::Anchor {
        wrap: wp_docx::anchor::Wrap::TopAndBottom,
        behind_text: true,
        depth: wp_docx::anchor::USUAL_DEPTH + 3,
        ..wp_docx::anchor::Anchor::default()
    };
    document.set_anchor_here(Some(&anchor));

    let saved = document.save().expect("saving");
    let reopened = Document::open(&saved).expect("reopening");
    let found = picture_of(&reopened).anchor.expect("an anchor");
    assert_eq!(found.wrap, wp_docx::anchor::Wrap::TopAndBottom);
    assert!(found.behind_text);
    assert_eq!(found.depth, wp_docx::anchor::USUAL_DEPTH + 3);
}

#[test]
fn making_a_picture_float_keeps_everything_under_the_drawing() {
    // The graphic is never touched: that is the whole reason the anchor is
    // changed where it stands rather than the element being rebuilt.
    let mut document = floating_document();
    document.set_caret(wp_docx::TextPosition::new(0, 7));
    document.set_anchor_here(None);

    let saved = document.save().expect("saving");
    let text = String::from_utf8_lossy(&saved).to_string();
    let _ = text;
    let reopened = Document::open(&saved).expect("reopening");
    let picture = picture_of(&reopened);
    assert_eq!(picture.relationship, "rId1", "the picture lost what it points at");
    assert_eq!(picture.width_emu, 914_400, "the picture lost its size");
    assert_eq!(picture.description.as_deref(), Some("a gradient"));
}

#[test]
fn a_floating_picture_is_in_the_same_pile_as_a_shape() {
    // A picture laid over a shape is over it or under it; a count that saw
    // only one kind would put a new drawing among the others.
    let document = floating_document();
    assert_eq!(document.drawing_depths(), vec![251_658_250]);
    assert_eq!(document.next_drawing_depth(), 251_658_251);
}
