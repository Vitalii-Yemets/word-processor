//! Word's Rotate menu: turning the chosen drawings a quarter at a time, and
//! mirroring them.
//!
//! # Why a quarter is a number and not a case
//!
//! Because the format counts angles, not quarters. `a:xfrm/@rot` is in
//! sixtieths of a thousandth of a degree — 21,600,000 to the whole turn — and
//! a right angle is a quarter of that like any other angle. So Rotate Right is
//! the same arithmetic the round handle does, with the sweep chosen for it
//! instead of measured from a pointer. See [`wp_docx::floating::Turned`].
//!
//! # Why mirroring is here too
//!
//! Word's menu holds both, and for good reason: `flipH` and `flipV` live on
//! the same transform as the angle, and a drawing that is mirrored and turned
//! is mirrored first. Keeping them apart would mean two commands that had to
//! know about each other.

use wp_docx::floating::Turned;
use wp_shell::Response;

use crate::chrome::{Choice, Command, Popup};

use super::Editor;

/// What one row of the menu does to a drawing's transform.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Rotation {
    /// A quarter turn, clockwise or the other way.
    Quarter(i32),
    /// Turned over top to bottom, or left to right.
    FlipDown,
    FlipAcross,
}

/// The last row of the menu, which is where Word puts the Layout dialog.
pub(super) const MORE: &str = "More Rotation Options…";

/// A quarter of a whole turn, in the unit the format counts in.
const QUARTER: i32 = Turned::WHOLE / 4;

/// The menu, in Word's order.
pub(super) const ROWS: &[(&str, Rotation)] = &[
    ("Rotate Right 90°", Rotation::Quarter(QUARTER)),
    ("Rotate Left 90°", Rotation::Quarter(-QUARTER)),
    ("Flip Vertical", Rotation::FlipDown),
    ("Flip Horizontal", Rotation::FlipAcross),
];

impl Rotation {
    /// The same transform with this done to it.
    fn applied(self, turned: Turned) -> Turned {
        match self {
            Self::Quarter(sixtieths) => turned.turned_by(sixtieths),
            Self::FlipDown => Turned { flipped_down: !turned.flipped_down, ..turned },
            Self::FlipAcross => Turned { flipped_across: !turned.flipped_across, ..turned },
        }
    }
}

impl Editor {
    /// Drops the Rotate menu open.
    pub(super) fn open_rotate(&mut self) -> Response {
        if self.close_popup_if(Choice::RotateObjects) {
            return Response::Redraw;
        }
        if self.drawings_in_hand().is_empty() {
            return self.report("Click a shape or a picture first");
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::RotateObjects) else {
            return Response::Ignored;
        };

        let mut items: Vec<String> = ROWS.iter().map(|(label, _)| (*label).to_owned()).collect();
        // The last row is Word's door to the Layout dialog, where the angle can
        // be typed rather than turned a quarter at a time.
        items.push(MORE.to_owned());
        // None of the four is a state to be in: each does something to whatever
        // angle the drawing already has, so none of them is ticked.
        self.popup = Some(Popup::new(Choice::RotateObjects, items, None, left, top, 220.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Does whichever row was picked.
    pub(super) fn choose_rotate(&mut self, index: usize) -> Response {
        self.popup = None;
        if index == ROWS.len() {
            return self.open_layout_dialog();
        }
        let Some((label, row)) = ROWS.get(index).copied() else { return Response::Ignored };
        self.rotate_drawings(row, label)
    }

    /// Turns or mirrors every chosen drawing.
    ///
    /// Each keeps its own angle: turning two drawings right does not make them
    /// agree, it turns each of them right.
    fn rotate_drawings(&mut self, row: Rotation, label: &str) -> Response {
        let chosen = self.drawings_in_hand();
        if chosen.is_empty() {
            return self.report("Click a shape or a picture first");
        }

        // One command is one thing to undo, however many drawings it turned.
        self.document.begin_gesture();
        let mut changed = false;
        for at in chosen {
            let turned = row.applied(self.document.drawing_turn_at(at));
            changed |= self.document.set_drawing_turn_at(at, turned);
        }
        self.document.end_gesture();

        self.relayout();
        self.edited(changed, label)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quarter_is_a_quarter_of_the_whole_turn() {
        assert_eq!(QUARTER * 4, Turned::WHOLE);
    }

    #[test]
    fn rotating_right_four_times_comes_back_to_where_it_started() {
        let mut turned = Turned::default();
        for _ in 0..4 {
            turned = Rotation::Quarter(QUARTER).applied(turned);
        }
        assert_eq!(turned, Turned::default());
    }

    #[test]
    fn rotating_left_from_straight_goes_the_long_way_round() {
        // Angles are counted from zero upwards, so a quarter to the left is
        // three quarters to the right and not a negative number.
        let turned = Rotation::Quarter(-QUARTER).applied(Turned::default());
        assert_eq!(turned.rotation, QUARTER * 3);
    }

    #[test]
    fn flipping_twice_puts_it_back() {
        let once = Rotation::FlipAcross.applied(Turned::default());
        assert!(once.flipped_across);
        assert_eq!(Rotation::FlipAcross.applied(once), Turned::default());
    }

    #[test]
    fn mirroring_does_not_disturb_the_angle() {
        let turned = Rotation::Quarter(QUARTER).applied(Turned::default());
        let mirrored = Rotation::FlipDown.applied(turned);
        assert_eq!(mirrored.rotation, turned.rotation);
        assert!(mirrored.flipped_down);
    }

    #[test]
    fn the_menu_is_words_four() {
        assert_eq!(ROWS.len(), 4);
        assert_eq!(ROWS[0].0, "Rotate Right 90°");
        assert_eq!(ROWS[3].0, "Flip Horizontal");
    }

    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::{Document, TextPosition};
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event};

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    /// An editor with two floating shapes, both chosen.
    fn editor() -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("Words to flow round them")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });

        for across in [0i64, 1_371_600] {
            let shape = wp_docx::shapes::Shape {
                name: "Box".to_owned(),
                width_emu: 914_400,
                height_emu: 457_200,
                fill: wp_docx::fills::Fill::Solid("4472C4".to_owned()),
                anchor: Some(wp_docx::anchor::Anchor {
                    wrap: wp_docx::anchor::Wrap::None,
                    horizontal: wp_docx::anchor::Placement::Offset(across),
                    ..wp_docx::anchor::Anchor::default()
                }),
                ..wp_docx::shapes::Shape::default()
            };
            editor.document.set_caret(TextPosition::new(0, 0));
            assert!(editor.document.insert_shape(&shape), "the shape went nowhere");
        }
        editor.relayout();

        let all: Vec<TextPosition> =
            editor.drawings_facing().into_iter().map(|drawing| drawing.at).collect();
        for (at, place) in all.into_iter().enumerate() {
            if at == 0 {
                editor.choose_drawing_at(place);
            } else {
                editor.also_choose_drawing_at(place);
            }
        }
        editor
    }

    fn row(label: &str) -> usize {
        ROWS.iter().position(|(name, _)| *name == label).expect("no such row")
    }

    #[test]
    fn the_menu_turns_every_drawing_that_is_chosen() {
        let mut editor = editor();
        let chosen = editor.drawings_in_hand();
        assert_eq!(chosen.len(), 2, "both should be chosen");

        editor.choose_rotate(row("Rotate Right 90°"));
        for at in &chosen {
            assert_eq!(editor.document.drawing_turn_at(*at).rotation, QUARTER);
        }
    }

    #[test]
    fn one_undo_takes_back_a_turn_of_them_all() {
        let mut editor = editor();
        let chosen = editor.drawings_in_hand();
        editor.choose_rotate(row("Rotate Left 90°"));
        editor.document.undo();
        for at in &chosen {
            assert_eq!(editor.document.drawing_turn_at(*at).rotation, 0, "one undo was not enough");
        }
    }

    #[test]
    fn a_turn_survives_being_saved_and_reopened() {
        let mut editor = editor();
        let chosen = editor.drawings_in_hand();
        editor.choose_rotate(row("Rotate Right 90°"));
        editor.choose_rotate(row("Flip Horizontal"));

        let bytes = editor.document.save().expect("saving");
        let reopened = Document::open(&bytes).expect("reopening");
        for at in &chosen {
            let turned = reopened.drawing_turn_at(*at);
            assert_eq!(turned.rotation, QUARTER, "the angle did not survive");
            assert!(turned.flipped_across, "the mirroring did not survive");
        }
    }

    #[test]
    fn a_picture_turns_by_the_menu_too() {
        // A picture is not rebuilt from the model the way a shape is, so it is
        // the one that could quietly do nothing.
        let mut editor = editor();
        let canvas = wp_raster::Canvas::filled(40, 30, wp_raster::Color::BLACK);
        let bytes = wp_raster::encode_png(&canvas);
        let end = editor.document.paragraph_text(0).map_or(0, |text| text.len());
        editor.document.set_caret(TextPosition::new(0, end));
        editor.document.insert_picture(&bytes, "png", 914_400, 685_800).expect("a picture");
        editor.relayout();

        let at = TextPosition::new(0, end);
        editor.choose_drawing_at(at);
        editor.choose_rotate(row("Rotate Right 90°"));
        assert_eq!(editor.document.drawing_turn_at(at).rotation, QUARTER);

        let saved = editor.document.save().expect("saving");
        let reopened = Document::open(&saved).expect("reopening");
        assert_eq!(reopened.drawing_turn_at(at).rotation, QUARTER);
    }

    #[test]
    fn nothing_chosen_is_told_rather_than_ignored() {
        let mut editor = editor();
        editor.drop_chosen_drawing();
        editor.document.set_caret(TextPosition::new(0, 5));
        editor.rotate_drawings(Rotation::Quarter(QUARTER), "Rotate Right 90°");
        assert!(editor.status.contains("shape"), "it said {:?}", editor.status);
    }
}
