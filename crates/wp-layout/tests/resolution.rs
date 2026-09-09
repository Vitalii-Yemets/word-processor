//! The same document, laid out for a screen and for a printer.
//!
//! A printer draws six to twelve times finer than a screen. What must not
//! change with it is *where the words go*: a line that ends after "the" on
//! screen ends after "the" on paper, and page seven holds the same paragraph in
//! both. If it did not, print preview would be a picture of a different
//! document from the one that comes out of the printer — which is exactly the
//! fault people used to complain about in word processors.

use wp_docx::model::{Block, Body, Paragraph};
use wp_docx::Document;
use wp_layout::{Device, FontLibrary, LayoutEngine, Page, PageMetrics};

fn library() -> &'static FontLibrary {
    Box::leak(Box::new(FontLibrary::scan_system()))
}

/// A document long enough to fill several pages, with headings, lists and a
/// table, so that more than one part of the layout is asked the question.
fn document() -> Document {
    let mut body = Body::default();
    for number in 0..40 {
        body.blocks.push(Block::Paragraph(
            Paragraph::text(&format!("Heading {number}")).with_style("Heading1"),
        ));
        for _ in 0..3 {
            body.blocks.push(Block::Paragraph(Paragraph::text(
                "Some words that go on for long enough to fill several lines of a page, \
                 so that the line breaking has something to decide and the decision can \
                 be compared between one resolution and another.",
            )));
        }
    }
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

/// Where every line of the document falls: which paragraph, and which stretch
/// of its text. Nothing here is measured in pixels, so the same answer is
/// expected whatever the resolution.
fn breaks(pages: &[Page]) -> Vec<(usize, usize, usize, usize)> {
    pages
        .iter()
        .enumerate()
        .flat_map(|(number, page)| {
            page.lines
                .iter()
                .map(move |line| (number, line.paragraph, line.start_offset, line.end_offset))
        })
        .collect()
}

fn laid_out(device: Device) -> Vec<Page> {
    let mut engine = LayoutEngine::for_device(library(), device);
    engine.layout_document_with(&document(), PageMetrics::default())
}

#[test]
fn a_page_breaks_the_same_way_on_paper_as_on_screen() {
    let screen = laid_out(Device::screen());
    let printer = laid_out(Device::printer(600.0));

    assert!(screen.len() > 3, "the document should fill several pages");
    assert_eq!(printer.len(), screen.len(), "a different number of pages came out");
    assert_eq!(breaks(&printer), breaks(&screen), "the lines fell differently");
}

#[test]
fn the_finest_resolution_a_printer_has_breaks_the_same_way_too() {
    let screen = laid_out(Device::screen());
    let printer = laid_out(Device::printer(1200.0));
    assert_eq!(breaks(&printer), breaks(&screen));
}

#[test]
fn the_page_is_the_paper_it_was_asked_for() {
    // A4 is 8.27 by 11.69 inches, so at 600 dots to the inch it is 4960 by
    // 7015 of them. The page a printer is given has to be that, or the text is
    // drawn at the wrong size on paper however right it looked on screen.
    let pages = laid_out(Device::printer(600.0));
    let page = &pages[0];
    assert!((page.width - 4960.0).abs() < 6.0, "width was {}", page.width);
    assert!((page.height - 7015.0).abs() < 6.0, "height was {}", page.height);
}

#[test]
fn the_image_a_printer_is_given_leaves_out_what_it_cannot_reach() {
    use wp_layout::{Renderer, Unprintable};

    let device = Device::printer(600.0).with_unprintable(Unprintable::all(18.0));
    let pages = laid_out(device);
    let mut renderer = Renderer::new(library());

    let whole = renderer.render(&pages[0], wp_raster::Color::rgb(255, 255, 255));
    let printed = renderer.page_for_device(&pages[0], device, wp_raster::Color::rgb(255, 255, 255));

    // A quarter of an inch off each side, at six hundred dots to the inch, is
    // a hundred and fifty of them.
    assert_eq!(printed.width(), whole.width() - 300);
    assert_eq!(printed.height(), whole.height() - 300);
}

#[test]
fn a_screen_is_given_the_whole_page() {
    use wp_layout::Renderer;

    let pages = laid_out(Device::screen());
    let mut renderer = Renderer::new(library());
    let white = wp_raster::Color::rgb(255, 255, 255);
    let whole = renderer.render(&pages[0], white);
    let shown = renderer.page_for_device(&pages[0], Device::screen(), white);
    assert_eq!((shown.width(), shown.height()), (whole.width(), whole.height()));
}
