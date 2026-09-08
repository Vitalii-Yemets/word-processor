//! Text flowing round a drawing that floats on the page.

use wp_docx::anchor::{Anchor, Placement, Wrap};
use wp_docx::model::{Block, Body, Paragraph, Run, RunContent};
use wp_docx::shapes::Shape;
use wp_docx::Document;
use wp_layout::{FontLibrary, LayoutEngine, PageMetrics};

/// The fonts, read once per test.
fn library() -> &'static FontLibrary {
    Box::leak(Box::new(FontLibrary::scan_system()))
}

/// A document of one long paragraph, with a drawing at the front of it.
fn document(anchor: Option<Anchor>) -> Document {
    let words =
        "Some words that go on for long enough to fill several lines of a page. ".repeat(12);

    let mut runs = Vec::new();
    if let Some(anchor) = anchor {
        let shape = Shape::preset("rect", 144.0, 108.0).floating(anchor);
        runs.push(Run {
            properties: wp_docx::model::RunProperties::default(),
            content: vec![RunContent::Shape(shape)],
            field: None,
            revision: None,
        });
    }
    runs.push(Run::text(&words));

    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::from_runs(runs)));
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

/// The pages of a document, laid out on A4.
fn pages(document: &Document) -> Vec<wp_layout::Page> {
    let mut engine = LayoutEngine::new(library());
    engine.layout_document_with(document, PageMetrics::default())
}

/// The left edge of the widest line on a page, and of the narrowest.
fn line_extents(page: &wp_layout::Page) -> (f32, f32) {
    let widest = page.lines.iter().map(|line| line.right - line.left).fold(0.0f32, f32::max);
    let leftmost = page.lines.iter().map(|line| line.left).fold(f32::MAX, f32::min);
    (leftmost, widest)
}

#[test]
fn a_document_with_no_drawing_uses_the_whole_width() {
    let laid = pages(&document(None));
    assert!(!laid.is_empty());
    assert!(!laid[0].lines.is_empty(), "the text should be on the page");
}

#[test]
fn a_drawing_that_floats_is_drawn_on_the_page() {
    let laid = pages(&document(Some(Anchor::default())));
    assert_eq!(laid[0].shapes.len(), 1, "the shape should be placed once");
}

#[test]
fn text_beside_a_square_wrapped_drawing_is_narrower_than_text_below_it() {
    let with = pages(&document(Some(Anchor::default())));
    let without = pages(&document(None));

    let (_, widest_with) = line_extents(&with[0]);
    let (_, widest_without) = line_extents(&without[0]);
    // The lines beside the drawing are narrower, but the ones below it are
    // not — so the widest line on the page is the same either way.
    // Within a few per cent: a line ends on a word, so the widest line is
    // never exactly the width available to it.
    assert!(
        widest_with > widest_without * 0.95,
        "the lines below the drawing should still fill the page: {widest_with} against {widest_without}"
    );

    let narrowest_with =
        with[0].lines.iter().map(|line| line.right - line.left).fold(f32::MAX, f32::min);
    assert!(
        narrowest_with < widest_without * 0.9,
        "no line was narrowed by the drawing: narrowest {narrowest_with}, full {widest_without}"
    );
}

#[test]
fn a_drawing_on_the_right_pushes_the_text_left_rather_than_right() {
    let anchor = Anchor { horizontal: Placement::Aligned("right".to_owned()), ..Anchor::default() };
    let laid = pages(&document(Some(anchor)));
    let shape = &laid[0].shapes[0];
    let page_middle = laid[0].width / 2.0;
    assert!(shape.x > page_middle, "the drawing should be on the right, not at {}", shape.x);

    // Every line starts at the margin; the narrowed ones simply end sooner.
    let (leftmost, _) = line_extents(&laid[0]);
    assert!(leftmost < page_middle);
}

#[test]
fn a_drawing_wrapped_above_and_below_takes_the_whole_width() {
    let anchor = Anchor { wrap: Wrap::TopAndBottom, ..Anchor::default() };
    let laid = pages(&document(Some(anchor)));

    // No line is narrowed: the text stops above the drawing and starts below.
    let widths: Vec<f32> = laid[0].lines.iter().map(|line| line.right - line.left).collect();
    let widest = widths.iter().copied().fold(0.0f32, f32::max);
    let narrow = widths.iter().filter(|width| **width < widest * 0.9).count();
    assert!(narrow <= 1, "{narrow} lines were narrowed by a top-and-bottom drawing");
}

#[test]
fn a_drawing_behind_the_text_narrows_nothing() {
    let anchor = Anchor { wrap: Wrap::None, behind_text: true, ..Anchor::default() };
    let with = pages(&document(Some(anchor)));
    let without = pages(&document(None));

    let widths = |page: &wp_layout::Page| {
        page.lines.iter().map(|line| line.right - line.left).fold(f32::MAX, f32::min)
    };
    assert!(
        (widths(&with[0]) - widths(&without[0])).abs() < 2.0,
        "the text should run under a drawing that is behind it"
    );
}

#[test]
fn a_floating_drawing_takes_no_room_on_the_line_it_is_anchored_in() {
    let with = pages(&document(Some(Anchor { wrap: Wrap::None, ..Anchor::default() })));
    let without = pages(&document(None));
    // The same words, the same lines: an anchor is not a very large letter.
    assert_eq!(with[0].lines.len(), without[0].lines.len());
}

#[test]
fn a_drawing_in_the_line_does_take_room() {
    // The same words either way, so the only difference is the shape.
    let words =
        "Some words that go on for long enough to fill several lines of a page. ".repeat(12);
    let build = |with_shape: bool| {
        let mut runs = Vec::new();
        if with_shape {
            runs.push(Run {
                properties: wp_docx::model::RunProperties::default(),
                content: vec![RunContent::Shape(Shape::preset("rect", 144.0, 108.0))],
                field: None,
                revision: None,
            });
        }
        runs.push(Run::text(&words));
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::from_runs(runs)));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        Document::open(&bytes).expect("reopening")
    };

    let with = pages(&build(true));
    let without = pages(&build(false));
    assert!(
        with[0].lines.len() >= without[0].lines.len(),
        "a shape in the line should push the text along: {} against {}",
        with[0].lines.len(),
        without[0].lines.len()
    );
    assert_eq!(with[0].shapes.len(), 1);
}

/// The same document with a shape of a given kind and wrapping.
fn shaped(preset: &str, wrap: Wrap) -> Document {
    let words =
        "Some words that go on for long enough to fill several lines of a page. ".repeat(12);

    let anchor = Anchor {
        wrap,
        horizontal: Placement::Offset(0),
        vertical: Placement::Offset(0),
        ..Anchor::default()
    };
    let shape = Shape::preset(preset, 144.0, 144.0).floating(anchor);

    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::from_runs(vec![
        Run {
            properties: wp_docx::model::RunProperties::default(),
            content: vec![RunContent::Shape(shape)],
            field: None,
            revision: None,
        },
        Run::text(&words),
    ])));
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

/// How wide the lines beside a drawing come out.
///
/// The first line is left out: the line a floating drawing is anchored in keeps
/// the whole width, which is what makes it the anchor rather than an obstacle.
fn top_line_widths(page: &wp_layout::Page) -> Vec<f32> {
    page.lines.iter().skip(1).take(4).map(|line| line.right - line.left).collect()
}

#[test]
fn tight_wrapping_round_a_triangle_widens_towards_its_point() {
    // The point of a triangle is at the top, so the first line beside it has
    // more room than the ones further down.
    let laid = pages(&shaped("triangle", Wrap::Tight));
    let widths = top_line_widths(&laid[0]);
    assert!(widths.len() >= 3, "not enough lines to compare: {widths:?}");
    assert!(widths[0] > widths[3], "the text did not follow the outline: {widths:?}");
}

#[test]
fn square_wrapping_round_the_same_triangle_keeps_every_line_the_same() {
    let laid = pages(&shaped("triangle", Wrap::Square));
    let widths = top_line_widths(&laid[0]);
    // Every line beside the drawing has the same room, because the box is the
    // same all the way down. Lines end on a word, so they differ a little.
    let widest = widths.iter().copied().fold(0.0f32, f32::max);
    let narrowest = widths.iter().copied().fold(f32::MAX, f32::min);
    assert!(widest - narrowest < widest * 0.3, "{widths:?}");
}

#[test]
fn tight_wrapping_round_a_rectangle_is_the_same_as_square_wrapping() {
    // A rectangle is its own box, so following the outline changes nothing.
    let tight = top_line_widths(&pages(&shaped("rect", Wrap::Tight))[0]);
    let square = top_line_widths(&pages(&shaped("rect", Wrap::Square))[0]);
    assert_eq!(tight, square);
}
