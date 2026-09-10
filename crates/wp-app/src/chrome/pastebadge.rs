//! The little button that appears at the end of a paste.
//!
//! # What it is for
//!
//! A paste is a guess. The words came from somewhere with its own font and its
//! own styles, and the program has to decide whether they arrive dressed as
//! they were or dressed as their new surroundings. Word does not ask first —
//! that would put a dialog in front of the commonest gesture in the program —
//! it pastes, and then leaves this button at the end of what it put down. Press
//! it and the other answers are there; ignore it and it goes at the next thing
//! you do.
//!
//! # Why it says "(Ctrl)"
//!
//! Because pressing Control on its own opens it, which is the only way to reach
//! the options without letting go of the keyboard. Word prints the key on the
//! button rather than expecting anybody to know, and so does this — see
//! `Event::ControlKey`.

use wp_layout::{LayoutEngine, Renderer};
use wp_raster::Canvas;

use super::icons::{self, Icon};
use super::theme::Theme;

pub const WIDTH: f32 = 58.0;
pub const HEIGHT: f32 = 22.0;

/// How far below the end of the pasted text it sits.
///
/// Enough that it does not touch the line above, and no more: it points at what
/// was pasted, so it has to look attached to it.
pub const DROP: f32 = 3.0;

/// Where the button is and whether the pointer is on it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PasteBadge {
    pub left: f32,
    pub top: f32,
    pub hot: bool,
}

impl PasteBadge {
    #[must_use]
    pub fn new(left: f32, top: f32) -> Self {
        Self { left, top, hot: false }
    }

    /// Whether a point is on it.
    #[must_use]
    pub fn covers(&self, x: i32, y: i32) -> bool {
        let (x, y) = (x as f32, y as f32);
        x >= self.left && x < self.left + WIDTH && y >= self.top && y < self.top + HEIGHT
    }

    pub fn draw(
        &self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        theme: &Theme,
    ) {
        let (left, top) = (self.left as i32, self.top as i32);
        let background = if self.hot { theme.hover } else { theme.field };
        canvas.fill_rect(left, top, WIDTH as i32, HEIGHT as i32, background);

        // A line round it, so it reads as a button sitting on the page rather
        // than as something printed on it.
        let edge = theme.field_edge;
        canvas.fill_rect(left, top, WIDTH as i32, 1, edge);
        canvas.fill_rect(left, top + HEIGHT as i32 - 1, WIDTH as i32, 1, edge);
        canvas.fill_rect(left, top, 1, HEIGHT as i32, edge);
        canvas.fill_rect(left + WIDTH as i32 - 1, top, 1, HEIGHT as i32, edge);

        icons::draw_sized(
            canvas,
            Icon::Clipboard,
            self.left + 3.0,
            self.top + 3.0,
            16.0,
            theme.text,
        );
        let line = engine.simple_line("(Ctrl)", self.left + 21.0, self.top + 15.0, 7.5, theme.text);
        renderer.draw_onto(canvas, &line, 0.0, 0.0);
        chevron(canvas, self.left + WIDTH - 9.0, self.top + HEIGHT / 2.0, theme.dim_text);
    }
}

/// The little arrow that says a list drops from here.
fn chevron(canvas: &mut Canvas, x: f32, y: f32, colour: wp_raster::Color) {
    for step in 0..3 {
        let step = step as f32;
        canvas.fill_rect((x - 2.0 + step) as i32, (y - 1.0 + step) as i32, 1, 1, colour);
        canvas.fill_rect((x + 2.0 - step) as i32, (y - 1.0 + step) as i32, 1, 1, colour);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_is_pressed_where_it_is_drawn() {
        let badge = PasteBadge::new(100.0, 200.0);
        assert!(badge.covers(101, 201));
        assert!(badge.covers(100 + WIDTH as i32 - 1, 200 + HEIGHT as i32 - 1));
        assert!(!badge.covers(99, 201));
        assert!(!badge.covers(100 + WIDTH as i32, 201));
        assert!(!badge.covers(101, 200 + HEIGHT as i32));
    }
}
