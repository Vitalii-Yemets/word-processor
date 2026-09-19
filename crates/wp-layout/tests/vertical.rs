//! A section written down the page: Word's Vertical text direction.
//!
//! The body is laid out into a box as long as the page is tall and turned
//! onto the page — so a line is a column, the lines go from right to left,
//! the ideographs stand upright in them and the Latin lies on its side. These
//! tests hold the pages to that, and to being the same pages whatever the
//! engine has been through before.

use wp_docx::model::{Block, Body, Paragraph, Run, TextDirection};
use wp_docx::{Document, TextPosition};
use wp_layout::{FontLibrary, LayoutEngine, Page, PageMetrics, Turn};

fn library() -> &'static FontLibrary {
    Box::leak(Box::new(FontLibrary::scan_system()))
}

/// Whether the machine has a font that draws Japanese, which the build image
/// does and a bare machine may not.
fn has_japanese() -> bool {
    library().fallback_for('日', false, false).is_some()
}

const JAPANESE: &str = "日本語の文章を縦に書く。";

fn document(paragraphs: &[&str], direction: TextDirection) -> Document {
    let mut body = Body::default();
    for text in paragraphs {
        body.blocks.push(Block::Paragraph(Paragraph::text(text)));
    }
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    let mut document = Document::open(&bytes).expect("reopening");
    if direction != TextDirection::Horizontal {
        assert!(document.set_text_direction(direction), "the direction did not take");
    }
    document
}

fn pages(document: &Document) -> Vec<Page> {
    let mut engine = LayoutEngine::new(library());
    engine.layout_document(document)
}

#[test]
fn the_direction_is_written_and_read_back() {
    let document = document(&["Across"], TextDirection::Down);
    assert_eq!(document.text_direction(), TextDirection::Down);
    let bytes = document.save().expect("saving");
    let again = Document::open(&bytes).expect("reopening");
    assert_eq!(again.text_direction(), TextDirection::Down);
    assert_eq!(PageMetrics::from_document(&again).direction, TextDirection::Down);
}

#[test]
fn the_box_of_a_turned_page_is_the_paper_on_its_side() {
    let paper = PageMetrics {
        margin_top: 10.0,
        margin_right: 20.0,
        margin_bottom: 30.0,
        margin_left: 40.0,
        direction: TextDirection::Down,
        ..PageMetrics::default()
    };
    let boxed = paper.boxed();
    assert_eq!(boxed.width, paper.height);
    assert_eq!(boxed.height, paper.width);
    // The lines begin at the top of the page and the first stands at the
    // right, so the box's left is the top margin and its top the right one.
    assert_eq!(boxed.margin_left, 10.0);
    assert_eq!(boxed.margin_top, 20.0);
    assert_eq!(boxed.margin_right, 30.0);
    assert_eq!(boxed.margin_bottom, 40.0);
    assert_eq!(boxed.direction, TextDirection::Horizontal);
    assert_eq!(PageMetrics::default().boxed(), PageMetrics::default());
}

#[test]
fn lines_are_columns_from_the_right_edge_leftwards() {
    if library().is_empty() {
        return;
    }
    let text = "The quick brown fox jumps over the lazy dog, again and again, ".repeat(6);
    let document = document(&[&text, "Second paragraph"], TextDirection::Down);
    let pages = pages(&document);
    let page = &pages[0];
    // The paper is still upright.
    assert!(page.height > page.width, "the page was turned: {} by {}", page.width, page.height);
    assert!(page.frame.is_turned());
    assert_eq!(page.turned_glyphs, page.glyphs.len());

    let lines: Vec<_> = page.lines.iter().collect();
    assert!(lines.len() >= 3, "expected several lines, got {}", lines.len());
    for line in &lines {
        assert!(line.frame.is_sideways(), "a line was not turned: {line:?}");
        // Each line's band is a tall narrow column.
        let (_, _, width, height) = line.band(line.left, line.right);
        assert!(height > width, "a line's band is not a column: {width} by {height}");
    }
    // Each line stands to the left of the one before it.
    let across: Vec<f32> = lines.iter().map(|line| line.band(line.left, line.right).0).collect();
    for pair in across.windows(2) {
        assert!(pair[1] < pair[0], "the lines do not go leftwards: {across:?}");
    }
    // The first line starts at the top right, inside the margins.
    let scale = 96.0 / 72.0;
    let (x, y, width, _) = lines[0].band(lines[0].left, lines[0].right);
    let margin = 72.0 * scale;
    assert!((x + width - (page.width - margin)).abs() < 2.0, "first line at {x}+{width}");
    assert!((y - margin).abs() < 2.0, "first line begins at {y}");

    // A Latin letter lies on its side, turned clockwise.
    assert_eq!(page.turn_of(0), Turn::Down);
    // And the text reads downwards: the second glyph is below the first.
    assert!(page.glyphs[1].baseline > page.glyphs[0].baseline);
}

#[test]
fn ideographs_stand_upright_and_latin_lies_down() {
    if !has_japanese() {
        return;
    }
    let down = document(&[&format!("{JAPANESE}ABC")], TextDirection::Down);
    let laid = pages(&down);
    let page = &laid[0];
    let upright =
        page.glyphs.iter().enumerate().filter(|(at, _)| page.turn_of(*at) == Turn::Upright);
    let turned = page.glyphs.iter().enumerate().filter(|(at, _)| page.turn_of(*at) == Turn::Down);
    let upright: Vec<_> = upright.map(|(_, glyph)| glyph).collect();
    let turned: Vec<_> = turned.map(|(_, glyph)| glyph).collect();
    assert_eq!(upright.len(), JAPANESE.chars().count(), "the ideographs and kana stand upright");
    assert_eq!(turned.len(), 3, "the Latin letters lie down");
    // An upright letter is drawn beside the pen, not at it.
    assert!(upright.iter().all(|glyph| glyph.shift_x != 0.0 || glyph.shift_y != 0.0));
    assert!(turned.iter().all(|glyph| glyph.shift_x == 0.0 && glyph.shift_y == 0.0));
    // An ideograph takes a square along the line: as much as it is tall.
    // The kana and the full stop take what the font gives them downwards,
    // which in a proportional font is less.
    for glyph in &upright[..3] {
        assert!((glyph.advance - glyph.size).abs() < glyph.size * 0.05, "{}", glyph.advance);
    }
    // And the full stop moved to its vertical form, which is a different glyph
    // from the one the horizontal text uses.
    let across = pages(&document(&[JAPANESE], TextDirection::Horizontal));
    let stop_across = across[0].glyphs.last().expect("a glyph").glyph;
    let stop_down = upright.last().expect("a glyph").glyph;
    assert_ne!(stop_across, stop_down, "the full stop kept its horizontal form");
}

#[test]
fn a_click_and_a_caret_agree_down_the_column() {
    if library().is_empty() {
        return;
    }
    let document = document(&["Hello vertical world"], TextDirection::Down);
    let pages = pages(&document);
    let page = &pages[0];
    for offset in [0usize, 3, 6, 12, 20] {
        let (x, y, width, height) =
            page.caret_at(TextPosition::new(0, offset), 1.0).expect("a caret");
        // A caret in a vertical line lies across the column.
        assert!(width > height, "the caret stands up: {width} by {height}");
        let back = page.position_at(x + width / 2.0, y + 1.0).expect("a position");
        assert_eq!(back.offset, offset, "offset {offset} came back as {}", back.offset);
    }
}

#[test]
fn the_pages_are_the_same_whatever_the_engine_has_been_through() {
    if library().is_empty() {
        return;
    }
    let text = "Words words words words words words words words words words. ".repeat(40);
    let mut document = document(&[&text, "Second", &text], TextDirection::Down);
    let mut engine = LayoutEngine::new(library());
    let metrics = PageMetrics::from_document(&document);
    let first = engine.layout_document_with(&document, metrics);
    // The same pages again, given the last ones back.
    let again = engine.layout_document_again(&document, metrics, first.clone());
    assert_eq!(again, first, "the pages changed with nothing changed");

    // A letter typed into the second paragraph: the pages are what a new
    // engine gives.
    document.insert_text(TextPosition::new(1, 6), "!");
    let edited = engine.layout_document_again(&document, metrics, again);
    let fresh = LayoutEngine::new(library()).layout_document_with(&document, metrics);
    assert_eq!(edited.len(), fresh.len());
    for (page, other) in edited.iter().zip(&fresh) {
        assert_eq!(page.glyphs.len(), other.glyphs.len());
        for (glyph, theirs) in page.glyphs.iter().zip(&other.glyphs) {
            assert!(
                (glyph.x - theirs.x).abs() < 0.01
                    && (glyph.baseline - theirs.baseline).abs() < 0.01,
                "a glyph moved: {glyph:?} against {theirs:?}"
            );
            assert_eq!(glyph.source, theirs.source);
        }
        assert_eq!(page.frame, other.frame);
        assert_eq!(page.turned, other.turned);
        assert_eq!(page.lines.len(), other.lines.len());
    }
}

#[test]
fn columns_are_bands_down_the_page() {
    if library().is_empty() {
        return;
    }
    let text = "Column text column text column text column text column text. ".repeat(30);
    let mut document = document(&[&text], TextDirection::Down);
    assert!(document.set_columns(2, 720));
    let pages = pages(&document);
    let page = &pages[0];
    let tops: Vec<f32> = page.lines.iter().map(|line| line.band(line.left, line.right).1).collect();
    let lowest = tops.iter().copied().fold(0.0f32, f32::max);
    let highest = tops.iter().copied().fold(f32::MAX, f32::min);
    // Two bands: some lines begin near the top margin and some halfway down.
    assert!(lowest > page.height / 2.0, "no line began in the lower band: {tops:?}");
    assert!(highest < page.height / 4.0, "no line began in the upper band: {tops:?}");
}

#[test]
fn a_run_set_across_the_line_stands_upright_in_one_square() {
    if !has_japanese() {
        return;
    }
    let mut paragraph = Paragraph::text("");
    paragraph.runs = vec![
        Run::text("令和"),
        {
            let mut run = Run::text("12");
            run.properties.east_asian_layout = Some(wp_docx::eastasian::EastAsianLayout {
                horizontal_in_vertical: true,
                fit_in_line: true,
                ..Default::default()
            });
            run
        },
        Run::text("年"),
    ];
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(paragraph));
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    let mut document = Document::open(&bytes).expect("reopening");
    assert!(document.set_text_direction(TextDirection::Down));
    let pages = pages(&document);
    let page = &pages[0];
    assert_eq!(page.glyphs.len(), 5);
    // The two figures stand upright, side by side across the column: the
    // same height on the page, one to the right of the other.
    assert_eq!(page.turn_of(2), Turn::Upright);
    assert_eq!(page.turn_of(3), Turn::Upright);
    let one = &page.glyphs[2];
    let two = &page.glyphs[3];
    let drawn = |glyph: &wp_layout::PositionedGlyph| {
        (glyph.x + glyph.shift_x, glyph.baseline + glyph.shift_y)
    };
    let (x1, y1) = drawn(one);
    let (x2, y2) = drawn(two);
    assert!((y1 - y2).abs() < 0.01, "the figures are not on one line: {y1} and {y2}");
    assert!(x2 > x1, "the figures read backwards: {x1} then {x2}");
    // And together they take one square along the column, the same as the
    // ideograph before them.
    let square = page.glyphs[0].advance;
    assert!(
        ((one.advance + two.advance) - square).abs() < 0.5,
        "{} + {}",
        one.advance,
        two.advance
    );
}

#[test]
fn two_lines_in_one_stack_inside_the_line() {
    if library().is_empty() {
        return;
    }
    let mut paragraph = Paragraph::text("");
    paragraph.runs = vec![
        Run::text("Before "),
        {
            let mut run = Run::text("abcd");
            run.properties.east_asian_layout = Some(wp_docx::eastasian::EastAsianLayout {
                two_lines_in_one: true,
                brackets: wp_docx::eastasian::CombineBrackets::Round,
                ..Default::default()
            });
            run
        },
        Run::text(" after"),
    ];
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(paragraph));
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    let document = Document::open(&bytes).expect("reopening");
    let pages = pages(&document);
    let page = &pages[0];
    let of = |offset: usize| {
        page.glyphs
            .iter()
            .find(|glyph| glyph.source.offset == offset && glyph.source_length > 0)
            .expect("a glyph")
    };
    let (a, b, c, d) = (of(7), of(8), of(9), of(10));
    // Half the size of the text round them.
    assert!((a.size - of(0).size / 2.0).abs() < 0.01);
    // "ab" on the upper line and "cd" on the lower, each pair side by side.
    assert!(a.shift_y < 0.0 && b.shift_y < 0.0, "the upper line is not raised");
    assert!(c.shift_y > 0.0 && d.shift_y > 0.0, "the lower line is not lowered");
    assert!((a.baseline + a.shift_y - (b.baseline + b.shift_y)).abs() < 0.01);
    assert!(b.x + b.shift_x > a.x + a.shift_x);
    assert!(d.x + d.shift_x > c.x + c.shift_x);
    // The lower line starts where the upper one does.
    assert!((c.x + c.shift_x - (a.x + a.shift_x)).abs() < 0.5);
    // The brackets are there, and are nobody's characters.
    let brackets =
        page.glyphs.iter().filter(|glyph| glyph.source_length == 0 && glyph.size > a.size);
    assert_eq!(brackets.count(), 2);
    // The text after carries on past the pair.
    assert!(of(12).x > d.x + d.shift_x);
}
