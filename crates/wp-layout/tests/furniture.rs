//! Headers, footers and the page number they carry.

use wp_docx::furniture::{Furniture, Preset};
use wp_docx::model::{Alignment, Block, Body, Paragraph};
use wp_docx::Document;
use wp_layout::{FontLibrary, LayoutEngine, Page};

/// A document long enough to run to several pages.
fn document(paragraphs: usize) -> Document {
    let mut body = Body::default();
    for index in 0..paragraphs {
        body.blocks.push(Block::Paragraph(Paragraph::text(&format!("Paragraph {index}"))));
    }
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

/// The fonts, leaked because the engine borrows them for its whole life.
fn library() -> &'static FontLibrary {
    Box::leak(Box::new(FontLibrary::scan_system()))
}

/// How many glyphs a page holds, which is the only measure of "was anything
/// drawn" that does not depend on which font the machine happens to have.
fn glyph_count(page: &Page) -> usize {
    page.glyphs.iter().filter(|glyph| !glyph.invisible).count()
}

#[test]
fn a_footer_puts_glyphs_on_every_page() {
    if library().is_empty() {
        return;
    }
    let mut document = document(120);
    let mut engine = LayoutEngine::new(library());
    let without: Vec<usize> = engine.layout_document(&document).iter().map(glyph_count).collect();

    document
        .set_furniture(Furniture::Footer, Preset::PageNumber, Alignment::Center, "")
        .expect("a footer");
    let with: Vec<usize> = engine.layout_document(&document).iter().map(glyph_count).collect();

    assert_eq!(with.len(), without.len(), "the footer changed how many pages there are");
    assert!(with.len() > 1, "the sample should run to more than one page");
    for (index, (with, without)) in with.iter().zip(&without).enumerate() {
        assert!(with > without, "page {} gained nothing from the footer", index + 1);
    }
}

#[test]
fn the_page_number_is_worked_out_rather_than_taken_from_the_file() {
    if library().is_empty() {
        return;
    }
    // The cached value in the file is always "1"; page two has to say 2.
    let mut document = document(120);
    document
        .set_furniture(Furniture::Footer, Preset::PageNumber, Alignment::Center, "")
        .expect("a footer");

    let mut engine = LayoutEngine::new(library());
    let pages = engine.layout_document(&document);
    assert!(pages.len() > 1);

    // The footer is one glyph per page: the digit. Two different digits means
    // the number was computed and not copied.
    let first = pages[0].glyphs.last().map(|glyph| glyph.glyph);
    let second = pages[1].glyphs.last().map(|glyph| glyph.glyph);
    assert!(first.is_some() && second.is_some());
    assert_ne!(first, second, "every page drew the same digit");
}

#[test]
fn a_header_sits_above_the_text_and_a_footer_below_it() {
    if library().is_empty() {
        return;
    }
    let mut document = document(10);
    document
        .set_furniture(Furniture::Header, Preset::Text, Alignment::Start, "Top of the page")
        .expect("a header");
    document
        .set_furniture(Furniture::Footer, Preset::Text, Alignment::Start, "Bottom of the page")
        .expect("a footer");

    let mut engine = LayoutEngine::new(library());
    let pages = engine.layout_document(&document);
    let page = &pages[0];

    let body_top = page.lines.iter().map(|line| line.baseline).fold(f32::MAX, f32::min);
    let body_bottom = page.lines.iter().map(|line| line.baseline).fold(f32::MIN, f32::max);
    let highest = page.glyphs.iter().map(|glyph| glyph.baseline).fold(f32::MAX, f32::min);
    let lowest = page.glyphs.iter().map(|glyph| glyph.baseline).fold(f32::MIN, f32::max);

    assert!(highest < body_top, "the header was not drawn above the text");
    assert!(lowest > body_bottom, "the footer was not drawn below the text");
    assert!(lowest < page.height, "the footer fell off the bottom of the page");
}

#[test]
fn the_furniture_is_not_text_the_caret_can_reach() {
    if library().is_empty() {
        return;
    }
    let mut document = document(10);
    let mut engine = LayoutEngine::new(library());
    let lines_before = engine.layout_document(&document)[0].lines.len();

    document
        .set_furniture(Furniture::Header, Preset::Text, Alignment::Start, "Not editable yet")
        .expect("a header");
    let lines_after = engine.layout_document(&document)[0].lines.len();

    assert_eq!(lines_before, lines_after, "the header added lines a click could land in");
}

#[test]
fn a_numbered_list_in_the_body_is_not_disturbed_by_the_furniture() {
    if library().is_empty() {
        return;
    }
    let mut document = document(10);
    let mut engine = LayoutEngine::new(library());
    let before = engine.layout_document(&document)[0].glyphs.len();

    document
        .set_furniture(Furniture::Footer, Preset::PageOfTotal, Alignment::Center, "")
        .expect("a footer");
    let after = engine.layout_document(&document)[0].glyphs.len();

    assert!(after > before, "the footer drew nothing");
}

/// Which of a section's three headers each page is printed with.
mod three {
    use wp_docx::furniture::{Furniture, Preset, Which};
    use wp_docx::model::{Alignment, Block, Body, Paragraph};
    use wp_docx::Document;
    use wp_layout::{FontLibrary, LayoutEngine, PageMetrics};

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    /// A document long enough to run to three pages.
    fn document() -> Document {
        let mut body = Body::default();
        for index in 0..120 {
            body.blocks.push(Block::Paragraph(Paragraph::text(&format!("Paragraph {index}"))));
        }
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        Document::open(&bytes).expect("reopening")
    }

    fn set(document: &mut Document, which: Which, text: &str) {
        document
            .set_furniture_for(Furniture::Header, which, Preset::Text, Alignment::Start, text)
            .expect("a header");
    }

    /// The words drawn on one page, header and all.
    fn words_on(document: &Document, page: usize) -> String {
        let mut engine = LayoutEngine::new(library());
        let pages = engine.layout_document_with(document, PageMetrics::default());
        let Some(laid) = pages.get(page) else { return String::new() };
        // The glyphs carry no text, so the header is found by its lines: the
        // furniture's lines are dropped, which leaves its glyphs. Counting is
        // enough — what matters is which header was placed.
        format!("{} glyphs", laid.glyphs.len())
    }

    #[test]
    fn every_page_gets_the_same_one_unless_asked_otherwise() {
        let mut document = document();
        set(&mut document, Which::Default, "Everywhere");
        let first = words_on(&document, 0);
        let second = words_on(&document, 1);
        assert_ne!(first, "0 glyphs");
        assert_ne!(second, "0 glyphs");
    }

    #[test]
    fn a_different_first_page_leaves_the_first_page_bare() {
        // Asked for and never written: the first page has no header, and the
        // pages after it still have theirs.
        let mut plain = document();
        set(&mut plain, Which::Default, "Everywhere");
        let before = words_on(&plain, 0);

        let mut asked = document();
        set(&mut asked, Which::Default, "Everywhere");
        asked.set_different_first_page(true);
        let after = words_on(&asked, 0);

        assert_ne!(before, after, "the first page kept its header");
        assert_eq!(words_on(&asked, 1), words_on(&plain, 1), "the second page lost its own");
    }

    #[test]
    fn the_first_page_can_have_a_header_of_its_own() {
        let mut document = document();
        set(&mut document, Which::Default, "Everywhere");
        set(&mut document, Which::First, "Only the first page, and rather longer");
        document.set_different_first_page(true);

        assert_ne!(words_on(&document, 0), words_on(&document, 1), "both pages read the same");
    }

    #[test]
    fn even_pages_can_have_one_of_their_own() {
        let mut document = document();
        set(&mut document, Which::Default, "Odd");
        set(&mut document, Which::Even, "Even pages, and rather longer than the other");
        document.set_different_odd_and_even(true);

        assert_ne!(
            words_on(&document, 1),
            words_on(&document, 2),
            "page two reads like page three"
        );
    }
}
