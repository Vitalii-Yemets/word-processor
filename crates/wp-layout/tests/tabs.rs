//! Where a tab actually puts the text: at a stop, centred on one, ended on
//! one, or lined up on its decimal point.

use wp_docx::model::{Block, Body, Paragraph, TabAlignment, TabLeader, TabStop};
use wp_docx::Document;
use wp_layout::{FontLibrary, LayoutEngine, Page, PageMetrics};

/// The fonts, read once per test.
fn library() -> &'static FontLibrary {
    Box::leak(Box::new(FontLibrary::scan_system()))
}

/// A document of one paragraph with the stops given.
fn document(text: &str, stops: &[TabStop]) -> Document {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text(text)));
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    let mut document = Document::open(&bytes).expect("reopening");
    if !stops.is_empty() {
        document.set_tab_stops_here(stops);
    }
    let bytes = document.save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

fn pages(document: &Document) -> Vec<Page> {
    let mut engine = LayoutEngine::new(library());
    engine.layout_document_with(document, PageMetrics::default())
}

fn stop(position: i32, alignment: TabAlignment, leader: TabLeader) -> TabStop {
    TabStop { position, alignment, leader }
}

/// Where the glyph for a character sits across the page, in pixels.
///
/// The first one of that character, which is all these tests need.
fn x_of(page: &Page, wanted: char, document: &Document) -> f32 {
    let text = document.plain_text();
    let offset = text.find(wanted).expect("the character is in the text");
    page.glyphs
        .iter()
        .find(|glyph| glyph.source.offset == offset && !glyph.invisible)
        .map(|glyph| glyph.x)
        .expect("the character was laid out")
}

/// How far along the page the text area begins, in pixels.
fn origin() -> f32 {
    let metrics = PageMetrics::default();
    // The layout works in pixels at 96 dots per inch by default.
    metrics.margin_left * 96.0 / 72.0
}

/// A position in twips as the layout would put it, in pixels.
fn pixels(twips: i32) -> f32 {
    origin() + twips as f32 / 20.0 * 96.0 / 72.0
}

#[test]
fn a_tab_with_no_stops_reaches_the_default_grid() {
    // Half an inch is the default gap, so the first tab of a line lands there.
    let document = document("A\tB", &[]);
    let laid = pages(&document);
    let at = x_of(&laid[0], 'B', &document);
    assert!((at - pixels(720)).abs() < 1.0, "{at} is not half an inch along");
}

#[test]
fn a_left_hand_stop_starts_the_text_at_it() {
    let document = document("A\tB", &[stop(2880, TabAlignment::Start, TabLeader::None)]);
    let laid = pages(&document);
    let at = x_of(&laid[0], 'B', &document);
    assert!((at - pixels(2880)).abs() < 1.0, "{at} is not at the stop");
}

#[test]
fn a_right_hand_stop_ends_the_text_at_it() {
    // What puts a page number against the margin.
    let document = document("A\tBBBB", &[stop(2880, TabAlignment::End, TabLeader::None)]);
    let laid = pages(&document);
    let at = x_of(&laid[0], 'B', &document);
    assert!(at < pixels(2880), "the text did not end at the stop: {at}");

    let width: f32 = laid[0]
        .glyphs
        .iter()
        .filter(|glyph| glyph.x >= at && !glyph.invisible)
        .map(|glyph| glyph.advance)
        .sum();
    assert!((at + width - pixels(2880)).abs() < 1.5, "the text ends at {}", at + width);
}

#[test]
fn a_centred_stop_puts_the_middle_of_the_text_on_it() {
    let document = document("A\tBBBB", &[stop(2880, TabAlignment::Center, TabLeader::None)]);
    let laid = pages(&document);
    let at = x_of(&laid[0], 'B', &document);

    let width: f32 = laid[0]
        .glyphs
        .iter()
        .filter(|glyph| glyph.x >= at && !glyph.invisible)
        .map(|glyph| glyph.advance)
        .sum();
    let middle = at + width / 2.0;
    assert!((middle - pixels(2880)).abs() < 1.5, "the middle is at {middle}");
}

#[test]
fn a_decimal_stop_lines_the_points_up() {
    // Two lines whose figures are different lengths: what makes a column of
    // money line up is the point, not the first digit.
    let stops = [stop(2880, TabAlignment::Decimal, TabLeader::None)];
    let first = document("A\t12.5", &stops);
    let second = document("A\t1234.5", &stops);

    let first_point = x_of(&pages(&first)[0], '.', &first);
    let second_point = x_of(&pages(&second)[0], '.', &second);
    assert!(
        (first_point - second_point).abs() < 1.0,
        "the points are at {first_point} and {second_point}"
    );
    assert!((first_point - pixels(2880)).abs() < 1.5, "the point is not on the stop");
}

#[test]
fn a_stop_that_has_been_passed_is_not_gone_back_to() {
    // The text before the tab is already past the first stop, so the tab has to
    // reach the second one rather than move backwards.
    let stops = [
        stop(200, TabAlignment::Start, TabLeader::None),
        stop(4320, TabAlignment::Start, TabLeader::None),
    ];
    let document = document("A long piece of text\tB", &stops);
    let laid = pages(&document);
    let at = x_of(&laid[0], 'B', &document);
    assert!((at - pixels(4320)).abs() < 1.0, "{at} is not at the second stop");
}

#[test]
fn text_too_wide_for_a_right_hand_stop_carries_on_from_where_it_was() {
    // Rather than being pushed backwards over what came before it.
    let stops = [stop(400, TabAlignment::End, TabLeader::None)];
    let document = document("A very long piece of text indeed\tBBBB", &stops);
    let laid = pages(&document);
    let at = x_of(&laid[0], 'B', &document);
    assert!(at > origin(), "the text was pushed off the left of the page: {at}");
}

#[test]
fn a_dotted_leader_fills_the_space_the_tab_jumped() {
    let with = document("A\tB", &[stop(2880, TabAlignment::Start, TabLeader::Dot)]);
    let without = document("A\tB", &[stop(2880, TabAlignment::Start, TabLeader::None)]);

    let dotted = pages(&with)[0].glyphs.len();
    let plain = pages(&without)[0].glyphs.len();
    assert!(dotted > plain + 5, "no dots were drawn: {dotted} against {plain}");
}

#[test]
fn the_dots_stop_where_the_text_begins() {
    let document = document("A\tB", &[stop(2880, TabAlignment::Start, TabLeader::Dot)]);
    let laid = pages(&document);
    let text_at = x_of(&laid[0], 'B', &document);
    let past: Vec<f32> = laid[0]
        .glyphs
        .iter()
        .filter(|glyph| glyph.source_length == 0 && glyph.x > text_at)
        .map(|glyph| glyph.x)
        .collect();
    assert!(past.is_empty(), "dots were drawn past the text: {past:?}");
}

#[test]
fn a_bar_stop_draws_a_line_and_moves_nothing() {
    let with = document("A\tB", &[stop(2880, TabAlignment::Bar, TabLeader::None)]);
    let laid = pages(&with);
    // The tab passes the bar and lands on the default grid instead.
    let at = x_of(&laid[0], 'B', &with);
    assert!((at - pixels(720)).abs() < 1.0, "the bar caught the tab: {at}");

    let bars: Vec<f32> = laid[0]
        .decorations
        .iter()
        .filter(|rule| (rule.x - pixels(2880)).abs() < 1.0)
        .map(|rule| rule.x)
        .collect();
    assert_eq!(bars.len(), 1, "no line was drawn down the page: {:?}", laid[0].decorations);
}
