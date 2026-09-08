//! Tests for editing at a place in the text, the way typing does.
//!
//! Every one of these asks the same question in a different form: did the edit
//! change what it was asked to change, and nothing else? Formatting on the runs
//! either side, markup the program does not model, and the other parts of the
//! package all have to survive.

use wp_docx::model::{Block, Body, Paragraph, Run, RunContent, RunProperties};
use wp_docx::{Document, TextPosition};

/// A document of plain paragraphs.
fn document_with(lines: &[&str]) -> Vec<u8> {
    let mut body = Body::default();
    for line in lines {
        body.blocks.push(Block::Paragraph(Paragraph::text(line)));
    }
    Document::create(&body).unwrap().save().unwrap()
}

/// Wraps a `document.xml` in a package so a fragment can be tested.
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

fn main_part_xml(bytes: &[u8]) -> String {
    let package = wp_opc::Package::open(bytes).unwrap();
    String::from_utf8(package.part("word/document.xml").unwrap().to_vec()).unwrap()
}

#[test]
fn paragraphs_are_counted_in_reading_order() {
    let bytes = document_with(&["first", "second", "third"]);
    let document = Document::open(&bytes).unwrap();

    assert_eq!(document.paragraph_count(), 3);
    assert_eq!(document.paragraph_text(0).as_deref(), Some("first"));
    assert_eq!(document.paragraph_text(2).as_deref(), Some("third"));
    assert_eq!(document.paragraph_text(3), None);
}

#[test]
fn typing_inserts_at_the_caret() {
    let bytes = document_with(&["Hello world"]);
    let mut document = Document::open(&bytes).unwrap();

    assert!(document.insert_text(TextPosition::new(0, 5), ","));
    assert_eq!(document.paragraph_text(0).as_deref(), Some("Hello, world"));

    // And it survives a save and a reopen.
    let saved = document.save().unwrap();
    assert_eq!(Document::open(&saved).unwrap().plain_text(), "Hello, world");
}

#[test]
fn typing_at_the_start_and_the_end_both_work() {
    let bytes = document_with(&["middle"]);
    let mut document = Document::open(&bytes).unwrap();

    assert!(document.insert_text(TextPosition::new(0, 0), "the "));
    assert!(document.insert_text(TextPosition::new(0, 10), " part"));
    assert_eq!(document.paragraph_text(0).as_deref(), Some("the middle part"));
}

#[test]
fn typing_into_an_empty_paragraph_works() {
    // An empty paragraph has no run to put text in, so one has to be made.
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::default()));
    let bytes = Document::create(&body).unwrap().save().unwrap();

    let mut document = Document::open(&bytes).unwrap();
    assert!(document.insert_text(TextPosition::new(0, 0), "first words"));

    let saved = document.save().unwrap();
    assert_eq!(Document::open(&saved).unwrap().plain_text(), "first words");
}

#[test]
fn typing_in_a_formatted_run_keeps_the_formatting() {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::from_runs(vec![
        Run::text("plain "),
        Run::text("bold").bold(),
    ])));
    let bytes = Document::create(&body).unwrap().save().unwrap();

    let mut document = Document::open(&bytes).unwrap();
    // Inside the bold run.
    assert!(document.insert_text(TextPosition::new(0, 8), "XX"));
    assert_eq!(document.paragraph_text(0).as_deref(), Some("plain boXXld"));

    let saved = document.save().unwrap();
    let reopened = Document::open(&saved).unwrap();
    let Block::Paragraph(paragraph) = &reopened.body().blocks[0] else { panic!() };

    assert_eq!(paragraph.runs.len(), 2, "no run should have been added");
    assert_eq!(paragraph.runs[1].properties.bold, Some(true));
    assert_eq!(paragraph.runs[1].plain_text(), "boXXld");
}

#[test]
fn typing_at_a_run_boundary_stays_with_the_earlier_run() {
    // Typing at the end of a bold word should continue in bold, not jump to
    // whatever comes next.
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::from_runs(vec![
        Run::text("bold").bold(),
        Run::text(" plain"),
    ])));
    let bytes = Document::create(&body).unwrap().save().unwrap();

    let mut document = Document::open(&bytes).unwrap();
    assert!(document.insert_text(TextPosition::new(0, 4), "er"));

    let saved = document.save().unwrap();
    let reopened = Document::open(&saved).unwrap();
    let Block::Paragraph(paragraph) = &reopened.body().blocks[0] else { panic!() };

    assert_eq!(paragraph.runs[0].plain_text(), "bolder");
    assert_eq!(paragraph.runs[0].properties.bold, Some(true));
}

#[test]
fn deleting_removes_exactly_the_range() {
    let bytes = document_with(&["abcdefghij"]);
    let mut document = Document::open(&bytes).unwrap();

    assert!(document.delete_range(0, 3, 6));
    assert_eq!(document.paragraph_text(0).as_deref(), Some("abcghij"));
}

#[test]
fn deleting_across_runs_removes_from_each() {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::from_runs(vec![
        Run::text("aaa"),
        Run::text("bbb").bold(),
        Run::text("ccc"),
    ])));
    let bytes = Document::create(&body).unwrap().save().unwrap();

    let mut document = Document::open(&bytes).unwrap();
    // From the middle of the first run to the middle of the last.
    assert!(document.delete_range(0, 2, 7));
    assert_eq!(document.paragraph_text(0).as_deref(), Some("aacc"));

    let saved = document.save().unwrap();
    let reopened = Document::open(&saved).unwrap();
    let Block::Paragraph(paragraph) = &reopened.body().blocks[0] else { panic!() };
    assert_eq!(paragraph.runs.len(), 3, "the emptied run should still be there");
    assert_eq!(paragraph.runs[1].properties.bold, Some(true));
}

#[test]
fn deleting_nothing_changes_nothing() {
    let bytes = document_with(&["unchanged"]);
    let mut document = Document::open(&bytes).unwrap();

    assert!(!document.delete_range(0, 3, 3));
    assert!(!document.delete_range(0, 5, 2));
    assert!(!document.is_modified());
    assert_eq!(document.save().unwrap(), bytes);
}

#[test]
fn multibyte_characters_are_never_cut_in_half() {
    // Offsets are in bytes, and a Cyrillic letter is two of them. Splitting one
    // would produce a string that is not text at all.
    let bytes = document_with(&["Привет"]);
    let mut document = Document::open(&bytes).unwrap();

    // Offset 1 is inside the first letter.
    assert!(!document.insert_text(TextPosition::new(0, 1), "X"));
    assert!(!document.delete_range(0, 1, 3));
    assert_eq!(document.paragraph_text(0).as_deref(), Some("Привет"));

    // On a boundary it works.
    assert!(document.insert_text(TextPosition::new(0, 2), "X"));
    assert_eq!(document.paragraph_text(0).as_deref(), Some("ПXривет"));
}

#[test]
fn pressing_enter_splits_a_paragraph() {
    let bytes = document_with(&["one two"]);
    let mut document = Document::open(&bytes).unwrap();

    assert!(document.split_paragraph(TextPosition::new(0, 3)));
    assert_eq!(document.paragraph_count(), 2);
    assert_eq!(document.paragraph_text(0).as_deref(), Some("one"));
    assert_eq!(document.paragraph_text(1).as_deref(), Some(" two"));

    let saved = document.save().unwrap();
    assert_eq!(Document::open(&saved).unwrap().plain_text(), "one\n two");
}

#[test]
fn splitting_keeps_the_style_on_both_halves() {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("a heading").with_style("Heading1")));
    let bytes = Document::create(&body).unwrap().save().unwrap();

    let mut document = Document::open(&bytes).unwrap();
    assert!(document.split_paragraph(TextPosition::new(0, 2)));

    let saved = document.save().unwrap();
    let reopened = Document::open(&saved).unwrap();
    let body = reopened.body();
    let styles: Vec<Option<&str>> =
        body.paragraphs().iter().map(|paragraph| paragraph.style()).collect();

    assert_eq!(styles, [Some("Heading1"), Some("Heading1")]);
}

#[test]
fn splitting_inside_a_run_keeps_its_formatting_on_both_sides() {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::from_runs(vec![Run::text("bolded").bold()])));
    let bytes = Document::create(&body).unwrap().save().unwrap();

    let mut document = Document::open(&bytes).unwrap();
    assert!(document.split_paragraph(TextPosition::new(0, 4)));

    let saved = document.save().unwrap();
    let reopened = Document::open(&saved).unwrap();
    let body = reopened.body();
    let paragraphs = body.paragraphs();

    assert_eq!(paragraphs[0].plain_text(), "bold");
    assert_eq!(paragraphs[1].plain_text(), "ed");
    for paragraph in &paragraphs {
        for run in &paragraph.runs {
            if !run.plain_text().is_empty() {
                assert_eq!(run.properties.bold, Some(true), "a half lost its weight");
            }
        }
    }
}

#[test]
fn splitting_at_the_end_leaves_an_empty_paragraph() {
    let bytes = document_with(&["complete"]);
    let mut document = Document::open(&bytes).unwrap();

    assert!(document.split_paragraph(TextPosition::new(0, 8)));
    assert_eq!(document.paragraph_count(), 2);
    assert_eq!(document.paragraph_text(0).as_deref(), Some("complete"));
    assert_eq!(document.paragraph_text(1).as_deref(), Some(""));
}

#[test]
fn backspace_at_the_start_joins_onto_the_previous_paragraph() {
    let bytes = document_with(&["first", "second"]);
    let mut document = Document::open(&bytes).unwrap();

    assert!(document.merge_with_previous(1));
    assert_eq!(document.paragraph_count(), 1);
    assert_eq!(document.paragraph_text(0).as_deref(), Some("firstsecond"));
}

#[test]
fn there_is_nothing_before_the_first_paragraph_to_join_onto() {
    let bytes = document_with(&["only"]);
    let mut document = Document::open(&bytes).unwrap();

    assert!(!document.merge_with_previous(0));
    assert!(!document.is_modified());
}

#[test]
fn a_split_and_a_join_return_the_document_to_where_it_started() {
    let original = document_with(&["one two three"]);
    let mut document = Document::open(&original).unwrap();

    document.split_paragraph(TextPosition::new(0, 7));
    document.merge_with_previous(1);

    assert_eq!(document.paragraph_count(), 1);
    assert_eq!(document.paragraph_text(0).as_deref(), Some("one two three"));
}

#[test]
fn joining_does_not_reach_across_a_table_cell() {
    // The paragraph after the one in a cell is outside the table. Pressing
    // Backspace there must not drag text out of the cell.
    let source = format!(
        "<w:document xmlns:w=\"{ns}\"><w:body>\
<w:tbl><w:tr><w:tc><w:p><w:r><w:t>in a cell</w:t></w:r></w:p></w:tc></w:tr></w:tbl>\
<w:p><w:r><w:t>after the table</w:t></w:r></w:p>\
</w:body></w:document>",
        ns = wp_docx::WORDPROCESSING_NAMESPACE
    );

    let mut document = Document::open(&package_around(&source)).unwrap();
    assert_eq!(document.paragraph_count(), 2);

    assert!(!document.merge_with_previous(1), "the join should have been refused");
    assert!(!document.is_modified());
}

#[test]
fn editing_carries_unknown_markup_through_untouched() {
    let source = format!(
        "<w:document xmlns:w=\"{ns}\"><w:body>\
<w:p><w:bookmarkStart w:id=\"1\" w:name=\"mark\"/><w:r><w:t>edit here</w:t></w:r>\
<w:bookmarkEnd w:id=\"1\"/></w:p>\
<w:customUnknown attribute=\"kept\"><!-- note --></w:customUnknown>\
<w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/></w:sectPr>\
</w:body></w:document>",
        ns = wp_docx::WORDPROCESSING_NAMESPACE
    );

    let mut document = Document::open(&package_around(&source)).unwrap();
    assert!(document.insert_text(TextPosition::new(0, 4), "ed"));

    let xml = main_part_xml(&document.save().unwrap());
    assert!(xml.contains("edited here"), "the edit did not apply: {xml}");
    assert!(xml.contains("<w:bookmarkStart w:id=\"1\" w:name=\"mark\"/>"), "bookmark lost");
    assert!(xml.contains("<w:customUnknown attribute=\"kept\"><!-- note --></w:customUnknown>"));
    assert!(xml.contains("<w:sectPr>"), "the section properties were lost");
}

#[test]
fn an_edit_changes_only_the_main_part() {
    let original = document_with(&["one", "two"]);
    let before = wp_opc::Package::open(&original).unwrap();

    let mut document = Document::open(&original).unwrap();
    document.insert_text(TextPosition::new(1, 3), " more");
    let saved = document.save().unwrap();
    let after = wp_opc::Package::open(&saved).unwrap();

    for entry in before.entries() {
        let updated = after.part(&entry.name).unwrap_or_else(|| panic!("{} lost", entry.name));
        if entry.name != "word/document.xml" {
            assert_eq!(updated, entry.data.as_slice(), "{} should not have changed", entry.name);
        }
    }
}

#[test]
fn positions_outside_the_document_are_refused() {
    let bytes = document_with(&["short"]);
    let mut document = Document::open(&bytes).unwrap();

    assert!(!document.insert_text(TextPosition::new(9, 0), "x"));
    assert!(!document.delete_range(9, 0, 1));
    assert!(!document.split_paragraph(TextPosition::new(9, 0)));
    assert!(!document.merge_with_previous(9));
    assert!(!document.is_modified());

    // An offset past the end of a paragraph appends rather than failing, which
    // is what a caret at the end of a line needs.
    assert!(document.insert_text(TextPosition::new(0, 999), "!"));
    assert_eq!(document.paragraph_text(0).as_deref(), Some("short!"));
}

#[test]
fn a_run_of_edits_leaves_a_readable_document() {
    // Roughly what a person does: type, split, type again, delete, join.
    let bytes = document_with(&["start"]);
    let mut document = Document::open(&bytes).unwrap();

    document.insert_text(TextPosition::new(0, 5), " here");
    document.split_paragraph(TextPosition::new(0, 5));
    document.insert_text(TextPosition::new(1, 0), "and");
    document.delete_range(0, 0, 1);
    document.merge_with_previous(1);

    let saved = document.save().unwrap();
    let reopened = Document::open(&saved).unwrap();
    assert_eq!(reopened.plain_text(), "tartand here");
    assert_eq!(reopened.paragraph_count(), 1);
}

#[test]
fn text_in_every_script_can_be_typed() {
    let bytes = document_with(&[""]);
    let mut document = Document::open(&bytes).unwrap();

    let mut expected = String::new();
    for piece in ["Hello ", "Привет ", "Γειά ", "你好 ", "مرحبا ", "שלום"] {
        assert!(document.insert_text(TextPosition::new(0, expected.len()), piece));
        expected.push_str(piece);
    }

    let saved = document.save().unwrap();
    assert_eq!(Document::open(&saved).unwrap().plain_text(), expected);
}

#[test]
fn a_tab_counts_as_one_character_of_the_paragraph() {
    // A tab is an element, not text, but the caret passes over it like any
    // other character — so it has to take up exactly one place.
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph {
        runs: vec![Run {
            properties: RunProperties::default(),
            field: None,
            revision: None,
            content: vec![RunContent::Tab],
        }],
        ..Paragraph::default()
    }));
    let bytes = Document::create(&body).unwrap().save().unwrap();

    let document = Document::open(&bytes).unwrap();
    assert_eq!(document.paragraph_text(0).as_deref(), Some("\t"));
}

#[test]
fn a_paragraph_with_no_runs_reports_empty_text_rather_than_failing() {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::default()));
    let bytes = Document::create(&body).unwrap().save().unwrap();

    let document = Document::open(&bytes).unwrap();
    assert_eq!(document.paragraph_text(0).as_deref(), Some(""));
}

#[test]
fn saving_twice_keeps_the_edits_both_times() {
    // Clearing the modified flag without committing the tree into the package
    // would make the second save write the document as it was before the edits.
    let bytes = document_with(&["original"]);
    let mut document = Document::open(&bytes).unwrap();

    document.insert_text(TextPosition::new(0, 8), " and more");
    let first = document.save().unwrap();
    document.mark_saved().unwrap();

    assert!(!document.is_modified(), "the document should count as saved");
    let second = document.save().unwrap();

    assert_eq!(first, second, "the second save wrote something different");
    assert_eq!(Document::open(&second).unwrap().plain_text(), "original and more");
}

#[test]
fn marking_an_unedited_document_saved_changes_nothing() {
    let bytes = document_with(&["untouched"]);
    let mut document = Document::open(&bytes).unwrap();

    document.mark_saved().unwrap();
    assert_eq!(document.save().unwrap(), bytes);
}
