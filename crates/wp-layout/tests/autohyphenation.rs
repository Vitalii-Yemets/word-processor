//! Automatic hyphenation: a word broken where nobody marked it, because the
//! language's patterns say it may be — when the document asks for that.
//!
//! The patterns come from the machine, as the spelling dictionaries do; the
//! build image has the English ones, and a machine without them lays the
//! text out with every word whole, which is what these tests check first.

use wp_docx::model::{Block, Body, Paragraph, Run};
use wp_docx::Document;
use wp_layout::{FontLibrary, LayoutEngine, Page, PageMetrics};

fn library() -> &'static FontLibrary {
    Box::leak(Box::new(FontLibrary::scan_system()))
}

/// Whether the machine has English patterns at all.
fn patterns_present() -> bool {
    if wp_dict::hyphenation::for_language("en-US").is_none() {
        eprintln!("no English hyphenation patterns on this machine; skipping");
        return false;
    }
    true
}

/// One paragraph laid out in a page of a given text width, with the
/// document set as asked.
fn laid_out(text: &str, width: f32, set: impl FnOnce(&mut Document)) -> Vec<Page> {
    let mut body = Body::default();
    let mut paragraph = Paragraph::from_runs(vec![Run::text(text)]);
    paragraph.runs[0].properties.language = Some("en-US".to_owned());
    body.blocks.push(Block::Paragraph(paragraph));

    let mut document = Document::create(&body).expect("a document");
    set(&mut document);
    let bytes = document.save().expect("saving");
    let document = Document::open(&bytes).expect("reopening");

    let metrics = PageMetrics {
        width: width + 144.0,
        margin_left: 72.0,
        margin_right: 72.0,
        ..PageMetrics::default()
    };
    LayoutEngine::new(library()).layout_document_with(&document, metrics)
}

/// How many letters a page draws: the glyphs that stand for text and are
/// not spaces, which a line may or may not carry at its end.
fn visible(pages: &[Page]) -> usize {
    pages
        .iter()
        .flat_map(|page| page.glyphs.iter())
        .filter(|glyph| !glyph.invisible && glyph.source_length > 0)
        .filter(|glyph| TEXT.as_bytes().get(glyph.source.offset).is_some_and(|byte| *byte != b' '))
        .count()
}

/// How many lines end with a hyphen that is not text: the hyphen the line
/// drew for a word it broke.
fn hyphenated_lines(pages: &[Page]) -> usize {
    pages
        .iter()
        .flat_map(|page| {
            page.lines.iter().map(move |line| {
                page.glyphs[line.glyphs.clone()]
                    .iter()
                    .rev()
                    .find(|glyph| !glyph.invisible)
                    .is_some_and(|glyph| glyph.source_length == 0)
            })
        })
        .filter(|hyphenated| *hyphenated)
        .count()
}

const TEXT: &str = "hyphenation information responsibility understanding";

#[test]
fn a_document_that_does_not_ask_for_it_keeps_every_word_whole() {
    let pages = laid_out(TEXT, 90.0, |_| {});
    assert_eq!(hyphenated_lines(&pages), 0);
    // Every word on a line of its own, since none fits beside another.
    assert!(pages[0].lines.len() >= 4);
}

#[test]
fn a_document_that_asks_for_it_breaks_words_where_the_language_allows() {
    if !patterns_present() {
        return;
    }
    let pages = laid_out(TEXT, 90.0, |document| {
        assert!(document.set_automatic_hyphenation(true));
    });
    assert!(hyphenated_lines(&pages) >= 2, "no word was broken: {} lines", pages[0].lines.len());
    // Every letter is still drawn once: nothing is added or lost by the cut.
    let whole = laid_out(TEXT, 400.0, |_| {});
    assert_eq!(visible(&pages), visible(&whole));
    // And a wide page breaks nothing, hyphenation or no.
    let wide = laid_out(TEXT, 400.0, |document| {
        document.set_automatic_hyphenation(true);
    });
    assert_eq!(hyphenated_lines(&wide), 0);
    assert_eq!(wide[0].lines.len(), 1);
}

#[test]
fn a_paragraph_that_says_to_leave_its_words_alone_is_left_alone() {
    if !patterns_present() {
        return;
    }
    let pages = laid_out(TEXT, 90.0, |document| {
        document.set_automatic_hyphenation(true);
        document.set_caret(wp_docx::TextPosition::new(0, 0));
        assert!(document.set_paragraph_format(&wp_docx::model::ParagraphProperties {
            no_hyphenation: Some(true),
            ..wp_docx::model::ParagraphProperties::default()
        }));
    });
    assert_eq!(hyphenated_lines(&pages), 0);
}

#[test]
fn the_zone_keeps_a_word_whole_when_carrying_it_over_leaves_little_room() {
    if !patterns_present() {
        return;
    }
    // "in" fits and "formation" would not: with a small zone the word is
    // broken after "in"; with a zone wider than the room "in" would leave,
    // the whole word goes over.
    let text = "in information";
    let broken = laid_out(text, 60.0, |document| {
        document.set_automatic_hyphenation(true);
        document.set_hyphenation_zone(Some(20));
    });
    let whole = laid_out(text, 60.0, |document| {
        document.set_automatic_hyphenation(true);
        document.set_hyphenation_zone(Some(2000));
    });
    assert!(hyphenated_lines(&broken) >= 1, "a small zone did not break the word");
    assert_eq!(hyphenated_lines(&whole), 0, "a wide zone broke the word");
}

#[test]
fn the_limit_on_hyphens_in_a_row_is_kept() {
    if !patterns_present() {
        return;
    }
    let text = "responsibility responsibility responsibility responsibility";
    let unlimited = laid_out(text, 60.0, |document| {
        document.set_automatic_hyphenation(true);
        document.set_hyphenation_zone(Some(20));
    });
    let limited = laid_out(text, 60.0, |document| {
        document.set_automatic_hyphenation(true);
        document.set_hyphenation_zone(Some(20));
        document.set_consecutive_hyphen_limit(Some(1));
    });
    assert!(hyphenated_lines(&unlimited) >= 3, "{}", hyphenated_lines(&unlimited));
    // No two lines in a row end with a hyphen.
    let page = &limited[0];
    let ends: Vec<bool> = page
        .lines
        .iter()
        .map(|line| {
            page.glyphs[line.glyphs.clone()]
                .iter()
                .rev()
                .find(|glyph| !glyph.invisible)
                .is_some_and(|glyph| glyph.source_length == 0)
        })
        .collect();
    assert!(!ends.windows(2).any(|pair| pair[0] && pair[1]), "{ends:?}");
    assert!(hyphenated_lines(&limited) >= 1);
}

#[test]
fn words_in_capitals_are_left_alone_when_asked() {
    if !patterns_present() {
        return;
    }
    let text = "HYPHENATION INFORMATION";
    let broken = laid_out(text, 70.0, |document| {
        document.set_automatic_hyphenation(true);
    });
    let whole = laid_out(text, 70.0, |document| {
        document.set_automatic_hyphenation(true);
        document.set_hyphenate_capitals(false);
    });
    assert!(hyphenated_lines(&broken) >= 1);
    assert_eq!(hyphenated_lines(&whole), 0);
}
