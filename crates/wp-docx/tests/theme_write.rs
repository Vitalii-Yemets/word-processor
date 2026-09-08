//! Writing a theme, and what happens to the document that named it.

use wp_docx::gallery::{COLOR_SCHEMES, FONT_PAIRS};
use wp_docx::model::{Block, Body, Paragraph};
use wp_docx::theme::{FontSlot, Slot, Theme};
use wp_docx::{Document, TextPosition};

/// A document whose one run names the theme rather than a colour and a font.
///
/// Built by hand because nothing here writes such a run — Word does, and a
/// theme is only worth changing for a document that names it.
fn document_naming_the_theme() -> Document {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("text")));
    let bytes = Document::create(&body).expect("a document").save().expect("saving");

    let mut package = wp_opc::Package::open(&bytes).expect("a package");
    let main = "word/document.xml";
    let source = package.xml_part(main).expect("the document").expect("readable");
    let named = source.replace(
        "<w:r>",
        r#"<w:r><w:rPr><w:color w:themeColor="accent1"/><w:rFonts w:asciiTheme="minorHAnsi"/></w:rPr>"#,
    );
    package.set_part(main, named.into_bytes());

    let bytes = package.save().expect("saving the package");
    Document::open(&bytes).expect("reopening")
}

fn round_trip(document: &Document) -> Document {
    let bytes = document.save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

fn scheme(name: &str) -> &'static wp_docx::gallery::ColorScheme {
    COLOR_SCHEMES.iter().find(|scheme| scheme.name == name).expect("a scheme by that name")
}

#[test]
fn a_theme_can_be_written_and_reads_back_after_saving() {
    let mut document = document_naming_the_theme();
    let wanted = scheme("Grayscale").theme();
    assert!(document.set_theme(&wanted).expect("writing"));
    assert_eq!(round_trip(&document).theme(), wanted);
}

#[test]
fn writing_one_makes_the_part_and_points_at_it() {
    let mut document = document_naming_the_theme();
    assert_eq!(document.theme_part(), None, "a fresh document has no theme part");

    document.set_theme(&scheme("Blue").theme()).expect("writing");
    let reopened = round_trip(&document);
    assert_eq!(reopened.theme_part().as_deref(), Some("word/theme/theme1.xml"));
}

#[test]
fn the_text_named_after_the_theme_changes_with_it() {
    let mut document = document_naming_the_theme();
    document.set_caret(TextPosition::new(0, 0));
    assert_eq!(document.color_here().as_deref(), Some("4472C4"), "the Office accent to begin");

    document.set_theme(&scheme("Grayscale").theme()).expect("writing");
    document.set_caret(TextPosition::new(0, 0));
    assert_eq!(
        document.color_here().as_deref(),
        Some(scheme("Grayscale").color(Slot::Accent1)),
        "the colour should follow the theme without the document being reopened"
    );
}

#[test]
fn the_font_named_after_the_theme_changes_with_it() {
    let mut document = document_naming_the_theme();
    document.set_caret(TextPosition::new(0, 0));
    assert_eq!(document.font_here().as_deref(), Some("Calibri"));

    let wanted = Theme::default().with_fonts(&FONT_PAIRS[3]);
    document.set_theme(&wanted).expect("writing");
    document.set_caret(TextPosition::new(0, 0));
    assert_eq!(document.font_here().as_deref(), Some("Georgia"));
}

#[test]
fn not_one_character_of_the_text_changes() {
    let mut document = document_naming_the_theme();
    document.set_theme(&scheme("Red").theme()).expect("writing");
    assert_eq!(round_trip(&document).plain_text(), "text");
}

#[test]
fn writing_the_same_theme_twice_changes_nothing() {
    let mut document = document_naming_the_theme();
    let wanted = scheme("Green").theme();
    assert!(document.set_theme(&wanted).expect("writing"));
    assert!(!document.set_theme(&wanted).expect("writing"));
}

#[test]
fn the_part_carries_a_format_scheme_so_word_does_not_repair_it() {
    let mut document = document_naming_the_theme();
    document.set_theme(&scheme("Blue").theme()).expect("writing");

    let reopened = round_trip(&document);
    let part = reopened
        .package()
        .xml_part("word/theme/theme1.xml")
        .expect("the theme part")
        .expect("readable");
    for required in ["clrScheme", "fontScheme", "fmtScheme"] {
        assert!(part.contains(required), "the theme has no {required}: {part}");
    }
}

#[test]
fn changing_the_colours_twice_leaves_one_scheme_and_not_two() {
    let mut document = document_naming_the_theme();
    document.set_theme(&scheme("Blue").theme()).expect("writing");
    document.set_theme(&scheme("Red").theme()).expect("writing");

    let reopened = round_trip(&document);
    let part = reopened
        .package()
        .xml_part("word/theme/theme1.xml")
        .expect("the theme part")
        .expect("readable");
    assert_eq!(part.matches("<a:clrScheme").count(), 1);
    assert_eq!(part.matches("<a:fontScheme").count(), 1);
    assert_eq!(reopened.theme().color(Slot::Accent1), scheme("Red").color(Slot::Accent1));
}

#[test]
fn changing_only_the_fonts_leaves_the_colours_where_they_were() {
    let mut document = document_naming_the_theme();
    document.set_theme(&scheme("Green").theme()).expect("writing");

    let with_fonts = document.theme().with_fonts(&FONT_PAIRS[2]);
    document.set_theme(&with_fonts).expect("writing");

    let theme = round_trip(&document).theme();
    assert_eq!(theme.color(Slot::Accent1), scheme("Green").color(Slot::Accent1));
    assert_eq!(theme.font(FontSlot::Minor), "Arial");
}

#[test]
fn a_theme_the_document_already_had_keeps_the_rest_of_its_part() {
    let mut document = document_naming_the_theme();
    document.set_theme(&scheme("Blue").theme()).expect("writing");

    // Something this program does not write, put into the part by hand — a
    // theme is not this program's to rewrite wholesale.
    let bytes = document.save().expect("saving");
    let mut package = wp_opc::Package::open(&bytes).expect("a package");
    let part = "word/theme/theme1.xml";
    let source = package.xml_part(part).expect("the theme").expect("readable");
    let marked = source.replace("</a:themeElements>", "<a:extLst/></a:themeElements>");
    package.set_part(part, marked.into_bytes());
    let bytes = package.save().expect("saving the package");

    let mut document = Document::open(&bytes).expect("reopening");
    document.set_theme(&scheme("Red").theme()).expect("writing");

    let reopened = round_trip(&document);
    let part = reopened.package().xml_part(part).expect("the theme").expect("readable");
    assert!(part.contains("extLst"), "what was already there was thrown away: {part}");
}
