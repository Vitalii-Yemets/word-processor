//! A connector fastened to two shapes goes where they are.
//!
//! The box a connector was saved with is only the answer from the last time
//! anybody worked out where it should be. What it is fastened to is the thing
//! the file really says, and these tests move a shape and look at where the
//! connector went.

use wp_docx::anchor::{Anchor, Placement, Wrap};
use wp_docx::joins::{Join, Joins};
use wp_docx::model::{Block, Body, Paragraph, Run, RunContent};
use wp_docx::shapes::Shape;
use wp_docx::Document;
use wp_layout::{FontLibrary, LayoutEngine, PageMetrics};

/// The fonts, read once per test.
fn library() -> &'static FontLibrary {
    Box::leak(Box::new(FontLibrary::scan_system()))
}

/// A floating shape of a given id, at a place on the page.
fn box_at(id: u32, name: &str, across: i64, down: i64) -> Shape {
    let mut shape = Shape::preset("rect", 72.0, 36.0).floating(Anchor {
        wrap: Wrap::None,
        horizontal: Placement::Offset(across),
        vertical: Placement::Offset(down),
        ..Anchor::default()
    });
    shape.id = id;
    shape.name = name.to_owned();
    shape
}

/// Two boxes and a connector fastened between them, with the second box
/// wherever it is asked for.
///
/// The connector is given a box of its own that is deliberately nowhere near
/// either shape: what is being tested is that where it was saved does not
/// matter once it is fastened to something.
fn document(second_across: i64, second_down: i64) -> Document {
    let mut connector = Shape::preset("bentConnector3", 36.0, 18.0).floating(Anchor {
        wrap: Wrap::None,
        horizontal: Placement::Offset(4_572_000),
        vertical: Placement::Offset(4_572_000),
        ..Anchor::default()
    });
    connector.id = 3;
    connector.name = "Connector".to_owned();
    connector.joins = Joins {
        // From the right-hand side of the first box to the left-hand side of
        // the second, which are the fourth and second connection points.
        start: Some(Join { shape: 1, site: 3 }),
        end: Some(Join { shape: 2, site: 1 }),
    };

    let shapes =
        [box_at(1, "First", 0, 0), box_at(2, "Second", second_across, second_down), connector];
    let runs: Vec<Run> = shapes
        .into_iter()
        .map(|shape| Run {
            properties: wp_docx::model::RunProperties::default(),
            content: vec![RunContent::Shape(Box::new(shape))],
            field: None,
            revision: None,
            format_change: None,
        })
        .collect();

    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::from_runs(runs)));
    body.blocks.push(Block::Paragraph(Paragraph::from_runs(vec![Run::text("Words.")])));
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

/// Where each drawing ended up on the first page, by the name it was given.
fn placed(document: &Document) -> Vec<(String, f32, f32, f32, f32)> {
    let mut engine = LayoutEngine::new(library());
    let pages = engine.layout_document_with(document, PageMetrics::default());
    pages[0]
        .shapes
        .iter()
        .map(|shape| (shape.name.clone(), shape.x, shape.y, shape.width, shape.height))
        .collect()
}

fn find(places: &[(String, f32, f32, f32, f32)], name: &str) -> (f32, f32, f32, f32) {
    places
        .iter()
        .find(|(it, ..)| it == name)
        .map(|(_, x, y, width, height)| (*x, *y, *width, *height))
        .unwrap_or_else(|| panic!("{name} was not placed"))
}

#[test]
fn a_connector_reaches_from_one_shape_to_the_other() {
    // The right-hand side of the first box to the left-hand side of the
    // second, whatever box the connector itself was saved with.
    let places = placed(&document(2_286_000, 914_400));
    let (first_x, first_y, first_width, first_height) = find(&places, "First");
    let (second_x, second_y, _, second_height) = find(&places, "Second");
    let (x, y, width, height) = find(&places, "Connector");

    let start = (first_x + first_width, first_y + first_height / 2.0);
    let end = (second_x, second_y + second_height / 2.0);
    assert!((x - start.0).abs() < 0.5, "it starts at {x} and the shape ends at {}", start.0);
    assert!((y - start.1).abs() < 0.5, "it starts at {y} and the shape is at {}", start.1);
    assert!(
        (x + width - end.0).abs() < 0.5,
        "it ends at {} and the shape is at {}",
        x + width,
        end.0
    );
    assert!(
        (y + height - end.1).abs() < 0.5,
        "it ends at {} and the shape is at {}",
        y + height,
        end.1
    );
}

#[test]
fn moving_a_shape_moves_the_connector_fastened_to_it() {
    // The whole point of a connector: the second box is put somewhere else and
    // the connector follows it without anybody telling it to.
    let near = placed(&document(1_828_800, 457_200));
    let far = placed(&document(3_200_400, 1_828_800));

    let (_, _, near_width, near_height) = find(&near, "Connector");
    let (_, _, far_width, far_height) = find(&far, "Connector");
    assert!(far_width > near_width + 10.0, "{far_width} against {near_width}");
    assert!(far_height > near_height + 10.0, "{far_height} against {near_height}");

    let (second_x, second_y, _, second_height) = find(&far, "Second");
    let (x, y, width, height) = find(&far, "Connector");
    assert!((x + width - second_x).abs() < 0.5, "it stopped following the shape");
    assert!((y + height - (second_y + second_height / 2.0)).abs() < 0.5);
}

#[test]
fn a_connector_fastened_to_nothing_stays_where_it_was_put() {
    // Only a connector that says what it is fastened to is moved. One drawn by
    // hand between two shapes is a line, and a line stays where it was drawn.
    let mut loose = Shape::preset("straightConnector1", 72.0, 36.0).floating(Anchor {
        wrap: Wrap::None,
        horizontal: Placement::Offset(914_400),
        vertical: Placement::Offset(914_400),
        ..Anchor::default()
    });
    loose.id = 9;
    loose.name = "Loose".to_owned();

    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::from_runs(vec![Run {
        properties: wp_docx::model::RunProperties::default(),
        content: vec![RunContent::Shape(Box::new(loose))],
        field: None,
        revision: None,
        format_change: None,
    }])));
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    let document = Document::open(&bytes).expect("reopening");

    let places = placed(&document);
    let (_, _, width, height) = find(&places, "Loose");
    assert!((width - 96.0).abs() < 1.0, "it is {width} wide and was made 72 points");
    assert!((height - 48.0).abs() < 1.0, "it is {height} tall and was made 36 points");
}
