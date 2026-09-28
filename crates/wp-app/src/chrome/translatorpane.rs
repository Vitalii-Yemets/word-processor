//! Word's Translator pane: what a word or a few words are in another
//! language, from a box of their own down the side of the window.
//!
//! # What it shows
//!
//! Which way it translates — two languages with a button between them that
//! turns them round — a box holding what is being translated, and under it
//! what the dictionary says: the words as headings, each sense of each under
//! it, any of which can be put into the document. What is in the box is
//! whatever was selected when the pane was opened, or whatever is typed into
//! it since, which is the want **F4** named: somewhere to ask about a word
//! that is not in the document.
//!
//! # What the pane does not decide
//!
//! Anything. It draws what it is given and reports where it was pressed, as
//! the other panes down the side do; the looking up is the editor's.

use wp_layout::{LayoutEngine, Renderer};
use wp_raster::Canvas;

use crate::messages::t;

use super::icons::{self, Icon};
use super::pane::{Pen, Scroll, HEADING, PADDING, TEXT};
use super::theme::Theme;

/// How wide the pane is drawn.
pub const WIDTH: f32 = 320.0;

/// How tall a line of the pane is.
const ROW: f32 = 20.0;
/// How tall the box is.
const BOX: f32 = 26.0;

/// One line of what the dictionary said.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Line {
    /// A word of what is being translated, as a heading over its senses.
    Word(String),
    /// One sense of it, which can be put in.
    Sense(String),
    /// A word the dictionary has no entry for.
    Missing(String),
}

/// Everything the pane draws.
#[derive(Clone, Debug, Default)]
pub struct Shown {
    /// The two languages, by name.
    pub from: String,
    pub to: String,
    /// What the box holds.
    pub text: String,
    /// Where the caret stands in it, in characters, while the box has the
    /// keyboard.
    pub caret: Option<usize>,
    /// What the dictionary says about it.
    pub lines: Vec<Line>,
    /// What to say where there is nothing to list.
    pub note: Option<String>,
}

/// Where a press on the pane can land.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Close,
    /// The language translated from, which opens the list of them.
    From,
    /// And into.
    To,
    /// The button that turns the two round.
    Swap,
    /// The box.
    Box,
    /// A sense, by its place among the lines.
    Sense(usize),
}

/// The pane, and what it remembers between one drawing and the next.
#[derive(Clone, Debug, Default)]
pub struct TranslatorPane {
    hovered: Option<Hit>,
    scroll: Scroll,
    placed: Vec<(Hit, f32, f32, f32, f32)>,
    /// Where the box's words begin, for a press to say where in them it
    /// landed.
    box_start: f32,
}

impl TranslatorPane {
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

    /// Where a hit was drawn: its left, top, width and height.
    #[must_use]
    pub fn place_of(&self, hit: Hit) -> Option<(f32, f32, f32, f32)> {
        self.placed
            .iter()
            .find(|(found, ..)| *found == hit)
            .map(|(_, left, top, width, height)| (*left, *top, *width, *height))
    }

    /// How far into the box's words a point is, in pixels.
    #[must_use]
    pub fn along(&self, x: i32) -> f32 {
        let x = super::mirror::flip(x) as f32;
        (x - self.box_start).max(0.0)
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

        let mut pen =
            Pen { canvas, engine, renderer, theme, left, width: WIDTH, y: top + 8.0, bottom, top };
        self.caption(&mut pen, top);
        self.languages(&mut pen, shown);
        self.text_box(&mut pen, shown);
        pen.gap();
        pen.rule();

        // What the dictionary says scrolls under the part that asks.
        let header = pen.y;
        let started = header - self.scroll.offset;
        pen.y = started;
        if let Some(note) = &shown.note {
            pen.note(note);
        }
        for (index, line) in shown.lines.iter().enumerate() {
            self.line(&mut pen, index, line);
        }
        let reach = pen.y - started;
        self.scroll.reached(reach, bottom - header);
        self.scroll.draw_bar(canvas_of(&mut pen), left + WIDTH, header, bottom, theme);
    }

    /// The name of the pane, and the cross that shuts it.
    fn caption(&mut self, pen: &mut Pen<'_, '_, '_>, top: f32) {
        pen.words(t("Translator"), pen.left + PADDING, HEADING, pen.theme.text);
        let close_left = pen.left + WIDTH - 26.0;
        if self.hovered == Some(Hit::Close) {
            pen.canvas.fill_rect(close_left as i32 - 4, top as i32 + 6, 24, 22, pen.theme.hover);
        }
        icons::draw_sized(pen.canvas, Icon::Close, close_left, top + 10.0, 12.0, pen.theme.text);
        self.placed.push((Hit::Close, close_left - 4.0, top + 6.0, 24.0, 22.0));
        pen.y = top + 34.0;
        pen.rule();
    }

    /// From, the button that turns them round, and To.
    fn languages(&mut self, pen: &mut Pen<'_, '_, '_>, shown: &Shown) {
        let x = pen.left + PADDING;
        let inner = WIDTH - PADDING * 2.0;
        let swap = 30.0;
        let each = (inner - swap - 8.0) / 2.0;
        let y = pen.y;
        for (hit, label, name, at) in [
            (Hit::From, t("From"), &shown.from, x),
            (Hit::To, t("To"), &shown.to, x + each + swap + 8.0),
        ] {
            pen.y = y;
            pen.words(label, at, TEXT, pen.theme.dim_text);
            let field_top = y + 14.0;
            let background =
                if self.hovered == Some(hit) { pen.theme.hover } else { pen.theme.field };
            pen.canvas.fill_rect(at as i32, field_top as i32, each as i32, ROW as i32, background);
            outline(pen, at, field_top, each, ROW);
            let line =
                pen.engine.simple_line(name, at + 6.0, field_top + 14.0, TEXT, pen.theme.text);
            pen.renderer.draw_within(pen.canvas, &line, at, field_top, each - 16.0, ROW);
            super::dialog::chevron(
                pen.canvas,
                at + each - 14.0,
                field_top + ROW / 2.0,
                pen.theme.dim_text,
            );
            self.placed.push((hit, at, field_top, each, ROW));
        }
        let swap_left = x + each + 4.0;
        let swap_top = y + 14.0;
        if self.hovered == Some(Hit::Swap) {
            pen.canvas.fill_rect(
                swap_left as i32,
                swap_top as i32,
                swap as i32,
                ROW as i32,
                pen.theme.hover,
            );
        }
        let arrows = pen.engine.simple_line(
            "\u{21C4}",
            swap_left + 8.0,
            swap_top + 15.0,
            11.0,
            pen.theme.text,
        );
        pen.renderer.draw_onto(pen.canvas, &arrows, 0.0, 0.0);
        self.placed.push((Hit::Swap, swap_left, swap_top, swap, ROW));
        pen.y = y + 14.0 + ROW + 10.0;
    }

    /// The box, with what is being translated in it and the caret when it
    /// has the keyboard.
    fn text_box(&mut self, pen: &mut Pen<'_, '_, '_>, shown: &Shown) {
        let x = pen.left + PADDING;
        let width = WIDTH - PADDING * 2.0;
        let top = pen.y;
        let background = pen.theme.field;
        pen.canvas.fill_rect(x as i32, top as i32, width as i32, BOX as i32, background);
        let edge = if shown.caret.is_some() { pen.theme.accent } else { pen.theme.field_edge };
        pen.canvas.fill_rect(x as i32, top as i32, width as i32, 1, edge);
        pen.canvas.fill_rect(x as i32, (top + BOX) as i32 - 1, width as i32, 1, edge);
        pen.canvas.fill_rect(x as i32, top as i32, 1, BOX as i32, edge);
        pen.canvas.fill_rect((x + width) as i32 - 1, top as i32, 1, BOX as i32, edge);

        let text_x = x + 6.0;
        let baseline = top + 17.0;
        if shown.text.is_empty() && shown.caret.is_none() {
            let hint = pen.engine.simple_line(
                t("Type a word to translate"),
                text_x,
                baseline,
                TEXT,
                pen.theme.dim_text,
            );
            pen.renderer.draw_within(pen.canvas, &hint, x, top, width - 6.0, BOX);
        } else {
            let line = pen.engine.simple_line(&shown.text, text_x, baseline, TEXT, pen.theme.text);
            pen.renderer.draw_within(pen.canvas, &line, x, top, width - 6.0, BOX);
        }
        if let Some(caret) = shown.caret {
            let before: String = shown.text.chars().take(caret).collect();
            let measured = pen.engine.simple_line(&before, text_x, baseline, TEXT, pen.theme.text);
            let caret_x = if before.is_empty() { text_x } else { measured.width };
            pen.canvas.fill_rect(
                caret_x as i32,
                top as i32 + 5,
                1,
                BOX as i32 - 10,
                pen.theme.text,
            );
        }
        self.box_start = text_x;
        self.placed.push((Hit::Box, x, top, width, BOX));
        pen.y = top + BOX;
    }

    /// One line of what the dictionary said.
    fn line(&mut self, pen: &mut Pen<'_, '_, '_>, index: usize, line: &Line) {
        let x = pen.left + PADDING;
        let width = WIDTH - PADDING * 2.0;
        match line {
            Line::Word(word) => {
                pen.y += 4.0;
                if pen.showing(ROW) {
                    let drawn =
                        pen.engine.simple_line(word, x, pen.y + 14.0, HEADING, pen.theme.text);
                    pen.renderer.draw_within(pen.canvas, &drawn, x, pen.y, width, ROW);
                }
                pen.y += ROW;
            }
            Line::Missing(word) => {
                let said = crate::messages::with("{0} \u{2014} no entry", &[word]);
                if pen.showing(ROW) {
                    let drawn =
                        pen.engine.simple_line(&said, x, pen.y + 14.0, TEXT, pen.theme.dim_text);
                    pen.renderer.draw_within(pen.canvas, &drawn, x, pen.y, width, ROW);
                }
                pen.y += ROW;
            }
            Line::Sense(sense) => {
                // A sense runs to as many lines as it needs: a dictionary's
                // list of translations is often wider than the pane, and the
                // one cut off is the one somebody wanted.
                let hit = Hit::Sense(index);
                let height = (pen.wrapped_height(sense, width - 16.0) + 5.0).max(ROW);
                if pen.showing(height) && self.hovered == Some(hit) {
                    pen.canvas.fill_rect(
                        x as i32,
                        pen.y as i32,
                        width as i32,
                        height as i32,
                        pen.theme.hover,
                    );
                }
                let colour = pen.theme.text;
                pen.wrapped(sense, x + 12.0, pen.y + 14.0, width - 16.0, colour);
                self.placed.push((hit, x, pen.y, width, height));
                pen.y += height;
            }
        }
    }
}

/// A line round a field.
fn outline(pen: &mut Pen<'_, '_, '_>, left: f32, top: f32, width: f32, height: f32) {
    let edge = pen.theme.field_edge;
    pen.canvas.fill_rect(left as i32, top as i32, width as i32, 1, edge);
    pen.canvas.fill_rect(left as i32, (top + height) as i32 - 1, width as i32, 1, edge);
    pen.canvas.fill_rect(left as i32, top as i32, 1, height as i32, edge);
    pen.canvas.fill_rect((left + width) as i32 - 1, top as i32, 1, height as i32, edge);
}

/// The canvas a pen draws on, for what is drawn after the pen is done.
fn canvas_of<'p>(pen: &'p mut Pen<'_, '_, '_>) -> &'p mut Canvas {
    pen.canvas
}
