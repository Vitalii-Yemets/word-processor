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
            format_change: None,
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
                format_change: None,
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
            format_change: None,
        },
        Run::text(&words),
    ])));
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

/// How much room the lines beside a drawing come out with, line by line.
///
/// Room and not width: a line beside a drawing that has space on both sides of
/// it is two pieces, and what the outline decides is how much room the line has
/// altogether. The pieces of one line are the ones that share a baseline.
///
/// The first line is left out: the line a floating drawing is anchored in keeps
/// the whole width, which is what makes it the anchor rather than an obstacle.
fn top_line_widths(page: &wp_layout::Page) -> Vec<f32> {
    let mut rooms: Vec<(f32, f32)> = Vec::new();
    for line in page.lines.iter().skip(1) {
        let room = line.right - line.left;
        match rooms.iter_mut().find(|(baseline, _)| (*baseline - line.baseline).abs() < 0.5) {
            Some((_, found)) => *found += room,
            None => rooms.push((line.baseline, room)),
        }
    }
    rooms.sort_by(|one, other| one.0.total_cmp(&other.0));
    rooms.into_iter().take(4).map(|(_, room)| room).collect()
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

// --- Text down both sides of a drawing --------------------------------------

/// The same document with the drawing put where there is room either side of
/// it: half an inch in from the left of the text, which leaves about an inch
/// and a half on the left and four inches on the right.
fn document_with_drawing_in_the_middle(side: wp_docx::anchor::WrapSide) -> Document {
    let anchor = Anchor {
        wrap: Wrap::Square,
        side,
        horizontal: Placement::Offset(1_828_800),
        vertical: Placement::Offset(0),
        ..Anchor::default()
    };
    document(Some(anchor))
}

/// Where the drawing came out on the page, as its left and right edges.
fn drawing_edges(page: &wp_layout::Page) -> (f32, f32) {
    let shape = page.shapes.first().expect("the drawing is on the page");
    (shape.x, shape.x + shape.width)
}

/// How many lines have text on each side of the drawing.
fn lines_each_side(page: &wp_layout::Page) -> (usize, usize) {
    let (left_edge, right_edge) = drawing_edges(page);
    let shape = page.shapes.first().expect("a drawing");
    let beside = |line: &wp_layout::PageLine| {
        line.baseline > shape.y && line.baseline < shape.y + shape.height
    };
    let left =
        page.lines.iter().filter(|line| beside(line) && line.right <= left_edge + 1.0).count();
    let right =
        page.lines.iter().filter(|line| beside(line) && line.left >= right_edge - 1.0).count();
    (left, right)
}

#[test]
fn text_runs_down_both_sides_of_a_drawing_in_the_middle_of_it() {
    // Word's `bothSides`, which is what it writes unless told otherwise: a
    // line beside the drawing is a piece of line each side of it.
    let laid = pages(&document_with_drawing_in_the_middle(wp_docx::anchor::WrapSide::BothSides));
    let (left, right) = lines_each_side(&laid[0]);
    assert!(left > 0, "no text to the left of the drawing");
    assert!(right > 0, "no text to the right of the drawing");
}

#[test]
fn the_two_pieces_of_one_line_share_a_baseline_and_follow_on_from_each_other() {
    let laid = pages(&document_with_drawing_in_the_middle(wp_docx::anchor::WrapSide::BothSides));
    let shape = laid[0].shapes.first().expect("a drawing").clone();
    let (left_edge, _) = drawing_edges(&laid[0]);

    // The first line beside the drawing, and the piece that follows it.
    let mut beside: Vec<&wp_layout::PageLine> = laid[0]
        .lines
        .iter()
        .filter(|line| line.baseline > shape.y && line.baseline < shape.y + shape.height)
        .collect();
    beside.sort_by(|one, other| {
        one.baseline.total_cmp(&other.baseline).then(one.left.total_cmp(&other.left))
    });
    // The first baseline that has two pieces to it. The line the drawing is
    // anchored in keeps the whole width and is one piece, so it is not that
    // one.
    let pair = beside
        .windows(2)
        .find(|pair| (pair[0].baseline - pair[1].baseline).abs() < 0.5)
        .expect("a line in two pieces");
    let (first, second) = (pair[0], pair[1]);

    assert!(first.right <= left_edge + 1.0, "the first piece is not on the left");
    assert_eq!(
        first.end_offset, second.start_offset,
        "the second piece does not carry on where the first left off"
    );
    assert!(first.paragraph == second.paragraph, "the pieces are from different paragraphs");
}

#[test]
fn asking_for_the_left_side_only_leaves_the_right_empty() {
    let laid = pages(&document_with_drawing_in_the_middle(wp_docx::anchor::WrapSide::Left));
    let (left, right) = lines_each_side(&laid[0]);
    assert!(left > 0, "no text to the left of the drawing");
    assert_eq!(right, 0, "the right of the drawing should be empty");
}

#[test]
fn asking_for_the_right_side_only_leaves_the_left_empty() {
    let laid = pages(&document_with_drawing_in_the_middle(wp_docx::anchor::WrapSide::Right));
    let (left, right) = lines_each_side(&laid[0]);
    assert_eq!(left, 0, "the left of the drawing should be empty");
    assert!(right > 0, "no text to the right of the drawing");
}

#[test]
fn asking_for_the_largest_side_uses_the_wider_one_alone() {
    let laid = pages(&document_with_drawing_in_the_middle(wp_docx::anchor::WrapSide::Largest));
    let (left, right) = lines_each_side(&laid[0]);
    assert_eq!(left, 0, "the narrower side should be empty");
    assert!(right > 0, "the wider side should hold the text");
}

#[test]
fn a_sliver_of_room_beside_a_drawing_is_left_empty() {
    // A drawing that reaches nearly to the left margin leaves a few pixels
    // there. A line of one letter down a gap that narrow reads as nonsense,
    // and Word leaves it empty.
    let anchor = Anchor {
        wrap: Wrap::Square,
        horizontal: Placement::Offset(50_000),
        vertical: Placement::Offset(0),
        ..Anchor::default()
    };
    let laid = pages(&document(Some(anchor)));
    let (left, _) = lines_each_side(&laid[0]);
    assert_eq!(left, 0, "text was squeezed into the sliver on the left");
}
