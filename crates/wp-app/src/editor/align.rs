//! Word's Align: lining drawings up with each other, with the page, or with the
//! margins.
//!
//! # Why a delta and not a position
//!
//! A drawing's anchor says where it is as a distance from something — the text
//! area, the paper, the paragraph it hangs from — and which of those it is
//! measured from is the drawing's own business. Working out "the left edge of
//! the page, in the units this particular drawing counts in" would mean
//! knowing every one of those frames.
//!
//! There is no need. The layout has already worked out where each drawing is
//! *on the page*, so lining one up is a matter of moving it by the difference
//! between where it is and where it should be — and a distance is the same
//! distance whatever it is measured from. That is the same arithmetic a drag
//! does, and it is done in the same place: see [`super::handles`].
//!
//! # What they line up against
//!
//! Word's menu ends with three rows that are not commands: each other, the
//! page, or the margins. They say what the eight above them are measured
//! against, and picking one changes nothing on the page until one of the eight
//! is pressed again. See [`super::handles::AlignTo`].
//!
//! Lining drawings up with each other needs more than one of them, and Word's
//! menu is the same menu whichever is in force — so a single drawing lined up
//! with itself is told so rather than left wondering.

use wp_docx::anchor::{Anchor, Placement, Wrap};
use wp_docx::shapes::EMU_PER_POINT;
use wp_docx::TextPosition;
use wp_shell::Response;

use crate::chrome::{Choice, Command, Popup};

use super::handles::AlignTo;
use super::Editor;

/// One row of Word's Align menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Alignment {
    Left,
    Centre,
    Right,
    Top,
    Middle,
    Bottom,
    /// Spread out so the gaps between them are equal.
    SpreadAcross,
    SpreadDown,
    /// The three that say what the others are measured against.
    Against(AlignTo),
}

impl Alignment {
    /// Whether it moves things across the page rather than down it.
    fn sideways(self) -> bool {
        matches!(self, Self::Left | Self::Centre | Self::Right | Self::SpreadAcross)
    }
}

/// The menu, in Word's order: the six alignments, the two that spread, and the
/// three that say what against.
pub(super) const ROWS: &[(&str, Alignment)] = &[
    ("Align Left", Alignment::Left),
    ("Align Center", Alignment::Centre),
    ("Align Right", Alignment::Right),
    ("Align Top", Alignment::Top),
    ("Align Middle", Alignment::Middle),
    ("Align Bottom", Alignment::Bottom),
    ("Distribute Horizontally", Alignment::SpreadAcross),
    ("Distribute Vertically", Alignment::SpreadDown),
    ("Align Selected Objects", Alignment::Against(AlignTo::EachOther)),
    ("Align to Page", Alignment::Against(AlignTo::Page)),
    ("Align to Margin", Alignment::Against(AlignTo::Margin)),
];

impl Editor {
    /// Drops the Align menu open.
    pub(super) fn open_align(&mut self) -> Response {
        if self.close_popup_if(Choice::AlignObjects) {
            return Response::Redraw;
        }
        if self.drawings_in_hand().is_empty() {
            return self.report("Click a shape or a picture first");
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::AlignObjects) else {
            return Response::Ignored;
        };

        let items = ROWS.iter().map(|(label, _)| (*label).to_owned()).collect();
        // Which of the three the eight are being measured against, so the menu
        // says what is in force rather than offering three alike.
        let current = ROWS.iter().position(|(_, row)| *row == Alignment::Against(self.align_to));
        self.popup = Some(Popup::new(Choice::AlignObjects, items, current, left, top, 260.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Does whichever row was picked.
    pub(super) fn choose_align(&mut self, index: usize) -> Response {
        self.popup = None;
        let Some((label, row)) = ROWS.get(index).copied() else { return Response::Ignored };

        // The three at the foot say what the eight above are measured against,
        // and change nothing until one of those is pressed.
        if let Alignment::Against(against) = row {
            self.align_to = against;
            return self.report(label);
        }
        self.align_drawings(row, label)
    }

    /// Lines the chosen drawings up.
    fn align_drawings(&mut self, wanted: Alignment, label: &str) -> Response {
        let chosen = self.drawings_in_hand();
        if chosen.is_empty() {
            return self.report("Click a shape or a picture first");
        }

        // Where each of them is now, on the page it is on. A drawing that is
        // not laid out — on a page that has not been reached — cannot be lined
        // up with anything.
        let facing = self.drawings_facing();
        let mut boxes: Vec<super::handles::OnPage> = chosen
            .iter()
            .filter_map(|at| facing.iter().find(|drawing| drawing.at == *at).copied())
            .collect();
        if boxes.is_empty() {
            return Response::Ignored;
        }

        // Along the axis this alignment works in.
        let near = |drawing: &super::handles::OnPage| {
            if wanted.sideways() {
                drawing.left
            } else {
                drawing.top
            }
        };
        let size = |drawing: &super::handles::OnPage| {
            if wanted.sideways() {
                drawing.width
            } else {
                drawing.height
            }
        };
        boxes.sort_by(|one, other| near(one).total_cmp(&near(other)));

        let Some((band_near, band_far)) = self.aligning_band(wanted.sideways(), &boxes) else {
            return self.report("Choose more than one drawing to line them up with each other");
        };

        // How far each drawing has to move, in pixels on the page.
        let moves: Vec<(TextPosition, f32)> = match wanted {
            Alignment::Left | Alignment::Top => {
                boxes.iter().map(|drawing| (drawing.at, band_near - near(drawing))).collect()
            }
            Alignment::Right | Alignment::Bottom => boxes
                .iter()
                .map(|drawing| (drawing.at, band_far - size(drawing) - near(drawing)))
                .collect(),
            Alignment::Centre | Alignment::Middle => boxes
                .iter()
                .map(|drawing| {
                    let middle = (band_near + band_far - size(drawing)) / 2.0;
                    (drawing.at, middle - near(drawing))
                })
                .collect(),
            // Spread out: the first and the last stay where they are and the
            // rest are put at even gaps between them.
            Alignment::SpreadAcross | Alignment::SpreadDown => {
                if boxes.len() < 3 {
                    return self.report("Choose three or more drawings to spread them out");
                }
                let first = boxes.first().expect("at least three");
                let last = boxes.last().expect("at least three");
                let filled: f32 = boxes.iter().map(size).sum();
                let room = (near(last) + size(last)) - near(first) - filled;
                let gap = room / (boxes.len() - 1) as f32;

                let mut along = near(first);
                boxes
                    .iter()
                    .map(|drawing| {
                        let wanted = along;
                        along += size(drawing) + gap;
                        (drawing.at, wanted - near(drawing))
                    })
                    .collect()
            }
            Alignment::Against(_) => return Response::Ignored,
        };

        // One command is one thing to undo, however many drawings it moved.
        let scale = self.pixels_per_inch() / 72.0;
        if scale <= 0.0 {
            return Response::Ignored;
        }
        let depth = self.document.next_drawing_depth();
        self.document.begin_gesture();
        let mut changed = false;
        for (at, distance) in moves {
            if distance.abs() < 0.01 {
                continue;
            }
            let (across, down) =
                if wanted.sideways() { (distance / scale, 0.0) } else { (0.0, distance / scale) };
            let anchor = moved(self.document.anchor_at(at), across, down, depth);
            changed |= self.document.set_anchor_at(at, Some(&anchor));
        }
        self.document.end_gesture();

        self.relayout();
        self.edited(changed, label)
    }

    /// The band the drawings are lined up inside, along one axis.
    ///
    /// Each other, the paper, or the text — in screen coordinates, which is
    /// what the drawings' own boxes are in.
    fn aligning_band(
        &self,
        sideways: bool,
        boxes: &[super::handles::OnPage],
    ) -> Option<(f32, f32)> {
        let first = boxes.first()?;
        match self.align_to {
            AlignTo::EachOther => {
                // Lining one drawing up with itself moves nothing, and saying so
                // is better than saying nothing happened.
                if boxes.len() < 2 {
                    return None;
                }
                let near = boxes
                    .iter()
                    .map(|drawing| if sideways { drawing.left } else { drawing.top })
                    .fold(f32::MAX, f32::min);
                let far = boxes
                    .iter()
                    .map(|drawing| {
                        if sideways {
                            drawing.left + drawing.width
                        } else {
                            drawing.top + drawing.height
                        }
                    })
                    .fold(f32::MIN, f32::max);
                Some((near, far))
            }
            AlignTo::Page | AlignTo::Margin => {
                let page = self.pages.get(first.page)?;
                let (origin_x, origin_y) = self.page_origin(first.page);
                let top = self.content_top() + origin_y - self.scroll_down();
                let scale = self.pixels_per_inch() / 72.0;

                let inset = match self.align_to {
                    AlignTo::Margin => {
                        let metrics = wp_layout::PageMetrics::from_document(&self.document);
                        if sideways {
                            (metrics.margin_left * scale, metrics.margin_right * scale)
                        } else {
                            (metrics.margin_top * scale, metrics.margin_bottom * scale)
                        }
                    }
                    _ => (0.0, 0.0),
                };
                if sideways {
                    Some((origin_x + inset.0, origin_x + page.width - inset.1))
                } else {
                    Some((top + inset.0, top + page.height - inset.1))
                }
            }
        }
    }
}

/// The same anchor, moved by a distance in points.
///
/// A drawing that was in the line of text starts floating, because a drawing in
/// the line has no position of its own to move — the same rule a drag follows.
fn moved(anchor: Option<Anchor>, dx: f32, dy: f32, depth: u32) -> Anchor {
    let mut anchor = anchor.unwrap_or(Anchor { wrap: Wrap::Square, depth, ..Anchor::default() });
    let emu = |points: f32| (f64::from(points) * EMU_PER_POINT as f64) as i64;
    let across = match anchor.horizontal {
        Placement::Offset(distance) => distance,
        // A share of a frame gives way too: what a drag or an Align hands
        // over is a distance, and the drawing is no longer a share of
        // anything.
        Placement::Aligned(_) | Placement::Percent(_) => 0,
    };
    let down = match anchor.vertical {
        Placement::Offset(distance) => distance,
        // A share of a frame gives way too: what a drag or an Align hands
        // over is a distance, and the drawing is no longer a share of
        // anything.
        Placement::Aligned(_) | Placement::Percent(_) => 0,
    };
    anchor.horizontal = Placement::Offset(across + emu(dx));
    anchor.vertical = Placement::Offset(down + emu(dy));
    anchor
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::Document;
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event};

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    /// An editor with three floating shapes at three different places.
    fn editor() -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("Words to flow round them")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });

        for (name, across, down) in
            [("One", 0i64, 0i64), ("Two", 914_400, 457_200), ("Three", 1_828_800, 1_371_600)]
        {
            let shape = wp_docx::shapes::Shape {
                name: name.to_owned(),
                width_emu: 457_200,
                height_emu: 457_200,
                fill: wp_docx::fills::Fill::solid("4472C4"),
                anchor: Some(Anchor {
                    wrap: Wrap::None,
                    horizontal: Placement::Offset(across),
                    vertical: Placement::Offset(down),
                    ..Anchor::default()
                }),
                ..wp_docx::shapes::Shape::default()
            };
            editor.document.set_caret(wp_docx::TextPosition::new(0, 0));
            assert!(editor.document.insert_shape(&shape), "the shape went nowhere");
        }
        editor.relayout();
        editor
    }

    /// Chooses every drawing in the document.
    fn choose_all(editor: &mut Editor) {
        let all: Vec<TextPosition> =
            editor.drawings_facing().into_iter().map(|drawing| drawing.at).collect();
        for (at, place) in all.into_iter().enumerate() {
            if at == 0 {
                editor.choose_drawing_at(place);
            } else {
                editor.also_choose_drawing_at(place);
            }
        }
    }

    /// Where each chosen drawing is now.
    fn lefts(editor: &Editor) -> Vec<f32> {
        let mut out: Vec<f32> =
            editor.chosen_drawing_boxes().iter().map(|drawing| drawing.left).collect();
        out.sort_by(f32::total_cmp);
        out
    }

    fn tops(editor: &Editor) -> Vec<f32> {
        let mut out: Vec<f32> =
            editor.chosen_drawing_boxes().iter().map(|drawing| drawing.top).collect();
        out.sort_by(f32::total_cmp);
        out
    }

    /// Which row of the menu an alignment is.
    fn row_of(wanted: Alignment) -> usize {
        ROWS.iter().position(|(_, row)| *row == wanted).expect("a row")
    }

    #[test]
    fn three_drawings_can_be_chosen_at_once() {
        let mut editor = editor();
        choose_all(&mut editor);
        assert_eq!(editor.chosen_drawings.len(), 3);
        assert_eq!(editor.drawings_in_hand().len(), 3);
    }

    #[test]
    fn shift_and_a_click_on_one_already_chosen_lets_go_of_it() {
        let mut editor = editor();
        choose_all(&mut editor);
        let at = editor.chosen_drawings[1];
        editor.also_choose_drawing_at(at);
        assert_eq!(editor.chosen_drawings.len(), 2);
        assert!(!editor.chosen_drawings.contains(&at));
    }

    #[test]
    fn lining_them_up_on_the_left_puts_them_all_at_the_same_place() {
        let mut editor = editor();
        choose_all(&mut editor);
        assert!(lefts(&editor).windows(2).any(|pair| pair[1] > pair[0] + 1.0), "they start apart");

        editor.choose_align(row_of(Alignment::Left));
        let after = lefts(&editor);
        assert!(after.windows(2).all(|pair| (pair[1] - pair[0]).abs() < 1.0), "got {after:?}");
    }

    #[test]
    fn lining_them_up_at_the_top_does_the_same_the_other_way() {
        let mut editor = editor();
        choose_all(&mut editor);
        editor.choose_align(row_of(Alignment::Top));
        let after = tops(&editor);
        assert!(after.windows(2).all(|pair| (pair[1] - pair[0]).abs() < 1.0), "got {after:?}");
    }

    #[test]
    fn the_right_hand_edges_can_be_lined_up_too() {
        let mut editor = editor();
        choose_all(&mut editor);
        editor.choose_align(row_of(Alignment::Right));

        let mut rights: Vec<f32> = editor
            .chosen_drawing_boxes()
            .iter()
            .map(|drawing| drawing.left + drawing.width)
            .collect();
        rights.sort_by(f32::total_cmp);
        assert!(rights.windows(2).all(|pair| (pair[1] - pair[0]).abs() < 1.0), "got {rights:?}");
    }

    #[test]
    fn spreading_them_out_makes_the_gaps_even() {
        let mut editor = editor();
        choose_all(&mut editor);
        editor.choose_align(row_of(Alignment::SpreadAcross));

        let mut boxes = editor.chosen_drawing_boxes();
        boxes.sort_by(|one, other| one.left.total_cmp(&other.left));
        let gaps: Vec<f32> =
            boxes.windows(2).map(|pair| pair[1].left - (pair[0].left + pair[0].width)).collect();
        assert!(gaps.windows(2).all(|pair| (pair[1] - pair[0]).abs() < 1.0), "got {gaps:?}");
    }

    #[test]
    fn one_drawing_cannot_be_lined_up_with_itself() {
        let mut editor = editor();
        let at = editor.drawings_facing()[0].at;
        editor.choose_drawing_at(at);
        let before = lefts(&editor);

        editor.choose_align(row_of(Alignment::Left));
        assert_eq!(lefts(&editor), before, "one drawing moved");
        assert!(editor.status.contains("more than one"), "nothing was said: {}", editor.status);
    }

    #[test]
    fn one_drawing_can_be_lined_up_with_the_page() {
        let mut editor = editor();
        let at = editor.drawings_facing()[0].at;
        editor.choose_drawing_at(at);

        editor.choose_align(row_of(Alignment::Against(AlignTo::Page)));
        editor.choose_align(row_of(Alignment::Right));

        let drawing = editor.chosen_drawing_boxes().into_iter().next().expect("a chosen drawing");
        let page = editor.pages.first().expect("a page");
        let (origin_x, _) = editor.page_origin(0);
        assert!(
            ((drawing.left + drawing.width) - (origin_x + page.width)).abs() < 1.5,
            "it is not against the right edge of the paper"
        );
    }

    #[test]
    fn what_they_are_lined_up_against_is_a_mode_and_not_a_command() {
        let mut editor = editor();
        choose_all(&mut editor);
        let before = lefts(&editor);

        editor.choose_align(row_of(Alignment::Against(AlignTo::Margin)));
        assert_eq!(editor.align_to, AlignTo::Margin);
        assert_eq!(lefts(&editor), before, "picking what to line up against moved something");
    }

    #[test]
    fn a_band_swept_round_a_handful_of_them_chooses_all_it_touches() {
        let mut editor = editor();
        editor.toggle_choosing_drawings();

        // From the top left of the page to past the second drawing, which
        // takes those two and leaves the third.
        let boxes = editor.drawings_facing();
        let first = boxes.iter().map(|drawing| drawing.at).collect::<Vec<_>>();
        let second =
            boxes.iter().find(|drawing| drawing.at == first[1]).copied().expect("a second drawing");

        let from = (10, 10);
        let to = ((second.left + second.width) as i32, (second.top + second.height) as i32);
        assert!(editor.press_on_shape(from.0, from.1, false), "the band did not start");
        editor.drag_band(to.0, to.1);
        assert!(editor.dragging_band());
        editor.release_band();

        assert_eq!(editor.chosen_drawings.len(), 2, "got {:?}", editor.chosen_drawings);
    }

    #[test]
    fn a_drag_on_one_of_several_moves_all_of_them() {
        let mut editor = editor();
        choose_all(&mut editor);
        let before = lefts(&editor);

        // Taken hold of by the middle of the first, and dragged across.
        let drawing = editor.chosen_drawing_boxes().into_iter().next().expect("a drawing");
        let (x, y) = (
            (drawing.left + drawing.width / 2.0) as i32,
            (drawing.top + drawing.height / 2.0) as i32,
        );
        assert!(editor.press_on_shape(x, y, false), "nothing was taken hold of");
        editor.drag_shape(x + 60, y);
        editor.release_shape();

        let after = lefts(&editor);
        assert!(
            before.iter().zip(&after).all(|(one, other)| (other - one - 60.0).abs() < 1.5),
            "they did not all move together: {before:?} then {after:?}"
        );
    }

    #[test]
    fn one_undo_takes_back_the_whole_command() {
        let mut editor = editor();
        choose_all(&mut editor);
        let before = lefts(&editor);

        editor.choose_align(row_of(Alignment::Left));
        assert_ne!(lefts(&editor), before);

        editor.document.undo();
        editor.relayout();
        assert_eq!(lefts(&editor), before, "undo left them where the command put them");
    }
}
