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
        pressure: Vec::new(),
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
    // Moved so that the ink's own space starts at the corner of what the
    // pen covered, which is half the pen's width in from the first point.
    let (left, top, _, _) = drawn().bounds().expect("some ink");
    let moved = |points: &[(i64, i64)]| -> Vec<(i64, i64)> {
        points.iter().map(|(x, y)| (x - left, y - top)).collect()
    };
    assert_eq!(ink.strokes[0].points, moved(&drawn().strokes[0].points));
    assert_eq!(ink.strokes[1].points, moved(&drawn().strokes[1].points));
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
            pressure: Vec::new(),
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

/// The same tick, drawn on the page rather than typed: it floats.
fn floating_anchor() -> wp_docx::anchor::Anchor {
    use wp_docx::anchor::{Anchor, Placement, Relative, Wrap};
    Anchor {
        wrap: Wrap::None,
        horizontal_from: Relative::Page,
        horizontal: Placement::Offset(914_400),
        vertical_from: Relative::Paragraph,
        vertical: Placement::Offset(457_200),
        ..Anchor::default()
    }
}

fn with_floating_ink() -> Document {
    let mut document = document();
    assert!(document.insert_ink_floating(&drawn(), &floating_anchor()).expect("drawing"));
    let bytes = document.save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

#[test]
fn ink_drawn_on_the_page_floats_where_the_pen_went_and_reads_back_so() {
    let document = with_floating_ink();
    let reference = reference_in(&document).expect("a reference to ink");
    let anchor = reference.anchor.expect("the ink floats");
    assert_eq!(anchor, floating_anchor());
    assert_eq!(reference.name, "Ink 1");
    let (left, top, right, bottom) = drawn().bounds().expect("some ink");
    assert_eq!((reference.width_emu, reference.height_emu), (right - left, bottom - top));
    // The strokes are the same, and the drawing is one character of the text.
    let ink = document.ink(&reference.relationship).expect("the ink");
    assert_eq!(ink.strokes.len(), 2);
    assert_eq!(document.paragraph_text(0).unwrap_or_default().chars().count(), 13);
    // Written the way Word writes ink: a drawing, in an alternative that
    // names the ink extension.
    let xml = document.package().xml_part("word/document.xml").expect("the part").expect("text");
    assert!(xml.contains("Requires=\"wpi\""), "the alternative does not name the extension");
    assert!(xml.contains("wordprocessingInk"), "the graphic does not say it is ink");
    assert!(xml.contains("<wp:anchor") || xml.contains(":anchor "), "the drawing does not float");
}

#[test]
fn ink_written_the_way_word_writes_it_is_read_as_ink_and_not_as_a_picture() {
    let document = with_floating_ink();
    assert!(document.ink_at(TextPosition::new(0, 7)).is_some(), "the drawing is not ink");
    assert!(document.shapes().is_empty(), "the ink was read as a shape");
}

#[test]
fn how_hard_the_pen_was_pressed_is_written_and_read_back() {
    let mut document = document();
    let mut pressed = drawn();
    pressed.strokes[0].pressure = vec![0.25, 1.0];
    assert!(document.insert_ink(&pressed).expect("putting the ink in"));
    let reference = reference_in(&document).expect("a reference to ink");
    let ink = document.ink(&reference.relationship).expect("the ink");
    assert_eq!(ink.strokes[0].pressure.len(), 2);
    assert!((ink.strokes[0].pressure[0] - 0.25).abs() < 0.001);
    assert!((ink.strokes[0].pressure[1] - 1.0).abs() < 0.001);
    // The stroke that never said is pressed evenly, halfway, once written
    // beside one that did.
    assert_eq!(ink.strokes[1].pressure.len(), 2);
    assert!((ink.strokes[1].pressure[0] - 0.5).abs() < 0.001);
    // And ink with no pressure anywhere writes none.
    let plain = with_ink();
    let reference = reference_in(&plain).expect("a reference to ink");
    let ink = plain.ink(&reference.relationship).expect("the ink");
    assert!(ink.strokes.iter().all(|stroke| stroke.pressure.is_empty()));
}

#[test]
fn the_eraser_leaves_what_it_did_not_touch_where_it_was() {
    let mut document = with_floating_ink();
    let at = TextPosition::new(0, 7);
    let mut ink = document.ink(&document.ink_at(at).expect("ink").relationship).expect("the ink");
    // Rub out the first stroke: what is left starts lower and further right
    // in the ink's own space, so the drawing moves by that much.
    ink.strokes.remove(0);
    let (left, top, right, bottom) = ink.bounds().expect("some ink");
    assert!(document.replace_ink_at(at, &ink));
    let reference = document.ink_at(at).expect("still ink");
    assert_eq!((reference.width_emu, reference.height_emu), (right - left, bottom - top));
    let anchor = reference.anchor.expect("still floats");
    assert_eq!(anchor.horizontal, wp_docx::anchor::Placement::Offset(914_400 + left));
    assert_eq!(anchor.vertical, wp_docx::anchor::Placement::Offset(457_200 + top));
    let back = document.ink(&reference.relationship).expect("the ink");
    assert_eq!(back.strokes.len(), 1);
    assert_eq!(
        back.strokes[0].points[0],
        (ink.strokes[0].points[0].0 - left, ink.strokes[0].points[0].1 - top)
    );
    // One undo puts the stroke back.
    assert!(document.undo());
    let ink = document.ink(&document.ink_at(at).expect("ink").relationship).expect("the ink");
    assert_eq!(ink.strokes.len(), 2);

    // Nothing left takes the drawing away altogether.
    assert!(document.replace_ink_at(at, &Ink::default()));
    assert!(document.ink_at(at).is_none());
    assert_eq!(document.paragraph_text(0).unwrap_or_default(), "Before after");
}
