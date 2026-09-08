//! Charts in a document: the part, the relationship and the drawing.

use wp_docx::chart::{Chart, Kind};
use wp_docx::model::{Block, Body, Paragraph, RunContent};
use wp_docx::{Document, TextPosition, EMU_PER_INCH};

fn document() -> Document {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("Before after")));

    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    let mut document = Document::open(&bytes).expect("reopening");
    document.set_caret(TextPosition::new(0, 7));
    document
}

fn round_trip(document: &Document) -> Document {
    let bytes = document.save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

fn sample() -> Chart {
    Chart::parse(Kind::Column, "Sales", "North=10; South=20; East=5")
}

fn with_chart(chart: &Chart) -> Document {
    let mut document = document();
    assert!(document
        .insert_chart(chart, EMU_PER_INCH * 4, EMU_PER_INCH * 3)
        .expect("inserting the chart"));
    round_trip(&document)
}

/// The chart reference in the first paragraph, if there is one.
fn reference_in(document: &Document) -> Option<String> {
    let Block::Paragraph(paragraph) = &document.body().blocks[0] else { return None };
    paragraph.runs.iter().find_map(|run| {
        run.content.iter().find_map(|piece| match piece {
            RunContent::Chart(reference) => Some(reference.relationship.clone()),
            _ => None,
        })
    })
}

#[test]
fn a_document_has_no_charts_to_begin_with() {
    assert_eq!(reference_in(&document()), None);
}

#[test]
fn a_chart_becomes_a_part_of_the_package() {
    let document = with_chart(&sample());
    assert!(
        document.package().part("word/charts/chart1.xml").is_some(),
        "the chart part is missing"
    );
}

#[test]
fn the_drawing_points_at_the_chart_through_a_relationship() {
    let document = with_chart(&sample());
    let id = reference_in(&document).expect("a chart reference");
    assert_eq!(
        document.relationship_target(&id).as_deref(),
        Some("word/charts/chart1.xml"),
        "the relationship points somewhere else"
    );
}

#[test]
fn the_numbers_read_back_out_of_the_part() {
    let document = with_chart(&sample());
    let id = reference_in(&document).expect("a chart reference");
    let chart = document.chart(&id).expect("the chart");

    assert_eq!(chart.kind, Kind::Column);
    assert_eq!(chart.title, "Sales");
    assert_eq!(chart.categories, vec!["North", "South", "East"]);
    assert_eq!(chart.values, vec![10.0, 20.0, 5.0]);
}

#[test]
fn every_kind_of_chart_survives_being_saved_and_reopened() {
    for kind in Kind::ALL {
        let mut chart = sample();
        chart.kind = *kind;
        let document = with_chart(&chart);
        let id = reference_in(&document).expect("a chart reference");
        let read = document.chart(&id).expect("the chart");
        assert_eq!(read.kind, *kind, "{}", kind.label());
        assert_eq!(read.values, chart.values, "{}", kind.label());
    }
}

#[test]
fn a_chart_with_no_numbers_is_not_put_in() {
    let mut document = document();
    let empty = Chart::parse(Kind::Column, "", "");
    assert!(!document.insert_chart(&empty, 100, 100).expect("no error"));
    assert!(document.package().part("word/charts/chart1.xml").is_none());
}

#[test]
fn a_chart_stands_in_the_text_as_one_character() {
    let document = with_chart(&sample());
    let text = document.paragraph_text(0).expect("the paragraph");
    assert_eq!(text.chars().count(), "Before after".chars().count() + 1, "got {text:?}");
}

#[test]
fn a_chart_does_not_disturb_the_words_round_it() {
    let document = with_chart(&sample());
    let text = document.plain_text();
    assert!(text.starts_with("Before "), "{text:?}");
    assert!(text.ends_with("after"), "{text:?}");
}

#[test]
fn two_charts_get_two_parts() {
    let mut document = document();
    document.insert_chart(&sample(), 100, 100).expect("the first");
    document.insert_chart(&sample(), 100, 100).expect("the second");

    let reopened = round_trip(&document);
    assert!(reopened.package().part("word/charts/chart1.xml").is_some());
    assert!(reopened.package().part("word/charts/chart2.xml").is_some());
}

#[test]
fn the_drawing_carries_the_size_it_was_given() {
    let document = with_chart(&sample());
    let Block::Paragraph(paragraph) = &document.body().blocks[0] else { panic!("a paragraph") };
    let reference = paragraph
        .runs
        .iter()
        .find_map(|run| {
            run.content.iter().find_map(|piece| match piece {
                RunContent::Chart(reference) => Some(reference.clone()),
                _ => None,
            })
        })
        .expect("a chart");

    assert_eq!(reference.width_emu, EMU_PER_INCH * 4);
    assert_eq!(reference.height_emu, EMU_PER_INCH * 3);
    assert!((reference.width_points() - 288.0).abs() < 0.01, "{}", reference.width_points());
}

#[test]
fn putting_a_chart_in_can_be_undone() {
    let mut document = document();
    document.insert_chart(&sample(), 100, 100).expect("inserting");
    assert!(document.undo());
    assert_eq!(reference_in(&document), None);
}
