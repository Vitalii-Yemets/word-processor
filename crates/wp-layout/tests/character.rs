//! The character formatting behind Word's Font dialog, as it is drawn.
//!
//! The file half of it is proved in `wp-docx`: what is written comes back. This
//! is the other half, and the one that is easier to get wrong — a property can
//! round-trip through the file perfectly and still be drawn as though it were
//! not there. So every test here asks the engine to lay text out twice, once
//! with the property and once without, and holds it to a difference that can
//! only come from the property having been honoured.

use wp_docx::model::{Block, Body, Paragraph, Run, RunProperties};
use wp_docx::typography::{Ligatures, NumberSpacing, OpenType};
use wp_docx::Document;
use wp_layout::{FontLibrary, LayoutEngine, Page, PageMetrics};

fn library() -> &'static FontLibrary {
    Box::leak(Box::new(FontLibrary::scan_system()))
}

/// One paragraph of the given text, formatted the given way, laid out.
fn laid_out(text: &str, properties: RunProperties) -> Vec<Page> {
    let mut run = Run::text(text);
    run.properties = properties;
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::from_runs(vec![run])));

    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    let document = Document::open(&bytes).expect("reopening");
    LayoutEngine::new(library()).layout_document_with(&document, PageMetrics::default())
}

/// How far the text of the first line reaches.
fn width(pages: &[Page]) -> f32 {
    let Some(page) = pages.first() else { return 0.0 };
    page.glyphs.iter().map(|glyph| glyph.x + glyph.advance).fold(0.0f32, f32::max)
        - page.glyphs.iter().map(|glyph| glyph.x).fold(f32::MAX, f32::min)
}

/// The sizes the glyphs of the first line are drawn at, largest first.
fn sizes(pages: &[Page]) -> Vec<f32> {
    let Some(page) = pages.first() else { return Vec::new() };
    let mut out: Vec<f32> = page.glyphs.iter().filter(|g| !g.invisible).map(|g| g.size).collect();
    out.sort_by(|a, b| b.partial_cmp(a).expect("no glyph has no size"));
    out.dedup_by(|a, b| (*a - *b).abs() < 0.01);
    out
}

/// How many glyphs the first line draws.
fn drawn(pages: &[Page]) -> usize {
    pages.first().map_or(0, |page| page.glyphs.iter().filter(|g| !g.invisible).count())
}

#[test]
fn capitals_are_drawn_where_the_document_holds_small_letters() {
    let plain = laid_out("word", RunProperties::default());
    let capitals = laid_out("word", RunProperties { caps: Some(true), ..RunProperties::default() });

    // Capitals are wider than the small letters they stand for, in every font
    // that has both.
    assert!(
        width(&capitals) > width(&plain),
        "{} is not wider than {}",
        width(&capitals),
        width(&plain)
    );
    // And the text itself is untouched: this is formatting, not Change Case.
    assert_eq!(drawn(&capitals), drawn(&plain));
}

#[test]
fn small_capitals_are_capitals_drawn_smaller() {
    // "Word" has one letter that was already a capital and three that were not,
    // so a run set in small capitals is drawn at two sizes.
    let small = laid_out("Word", RunProperties { small_caps: Some(true), ..Default::default() });
    let plain = laid_out("Word", RunProperties::default());

    assert_eq!(sizes(&plain).len(), 1, "plain text is all one size");
    assert_eq!(sizes(&small).len(), 2, "small capitals are two sizes: {:?}", sizes(&small));

    // The larger of the two is the size the run asked for, and the smaller is
    // the one the letters that were small are shrunk to.
    let two = sizes(&small);
    assert!((two[0] - sizes(&plain)[0]).abs() < 0.01, "the capital changed size");
    assert!(two[1] < two[0], "the small capitals are not smaller");
}

#[test]
fn a_letter_whose_capital_is_two_letters_still_points_at_one_character() {
    // The German ß becomes SS. Two glyphs are drawn and the caret must still
    // have exactly the places the text has, or a click lands in the wrong word.
    let pages = laid_out("straße", RunProperties { caps: Some(true), ..Default::default() });
    let page = pages.first().expect("a page");

    let offsets: Vec<usize> = page.glyphs.iter().map(|glyph| glyph.source.offset).collect();
    assert!(drawn(&pages) > "straße".chars().count(), "the ß was not drawn as two letters");
    // Every offset is one the text actually has.
    for offset in offsets {
        assert!("straße".is_char_boundary(offset), "{offset} is inside a character");
    }
}

#[test]
fn hidden_text_takes_up_no_room_and_draws_nothing() {
    let plain = laid_out("hidden words", RunProperties::default());
    let hidden =
        laid_out("hidden words", RunProperties { hidden: Some(true), ..Default::default() });

    assert!(drawn(&plain) > 0);
    assert_eq!(drawn(&hidden), 0, "hidden text was drawn");
    assert_eq!(width(&hidden), 0.0, "hidden text took up room");
}

#[test]
fn letters_drawn_wider_take_up_more_room() {
    let plain = laid_out("width", RunProperties::default());
    let wide = laid_out("width", RunProperties { scale: Some(200), ..Default::default() });
    let narrow = laid_out("width", RunProperties { scale: Some(50), ..Default::default() });

    // Twice as wide, near enough: the last glyph's own advance is scaled too.
    let ratio = width(&wide) / width(&plain);
    assert!((ratio - 2.0).abs() < 0.05, "twice the scale gave {ratio} times the width");
    assert!(width(&narrow) < width(&plain));

    // And the outline is stretched, not merely the room after it — which is
    // what separates Word's Scale from its Spacing.
    let page = wide.first().expect("a page");
    assert!(page.glyphs.iter().all(|glyph| (glyph.stretch - 2.0).abs() < 0.001));
}

#[test]
fn room_asked_for_between_letters_is_added_to_every_one() {
    let plain = laid_out("spacing", RunProperties::default());
    // One point between each letter, in the twentieths of a point the file uses.
    let spaced =
        laid_out("spacing", RunProperties { spacing_twentieths: Some(20), ..Default::default() });
    let tight =
        laid_out("spacing", RunProperties { spacing_twentieths: Some(-10), ..Default::default() });

    let letters = drawn(&plain) as f32;
    let added = width(&spaced) - width(&plain);
    // A point is 96/72 pixels at the screen's own resolution, once per letter.
    let expected = letters * 96.0 / 72.0;
    assert!((added - expected).abs() < letters, "{added} was added, not about {expected}");
    assert!(width(&tight) < width(&plain), "condensed text is not narrower");
}

#[test]
fn text_can_be_raised_off_the_line_without_being_made_smaller() {
    let plain = laid_out("raised", RunProperties::default());
    // Three points up, in the half-points the file uses.
    let raised =
        laid_out("raised", RunProperties { position_half_points: Some(6), ..Default::default() });

    let baseline = |pages: &[Page]| {
        pages.first().and_then(|page| page.glyphs.first()).map_or(0.0, |glyph| glyph.baseline)
    };
    assert!(baseline(&raised) < baseline(&plain), "the text was not lifted");
    // Unlike a superscript, which shrinks the letters.
    assert_eq!(sizes(&raised), sizes(&plain), "raising the text changed its size");
}

#[test]
fn kerning_is_used_at_the_size_the_document_names_and_not_below_it() {
    // A pair the fonts of this world kern: the A tucks under the V.
    let text = "AVAVAVAV";
    let below = laid_out(
        text,
        RunProperties {
            size_half_points: Some(20),
            // Kern nothing under 24 points, which this text is well under.
            kerning_half_points: Some(48),
            ..Default::default()
        },
    );
    let above = laid_out(
        text,
        RunProperties {
            size_half_points: Some(20),
            kerning_half_points: Some(2),
            ..Default::default()
        },
    );

    // Kerning can only pull letters together, so the kerned text is no wider —
    // and in any font that kerns this pair at all, narrower.
    assert!(width(&above) <= width(&below), "kerning made the text wider");
}

#[test]
fn asking_the_font_for_nothing_leaves_the_letters_alone() {
    // The path through the shaper is only taken when something is asked for,
    // and text that asks for nothing must come out exactly as it did before.
    let plain = laid_out("figures 0123456789", RunProperties::default());
    let empty = laid_out(
        "figures 0123456789",
        RunProperties { open_type: Some(OpenType::default()), ..Default::default() },
    );
    assert_eq!(drawn(&plain), drawn(&empty));
    assert!((width(&plain) - width(&empty)).abs() < 0.01);
}

#[test]
fn a_run_that_asks_the_font_for_something_still_draws_its_text() {
    // Whether a particular font on this machine has tabular figures or standard
    // ligatures cannot be relied on. What can be relied on is that asking for
    // them never loses the text: the shaper is a different path through the
    // engine, and a path that dropped letters would be far worse than one that
    // changed none.
    let asked = laid_out(
        "figures 0123456789",
        RunProperties {
            open_type: Some(OpenType {
                ligatures: Ligatures::Standard,
                number_spacing: NumberSpacing::Tabular,
                ..OpenType::default()
            }),
            ..Default::default()
        },
    );
    let plain = laid_out("figures 0123456789", RunProperties::default());

    assert!(drawn(&asked) > 0, "the text was lost");
    // A ligature joins letters, so there can be fewer glyphs — never more, and
    // never a page of nothing.
    assert!(drawn(&asked) <= drawn(&plain));
    assert!(width(&asked) > 0.0);
}

#[test]
fn a_second_line_is_drawn_through_text_that_asks_for_one() {
    // Counted rather than named: a line is drawn per piece of the text the
    // engine measured separately, so what matters is that the second kind draws
    // exactly twice as many lines as the first.
    let none = laid_out("struck", RunProperties::default());
    let one = laid_out("struck", RunProperties { strike: Some(true), ..Default::default() });
    let two = laid_out("struck", RunProperties { double_strike: Some(true), ..Default::default() });

    let lines = |pages: &[Page]| pages.first().map_or(0, |page| page.decorations.len());
    assert_eq!(lines(&none), 0, "plain text has no lines through it");
    assert!(lines(&one) > 0, "nothing was struck through");
    assert_eq!(lines(&two), lines(&one) * 2, "the second line is not drawn");
}

#[test]
fn an_underline_can_be_a_colour_of_its_own() {
    use wp_docx::model::Underline;

    let plain = laid_out(
        "underlined",
        RunProperties { underline: Some(Underline::Single), ..Default::default() },
    );
    let coloured = laid_out(
        "underlined",
        RunProperties {
            underline: Some(Underline::Single),
            underline_color: Some("FF0000".to_owned()),
            ..Default::default()
        },
    );

    let line_colour = |pages: &[Page]| {
        pages.first().and_then(|page| page.decorations.first()).map(|line| line.color)
    };
    assert_eq!(line_colour(&coloured), Some(wp_raster::Color::rgb(255, 0, 0)));
    assert_ne!(line_colour(&coloured), line_colour(&plain));
}
