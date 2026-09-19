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

/// The same document with its first chart part rewritten by a function of
/// its markup.
fn with_chart_part_rewritten(document: &Document, rewrite: impl Fn(&str) -> String) -> Document {
    let bytes = document.save().expect("saving");
    let mut package = wp_opc::Package::open(&bytes).expect("the package");
    let xml = package.xml_part("word/charts/chart1.xml").expect("the part").expect("text");
    let rewritten = rewrite(&xml);
    assert_ne!(rewritten, xml, "the rewriting changed nothing");
    package.set_part("word/charts/chart1.xml", rewritten.into_bytes());
    Document::open(&package.save().expect("saving the package")).expect("reopening")
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
    assert_eq!(chart.series[0].values, vec![10.0, 20.0, 5.0]);
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
        assert_eq!(read.series[0].values, chart.series[0].values, "{}", kind.label());
    }
}

#[test]
fn the_workbook_behind_the_chart_is_in_the_package_and_the_chart_points_at_it() {
    let document = with_chart(&sample());
    let workbook = "word/embeddings/Microsoft_Excel_Worksheet1.xlsx";
    assert!(document.package().part(workbook).is_some(), "the workbook is missing");
    assert_eq!(
        document.package().content_type(workbook),
        Some(wp_docx::workbook::WORKBOOK_CONTENT_TYPE)
    );

    // The chart part points at the workbook, and says so in its own markup.
    let relationships =
        document.package().relationships("word/charts/chart1.xml").expect("relationships");
    let found = relationships
        .single_by_type(wp_docx::workbook::PACKAGE_RELATIONSHIP)
        .expect("the package relationship");
    assert_eq!(found.resolved_target("word/charts/chart1.xml").unwrap().unwrap(), workbook);
    let xml = document.package().xml_part("word/charts/chart1.xml").unwrap().unwrap();
    assert!(xml.contains("<c:externalData r:id=\""), "the chart does not name its workbook");

    // And the workbook holds the numbers, laid out as Word lays them out.
    let sheet = wp_docx::workbook::read_first_sheet(document.package().part(workbook).unwrap())
        .expect("a sheet");
    let values: Vec<f64> = sheet
        .range("Sheet1!$B$2:$B$4")
        .iter()
        .map(|cell| cell.expect("a cell").number().expect("a number"))
        .collect();
    assert_eq!(values, vec![10.0, 20.0, 5.0]);
    assert_eq!(sheet.range("Sheet1!$A$2")[0].unwrap().text(), "North");
}

#[test]
fn numbers_a_chart_left_to_its_workbook_are_read_from_it() {
    // The caches taken out of the chart part, leaving only the references.
    let document = with_chart_part_rewritten(&with_chart(&sample()), |xml| {
        let mut stripped = xml.to_owned();
        while let Some(start) = stripped.find("<c:pt idx=") {
            let end = stripped[start..].find("</c:pt>").expect("a point ends") + start + 7;
            stripped.replace_range(start..end, "");
        }
        stripped
    });

    let id = reference_in(&document).expect("a chart reference");
    let chart = document.chart(&id).expect("the chart");
    assert_eq!(chart.series[0].values, vec![10.0, 20.0, 5.0]);
    assert_eq!(chart.categories, vec!["North", "South", "East"]);
    assert_eq!(chart.series[0].name, "Series 1");
}

#[test]
fn a_colour_the_chart_names_from_the_theme_is_the_documents() {
    let document = with_chart_part_rewritten(&with_chart(&sample()), |xml| {
        xml.replace(
            "</c:tx>",
            "</c:tx><c:spPr><a:solidFill><a:schemeClr val=\"accent2\"/></a:solidFill></c:spPr>",
        )
    });

    let id = reference_in(&document).expect("a chart reference");
    let chart = document.chart(&id).expect("the chart");
    let accent = document.theme().color(wp_docx::theme::Slot::Accent2);
    assert_eq!(chart.series[0].fill, Some(accent));
}

#[test]
fn every_grouping_and_the_rest_of_a_chart_survive_the_file() {
    use wp_docx::chart::{Axis, DataTable, Grouping, Labels, Legend, LegendPosition};

    let mut chart = sample();
    chart.grouping = Grouping::PercentStacked;
    chart.legend = Some(Legend::at(LegendPosition::Top));
    chart.labels = Labels { value: true, category: true, ..Labels::default() };
    chart.data_table = Some(DataTable::default());
    chart.value_axis =
        Axis { max: Some(100.0), number_format: Some("0%".to_owned()), ..Axis::default() };
    let document = with_chart(&chart);
    let id = reference_in(&document).expect("a chart reference");
    let read = document.chart(&id).expect("the chart");
    assert_eq!(read.grouping, Grouping::PercentStacked);
    assert_eq!(read.legend, chart.legend);
    assert_eq!(read.labels, chart.labels);
    assert_eq!(read.data_table, chart.data_table);
    assert_eq!(read.value_axis, chart.value_axis);
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
