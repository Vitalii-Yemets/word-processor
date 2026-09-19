//! Diagrams in a document: the five parts, the relationships and the frame.

use wp_docx::diagram::Arrangement;
use wp_docx::model::{Block, Body, Paragraph, RunContent};
use wp_docx::{Document, TextPosition, EMU_PER_INCH};

const ROOM: i64 = EMU_PER_INCH * 6;

fn document() -> Document {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("Before after")));

    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    let mut document = Document::open(&bytes).expect("reopening");
    document.set_caret(TextPosition::new(0, 7));
    document
}

fn items() -> Vec<String> {
    ["Plan", "Draw", "Check"].iter().map(|item| (*item).to_owned()).collect()
}

fn with_diagram(arrangement: Arrangement) -> Document {
    let mut document = document();
    assert!(document.insert_diagram(arrangement, &items(), ROOM).expect("inserting the diagram"));
    let bytes = document.save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

/// The diagram reference in the first paragraph, if there is one.
fn reference_in(document: &Document) -> Option<wp_docx::model::DiagramReference> {
    let Block::Paragraph(paragraph) = &document.body().blocks[0] else { return None };
    paragraph.runs.iter().find_map(|run| {
        run.content.iter().find_map(|piece| match piece {
            RunContent::Diagram(reference) => Some(reference.clone()),
            _ => None,
        })
    })
}

#[test]
fn a_document_has_no_diagrams_to_begin_with() {
    assert!(reference_in(&document()).is_none());
}

#[test]
fn every_part_a_diagram_needs_becomes_part_of_the_package() {
    let document = with_diagram(Arrangement::Process);
    for part in [
        "word/diagrams/data1.xml",
        "word/diagrams/layout1.xml",
        "word/diagrams/quickStyle1.xml",
        "word/diagrams/colors1.xml",
        "word/diagrams/drawing1.xml",
    ] {
        assert!(document.package().part(part).is_some(), "{part} is missing");
    }
}

#[test]
fn every_part_says_what_it_is() {
    // A part whose type the package does not declare makes the package
    // invalid, and Word says so rather than opening it.
    let document = with_diagram(Arrangement::Process);
    for (part, kind) in [
        ("word/diagrams/data1.xml", wp_docx::diagram::DATA_CONTENT_TYPE),
        ("word/diagrams/layout1.xml", wp_docx::diagram::LAYOUT_CONTENT_TYPE),
        ("word/diagrams/quickStyle1.xml", wp_docx::diagram::STYLE_CONTENT_TYPE),
        ("word/diagrams/colors1.xml", wp_docx::diagram::COLORS_CONTENT_TYPE),
        ("word/diagrams/drawing1.xml", wp_docx::diagram::DRAWING_CONTENT_TYPE),
    ] {
        assert_eq!(document.package().content_type(part), Some(kind), "{part}");
    }
}

#[test]
fn the_frame_points_at_the_data_model_through_a_relationship() {
    let document = with_diagram(Arrangement::Process);
    let reference = reference_in(&document).expect("a diagram reference");
    assert_eq!(
        document.relationship_target(&reference.relationship).as_deref(),
        Some("word/diagrams/data1.xml"),
    );
    assert_eq!(reference.width_emu, ROOM);
    assert!(reference.height_emu > 0, "a diagram with no height is drawn as nothing");
}

#[test]
fn the_drawing_is_reached_from_the_data_model_and_not_from_the_document() {
    // Which is where the format puts it: the drawing is the data model's own.
    let document = with_diagram(Arrangement::Process);
    let relationships =
        document.package().relationships("word/diagrams/data1.xml").expect("relationships");
    let drawing = relationships
        .single_by_type(wp_docx::diagram::DRAWING_RELATIONSHIP)
        .expect("the drawing relationship");
    assert_eq!(drawing.target, "drawing1.xml");

    let from_document =
        document.package().relationships("word/document.xml").expect("the document");
    assert!(
        from_document.by_type(wp_docx::diagram::DRAWING_RELATIONSHIP).next().is_none(),
        "the document points at the drawing itself"
    );
}

#[test]
fn the_words_read_back_out_of_the_data_model() {
    let document = with_diagram(Arrangement::Process);
    let reference = reference_in(&document).expect("a diagram reference");
    let diagram = document.diagram(&reference).expect("the diagram");

    assert_eq!(diagram.arrangement, Some(Arrangement::Process));
    assert_eq!(diagram.text(), items());
}

#[test]
fn a_hierarchy_comes_back_as_a_tree() {
    let document = with_diagram(Arrangement::Hierarchy);
    let reference = reference_in(&document).expect("a diagram reference");
    let diagram = document.diagram(&reference).expect("the diagram");

    assert_eq!(diagram.nodes.len(), 1);
    assert_eq!(diagram.nodes[0].text, "Plan");
    assert_eq!(diagram.nodes[0].children.len(), 2);
    // And the text pane lists them all the same, in the order it walks them.
    assert_eq!(diagram.text(), items());
}

#[test]
fn what_is_drawn_is_a_shape_for_every_box_with_its_words_in_it() {
    let document = with_diagram(Arrangement::Process);
    let reference = reference_in(&document).expect("a diagram reference");
    let diagram = document.diagram(&reference).expect("the diagram");
    let drawing = diagram.drawing.expect("the drawing");

    // Three boxes and the two arrows between them, and the words of each
    // box as a member of their own, hung in the middle of it.
    assert_eq!(drawing.members.len(), 8);
    let words: Vec<String> = drawing
        .members
        .iter()
        .filter_map(|member| match &member.what {
            wp_docx::group::Inside::Shape(shape) => Some(shape),
            _ => None,
        })
        .filter(|shape| !shape.text.is_empty())
        .map(|shape| shape.text[0].plain_text())
        .collect();
    assert_eq!(words, items());
}

#[test]
fn the_drawing_fills_the_frame_it_was_given() {
    // The frame is what the text made room for; what the shapes are measured
    // in is their own rectangle. A drawing measured in the wrong one is drawn
    // at the wrong size.
    let document = with_diagram(Arrangement::Process);
    let reference = reference_in(&document).expect("a diagram reference");
    let diagram = document.diagram(&reference).expect("the diagram");
    let drawing = diagram.drawing.expect("the drawing");

    assert_eq!(drawing.width_emu, reference.width_emu);
    assert_eq!(drawing.height_emu, reference.height_emu);
    assert!(drawing.child_width > 0 && drawing.child_height > 0);
}

#[test]
fn two_diagrams_keep_their_parts_apart() {
    let mut document = document();
    assert!(document.insert_diagram(Arrangement::Process, &items(), ROOM).expect("the first"));
    assert!(document.insert_diagram(Arrangement::List, &items(), ROOM).expect("the second"));
    let bytes = document.save().expect("saving");
    let document = Document::open(&bytes).expect("reopening");

    for part in ["word/diagrams/data1.xml", "word/diagrams/data2.xml"] {
        assert!(document.package().part(part).is_some(), "{part} is missing");
    }
}

#[test]
fn nothing_typed_is_no_diagram() {
    let mut document = document();
    assert!(!document.insert_diagram(Arrangement::Process, &[], ROOM).expect("nothing"));
    assert!(!document
        .insert_diagram(Arrangement::Process, &["  ".to_owned()], ROOM)
        .expect("nothing but spaces"));
    assert!(document.package().part("word/diagrams/data1.xml").is_none());
}

#[test]
fn a_diagram_is_one_undo() {
    let mut document = document();
    assert!(document.insert_diagram(Arrangement::Process, &items(), ROOM).expect("inserting"));
    assert!(reference_in(&document).is_some());

    assert!(document.undo());
    assert!(reference_in(&document).is_none(), "the frame is still in the text");
}

#[test]
fn a_diagram_with_nothing_said_about_it_is_reported() {
    // Word's own checker says the same about SmartArt with no alternative
    // text, and a diagram is one drawing however many boxes it holds: the
    // words in the boxes are not a description of what it says.
    let document = with_diagram(Arrangement::Process);
    let findings = document.accessibility_findings();
    assert!(
        findings.iter().any(|finding| finding.problem.contains("has no description")),
        "a diagram with no description passed the check: {findings:?}"
    );
}
