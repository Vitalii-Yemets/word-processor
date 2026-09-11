//! Word's Group, Ungroup and Regroup: making one drawing of several, taking it
//! apart again, and putting it back together.
//!
//! (Not to be confused with [`super::groups`], which is about a group of the
//! ribbon. Word calls both of them groups.)
//!
//! # Why the places come from here
//!
//! Because a group is made where its members already are, and where they are is
//! something only the layout knows. A drawing's anchor says how far it is from
//! the text, the paper or the paragraph it hangs from, and which of those is the
//! drawing's own business — so the document cannot work out that two drawings
//! are an inch apart, and this can: the layout has already put both on a page.
//!
//! So the rectangles are measured on the page and handed down in the units the
//! format counts in. The same reasoning Align follows, for the same reason. See
//! [`super::align`].
//!
//! # Why Regroup remembers only one group
//!
//! Because that is all Word's remembers. Ungroup lets a handful of drawings
//! loose; Regroup puts that handful back. Ungrouping something else forgets the
//! first, and so does closing the document.

use wp_docx::group::Rect;
use wp_docx::shapes::EMU_PER_POINT;
use wp_docx::TextPosition;
use wp_shell::Response;

use crate::chrome::{Choice, Command, Popup};

use super::Editor;

/// One row of Word's Group menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Grouping {
    Group,
    Ungroup,
    Regroup,
}

/// The menu, in Word's order.
pub(super) const ROWS: &[(&str, Grouping)] =
    &[("Group", Grouping::Group), ("Ungroup", Grouping::Ungroup), ("Regroup", Grouping::Regroup)];

impl Editor {
    /// Drops the Group menu open.
    pub(super) fn open_grouping(&mut self) -> Response {
        if self.close_popup_if(Choice::GroupObjects) {
            return Response::Redraw;
        }
        if self.drawings_in_hand().is_empty() {
            return self.report("Click a shape or a picture first");
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::GroupObjects) else {
            return Response::Ignored;
        };

        let items = ROWS.iter().map(|(label, _)| (*label).to_owned()).collect();
        // None of the three is a state to be in: each does something, and what
        // it does depends on what is chosen rather than on what was picked last.
        self.popup = Some(Popup::new(Choice::GroupObjects, items, None, left, top, 200.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Does whichever row was picked.
    pub(super) fn choose_grouping(&mut self, index: usize) -> Response {
        self.popup = None;
        let Some((_, row)) = ROWS.get(index).copied() else { return Response::Ignored };
        match row {
            Grouping::Group => self.group_chosen_drawings(),
            Grouping::Ungroup => self.ungroup_chosen_drawings(),
            Grouping::Regroup => self.regroup_drawings(),
        }
    }

    /// Makes one drawing of the ones that are chosen.
    pub(super) fn group_chosen_drawings(&mut self) -> Response {
        let members = self.chosen_rectangles();
        if members.len() < 2 {
            return self.report("Choose more than one drawing to group them");
        }

        let Some(at) = self.document.group_drawings(&members) else {
            return self.report("These drawings cannot be grouped");
        };
        self.relayout();
        // The group is the drawing now, so it is the drawing in hand: a person
        // who has just grouped something expects to be holding it.
        self.choose_drawing_at(at);
        self.edited(true, "Group")
    }

    /// Takes the chosen group apart.
    ///
    /// Several chosen groups are all taken apart, which is what Word does, and
    /// only the last of them can be put back together — see the note at the top
    /// of this file.
    pub(super) fn ungroup_chosen_drawings(&mut self) -> Response {
        let chosen = self.drawings_in_hand();
        if chosen.is_empty() {
            return self.report("Click a group first");
        }
        if !chosen.iter().any(|at| self.document.group_at(*at).is_some()) {
            return self.report("That is not a group");
        }

        // From the last backwards: taking one apart puts several drawings where
        // one was, which moves the places of everything after it.
        let mut places = chosen;
        places.sort_by_key(|at| (at.paragraph, at.offset));
        let mut loose = Vec::new();
        for at in places.iter().rev() {
            let out = self.document.ungroup_at(*at);
            if !out.is_empty() {
                loose = out;
            }
        }
        if loose.is_empty() {
            return self.report("That group is empty");
        }

        self.ungrouped.clone_from(&loose);
        self.relayout();
        self.drop_chosen_drawing();
        for (index, at) in loose.iter().enumerate() {
            if index == 0 {
                self.choose_drawing_at(*at);
            } else {
                self.also_choose_drawing_at(*at);
            }
        }
        self.edited(true, "Ungroup")
    }

    /// Puts the last group that was taken apart back together.
    pub(super) fn regroup_drawings(&mut self) -> Response {
        if self.ungrouped.is_empty() {
            return self.report("Nothing has been ungrouped to put back");
        }
        // The drawings have to still be there. Anything that removed one leaves
        // Regroup with nothing to work from, and saying so beats making a group
        // of whatever happens to be at those places now.
        let wanted = self.ungrouped.clone();
        if !wanted.iter().all(|at| self.document.drawing_at(*at)) {
            self.ungrouped.clear();
            return self.report("The drawings that were ungrouped are no longer there");
        }

        self.drop_chosen_drawing();
        for (index, at) in wanted.iter().enumerate() {
            if index == 0 {
                self.choose_drawing_at(*at);
            } else {
                self.also_choose_drawing_at(*at);
            }
        }
        self.ungrouped.clear();
        self.group_chosen_drawings()
    }

    /// Where each chosen drawing is, in the units the format counts in.
    ///
    /// Measured from the corner of the page they are on, which is a common
    /// origin for all of them and the only one that means anything: two
    /// drawings on two pages are not near each other in any sense a group could
    /// keep, so a selection spread over two pages yields nothing.
    fn chosen_rectangles(&self) -> Vec<(TextPosition, Rect)> {
        rectangles_of(&self.chosen_drawing_boxes(), self.pixels_per_inch() / 72.0)
    }
}

/// The same, as arithmetic on its own.
///
/// Nothing at all when they are not all on one page: there is then no origin
/// to measure both from, and a group whose members were measured against two
/// different sheets of paper would put them anywhere but where they were.
fn rectangles_of(boxes: &[super::handles::OnPage], scale: f32) -> Vec<(TextPosition, Rect)> {
    if scale <= 0.0 {
        return Vec::new();
    }
    let Some(first) = boxes.first() else { return Vec::new() };
    if boxes.iter().any(|drawing| drawing.page != first.page) {
        return Vec::new();
    }

    // Rounded rather than truncated: a drawing measured in pixels and written
    // in units 12,700 to the point is going to land between two of them, and
    // always taking the lower one would creep a drawing up and to the left.
    let emu =
        |pixels: f32| (f64::from(pixels) / f64::from(scale) * EMU_PER_POINT as f64).round() as i64;
    boxes
        .iter()
        .map(|drawing| {
            (
                drawing.at,
                Rect {
                    x: emu(drawing.left),
                    y: emu(drawing.top),
                    width: emu(drawing.width),
                    height: emu(drawing.height),
                },
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_docx::anchor::{Anchor, Placement, Wrap};
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::Document;
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event};

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    /// An editor with two floating shapes an inch apart, both chosen.
    fn editor() -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("Words to flow round them")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });

        for offset in [0i64, 914_400] {
            let shape = wp_docx::shapes::Shape {
                name: "Box".to_owned(),
                width_emu: 914_400,
                height_emu: 914_400,
                fill: Some("4472C4".to_owned()),
                anchor: Some(Anchor {
                    wrap: Wrap::None,
                    horizontal: Placement::Offset(offset),
                    vertical: Placement::Offset(offset),
                    ..Anchor::default()
                }),
                ..wp_docx::shapes::Shape::default()
            };
            editor.document.set_caret(TextPosition::new(0, 0));
            assert!(editor.document.insert_shape(&shape), "the shape went nowhere");
        }
        editor.relayout();
        choose_all(&mut editor);
        editor
    }

    fn choose_all(editor: &mut Editor) {
        let all: Vec<TextPosition> =
            editor.drawings_facing().into_iter().map(|drawing| drawing.at).collect();
        editor.drop_chosen_drawing();
        for (index, place) in all.into_iter().enumerate() {
            if index == 0 {
                editor.choose_drawing_at(place);
            } else {
                editor.also_choose_drawing_at(place);
            }
        }
    }

    fn row(label: &str) -> usize {
        ROWS.iter().position(|(name, _)| *name == label).expect("no such row")
    }

    #[test]
    fn the_menu_is_words_three() {
        assert_eq!(ROWS.len(), 3);
        assert_eq!(ROWS[0].0, "Group");
        assert_eq!(ROWS[2].0, "Regroup");
    }

    #[test]
    fn grouping_makes_one_drawing_of_two() {
        let mut editor = editor();
        assert_eq!(editor.document.shapes().len(), 2);

        editor.choose_grouping(row("Group"));
        assert_eq!(editor.document.groups().len(), 1, "nothing was grouped");
        assert!(editor.document.shapes().is_empty());
        // And the group is what is now in hand.
        assert_eq!(editor.drawings_in_hand().len(), 1);
    }

    #[test]
    fn the_group_is_the_size_the_drawings_covered() {
        let mut editor = editor();
        editor.choose_grouping(row("Group"));

        let group = editor.document.groups().into_iter().next().expect("a group");
        // Two one-inch squares an inch apart cover two inches each way. The
        // rectangles are measured in pixels and turned back into the format's
        // units, so a pixel of rounding either way is expected.
        let inch = 914_400;
        assert!(
            (group.width_emu - 2 * inch).abs() < inch / 20,
            "the group is {} wide",
            group.width_emu
        );
        assert!((group.height_emu - 2 * inch).abs() < inch / 20);
    }

    #[test]
    fn one_drawing_is_told_rather_than_grouped_with_itself() {
        let mut editor = editor();
        let first = editor.drawings_in_hand().first().copied().expect("a drawing");
        editor.drop_chosen_drawing();
        editor.choose_drawing_at(first);

        editor.choose_grouping(row("Group"));
        assert!(editor.document.groups().is_empty());
        assert!(editor.status.contains("more than one"), "it said {:?}", editor.status);
    }

    #[test]
    fn ungrouping_gives_the_drawings_back_and_holds_them() {
        let mut editor = editor();
        editor.choose_grouping(row("Group"));
        editor.choose_grouping(row("Ungroup"));

        assert!(editor.document.groups().is_empty(), "it is still a group");
        assert_eq!(editor.document.shapes().len(), 2);
        assert_eq!(editor.drawings_in_hand().len(), 2, "both should be in hand");
    }

    #[test]
    fn ungrouping_something_that_is_not_a_group_says_so() {
        let mut editor = editor();
        editor.choose_grouping(row("Ungroup"));
        assert!(editor.status.contains("not a group"), "it said {:?}", editor.status);
    }

    #[test]
    fn regroup_puts_the_last_one_back() {
        let mut editor = editor();
        editor.choose_grouping(row("Group"));
        editor.choose_grouping(row("Ungroup"));
        // Nothing chosen, the way it would be after a click elsewhere: Regroup
        // works from what it remembers and not from what is in hand.
        editor.drop_chosen_drawing();

        editor.choose_grouping(row("Regroup"));
        assert_eq!(editor.document.groups().len(), 1, "it was not put back together");
    }

    #[test]
    fn regroup_with_nothing_to_regroup_says_so() {
        let mut editor = editor();
        editor.choose_grouping(row("Regroup"));
        assert!(
            editor.status.contains("Nothing has been ungrouped"),
            "it said {:?}",
            editor.status
        );
    }

    #[test]
    fn one_undo_takes_back_a_group() {
        let mut editor = editor();
        editor.choose_grouping(row("Group"));
        editor.document.undo();
        assert!(editor.document.groups().is_empty(), "the group is still there");
        assert_eq!(editor.document.shapes().len(), 2);
    }

    /// A drawing on a page, as the layout reports one.
    fn on_page(page: usize, offset: usize, left: f32, top: f32) -> super::super::handles::OnPage {
        super::super::handles::OnPage {
            at: TextPosition::new(0, offset),
            page,
            left,
            top,
            width: 96.0,
            height: 96.0,
        }
    }

    #[test]
    fn drawings_on_one_page_are_measured_from_that_page() {
        let boxes = [on_page(0, 0, 100.0, 200.0), on_page(0, 1, 196.0, 296.0)];
        let rectangles = rectangles_of(&boxes, 96.0 / 72.0);
        assert_eq!(rectangles.len(), 2);
        // Ninety-six pixels at ninety-six to the inch is an inch, and an inch
        // is 914,400 of the units the format counts in.
        assert_eq!(rectangles[1].1.x - rectangles[0].1.x, 914_400);
        assert_eq!(rectangles[0].1.width, 914_400);
    }

    #[test]
    fn drawings_on_two_pages_are_not_grouped() {
        // There is nothing to measure them both against: two drawings on two
        // sheets of paper are not near each other in any sense a group could
        // keep. So nothing is offered to the document at all.
        let boxes = [on_page(0, 0, 100.0, 200.0), on_page(1, 1, 100.0, 200.0)];
        assert!(rectangles_of(&boxes, 96.0 / 72.0).is_empty());
    }
}
