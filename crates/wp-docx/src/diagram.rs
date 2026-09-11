//! Diagrams: a handful of boxes arranged to say how things relate.
//!
//! # What Word's SmartArt is, and what this is
//!
//! Word stores SmartArt as three parts of its own — the data, the layout rules
//! and the drawing — and rebuilds the picture from the rules whenever the text
//! or the theme changes. That is a small typesetting engine for diagrams, and
//! it is not what a person pressing the button wants: they want four boxes with
//! arrows between them.
//!
//! So this writes the boxes. Each item of the diagram is an ordinary shape with
//! its words inside, placed where the arrangement puts it. Word opens it as a
//! group of shapes, which means every box can be moved, recoloured and retyped
//! there — but it is not SmartArt, and changing the text will not re-lay it
//! out. That is the trade, and it is stated here rather than hidden.
//!
//! # Why the sizes are worked out from the room available
//!
//! A diagram wider than the text is a diagram half off the page. The caller
//! passes how much room there is — it is the one that knows the margins — and
//! the boxes are divided out of it.

use crate::anchor::{Anchor, Placement, Relative, Wrap};
use crate::model::{Alignment, Paragraph, ParagraphProperties, Run, RunContent, RunProperties};
use crate::shapes::Shape;
use crate::{Document, EMU_PER_INCH};

/// How the boxes are arranged.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Arrangement {
    /// One box after another across the page, with arrows between them.
    #[default]
    Process,
    /// One box under another, the whole width of the text.
    List,
    /// The first box over the rest, which fan out beneath it.
    Hierarchy,
}

impl Arrangement {
    pub const ALL: &'static [Self] = &[Self::Process, Self::List, Self::Hierarchy];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Process => "Process",
            Self::List => "Vertical list",
            Self::Hierarchy => "Hierarchy",
        }
    }
}

/// How tall a box is.
const BOX_HEIGHT: i64 = EMU_PER_INCH * 7 / 10;
/// And a box in a stacked list, which holds a line and no more.
const LIST_HEIGHT: i64 = EMU_PER_INCH / 2;
/// The room between one box and the next.
const GAP: i64 = EMU_PER_INCH / 4;
/// How wide the arrow between two boxes of a process is.
const ARROW: i64 = EMU_PER_INCH / 5;

impl Document {
    /// Puts a diagram of `items` at the caret, inside `width_emu` of room.
    ///
    /// Returns how many shapes were drawn, which is zero when there is nothing
    /// to draw one of.
    pub fn insert_diagram(
        &mut self,
        arrangement: Arrangement,
        items: &[String],
        width_emu: i64,
    ) -> usize {
        let items: Vec<&String> = items.iter().filter(|item| !item.trim().is_empty()).collect();
        if items.is_empty() || width_emu <= 0 {
            return 0;
        }

        let fill = self.theme().color(crate::theme::Slot::Accent1);
        let shapes = arrange(arrangement, &items, width_emu, &fill);
        let drawn = shapes.len();

        // One gesture: a diagram is several drawings and one undo should take
        // the whole of it back.
        self.begin_gesture();
        for shape in &shapes {
            self.insert_shape(shape);
        }
        self.end_gesture();
        drawn
    }
}

/// Works out where every box of a diagram goes.
#[must_use]
fn arrange(arrangement: Arrangement, items: &[&String], width: i64, fill: &str) -> Vec<Shape> {
    match arrangement {
        Arrangement::Process => process(items, width, fill),
        Arrangement::List => list(items, width, fill),
        Arrangement::Hierarchy => hierarchy(items, width, fill),
    }
}

/// Boxes across the page with arrows between them.
fn process(items: &[&String], width: i64, fill: &str) -> Vec<Shape> {
    let count = items.len() as i64;
    // The arrows take room out of the row before the boxes are divided up.
    let arrows = (count - 1).max(0) * (ARROW + GAP);
    let box_width = ((width - arrows) / count).max(EMU_PER_INCH / 2);

    let mut shapes = Vec::new();
    let mut x = 0i64;
    for (index, item) in items.iter().enumerate() {
        shapes.push(boxed("roundRect", item, x, 0, box_width, BOX_HEIGHT, fill));
        x += box_width;
        if index + 1 < items.len() {
            // The arrow sits in the middle of the gap, half the height of a box
            // so it points along the row rather than filling it.
            let arrow_top = (BOX_HEIGHT - BOX_HEIGHT / 3) / 2;
            shapes.push(plain("rightArrow", x + GAP / 2, arrow_top, ARROW, BOX_HEIGHT / 3, fill));
            x += ARROW + GAP;
        }
    }
    shapes
}

/// Boxes one under another.
fn list(items: &[&String], width: i64, fill: &str) -> Vec<Shape> {
    let mut shapes = Vec::new();
    let mut y = 0i64;
    for item in items {
        shapes.push(boxed("roundRect", item, 0, y, width, LIST_HEIGHT, fill));
        y += LIST_HEIGHT + GAP / 2;
    }
    shapes
}

/// One box over the rest.
fn hierarchy(items: &[&String], width: i64, fill: &str) -> Vec<Shape> {
    let Some((first, rest)) = items.split_first() else { return Vec::new() };

    // The top box is a third of the width, in the middle.
    let top_width = (width / 3).max(EMU_PER_INCH);
    let mut shapes =
        vec![boxed("roundRect", first, (width - top_width) / 2, 0, top_width, BOX_HEIGHT, fill)];
    if rest.is_empty() {
        return shapes;
    }

    let count = rest.len() as i64;
    let gaps = (count - 1).max(0) * GAP;
    let box_width = ((width - gaps) / count).max(EMU_PER_INCH / 2);
    let row_top = BOX_HEIGHT + GAP;

    let mut x = 0i64;
    for item in rest {
        shapes.push(boxed("roundRect", item, x, row_top, box_width, BOX_HEIGHT, fill));
        x += box_width + GAP;
    }
    shapes
}

/// A box with words in it, floating where it is put.
fn boxed(preset: &str, text: &str, x: i64, y: i64, width: i64, height: i64, fill: &str) -> Shape {
    Shape {
        text: vec![caption(text)],
        description: text.to_owned(),
        name: text.to_owned(),
        ..plain(preset, x, y, width, height, fill)
    }
}

/// The same with nothing written in it, which is what an arrow is.
fn plain(preset: &str, x: i64, y: i64, width: i64, height: i64, fill: &str) -> Shape {
    Shape {
        preset: preset.to_owned(),
        width_emu: width,
        height_emu: height,
        fill: Some(fill.to_owned()),
        outline: None,
        outline_emu: 0,
        text: Vec::new(),
        name: preset.to_owned(),
        anchor: Some(Anchor {
            // The text goes above and below the diagram rather than beside it:
            // a line of words running down the side of a chain of boxes is a
            // line nobody can read.
            wrap: Wrap::TopAndBottom,
            behind_text: false,
            horizontal_from: Relative::Column,
            horizontal: Placement::Offset(x),
            vertical_from: Relative::Paragraph,
            vertical: Placement::Offset(y),
            distance: (0, 0, GAP / 4, GAP / 4),
            // Every box of a diagram is at the same depth: they are laid out
            // side by side and never overlap.
            depth: crate::anchor::USUAL_DEPTH,
        }),
        description: String::new(),
    }
}

/// The words inside a box: centred, white, and small enough to fit.
fn caption(text: &str) -> Paragraph {
    Paragraph {
        properties: ParagraphProperties {
            alignment: Some(Alignment::Center),
            space_after: Some(0),
            ..ParagraphProperties::default()
        },
        runs: vec![Run {
            properties: RunProperties {
                color: Some("FFFFFF".to_owned()),
                size_half_points: Some(20),
                bold: Some(true),
                ..RunProperties::default()
            },
            content: vec![RunContent::Text(text.to_owned())],
            field: None,
            revision: None,
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOM: i64 = EMU_PER_INCH * 6;

    fn items(count: usize) -> Vec<String> {
        (0..count).map(|index| format!("Step {index}")).collect()
    }

    fn borrowed(items: &[String]) -> Vec<&String> {
        items.iter().collect()
    }

    #[test]
    fn every_arrangement_says_what_it_is_called() {
        for arrangement in Arrangement::ALL {
            assert!(!arrangement.label().is_empty());
        }
    }

    #[test]
    fn a_process_has_a_box_for_every_step_and_an_arrow_between_each_pair() {
        let items = items(4);
        let shapes = process(&borrowed(&items), ROOM, "4472C4");
        let boxes = shapes.iter().filter(|shape| shape.preset == "roundRect").count();
        let arrows = shapes.iter().filter(|shape| shape.preset == "rightArrow").count();
        assert_eq!(boxes, 4);
        assert_eq!(arrows, 3);
    }

    #[test]
    fn one_step_needs_no_arrow() {
        let items = items(1);
        let shapes = process(&borrowed(&items), ROOM, "4472C4");
        assert_eq!(shapes.len(), 1);
    }

    #[test]
    fn a_process_stays_inside_the_room_it_was_given() {
        let items = items(4);
        let shapes = process(&borrowed(&items), ROOM, "4472C4");
        let right = shapes
            .iter()
            .map(|shape| offset(shape).0 + shape.width_emu)
            .max()
            .expect("some shapes");
        assert!(right <= ROOM, "the diagram is {right} wide in {ROOM} of room");
    }

    #[test]
    fn a_list_puts_each_box_under_the_last() {
        let items = items(3);
        let shapes = list(&borrowed(&items), ROOM, "4472C4");
        assert_eq!(shapes.len(), 3);
        let tops: Vec<i64> = shapes.iter().map(|shape| offset(shape).1).collect();
        assert!(tops[0] < tops[1] && tops[1] < tops[2], "{tops:?}");
        // And each is the full width, which is what a list looks like.
        assert!(shapes.iter().all(|shape| shape.width_emu == ROOM));
    }

    #[test]
    fn a_hierarchy_puts_the_first_box_above_the_others() {
        let items = items(4);
        let shapes = hierarchy(&borrowed(&items), ROOM, "4472C4");
        assert_eq!(shapes.len(), 4);
        let top = offset(&shapes[0]).1;
        assert!(shapes[1..].iter().all(|shape| offset(shape).1 > top), "the row is not below");
    }

    #[test]
    fn a_hierarchy_of_one_is_the_one_box() {
        let items = items(1);
        assert_eq!(hierarchy(&borrowed(&items), ROOM, "4472C4").len(), 1);
    }

    #[test]
    fn every_box_carries_its_words_and_a_description() {
        let items = items(2);
        for shape in list(&borrowed(&items), ROOM, "4472C4") {
            assert_eq!(shape.text.len(), 1);
            assert_eq!(shape.text[0].plain_text(), shape.description);
            assert!(!shape.description.is_empty());
        }
    }

    #[test]
    fn a_diagram_takes_its_colour_from_what_it_was_given() {
        let items = items(2);
        for shape in list(&borrowed(&items), ROOM, "70AD47") {
            assert_eq!(shape.fill.as_deref(), Some("70AD47"));
        }
    }

    /// Where a shape floats, which is the only place its position is kept.
    fn offset(shape: &Shape) -> (i64, i64) {
        let anchor = shape.anchor.as_ref().expect("a floating shape");
        let across = match anchor.horizontal {
            Placement::Offset(value) => value,
            Placement::Aligned(_) => 0,
        };
        let down = match anchor.vertical {
            Placement::Offset(value) => value,
            Placement::Aligned(_) => 0,
        };
        (across, down)
    }
}
