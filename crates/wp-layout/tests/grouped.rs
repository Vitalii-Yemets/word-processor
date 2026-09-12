//! Laying out a group: the drawings inside it, each at its own fraction of the
//! rectangle the group covers.

use wp_docx::anchor::{Anchor, Placement, Wrap};
use wp_docx::floating::Turned;
use wp_docx::group::Rect;
use wp_docx::model::{Block, Body, Paragraph};
use wp_docx::shapes::Shape;
use wp_docx::{Document, TextPosition};
use wp_layout::{FontLibrary, LayoutEngine, Page, PageMetrics};

fn library() -> &'static FontLibrary {
    Box::leak(Box::new(FontLibrary::scan_system()))
}

/// A document with two one-inch shapes an inch apart, in the line of text.
///
/// In the line rather than floating, so that the place they end up is the
/// line's and not an anchor's — one thing at a time.
fn two_shapes() -> Document {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("Words to flow round them")));
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    let mut document = Document::open(&bytes).expect("reopening");

    for (name, colour) in [("One", "4472C4"), ("Two", "ED7D31")] {
        let shape = Shape {
            name: name.to_owned(),
            width_emu: 914_400,
            height_emu: 914_400,
            fill: wp_docx::fills::Fill::Solid(colour.to_owned()),
            anchor: Some(Anchor { wrap: Wrap::None, ..Anchor::default() }),
            ..Shape::default()
        };
        document.set_caret(TextPosition::new(0, 0));
        assert!(document.insert_shape(&shape), "the shape went nowhere");
    }
    document
}

/// The two drawings, side by side and an inch apart each way.
fn side_by_side() -> Vec<(TextPosition, Rect)> {
    vec![
        (TextPosition::new(0, 0), Rect { x: 0, y: 0, width: 914_400, height: 914_400 }),
        (TextPosition::new(0, 1), Rect { x: 914_400, y: 914_400, width: 914_400, height: 914_400 }),
    ]
}

fn pages(document: &Document) -> Vec<Page> {
    let mut engine = LayoutEngine::new(library());
    engine.layout_document_with(document, PageMetrics::from_document(document))
}

/// A document with the two shapes made one group.
fn grouped() -> Document {
    let mut document = two_shapes();
    document.group_drawings(&side_by_side()).expect("nothing was grouped");
    document
}

#[test]
fn everything_in_a_group_reaches_the_page() {
    let laid = pages(&grouped());
    let page = laid.first().expect("a page");
    assert_eq!(page.shapes.len(), 2, "the drawings in the group were not laid out");
}

#[test]
fn the_members_are_where_the_group_says_they_are() {
    let laid = pages(&grouped());
    let page = laid.first().expect("a page");

    let mut corners: Vec<(i32, i32)> =
        page.shapes.iter().map(|shape| (shape.x as i32, shape.y as i32)).collect();
    corners.sort_unstable();
    let (first, second) = (corners[0], corners[1]);
    // A group two inches square holds two one-inch drawings at opposite
    // corners, so the second is an inch along and an inch down from the first.
    assert_eq!(second.0 - first.0, second.1 - first.1, "they are not on the diagonal");
    assert!(second.0 - first.0 > 90, "an inch is about ninety-six pixels: {corners:?}");
    assert!(second.0 - first.0 < 102);
}

#[test]
fn a_member_is_drawn_at_its_own_size() {
    let laid = pages(&grouped());
    let page = laid.first().expect("a page");
    for shape in &page.shapes {
        // Half of a group two inches square: about ninety-six pixels.
        assert!(shape.width > 90.0 && shape.width < 102.0, "it is {} wide", shape.width);
        assert!(shape.height > 90.0 && shape.height < 102.0);
    }
}

#[test]
fn every_drawing_in_a_group_answers_to_the_group() {
    // A press on any of them takes hold of the group, which is what a group is
    // for: one place in the text, one thing to move.
    let laid = pages(&grouped());
    let page = laid.first().expect("a page");
    let places: Vec<Option<TextPosition>> = page.shapes.iter().map(|shape| shape.at).collect();
    assert_eq!(places[0], places[1], "the members answer to different places");
    assert_eq!(places[0], Some(TextPosition::new(0, 0)));
}

#[test]
fn a_group_is_as_tall_on_the_line_as_the_box_it_covers() {
    let laid = pages(&grouped());
    let page = laid.first().expect("a page");
    let line = page.lines.first().expect("a line");
    // Two inches, and the line has to be tall enough to hold it.
    assert!(line.ascent > 180.0, "the line is only {} tall", line.ascent);
}

#[test]
fn a_group_that_floats_goes_where_its_anchor_says() {
    let mut document = grouped();
    let at = TextPosition::new(0, 0);
    let anchor = Anchor {
        wrap: Wrap::Square,
        horizontal: Placement::Offset(1_828_800),
        vertical: Placement::Offset(914_400),
        ..Anchor::default()
    };
    assert!(document.set_anchor_at(at, Some(&anchor)));

    let laid = pages(&document);
    let page = laid.first().expect("a page");
    let leftmost = page.shapes.iter().map(|shape| shape.x).fold(f32::MAX, f32::min);
    // Two inches from the left edge of the text area, which starts an inch in.
    assert!(leftmost > 270.0, "the group is at {leftmost} across");
}

#[test]
fn turning_a_group_turns_everything_in_it() {
    let mut document = grouped();
    let at = TextPosition::new(0, 0);
    let quarter = Turned { rotation: Turned::WHOLE / 4, ..Turned::default() };
    assert!(document.set_drawing_turn_at(at, quarter));

    let laid = pages(&document);
    let page = laid.first().expect("a page");
    for shape in &page.shapes {
        assert!(
            (shape.turn - core::f32::consts::FRAC_PI_2).abs() < 0.001,
            "a member came out at {} radians",
            shape.turn
        );
    }

    // And it moved them: the two were on a diagonal going down to the right,
    // and a quarter turn clockwise puts that diagonal the other way.
    let mut corners: Vec<(f32, f32)> = page.shapes.iter().map(|shape| (shape.x, shape.y)).collect();
    corners.sort_by(|one, other| one.0.total_cmp(&other.0));
    assert!(
        corners[1].1 < corners[0].1,
        "the members did not go round with the group: {corners:?}"
    );
}

#[test]
fn a_group_inside_a_group_is_laid_out_through_both_rectangles() {
    let mut document = grouped();
    let inner = TextPosition::new(0, 0);

    // A third drawing beside the group, and the two made one.
    let shape = Shape {
        name: "Three".to_owned(),
        width_emu: 914_400,
        height_emu: 914_400,
        fill: wp_docx::fills::Fill::Solid("70AD47".to_owned()),
        anchor: Some(Anchor { wrap: Wrap::None, ..Anchor::default() }),
        ..Shape::default()
    };
    document.set_caret(TextPosition::new(0, inner.offset + 1));
    assert!(document.insert_shape(&shape));

    let outer = document
        .group_drawings(&[
            (inner, Rect { x: 0, y: 0, width: 1_828_800, height: 1_828_800 }),
            (
                TextPosition::new(0, inner.offset + 1),
                Rect { x: 1_828_800, y: 0, width: 914_400, height: 914_400 },
            ),
        ])
        .expect("nothing was grouped");
    let _ = outer;

    let laid = pages(&document);
    let page = laid.first().expect("a page");
    assert_eq!(page.shapes.len(), 3, "a group inside a group lost a drawing");
    // All three answer to the outer group, however deep they sit.
    assert!(page.shapes.iter().all(|shape| shape.at == Some(TextPosition::new(0, 0))));
}
