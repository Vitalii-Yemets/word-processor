//! What a window read right to left has to hold to.
//!
//! Drawing it the other way round is only half of it: a button drawn on
//! the right has to be found on the right when the pointer comes down on
//! it, and the text on the page — which is not turned, because an English
//! document does not read backwards in an Arabic window — has to be found
//! where it is drawn too. Those two are what these tests are about, and
//! they are the two that a picture cannot show.

use wp_docx::model::{Block, Body, Paragraph};
use wp_docx::Document;
use wp_layout::FontLibrary;
use wp_shell::{App, Event, Modifiers};

use crate::chrome::Command;
use crate::editor::Editor;
use crate::messages;
use wp_docx::CharacterFormat;

/// The fonts, read once per test.
fn library() -> &'static FontLibrary {
    Box::leak(Box::new(FontLibrary::scan_system()))
}

/// An editor with a line of text in it, drawn once so that everything has
/// a place.
fn editor() -> Editor {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("The quick brown fox jumps over it")));
    let document = Document::create(&body).expect("a document");
    let mut editor = Editor::new(library(), document, None);
    editor.handle(Event::Resized { width: 1400, height: 900 });
    editor.draw(1400, 900);
    editor
}

/// A window to look at both ways round, and one test at a time: which
/// language the program is in belongs to the whole program, so a test that
/// turns the window has to be the only one turning it.
fn in_a_window<R>(work: impl FnOnce(&mut Editor) -> R) -> R {
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _held = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let was = messages::language();
    messages::set_language(messages::ENGLISH);
    let mut editor = editor();
    let out = work(&mut editor);
    messages::set_language(&was);
    out
}

/// Turns the window, and draws it again: everything is put where it goes
/// while it is being drawn, so nothing has moved until it has been.
fn turn(editor: &mut Editor, on: bool) {
    messages::set_language(if on { messages::PSEUDO_MIRRORED } else { messages::ENGLISH });
    editor.draw(1400, 900);
}

#[test]
fn a_button_drawn_on_the_other_side_is_found_on_that_side() {
    const BOLD: Command = Command::Format(CharacterFormat::Bold);
    in_a_window(|editor| {
        // The rectangle is where a menu would hang from the button, so its
        // second number is the button's bottom rather than its top.
        let (left, bottom, width) =
            editor.ribbon_for_test().command_rect(BOLD).expect("Bold is on the ribbon");
        let middle = (left + width / 2.0) as i32;
        let row = (bottom - 8.0) as i32;
        assert_eq!(
            editor.ribbon_for_test().command_at(middle, row),
            Some(BOLD),
            "where it is drawn in a window read the usual way"
        );

        turn(editor, true);
        // The same button, at the same place in the furniture's own
        // coordinates — which is the other side of the window.
        let (left, bottom, width) = editor.ribbon_for_test().command_rect(BOLD).expect("Bold");
        let drawn = 1400 - (left + width / 2.0) as i32;
        assert_eq!(
            editor.ribbon_for_test().command_at(drawn, (bottom - 8.0) as i32),
            Some(BOLD),
            "and where it is drawn in a window read the other way"
        );
        assert_ne!(
            editor.ribbon_for_test().command_at(middle, row),
            Some(BOLD),
            "and it is no longer where it used to be — that place belongs to
             whatever the turned ribbon put there"
        );
    });
}

#[test]
fn the_pane_moves_to_the_other_side_and_answers_there() {
    in_a_window(|editor| {
        editor.show_navigation_for_test(true);
        editor.draw(1400, 900);
        assert!(editor.pane_width_for_test() > 0.0, "the pane is showing");
        assert!(editor.over_pane_for_test(10), "on the left, where it is drawn");
        assert!(!editor.over_pane_for_test(1400 - 10));

        turn(editor, true);
        // The left of the window is now the document; the right is the pane.
        assert!(!editor.over_pane_for_test(10), "nothing of the pane is on the left");
        assert!(editor.over_pane_for_test(1400 - 10), "and all of it is on the right");
    });
}

#[test]
fn a_click_on_the_page_lands_on_the_same_letter_as_its_mirror_image() {
    in_a_window(|editor| {
        // A point a little way into the text of the first line.
        let (page_x, page_y) = editor.page_origin_for_test(0);
        let (x, y) = ((page_x + 160.0) as i32, (page_y + 90.0) as i32);
        let plain = editor.position_for_test(x, y).expect("a place in the text");

        turn(editor, true);
        // The page has moved across; the same point of it is the same
        // place in the text, because what is on the page is not turned.
        let (mirrored_x, mirrored_y) = editor.page_origin_for_test(0);
        let at = editor
            .position_for_test((mirrored_x + 160.0) as i32, (mirrored_y + 90.0) as i32)
            .expect("a place in the text");
        assert_eq!(at, plain, "the same letter, on the other side of the window");
    });
}

#[test]
fn a_press_on_the_page_puts_the_caret_where_the_pointer_is() {
    in_a_window(|editor| {
        turn(editor, true);
        let (page_x, page_y) = editor.page_origin_for_test(0);
        let (x, y) = ((page_x + 160.0) as i32, (page_y + 90.0) as i32);
        let expected = editor.position_for_test(x, y).expect("a place in the text");
        editor.handle(Event::MouseDown { x, y, modifiers: Modifiers::default() });
        assert_eq!(editor.caret_for_test(), expected, "the press landed where it looked");
    });
}
