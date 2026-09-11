//! The measurement boxes on the ribbon, and typing into them.
//!
//! # Why they have to be typed into
//!
//! Because a measurement is a number a person knows. "Indent this half an inch"
//! is not a thing to find by dragging a marker along a ruler until the number
//! under the pointer says what was wanted; it is a thing to type. Word's Layout
//! tab has four of these boxes and every one of them takes the keyboard.
//!
//! Before this they were drawn and pressing one said "drag the ruler instead",
//! which is a box that looks like a box and is a label.
//!
//! # What is in each
//!
//! The indents in whatever unit Options was set to, and the room above and
//! below the paragraph in points whatever it was set to — which is Word's own
//! division, and the one [`crate::measure`] was written round.
//!
//! # How one behaves
//!
//! Pressing it puts the keyboard in it with what is there ready to be replaced,
//! as pressing a box in Word does. Typing replaces it, Backspace rubs out,
//! Enter applies it and lets go, Escape lets go without applying, Tab applies
//! it and moves to the next box. Pressing anywhere else applies it too: a
//! number typed and then left is a number meant.

use wp_docx::model::ParagraphProperties;
use wp_shell::Response;

use crate::chrome::Command;
use crate::measure;

use super::Editor;

/// The four boxes, in the order Tab walks them.
const BOXES: &[Command] = &[
    Command::HeaderFromTopBox,
    Command::FooterFromBottomBox,
    Command::RowHeightBox,
    Command::ColumnWidthBox,
    Command::IndentLeftBox,
    Command::IndentRightBox,
    Command::SpaceBeforeBox,
    Command::SpaceAfterBox,
];

/// How far one press of a spinner moves an indent: a tenth of an inch, which
/// is Word's step.
const INDENT_STEP: i32 = 144;

/// And how far it moves the room round a paragraph: six points, which is
/// Word's.
const SPACE_STEP: i32 = 120;

/// The most a box will take, so a typed measurement cannot push a paragraph off
/// the paper. Twenty-two inches, the same limit [`measure::parse`] uses.
const LIMIT: i32 = 22 * 1440;

impl Editor {
    /// Whether one of the ribbon's boxes has the keyboard.
    pub(super) fn typing_in_box(&self) -> bool {
        self.ribbon_box.is_some()
    }

    /// Puts the keyboard in one of them.
    ///
    /// What is in the box is kept, not cleared: a person who presses a box
    /// meaning to nudge the number should see the number. It is marked as
    /// untouched, so the first character typed replaces the lot — which is what
    /// a box whose contents are selected does.
    pub(super) fn type_in_box(&mut self, command: Command) -> Response {
        if !BOXES.contains(&command) {
            return Response::Ignored;
        }
        // Moving from one box to another applies the one being left.
        if self.ribbon_box.is_some_and(|(found, _)| found != command) {
            self.finish_box();
        }
        if self.ribbon_box.is_none() {
            // Read before the box is marked as having the keyboard, because
            // what a box shows once it has the keyboard is what has been typed
            // into it — and nothing has been yet.
            let showing = self.toolbar_state().measure(command);
            self.ribbon_box = Some((command, true));
            self.box_text = showing;
        }
        self.needs_redraw = true;
        Response::Redraw
    }

    /// A character typed while a box has the keyboard.
    pub(super) fn type_into_box(&mut self, character: char) -> Response {
        let Some((command, untouched)) = self.ribbon_box else { return Response::Ignored };
        // Only what a measurement can be made of. Anything else is somebody
        // typing into the document by mistake, and is dropped rather than put
        // in a box that would then refuse to be read.
        if !character.is_ascii_digit() && !matches!(character, '.' | ',' | '-' | ' ') {
            return Response::Ignored;
        }

        if untouched {
            self.box_text.clear();
            self.ribbon_box = Some((command, false));
        }
        // Long enough for any measurement anybody types, and no longer: a box
        // that takes a hundred characters is a box that can be scrolled, and
        // there is nothing to scroll it with.
        if self.box_text.chars().count() < 12 {
            self.box_text.push(character);
        }
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Backspace in a box.
    pub(super) fn rub_out_in_box(&mut self) -> Response {
        let Some((command, untouched)) = self.ribbon_box else { return Response::Ignored };
        if untouched {
            self.box_text.clear();
            self.ribbon_box = Some((command, false));
        } else {
            self.box_text.pop();
        }
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Applies what was typed and lets the keyboard go.
    ///
    /// A box holding something that is not a measurement applies nothing: the
    /// paragraph is left as it was and the box goes back to saying what it is.
    pub(super) fn finish_box(&mut self) -> Response {
        let Some((command, untouched)) = self.ribbon_box.take() else { return Response::Ignored };
        self.needs_redraw = true;
        if untouched {
            // Nothing was typed, so there is nothing to apply.
            return Response::Redraw;
        }

        let typed = std::mem::take(&mut self.box_text);
        let Some(twips) = self.read_box(command, &typed) else {
            return self.report("That is not a measurement");
        };
        self.apply_box(command, twips)
    }

    /// Lets go without applying.
    pub(super) fn leave_box(&mut self) -> Response {
        if self.ribbon_box.take().is_none() {
            return Response::Ignored;
        }
        self.box_text.clear();
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Tab: applies this box and puts the keyboard in the next one.
    pub(super) fn next_box(&mut self, forwards: bool) -> Response {
        let Some((command, _)) = self.ribbon_box else { return Response::Ignored };
        let Some(at) = BOXES.iter().position(|found| *found == command) else {
            return Response::Ignored;
        };
        self.finish_box();

        let count = BOXES.len();
        let wanted = if forwards { (at + 1) % count } else { (at + count - 1) % count };
        self.type_in_box(BOXES[wanted])
    }

    /// One press of a spinner, or one notch of the wheel over a box.
    pub(super) fn step_box(&mut self, command: Command, up: bool) -> Response {
        // Whatever was being typed is settled first, so the arrows nudge what
        // the box says rather than what the paragraph said before it.
        if self.ribbon_box.is_some_and(|(found, _)| found == command) {
            self.finish_box();
        }
        let step = if is_indent(command) { INDENT_STEP } else { SPACE_STEP };
        let now = self.box_value(command);
        let wanted = if up { now + step } else { now - step };
        self.apply_box(command, wanted)
    }

    /// What the box says now, in twentieths of a point.
    fn box_value(&self, command: Command) -> i32 {
        let indents = self.document.indents_here();
        let room = self.document.paragraph_format_here();
        match command {
            Command::IndentLeftBox => indents.0,
            Command::IndentRightBox => indents.2,
            Command::SpaceBeforeBox => room.space_before,
            Command::SpaceAfterBox => room.space_after,
            Command::RowHeightBox => self.document.table_row_height().unwrap_or(0),
            Command::ColumnWidthBox => self.document.cell_width().unwrap_or(0),
            Command::HeaderFromTopBox => self.document.furniture_distances().0,
            Command::FooterFromBottomBox => self.document.furniture_distances().1,
            _ => 0,
        }
    }

    /// Reads what was typed into a box.
    ///
    /// An indent is in the unit Options was set to; the room round a paragraph
    /// is in points however that was set. The mark after the number — `cm`,
    /// `pt`, a double prime — is taken off first, so that a box can be edited
    /// without having to remember to keep it.
    fn read_box(&self, command: Command, typed: &str) -> Option<i32> {
        let cleaned: String = typed
            .chars()
            .filter(|character| character.is_ascii_digit() || matches!(character, '.' | ',' | '-'))
            .collect();
        if cleaned.is_empty() {
            return None;
        }

        let unit = if is_indent(command) { self.unit } else { measure::Unit::Points };
        let twips = measure::parse(&cleaned, unit)?;
        Some(twips.clamp(-LIMIT, LIMIT))
    }

    /// Puts a measurement on the paragraph at the caret.
    fn apply_box(&mut self, command: Command, twips: i32) -> Response {
        // The room round a paragraph cannot be negative — there is no such
        // thing as less than no space — and Word clamps it at zero rather than
        // refusing what was typed. Nor can a row be less than no rows tall.
        let twips = if matches!(command, Command::IndentLeftBox | Command::IndentRightBox) {
            twips
        } else {
            twips.max(0)
        };

        // The two on the Table Layout tab are about the table rather than the
        // paragraph, so they are set through the table and not through the
        // paragraph formatting.
        match command {
            Command::RowHeightBox => {
                let exact = self.document.table_row_height_is_exact();
                let changed = self.document.set_table_row_height(Some(twips), exact);
                self.relayout();
                let shown = measure::format(twips, self.unit);
                return self.edited(changed, &format!("Row height {shown}{}", self.unit.mark()));
            }
            Command::ColumnWidthBox => {
                let changed = self.document.set_cell_width(Some(twips));
                self.relayout();
                let shown = measure::format(twips, self.unit);
                return self.edited(changed, &format!("Column width {shown}{}", self.unit.mark()));
            }
            // And these two are about the section: where the header sits in the
            // space above the text and the footer in the space below it.
            Command::HeaderFromTopBox | Command::FooterFromBottomBox => {
                let (header, footer) = self.document.furniture_distances();
                let changed = if command == Command::HeaderFromTopBox {
                    self.document.set_furniture_distances(twips, footer)
                } else {
                    self.document.set_furniture_distances(header, twips)
                };
                self.relayout();
                let shown = measure::format(twips, self.unit);
                return self
                    .edited(changed, &format!("{} {shown}{}", name_of(command), self.unit.mark()));
            }
            _ => {}
        }

        let change = match command {
            Command::IndentLeftBox => {
                ParagraphProperties { indent_start: Some(twips), ..ParagraphProperties::default() }
            }
            Command::IndentRightBox => {
                ParagraphProperties { indent_end: Some(twips), ..ParagraphProperties::default() }
            }
            Command::SpaceBeforeBox => {
                ParagraphProperties { space_before: Some(twips), ..ParagraphProperties::default() }
            }
            Command::SpaceAfterBox => {
                ParagraphProperties { space_after: Some(twips), ..ParagraphProperties::default() }
            }
            _ => return Response::Ignored,
        };

        let changed = self.document.set_paragraph_format(&change);
        let shown = if is_indent(command) {
            format!("{}{}", measure::format(twips, self.unit), self.unit.mark())
        } else {
            format!("{} pt", f64::from(twips) / 20.0)
        };
        self.edited(changed, &format!("{} {shown}", name_of(command)))
    }
}

/// Whether a box holds a length rather than an amount of room.
fn is_indent(command: Command) -> bool {
    // A row's height and a cell's width are lengths too, and follow the same
    // unit as the indents.
    matches!(
        command,
        Command::IndentLeftBox
            | Command::IndentRightBox
            | Command::RowHeightBox
            | Command::ColumnWidthBox
            | Command::HeaderFromTopBox
            | Command::FooterFromBottomBox
    )
}

/// What a box is called, for the strip along the bottom.
fn name_of(command: Command) -> &'static str {
    match command {
        Command::IndentLeftBox => "Indent left",
        Command::IndentRightBox => "Indent right",
        Command::SpaceBeforeBox => "Space before",
        Command::SpaceAfterBox => "Space after",
        Command::RowHeightBox => "Row height",
        Command::ColumnWidthBox => "Column width",
        Command::HeaderFromTopBox => "Header from top",
        Command::FooterFromBottomBox => "Footer from bottom",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::Document;
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event, Key, Modifiers};

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    fn editor() -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("A paragraph to indent")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor
    }

    fn type_into(editor: &mut Editor, text: &str) {
        for character in text.chars() {
            editor.type_into_box(character);
        }
    }

    #[test]
    fn what_is_typed_reaches_the_paragraph() {
        let mut editor = editor();
        editor.type_in_box(Command::IndentLeftBox);
        type_into(&mut editor, "1");
        editor.finish_box();

        // An inch is 1440 twentieths of a point, and inches is what the
        // program starts in.
        assert_eq!(editor.document.indents_here().0, 1440);
        assert!(!editor.typing_in_box(), "the box kept the keyboard");
    }

    #[test]
    fn the_first_character_typed_replaces_what_was_there() {
        // The box opens with its value ready to be replaced, as Word's does.
        let mut editor = editor();
        editor.document.set_paragraph_format(&ParagraphProperties {
            indent_start: Some(1440),
            ..ParagraphProperties::default()
        });

        editor.type_in_box(Command::IndentLeftBox);
        assert_eq!(editor.box_text, "1.00\"", "the box did not open showing the indent");
        type_into(&mut editor, "2");
        assert_eq!(editor.box_text, "2", "what was there was not replaced");
        editor.finish_box();
        assert_eq!(editor.document.indents_here().0, 2880);
    }

    #[test]
    fn escape_leaves_the_paragraph_alone() {
        let mut editor = editor();
        editor.type_in_box(Command::IndentLeftBox);
        type_into(&mut editor, "3");
        editor.leave_box();

        assert_eq!(editor.document.indents_here().0, 0, "escape applied it anyway");
        assert!(!editor.typing_in_box());
    }

    #[test]
    fn nonsense_in_a_box_applies_nothing() {
        let mut editor = editor();
        editor.type_in_box(Command::IndentLeftBox);
        let showing = editor.box_text.clone();
        // Only what a measurement is made of gets in at all, so the box is
        // left saying exactly what it said before.
        type_into(&mut editor, "abc");
        assert_eq!(editor.box_text, showing, "letters went into a measurement box");

        // And a minus sign on its own is a measurement of nothing, so it
        // applies nothing rather than something arbitrary.
        type_into(&mut editor, "-");
        editor.finish_box();
        assert_eq!(editor.document.indents_here().0, 0);
    }

    #[test]
    fn the_mark_after_the_number_need_not_be_kept() {
        // The box shows `1.00"`; editing it to `2.5"` has to work, and so does
        // editing it to `2.5`.
        let mut editor = editor();
        editor.type_in_box(Command::IndentLeftBox);
        type_into(&mut editor, "2.5\"");
        editor.finish_box();
        assert_eq!(editor.document.indents_here().0, 3600);
    }

    #[test]
    fn the_room_round_a_paragraph_is_typed_in_points() {
        let mut editor = editor();
        editor.type_in_box(Command::SpaceAfterBox);
        type_into(&mut editor, "18");
        editor.finish_box();
        // Eighteen points is 360 twentieths of a point.
        assert_eq!(editor.document.paragraph_format_here().space_after, 360);
    }

    #[test]
    fn a_box_in_points_stays_in_points_when_the_unit_changes() {
        // Word shows the room round a paragraph in points whatever the unit
        // setting says, and reads it back the same way.
        let mut editor = editor();
        editor.unit = measure::Unit::Centimetres;
        editor.type_in_box(Command::SpaceBeforeBox);
        type_into(&mut editor, "12");
        editor.finish_box();
        assert_eq!(editor.document.paragraph_format_here().space_before, 240);
    }

    #[test]
    fn an_indent_follows_the_unit_that_was_asked_for() {
        let mut editor = editor();
        editor.unit = measure::Unit::Centimetres;
        editor.type_in_box(Command::IndentLeftBox);
        type_into(&mut editor, "2.54");
        editor.finish_box();
        assert_eq!(editor.document.indents_here().0, 1440, "2.54 cm is an inch");
    }

    #[test]
    fn the_spinner_steps_by_what_word_steps_by() {
        let mut editor = editor();
        editor.step_box(Command::IndentLeftBox, true);
        assert_eq!(editor.document.indents_here().0, INDENT_STEP);
        editor.step_box(Command::IndentLeftBox, false);
        assert_eq!(editor.document.indents_here().0, 0);

        // The room round a paragraph starts at whatever the style says, and
        // the arrow steps from there rather than from nothing — which is what
        // makes it a nudge.
        let before = editor.document.paragraph_format_here().space_after;
        editor.step_box(Command::SpaceAfterBox, true);
        assert_eq!(editor.document.paragraph_format_here().space_after, before + SPACE_STEP);
    }

    #[test]
    fn the_room_round_a_paragraph_never_goes_below_nothing() {
        let mut editor = editor();
        editor.step_box(Command::SpaceBeforeBox, false);
        assert_eq!(editor.document.paragraph_format_here().space_before, 0);
    }

    #[test]
    fn an_indent_may_be_negative_because_word_allows_one() {
        // Text pulled out into the margin is a thing people do on purpose.
        let mut editor = editor();
        editor.type_in_box(Command::IndentLeftBox);
        type_into(&mut editor, "-0.5");
        editor.finish_box();
        assert_eq!(editor.document.indents_here().0, -720);
    }

    #[test]
    fn tab_applies_the_box_and_moves_to_the_next() {
        let mut editor = editor();
        editor.type_in_box(Command::IndentLeftBox);
        type_into(&mut editor, "1");
        editor.next_box(true);

        assert_eq!(editor.document.indents_here().0, 1440, "Tab did not apply it");
        assert_eq!(
            editor.ribbon_box.map(|(command, _)| command),
            Some(Command::IndentRightBox),
            "Tab did not move on"
        );
    }

    #[test]
    fn typing_in_a_box_does_not_type_in_the_document() {
        let mut editor = editor();
        let before = editor.document.plain_text();
        editor.type_in_box(Command::IndentLeftBox);
        editor.handle(Event::Char('5'));

        assert_eq!(editor.document.plain_text(), before, "the document took the keystroke");
        assert_eq!(editor.box_text, "5");
    }

    #[test]
    fn escape_gives_the_keyboard_back_to_the_document() {
        let mut editor = editor();
        editor.type_in_box(Command::IndentLeftBox);
        editor.handle(Event::KeyDown { key: Key::Escape, modifiers: Modifiers::default() });
        assert!(!editor.typing_in_box());

        editor.handle(Event::Char('x'));
        assert!(editor.document.plain_text().contains('x'), "the document did not get it back");
    }

    #[test]
    fn the_header_distance_is_typed_and_reaches_the_section() {
        let mut editor = editor();
        editor.type_in_box(Command::HeaderFromTopBox);
        type_into(&mut editor, "1");
        editor.finish_box();

        assert_eq!(editor.document.furniture_distances().0, 1440);
    }

    #[test]
    fn the_two_distances_are_set_apart_from_one_another() {
        // They share one element in the file — `w:pgMar` — so writing one must
        // not take the other with it.
        let mut editor = editor();
        editor.type_in_box(Command::FooterFromBottomBox);
        type_into(&mut editor, "0.5");
        editor.finish_box();

        editor.type_in_box(Command::HeaderFromTopBox);
        type_into(&mut editor, "2");
        editor.finish_box();

        assert_eq!(editor.document.furniture_distances(), (2880, 720));
    }

    #[test]
    fn neither_distance_goes_below_nothing() {
        let mut editor = editor();
        editor.type_in_box(Command::HeaderFromTopBox);
        type_into(&mut editor, "-3");
        editor.finish_box();
        assert_eq!(editor.document.furniture_distances().0, 0);
    }
}
