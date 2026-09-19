//! An engine given its pages back must give the pages a new engine gives.
//!
//! The engine keeps the pages before a change and the pages after it once
//! the pagination lands where it landed before, and lays out only what is
//! between. That is what makes a keystroke cost the same on a thousand pages
//! as on ten — and it is exactly the sort of thing that goes wrong quietly:
//! a page kept that should have moved, a list counter carried over from the
//! wrong item, a footnote's room read from the wrong page. So every test
//! here says the same thing: whatever the engine has been through, and
//! whatever pages it was given back, its pages are the pages a new engine
//! gives from the same document. And where the change is small, that it
//! placed only a few blocks to get there.

use wp_docx::model::{Block, Body, Paragraph, Run, Table, TableRow};
use wp_docx::{Document, TextPosition};
use wp_layout::{FontLibrary, LayoutEngine, Page, PageMetrics};

fn library() -> &'static FontLibrary {
    Box::leak(Box::new(FontLibrary::scan_system()))
}

const WORDS: &str = "Some words that go on for long enough to fill several lines of a page, so that \
                     the line breaking has something to decide and a change to one paragraph can be \
                     seen in the ones after it.";

/// A document of several pages: headings that keep with the next paragraph,
/// numbered lists, and a table.
fn document() -> Document {
    Document::open(&Document::create(&body()).expect("a document").save().expect("saving"))
        .expect("reopening")
}

fn body() -> Body {
    let mut body = Body::default();
    for number in 0..24 {
        body.blocks.push(Block::Paragraph(
            Paragraph::text(&format!("Heading {number}")).with_style("Heading1"),
        ));
        for _ in 0..3 {
            body.blocks.push(Block::Paragraph(Paragraph::text(WORDS)));
        }
        if number % 4 == 1 {
            for item in 0..3 {
                body.blocks.push(Block::Paragraph(
                    Paragraph::text(&format!("Item {item} of list {number}"))
                        .in_list(wp_docx::NUMBERED_LIST, 0),
                ));
            }
        }
        if number % 6 == 2 {
            body.blocks.push(Block::Table(Box::new(Table::from_rows(vec![
                TableRow::text(&["one", "two", "three"]),
                TableRow::text(&[WORDS, "five", "six"]),
            ]))));
        }
    }
    body
}

/// The pages a brand-new engine gives for a document.
fn fresh(document: &Document) -> Vec<Page> {
    LayoutEngine::new(library()).layout_document_with(document, PageMetrics::default())
}

/// Lays the document out, hands the pages back after the edit, and holds
/// what comes back to what a new engine gives. Says how many blocks the
/// engine placed to get there.
fn again(document: &mut Document, edit: impl FnOnce(&mut Document)) -> usize {
    let mut engine = LayoutEngine::new(library());
    let pages = engine.layout_document_with(document, PageMetrics::default());
    assert!(pages.len() > 3, "the document should fill several pages");
    edit(document);
    let pages = engine.layout_document_again(document, PageMetrics::default(), pages);
    assert_eq!(pages, fresh(document), "the pages given back were not laid out right");
    engine.blocks_placed()
}

/// The paragraph in the middle of the document, and one near the end.
fn middle(document: &Document) -> usize {
    document.paragraph_count() / 2
}

#[test]
fn nothing_changed_and_nothing_is_placed() {
    let mut document = document();
    let placed = again(&mut document, |_| {});
    assert_eq!(placed, 0);
}

#[test]
fn a_word_typed_into_the_middle_places_a_handful_of_blocks() {
    let mut document = document();
    let at = middle(&document);
    let placed = again(&mut document, |document| {
        document.insert_text(TextPosition::new(at, 5), "typed ");
    });
    let total = body().blocks.len();
    assert!(placed < total / 4, "placed {placed} of {total} blocks for one word");
}

#[test]
fn a_paragraph_split_moves_every_number_after_it() {
    let mut document = document();
    let at = middle(&document);
    again(&mut document, |document| {
        document.set_caret(TextPosition::new(at, 10));
        document.type_text("\n");
    });
}

#[test]
fn a_paragraph_deleted_moves_every_number_after_it() {
    let mut document = document();
    let at = middle(&document);
    again(&mut document, |document| {
        let length = document.paragraph_text(at).map_or(0, |text| text.len());
        document.set_selections(&[(TextPosition::new(at, 0), TextPosition::new(at + 1, 0))]);
        assert!(length > 0);
        document.delete_selection();
    });
}

#[test]
fn a_change_near_the_end_keeps_the_pages_before_it() {
    let mut document = document();
    let at = document.paragraph_count() - 3;
    let placed = again(&mut document, |document| {
        document.insert_text(TextPosition::new(at, 0), "At the end. ");
    });
    let total = body().blocks.len();
    assert!(placed < total / 4, "placed {placed} of {total} blocks");
}

#[test]
fn a_change_at_the_very_start_is_laid_out_from_the_start() {
    let mut document = document();
    again(&mut document, |document| {
        document.insert_text(TextPosition::new(0, 0), "First. ");
    });
}

#[test]
fn a_list_item_added_renumbers_the_items_after_it_and_no_others() {
    let mut document = document();
    // The first list begins after heading 1 and its three paragraphs.
    let first_item = 4 + 4;
    again(&mut document, |document| {
        document.set_caret(TextPosition::new(first_item, 0));
        document.type_text("Zero\n");
    });
}

#[test]
fn a_heading_that_keeps_with_the_next_is_placed_with_it() {
    let mut document = document();
    // Enough text before a heading to push it to the foot of a page, where
    // keeping with the next moves it over.
    let at = 4 * 5;
    again(&mut document, |document| {
        for _ in 0..3 {
            document.insert_text(TextPosition::new(at - 1, 0), WORDS);
        }
    });
}

#[test]
fn a_change_before_a_table_keeps_the_table_where_it_lands() {
    let mut document = document();
    // The first table follows heading 2 and its three paragraphs.
    let before_table = 4 * 3 - 1 + 3;
    again(&mut document, |document| {
        document.insert_text(TextPosition::new(before_table, 0), "Before the table. ");
    });
}

#[test]
fn a_change_inside_a_table_is_placed_with_the_table() {
    let mut document = document();
    let before_table = 4 * 3 - 1 + 3;
    let cell = before_table + 1;
    again(&mut document, |document| {
        document.insert_text(TextPosition::new(cell, 0), "More in the cell. ");
    });
}

#[test]
fn a_change_that_adds_a_page_moves_every_page_after_it() {
    let mut document = document();
    let at = middle(&document);
    let before = fresh(&document).len();
    again(&mut document, |document| {
        for _ in 0..12 {
            document.insert_text(TextPosition::new(at, 0), WORDS);
        }
    });
    assert!(fresh(&document).len() > before, "the edit was meant to add a page");
}

#[test]
fn a_change_that_takes_a_page_away_moves_every_page_after_it() {
    let mut document = document();
    let before = fresh(&document).len();
    again(&mut document, |document| {
        let from = middle(document);
        document.set_selections(&[(TextPosition::new(from, 0), TextPosition::new(from + 16, 0))]);
        document.delete_selection();
    });
    assert!(fresh(&document).len() < before, "the edit was meant to take a page away");
}

#[test]
fn one_change_after_another_after_another() {
    let mut document = document();
    let mut engine = LayoutEngine::new(library());
    let mut pages = engine.layout_document_with(&document, PageMetrics::default());
    for step in 0..12 {
        let at = (step * 7) % document.paragraph_count();
        let changed = match step % 4 {
            0 => document.insert_text(TextPosition::new(at, 0), "More. "),
            1 => {
                document.set_caret(TextPosition::new(at, 0));
                document.type_text("\n")
            }
            2 => {
                document
                    .set_selections(&[(TextPosition::new(at, 0), TextPosition::new(at + 1, 0))]);
                document.delete_selection()
            }
            _ => document.insert_text(TextPosition::new(at, 0), WORDS),
        };
        assert!(changed, "step {step} changed nothing");
        pages = engine.layout_document_again(&document, PageMetrics::default(), pages);
        assert_eq!(pages, fresh(&document), "step {step} came out wrong");
    }
}

#[test]
fn pages_from_another_engine_are_not_trusted() {
    let mut document = document();
    let mut engine = LayoutEngine::new(library());
    engine.layout_document_with(&document, PageMetrics::default());
    let strangers = fresh(&document);

    let at = middle(&document);
    document.insert_text(TextPosition::new(at, 0), "typed ");
    let pages = engine.layout_document_again(&document, PageMetrics::default(), strangers);
    assert_eq!(pages, fresh(&document));
    assert!(engine.blocks_placed() >= body().blocks.len(), "it took a stranger's pages");
}

#[test]
fn stale_pages_from_an_earlier_pass_are_not_trusted() {
    let mut document = document();
    let mut engine = LayoutEngine::new(library());
    let old = engine.layout_document_with(&document, PageMetrics::default());
    let at = middle(&document);
    document.insert_text(TextPosition::new(at, 0), "one ");
    engine.layout_document_again(&document, PageMetrics::default(), old.clone());
    document.insert_text(TextPosition::new(at, 0), "two ");
    let pages = engine.layout_document_again(&document, PageMetrics::default(), old);
    assert_eq!(pages, fresh(&document));
    assert!(engine.blocks_placed() >= body().blocks.len(), "it took the stale pages");
}

#[test]
fn a_style_edited_changes_every_paragraph_in_it() {
    let mut document = document();
    again(&mut document, |document| {
        let bigger =
            wp_docx::model::RunProperties { size_half_points: Some(36), ..Default::default() };
        assert!(document.set_default_character_format(&bigger));
    });
}

#[test]
fn a_footnote_keeps_its_room_when_the_text_before_it_changes() {
    let mut document = document();
    let at = middle(&document);
    document.set_caret(TextPosition::new(at + 2, 3));
    document.add_note(wp_docx::notes::Kind::Footnote, "A note at the foot.").expect("a note");
    document.set_caret(TextPosition::new(at + 30, 3));
    document.add_note(wp_docx::notes::Kind::Footnote, "Another, later.").expect("a note");
    again(&mut document, |document| {
        document.insert_text(TextPosition::new(at - 3, 0), "Before the notes. ");
    });
}

#[test]
fn a_footnote_pushed_onto_the_next_page_takes_its_room_with_it() {
    let mut document = document();
    let at = middle(&document);
    document.set_caret(TextPosition::new(at + 2, 3));
    document.add_note(wp_docx::notes::Kind::Footnote, "A note at the foot.").expect("a note");
    again(&mut document, |document| {
        for _ in 0..8 {
            document.insert_text(TextPosition::new(at, 0), WORDS);
        }
    });
}

#[test]
fn a_page_reference_says_the_new_page_when_its_bookmark_moves() {
    let mut document = document();
    let at = middle(&document);
    document.set_selections(&[(TextPosition::new(at + 20, 0), TextPosition::new(at + 20, 7))]);
    assert!(document.add_bookmark("later"));
    document.set_caret(TextPosition::new(1, 0));
    assert!(document.insert_field("PAGEREF later", "?"));
    let placed = again(&mut document, |document| {
        for _ in 0..12 {
            document.insert_text(TextPosition::new(at, 0), WORDS);
        }
    });
    assert!(placed > 0);
}

#[test]
fn a_sequence_field_counts_the_figure_added_before_it() {
    let mut document = document();
    let at = middle(&document);
    document.set_caret(TextPosition::new(at + 20, 0));
    assert!(document.insert_field("SEQ Figure", "1"));
    document.set_caret(TextPosition::new(at + 30, 0));
    assert!(document.insert_field("SEQ Figure", "2"));
    again(&mut document, |document| {
        document.set_caret(TextPosition::new(at, 0));
        assert!(document.insert_field("SEQ Figure", "0"));
    });
}

#[test]
fn a_new_section_on_a_new_page_is_laid_out_from_where_it_begins() {
    let mut document = document();
    let at = middle(&document);
    again(&mut document, |document| {
        document.set_caret(TextPosition::new(at, 0));
        assert!(document.insert_section_break(wp_docx::sections::Start::NextPage));
    });
}

#[test]
fn a_change_before_a_section_break_keeps_the_section_after_it() {
    let mut document = document();
    let at = middle(&document);
    document.set_caret(TextPosition::new(at, 0));
    assert!(document.insert_section_break(wp_docx::sections::Start::NextPage));
    let placed = again(&mut document, |document| {
        document.insert_text(TextPosition::new(at - 6, 0), "Before the break. ");
    });
    let total = body().blocks.len();
    assert!(placed < total / 4, "placed {placed} of {total} blocks");
}

#[test]
fn a_change_before_an_odd_page_section_that_adds_a_page_moves_the_blank_page() {
    let mut document = document();
    let at = middle(&document);
    document.set_caret(TextPosition::new(at + 12, 0));
    assert!(document.insert_section_break(wp_docx::sections::Start::OddPage));
    again(&mut document, |document| {
        for _ in 0..12 {
            document.insert_text(TextPosition::new(at, 0), WORDS);
        }
    });
}

#[test]
fn the_resolution_changed_between_the_passes_lays_everything_out_again() {
    let mut document = document();
    let mut engine = LayoutEngine::new(library());
    let pages = engine.layout_document_with(&document, PageMetrics::default());
    engine.set_dpi(144.0);
    let at = middle(&document);
    document.insert_text(TextPosition::new(at, 0), "typed ");
    let pages = engine.layout_document_again(&document, PageMetrics::default(), pages);
    let expected = LayoutEngine::new(library())
        .with_dpi(144.0)
        .layout_document_with(&document, PageMetrics::default());
    assert_eq!(pages, expected);
}

#[test]
fn a_document_with_a_header_gets_the_header_on_the_pages_kept() {
    let mut document = document();
    assert!(document
        .set_furniture(
            wp_docx::furniture::Furniture::Header,
            wp_docx::furniture::Preset::PageOfTotal,
            wp_docx::model::Alignment::Center,
            "",
        )
        .expect("a header"));
    let at = middle(&document);
    let placed = again(&mut document, |document| {
        document.insert_text(TextPosition::new(at, 0), "typed ");
    });
    let total = body().blocks.len();
    assert!(placed < total / 4, "placed {placed} of {total} blocks");
}

#[test]
fn a_run_of_text_replaced_by_the_same_length_stops_on_the_same_page() {
    let mut document = document();
    let at = middle(&document);
    again(&mut document, |document| {
        document.set_selections(&[(TextPosition::new(at, 0), TextPosition::new(at, 4))]);
        document.type_text("Many");
    });
}

#[test]
fn a_field_paragraph_is_still_answered_afresh() {
    let mut body = Body::default();
    for _ in 0..40 {
        body.blocks.push(Block::Paragraph(Paragraph::text(WORDS)));
    }
    body.blocks.push(Block::Paragraph(Paragraph::from_runs(vec![Run::field("PAGE", "1")])));
    for _ in 0..40 {
        body.blocks.push(Block::Paragraph(Paragraph::text(WORDS)));
    }
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    let mut document = Document::open(&bytes).expect("reopening");
    again(&mut document, |document| {
        for _ in 0..12 {
            document.insert_text(TextPosition::new(0, 0), WORDS);
        }
    });
}
