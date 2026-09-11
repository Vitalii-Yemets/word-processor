//! Drawings turned through an angle: the geometry, the words inside a shape
//! and the pixels of a picture all going round together.

use wp_docx::floating::Turned;
use wp_docx::model::{Block, Body, Paragraph, Run, RunContent};
use wp_docx::shapes::Shape;
use wp_docx::{Document, TextPosition};
use wp_layout::{FontLibrary, LayoutEngine, Page, PageMetrics, Renderer};
use wp_raster::{Canvas, Color};

fn library() -> &'static FontLibrary {
    Box::leak(Box::new(FontLibrary::scan_system()))
}

/// A document of one paragraph with a shape in the line of it, turned as asked.
fn document(turned: Turned) -> Document {
    let shape = Shape {
        name: "Box".to_owned(),
        fill: Some("4472C4".to_owned()),
        text: vec![Paragraph::text("Words")],
        rotation: turned.rotation,
        flipped_across: turned.flipped_across,
        flipped_down: turned.flipped_down,
        ..Shape::preset("rect", 144.0, 72.0)
    };
    let runs = vec![
        Run {
            properties: wp_docx::model::RunProperties::default(),
            content: vec![RunContent::Shape(shape)],
            field: None,
            revision: None,
            format_change: None,
        },
        Run::text(" after"),
    ];

    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::from_runs(runs)));
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

fn pages(document: &Document) -> Vec<Page> {
    let mut engine = LayoutEngine::new(library());
    engine.layout_document_with(document, PageMetrics::from_document(document))
}

/// The one shape of the first page.
fn shape_of(pages: &[Page]) -> &wp_layout::PlacedShape {
    pages.first().expect("a page").shapes.first().expect("a shape on the page")
}

#[test]
fn a_shape_that_was_never_turned_is_laid_out_straight() {
    let pages = pages(&document(Turned::default()));
    let shape = shape_of(&pages);
    assert_eq!(shape.turn, 0.0);
    assert!(!shape.flipped_across);
    assert!(!shape.flipped_down);
}

#[test]
fn the_angle_reaches_the_page_as_radians() {
    let quarter = Turned { rotation: Turned::WHOLE / 4, ..Turned::default() };
    let pages = pages(&document(quarter));
    let shape = shape_of(&pages);
    assert!(
        (shape.turn - core::f32::consts::FRAC_PI_2).abs() < 0.001,
        "a quarter of a turn came out as {} radians",
        shape.turn
    );
}

#[test]
fn mirroring_reaches_the_page_too() {
    let mirrored = Turned { flipped_across: true, flipped_down: true, ..Turned::default() };
    let pages = pages(&document(mirrored));
    let shape = shape_of(&pages);
    assert!(shape.flipped_across);
    assert!(shape.flipped_down);
}

#[test]
fn a_turned_shape_takes_up_the_same_room_on_the_line() {
    // Word turns the drawing inside the box the text flows round, and does not
    // move the words to make way for the corners.
    let straight = pages(&document(Turned::default()));
    let turned = pages(&document(Turned { rotation: Turned::WHOLE / 8, ..Turned::default() }));
    let (one, other) = (shape_of(&straight), shape_of(&turned));
    assert_eq!(
        (one.x, one.y, one.width, one.height),
        (other.x, other.y, other.width, other.height)
    );
}

/// Draws the first page onto white paper.
fn drawn(document: &Document) -> Canvas {
    let pages = pages(document);
    let page = pages.first().expect("a page");
    let mut renderer = Renderer::new(library());
    renderer.render(page, Color::WHITE)
}

/// How wide and how tall the shape's own fill is where it was drawn.
///
/// The fill and nothing else: the words on the page are drawn in black, and a
/// box round everything inked would be measuring them as well as the shape.
fn filled(canvas: &Canvas) -> (usize, usize) {
    let blue = Color::from_hex("4472C4").expect("a colour");
    let (mut left, mut top) = (usize::MAX, usize::MAX);
    let (mut right, mut bottom) = (0usize, 0usize);
    for y in 0..canvas.height() {
        for x in 0..canvas.width() {
            if canvas.pixel(x, y) != blue {
                continue;
            }
            left = left.min(x);
            top = top.min(y);
            right = right.max(x);
            bottom = bottom.max(y);
        }
    }
    assert!(left <= right, "the shape was not drawn at all");
    (right - left, bottom - top)
}

#[test]
fn a_shape_turned_a_quarter_is_drawn_taller_than_it_is_wide() {
    // The proof that the angle is not merely stored but drawn: a box twice as
    // wide as it is tall comes out taller than it is wide.
    let (wide, tall) = filled(&drawn(&document(Turned::default())));
    assert!(wide > tall, "the shape is not the way round it was made: {wide} by {tall}");

    let (wide, tall) =
        filled(&drawn(&document(Turned { rotation: Turned::WHOLE / 4, ..Turned::default() })));
    assert!(tall > wide, "a quarter turn left it lying down: {wide} by {tall}");
}

#[test]
fn a_picture_is_turned_with_the_drawing_and_not_only_its_box() {
    // A picture is drawn as pixels rather than as a path, which is a different
    // road through the renderer and could quietly ignore the angle.
    let mut canvas = Canvas::filled(40, 30, Color::BLACK);
    canvas.fill_rect(0, 0, 40, 8, Color::rgb(0xFF, 0, 0));
    let bytes = wp_raster::encode_png(&canvas);

    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("Before")));
    let made = Document::create(&body).expect("a document").save().expect("saving");
    let mut document = Document::open(&made).expect("reopening");
    document.set_caret(TextPosition::new(0, 6));
    document.insert_picture(&bytes, "png", 914_400, 685_800).expect("a picture");

    let straight = pages(&document);
    let placed = straight.first().expect("a page").images.first().expect("a picture");
    assert_eq!(placed.turn, 0.0);

    let at = TextPosition::new(0, 6);
    assert!(document
        .set_drawing_turn_at(at, Turned { rotation: Turned::WHOLE / 4, ..Turned::default() }));
    let turned = pages(&document);
    let placed = turned.first().expect("a page").images.first().expect("a picture");
    assert!(
        (placed.turn - core::f32::consts::FRAC_PI_2).abs() < 0.001,
        "the picture was placed at {} radians",
        placed.turn
    );

    // And the pixels went round with it: the red band along the top of the
    // picture is down the left-hand side of it once it is turned clockwise a
    // quarter — the far side from where a picture drawn straight would put it.
    let red = Color::rgb(0xFF, 0, 0);
    let (mut left, mut right) = (usize::MAX, 0usize);
    let drawn = Renderer::new(library()).render(turned.first().expect("a page"), Color::WHITE);
    for y in 0..drawn.height() {
        for x in 0..drawn.width() {
            if drawn.pixel(x, y) == red {
                left = left.min(x);
                right = right.max(x);
            }
        }
    }
    assert!(left <= right, "the picture was not drawn");
    let band = right - left;
    assert!(
        band < 40,
        "the red band is still lying across the picture rather than down it: {band} wide"
    );
}
