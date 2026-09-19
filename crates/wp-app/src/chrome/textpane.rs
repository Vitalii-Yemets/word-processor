//! Word's Text Pane: the words of a diagram as a list, typed into.
//!
//! # What it shows
//!
//! One line per box of the chosen diagram, set in by how deep the box hangs,
//! with a bullet in front of each — which is exactly how Word's pane shows
//! them, under "Type your text here". One line is chosen and carries a
//! caret; typing goes into it, Enter adds a box after it, Tab hangs it
//! under the box above and Shift+Tab lifts it out again. What is typed
//! lays the diagram out again as it is typed, which is the whole point of
//! the pane: the picture is the words.
//!
//! # What the pane does not decide
//!
//! Anything. It draws what it is given and reports where it was pressed,
//! as the other panes down the side do — see [`super::mappingpane`].

use wp_layout::{LayoutEngine, Renderer};
use wp_raster::Canvas;

use crate::messages::t;

use super::pane::{Pen, Scroll, HEADING, PADDING, TEXT};
use super::theme::Theme;

/// How wide the pane is drawn.
pub const WIDTH: f32 = 300.0;

/// How tall a line is.
const ROW: f32 = 20.0;
/// How far each level is set in.
const STEP: f32 = 16.0;

/// Everything the pane draws.
#[derive(Clone, Debug, Default)]
pub struct Shown {
    /// The words, one per box, with how deep each hangs.
    pub lines: Vec<(u8, String)>,
    /// Which line is chosen.
    pub chosen: usize,
    /// Where the caret stands in it, in characters.
    pub caret: usize,
    /// What arrangement the diagram is, for the pane's foot.
    pub arrangement: String,
}

/// Where a press on the pane can land.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    /// A line, and how far along it in pixels from the words' start.
    Line(usize),
    Close,
}

/// The pane, and what it remembers between one drawing and the next.
#[derive(Clone, Debug, Default)]
pub struct TextPane {
    hovered: Option<Hit>,
    scroll: Scroll,
    placed: Vec<(Hit, f32, f32, f32, f32)>,
    /// Where each line's words begin, for a press to say which character
    /// it landed on.
    starts: Vec<f32>,
}

impl TextPane {
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

    /// How far into a line's words a point is, in pixels.
    #[must_use]
    pub fn along(&self, line: usize, x: i32) -> f32 {
        let x = super::mirror::flip(x) as f32;
        (x - self.starts.get(line).copied().unwrap_or(0.0)).max(0.0)
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
        self.starts.clear();
        canvas.fill_rect(left as i32, top as i32, WIDTH as i32, (bottom - top) as i32, theme.pane);
        canvas.fill_rect(left as i32, top as i32, 1, (bottom - top) as i32, theme.pane_edge);

        let started = top - self.scroll.offset;
        let mut pen =
            Pen { canvas, engine, renderer, theme, left, width: WIDTH, y: top + 8.0, bottom, top };
        self.caption(&mut pen, top);
        let header = pen.y;
        pen.y = started + (header - top);

        pen.heading(t("Type your text here"));
        if shown.lines.is_empty() {
            pen.note(t("Choose a diagram to type its words here."));
        }
        for (index, (level, text)) in shown.lines.iter().enumerate() {
            let chosen = index == shown.chosen;
            self.line(&mut pen, index, *level, text, chosen, chosen.then_some(shown.caret));
        }
        pen.gap();
        pen.rule();
        if !shown.arrangement.is_empty() {
            pen.heading(&shown.arrangement);
        }
        pen.note(t("Enter adds a box, Tab hangs one under the last, Shift+Tab lifts it out."));

        let reach = pen.y - started;
        self.scroll.reached(reach, bottom - top);
        self.scroll.draw_bar(canvas, left + WIDTH, header, bottom, theme);
    }

    /// The name of the pane, and the cross that shuts it.
    fn caption(&mut self, pen: &mut Pen<'_, '_, '_>, top: f32) {
        pen.words(t("Text Pane"), pen.left + PADDING, HEADING, pen.theme.text);
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

    /// One line: a bullet set in by its depth, the words, and the caret
    /// when the line is the chosen one.
    fn line(
        &mut self,
        pen: &mut Pen<'_, '_, '_>,
        index: usize,
        level: u8,
        text: &str,
        chosen: bool,
        caret: Option<usize>,
    ) {
        let hit = Hit::Line(index);
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
        let bullet_x = x + 6.0 + f32::from(level) * STEP;
        let text_x = bullet_x + 12.0;
        let baseline = pen.y + 14.0;
        let bullet = pen.engine.simple_line("•", bullet_x, baseline, TEXT, colour);
        let line = pen.engine.simple_line(text, text_x, baseline, TEXT, colour);
        if pen.showing(ROW) {
            pen.renderer.draw_within(pen.canvas, &bullet, x, pen.y, width, ROW);
            pen.renderer.draw_within(pen.canvas, &line, x, pen.y, width, ROW);
            // The caret, after as many characters as it stands past.
            if let Some(caret) = caret {
                let before: String = text.chars().take(caret).collect();
                let measured = pen.engine.simple_line(&before, text_x, baseline, TEXT, colour);
                // A line's width is where its pen stopped, from the window's
                // edge; an empty line's is where it began.
                let caret_x = if before.is_empty() { text_x } else { measured.width };
                pen.canvas.fill_rect(caret_x as i32, pen.y as i32 + 3, 1, ROW as i32 - 6, colour);
            }
        }
        self.starts.push(text_x);
        self.placed.push((hit, x, pen.y, width, ROW));
        pen.y += ROW;
    }
}
