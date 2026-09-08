//! When the mini toolbar comes up, and what a press on it does.
//!
//! # When it appears
//!
//! When a selection is made with the mouse, and when the right button opens a
//! menu. Not when the selection is made from the keyboard: a hand on the keys
//! is not reaching for a floating toolbar, and Word does not show it either.
//!
//! # When it goes
//!
//! When the pointer wanders away from it, when anything is typed, when the
//! selection goes, when the view scrolls, and on Escape — every one of which is
//! somebody saying they were not after it.

use wp_shell::Response;

use std::time::Instant;

use crate::chrome::MiniBar;

use super::Editor;

impl Editor {
    /// Floats the bar over a point, faint until the pointer comes to it.
    pub(super) fn show_mini_bar(&mut self, x: i32, y: i32) {
        if self.document.selection().is_none() {
            return;
        }
        let room =
            (self.content_left(), self.content_top(), self.view_width as f32, self.window_bottom());
        self.mini_bar = Some(MiniBar::new(x as f32, y as f32, room));
        self.needs_redraw = true;
    }

    /// The same, solid at once, which is how it comes up with the menu.
    pub(super) fn show_mini_bar_awake(&mut self, x: i32, y: i32) {
        self.show_mini_bar(x, y);
        if let Some(bar) = &mut self.mini_bar {
            bar.wake();
        }
    }

    /// Takes it away. Returns whether there was one.
    pub(super) fn hide_mini_bar(&mut self) -> bool {
        if self.mini_bar.take().is_some() {
            self.needs_redraw = true;
            return true;
        }
        false
    }

    /// Follows the pointer over the bar, and gives it up once the pointer has
    /// gone well past it. Returns whether the window has to be drawn again.
    pub(super) fn mini_bar_moved(&mut self, x: i32, y: i32) -> bool {
        let Some(bar) = &mut self.mini_bar else { return false };
        if bar.abandoned(x, y) {
            return self.hide_mini_bar();
        }
        if bar.hover(x, y) {
            // The pointer has moved to another of its buttons, so a tip that was
            // showing is about the wrong one and the wait starts again.
            self.tip = None;
            self.hovered_since = Instant::now();
            self.needs_redraw = true;
            return true;
        }
        false
    }

    /// Whether a press landed on the bar, and what it means if it did.
    #[must_use]
    pub(super) fn mini_bar_press(&mut self, x: i32, y: i32) -> Option<Response> {
        let bar = self.mini_bar.as_ref()?;
        if !bar.covers(x, y) {
            return None;
        }
        let Some(command) = bar.hit(x, y) else {
            // A press on the bar but between its buttons: it is still the bar
            // that was pressed, so nothing else may act on it.
            return Some(Response::Ignored);
        };

        // A list dropped from one of its boxes hangs under that box.
        self.popup_anchor = bar.anchor(command);
        let response = self.run(command);
        self.popup_anchor = None;

        // The bar stays while it is being used, which is what lets somebody
        // press bold and then italic — but anything dropped open replaces it,
        // because the two would sit on top of one another.
        if self.popup.is_some() || self.palette.is_some() || self.table_grid.is_some() {
            self.mini_bar = None;
        }
        self.needs_redraw = true;
        Some(response)
    }
}
