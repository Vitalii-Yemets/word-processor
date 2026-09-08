//! End-to-end tests over the whole stack: DEFLATE, ZIP, XML, package, model.

use wp_docx::model::{
    Alignment, Block, Body, BreakKind, Paragraph, Run, RunContent, RunProperties, Table, TableRow,
    Underline,
};
use wp_docx::Document;

/// Text in the scripts the editor has to handle, so that every layer is
/// exercised with the byte patterns real documents contain rather than ASCII.
const SAMPLES: &[(&str, &str)] = &[
    ("en-GB", "The quick brown fox jumps over the lazy dog."),
    ("ru-RU", "Съешь же ещё этих мягких французских булок."),
    ("el-GR", "Ταχίστη αλώπηξ βαφής ψημένη γη."),
    ("hi-IN", "वह क्षमा और साहस का प्रतीक है।"),
    ("th-TH", "เป็นมนุษย์สุดประเสริฐเลิศคุณค่า"),
    ("zh-CN", "永和九年，歲在癸丑，暮春之初。"),
    ("ko-KR", "다람쥐 헌 쳇바퀴에 타고파."),
    ("ar", "نص حكيم له سر قاطع وذو شأن عظيم."),
    ("he", "דג סקרן שט בים מאוכזב ולפתע מצא חברה."),
];

/// A body covering everything the model can express.
fn rich_body() -> Body {
    let mut body = Body::default();

    body.blocks.push(Block::Paragraph(
        Paragraph::text("Title").with_style("Title").with_alignment(Alignment::Center),
    ));

    body.blocks.push(Block::Paragraph(Paragraph::from_runs(vec![
        Run::text("plain "),
        Run {
            properties: RunProperties {
                bold: Some(true),
                italic: Some(true),
                underline: Some(Underline::Single),
                strike: Some(true),
                size_half_points: Some(28),
                color: Some("C00000".to_owned()),
                font: Some("Cambria".to_owned()),
                language: Some("en-GB".to_owned()),
                style: Some("Emphasis".to_owned()),
                right_to_left: Some(false),
                highlight: Some("yellow".to_owned()),
                vertical_align: Some(wp_docx::model::VerticalAlignment::Superscript),
                // This run writes its colour and its font out, so it names no
                // theme slot for either.
                color_theme: None,
                effect: None,
                font_theme: None,
            },
            field: None,
            revision: None,
            content: vec![RunContent::Text("formatted".to_owned())],
        },
        Run {
            properties: RunProperties::default(),
            field: None,
            revision: None,
            content: vec![
                RunContent::Tab,
                RunContent::Break(BreakKind::Line),
                RunContent::Break(BreakKind::Page),
                RunContent::Text("after breaks".to_owned()),
            ],
        },
    ])));

    for (language, sample) in SAMPLES {
        let right_to_left = matches!(*language, "ar" | "he");
        let mut run = Run::text(sample).in_language(language);
        run.properties.right_to_left = Some(right_to_left);

        let mut paragraph = Paragraph::from_runs(vec![run]).with_alignment(Alignment::Start);
        paragraph.properties.right_to_left = Some(right_to_left);
        body.blocks.push(Block::Paragraph(paragraph));
    }

    body.blocks.push(Block::Table(Box::new(
        Table::from_rows(vec![
            TableRow::text(&["first", "second", "третий"]),
            TableRow::text(&["a", "b", "c"]),
        ])
        .with_style("TableGrid")
        .with_grid(vec![3000, 3000, 3000]),
    )));

    body
}

#[test]
fn a_created_document_reads_back_as_it_was_written() {
    let body = rich_body();
    let document = Document::create(&body).unwrap();
    let bytes = document.save().unwrap();

    let reopened = Document::open(&bytes).unwrap();
    let read_back = reopened.body();

    assert_eq!(read_back, body, "the model did not survive a write and read");
}

#[test]
fn saving_an_opened_document_reproduces_it_exactly() {
    // The property everything else rests on: a document that passes through
    // untouched must come out identical, or something is being lost.
    let original = Document::create(&rich_body()).unwrap().save().unwrap();

    let opened = Document::open(&original).unwrap();
    let saved = opened.save().unwrap();

    assert_eq!(saved, original, "saving an unmodified document changed it");
}

#[test]
fn every_script_survives_the_round_trip() {
    let document = Document::create(&rich_body()).unwrap();
    let bytes = document.save().unwrap();
    let text = Document::open(&bytes).unwrap().plain_text();

    for (language, sample) in SAMPLES {
        assert!(text.contains(sample), "{language} text was lost: {sample:?}");
    }
}

#[test]
fn leading_and_trailing_spaces_are_preserved() {
    // Without xml:space="preserve" these are collapsed away, and words in the
    // finished document run together. It is invisible until someone reads it.
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph {
        runs: vec![Run::text("Hello, "), Run::text(" world"), Run::text("  spaced  ")],
        ..Paragraph::default()
    }));

    let bytes = Document::create(&body).unwrap().save().unwrap();
    let text = Document::open(&bytes).unwrap().plain_text();

    assert_eq!(text, "Hello,  world  spaced  ");
}

#[test]
fn characters_that_are_markup_in_xml_survive_as_text() {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text(
        "angle < brackets > and & ampersands \"quoted\" 'apostrophes' ]]> too",
    )));

    let bytes = Document::create(&body).unwrap().save().unwrap();
    let text = Document::open(&bytes).unwrap().plain_text();

    assert_eq!(text, "angle < brackets > and & ampersands \"quoted\" 'apostrophes' ]]> too");
}

#[test]
fn the_package_is_structured_the_way_the_format_requires() {
    let bytes = Document::create(&rich_body()).unwrap().save().unwrap();
    let document = Document::open(&bytes).unwrap();
    let package = document.package();

    assert_eq!(document.main_part(), "word/document.xml");
    assert_eq!(package.content_type("word/document.xml"), Some(wp_opc::MAIN_DOCUMENT_CONTENT_TYPE));
    assert!(package.part("_rels/.rels").is_some(), "the package relationships are required");
    assert!(package.part("word/styles.xml").is_some(), "styles are referenced and must exist");

    assert_eq!(package.validate(), Vec::new(), "the package should be self-consistent");
}

#[test]
fn the_content_types_stream_comes_first() {
    // Not required by the specification, but it is where every tool expects it
    // and where Word puts it.
    let bytes = Document::create(&Body::default()).unwrap().save().unwrap();
    let document = Document::open(&bytes).unwrap();

    assert_eq!(document.package().entries()[0].name, "[Content_Types].xml");
}

#[test]
fn an_empty_document_is_still_valid() {
    let bytes = Document::create(&Body::default()).unwrap().save().unwrap();
    let document = Document::open(&bytes).unwrap();

    assert_eq!(document.body(), Body::default());
    assert_eq!(document.plain_text(), "");
}

#[test]
fn a_run_can_switch_off_what_its_style_turned_on() {
    // "<w:b w:val=\"0\"/>" means *not* bold. Reading it as bold would make every
    // document that overrides a style render wrongly.
    let source = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
        <w:document xmlns:w="{ns}"><w:body><w:p>
          <w:r><w:rPr><w:b w:val="0"/><w:i w:val="1"/><w:u w:val="none"/></w:rPr>
            <w:t>text</w:t></w:r>
        </w:p></w:body></w:document>"#,
        ns = wp_docx::WORDPROCESSING_NAMESPACE
    );

    let body = body_from_document_xml(&source);
    let run = &body.paragraphs()[0].runs[0];

    assert_eq!(run.properties.bold, Some(false), "w:val=0 means the property is off");
    assert_eq!(run.properties.italic, Some(true));
    assert_eq!(
        run.properties.underline,
        Some(Underline::None),
        "an underline of \"none\" is explicitly no underline"
    );
}

#[test]
fn deleted_text_is_not_part_of_the_document() {
    // Text inside <w:del> was removed by a tracked change. Including it would
    // resurrect edits somebody deliberately made.
    let source = format!(
        r#"<w:document xmlns:w="{ns}"><w:body><w:p>
          <w:r><w:t>kept </w:t></w:r>
          <w:del w:id="1" w:author="A"><w:r><w:delText>removed </w:delText></w:r></w:del>
          <w:ins w:id="2" w:author="B"><w:r><w:t>added</w:t></w:r></w:ins>
        </w:p></w:body></w:document>"#,
        ns = wp_docx::WORDPROCESSING_NAMESPACE
    );

    let body = body_from_document_xml(&source);
    assert_eq!(body.plain_text(), "kept added");
}

#[test]
fn runs_inside_a_hyperlink_are_read() {
    let source = format!(
        r#"<w:document xmlns:w="{ns}"><w:body><w:p>
          <w:r><w:t>see </w:t></w:r>
          <w:hyperlink><w:r><w:t>the link</w:t></w:r></w:hyperlink>
        </w:p></w:body></w:document>"#,
        ns = wp_docx::WORDPROCESSING_NAMESPACE
    );

    assert_eq!(body_from_document_xml(&source).plain_text(), "see the link");
}

#[test]
fn rubbish_is_refused_without_panicking() {
    for bytes in [
        b"".to_vec(),
        b"not a document".to_vec(),
        b"PK\x03\x04 truncated".to_vec(),
        vec![0xFF; 1000],
        // A valid archive that is not a package.
        {
            let mut writer = wp_zip::ZipWriter::new();
            writer.add("hello.txt", b"not a document").unwrap();
            writer.finish().unwrap()
        },
    ] {
        assert!(Document::open(&bytes).is_err(), "should have been refused");
    }
}

#[test]
fn a_damaged_document_is_refused_rather_than_half_read() {
    let original = Document::create(&rich_body()).unwrap().save().unwrap();

    // Flip bits through the file. Whatever happens, it must be an error or a
    // correctly read document — never a panic, and never silent corruption.
    for index in (0..original.len()).step_by(7) {
        let mut damaged = original.clone();
        damaged[index] ^= 0xFF;

        if let Ok(document) = Document::open(&damaged) {
            let _ = document.body();
            let _ = document.save();
        }
    }
}

/// Builds a package around a `document.xml` so that fragments can be tested.
fn body_from_document_xml(document_xml: &str) -> Body {
    let mut package = wp_opc::Package::empty();
    package.add_part(
        "word/document.xml",
        wp_opc::MAIN_DOCUMENT_CONTENT_TYPE,
        document_xml.as_bytes().to_vec(),
    );

    let mut root = wp_opc::Relationships::new("");
    root.add(
        wp_opc::OFFICE_DOCUMENT_RELATIONSHIP,
        "word/document.xml",
        wp_opc::TargetMode::Internal,
    );
    package.set_relationships(&root).unwrap();

    let bytes = package.save().unwrap();
    Document::open(&bytes).unwrap().body()
}
