//! Ink in a document: the part, the relationship and the run that points at it.

use wp_docx::ink::{Ink, Stroke};
use wp_docx::model::{Block, Body, Paragraph, RunContent};
use wp_docx::{Document, TextPosition};

fn document() -> Document {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("Before after")));

    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    let mut document = Document::open(&bytes).expect("reopening");
    document.set_caret(TextPosition::new(0, 7));
    document
}

/// A tick, drawn in two strokes of a blue pen.
fn drawn() -> Ink {
    let pen = |points: Vec<(i64, i64)>| Stroke {
        colour: "0070C0".to_owned(),
        width_emu: 18_000,
        transparency: 0,
        flat: false,
        points,
    };
    Ink {
        strokes: vec![
            pen(vec![(0, 180_000), (90_000, 270_000)]),
            pen(vec![(90_000, 270_000), (270_000, 0)]),
        ],
    }
}

fn with_ink() -> Document {
    let mut document = document();
    assert!(document.insert_ink(&drawn()).expect("putting the ink in"));
    let bytes = document.save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

/// What the first paragraph says about ink, if it says anything.
fn reference_in(document: &Document) -> Option<wp_docx::model::InkReference> {
    let Block::Paragraph(paragraph) = &document.body().blocks[0] else { return None };
    paragraph.runs.iter().find_map(|run| {
        run.content.iter().find_map(|piece| match piece {
            RunContent::Ink(reference) => Some(reference.clone()),
            _ => None,
        })
    })
}

#[test]
fn a_document_has_no_ink_to_begin_with() {
    assert!(reference_in(&document()).is_none());
}

#[test]
fn ink_becomes_a_part_of_the_package_and_says_what_it_is() {
    let document = with_ink();
    assert!(document.package().part("word/ink/ink1.xml").is_some(), "the ink part is missing");
    assert_eq!(
        document.package().content_type("word/ink/ink1.xml"),
        Some(wp_docx::ink::INK_CONTENT_TYPE),
    );
}

#[test]
fn the_run_points_at_the_ink_through_a_relationship() {
    let document = with_ink();
    let reference = reference_in(&document).expect("a reference to ink");
    assert_eq!(
        document.relationship_target(&reference.relationship).as_deref(),
        Some("word/ink/ink1.xml"),
    );
}

#[test]
fn the_strokes_read_back_as_what_was_drawn() {
    let document = with_ink();
    let reference = reference_in(&document).expect("a reference to ink");
    let ink = document.ink(&reference.relationship).expect("the ink");

    assert_eq!(ink.strokes.len(), 2);
    assert_eq!(ink.strokes[0].colour, "0070C0");
    assert_eq!(ink.strokes[0].points, drawn().strokes[0].points);
    assert_eq!(ink.strokes[1].points, drawn().strokes[1].points);
}

#[test]
fn the_run_says_how_big_the_ink_is() {
    // Which is how big what was drawn is: the pen was somewhere when it drew
    // it, and nothing else states a size.
    let document = with_ink();
    let reference = reference_in(&document).expect("a reference to ink");
    let (left, top, right, bottom) = drawn().bounds().expect("some ink");
    assert_eq!(reference.width_emu, right - left);
    assert_eq!(reference.height_emu, bottom - top);
}

#[test]
fn the_document_says_a_reader_may_pass_the_ink_over() {
    // Without that, a reader too strict to know the extension stops at it and
    // the document does not open at all.
    let document = with_ink();
    let xml = document.package().xml_part("word/document.xml").expect("the part").expect("text");
    assert!(xml.contains("Ignorable"), "nothing says the extension may be ignored");
    assert!(xml.contains("w14"), "the namespace the ink is written in is not declared");
}

#[test]
fn ink_takes_one_character_of_the_text() {
    let mut document = document();
    let before = document.paragraph_text(0).unwrap_or_default().chars().count();
    assert!(document.insert_ink(&drawn()).expect("putting the ink in"));
    assert_eq!(document.paragraph_text(0).unwrap_or_default().chars().count(), before + 1);
}

#[test]
fn nothing_drawn_is_no_ink() {
    let mut document = document();
    assert!(!document.insert_ink(&Ink::default()).expect("nothing"));
    let one_point = Ink {
        strokes: vec![Stroke {
            colour: "000000".to_owned(),
            width_emu: 9_525,
            transparency: 0,
            flat: false,
            points: vec![(10, 10)],
        }],
    };
    assert!(!document.insert_ink(&one_point).expect("a pen put down and lifted"));
    assert!(document.package().part("word/ink/ink1.xml").is_none());
}

#[test]
fn two_lots_of_ink_keep_their_parts_apart() {
    let mut document = document();
    assert!(document.insert_ink(&drawn()).expect("the first"));
    assert!(document.insert_ink(&drawn()).expect("the second"));
    let bytes = document.save().expect("saving");
    let document = Document::open(&bytes).expect("reopening");

    for part in ["word/ink/ink1.xml", "word/ink/ink2.xml"] {
        assert!(document.package().part(part).is_some(), "{part} is missing");
    }
}

#[test]
fn ink_is_one_undo() {
    let mut document = document();
    assert!(document.insert_ink(&drawn()).expect("putting the ink in"));
    assert!(reference_in(&document).is_some());

    assert!(document.undo());
    assert!(reference_in(&document).is_none(), "the run is still in the text");
}
