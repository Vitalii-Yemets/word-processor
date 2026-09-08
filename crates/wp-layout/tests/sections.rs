//! A document of several sections, laid out on the paper each of them names.

use wp_docx::model::{Block, Body, Paragraph};
use wp_docx::{Document, WORDPROCESSING_NAMESPACE as W};
use wp_layout::{FontLibrary, LayoutEngine, PageMetrics};
use wp_xml::tree::Element;

/// The fonts, read once per test.
fn library() -> &'static FontLibrary {
    Box::leak(Box::new(FontLibrary::scan_system()))
}

/// A4 and A4 turned on its side, in twentieths of a point.
const PORTRAIT: (i32, i32) = (11_906, 16_838);
const LANDSCAPE: (i32, i32) = (16_838, 11_906);

/// A `w:sectPr` for paper of a given size.
fn section_properties((width, height): (i32, i32), kind: &str) -> Element {
    let mut properties = Element::new("w:sectPr", Some(W));

    let mut size = Element::new("w:pgSz", Some(W));
    size.set_namespaced_attribute("w:w", W, &width.to_string());
    size.set_namespaced_attribute("w:h", W, &height.to_string());
    properties.push_element(size);

    let mut start = Element::new("w:type", Some(W));
    start.set_namespaced_attribute("w:val", W, kind);
    properties.push_element(start);

    let mut margins = Element::new("w:pgMar", Some(W));
    for side in ["top", "right", "bottom", "left"] {
        margins.set_namespaced_attribute(&format!("w:{side}"), W, "1440");
    }
    properties.push_element(margins);
    properties
}

/// The element at a path from the root.
fn at_path<'a>(root: &'a mut Element, path: &[usize]) -> &'a mut Element {
    let mut here = root;
    for index in path {
        here = here.children[*index].as_element_mut().expect("an element");
    }
    here
}

/// A document of `count` paragraphs, with a section break written onto the
/// paragraphs named — which is where the format keeps a section's properties.
fn document(
    count: usize,
    breaks: &[(usize, (i32, i32), &str)],
    last: (i32, i32),
    last_kind: &str,
) -> Document {
    let mut body = Body::default();
    for index in 0..count {
        body.blocks.push(Block::Paragraph(Paragraph::text(&format!("Paragraph {index}"))));
    }
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    let mut document = Document::open(&bytes).expect("reopening");

    for (paragraph, size, kind) in breaks {
        let path = wp_docx::position::paragraph_path(&document.tree().root, *paragraph)
            .expect("a paragraph");
        let element = at_path(&mut document.tree_mut().root, &path);
        if element.child(Some(W), "pPr").is_none() {
            element.insert_element(0, Element::new("w:pPr", Some(W)));
        }
        let properties = element.child_mut(Some(W), "pPr").expect("just inserted");
        properties.push_element(section_properties(*size, kind));
    }

    // And the last section's, at the end of the body where the format keeps it.
    let body_element = find_body(&mut document.tree_mut().root).expect("a body");
    body_element.remove_children_named(Some(W), "sectPr");
    body_element.push_element(section_properties(last, last_kind));

    let bytes = document.save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

fn find_body(root: &mut Element) -> Option<&mut Element> {
    if root.is(Some(W), "body") {
        return Some(root);
    }
    root.child_elements_mut().find_map(find_body)
}

fn pages(document: &Document) -> Vec<wp_layout::Page> {
    let mut engine = LayoutEngine::new(library());
    engine.layout_document_with(document, PageMetrics::default())
}

#[test]
fn a_document_with_no_breaks_is_one_section() {
    assert_eq!(document(2, &[], PORTRAIT, "nextPage").sections().len(), 1);
}

#[test]
fn a_break_makes_two_sections() {
    let document = document(2, &[(0, PORTRAIT, "nextPage")], LANDSCAPE, "nextPage");
    let sections = document.sections();
    assert_eq!(sections.len(), 2, "{sections:?}");
    assert_eq!((sections[0].first_block, sections[0].end_block), (0, 1));
    assert_eq!(sections[1].first_block, 1);
}

#[test]
fn each_section_is_printed_on_the_paper_it_names() {
    let document = document(2, &[(0, PORTRAIT, "nextPage")], LANDSCAPE, "nextPage");
    let sections = document.sections();
    assert!(!sections[0].setup.is_landscape());
    assert!(sections[1].setup.is_landscape(), "{:?}", sections[1].setup);
}

#[test]
fn a_landscape_section_is_laid_out_on_landscape_paper() {
    let laid = pages(&document(2, &[(0, PORTRAIT, "nextPage")], LANDSCAPE, "nextPage"));
    assert_eq!(laid.len(), 2, "a break onto a new page should make two pages");
    assert!(laid[0].height > laid[0].width, "the first page is not portrait");
    assert!(laid[1].width > laid[1].height, "the second page is not landscape");
}

#[test]
fn a_break_onto_a_new_page_starts_one() {
    let laid = pages(&document(2, &[(0, PORTRAIT, "nextPage")], PORTRAIT, "nextPage"));
    assert_eq!(laid.len(), 2);
    assert_eq!(laid[0].lines.len(), 1);
    assert_eq!(laid[1].lines.len(), 1);
}

#[test]
fn a_continuous_break_stays_on_the_page() {
    // The type belongs to the section it is written on and says how *that*
    // section begins — so what keeps the second section on the same page is
    // the second section's own type, at the end of the body.
    let laid = pages(&document(2, &[(0, PORTRAIT, "nextPage")], PORTRAIT, "continuous"));
    assert_eq!(laid.len(), 1, "nothing was broken, so there is one page");
    assert_eq!(laid[0].lines.len(), 2);
}

#[test]
fn the_caret_still_counts_paragraphs_across_a_break() {
    // The numbering has to run on through the sections, or a click on the
    // second page would put the caret in the first.
    let laid = pages(&document(2, &[(0, PORTRAIT, "nextPage")], PORTRAIT, "nextPage"));
    assert_eq!(laid[0].lines[0].paragraph, 0);
    assert_eq!(laid[1].lines[0].paragraph, 1, "the second section restarted the numbering");
}

#[test]
fn the_section_a_paragraph_is_in_is_the_one_it_belongs_to() {
    let document = document(2, &[(0, PORTRAIT, "nextPage")], LANDSCAPE, "nextPage");
    assert!(!document.section_of_block(0).setup.is_landscape());
    assert!(document.section_of_block(1).setup.is_landscape());
}

#[test]
fn three_sections_give_three_kinds_of_page() {
    let laid = pages(&document(
        3,
        &[(0, PORTRAIT, "nextPage"), (1, LANDSCAPE, "nextPage")],
        PORTRAIT,
        "nextPage",
    ));
    assert_eq!(laid.len(), 3);
    assert!(laid[0].height > laid[0].width);
    assert!(laid[1].width > laid[1].height);
    assert!(laid[2].height > laid[2].width);
}

#[test]
fn a_document_of_one_section_lays_out_as_it_always_did() {
    // The common case has to be untouched by any of this.
    let laid = pages(&document(3, &[], PORTRAIT, "nextPage"));
    assert_eq!(laid.len(), 1);
    assert_eq!(laid[0].lines.len(), 3);
}

#[test]
fn an_even_page_section_starts_on_an_even_page() {
    // The first section fills page one, so the second already falls on page
    // two and needs nothing put in front of it.
    let laid = pages(&document(2, &[(0, PORTRAIT, "nextPage")], PORTRAIT, "evenPage"));
    assert_eq!(laid.len(), 2);
    assert_eq!(laid[1].lines.len(), 1);
}

#[test]
fn an_odd_page_section_takes_a_blank_page_to_reach_one() {
    // Page two would be even, so a blank page goes in and the section opens on
    // page three — which is how a chapter always starts on a right-hand page.
    let laid = pages(&document(2, &[(0, PORTRAIT, "nextPage")], PORTRAIT, "oddPage"));
    assert_eq!(laid.len(), 3, "no blank page was put in");
    assert!(laid[1].lines.is_empty(), "the page put in is not blank");
    assert_eq!(laid[2].lines.len(), 1);
}
