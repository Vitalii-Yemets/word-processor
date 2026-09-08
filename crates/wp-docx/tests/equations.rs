//! Equations in a document: written, saved, reopened and read back.

use wp_docx::math::{self, Math, MATH_NAMESPACE};
use wp_docx::model::{Block, Body, Paragraph, RunContent};
use wp_docx::{Document, TextPosition};

fn document() -> Document {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("Before after")));

    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    let mut document = Document::open(&bytes).expect("reopening");
    // Between the two words.
    document.set_caret(TextPosition::new(0, 7));
    document
}

fn round_trip(document: &Document) -> Document {
    let bytes = document.save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

fn document_xml(document: &Document) -> String {
    document.package().xml_part("word/document.xml").expect("the document").expect("readable")
}

/// The equation in the first paragraph, if there is one.
fn equation_in(document: &Document) -> Option<Math> {
    let Block::Paragraph(paragraph) = &document.body().blocks[0] else { return None };
    paragraph.runs.iter().find_map(|run| {
        run.content.iter().find_map(|piece| match piece {
            RunContent::Math(math) => Some(math.clone()),
            _ => None,
        })
    })
}

#[test]
fn a_document_has_no_equations_to_begin_with() {
    assert_eq!(equation_in(&document()), None);
}

#[test]
fn an_equation_can_be_put_in_and_reads_back_after_saving() {
    let mut document = document();
    let wanted = math::parse("a/b");
    assert!(document.insert_equation(&wanted));

    assert_eq!(equation_in(&round_trip(&document)), Some(wanted));
}

#[test]
fn every_shape_of_equation_survives_the_round_trip() {
    for typed in ["x^2", "x_1", "sqrt(2)", "(a+b)/2", "\\pi r^2", "a/b/c"] {
        let mut document = document();
        let wanted = math::parse(typed);
        assert!(document.insert_equation(&wanted), "{typed}");
        assert_eq!(equation_in(&round_trip(&document)), Some(wanted), "{typed}");
    }
}

#[test]
fn an_equation_is_written_in_the_namespace_the_standard_gives_it() {
    let mut document = document();
    document.insert_equation(&math::parse("a/b"));

    let part = document_xml(&round_trip(&document));
    assert!(part.contains(&format!("xmlns:m=\"{MATH_NAMESPACE}\"")), "no namespace: {part}");
    assert!(part.contains("<m:oMath"), "no equation: {part}");
    assert!(part.contains("<m:f"), "no fraction: {part}");
}

#[test]
fn an_empty_equation_is_not_put_in() {
    let mut document = document();
    assert!(!document.insert_equation(&math::parse("")));
    assert!(!document_xml(&document).contains("oMath"));
}

#[test]
fn a_document_with_no_equations_does_not_declare_the_namespace() {
    assert!(!document_xml(&round_trip(&document())).contains(MATH_NAMESPACE));
}

#[test]
fn an_equation_stands_in_the_text_as_one_character() {
    let mut document = document();
    document.insert_equation(&math::parse("x^2"));

    // "Before " then the equation then "after": one character between them,
    // whatever the equation is made of.
    let reopened = round_trip(&document);
    let text = reopened.paragraph_text(0).expect("the paragraph");
    assert_eq!(text.chars().count(), "Before after".chars().count() + 1, "got {text:?}");
}

#[test]
fn the_caret_ends_up_after_the_equation() {
    let mut document = document();
    document.insert_equation(&math::parse("x^2"));
    assert_eq!(document.caret(), TextPosition::new(0, 8));
}

#[test]
fn an_equation_does_not_disturb_the_words_round_it() {
    let mut document = document();
    document.insert_equation(&math::parse("a/b"));

    let reopened = round_trip(&document);
    let text = reopened.plain_text();
    assert!(text.starts_with("Before "), "{text:?}");
    assert!(text.ends_with("after"), "{text:?}");
}

#[test]
fn putting_an_equation_in_can_be_undone() {
    let mut document = document();
    document.insert_equation(&math::parse("a/b"));
    assert!(document.undo());
    assert_eq!(equation_in(&document), None);
}

#[test]
fn a_display_equation_written_by_word_is_read_as_one() {
    // Word wraps an equation on a line of its own in `m:oMathPara`. A document
    // that arrives that way must read back as an equation, not as nothing.
    let inner = math::math_element(&math::parse("a/b"), "m");
    let mut wrapper = wp_xml::tree::Element::new("m:oMathPara", Some(MATH_NAMESPACE));
    wrapper.push_element(inner);

    let mut document = document();
    // Straight into the paragraph, beside its runs, which is where Word puts
    // it — and where the ordinary insert would not.
    let path = wp_docx::position::paragraph_path(&document.tree().root, 0).expect("a paragraph");
    let root = &mut document.tree_mut().root;
    let mut at = root;
    for index in &path {
        at = at.children[*index].as_element_mut().expect("an element");
    }
    at.push_element(wrapper);

    assert_eq!(equation_in(&document), Some(math::parse("a/b")));
}
