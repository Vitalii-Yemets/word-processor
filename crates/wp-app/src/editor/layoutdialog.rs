//! Word's Layout dialog: where a drawing sits, how the text keeps out of its
//! way, and how big it is.
//!
//! # Why one dialog with three tabs
//!
//! Because that is what it is in Word, and because the three answer one
//! question between them. A drawing half an inch from the left margin, with the
//! text flowing round it, three inches wide: move any one of those and the
//! other two are what decide whether the result looks right. Word reaches the
//! same dialog from More Layout Options under Position and under Wrap Text,
//! from More Rotation Options under Rotate, and from the Size group's own
//! launcher — four doors into one room.
//!
//! # What is behind it
//!
//! Nothing new. **C27** built the anchor a drawing is placed by, **C36** the
//! angle it is turned to, and dragging a handle already sets its size. This is
//! the dialog, and every box in it reads what the drawing says and changes it.

use wp_docx::anchor::{Anchor, Placement, Relative, Wrap, WrapSide};
use wp_docx::floating::Turned;
use wp_shell::Response;

use crate::chrome::dialog::{Answer, Button, Dialog, Field};
use crate::measure;

use super::dialogs::Asking;
use super::Editor;

// Position.
const TAB_POSITION: usize = 0;
const ROW_HORIZONTAL: usize = 1;
const HORIZONTAL_HOW: usize = 2;
const HORIZONTAL_AT: usize = 3;
const HORIZONTAL_FROM: usize = 4;
const ROW_VERTICAL: usize = 5;
const VERTICAL_HOW: usize = 6;
const VERTICAL_AT: usize = 7;
const VERTICAL_FROM: usize = 8;

// Text Wrapping.
const TAB_WRAPPING: usize = 9;
const WRAPPING_STYLE: usize = 10;
const WRAPPING_SIDE: usize = 11;
const DISTANCE: usize = 12;
const ROW_DISTANCE_DOWN: usize = 13;
const DISTANCE_TOP: usize = 14;
const DISTANCE_BOTTOM: usize = 15;
const ROW_DISTANCE_SIDES: usize = 16;
const DISTANCE_LEFT: usize = 17;
const DISTANCE_RIGHT: usize = 18;

// Size.
const TAB_SIZE: usize = 19;
const ROW_SIZE: usize = 20;
const SIZE_HEIGHT: usize = 21;
const SIZE_WIDTH: usize = 22;
const SCALE: usize = 23;
const ROW_SCALE: usize = 24;
const SCALE_HEIGHT: usize = 25;
const SCALE_WIDTH: usize = 26;
const SIZE_LOCKED: usize = 27;
const SIZE_ROTATION: usize = 28;

/// English Metric Units in one point, which is what the dialog measures in.
const EMU_PER_POINT: f32 = 12_700.0;

/// Twentieths of a point in a point: the unit [`measure`] reads and writes.
const TWIPS_PER_POINT: f32 = 20.0;

/// How a drawing is placed along one axis, as the dialog offers it.
///
/// Word offers four lines of radio buttons for this, two of which — Book
/// layout and Relative position — write the same `Placement` as the other two
/// with different words. These are the two the format actually holds.
const PLACEMENTS: &[(&str, Option<&str>)] = &[
    ("Absolute position", None),
    ("Aligned left or top", Some("left")),
    ("Centred", Some("center")),
    ("Aligned right or bottom", Some("right")),
    ("Inside", Some("inside")),
    ("Outside", Some("outside")),
];

/// What each of those is measured from.
const FRAMES: &[(&str, Relative)] = &[
    ("Margin", Relative::Margin),
    ("Page", Relative::Page),
    ("Column", Relative::Column),
    ("Paragraph", Relative::Paragraph),
    ("Line", Relative::Line),
    ("Character", Relative::Character),
];

/// Word's six wrapping styles, which are five wraps and a flag between them.
const STYLES: &[(&str, Wrap, bool)] = &[
    ("Square", Wrap::Square, false),
    ("Tight", Wrap::Tight, false),
    ("Through", Wrap::Through, false),
    ("Top and bottom", Wrap::TopAndBottom, false),
    ("Behind text", Wrap::None, true),
    ("In front of text", Wrap::None, false),
];

impl Editor {
    /// Opens it on the drawing in hand.
    pub(super) fn open_layout_dialog(&mut self) -> Response {
        let Some(at) = self.drawings_in_hand().first().copied() else {
            return self.report("Click a shape or a picture first");
        };
        let dialog = self.layout_dialog(at);
        self.laying_out = Some(at);
        self.ask(Asking::Layout, dialog)
    }

    /// The dialog itself, filled in from what the drawing says.
    pub(super) fn layout_dialog(&self, at: wp_docx::TextPosition) -> Dialog {
        let anchor = self.document.anchor_at(at).unwrap_or_default();
        let (width, height) = self.document.drawing_size_at(at).unwrap_or((0, 0));
        let turned = self.document.drawing_turn_at(at);

        let unit = self.unit;
        let emu = |value: i64| measure::format(emu_to_twips(value), unit);
        let choice = |label: &str, items: Vec<String>, current: usize| Field::Choice {
            label: label.to_owned(),
            items,
            current,
        };
        let number = |label: &str, value: String, mark: &'static str| Field::Number {
            label: label.to_owned(),
            value,
            unit: mark,
        };

        let names = |list: &[(&str, Option<&str>)]| {
            list.iter().map(|(name, _)| (*name).to_owned()).collect::<Vec<String>>()
        };
        let frames = FRAMES.iter().map(|(name, _)| (*name).to_owned()).collect::<Vec<String>>();

        let (across_how, across_at) = placement_rows(&anchor.horizontal);
        let (down_how, down_at) = placement_rows(&anchor.vertical);

        // A picture's own size, which the Scale boxes are a percentage of.
        let original = self.original_size(at);
        let scale = |now: i64, whole: Option<i64>| match whole.filter(|whole| *whole > 0) {
            Some(whole) => shown_scale(now, whole),
            None => shown_scale(now, now),
        };

        // A drawing in the text has no anchor at all, and Word shows the
        // wrapping style In line with text for it. That style is not here: it
        // is the tick that makes a drawing float, and it belongs to Wrap Text
        // on the ribbon rather than to this dialog. See **C27**.
        let style = STYLES
            .iter()
            .position(|(_, wrap, behind)| *wrap == anchor.wrap && *behind == anchor.behind_text)
            .unwrap_or(0);

        let fields = vec![
            // --- Position --------------------------------------------------
            Field::Tab("Position".to_owned()),
            Field::Columns(3),
            choice("Horizontal", names(PLACEMENTS), across_how),
            number("Position", emu(across_at), unit.mark()),
            choice("relative to", frames.clone(), frame_row(anchor.horizontal_from)),
            Field::Columns(3),
            choice("Vertical", names(PLACEMENTS), down_how),
            number("Position", emu(down_at), unit.mark()),
            choice("relative to", frames, frame_row(anchor.vertical_from)),
            // --- Text Wrapping ---------------------------------------------
            Field::Tab("Text Wrapping".to_owned()),
            choice(
                "Wrapping style",
                STYLES.iter().map(|(name, _, _)| (*name).to_owned()).collect(),
                style,
            ),
            // Which side of the drawing the text runs down. Only the three
            // wraps that let text beside the drawing at all can answer it,
            // and for the others Word shows it greyed; here it says what the
            // file says and is ignored when the wrap gives it nothing to mean.
            choice(
                "Wrap text",
                WrapSide::ALL.iter().map(|side| side.label().to_owned()).collect(),
                WrapSide::ALL.iter().position(|side| *side == anchor.side).unwrap_or(0),
            ),
            Field::Group("Distance from text".to_owned()),
            Field::Columns(2),
            number("Top", emu(anchor.distance.2), unit.mark()),
            number("Bottom", emu(anchor.distance.3), unit.mark()),
            Field::Columns(2),
            number("Left", emu(anchor.distance.0), unit.mark()),
            number("Right", emu(anchor.distance.1), unit.mark()),
            // --- Size ------------------------------------------------------
            Field::Tab("Size".to_owned()),
            Field::Columns(2),
            number("Height", emu(height), unit.mark()),
            number("Width", emu(width), unit.mark()),
            // The same size as a percentage. Of the picture's own size where
            // there is a picture — which is what Word's Scale is a percentage
            // of — and of the size it is now for a shape, which has no
            // original to be a percentage of. See [`Editor::original_size`].
            Field::Group(match original {
                Some(_) => "Scale, of the original picture".to_owned(),
                None => "Scale, of the size it is now".to_owned(),
            }),
            Field::Columns(2),
            number("Height", scale(height, original.map(|(_, tall)| tall)), "%"),
            number("Width", scale(width, original.map(|(wide, _)| wide)), "%"),
            Field::Check { label: "Lock aspect ratio".to_owned(), on: true },
            number("Rotation", format!("{:.0}", degrees_of(turned)), "°"),
        ];

        crate::chrome::dialog::check_rows(
            "Layout",
            &fields,
            &[
                (TAB_POSITION, "a tab"),
                (ROW_HORIZONTAL, "a row"),
                (HORIZONTAL_HOW, "a list"),
                (HORIZONTAL_AT, "a number"),
                (HORIZONTAL_FROM, "a list"),
                (ROW_VERTICAL, "a row"),
                (VERTICAL_HOW, "a list"),
                (VERTICAL_AT, "a number"),
                (VERTICAL_FROM, "a list"),
                (TAB_WRAPPING, "a tab"),
                (WRAPPING_STYLE, "a list"),
                (WRAPPING_SIDE, "a list"),
                (DISTANCE, "a group"),
                (ROW_DISTANCE_DOWN, "a row"),
                (DISTANCE_TOP, "a number"),
                (DISTANCE_BOTTOM, "a number"),
                (ROW_DISTANCE_SIDES, "a row"),
                (DISTANCE_LEFT, "a number"),
                (DISTANCE_RIGHT, "a number"),
                (TAB_SIZE, "a tab"),
                (ROW_SIZE, "a row"),
                (SIZE_HEIGHT, "a number"),
                (SIZE_WIDTH, "a number"),
                (SCALE, "a group"),
                (ROW_SCALE, "a row"),
                (SCALE_HEIGHT, "a number"),
                (SCALE_WIDTH, "a number"),
                (SIZE_LOCKED, "a tick box"),
                (SIZE_ROTATION, "a number"),
            ],
        );

        Dialog::with_buttons(
            "Layout",
            fields,
            vec![
                Button { label: "OK".to_owned(), answer: Answer::Accept, default: true },
                Button { label: "Cancel".to_owned(), answer: Answer::Cancel, default: false },
            ],
        )
        .wide(520.0)
    }

    /// The size a picture was drawn at, before anything resized it.
    ///
    /// Word's Scale is a percentage of this: a photograph brought down to fit
    /// the page is at twenty per cent of itself, and typing 50 there means half
    /// the photograph rather than half of what it is now. Read by decoding the
    /// picture, which is the only place the number exists — the file records
    /// what the drawing is *now*, not what it was.
    ///
    /// Nothing for a shape or a chart: neither has an original to be a
    /// percentage of, and Word greys the boxes out for them.
    fn original_size(&self, at: wp_docx::TextPosition) -> Option<(i64, i64)> {
        let relationship = self.document.picture_relationship_at(at)?;
        let bytes = self.document.embedded_part(&relationship)?;
        let image = wp_image::decode(bytes).ok()?;
        if image.width == 0 || image.height == 0 {
            return None;
        }
        // A picture's own size is its pixels read at the screen's usual
        // ninety-six to the inch, which is the size this program gives one when
        // it is inserted. See [`super::insert`].
        let per_pixel = wp_docx::EMU_PER_INCH / super::DPI as i64;
        Some((image.width as i64 * per_pixel, image.height as i64 * per_pixel))
    }

    /// Puts what the dialog says onto the drawing.
    pub(super) fn apply_layout_dialog(&mut self, dialog: &Dialog) -> Response {
        let Some(at) = self.laying_out.take() else { return Response::Ignored };
        let before = self.document.anchor_at(at).unwrap_or_default();

        let twips = |field: usize| measure::parse(&dialog.said(field), self.unit).unwrap_or(0);
        let emu = |field: usize| twips_to_emu(twips(field));

        let placement = |how: usize, at_field: usize| match PLACEMENTS.get(dialog.chose(how)) {
            Some((_, Some(aligned))) => Placement::Aligned((*aligned).to_owned()),
            _ => Placement::Offset(emu(at_field)),
        };
        let frame = |field: usize| {
            FRAMES.get(dialog.chose(field)).map_or(Relative::Margin, |(_, found)| *found)
        };
        let (wrap, behind) = STYLES
            .get(dialog.chose(WRAPPING_STYLE))
            .map_or((Wrap::Square, false), |(_, wrap, behind)| (*wrap, *behind));
        let side = WrapSide::ALL.get(dialog.chose(WRAPPING_SIDE)).copied().unwrap_or_default();

        let anchor = Anchor {
            wrap,
            side,
            behind_text: behind,
            horizontal_from: frame(HORIZONTAL_FROM),
            horizontal: placement(HORIZONTAL_HOW, HORIZONTAL_AT),
            vertical_from: frame(VERTICAL_FROM),
            vertical: placement(VERTICAL_HOW, VERTICAL_AT),
            distance: (
                emu(DISTANCE_LEFT),
                emu(DISTANCE_RIGHT),
                emu(DISTANCE_TOP),
                emu(DISTANCE_BOTTOM),
            ),
            depth: before.depth,
        };

        // The size: the two boxes, with the lock working the second one out
        // from the first when only one of them was changed. Word's lock does
        // the same, a box at a time, while the dialog is open; ours has one
        // moment to do it in, which is here.
        let (was_width, was_height) = self.document.drawing_size_at(at).unwrap_or((0, 0));
        // Whether a measurement box was typed into, told by what it says
        // rather than by what it parses to: the box was filled in with a
        // rounded measurement — a tenth of an inch shows as 0.12 — and reading
        // that back gives a number a few hundred English Metric Units away
        // from the one it was made from. Every box would then look changed, and
        // the percentages would never get a hearing.
        let untouched = |field: usize, value: i64| {
            dialog.said(field).trim() == measure::format(emu_to_twips(value), self.unit)
        };
        let mut width = if untouched(SIZE_WIDTH, was_width) { was_width } else { emu(SIZE_WIDTH) };
        let mut height =
            if untouched(SIZE_HEIGHT, was_height) { was_height } else { emu(SIZE_HEIGHT) };

        // A percentage changed rather than a measurement is the percentage that
        // is meant: the two boxes say the same thing two ways, and Word keeps
        // them in step as they are typed in. Ours has one moment to settle
        // them, which is here — so the measurement wins where it was changed
        // and the percentage wins where it was not.
        let original = self.original_size(at);
        let whole = |axis: Option<i64>, now: i64| axis.filter(|value| *value > 0).unwrap_or(now);
        // What the box said when the dialog opened, worked out the same way it
        // was filled in: a box still showing that is a box nobody touched.
        // Comparing against a hundred instead would mean a picture shown at
        // twenty-five per cent could never be asked for a hundred.
        let per_cent = |field: usize, whole: i64, now: i64| {
            let said = dialog.said(field).trim().to_owned();
            if said == shown_scale(now, whole) {
                return None;
            }
            said.parse::<f64>()
                .ok()
                .filter(|value| *value > 0.0)
                .map(|value| (whole as f64 * value / 100.0).round() as i64)
        };
        if width == was_width {
            let whole = whole(original.map(|(wide, _)| wide), was_width);
            if let Some(wanted) = per_cent(SCALE_WIDTH, whole, was_width) {
                width = wanted;
            }
        }
        if height == was_height {
            let whole = whole(original.map(|(_, tall)| tall), was_height);
            if let Some(wanted) = per_cent(SCALE_HEIGHT, whole, was_height) {
                height = wanted;
            }
        }

        if dialog.ticked(SIZE_LOCKED) && was_width > 0 && was_height > 0 {
            let ratio = was_height as f64 / was_width as f64;
            if width != was_width && height == was_height {
                height = (width as f64 * ratio).round() as i64;
            } else if height != was_height && width == was_width {
                width = (height as f64 / ratio).round() as i64;
            }
        }

        // The angle in degrees, as a person reads it, turned back into the
        // sixtieths of a thousandth of a degree the format counts in.
        let wanted = dialog.said(SIZE_ROTATION).trim().parse::<f32>().unwrap_or(0.0);
        let rotation =
            ((wanted / 360.0 * Turned::WHOLE as f32).round() as i32).rem_euclid(Turned::WHOLE);
        let turned = Turned { rotation, ..self.document.drawing_turn_at(at) };

        // One gesture: a dialog answered once is one thing to take back, not
        // three. See [`wp_docx::Document::begin_gesture`].
        self.document.begin_gesture();
        let mut changed = self.document.set_anchor_at(at, Some(&anchor));
        if width > 0 && height > 0 {
            changed |= self.document.set_drawing_size_at(at, width, height);
        }
        changed |= self.document.set_drawing_turn_at(at, turned);
        self.document.end_gesture();

        self.relayout();
        self.edited(changed, "Layout")
    }
}

/// A size as a percentage of what it is a percentage of, as the box shows it.
///
/// In one place because two things have to agree about it: the box is filled in
/// with it, and the answer compares what the box says against it to tell a box
/// that was typed into from one that was not.
fn shown_scale(now: i64, whole: i64) -> String {
    if whole <= 0 {
        return "100".to_owned();
    }
    format!("{:.0}", now as f64 / whole as f64 * 100.0)
}

/// Which row of the list a placement is, and the distance to show beside it.
///
/// An aligned drawing has no distance of its own, so the box shows nothing
/// rather than a nought: a drawing centred on the page is not a drawing nought
/// inches from its left edge.
fn placement_rows(placement: &Placement) -> (usize, i64) {
    match placement {
        Placement::Aligned(name) => {
            let row = PLACEMENTS
                .iter()
                .position(|(_, aligned)| *aligned == Some(name.as_str()))
                .unwrap_or(1);
            (row, 0)
        }
        Placement::Offset(emu) => (0, *emu),
    }
}

/// Which row of the list a frame is.
fn frame_row(frame: Relative) -> usize {
    FRAMES.iter().position(|(_, found)| *found == frame).unwrap_or(0)
}

/// The angle as a person reads it: degrees, the way round Word counts them.
fn degrees_of(turned: Turned) -> f32 {
    turned.rotation as f32 / Turned::WHOLE as f32 * 360.0
}

/// English Metric Units as twentieths of a point, which is what the measurement
/// boxes read and write.
fn emu_to_twips(emu: i64) -> i32 {
    ((emu as f32 / EMU_PER_POINT) * TWIPS_PER_POINT).round() as i32
}

fn twips_to_emu(twips: i32) -> i64 {
    ((twips as f32 / TWIPS_PER_POINT) * EMU_PER_POINT).round() as i64
}

#[cfg(test)]
mod tests {
    use wp_docx::anchor::{Placement, Relative, Wrap};
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::{Document, TextPosition};
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event};

    use crate::chrome::dialog::{Answer, Field};

    use super::{Editor, DISTANCE_TOP, HORIZONTAL_AT, HORIZONTAL_FROM, HORIZONTAL_HOW};
    use super::{SCALE_HEIGHT, SCALE_WIDTH, SIZE_HEIGHT, SIZE_LOCKED, SIZE_ROTATION};
    use super::{SIZE_WIDTH, WRAPPING_SIDE, WRAPPING_STYLE};

    /// Types into one of the dialog's measurement boxes.
    fn type_number(editor: &mut Editor, field: usize, text: &str) {
        let dialog = editor.dialog.as_mut().expect("a dialog");
        let Some(Field::Number { value, .. }) = dialog.fields.get_mut(field) else {
            panic!("field {field} is not a number");
        };
        *value = text.to_owned();
    }

    /// Picks a row of one of its lists.
    fn choose(editor: &mut Editor, field: usize, row: usize) {
        let dialog = editor.dialog.as_mut().expect("a dialog");
        let Some(Field::Choice { current, .. }) = dialog.fields.get_mut(field) else {
            panic!("field {field} is not a list");
        };
        *current = row;
    }

    /// Ticks or unticks one of its boxes.
    fn tick(editor: &mut Editor, field: usize, on: bool) {
        let dialog = editor.dialog.as_mut().expect("a dialog");
        let Some(Field::Check { on: found, .. }) = dialog.fields.get_mut(field) else {
            panic!("field {field} is not a tick box");
        };
        *found = on;
    }

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    /// An editor with one floating shape in it, chosen.
    fn editor() -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("Text round the shape")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor.document.set_caret(TextPosition::new(0, 0));
        // Two inches by one, in points, which is what a preset is measured in.
        let shape = wp_docx::shapes::Shape::preset("rect", 144.0, 72.0);
        assert!(editor.document.insert_shape(&shape), "the shape went nowhere");
        editor.relayout();
        let at = editor.document.drawing_place_here().expect("a drawing");
        editor.chosen_drawings = vec![at];
        editor
    }

    #[test]
    fn the_dialog_opens_with_three_tabs_and_what_the_drawing_says() {
        let mut editor = editor();
        editor.open_layout_dialog();
        let dialog = editor.dialog.as_ref().expect("the dialog did not open");
        assert_eq!(dialog.title, "Layout");

        let tabs = dialog
            .fields
            .iter()
            .filter(|field| matches!(field, crate::chrome::dialog::Field::Tab(_)))
            .count();
        assert_eq!(tabs, 3, "Word's Layout dialog has three tabs");
    }

    #[test]
    fn the_dialog_will_not_open_with_nothing_chosen() {
        let mut editor = editor();
        editor.chosen_drawings.clear();
        editor.document.set_caret(TextPosition::new(0, 10));
        editor.open_layout_dialog();
        assert!(editor.dialog.is_none(), "it opened with no drawing in hand");
    }

    #[test]
    fn a_position_typed_into_it_moves_the_drawing() {
        let mut editor = editor();
        editor.open_layout_dialog();
        choose(&mut editor, HORIZONTAL_HOW, 0);
        type_number(&mut editor, HORIZONTAL_AT, "2");
        choose(&mut editor, HORIZONTAL_FROM, 1);
        editor.finish_dialog(Answer::Accept);

        let at = editor.chosen_drawings[0];
        let anchor = editor.document.anchor_at(at).expect("an anchor");
        assert_eq!(anchor.horizontal_from, Relative::Page, "the frame was not taken");
        match anchor.horizontal {
            Placement::Offset(emu) => {
                // Two inches, in English Metric Units.
                assert!((emu - 1_828_800).abs() < 2000, "two inches came out as {emu}");
            }
            other => panic!("the position came out as {other:?}"),
        }
    }

    #[test]
    fn an_alignment_chosen_in_it_replaces_the_position() {
        let mut editor = editor();
        editor.open_layout_dialog();
        // Centred, which is the third row of the list.
        choose(&mut editor, HORIZONTAL_HOW, 2);
        editor.finish_dialog(Answer::Accept);

        let at = editor.chosen_drawings[0];
        let anchor = editor.document.anchor_at(at).expect("an anchor");
        assert_eq!(anchor.horizontal, Placement::Aligned("center".to_owned()));
    }

    #[test]
    fn the_wrapping_style_and_the_room_round_it_are_taken() {
        let mut editor = editor();
        editor.open_layout_dialog();
        // Behind text, which is the fifth row.
        choose(&mut editor, WRAPPING_STYLE, 4);
        type_number(&mut editor, DISTANCE_TOP, "0.5");
        editor.finish_dialog(Answer::Accept);

        let at = editor.chosen_drawings[0];
        let anchor = editor.document.anchor_at(at).expect("an anchor");
        assert_eq!(anchor.wrap, Wrap::None, "the wrap was not taken");
        assert!(anchor.behind_text, "the drawing is not behind the text");
        assert!(
            (anchor.distance.2 - 457_200).abs() < 2000,
            "half an inch came out as {}",
            anchor.distance.2
        );
    }

    #[test]
    fn a_size_typed_into_it_resizes_the_drawing() {
        let mut editor = editor();
        editor.open_layout_dialog();
        tick(&mut editor, SIZE_LOCKED, false);
        type_number(&mut editor, SIZE_WIDTH, "3");
        type_number(&mut editor, SIZE_HEIGHT, "1");
        editor.finish_dialog(Answer::Accept);

        let at = editor.chosen_drawings[0];
        let (width, height) = editor.document.drawing_size_at(at).expect("a size");
        assert!((width - 2_743_200).abs() < 3000, "three inches came out as {width}");
        assert!((height - 914_400).abs() < 3000, "one inch came out as {height}");
    }

    #[test]
    fn the_lock_works_the_other_side_out_from_the_one_that_changed() {
        let mut editor = editor();
        let at = editor.chosen_drawings[0];
        let (was_width, was_height) = editor.document.drawing_size_at(at).expect("a size");
        let ratio = was_height as f64 / was_width as f64;

        editor.open_layout_dialog();
        type_number(&mut editor, SIZE_WIDTH, "1");
        editor.finish_dialog(Answer::Accept);

        let (width, height) = editor.document.drawing_size_at(at).expect("a size");
        let wanted = (width as f64 * ratio).round() as i64;
        assert!(
            (height - wanted).abs() < 5000,
            "the height came out {height} where the ratio wanted {wanted}"
        );
    }

    #[test]
    fn a_rotation_typed_into_it_turns_the_drawing() {
        let mut editor = editor();
        editor.open_layout_dialog();
        type_number(&mut editor, SIZE_ROTATION, "45");
        editor.finish_dialog(Answer::Accept);

        let at = editor.chosen_drawings[0];
        let turned = editor.document.drawing_turn_at(at);
        assert!(
            (super::degrees_of(turned) - 45.0).abs() < 0.5,
            "forty-five degrees came out as {}",
            super::degrees_of(turned)
        );
    }

    #[test]
    fn one_answer_is_one_thing_to_take_back() {
        let mut editor = editor();
        let at = editor.chosen_drawings[0];
        let before = editor.document.drawing_size_at(at).expect("a size");

        editor.open_layout_dialog();
        tick(&mut editor, SIZE_LOCKED, false);
        type_number(&mut editor, SIZE_WIDTH, "4");
        type_number(&mut editor, SIZE_ROTATION, "30");
        choose(&mut editor, WRAPPING_STYLE, 3);
        editor.finish_dialog(Answer::Accept);
        assert_ne!(editor.document.drawing_size_at(at), Some(before), "nothing changed");

        assert!(editor.document.undo(), "there was nothing to undo");
        assert_eq!(
            editor.document.drawing_size_at(at),
            Some(before),
            "one undo did not put the whole answer back"
        );
    }

    #[test]
    fn the_side_the_text_runs_down_is_taken_and_the_page_obeys_it() {
        let mut editor = editor();
        // Square wrapping, so there is text beside the drawing to keep to one
        // side of, and the shape in the middle of the text width.
        editor.open_layout_dialog();
        choose(&mut editor, WRAPPING_STYLE, 0);
        choose(&mut editor, HORIZONTAL_HOW, 2);
        editor.finish_dialog(Answer::Accept);

        let at = editor.chosen_drawings[0];
        assert_eq!(editor.document.anchor_at(at).expect("an anchor").wrap, Wrap::Square);

        // Left only: nothing may be drawn to the right of the shape.
        editor.open_layout_dialog();
        choose(&mut editor, WRAPPING_SIDE, 1);
        editor.finish_dialog(Answer::Accept);
        assert_eq!(
            editor.document.anchor_at(at).expect("an anchor").side,
            wp_docx::anchor::WrapSide::Left,
            "the side was not taken"
        );

        let shape = editor.pages[0].shapes.first().cloned().expect("the shape is on the page");
        let right_edge = shape.x + shape.width;
        let beyond = editor.pages[0]
            .glyphs
            .iter()
            .filter(|glyph| glyph.baseline > shape.y && glyph.baseline < shape.y + shape.height)
            .filter(|glyph| glyph.x >= right_edge)
            .count();
        assert_eq!(beyond, 0, "the text still runs down the right of the shape");
    }

    #[test]
    fn a_percentage_typed_into_the_scale_resizes_the_drawing() {
        // A shape has no original size to be a percentage of, so the
        // percentage is of the size it is now — which is what Word's boxes do
        // for a shape, and what the group's label says.
        let mut editor = editor();
        let at = editor.chosen_drawings[0];
        let (was_width, was_height) = editor.document.drawing_size_at(at).expect("a size");

        editor.open_layout_dialog();
        tick(&mut editor, SIZE_LOCKED, false);
        type_number(&mut editor, SCALE_WIDTH, "50");
        type_number(&mut editor, SCALE_HEIGHT, "50");
        editor.finish_dialog(Answer::Accept);

        let (width, height) = editor.document.drawing_size_at(at).expect("a size");
        assert!((width - was_width / 2).abs() < 3000, "half of {was_width} came out as {width}");
        assert!((height - was_height / 2).abs() < 3000, "and the height as {height}");
    }

    #[test]
    fn a_measurement_wins_over_a_percentage_that_was_not_touched() {
        let mut editor = editor();
        let at = editor.chosen_drawings[0];

        editor.open_layout_dialog();
        tick(&mut editor, SIZE_LOCKED, false);
        type_number(&mut editor, SIZE_WIDTH, "3");
        editor.finish_dialog(Answer::Accept);

        let (width, _) = editor.document.drawing_size_at(at).expect("a size");
        assert!((width - 2_743_200).abs() < 3000, "three inches came out as {width}");
    }

    #[test]
    fn a_picture_is_scaled_from_the_size_it_was_drawn_at() {
        // The other half of the rule: a picture *has* an original, and Word's
        // percentage is of that rather than of what the picture is now.
        let mut editor = editor();
        editor.chosen_drawings.clear();
        let canvas = wp_raster::Canvas::filled(96, 48, wp_raster::Color::BLACK);
        let bytes = wp_raster::encode_png(&canvas);
        let end = editor.document.paragraph_text(0).map_or(0, |text| text.len());
        editor.document.set_caret(TextPosition::new(0, end));
        // Put in at a quarter of its own size, so a scale of 100 has something
        // to put back.
        editor.document.insert_picture(&bytes, "png", 228_600, 114_300).expect("a picture");
        editor.relayout();
        let at = editor.document.drawing_place_here().expect("a drawing");
        editor.chosen_drawings = vec![at];

        editor.open_layout_dialog();
        tick(&mut editor, SIZE_LOCKED, false);
        type_number(&mut editor, SCALE_WIDTH, "100");
        type_number(&mut editor, SCALE_HEIGHT, "100");
        editor.finish_dialog(Answer::Accept);

        // Ninety-six pixels at ninety-six to the inch is one inch.
        let (width, height) = editor.document.drawing_size_at(at).expect("a size");
        assert!((width - 914_400).abs() < 3000, "one inch came out as {width}");
        assert!((height - 457_200).abs() < 3000, "half an inch came out as {height}");
    }

    #[test]
    fn the_drawing_on_the_page_agrees_with_what_was_typed() {
        // The boxes change the file; this is the other half of the promise —
        // that the page changes with it.
        let mut editor = editor();
        let drawn = |editor: &Editor| {
            editor.pages[0].shapes.first().map(|shape| (shape.width, shape.height))
        };
        let before = drawn(&editor).expect("the shape is on the page");

        editor.open_layout_dialog();
        tick(&mut editor, SIZE_LOCKED, false);
        type_number(&mut editor, SIZE_WIDTH, "4");
        type_number(&mut editor, SIZE_HEIGHT, "1");
        editor.finish_dialog(Answer::Accept);

        let after = drawn(&editor).expect("the shape is on the page");
        assert!(after.0 > before.0 * 1.5, "the page still draws it {} wide", after.0);
        assert!(after.1 < before.1 * 1.2, "the height did not follow: {}", after.1);
    }

    #[test]
    fn cancelling_it_changes_nothing() {
        let mut editor = editor();
        let at = editor.chosen_drawings[0];
        let before = (
            editor.document.anchor_at(at),
            editor.document.drawing_size_at(at),
            editor.document.drawing_turn_at(at),
        );

        editor.open_layout_dialog();
        type_number(&mut editor, SIZE_WIDTH, "5");
        editor.finish_dialog(Answer::Cancel);

        assert_eq!(
            (
                editor.document.anchor_at(at),
                editor.document.drawing_size_at(at),
                editor.document.drawing_turn_at(at)
            ),
            before,
            "cancelling changed the drawing"
        );
    }
}
