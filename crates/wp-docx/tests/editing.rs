//! Tests for editing an existing document.
//!
//! The property under test throughout: an edit changes what it was asked to
//! change and nothing else. Everything the program does not understand has to
//! come back byte for byte, because a real document is full of it.

use wp_docx::model::{Alignment, Block, Body, Paragraph, Run, RunContent, RunProperties};
use wp_docx::Document;

/// Text in the scripts the editor has to handle.
const SAMPLES: &[&str] = &[
    "The quick brown fox jumps over the lazy dog.",
    "Съешь же ещё этих мягких французских булок.",
    "Ταχίστη αλώπηξ βαφής ψημένη γη.",
    "वह क्षमा और साहस का प्रतीक है।",
    "เป็นมนุษย์สุดประเสริฐเลิศคุณค่า",
    "永和九年，歲在癸丑，暮春之初。",
    "다람쥐 헌 쳇바퀴에 타고파.",
    "نص حكيم له سر قاطع وذو شأن عظيم.",
    "דג סקרן שט בים מאוכזב ולפתע מצא חברה.",
];

/// A document with a few paragraphs, one per script.
fn multilingual_document() -> Vec<u8> {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("Title").with_style("Title")));
    for sample in SAMPLES {
        body.blocks.push(Block::Paragraph(Paragraph::text(sample)));
    }
    Document::create(&body).unwrap().save().unwrap()
}

#[test]
fn replacing_text_changes_only_the_main_part() {
    // The whole reason for holding an element tree: an edit must not disturb
    // anything else in the package.
    let original = multilingual_document();
    let before = wp_opc::Package::open(&original).unwrap();

    let mut document = Document::open(&original).unwrap();
    assert_eq!(document.replace_text("Title", "Heading"), 1);
    let saved = document.save().unwrap();
    let after = wp_opc::Package::open(&saved).unwrap();

    for entry in before.entries() {
        let updated = after
            .part(&entry.name)
            .unwrap_or_else(|| panic!("{} was lost", entry.name));
        if entry.name == "word/document.xml" {
            assert_ne!(updated, entry.data.as_slice(), "the main part should have changed");
        } else {
            assert_eq!(updated, entry.data.as_slice(), "{} should not have changed", entry.name);
        }
    }
    assert_eq!(after.entries().len(), before.entries().len(), "no part should appear or vanish");
}

#[test]
fn replacing_text_works_across_run_boundaries() {
    // Word splits a paragraph text between runs wherever formatting changes, and
    // often where it does not. A search looking at one run at a time would
    // simply not find a word stored in two pieces.
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph {
        runs: vec![
            Run::text("Hello, wo"),
            Run {
                properties: RunProperties { bold: Some(true), ..RunProperties::default() },
                content: vec![RunContent::Text("rl".to_owned())],
            },
            Run::text("d!"),
        ],
        ..Paragraph::default()
    }));

    let bytes = Document::create(&body).unwrap().save().unwrap();
    let mut document = Document::open(&bytes).unwrap();

    assert_eq!(document.plain_text(), "Hello, world!");
    assert_eq!(document.replace_text("world", "everyone"), 1);
    assert_eq!(document.plain_text(), "Hello, everyone!");

    let saved = document.save().unwrap();
    assert_eq!(Document::open(&saved).unwrap().plain_text(), "Hello, everyone!");
}

#[test]
fn a_match_spanning_runs_leaves_the_untouched_run_alone() {
    // The replacement goes into the run where the match starts. The bold run in
    // the middle held only matched characters, so it empties, but it and its
    // formatting survive.
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph {
        runs: vec![
            Run::text("before "),
            Run::text("mat"),
            Run {
                properties: RunProperties { bold: Some(true), ..RunProperties::default() },
                content: vec![RunContent::Text("ch".to_owned())],
            },
            Run::text(" after"),
        ],
        ..Paragraph::default()
    }));

    let bytes = Document::create(&body).unwrap().save().unwrap();
    let mut document = Document::open(&bytes).unwrap();
    assert_eq!(document.replace_text("match", "found"), 1);

    assert_eq!(document.plain_text(), "before found after");

    let saved = document.save().unwrap();
    let reopened = Document::open(&saved).unwrap();
    let Block::Paragraph(paragraph) = &reopened.body().blocks[0] else {
        panic!("expected a paragraph")
    };
    assert_eq!(paragraph.runs.len(), 4, "no run should have been removed");
    assert_eq!(
        paragraph.runs[2].properties.bold,
        Some(true),
        "the bold run lost its formatting"
    );
}

#[test]
fn every_occurrence_is_replaced() {
    let mut body = Body::default();
    for _ in 0..3 {
        body.blocks.push(Block::Paragraph(Paragraph::text("one two one two one")));
    }

    let bytes = Document::create(&body).unwrap().save().unwrap();
    let mut document = Document::open(&bytes).unwrap();

    assert_eq!(document.replace_text("one", "1"), 9);
    assert_eq!(document.plain_text(), "1 two 1 two 1\n1 two 1 two 1\n1 two 1 two 1");
}

#[test]
fn replacing_works_in_every_script() {
    let original = multilingual_document();

    for sample in SAMPLES {
        let mut document = Document::open(&original).unwrap();
        assert_eq!(document.replace_text(sample, "REPLACED"), 1, "sample {sample:?}");

        let saved = document.save().unwrap();
        let text = Document::open(&saved).unwrap().plain_text();
        assert!(!text.contains(sample), "the original text is still there: {sample:?}");
        assert!(text.contains("REPLACED"), "the replacement is missing for {sample:?}");
    }
}

#[test]
fn an_edit_carries_unknown_markup_through_untouched() {
    // Stands in for what a real document holds: a content control, a chart, a
    // field, tracked changes from a colleague. None of it is understood here,
    // and all of it must come back.
    let source = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<w:document xmlns:w=\"{ns}\"><w:body>\
<w:p><w:r><w:t>replace me</w:t></w:r></w:p>\
<w:sdt><w:sdtPr><w:alias w:val=\"a content control\"/></w:sdtPr>\
<w:sdtContent><w:p><w:r><w:t>inside</w:t></w:r></w:p></w:sdtContent></w:sdt>\
<w:customUnknown attribute=\"kept\"><!-- a note --><w:deep>data</w:deep></w:customUnknown>\
<w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/></w:sectPr>\
</w:body></w:document>",
        ns = wp_docx::WORDPROCESSING_NAMESPACE
    );

    let bytes = package_around(&source);
    let mut document = Document::open(&bytes).unwrap();
    assert_eq!(document.replace_text("replace me", "replaced"), 1);

    let saved = document.save().unwrap();
    let package = wp_opc::Package::open(&saved).unwrap();
    let xml = String::from_utf8(package.part("word/document.xml").unwrap().to_vec()).unwrap();

    assert!(xml.contains("replaced"), "the edit did not apply: {xml}");
    assert!(
        xml.contains(
            "<w:customUnknown attribute=\"kept\"><!-- a note --><w:deep>data</w:deep></w:customUnknown>"
        ),
        "unknown markup was altered: {xml}"
    );
    assert!(
        xml.contains("<w:alias w:val=\"a content control\"/>"),
        "the content control was altered: {xml}"
    );
    assert!(xml.contains("<w:sectPr>"), "the section properties were lost: {xml}");
}

#[test]
fn text_inside_a_content_control_is_still_found() {
    let source = format!(
        "<w:document xmlns:w=\"{ns}\"><w:body>\
<w:sdt><w:sdtContent><w:p><w:r><w:t>inside a control</w:t></w:r></w:p></w:sdtContent></w:sdt>\
</w:body></w:document>",
        ns = wp_docx::WORDPROCESSING_NAMESPACE
    );

    let mut document = Document::open(&package_around(&source)).unwrap();
    assert_eq!(document.replace_text("inside", "within"), 1);
    assert_eq!(document.plain_text(), "within a control");
}

#[test]
fn deleted_text_is_never_searched_or_rewritten() {
    // Text inside w:del was removed by a tracked change. Replacing into it would
    // rewrite an edit somebody deliberately made.
    let source = format!(
        "<w:document xmlns:w=\"{ns}\"><w:body><w:p>\
<w:r><w:t>keep target</w:t></w:r>\
<w:del w:id=\"1\" w:author=\"A\"><w:r><w:delText>target</w:delText></w:r></w:del>\
</w:p></w:body></w:document>",
        ns = wp_docx::WORDPROCESSING_NAMESPACE
    );

    let mut document = Document::open(&package_around(&source)).unwrap();
    assert_eq!(document.replace_text("target", "changed"), 1, "only the live text should match");

    let saved = document.save().unwrap();
    let package = wp_opc::Package::open(&saved).unwrap();
    let xml = String::from_utf8(package.part("word/document.xml").unwrap().to_vec()).unwrap();

    assert!(xml.contains("keep changed"), "the live text was not changed: {xml}");
    assert!(xml.contains("<w:delText>target</w:delText>"), "the deleted text was rewritten: {xml}");
}

#[test]
fn appending_a_paragraph_keeps_the_section_properties_last() {
    // w:sectPr must remain the final child of w:body; a document with anything
    // after it is rejected.
    let bytes = Document::create(&Body::default()).unwrap().save().unwrap();
    let mut document = Document::open(&bytes).unwrap();

    assert!(document.append_paragraph(&Paragraph::text("added at the end")));
    let saved = document.save().unwrap();

    let package = wp_opc::Package::open(&saved).unwrap();
    let xml = String::from_utf8(package.part("word/document.xml").unwrap().to_vec()).unwrap();

    let paragraph_at = xml.find("added at the end").expect("the paragraph should be there");
    let section_at = xml.find("<w:sectPr>").expect("the section properties should be there");
    assert!(paragraph_at < section_at, "the paragraph must come before w:sectPr");

    assert_eq!(Document::open(&saved).unwrap().plain_text(), "added at the end");
}

#[test]
fn paragraph_style_and_alignment_can_be_changed() {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("plain")));

    let bytes = Document::create(&body).unwrap().save().unwrap();
    let mut document = Document::open(&bytes).unwrap();

    assert!(document.set_paragraph_style(0, Some("Heading1")));
    assert!(document.set_paragraph_alignment(0, Alignment::Center));

    let saved = document.save().unwrap();
    let reopened = Document::open(&saved).unwrap();
    let Block::Paragraph(paragraph) = &reopened.body().blocks[0] else {
        panic!("expected a paragraph")
    };

    assert_eq!(paragraph.style(), Some("Heading1"));
    assert_eq!(paragraph.properties.alignment, Some(Alignment::Center));
    assert_eq!(paragraph.plain_text(), "plain", "the text should be untouched");
}

#[test]
fn an_untouched_document_is_not_re_serialized() {
    let original = multilingual_document();

    let document = Document::open(&original).unwrap();
    assert!(!document.is_modified());
    assert_eq!(document.save().unwrap(), original);

    // A search that finds nothing is not an edit.
    let mut document = Document::open(&original).unwrap();
    assert_eq!(document.replace_text("nothing here matches this", "x"), 0);
    assert!(!document.is_modified());
    assert_eq!(document.save().unwrap(), original);
}

#[test]
fn an_empty_search_string_matches_nothing() {
    // Searching for the empty string would otherwise match at every position and
    // never terminate.
    let mut document = Document::open(&multilingual_document()).unwrap();

    assert_eq!(document.replace_text("", "x"), 0);
    assert!(!document.is_modified());
}

#[test]
fn a_replacement_that_needs_preserved_space_gets_it() {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("keepXme")));

    let bytes = Document::create(&body).unwrap().save().unwrap();
    let mut document = Document::open(&bytes).unwrap();
    document.replace_text("keepXme", " padded ");

    let saved = document.save().unwrap();
    assert_eq!(
        Document::open(&saved).unwrap().plain_text(),
        " padded ",
        "the surrounding spaces were collapsed"
    );
}

#[test]
fn a_document_using_a_different_prefix_is_edited_correctly() {
    // Nothing requires the WordprocessingML namespace to be bound to "w".
    // Building "w:t" into a document that called it something else would produce
    // an undeclared prefix and a file Word refuses to open.
    let source = format!(
        "<x:document xmlns:x=\"{ns}\"><x:body>\
<x:p><x:r><x:t>original</x:t></x:r></x:p>\
</x:body></x:document>",
        ns = wp_docx::WORDPROCESSING_NAMESPACE
    );

    let mut document = Document::open(&package_around(&source)).unwrap();
    assert_eq!(document.replace_text("original", "edited"), 1);
    assert!(document.append_paragraph(&Paragraph::text("appended")));

    let saved = document.save().unwrap();
    let package = wp_opc::Package::open(&saved).unwrap();
    let xml = String::from_utf8(package.part("word/document.xml").unwrap().to_vec()).unwrap();

    assert!(!xml.contains("<w:"), "elements were written with the wrong prefix: {xml}");
    assert_eq!(Document::open(&saved).unwrap().plain_text(), "edited\nappended");
}

/// Wraps a `document.xml` in a minimal package so fragments can be tested.
fn package_around(document_xml: &str) -> Vec<u8> {
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
    package.save().unwrap()
}
