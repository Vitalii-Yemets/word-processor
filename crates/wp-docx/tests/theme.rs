//! Reading a document that names its colours and fonts after the theme.
//!
//! Word writes `w:themeColor="accent1"` and `w:asciiTheme="minorHAnsi"` rather
//! than a colour and a typeface, so this is the ordinary case for a real
//! document rather than an unusual one.

use wp_docx::model::{Block, Body, Paragraph};
use wp_docx::theme::{FontSlot, Slot};
use wp_docx::{Document, TextPosition};

/// A theme whose values are nothing like the Office ones, so a resolved colour
/// or font cannot come out right by accident.
const THEME: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="Test">
  <a:themeElements>
    <a:clrScheme name="Test">
      <a:dk1><a:sysClr val="windowText" lastClr="101010"/></a:dk1>
      <a:lt1><a:sysClr val="window" lastClr="FAFAFA"/></a:lt1>
      <a:dk2><a:srgbClr val="202020"/></a:dk2>
      <a:lt2><a:srgbClr val="EFEFEF"/></a:lt2>
      <a:accent1><a:srgbClr val="C00000"/></a:accent1>
      <a:accent2><a:srgbClr val="00C000"/></a:accent2>
      <a:accent3><a:srgbClr val="0000C0"/></a:accent3>
      <a:accent4><a:srgbClr val="C0C000"/></a:accent4>
      <a:accent5><a:srgbClr val="00C0C0"/></a:accent5>
      <a:accent6><a:srgbClr val="C000C0"/></a:accent6>
      <a:hlink><a:srgbClr val="123456"/></a:hlink>
      <a:folHlink><a:srgbClr val="654321"/></a:folHlink>
    </a:clrScheme>
    <a:fontScheme name="Test">
      <a:majorFont><a:latin typeface="Georgia"/></a:majorFont>
      <a:minorFont><a:latin typeface="Verdana"/></a:minorFont>
    </a:fontScheme>
  </a:themeElements>
</a:theme>"#;

const THEME_RELATIONSHIP: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme";
const THEME_CONTENT_TYPE: &str = "application/vnd.openxmlformats-officedocument.theme+xml";

/// A document whose only run is the given `w:rPr`, with a theme part beside it.
///
/// Built by hand because nothing in this program writes a run that names the
/// theme — Word does, and reading what Word writes is the point.
fn document_with(run_properties: &str, with_theme: bool) -> Document {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("text")));
    let bytes = Document::create(&body).expect("a document").save().expect("saving");

    let mut package = wp_opc::Package::open(&bytes).expect("a package");
    let main = "word/document.xml";
    let source = package.xml_part(main).expect("the document").expect("readable");

    // The one run gets the properties asked for.
    let replaced = source.replace("<w:r>", &format!("<w:r>{run_properties}"));
    if !run_properties.is_empty() {
        assert_ne!(replaced, source, "the document should have had a run to change");
    }
    package.set_part(main, replaced.into_bytes());

    if with_theme {
        package.add_part("word/theme/theme1.xml", THEME_CONTENT_TYPE, THEME.as_bytes().to_vec());
        let mut relationships = package.relationships(main).expect("the relationships");
        relationships.add(THEME_RELATIONSHIP, "theme/theme1.xml", wp_opc::TargetMode::Internal);
        package.set_relationships(&relationships).expect("writing the relationships");
    }

    let bytes = package.save().expect("saving the package");
    Document::open(&bytes).expect("reopening")
}

fn at_start(document: &mut Document) {
    document.set_caret(TextPosition::new(0, 0));
}

#[test]
fn a_document_with_no_theme_part_gets_the_office_one() {
    let document = document_with("", false);
    let theme = document.theme();
    assert_eq!(theme.minor_font, "Calibri");
    assert_eq!(theme.color(Slot::Accent1), "4472C4");
}

#[test]
fn a_theme_part_is_found_and_read() {
    let document = document_with("", true);
    assert_eq!(document.theme_part().as_deref(), Some("word/theme/theme1.xml"));

    let theme = document.theme();
    assert_eq!(theme.name, "Test");
    assert_eq!(theme.color(Slot::Accent1), "C00000");
    assert_eq!(theme.font(FontSlot::Major), "Georgia");
    assert_eq!(theme.font(FontSlot::Minor), "Verdana");
}

#[test]
fn a_run_that_names_an_accent_comes_out_that_colour() {
    let mut document = document_with(r#"<w:rPr><w:color w:themeColor="accent1"/></w:rPr>"#, true);
    at_start(&mut document);
    assert_eq!(document.color_here().as_deref(), Some("C00000"));
}

#[test]
fn a_run_that_names_the_heading_font_comes_out_in_it() {
    let mut document =
        document_with(r#"<w:rPr><w:rFonts w:asciiTheme="majorHAnsi"/></w:rPr>"#, true);
    at_start(&mut document);
    assert_eq!(document.font_here().as_deref(), Some("Georgia"));
}

#[test]
fn a_run_that_names_the_body_font_comes_out_in_it() {
    let mut document =
        document_with(r#"<w:rPr><w:rFonts w:asciiTheme="minorHAnsi"/></w:rPr>"#, true);
    at_start(&mut document);
    assert_eq!(document.font_here().as_deref(), Some("Verdana"));
}

#[test]
fn the_text_and_background_names_mean_the_same_slots_as_dark_and_light() {
    let mut document = document_with(r#"<w:rPr><w:color w:themeColor="text1"/></w:rPr>"#, true);
    at_start(&mut document);
    assert_eq!(document.color_here().as_deref(), Some("101010"));
}

#[test]
fn a_tint_lightens_the_named_colour() {
    let mut document =
        document_with(r#"<w:rPr><w:color w:themeColor="accent1" w:themeTint="00"/></w:rPr>"#, true);
    at_start(&mut document);
    assert_eq!(document.color_here().as_deref(), Some("FFFFFF"), "no tint at all is white");
}

#[test]
fn a_shade_darkens_it() {
    let mut document = document_with(
        r#"<w:rPr><w:color w:themeColor="accent1" w:themeShade="00"/></w:rPr>"#,
        true,
    );
    at_start(&mut document);
    assert_eq!(document.color_here().as_deref(), Some("000000"), "no shade at all is black");
}

#[test]
fn a_colour_written_out_beside_a_name_is_the_one_that_counts() {
    let mut document =
        document_with(r#"<w:rPr><w:color w:val="00FF00" w:themeColor="accent1"/></w:rPr>"#, true);
    at_start(&mut document);
    assert_eq!(document.color_here().as_deref(), Some("00FF00"));
}

#[test]
fn a_name_with_no_theme_part_resolves_against_the_office_theme() {
    let mut document = document_with(r#"<w:rPr><w:color w:themeColor="accent1"/></w:rPr>"#, false);
    at_start(&mut document);
    assert_eq!(document.color_here().as_deref(), Some("4472C4"));
}

#[test]
fn choosing_a_colour_takes_the_name_off_so_the_file_says_one_thing() {
    let mut document = document_with(r#"<w:rPr><w:color w:themeColor="accent1"/></w:rPr>"#, true);
    document.move_caret(TextPosition::new(0, 0), false);
    document.move_caret(TextPosition::new(0, 4), true);
    assert!(document.set_color(Some("00FF00")));

    let bytes = document.save().expect("saving");
    let reopened = Document::open(&bytes).expect("reopening");
    let part =
        reopened.package().xml_part("word/document.xml").expect("the document").expect("readable");
    assert!(!part.contains("themeColor"), "the name should have gone with the colour: {part}");
}
