//! Copying and pasting inside a document, with the formatting kept.

use wp_docx::clipboard::Formatting;
use wp_docx::model::{Block, Body, Paragraph, Run, RunContent, RunProperties};
use wp_docx::{Document, TextPosition};

/// Two paragraphs, the first of them "plain bold after" with "bold" in bold.
fn document() -> Document {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph {
        properties: wp_docx::model::ParagraphProperties::default(),
        runs: vec![
            Run::text("plain "),
            Run {
                properties: RunProperties { bold: Some(true), ..RunProperties::default() },
                content: vec![RunContent::Text("bold".to_owned())],
                field: None,
                revision: None,
                format_change: None,
            },
            Run::text(" after"),
        ],
    }));
    body.blocks.push(Block::Paragraph(Paragraph::text("second")));

    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

fn round_trip(document: &Document) -> Document {
    let bytes = document.save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

/// Selects a stretch of the first paragraph.
fn select(document: &mut Document, from: usize, to: usize) {
    document.set_caret(TextPosition::new(0, from));
    document.extend_selection_to(TextPosition::new(0, to));
}

#[test]
fn nothing_selected_copies_nothing() {
    assert!(document().copy_selection().is_empty());
}

#[test]
fn the_words_of_the_selection_are_what_is_copied() {
    let mut document = document();
    select(&mut document, 0, 5);
    let copied = document.copy_selection();
    assert_eq!(copied.len(), 1);
    assert_eq!(copied[0].plain_text(), "plain");
}

#[test]
fn a_selection_over_several_paragraphs_copies_each_of_them() {
    let mut document = document();
    document.set_caret(TextPosition::new(0, 6));
    document.extend_selection_to(TextPosition::new(1, 6));
    let copied = document.copy_selection();
    assert_eq!(copied.len(), 2, "{copied:?}");
    assert_eq!(copied[0].plain_text(), "bold after");
    assert_eq!(copied[1].plain_text(), "second");
}

#[test]
fn pasting_puts_the_words_where_the_caret_is() {
    let mut document = document();
    select(&mut document, 0, 5);
    let copied = document.copy_selection();

    document.set_caret(TextPosition::new(1, 6));
    assert!(document.paste_blocks(&copied));
    assert_eq!(round_trip(&document).plain_text(), "plain bold after\nsecondplain");
}

#[test]
fn what_was_bold_is_bold_where_it_lands() {
    let mut document = document();
    // Just the bold word.
    select(&mut document, 6, 10);
    let copied = document.copy_selection();

    document.set_caret(TextPosition::new(1, 6));
    document.paste_blocks(&copied);

    let mut reopened = round_trip(&document);
    // Inside what was pasted, which now ends the second paragraph.
    reopened.set_caret(TextPosition::new(1, 8));
    assert!(
        reopened.format_is_on(wp_docx::CharacterFormat::Bold),
        "the formatting was lost: {:?}",
        reopened.body().blocks[1]
    );
}

#[test]
fn plain_text_stays_plain_where_it_lands() {
    let mut document = document();
    select(&mut document, 0, 5);
    let copied = document.copy_selection();

    document.set_caret(TextPosition::new(1, 6));
    document.paste_blocks(&copied);

    let mut reopened = round_trip(&document);
    reopened.set_caret(TextPosition::new(1, 8));
    assert!(!reopened.format_is_on(wp_docx::CharacterFormat::Bold));
}

#[test]
fn pasting_several_paragraphs_makes_several_paragraphs() {
    let mut document = document();
    document.select_all();
    let copied = document.copy_selection();

    document.set_caret(TextPosition::new(1, 6));
    document.paste_blocks(&copied);

    let reopened = round_trip(&document);
    assert_eq!(reopened.paragraph_count(), 3, "{}", reopened.plain_text());
}

#[test]
fn pasting_over_a_selection_replaces_it() {
    let mut document = document();
    select(&mut document, 0, 5);
    let copied = document.copy_selection();

    // Over the whole of the second paragraph.
    document.set_caret(TextPosition::new(1, 0));
    document.extend_selection_to(TextPosition::new(1, 6));
    document.paste_blocks(&copied);

    assert_eq!(round_trip(&document).plain_text(), "plain bold after\nplain");
}

#[test]
fn pasting_nothing_changes_nothing() {
    let mut document = document();
    assert!(!document.paste_blocks(&[]));
}

#[test]
fn a_paste_is_one_thing_to_undo() {
    let mut document = document();
    document.select_all();
    let copied = document.copy_selection();

    document.set_caret(TextPosition::new(1, 6));
    document.paste_blocks(&copied);
    assert!(document.undo());
    assert_eq!(document.plain_text(), "plain bold after\nsecond");
}

#[test]
fn the_style_of_a_paragraph_comes_across_with_it() {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph {
        properties: wp_docx::model::ParagraphProperties {
            style: Some("Heading1".to_owned()),
            ..wp_docx::model::ParagraphProperties::default()
        },
        runs: vec![Run::text("A heading")],
    }));
    body.blocks.push(Block::Paragraph(Paragraph::text("body")));

    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    let mut document = Document::open(&bytes).expect("reopening");
    select(&mut document, 0, 9);
    let copied = document.copy_selection();

    let Block::Paragraph(first) = &copied[0] else { panic!("a paragraph") };
    assert_eq!(first.properties.style.as_deref(), Some("Heading1"));
}

// --- The paste options ------------------------------------------------------

/// A document whose first paragraph is a centred heading in twenty-eight point
/// red Georgia with a bold word in it, and whose second is ordinary text.
fn heading_and_body() -> Document {
    let large = |text: &str, bold: bool| Run {
        properties: RunProperties {
            bold: bold.then_some(true),
            font: Some("Georgia".to_owned()),
            size_half_points: Some(56),
            color: Some("FF0000".to_owned()),
            ..RunProperties::default()
        },
        content: vec![RunContent::Text(text.to_owned())],
        field: None,
        revision: None,
        format_change: None,
    };

    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph {
        properties: wp_docx::model::ParagraphProperties {
            style: Some("Heading1".to_owned()),
            alignment: Some(wp_docx::model::Alignment::Center),
            ..wp_docx::model::ParagraphProperties::default()
        },
        runs: vec![large("Large ", false), large("bold", true)],
    }));
    body.blocks.push(Block::Paragraph(Paragraph::text("ordinary")));

    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

/// Copies the whole of the heading.
fn copy_the_heading(document: &mut Document) -> Vec<Block> {
    select(document, 0, "Large bold".len());
    document.copy_selection()
}

/// One paragraph of a document, read back from what was written.
fn paragraph_of(document: &Document, index: usize) -> Paragraph {
    let body = document.body();
    let Some(Block::Paragraph(paragraph)) = body.blocks.get(index) else {
        panic!("no paragraph {index}");
    };
    paragraph.clone()
}

/// The run of a paragraph that holds a word.
fn run_holding(paragraph: &Paragraph, word: &str) -> Run {
    paragraph
        .runs
        .iter()
        .find(|run| run.plain_text().contains(word))
        .unwrap_or_else(|| panic!("no run holding {word:?} in {:?}", paragraph.plain_text()))
        .clone()
}

#[test]
fn keeping_the_source_formatting_keeps_the_size_and_the_colour() {
    let mut document = heading_and_body();
    let copied = copy_the_heading(&mut document);

    // Into the end of the ordinary paragraph.
    document.set_caret(TextPosition::new(1, "ordinary".len()));
    assert!(document.paste_blocks_as(&copied, Formatting::Source));

    let document = round_trip(&document);
    let paragraph = paragraph_of(&document, 1);
    assert_eq!(paragraph.plain_text(), "ordinaryLarge bold");

    let large = run_holding(&paragraph, "Large");
    assert_eq!(large.properties.size_half_points, Some(56), "the size did not come across");
    assert_eq!(large.properties.color.as_deref(), Some("FF0000"));
    assert_eq!(large.properties.font.as_deref(), Some("Georgia"));
}

#[test]
fn merging_the_formatting_keeps_the_emphasis_and_nothing_else() {
    let mut document = heading_and_body();
    let copied = copy_the_heading(&mut document);

    document.set_caret(TextPosition::new(1, "ordinary".len()));
    assert!(document.paste_blocks_as(&copied, Formatting::Merged));

    let document = round_trip(&document);
    let paragraph = paragraph_of(&document, 1);

    // The size, the colour and the font are the destination's now.
    let large = run_holding(&paragraph, "Large");
    assert_eq!(large.properties.size_half_points, None, "the size came with it");
    assert_eq!(large.properties.color, None, "the colour came with it");
    assert_eq!(large.properties.font, None, "the font came with it");

    // But a bold word is still bold: that is the whole point of merging rather
    // than pasting the words alone.
    let bold = run_holding(&paragraph, "bold");
    assert_eq!(bold.properties.bold, Some(true));
    assert_eq!(bold.properties.size_half_points, None);
}

#[test]
fn an_empty_paragraph_takes_the_shape_of_what_is_pasted_into_it() {
    // Word's rule: there is nothing there to disagree with the pasted
    // paragraph, so the pasted paragraph's own shape wins.
    let mut document = heading_and_body();
    let copied = copy_the_heading(&mut document);

    document.set_caret(TextPosition::new(1, 0));
    document.extend_selection_to(TextPosition::new(1, "ordinary".len()));
    document.delete_selection();
    assert!(document.paste_blocks_as(&copied, Formatting::Source));

    let document = round_trip(&document);
    let paragraph = paragraph_of(&document, 1);
    assert_eq!(paragraph.properties.style.as_deref(), Some("Heading1"));
    assert_eq!(paragraph.properties.alignment, Some(wp_docx::model::Alignment::Center));
}

#[test]
fn a_paragraph_with_words_in_it_keeps_its_own_shape() {
    let mut document = heading_and_body();
    let copied = copy_the_heading(&mut document);

    document.set_caret(TextPosition::new(1, "ordinary".len()));
    assert!(document.paste_blocks_as(&copied, Formatting::Source));

    let document = round_trip(&document);
    let paragraph = paragraph_of(&document, 1);
    assert_eq!(paragraph.properties.style, None, "the paragraph became a heading");
    assert_eq!(paragraph.properties.alignment, None, "the paragraph was centred");
}

#[test]
fn merging_never_takes_the_shape_of_what_is_pasted() {
    let mut document = heading_and_body();
    let copied = copy_the_heading(&mut document);

    // Even into an empty paragraph, which is where Keep Source would.
    document.set_caret(TextPosition::new(1, 0));
    document.extend_selection_to(TextPosition::new(1, "ordinary".len()));
    document.delete_selection();
    assert!(document.paste_blocks_as(&copied, Formatting::Merged));

    let document = round_trip(&document);
    assert_eq!(paragraph_of(&document, 1).properties.style, None, "merging made it a heading");
}

#[test]
fn a_paragraph_made_by_the_paste_carries_the_shape_it_was_copied_with() {
    // Two paragraphs copied and pasted at the end of the document: the second
    // of them was made by this paste, so it is the source's shape throughout.
    let mut document = heading_and_body();
    document.select_all();
    let copied = document.copy_selection();
    assert_eq!(copied.len(), 2);

    document.set_caret(TextPosition::new(1, "ordinary".len()));
    assert!(document.paste_blocks_as(&copied, Formatting::Source));

    let document = round_trip(&document);
    assert_eq!(paragraph_of(&document, 1).plain_text(), "ordinaryLarge bold");
    // The paragraph the paste made carries the second copied paragraph's
    // shape, which is no style at all.
    assert_eq!(paragraph_of(&document, 2).plain_text(), "ordinary");
    assert_eq!(paragraph_of(&document, 2).properties.style, None);
}

#[test]
fn either_way_of_pasting_is_one_thing_to_undo() {
    for formatting in [Formatting::Source, Formatting::Merged] {
        let mut document = heading_and_body();
        let before = document.plain_text();
        let copied = copy_the_heading(&mut document);

        document.set_caret(TextPosition::new(1, "ordinary".len()));
        document.paste_blocks_as(&copied, formatting);
        assert!(document.undo(), "{formatting:?} could not be taken back");
        assert_eq!(document.plain_text(), before, "{formatting:?} left something behind");
    }
}

// --- What a copy carries ----------------------------------------------------
//
// A copy of a drawing is a copy of the element it is written as and of every
// part that element points at, so that a paste can put it down again in the
// same document or in another one. Each kind is copied out of the middle of
// "Before after", pasted, saved and opened again, and then looked for by the
// readers that find it.

/// A picture's bytes. Nothing here decodes them, so any bytes will do, and
/// two different ones make two pictures that can be told apart.
const PICTURE: &[u8] = b"\x89PNG\r\n\x1a\n the first picture";
const OTHER_PICTURE: &[u8] = b"\x89PNG\r\n\x1a\n the second picture";

/// A document of one paragraph of words, opened as a file would be.
fn words(text: &str) -> Document {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text(text)));
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

/// "Before after", with something put in between the two words by `put`, so
/// that it is the eighth character of the paragraph.
fn holding(put: impl FnOnce(&mut Document)) -> Document {
    let mut document = words("Before after");
    document.set_caret(TextPosition::new(0, 7));
    put(&mut document);
    let document = round_trip(&document);
    assert_eq!(
        document.paragraph_text(0).as_deref(),
        Some("Before \u{1}after"),
        "the object is not where the tests expect it"
    );
    document
}

fn with_picture(document: &mut Document) {
    assert!(document.insert_picture(PICTURE, "png", 914_400, 914_400).expect("the picture"));
}

fn with_chart(document: &mut Document) {
    let chart = wp_docx::chart::Chart::parse(
        wp_docx::chart::Kind::Column,
        "Sales",
        "North=10; South=20; East=5",
    );
    assert!(document.insert_chart(&chart, 914_400 * 4, 914_400 * 3).expect("the chart"));
}

fn with_ink(document: &mut Document) {
    let pen = |points: Vec<(i64, i64)>| wp_docx::ink::Stroke {
        colour: "0070C0".to_owned(),
        width_emu: 18_000,
        transparency: 0,
        flat: false,
        points,
        pressure: Vec::new(),
    };
    let ink = wp_docx::ink::Ink {
        strokes: vec![
            pen(vec![(0, 180_000), (90_000, 270_000)]),
            pen(vec![(90_000, 270_000), (270_000, 0)]),
        ],
    };
    assert!(document.insert_ink(&ink).expect("the ink"));
}

fn with_diagram(document: &mut Document) {
    let items = vec!["Plan".to_owned(), "Build".to_owned(), "Ship".to_owned()];
    assert!(document
        .insert_diagram(wp_docx::diagram::Arrangement::Process, &items, 914_400 * 5)
        .expect("the diagram"));
}

/// A group of a shape and a picture, so that what the group holds points at
/// a part too.
fn with_group(document: &mut Document) {
    use wp_docx::anchor::{Anchor, Placement, Wrap};
    use wp_docx::group::Rect;

    let shape = wp_docx::shapes::Shape {
        name: "Square".to_owned(),
        width_emu: 914_400,
        height_emu: 914_400,
        fill: wp_docx::fills::Fill::solid("4472C4"),
        anchor: Some(Anchor {
            wrap: Wrap::None,
            horizontal: Placement::Offset(0),
            vertical: Placement::Offset(0),
            ..Anchor::default()
        }),
        ..wp_docx::shapes::Shape::default()
    };
    assert!(document.insert_shape(&shape), "the shape went nowhere");
    document.set_caret(TextPosition::new(0, 8));
    with_picture(document);
    let square = Rect { x: 0, y: 0, width: 914_400, height: 914_400 };
    let beside = Rect { x: 914_400, y: 0, width: 914_400, height: 914_400 };
    document
        .group_drawings(&[(TextPosition::new(0, 7), square), (TextPosition::new(0, 8), beside)])
        .expect("nothing was grouped");
}

fn with_equation(document: &mut Document) {
    assert!(document.insert_equation(&wp_docx::math::parse("(a+b)/2")));
}

/// How many of each kind of object a document holds, every one of them
/// checked to reach what it points at.
#[derive(Debug, Default, PartialEq, Eq)]
struct Found {
    pictures: usize,
    charts: usize,
    ink: usize,
    diagrams: usize,
    groups: usize,
    equations: usize,
}

fn found_in(document: &Document) -> Found {
    let mut found = Found::default();
    let body = document.body();
    for paragraph in body.paragraphs() {
        for run in &paragraph.runs {
            for piece in &run.content {
                match piece {
                    RunContent::Picture(picture) => {
                        assert!(
                            document.embedded_part(&picture.relationship).is_some(),
                            "a picture points at nothing: {}",
                            picture.relationship
                        );
                        found.pictures += 1;
                    }
                    RunContent::Chart(chart) => {
                        assert!(
                            document.chart(&chart.relationship).is_some(),
                            "a chart points at nothing: {}",
                            chart.relationship
                        );
                        found.charts += 1;
                    }
                    RunContent::Ink(ink) => {
                        assert!(
                            document.ink(&ink.relationship).is_some_and(|ink| !ink.is_empty()),
                            "ink points at nothing: {}",
                            ink.relationship
                        );
                        found.ink += 1;
                    }
                    RunContent::Diagram(diagram) => {
                        let read = document.diagram(diagram);
                        assert!(
                            read.is_some_and(|diagram| diagram.text().len() == 3),
                            "a diagram points at nothing: {diagram:?}"
                        );
                        found.diagrams += 1;
                    }
                    RunContent::Group(group) => {
                        let pictures: Vec<_> = group
                            .members
                            .iter()
                            .filter_map(|member| match &member.what {
                                wp_docx::group::Inside::Picture(picture) => Some(picture),
                                _ => None,
                            })
                            .collect();
                        assert_eq!(pictures.len(), 1, "the picture in the group is gone");
                        assert!(
                            document.embedded_part(&pictures[0].relationship).is_some(),
                            "the picture in a group points at nothing"
                        );
                        assert_eq!(group.members.len(), 2, "a member of the group is gone");
                        found.groups += 1;
                    }
                    RunContent::Math(math) => {
                        assert_eq!(math.plain_text(), "((a+b))/(2)");
                        found.equations += 1;
                    }
                    _ => {}
                }
            }
        }
    }
    found
}

/// Copies the object out of "Before after" and pastes it at the end of the
/// same paragraph, then saves and opens the document again.
fn pasted_within(mut document: Document) -> Document {
    select(&mut document, 7, 8);
    let copied = document.copy_selection();
    let end = document.paragraph_text(0).expect("a paragraph").len();
    document.set_caret(TextPosition::new(0, end));
    assert!(document.paste_blocks(&copied), "the paste said nothing was pasted");
    round_trip(&document)
}

/// Copies it out and pastes it into another document.
fn pasted_across(mut document: Document) -> Document {
    select(&mut document, 7, 8);
    let copied = document.copy_selection();
    let mut other = words("Elsewhere ");
    other.set_caret(TextPosition::new(0, "Elsewhere ".len()));
    assert!(other.paste_blocks(&copied), "the paste said nothing was pasted");
    round_trip(&other)
}

#[test]
fn a_picture_pasted_in_its_own_document_is_there_twice() {
    let pasted = pasted_within(holding(with_picture));
    assert_eq!(found_in(&pasted), Found { pictures: 2, ..Found::default() });
}

#[test]
fn a_picture_pasted_into_another_document_brings_its_part() {
    let pasted = pasted_across(holding(with_picture));
    assert_eq!(found_in(&pasted), Found { pictures: 1, ..Found::default() });
}

#[test]
fn a_chart_pasted_in_its_own_document_is_there_twice() {
    let pasted = pasted_within(holding(with_chart));
    assert_eq!(found_in(&pasted), Found { charts: 2, ..Found::default() });
}

#[test]
fn a_chart_pasted_into_another_document_brings_its_part() {
    let pasted = pasted_across(holding(with_chart));
    assert_eq!(found_in(&pasted), Found { charts: 1, ..Found::default() });
}

#[test]
fn ink_pasted_in_its_own_document_is_there_twice() {
    let pasted = pasted_within(holding(with_ink));
    assert_eq!(found_in(&pasted), Found { ink: 2, ..Found::default() });
}

#[test]
fn ink_pasted_into_another_document_brings_its_part() {
    let pasted = pasted_across(holding(with_ink));
    assert_eq!(found_in(&pasted), Found { ink: 1, ..Found::default() });
}

#[test]
fn a_diagram_pasted_in_its_own_document_is_there_twice() {
    let pasted = pasted_within(holding(with_diagram));
    assert_eq!(found_in(&pasted), Found { diagrams: 2, ..Found::default() });
}

#[test]
fn a_diagram_pasted_into_another_document_brings_its_parts() {
    let pasted = pasted_across(holding(with_diagram));
    assert_eq!(found_in(&pasted), Found { diagrams: 1, ..Found::default() });
}

#[test]
fn a_diagram_word_wrote_brings_the_drawing_its_document_keeps_for_it() {
    // Word keeps a diagram's drawing on the document's own relationships
    // and has the data model name it there, where this program keeps it on
    // the data model's. The same diagram, moved to Word's arrangement.
    use wp_docx::diagram::DRAWING_RELATIONSHIP;
    let data = "word/diagrams/data1.xml";
    let bytes = holding(with_diagram).save().expect("saving");
    let mut package = wp_opc::Package::open(&bytes).expect("the package");
    let mut own = package.relationships(data).expect("the data model's");
    let was = own.single_by_type(DRAWING_RELATIONSHIP).expect("its drawing").id.clone();
    own.remove(&was);
    package.set_relationships(&own).expect("written");
    let mut document_relationships =
        package.relationships("word/document.xml").expect("the document's");
    let now = document_relationships
        .add(DRAWING_RELATIONSHIP, "diagrams/drawing1.xml", wp_opc::TargetMode::Internal)
        .id
        .clone();
    package.set_relationships(&document_relationships).expect("written");
    let xml = package.xml_part(data).expect("the data model").expect("text");
    let renamed = xml.replace(&format!("relId=\"{was}\""), &format!("relId=\"{now}\""));
    assert_ne!(renamed, xml, "the data model did not name its drawing");
    package.set_part(data, renamed.into_bytes());
    let drawing = package.part("word/diagrams/drawing1.xml").expect("the drawing").to_vec();
    let document = Document::open(&package.save().expect("saving")).expect("reopening");

    let pasted = pasted_across(document);
    assert_eq!(found_in(&pasted), Found { diagrams: 1, ..Found::default() });

    // The pasted data model names a relationship of the document it is in,
    // and that relationship reaches the drawing.
    let bytes = pasted.save().expect("saving");
    let package = wp_opc::Package::open(&bytes).expect("the package");
    let relationships = package.relationships("word/document.xml").expect("the document's");
    let data = relationships
        .single_by_type(wp_docx::diagram::DATA_RELATIONSHIP)
        .and_then(|found| found.resolved_target("word/document.xml"))
        .and_then(Result::ok)
        .expect("the pasted data model");
    let xml = package.xml_part(&data).expect("the data model").expect("text");
    let named = xml.split("relId=\"").nth(1).and_then(|rest| rest.split('"').next());
    let reached = named
        .and_then(|id| relationships.by_id(id))
        .filter(|found| found.kind == DRAWING_RELATIONSHIP)
        .and_then(|found| found.resolved_target("word/document.xml"))
        .and_then(Result::ok)
        .expect("the data model names no drawing relationship of its document");
    assert_eq!(package.part(&reached), Some(drawing.as_slice()));
}

#[test]
fn a_group_pasted_in_its_own_document_is_there_twice() {
    let pasted = pasted_within(holding(with_group));
    assert_eq!(found_in(&pasted), Found { groups: 2, ..Found::default() });
}

#[test]
fn a_group_pasted_into_another_document_brings_what_it_holds() {
    let pasted = pasted_across(holding(with_group));
    assert_eq!(found_in(&pasted), Found { groups: 1, ..Found::default() });
}

#[test]
fn an_equation_pasted_in_its_own_document_is_there_twice() {
    let pasted = pasted_within(holding(with_equation));
    assert_eq!(found_in(&pasted), Found { equations: 2, ..Found::default() });
}

#[test]
fn an_equation_pasted_into_another_document_is_there() {
    let pasted = pasted_across(holding(with_equation));
    assert_eq!(found_in(&pasted), Found { equations: 1, ..Found::default() });
}

#[test]
fn a_picture_cut_and_pasted_is_still_there() {
    let mut document = holding(with_picture);
    select(&mut document, 7, 8);
    let copied = document.copy_selection();
    assert!(document.delete_selection());
    assert_eq!(document.paragraph_text(0).as_deref(), Some("Before after"));

    document.set_caret(TextPosition::new(0, "Before after".len()));
    assert!(document.paste_blocks(&copied), "the paste said nothing was pasted");
    let document = round_trip(&document);
    assert_eq!(document.paragraph_text(0).as_deref(), Some("Before after\u{1}"));
    assert_eq!(found_in(&document), Found { pictures: 1, ..Found::default() });
}

#[test]
fn a_picture_pasted_beside_another_keeps_both_apart() {
    // Both documents call their picture image1.png; the one pasted into
    // must keep its own, and the pasted one must come with its own bytes.
    let mut document = holding(with_picture);
    select(&mut document, 7, 8);
    let copied = document.copy_selection();

    let mut other = words("Elsewhere ");
    other.set_caret(TextPosition::new(0, 0));
    assert!(other.insert_picture(OTHER_PICTURE, "png", 914_400, 914_400).expect("a picture"));
    other.set_caret(TextPosition::new(0, other.paragraph_text(0).expect("text").len()));
    assert!(other.paste_blocks(&copied));
    let other = round_trip(&other);

    assert_eq!(found_in(&other), Found { pictures: 2, ..Found::default() });
    let bytes: Vec<Vec<u8>> = other
        .body()
        .paragraphs()
        .iter()
        .flat_map(|paragraph| &paragraph.runs)
        .flat_map(|run| &run.content)
        .filter_map(|piece| match piece {
            RunContent::Picture(picture) => other.embedded_part(&picture.relationship),
            _ => None,
        })
        .map(<[u8]>::to_vec)
        .collect();
    assert_eq!(bytes, vec![OTHER_PICTURE.to_vec(), PICTURE.to_vec()]);
}

#[test]
fn a_chart_pasted_beside_another_keeps_both_apart() {
    let mut document = holding(with_chart);
    select(&mut document, 7, 8);
    let copied = document.copy_selection();

    let mut other = words("Elsewhere ");
    other.set_caret(TextPosition::new(0, 0));
    let costs = wp_docx::chart::Chart::parse(wp_docx::chart::Kind::Bar, "Costs", "Rent=3; Wages=7");
    assert!(other.insert_chart(&costs, 914_400 * 4, 914_400 * 3).expect("a chart"));
    other.set_caret(TextPosition::new(0, other.paragraph_text(0).expect("text").len()));
    assert!(other.paste_blocks(&copied));
    let other = round_trip(&other);

    assert_eq!(found_in(&other), Found { charts: 2, ..Found::default() });
    let titles: Vec<String> = other
        .body()
        .paragraphs()
        .iter()
        .flat_map(|paragraph| &paragraph.runs)
        .flat_map(|run| &run.content)
        .filter_map(|piece| match piece {
            RunContent::Chart(chart) => other.chart(&chart.relationship),
            _ => None,
        })
        .map(|chart| chart.title)
        .collect();
    assert_eq!(titles, vec!["Costs".to_owned(), "Sales".to_owned()]);
}

#[test]
fn a_paste_of_nothing_that_can_be_put_down_says_nothing_was_pasted() {
    // A picture that names a relationship of no part anybody knows: there is
    // nothing to put down, and saying otherwise is the lie the review found.
    let mut document = words("Here");
    let orphan = Run {
        content: vec![RunContent::Picture(Box::new(wp_docx::model::Picture {
            relationship: "rId42".to_owned(),
            width_emu: 914_400,
            height_emu: 914_400,
            ..wp_docx::model::Picture::default()
        }))],
        ..Run::text("")
    };
    document.set_caret(TextPosition::new(0, 4));
    assert!(!document.paste_blocks(&[Block::Paragraph(Paragraph::from_runs(vec![orphan]))]));
    assert_eq!(round_trip(&document).paragraph_text(0).as_deref(), Some("Here"));
}

#[test]
fn a_field_in_a_copy_is_pasted_as_a_field() {
    // The paste used to write a field's runs as runs, and what came across
    // was its last answer as plain words.
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::from_runs(vec![
        Run::text("Page "),
        Run::field("PAGE", "7"),
    ])));
    body.blocks.push(Block::Paragraph(Paragraph::text("Here: ")));
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    let mut document = Document::open(&bytes).expect("reopening");
    select(&mut document, 0, 6);
    let copied = document.copy_selection();

    document.set_caret(TextPosition::new(1, "Here: ".len()));
    assert!(document.paste_blocks(&copied));
    let pasted = paragraph_of(&round_trip(&document), 1);
    assert_eq!(pasted.plain_text(), "Here: Page 7");
    assert_eq!(run_holding(&pasted, "7").field.as_deref(), Some("PAGE"));
}

// --- Links in a copy --------------------------------------------------------

const MANUAL: &str = "https://example.com/manual";

/// "See the manual." with "manual" a link to wherever `typed` says.
fn with_a_link(typed: &str) -> Document {
    let mut document = words("See the manual.");
    select(&mut document, 8, 14);
    assert!(document.add_hyperlink(typed, ""), "no link was made");
    let document = round_trip(&document);
    assert_eq!(links_in(&document).len(), 1);
    document
}

/// Every link in a document: what it shows, and where it goes.
fn links_in(document: &Document) -> Vec<(String, wp_docx::links::Destination)> {
    document.hyperlinks().into_iter().map(|link| (link.text, link.destination)).collect()
}

fn to_the_manual(text: &str) -> (String, wp_docx::links::Destination) {
    (text.to_owned(), wp_docx::links::Destination::Address(MANUAL.to_owned()))
}

#[test]
fn a_link_copied_and_pasted_in_its_own_document_is_a_link_to_the_same_place() {
    let mut document = with_a_link(MANUAL);
    select(&mut document, 4, 14);
    let copied = document.copy_selection();
    document.set_caret(TextPosition::new(0, "See the manual.".len()));
    assert!(document.paste_blocks(&copied));

    let document = round_trip(&document);
    assert_eq!(document.paragraph_text(0).as_deref(), Some("See the manual.the manual"));
    assert_eq!(links_in(&document), vec![to_the_manual("manual"), to_the_manual("manual")]);
}

#[test]
fn a_link_pasted_into_another_document_is_a_link_to_the_same_place() {
    let mut document = with_a_link(MANUAL);
    select(&mut document, 4, 14);
    let copied = document.copy_selection();
    let mut other = words("Elsewhere ");
    other.set_caret(TextPosition::new(0, "Elsewhere ".len()));
    assert!(other.paste_blocks(&copied));

    let other = round_trip(&other);
    assert_eq!(links_in(&other), vec![to_the_manual("manual")]);
}

#[test]
fn a_link_cut_in_two_by_the_selection_is_pasted_as_a_link() {
    // "the man": the selection ends in the middle of the link, and what of
    // the link it took is still a link.
    let mut document = with_a_link(MANUAL);
    select(&mut document, 4, 11);
    let copied = document.copy_selection();
    let mut other = words("Elsewhere ");
    other.set_caret(TextPosition::new(0, "Elsewhere ".len()));
    assert!(other.paste_blocks(&copied));

    let other = round_trip(&other);
    assert_eq!(other.paragraph_text(0).as_deref(), Some("Elsewhere the man"));
    assert_eq!(links_in(&other), vec![to_the_manual("man")]);
}

#[test]
fn a_link_to_a_place_in_the_document_is_pasted_as_one() {
    let mut document = with_a_link("#Intro");
    select(&mut document, 8, 14);
    let copied = document.copy_selection();
    let mut other = words("Elsewhere ");
    other.set_caret(TextPosition::new(0, "Elsewhere ".len()));
    assert!(other.paste_blocks(&copied));

    let other = round_trip(&other);
    let place = wp_docx::links::Destination::Place("Intro".to_owned());
    assert_eq!(links_in(&other), vec![("manual".to_owned(), place)]);
}

#[test]
fn a_link_pasted_into_a_link_goes_beside_it_and_not_inside() {
    // A link cannot hold a link: the one pasted into is cut in two round it.
    let mut document = with_a_link(MANUAL);
    select(&mut document, 8, 14);
    let copied = document.copy_selection();
    document.set_caret(TextPosition::new(0, 10));
    assert!(document.paste_blocks(&copied));

    let document = round_trip(&document);
    assert_eq!(document.paragraph_text(0).as_deref(), Some("See the mamanualnual."));
    assert_eq!(
        links_in(&document),
        vec![to_the_manual("ma"), to_the_manual("manual"), to_the_manual("nual")]
    );
    fn nested(element: &wp_xml::tree::Element, inside: bool) -> bool {
        let here = element.local_name() == "hyperlink";
        (here && inside) || element.child_elements().any(|child| nested(child, inside || here))
    }
    assert!(!nested(&document.tree().root, false), "a link was put inside a link");
}

// --- What a save leaves out -------------------------------------------------

/// The names of a saved file's parts that start with a folder's name.
fn parts_under(bytes: &[u8], folder: &str) -> Vec<String> {
    let package = wp_opc::Package::open(bytes).expect("the package");
    assert!(package.validate().is_empty(), "{:?}", package.validate());
    package
        .content_parts()
        .map(|entry| entry.name.clone())
        .filter(|name| name.starts_with(folder))
        .collect()
}

#[test]
fn a_chart_cut_and_pasted_leaves_one_chart_part_in_the_file() {
    let mut document = holding(with_chart);
    select(&mut document, 7, 8);
    let copied = document.copy_selection();
    assert!(document.delete_selection());
    document.set_caret(TextPosition::new(0, "Before after".len()));
    assert!(document.paste_blocks(&copied));

    let bytes = document.save().expect("saving");
    assert_eq!(parts_under(&bytes, "word/charts/").len(), 1, "the old chart is still in the file");
    assert_eq!(parts_under(&bytes, "word/embeddings/").len(), 1, "and its workbook");
    assert_eq!(found_in(&Document::open(&bytes).expect("reopening")).charts, 1);
}

#[test]
fn a_paste_taken_back_leaves_nothing_of_itself_in_the_file() {
    let mut document = holding(with_chart);
    // Changed first, so that what is saved is not the file as it was opened.
    document.set_caret(TextPosition::new(0, 0));
    document.type_text("So: ");
    select(&mut document, 11, 12);
    let copied = document.copy_selection();
    document.set_caret(TextPosition::new(0, "So: Before \u{1}after".len()));
    assert!(document.paste_blocks(&copied));
    assert!(document.undo(), "the paste could not be taken back");

    let bytes = document.save().expect("saving");
    assert_eq!(parts_under(&bytes, "word/charts/"), vec!["word/charts/chart1.xml".to_owned()]);
    assert_eq!(parts_under(&bytes, "word/embeddings/").len(), 1);
    let reopened = Document::open(&bytes).expect("reopening");
    assert_eq!(reopened.paragraph_text(0).as_deref(), Some("So: Before \u{1}after"));
    assert_eq!(found_in(&reopened).charts, 1);
}

#[test]
fn a_picture_cut_saved_and_taken_back_is_still_whole() {
    // The file leaves the picture out; the document open on the screen does
    // not, because undo may want it back, and a save after that has it.
    let mut document = holding(with_picture);
    select(&mut document, 7, 8);
    assert!(document.delete_selection());
    let bytes = document.save().expect("saving");
    assert!(parts_under(&bytes, "word/media/").is_empty(), "the cut picture is in the file");
    document.mark_saved().expect("marked");

    assert!(document.undo());
    let bytes = document.save().expect("saving");
    assert_eq!(parts_under(&bytes, "word/media/").len(), 1);
    assert_eq!(found_in(&Document::open(&bytes).expect("reopening")).pictures, 1);
}

/// The part [`with_everything`] adds that nothing reaches.
const STRAY: &str = "word/media/stray.png";

/// A document with a part of every kind something reaches, each reached the
/// way Word reaches it, and one part nothing reaches.
///
/// Reached from the document: a chart and the workbook the chart reaches,
/// ink, a diagram's four parts and its drawing, a header and the picture it
/// reaches, the footnotes and theirs, the comments with the people who wrote
/// them and Word's extension of them, custom XML and its datastore item, the
/// building blocks and their own parts, the font table and the font it
/// embeds, the macros and the data they reach — a part that is not XML and
/// has relationships all the same — and the settings, which reach a template
/// outside the package. Reached from the package itself: the thumbnail.
fn with_everything() -> Vec<u8> {
    use wp_docx::furniture::{Furniture, Preset};
    let mut document = words("Body");
    document.set_caret(TextPosition::new(0, 4));
    document.add_note(wp_docx::notes::Kind::Footnote, "Note").expect("a footnote");
    for put in [with_chart, with_ink, with_diagram] {
        let end = document.paragraph_text(0).expect("the paragraph").len();
        document.set_caret(TextPosition::new(0, end));
        put(&mut document);
    }
    select(&mut document, 0, 4);
    document.add_comment("A remark", "Ann", "2026-01-01T00:00:00Z").expect("a comment");

    document
        .set_furniture(Furniture::Header, Preset::Text, wp_docx::model::Alignment::Start, "Top")
        .expect("a header");
    let header = document.furniture_part(Furniture::Header).expect("the header's part");
    assert!(document.enter_part(&header));
    document.set_caret(TextPosition::new(0, 0));
    assert!(document.insert_picture(PICTURE, "png", 9, 9).expect("a picture"));
    assert!(document.leave_part());
    let notes = document.notes_part(wp_docx::notes::Kind::Footnote).expect("the notes' part");
    assert!(document.enter_part(&notes));
    let last = document.paragraph_count() - 1;
    document.set_caret(TextPosition::new(last, 0));
    assert!(document.insert_picture(OTHER_PICTURE, "png", 9, 9).expect("a picture"));
    assert!(document.leave_part());

    document.add_custom_xml("<data><name>Ann</name></data>").expect("custom XML");
    let mut sign_off = Body::default();
    sign_off.blocks.push(Block::Paragraph(Paragraph::text("Regards")));
    assert!(
        document.add_building_block(&wp_docx::blocks::BuildingBlock::named("Sign-off"), &sign_off)
    );
    assert!(document.attach_template("/templates/Letters.dotm"));
    assert!(document.set_kind(wp_docx::kinds::Kind::MacroEnabledDocument));

    // What the program does not write itself, put in by hand as Word writes
    // it: where it is reached from, by what kind, and what it is.
    let mut package =
        wp_opc::Package::open(&document.save().expect("saving")).expect("the package");
    let office = |kind: &str| format!("{OFFICE}/relationships/{kind}");
    let microsoft = |kind: &str| format!("http://schemas.microsoft.com/office/{kind}");
    let metadata = |kind: &str| {
        format!("http://schemas.openxmlformats.org/package/2006/relationships/metadata/{kind}")
    };
    let word = |kind: &str| {
        format!("application/vnd.openxmlformats-officedocument.wordprocessingml.{kind}+xml")
    };

    let thumbnail = (&*metadata("thumbnail"), "image/png", PICTURE.to_vec());
    reach(&mut package, "", "docProps/thumbnail.png", thumbnail);
    let properties = format!("<cp:coreProperties xmlns:cp=\"{CORE}\"/>").into_bytes();
    let core = "application/vnd.openxmlformats-package.core-properties+xml";
    reach(&mut package, "", "docProps/core.xml", (&metadata("core-properties"), core, properties));

    // The building blocks have styles of their own, reached from their part.
    let styles = format!("<w:styles xmlns:w=\"{W}\"/>").into_bytes();
    let styles = (&*office("styles"), &*word("styles"), styles);
    reach(&mut package, "word/glossary/document.xml", "styles.xml", styles);

    // The font table names the font it embeds.
    let embedded = "application/vnd.openxmlformats-officedocument.obfuscatedFont";
    let font = (&*office("font"), embedded, b"an obfuscated font".to_vec());
    let font = reach(&mut package, "word/fontTable.xml", "fonts/font1.odttf", font);
    let fonts = format!(
        "<w:fonts xmlns:w=\"{W}\" xmlns:r=\"{OFFICE}/relationships\"><w:font w:name=\"Embedded\">\
         <w:embedRegular r:id=\"{font}\" w:fontKey=\"{{00000000-0000-0000-0000-000000000000}}\"/>\
         </w:font></w:fonts>"
    );
    let fonts = (&*office("fontTable"), &*word("fontTable"), fonts.into_bytes());
    reach(&mut package, "word/document.xml", "fontTable.xml", fonts);

    // The macros are not XML, and reach the data that goes with them.
    let project = b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1 a project".to_vec();
    let kind = microsoft("2006/relationships/vbaProject");
    let project = (&*kind, "application/vnd.ms-office.vbaProject", project);
    reach(&mut package, "word/document.xml", "vbaProject.bin", project);
    let data = format!("<wne:vbaSuppData xmlns:wne=\"{WNE}\"/>").into_bytes();
    let kind = microsoft("2006/relationships/wordVbaData");
    let data = (&*kind, "application/vnd.ms-word.vbaData+xml", data);
    reach(&mut package, "word/vbaProject.bin", "vbaData.xml", data);

    // Who wrote the comments, and what Word adds to them.
    let people =
        format!("<w15:people xmlns:w15=\"{W15}\"><w15:person w15:author=\"Ann\"/></w15:people>");
    let kind = microsoft("2011/relationships/people");
    let people = (&*kind, &*word("people"), people.into_bytes());
    reach(&mut package, "word/document.xml", "people.xml", people);
    let extended = format!("<w15:commentsEx xmlns:w15=\"{W15}\"/>").into_bytes();
    let kind = microsoft("2011/relationships/commentsExtended");
    let extended = (&*kind, &*word("commentsExtended"), extended);
    reach(&mut package, "word/document.xml", "commentsExtended.xml", extended);

    package.add_part(STRAY, "image/png", OTHER_PICTURE.to_vec());
    package.save().expect("saving the package")
}

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const W15: &str = "http://schemas.microsoft.com/office/word/2012/wordml";
const WNE: &str = "http://schemas.microsoft.com/office/word/2006/wordml";
const OFFICE: &str = "http://schemas.openxmlformats.org/officeDocument/2006";
const CORE: &str = "http://schemas.openxmlformats.org/package/2006/metadata/core-properties";

/// Puts a part in a package where `target` leads from `source`, with a
/// relationship reaching it, and gives back that relationship's identifier.
/// The last argument is the kind of the relationship, the content type of the
/// part and its bytes.
fn reach(
    package: &mut wp_opc::Package,
    source: &str,
    target: &str,
    (kind, content_type, bytes): (&str, &str, Vec<u8>),
) -> String {
    let name = wp_opc::resolve_target(source, target).expect("a target inside the package");
    package.add_part(&name, content_type, bytes);
    let mut relationships = package.relationships(source).expect("relationships");
    let id = relationships.add(kind, target, wp_opc::TargetMode::Internal).id.clone();
    package.set_relationships(&relationships).expect("written");
    id
}

#[test]
fn a_save_keeps_everything_something_reaches_and_nothing_else() {
    let mut document = Document::open(&with_everything()).expect("opening");
    document.set_caret(TextPosition::new(0, 4));
    document.type_text(" text");
    let bytes = document.save().expect("saving");

    let package = wp_opc::Package::open(&bytes).expect("the package");
    assert!(package.part(STRAY).is_none(), "what nothing reaches was kept");
    assert!(package.part("docProps/thumbnail.png").is_some(), "the thumbnail was lost");
    let mut reopened = Document::open(&bytes).expect("reopening");
    assert_eq!(reopened.custom_xml_parts().len(), 1, "the custom XML was lost");
    assert!(reopened.building_block_body("Sign-off").is_some(), "the building block was lost");
    let header =
        reopened.furniture_part(wp_docx::furniture::Furniture::Header).expect("the header");
    assert!(reopened.enter_part(&header));
    assert_eq!(found_in(&reopened).pictures, 1, "the header's picture was lost");
}

#[test]
fn every_part_of_every_kind_something_reaches_is_in_the_saved_file() {
    // A part is kept by being reached, and each kind is reached its own way:
    // from the document, from another part, from a part that is not XML,
    // from the package itself, or by a relationship nothing names.
    let names = |bytes: &[u8]| -> Vec<String> {
        wp_opc::Package::open(bytes)
            .expect("the package")
            .entries()
            .iter()
            .filter(|entry| !entry.is_directory())
            .map(|entry| entry.name.clone())
            .collect()
    };
    let bytes = with_everything();
    let before = names(&bytes);
    for kind in [
        "word/charts/chart",
        "word/embeddings/",
        "word/ink/",
        "word/diagrams/data",
        "word/diagrams/layout",
        "word/diagrams/quickStyle",
        "word/diagrams/colors",
        "word/diagrams/drawing",
        "word/_rels/header1.xml.rels",
        "word/_rels/footnotes.xml.rels",
        "word/comments.xml",
        "word/people.xml",
        "word/commentsExtended.xml",
        "customXml/item1.xml",
        "customXml/_rels/item1.xml.rels",
        "customXml/itemProps1.xml",
        "word/glossary/document.xml",
        "word/glossary/_rels/document.xml.rels",
        "word/glossary/styles.xml",
        "word/fontTable.xml",
        "word/fonts/font1.odttf",
        "word/vbaProject.bin",
        "word/_rels/vbaProject.bin.rels",
        "word/vbaData.xml",
        "word/_rels/settings.xml.rels",
        "docProps/thumbnail.png",
        "docProps/core.xml",
    ] {
        assert!(before.iter().any(|name| name.starts_with(kind)), "nothing to keep: {kind}");
    }

    let mut document = Document::open(&bytes).expect("opening");
    document.set_caret(TextPosition::new(0, 4));
    document.type_text(" text");
    let saved = document.save().expect("saving");
    let after = names(&saved);
    let lost: Vec<&String> =
        before.iter().filter(|name| *name != STRAY && !after.contains(name)).collect();
    assert!(lost.is_empty(), "lost at the save: {lost:?}");
    assert!(!after.iter().any(|name| name == STRAY), "what nothing reaches was kept");
    assert_eq!(after.len(), before.len() - 1, "{after:?}");

    let reopened = Document::open(&saved).expect("reopening");
    assert_eq!(found_in(&reopened), Found { charts: 1, ink: 1, diagrams: 1, ..Found::default() });
    assert!(reopened.has_macros(), "the macros were lost");
    assert!(reopened.attached_template().is_some(), "the template was lost");
}

#[test]
fn ink_cut_is_left_out_of_the_file_and_custom_xml_reached_the_same_way_is_not() {
    // Ink and custom XML are reached by one kind of relationship. The ink's
    // is named from the text, and ink cut leaves it named by nothing; the
    // custom XML's is named by nothing ever, and the part is kept for it.
    let mut document = holding(with_ink);
    document.add_custom_xml("<data><name>Ann</name></data>").expect("custom XML");
    select(&mut document, 7, 8);
    assert!(document.delete_selection());

    let bytes = document.save().expect("saving");
    assert!(parts_under(&bytes, "word/ink/").is_empty(), "the cut ink is in the file");
    assert_eq!(parts_under(&bytes, "customXml/").len(), 2, "the custom XML or its item is lost");
    assert_eq!(Document::open(&bytes).expect("reopening").custom_xml_parts().len(), 1);
}

#[test]
fn a_document_nobody_changed_is_saved_as_it_was_opened() {
    // Even the part nothing reaches: a file passed through untouched is the
    // same file.
    let bytes = with_everything();
    assert_eq!(Document::open(&bytes).expect("opening").save().expect("saving"), bytes);
}

// --- Notes in a copy --------------------------------------------------------

/// "Before after" with a footnote after "Before", saying "The note".
fn with_a_footnote() -> Document {
    let mut document = words("Before after");
    document.set_caret(TextPosition::new(0, 6));
    document.add_note(wp_docx::notes::Kind::Footnote, "The note").expect("the note");
    let document = round_trip(&document);
    assert_eq!(document.paragraph_text(0).as_deref(), Some("Before\u{2} after"));
    document
}

/// The notes of a document, by what they say, each checked to have its mark
/// in the text.
fn footnotes_in(document: &Document) -> Vec<String> {
    document
        .notes(wp_docx::notes::Kind::Footnote)
        .into_iter()
        .map(|note| {
            assert!(note.mark.is_some(), "note {} has no mark in the text", note.id);
            note.text
        })
        .collect()
}

#[test]
fn a_note_mark_copied_and_pasted_makes_a_note_of_its_own() {
    // Word's rule: the pasted mark is a new note saying what the copied one
    // said, and not a second mark of the same note.
    let mut document = with_a_footnote();
    select(&mut document, 0, 7);
    let copied = document.copy_selection();
    document.set_caret(TextPosition::new(0, "Before\u{2} after".len()));
    assert!(document.paste_blocks(&copied));

    let document = round_trip(&document);
    assert_eq!(document.paragraph_text(0).as_deref(), Some("Before\u{2} afterBefore\u{2}"));
    assert_eq!(footnotes_in(&document), vec!["The note".to_owned(), "The note".to_owned()]);
    let ids: Vec<i32> =
        document.notes(wp_docx::notes::Kind::Footnote).into_iter().map(|note| note.id).collect();
    assert_ne!(ids[0], ids[1], "both marks point at one note");
}

#[test]
fn a_note_mark_pasted_into_another_document_brings_its_note() {
    let mut document = with_a_footnote();
    select(&mut document, 0, 7);
    let copied = document.copy_selection();
    let mut other = words("Elsewhere ");
    other.set_caret(TextPosition::new(0, "Elsewhere ".len()));
    assert!(other.paste_blocks(&copied));

    let other = round_trip(&other);
    assert_eq!(footnotes_in(&other), vec!["The note".to_owned()]);
}

// --- Tracked changes in a copy ----------------------------------------------

fn revision(kind: wp_docx::model::RevisionKind, author: &str) -> wp_docx::model::Revision {
    wp_docx::model::Revision {
        kind,
        author: author.to_owned(),
        date: "2026-01-01T00:00:00Z".to_owned(),
        id: 1,
    }
}

/// A document whose first paragraph is `runs` and whose second says "Here: ".
fn tracked(runs: Vec<Run>) -> Document {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::from_runs(runs)));
    body.blocks.push(Block::Paragraph(Paragraph::text("Here: ")));
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

/// "old", deleted by Ann, and then "NEW": the caret sees three characters.
fn over_a_deletion() -> Document {
    let deleted = Run {
        revision: Some(revision(wp_docx::model::RevisionKind::Deleted, "Ann")),
        ..Run::text("old")
    };
    let document = tracked(vec![deleted, Run::text("NEW")]);
    assert_eq!(document.paragraph_text(0).as_deref(), Some("NEW"));
    document
}

/// Pastes a copy at the end of the second paragraph, saves, and gives back
/// that paragraph as it was read again.
fn pasted_into_the_second(mut document: Document, copied: &[Block]) -> Paragraph {
    document.set_caret(TextPosition::new(1, "Here: ".len()));
    assert!(document.paste_blocks(copied), "the paste said nothing was pasted");
    paragraph_of(&round_trip(&document), 1)
}

#[test]
fn a_copy_over_a_tracked_deletion_takes_what_the_caret_sees() {
    let mut document = over_a_deletion();
    select(&mut document, 0, 3);
    let copied = document.copy_selection();
    assert_eq!(copied.len(), 1);
    assert_eq!(copied[0].plain_text(), "NEW", "the copy took another stretch");
}

#[test]
fn a_tracked_deletion_in_a_copy_is_pasted_as_a_deletion() {
    let mut document = over_a_deletion();
    select(&mut document, 0, 3);
    let copied = document.copy_selection();

    let pasted = pasted_into_the_second(document, &copied);
    assert_eq!(pasted.plain_text(), "Here: NEW", "the deleted words became words");
    let old = pasted
        .runs
        .iter()
        .find(|run| run.content == vec![RunContent::Text("old".to_owned())])
        .expect("the deletion was not pasted at all");
    let change = old.revision.as_ref().expect("the deletion was pasted as plain text");
    assert_eq!(change.kind, wp_docx::model::RevisionKind::Deleted);
    assert_eq!(change.author, "Ann");
}

#[test]
fn a_tracked_insertion_in_a_copy_is_pasted_as_an_insertion() {
    let inserted = Run {
        revision: Some(revision(wp_docx::model::RevisionKind::Inserted, "Ann")),
        ..Run::text("NEW")
    };
    let mut document = tracked(vec![Run::text("A"), inserted, Run::text("B")]);
    select(&mut document, 1, 4);
    let copied = document.copy_selection();
    assert_eq!(copied[0].plain_text(), "NEW");

    let pasted = pasted_into_the_second(document, &copied);
    assert_eq!(pasted.plain_text(), "Here: NEW");
    let new = run_holding(&pasted, "NEW");
    let change = new.revision.as_ref().expect("the insertion was pasted as plain text");
    assert_eq!(change.kind, wp_docx::model::RevisionKind::Inserted);
    assert_eq!(change.author, "Ann");
}

#[test]
fn a_paste_into_somebody_elses_insertion_lands_where_the_caret_is() {
    // Text typed while changes were tracked is all inside one `w:ins`, and
    // the paste used to go after the whole of it.
    let inserted = Run {
        revision: Some(revision(wp_docx::model::RevisionKind::Inserted, "Ann")),
        ..Run::text("ABCD")
    };
    let mut document = tracked(vec![inserted]);
    document.set_caret(TextPosition::new(1, 0));
    document.extend_selection_to(TextPosition::new(1, 4));
    let copied = document.copy_selection();

    document.set_caret(TextPosition::new(0, 2));
    assert!(document.paste_blocks(&copied));
    let pasted = paragraph_of(&round_trip(&document), 0);
    assert_eq!(pasted.plain_text(), "ABHereCD");
    // Ann's insertion is cut in two round what was pasted, which is nobody's.
    assert!(run_holding(&pasted, "Here").revision.is_none());
    for half in ["AB", "CD"] {
        let change = run_holding(&pasted, half).revision.expect("the insertion was lost");
        assert_eq!(change.author, "Ann");
    }
}

#[test]
fn a_paste_at_the_start_of_a_paragraph_goes_after_its_properties() {
    // The properties come first in a paragraph, and Word will not open one
    // that has a run before them.
    let mut document = heading_and_body();
    select(&mut document, 0, 5);
    let copied = document.copy_selection();
    document.set_caret(TextPosition::new(0, 0));
    assert!(document.paste_blocks(&copied));

    let document = round_trip(&document);
    assert_eq!(paragraph_of(&document, 0).plain_text(), "LargeLarge bold");
    let body = document.tree().root.child_elements().next().expect("the body");
    let first = body.child_elements().next().expect("the first paragraph");
    assert_eq!(
        first.child_elements().next().map(wp_xml::tree::Element::local_name),
        Some("pPr"),
        "something went in before the paragraph's properties"
    );
}

#[test]
fn a_paste_while_changes_are_tracked_is_one_insertion_of_what_the_copy_says() {
    // Word's rule: with Track Changes on where it lands, a paste is the copy
    // with its changes accepted, recorded as the insertion of whoever pastes.
    let mut document = over_a_deletion();
    select(&mut document, 0, 3);
    let copied = document.copy_selection();

    document.set_tracking_changes(true);
    document.set_reviser(wp_docx::revisions::Reviser {
        author: "Bea".to_owned(),
        date: "2026-02-02T00:00:00Z".to_owned(),
    });
    let pasted = pasted_into_the_second(document, &copied);
    assert_eq!(pasted.plain_text(), "Here: NEW");
    assert!(
        pasted.runs.iter().all(|run| run.content != vec![RunContent::Text("old".to_owned())]),
        "the deletion came across: {:?}",
        pasted.runs
    );
    let new = run_holding(&pasted, "NEW");
    let change = new.revision.as_ref().expect("the paste was not recorded");
    assert_eq!(change.kind, wp_docx::model::RevisionKind::Inserted);
    assert_eq!(change.author, "Bea");
}

// --- Where the caret is after a paste ----------------------------------------

#[test]
fn the_caret_ends_after_a_pasted_shape() {
    let shape = wp_docx::shapes::Shape {
        name: "Square".to_owned(),
        width_emu: 914_400,
        height_emu: 914_400,
        ..wp_docx::shapes::Shape::default()
    };
    let runs = vec![
        Run { content: vec![RunContent::Shape(Box::new(shape))], ..Run::text("") },
        Run::text("A"),
    ];
    let mut document = words("");
    assert!(document.paste_blocks(&[Block::Paragraph(Paragraph::from_runs(runs))]));
    document.type_text("B");
    assert_eq!(document.paragraph_text(0).as_deref(), Some("\u{1}AB"));
}

#[test]
fn the_caret_ends_after_a_pasted_picture() {
    let mut document = holding(with_picture);
    select(&mut document, 7, 8);
    let copied = document.copy_selection();

    let mut other = words("");
    assert!(other.paste_blocks(&copied));
    other.type_text("B");
    assert_eq!(other.paragraph_text(0).as_deref(), Some("\u{1}B"));
}
