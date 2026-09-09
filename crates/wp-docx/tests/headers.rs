//! Headers, footers and the page-number field.

use wp_docx::furniture::{Furniture, Preset};
use wp_docx::model::{Alignment, Block, Body, Paragraph};
use wp_docx::Document;

fn document() -> Document {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("text")));
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

fn round_trip(document: &Document) -> Document {
    let bytes = document.save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

#[test]
fn a_new_document_has_neither() {
    let document = document();
    assert!(document.furniture(Furniture::Header).is_none());
    assert!(document.furniture(Furniture::Footer).is_none());
}

#[test]
fn a_header_can_be_added_and_reads_back() {
    let mut document = document();
    assert!(document
        .set_furniture(Furniture::Header, Preset::Text, Alignment::Center, "Report")
        .expect("setting a header"));

    let reopened = round_trip(&document);
    let header = reopened.furniture(Furniture::Header).expect("a header");
    assert_eq!(header.plain_text(), "Report");
}

#[test]
fn a_footer_is_a_different_part_from_the_header() {
    let mut document = document();
    document
        .set_furniture(Furniture::Header, Preset::Text, Alignment::Center, "Top")
        .expect("a header");
    document
        .set_furniture(Furniture::Footer, Preset::Text, Alignment::Center, "Bottom")
        .expect("a footer");

    let reopened = round_trip(&document);
    assert_eq!(reopened.furniture(Furniture::Header).expect("a header").plain_text(), "Top");
    assert_eq!(reopened.furniture(Furniture::Footer).expect("a footer").plain_text(), "Bottom");
    assert_ne!(
        reopened.furniture_part(Furniture::Header),
        reopened.furniture_part(Furniture::Footer)
    );
}

#[test]
fn a_page_number_is_stored_as_a_field_and_survives_the_round_trip() {
    let mut document = document();
    document
        .set_furniture(Furniture::Footer, Preset::PageNumber, Alignment::Center, "")
        .expect("a footer");

    let reopened = round_trip(&document);
    let footer = reopened.furniture(Furniture::Footer).expect("a footer");
    let Block::Paragraph(paragraph) = &footer.blocks[0] else { panic!("a paragraph") };
    assert_eq!(
        paragraph.runs[0].field.as_deref(),
        Some("PAGE"),
        "the field instruction was lost, leaving only the cached number"
    );
}

#[test]
fn page_of_total_keeps_both_fields_and_the_words_between_them() {
    let mut document = document();
    document
        .set_furniture(Furniture::Footer, Preset::PageOfTotal, Alignment::Center, "")
        .expect("a footer");

    let footer = round_trip(&document).furniture(Furniture::Footer).expect("a footer");
    let Block::Paragraph(paragraph) = &footer.blocks[0] else { panic!("a paragraph") };
    let fields: Vec<Option<&str>> = paragraph.runs.iter().map(|run| run.field.as_deref()).collect();
    assert_eq!(fields, vec![None, Some("PAGE"), None, Some("NUMPAGES")]);
    assert_eq!(footer.plain_text(), "Page 1 of 1");
}

#[test]
fn setting_a_header_twice_does_not_leave_two_parts_behind() {
    let mut document = document();
    document
        .set_furniture(Furniture::Header, Preset::Text, Alignment::Start, "First")
        .expect("a header");
    let first = document.furniture_part(Furniture::Header).expect("a part");

    document
        .set_furniture(Furniture::Header, Preset::Text, Alignment::Start, "Second")
        .expect("a header");
    let second = document.furniture_part(Furniture::Header).expect("a part");

    assert_eq!(first, second, "the second header went into a new part");
    assert_eq!(
        round_trip(&document).furniture(Furniture::Header).expect("a header").plain_text(),
        "Second"
    );
}

#[test]
fn a_header_can_be_taken_off_again() {
    let mut document = document();
    document
        .set_furniture(Furniture::Header, Preset::Text, Alignment::Start, "Gone soon")
        .expect("a header");
    assert!(document
        .set_furniture(Furniture::Header, Preset::None, Alignment::Start, "")
        .expect("removing"));

    assert!(round_trip(&document).furniture(Furniture::Header).is_none());
}

#[test]
fn the_alignment_asked_for_is_the_one_stored() {
    let mut document = document();
    document
        .set_furniture(Furniture::Footer, Preset::PageNumber, Alignment::End, "")
        .expect("a footer");

    let footer = round_trip(&document).furniture(Furniture::Footer).expect("a footer");
    let Block::Paragraph(paragraph) = &footer.blocks[0] else { panic!("a paragraph") };
    assert_eq!(paragraph.properties.alignment, Some(Alignment::End));
}

#[test]
fn the_body_is_untouched_by_any_of_it() {
    let mut document = document();
    document
        .set_furniture(Furniture::Header, Preset::Text, Alignment::Center, "Header")
        .expect("a header");
    document
        .set_furniture(Furniture::Footer, Preset::PageOfTotal, Alignment::Center, "")
        .expect("a footer");
    assert_eq!(round_trip(&document).plain_text(), "text");
}

#[test]
fn how_far_the_furniture_sits_from_the_edge_has_a_default() {
    // 708 twips, which is what this program writes and what Word writes too.
    assert_eq!(document().furniture_distances(), (708, 708));
}

/// The three headers a section can have, and which page gets which.
mod three {
    use wp_docx::furniture::{Furniture, Preset, Which};
    use wp_docx::model::{Alignment, Block, Body, Paragraph};
    use wp_docx::Document;

    fn document() -> Document {
        let mut body = Body::default();
        for index in 0..3 {
            body.blocks.push(Block::Paragraph(Paragraph::text(&format!("Paragraph {index}"))));
        }
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        Document::open(&bytes).expect("reopening")
    }

    fn round_trip(document: &Document) -> Document {
        let bytes = document.save().expect("saving");
        Document::open(&bytes).expect("reopening")
    }

    fn set(document: &mut Document, which: Which, text: &str) {
        document
            .set_furniture_for(Furniture::Header, which, Preset::Text, Alignment::Start, text)
            .expect("a header");
    }

    fn text_of(document: &Document, which: Which) -> String {
        document
            .furniture_of_page(Furniture::Header, 0, which)
            .map(|body| body.blocks.iter().map(Block::plain_text).collect::<Vec<_>>().join(" "))
            .unwrap_or_default()
    }

    #[test]
    fn a_document_asks_for_neither_to_begin_with() {
        let document = document();
        assert!(!document.different_first_page(0));
        assert!(!document.different_odd_and_even());
    }

    #[test]
    fn the_three_are_kept_apart() {
        let mut document = document();
        set(&mut document, Which::Default, "Every page");
        set(&mut document, Which::First, "The first");
        set(&mut document, Which::Even, "The left-hand ones");

        let reopened = round_trip(&document);
        assert!(text_of(&reopened, Which::Default).contains("Every page"));
        assert!(text_of(&reopened, Which::First).contains("The first"));
        assert!(text_of(&reopened, Which::Even).contains("left-hand"));
    }

    #[test]
    fn writing_one_leaves_the_others_alone() {
        // They live side by side in the section, told apart only by their type.
        let mut document = document();
        set(&mut document, Which::Default, "Every page");
        set(&mut document, Which::First, "The first");
        set(&mut document, Which::First, "Changed");

        let reopened = round_trip(&document);
        assert!(text_of(&reopened, Which::Default).contains("Every page"), "the ordinary one went");
        assert!(text_of(&reopened, Which::First).contains("Changed"));
    }

    #[test]
    fn asking_for_a_different_first_page_survives_the_trip() {
        let mut document = document();
        assert!(document.set_different_first_page(true));
        assert!(round_trip(&document).different_first_page(0));
    }

    #[test]
    fn asking_for_different_odd_and_even_pages_survives_the_trip() {
        let mut document = document();
        assert!(document.set_different_odd_and_even(true));
        assert!(round_trip(&document).different_odd_and_even());
    }

    #[test]
    fn the_first_page_takes_its_own_only_when_it_was_asked_for() {
        let mut document = document();
        set(&mut document, Which::Default, "Every page");
        set(&mut document, Which::First, "The first");

        // Nothing asked for: page one is an ordinary page.
        assert_eq!(document.which_for_page(0, true, 1), Which::Default);

        document.set_different_first_page(true);
        assert_eq!(document.which_for_page(0, true, 1), Which::First);
        assert_eq!(document.which_for_page(0, false, 2), Which::Default, "only the first page");
    }

    #[test]
    fn an_even_page_takes_the_even_one_only_when_it_was_asked_for() {
        let mut document = document();
        assert_eq!(document.which_for_page(0, false, 2), Which::Default);
        document.set_different_odd_and_even(true);
        assert_eq!(document.which_for_page(0, false, 2), Which::Even);
        assert_eq!(document.which_for_page(0, false, 3), Which::Default, "an odd page");
    }

    #[test]
    fn the_first_page_of_a_section_wins_over_the_even_one() {
        // Word settles it the same way: page two of a section that begins on an
        // even page shows the first-page header, not the even one.
        let mut document = document();
        document.set_different_first_page(true);
        document.set_different_odd_and_even(true);
        assert_eq!(document.which_for_page(0, true, 2), Which::First);
    }

    #[test]
    fn a_first_page_that_was_asked_for_and_never_written_stays_bare() {
        // Ticking the box and typing nothing leaves the first page empty rather
        // than showing the ordinary header, which is what Word does.
        let mut document = document();
        set(&mut document, Which::Default, "Every page");
        document.set_different_first_page(true);

        let reopened = round_trip(&document);
        assert!(reopened.furniture_of_page(Furniture::Header, 0, Which::First).is_none());
    }

    #[test]
    fn a_section_can_be_told_to_stop_following_the_one_before_it() {
        let mut document = document();
        set(&mut document, Which::Default, "Every page");
        assert!(document.has_own_furniture(Furniture::Header, 0, Which::Default));
        assert!(document.unset_furniture(Furniture::Header, Which::Default));
        assert!(!document.has_own_furniture(Furniture::Header, 0, Which::Default));
    }
}
