//! Word's XML Mapping pane: the data a document carries, as a tree, and the
//! way to bind a content control to one node of it.
//!
//! # What it shows
//!
//! The custom XML parts the document carries, one of them chosen; under it
//! the chosen part's tree, an element to a row with its attributes set in
//! under it and what each says beside it; and the three things to do —
//! bind a control to the chosen node, add a part from a file, take the
//! chosen part out. Word's pane is the same three things in the same order,
//! with the binding under a right-click; here it is a button, because a
//! button can be found.
//!
//! # What the pane does not decide
//!
//! Anything. It draws what it is given and reports where it was pressed,
//! as the other panes down the side do — see [`super::signaturepane`].

use wp_docx::customxml::NodeRow;
use wp_layout::{LayoutEngine, Renderer};
use wp_raster::{Canvas, Color};

use crate::messages::t;

use super::pane::{Pen, Scroll, HEADING, PADDING, TEXT};
use super::theme::Theme;

/// How wide the pane is drawn.
pub const WIDTH: f32 = 300.0;

/// How tall a button is, and a row of the tree.
const BUTTON: f32 = 24.0;
const ROW: f32 = 18.0;

/// How far each level of the tree is set in.
const STEP: f32 = 14.0;

/// One part, as the list at the top names it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PartRow {
    pub label: String,
    pub id: String,
}

/// Everything the pane draws.
#[derive(Clone, Debug, Default)]
pub struct Shown {
    pub parts: Vec<PartRow>,
    /// Which part is chosen, of those.
    pub part: usize,
    /// The chosen part's tree.
    pub rows: Vec<NodeRow>,
    /// Which row is chosen, if one is.
    pub row: Option<usize>,
    /// Whether the caret is in a content control that can be bound, rather
    /// than one being put in.
    pub caret_in_control: bool,
}

/// Where a press on the pane can land.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Part(usize),
    Row(usize),
    /// Bind a control to the chosen node: the one the caret is in, or a
    /// new one.
    Insert,
    AddPart,
    DeletePart,
    Close,
}

/// The pane, and what it remembers between one drawing and the next.
#[derive(Clone, Debug, Default)]
pub struct MappingPane {
    hovered: Option<Hit>,
    scroll: Scroll,
    placed: Vec<(Hit, f32, f32, f32, f32)>,
    /// Where the Insert button was last drawn, for the list that drops
    /// under it.
    pub insert_rect: (f32, f32, f32, f32),
}

impl MappingPane {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// What a point is on, if anything.
    #[must_use]
    pub fn at(&self, x: i32, y: i32) -> Option<Hit> {
        let x = super::mirror::flip(x);
        let (x, y) = (x as f32, y as f32);
        self.placed
            .iter()
            .find(|(_, left, top, width, height)| {
                x >= *left && x < left + width && y >= *top && y < top + height
            })
            .map(|(hit, ..)| *hit)
    }

    /// Follows the pointer. True when something has to be drawn again.
    pub fn hover(&mut self, x: i32, y: i32) -> bool {
        let over = self.at(x, y);
        if over == self.hovered {
            return false;
        }
        self.hovered = over;
        true
    }

    /// Moves it, and says whether it moved.
    pub fn scroll_by(&mut self, pixels: f32) -> bool {
        self.scroll.by(pixels)
    }

    /// Draws the pane.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        shown: &Shown,
        left: f32,
        top: f32,
        bottom: f32,
        theme: &Theme,
    ) {
        self.placed.clear();
        canvas.fill_rect(left as i32, top as i32, WIDTH as i32, (bottom - top) as i32, theme.pane);
        canvas.fill_rect(left as i32, top as i32, 1, (bottom - top) as i32, theme.pane_edge);

        let started = top - self.scroll.offset;
        let mut pen =
            Pen { canvas, engine, renderer, theme, left, width: WIDTH, y: top + 8.0, bottom, top };
        self.caption(&mut pen, top);
        let header = pen.y;
        pen.y = started + (header - top);

        pen.heading(t("Custom XML Part:"));
        if shown.parts.is_empty() {
            pen.note(t("This document carries no custom XML. Add a part from a file to bind controls to it."));
        }
        for (index, part) in shown.parts.iter().enumerate() {
            self.part(&mut pen, index, part, index == shown.part);
        }
        pen.gap();
        pen.rule();

        if !shown.parts.is_empty() {
            for (index, row) in shown.rows.iter().enumerate() {
                self.row(&mut pen, index, row, shown.row == Some(index));
            }
            pen.gap();
        }

        let label = if shown.caret_in_control {
            t("Bind the control to this node")
        } else {
            t("Insert content control")
        };
        let rect = self.button(&mut pen, Hit::Insert, label, shown.row.is_some());
        self.insert_rect = rect;
        self.button(&mut pen, Hit::AddPart, t("Add new part..."), true);
        self.button(&mut pen, Hit::DeletePart, t("Delete part"), !shown.parts.is_empty());

        let reach = pen.y - started;
        self.scroll.reached(reach, bottom - top);
        self.scroll.draw_bar(canvas, left + WIDTH, header, bottom, theme);
    }

    /// The name of the pane, and the cross that shuts it.
    fn caption(&mut self, pen: &mut Pen<'_, '_, '_>, top: f32) {
        pen.words(t("XML Mapping"), pen.left + PADDING, HEADING, pen.theme.text);
        let close_left = pen.left + WIDTH - 26.0;
        if self.hovered == Some(Hit::Close) {
            pen.canvas.fill_rect(close_left as i32 - 4, top as i32 + 6, 24, 22, pen.theme.hover);
        }
        super::icons::draw_sized(
            pen.canvas,
            super::icons::Icon::Close,
            close_left,
            top + 10.0,
            12.0,
            pen.theme.text,
        );
        self.placed.push((Hit::Close, close_left - 4.0, top + 6.0, 24.0, 22.0));
        pen.y = top + 34.0;
        pen.rule();
    }

    /// One part in the list at the top.
    fn part(&mut self, pen: &mut Pen<'_, '_, '_>, index: usize, part: &PartRow, chosen: bool) {
        let hit = Hit::Part(index);
        let x = pen.left + PADDING;
        let width = WIDTH - PADDING * 2.0;
        if pen.showing(ROW) {
            if chosen {
                pen.canvas.fill_rect(
                    x as i32,
                    pen.y as i32,
                    width as i32,
                    ROW as i32,
                    pen.theme.accent,
                );
            } else if self.hovered == Some(hit) {
                pen.canvas.fill_rect(
                    x as i32,
                    pen.y as i32,
                    width as i32,
                    ROW as i32,
                    pen.theme.hover,
                );
            }
        }
        let colour = if chosen { pen.theme.on_accent() } else { pen.theme.text };
        let line = pen.engine.simple_line(&part.label, x + 4.0, pen.y + 13.0, TEXT, colour);
        if pen.showing(ROW) {
            pen.renderer.draw_within(pen.canvas, &line, x, pen.y, width, ROW);
        }
        self.placed.push((hit, x, pen.y, width, ROW));
        pen.y += ROW;
    }

    /// One node of the tree: its name set in by its depth, and what it
    /// says beside it.
    fn row(&mut self, pen: &mut Pen<'_, '_, '_>, index: usize, row: &NodeRow, chosen: bool) {
        let hit = Hit::Row(index);
        let x = pen.left + PADDING;
        let width = WIDTH - PADDING * 2.0;
        if pen.showing(ROW) {
            if chosen {
                pen.canvas.fill_rect(
                    x as i32,
                    pen.y as i32,
                    width as i32,
                    ROW as i32,
                    pen.theme.accent,
                );
            } else if self.hovered == Some(hit) {
                pen.canvas.fill_rect(
                    x as i32,
                    pen.y as i32,
                    width as i32,
                    ROW as i32,
                    pen.theme.hover,
                );
            }
        }
        let colour = if chosen { pen.theme.on_accent() } else { pen.theme.text };
        let dim = if chosen { pen.theme.on_accent() } else { pen.theme.dim_text };
        let text_x = x + 4.0 + row.depth as f32 * STEP;
        let name = match &row.attribute {
            Some(_) => format!("@{}", row.name),
            None => row.name.clone(),
        };
        let line = pen.engine.simple_line(&name, text_x, pen.y + 13.0, TEXT, colour);
        // A line's width is where its pen stopped, from the window's edge.
        let name_end = line.width;
        if pen.showing(ROW) {
            pen.renderer.draw_within(pen.canvas, &line, x, pen.y, width, ROW);
        }
        if !row.value.trim().is_empty() {
            let value_x = name_end + 8.0;
            let line = pen.engine.simple_line(&row.value, value_x, pen.y + 13.0, TEXT, dim);
            if pen.showing(ROW) {
                pen.renderer.draw_within(
                    pen.canvas,
                    &line,
                    value_x,
                    pen.y,
                    (x + width - value_x).max(0.0),
                    ROW,
                );
            }
        }
        self.placed.push((hit, x, pen.y, width, ROW));
        pen.y += ROW;
    }

    /// A button across the pane, greyed when there is nothing for it to do.
    fn button(
        &mut self,
        pen: &mut Pen<'_, '_, '_>,
        hit: Hit,
        label: &str,
        enabled: bool,
    ) -> (f32, f32, f32, f32) {
        let x = pen.left + PADDING;
        let width = WIDTH - PADDING * 2.0;
        let fill =
            if enabled && self.hovered == Some(hit) { pen.theme.hover } else { pen.theme.field };
        if pen.showing(BUTTON) {
            pen.canvas.fill_rect(x as i32, pen.y as i32, width as i32, BUTTON as i32, fill);
            outline(pen.canvas, x, pen.y, width, BUTTON, pen.theme.field_edge);
        }
        let colour = if enabled { pen.theme.text } else { pen.theme.disabled_text };
        let measured = pen.engine.simple_line(label, 0.0, 0.0, TEXT, colour);
        let text_x = x + (width - measured.width).max(0.0) / 2.0;
        let line = pen.engine.simple_line(label, text_x, pen.y + 16.0, TEXT, colour);
        if pen.showing(BUTTON) {
            pen.renderer.draw_onto(pen.canvas, &line, 0.0, 0.0);
        }
        let rect = (x, pen.y, width, BUTTON);
        if enabled {
            self.placed.push((hit, x, pen.y, width, BUTTON));
        }
        pen.y += BUTTON + 8.0;
        rect
    }
}

/// A rectangle drawn as four lines rather than filled.
fn outline(canvas: &mut Canvas, left: f32, top: f32, width: f32, height: f32, colour: Color) {
    let (x, y, w, h) = (left as i32, top as i32, width as i32, height as i32);
    canvas.fill_rect(x, y, w, 1, colour);
    canvas.fill_rect(x, y + h - 1, w, 1, colour);
    canvas.fill_rect(x, y, 1, h, colour);
    canvas.fill_rect(x + w - 1, y, 1, h, colour);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_hit_before_it_has_been_drawn() {
        let pane = MappingPane::new();
        assert_eq!(pane.at(10, 10), None);
    }
}
