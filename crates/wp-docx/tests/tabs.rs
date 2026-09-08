//! Tab stops: where a tab lands, written into the paragraph and read back.

use wp_docx::model::{Block, Body, Paragraph, TabAlignment, TabLeader, TabStop};
use wp_docx::{Document, TextPosition};

fn document() -> Document {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("Chapter one\tpage")));
    body.blocks.push(Block::Paragraph(Paragraph::text("Chapter two\tpage")));
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

fn round_trip(document: &Document) -> Document {
    let bytes = document.save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

fn stop(position: i32, alignment: TabAlignment, leader: TabLeader) -> TabStop {
    TabStop { position, alignment, leader }
}

#[test]
fn a_paragraph_starts_with_no_stops_of_its_own() {
    assert!(document().tab_stops_here().is_empty());
}

#[test]
fn a_stop_survives_being_saved_and_read_back() {
    let mut document = document();
    assert!(document.set_tab_stops_here(&[stop(4320, TabAlignment::End, TabLeader::Dot)]));

    let reopened = round_trip(&document);
    let stops = reopened.tab_stops_here();
    assert_eq!(stops.len(), 1, "{stops:?}");
    assert_eq!(stops[0].position, 4320);
    assert_eq!(stops[0].alignment, TabAlignment::End);
    assert_eq!(stops[0].leader, TabLeader::Dot);
}

#[test]
fn every_kind_of_stop_survives_the_trip() {
    for alignment in [
        TabAlignment::Start,
        TabAlignment::Center,
        TabAlignment::End,
        TabAlignment::Decimal,
        TabAlignment::Bar,
    ] {
        let mut document = document();
        document.set_tab_stops_here(&[stop(2880, alignment, TabLeader::None)]);
        assert_eq!(round_trip(&document).tab_stops_here()[0].alignment, alignment, "{alignment:?}");
    }
}

#[test]
fn every_kind_of_leader_survives_the_trip() {
    for leader in [
        TabLeader::None,
        TabLeader::Dot,
        TabLeader::Hyphen,
        TabLeader::Underscore,
        TabLeader::MiddleDot,
    ] {
        let mut document = document();
        document.set_tab_stops_here(&[stop(2880, TabAlignment::Start, leader)]);
        assert_eq!(round_trip(&document).tab_stops_here()[0].leader, leader, "{leader:?}");
    }
}

#[test]
fn the_stops_come_back_in_order_along_the_line() {
    let mut document = document();
    document.set_tab_stops_here(&[
        stop(5000, TabAlignment::Start, TabLeader::None),
        stop(1000, TabAlignment::Start, TabLeader::None),
        stop(3000, TabAlignment::Start, TabLeader::None),
    ]);
    let positions: Vec<i32> =
        round_trip(&document).tab_stops_here().iter().map(|stop| stop.position).collect();
    assert_eq!(positions, [1000, 3000, 5000]);
}

#[test]
fn a_stop_can_be_added_to_the_ones_that_are_there() {
    let mut document = document();
    document.set_tab_stops_here(&[stop(1440, TabAlignment::Start, TabLeader::None)]);
    assert!(document.add_tab_stop_here(stop(2880, TabAlignment::End, TabLeader::Dot)));

    let stops = round_trip(&document).tab_stops_here();
    assert_eq!(stops.len(), 2, "{stops:?}");
    assert_eq!(stops[1].alignment, TabAlignment::End);
}

#[test]
fn adding_one_where_another_stands_replaces_it() {
    let mut document = document();
    document.set_tab_stops_here(&[stop(1440, TabAlignment::Start, TabLeader::None)]);
    document.add_tab_stop_here(stop(1440, TabAlignment::Center, TabLeader::Hyphen));

    let stops = round_trip(&document).tab_stops_here();
    assert_eq!(stops.len(), 1, "{stops:?}");
    assert_eq!(stops[0].alignment, TabAlignment::Center);
}

#[test]
fn a_stop_can_be_taken_away_by_pointing_near_it() {
    let mut document = document();
    document.set_tab_stops_here(&[
        stop(1440, TabAlignment::Start, TabLeader::None),
        stop(4320, TabAlignment::Start, TabLeader::None),
    ]);
    // Aimed at the second one and missing it by a little, as a hand does.
    assert!(document.remove_tab_stop_here(4400, 200));

    let positions: Vec<i32> =
        round_trip(&document).tab_stops_here().iter().map(|stop| stop.position).collect();
    assert_eq!(positions, [1440]);
}

#[test]
fn aiming_at_nothing_takes_nothing_away() {
    let mut document = document();
    document.set_tab_stops_here(&[stop(1440, TabAlignment::Start, TabLeader::None)]);
    assert!(!document.remove_tab_stop_here(9000, 200));
    assert_eq!(round_trip(&document).tab_stops_here().len(), 1);
}

#[test]
fn a_stop_can_be_dragged_to_another_place() {
    let mut document = document();
    document.set_tab_stops_here(&[stop(1440, TabAlignment::End, TabLeader::Dot)]);
    assert!(document.move_tab_stop_here(1440, 5760, 200));

    let stops = round_trip(&document).tab_stops_here();
    assert_eq!(stops[0].position, 5760);
    assert_eq!(stops[0].alignment, TabAlignment::End, "the kind of stop was not carried across");
    assert_eq!(stops[0].leader, TabLeader::Dot);
}

#[test]
fn clearing_them_leaves_the_paragraph_saying_nothing() {
    let mut document = document();
    document.set_tab_stops_here(&[stop(1440, TabAlignment::Start, TabLeader::None)]);
    assert!(document.set_tab_stops_here(&[]));
    assert!(round_trip(&document).tab_stops_here().is_empty());
}

#[test]
fn the_stops_belong_to_the_paragraph_they_were_set_on() {
    let mut document = document();
    document.set_caret(TextPosition::new(0, 0));
    document.set_tab_stops_here(&[stop(1440, TabAlignment::Start, TabLeader::None)]);

    let mut reopened = round_trip(&document);
    reopened.set_caret(TextPosition::new(1, 0));
    assert!(reopened.tab_stops_here().is_empty(), "the other paragraph took them too");
}

#[test]
fn every_paragraph_the_selection_touches_gets_them() {
    let mut document = document();
    document.set_caret(TextPosition::new(0, 0));
    document.extend_selection_to(TextPosition::new(1, 3));
    document.set_tab_stops_here(&[stop(2880, TabAlignment::Center, TabLeader::None)]);

    let mut reopened = round_trip(&document);
    reopened.set_caret(TextPosition::new(1, 0));
    assert_eq!(reopened.tab_stops_here().len(), 1, "the second paragraph missed out");
}

#[test]
fn a_document_that_says_nothing_falls_back_to_half_an_inch() {
    assert_eq!(document().default_tab_width(), 720);
}

#[test]
fn setting_the_stops_can_be_undone() {
    let mut document = document();
    document.set_tab_stops_here(&[stop(1440, TabAlignment::Start, TabLeader::None)]);
    assert!(document.undo());
    assert!(document.tab_stops_here().is_empty());
}
