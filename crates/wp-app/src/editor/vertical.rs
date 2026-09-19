//! Writing down the page: Word's Text Direction on the Layout tab, and the
//! Asian Layout menu on the Home tab.
//!
//! The first is a property of the section: the whole body of it runs down
//! the page, the lines going from right to left, and the layout crate does
//! the turning — see `Frame` there. The two dialogs of the second are about
//! a run: a number set upright across a vertical line (Horizontal in
//! Vertical), and a run set as two half-height lines in one (Two Lines in
//! One). Each dialog has Word's three buttons: OK, Remove and Cancel.

use wp_docx::eastasian::{CombineBrackets, EastAsianLayout};
use wp_docx::model::{RunProperties, TextDirection};
use wp_shell::Response;

use super::dialogs::Asking;
use super::Editor;
use crate::chrome::dialog::{Answer, Button, Dialog, Field};
use crate::chrome::popup::{Choice, Popup};
use crate::chrome::Command;
use crate::messages::t;

/// The button of both dialogs that takes the layout off the run.
pub(super) const REMOVE: &str = "Remove";

/// The directions Word's Text Direction offers a section, in the order its
/// menu lists them. Text reading upwards is not among them: Word takes no
/// notice of it on a section.
pub(crate) const DIRECTIONS: &[(TextDirection, &str)] = &[
    (TextDirection::Horizontal, "Horizontal"),
    (TextDirection::Down, "Vertical"),
    (TextDirection::DownLeftToRight, "Vertical, left to right"),
];

impl Editor {
    /// Opens the Text Direction menu under its button.
    pub(super) fn open_text_direction(&mut self) -> Response {
        if self.close_popup_if(Choice::TextDirection) {
            return Response::Redraw;
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::TextDirectionSection) else {
            return Response::Ignored;
        };
        let now = self.document.text_direction();
        let current = DIRECTIONS.iter().position(|(direction, _)| *direction == now);
        let items = DIRECTIONS.iter().map(|(_, label)| t(label).to_owned()).collect();
        self.popup = Some(Popup::new(Choice::TextDirection, items, current, left, top, 220.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// One direction chosen: the section is written that way from now on.
    pub(super) fn choose_text_direction(&mut self, index: usize) -> Response {
        self.popup = None;
        let Some((direction, label)) = DIRECTIONS.get(index) else { return Response::Redraw };
        let changed = self.document.set_text_direction(*direction);
        self.relayout();
        self.edited(changed, &format!("{}: {}", t("Text direction"), t(label)))
    }

    /// Opens the Asian Layout menu under its button.
    pub(super) fn open_asian_layout(&mut self) -> Response {
        if self.close_popup_if(Choice::AsianLayout) {
            return Response::Redraw;
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::AsianLayout) else {
            return Response::Ignored;
        };
        let items =
            vec![t("Horizontal in Vertical...").to_owned(), t("Two Lines in One...").to_owned()];
        self.popup = Some(Popup::new(Choice::AsianLayout, items, None, left, top, 220.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// One line of it chosen.
    pub(super) fn choose_asian_layout(&mut self, index: usize) -> Response {
        self.popup = None;
        match index {
            0 => self.open_horizontal_in_vertical(),
            1 => self.open_two_lines_in_one(),
            _ => Response::Redraw,
        }
    }

    /// What the selection is set to now, so the dialogs open showing it.
    fn layout_here(&self) -> EastAsianLayout {
        self.document.character_format_here().east_asian_layout
    }

    /// The buttons both dialogs share.
    fn layout_buttons() -> Vec<Button> {
        vec![
            Button { label: "OK".to_owned(), answer: Answer::Accept, default: true },
            Button { label: t(REMOVE).to_owned(), answer: Answer::Named(REMOVE), default: false },
            Button { label: "Cancel".to_owned(), answer: Answer::Cancel, default: false },
        ]
    }

    /// Word's Horizontal in Vertical dialog: one setting, whether the run is
    /// squeezed into the width of one square when it is wider.
    pub(super) fn open_horizontal_in_vertical(&mut self) -> Response {
        if self.document.selection().is_none() {
            return self.report(t("Select the characters to set across the line first"));
        }
        let now = self.layout_here();
        let dialog = Dialog::with_buttons(
            "Horizontal in Vertical",
            vec![Field::Check { label: "Fit in line".to_owned(), on: now.fit_in_line }],
            Self::layout_buttons(),
        );
        self.ask(Asking::HorizontalInVertical, dialog)
    }

    /// The dialog answered.
    pub(super) fn apply_horizontal_in_vertical(
        &mut self,
        dialog: &Dialog,
        remove: bool,
    ) -> Response {
        let layout = EastAsianLayout {
            horizontal_in_vertical: !remove,
            fit_in_line: !remove && dialog.ticked(0),
            ..EastAsianLayout::default()
        };
        self.set_east_asian_layout(layout, "Horizontal in Vertical")
    }

    /// Word's Two Lines in One dialog: which brackets to set round the pair.
    pub(super) fn open_two_lines_in_one(&mut self) -> Response {
        if self.document.selection().is_none() {
            return self.report(t("Select the characters to set as two lines first"));
        }
        let now = self.layout_here();
        let current =
            CombineBrackets::ALL.iter().position(|kind| *kind == now.brackets).unwrap_or(0);
        let dialog = Dialog::with_buttons(
            "Two Lines in One",
            vec![Field::Choice {
                label: "Enclose with characters".to_owned(),
                items: CombineBrackets::ALL.iter().map(|kind| t(kind.label()).to_owned()).collect(),
                current,
            }],
            Self::layout_buttons(),
        );
        self.ask(Asking::TwoLinesInOne, dialog)
    }

    /// The dialog answered.
    pub(super) fn apply_two_lines_in_one(&mut self, dialog: &Dialog, remove: bool) -> Response {
        let brackets = CombineBrackets::ALL.get(dialog.chose(0)).copied().unwrap_or_default();
        let layout = EastAsianLayout {
            two_lines_in_one: !remove,
            brackets: if remove { CombineBrackets::None } else { brackets },
            ..EastAsianLayout::default()
        };
        self.set_east_asian_layout(layout, "Two Lines in One")
    }

    /// Writes one layout onto the selection, in place of whatever it had.
    fn set_east_asian_layout(&mut self, layout: EastAsianLayout, what: &str) -> Response {
        let change = RunProperties { east_asian_layout: Some(layout), ..RunProperties::default() };
        let changed = self.document.set_character_format(&change);
        self.relayout();
        self.edited(changed, what)
    }
}

#[cfg(test)]
mod tests {
    use wp_docx::eastasian::CombineBrackets;
    use wp_docx::model::{Block, Body, Paragraph, TextDirection};
    use wp_docx::{Document, TextPosition};
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event, Key, Modifiers};

    use crate::chrome::dialog::Field;
    use crate::editor::dialogs::Asking;
    use crate::editor::Editor;

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    fn editor(text: &str) -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text(text)));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor
    }

    fn press(editor: &mut Editor, key: Key) {
        editor.handle(Event::KeyDown { key, modifiers: Modifiers::default() });
    }

    #[test]
    fn the_section_is_written_down_the_page_from_the_menu() {
        let mut editor = editor("Down the page");
        editor.choose_text_direction(1);
        assert_eq!(editor.document.text_direction(), TextDirection::Down);
        assert!(
            editor.pages[0].frame.is_turned(),
            "the page was not turned: {:?}",
            editor.pages[0].frame
        );
        assert!(editor.pages[0].lines.iter().all(|line| line.frame.is_sideways()));
        editor.choose_text_direction(0);
        assert_eq!(editor.document.text_direction(), TextDirection::Horizontal);
        assert!(!editor.pages[0].frame.is_turned());
    }

    #[test]
    fn the_arrows_walk_the_column_in_vertical_text() {
        let mut editor = editor("Down the page");
        editor.choose_text_direction(1);
        editor.document.move_caret(TextPosition::new(0, 0), false);
        // Down the page is along the text.
        press(&mut editor, Key::Down);
        assert_eq!(editor.document.caret().offset, 1);
        press(&mut editor, Key::Up);
        assert_eq!(editor.document.caret().offset, 0);
        // And across the page is horizontal text's up and down, which here
        // has nowhere to go: the caret stays put.
        press(&mut editor, Key::Left);
        assert_eq!(editor.document.caret().offset, 0);
    }

    #[test]
    fn two_lines_in_one_is_written_onto_the_selection_and_taken_off_again() {
        let mut editor = editor("abcd");
        editor.document.move_caret(TextPosition::new(0, 0), false);
        editor.document.move_caret(TextPosition::new(0, 4), true);
        editor.choose_asian_layout(1);
        assert_eq!(editor.asking, Some(Asking::TwoLinesInOne));
        let mut dialog = editor.dialog.take().expect("a dialog");
        if let Some(Field::Choice { current, .. }) = dialog.fields.get_mut(0) {
            *current = 2;
        }
        editor.apply_two_lines_in_one(&dialog, false);
        let now = editor.document.character_format_here().east_asian_layout;
        assert!(now.two_lines_in_one);
        assert_eq!(now.brackets, CombineBrackets::Square);

        editor.choose_asian_layout(1);
        let dialog = editor.dialog.take().expect("a dialog");
        editor.apply_two_lines_in_one(&dialog, true);
        let now = editor.document.character_format_here().east_asian_layout;
        assert!(!now.two_lines_in_one);
        assert_eq!(now.brackets, CombineBrackets::None);
    }

    #[test]
    fn horizontal_in_vertical_wants_a_selection() {
        let mut editor = editor("12");
        editor.document.move_caret(TextPosition::new(0, 2), false);
        editor.choose_asian_layout(0);
        assert_eq!(editor.asking, None, "the dialog opened with nothing selected");
        editor.document.move_caret(TextPosition::new(0, 0), false);
        editor.document.move_caret(TextPosition::new(0, 2), true);
        editor.choose_asian_layout(0);
        assert_eq!(editor.asking, Some(Asking::HorizontalInVertical));
        let mut dialog = editor.dialog.take().expect("a dialog");
        if let Some(Field::Check { on, .. }) = dialog.fields.get_mut(0) {
            *on = true;
        }
        editor.apply_horizontal_in_vertical(&dialog, false);
        let now = editor.document.character_format_here().east_asian_layout;
        assert!(now.horizontal_in_vertical && now.fit_in_line);
    }
}
