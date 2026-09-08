//! The scroll that follows the pointer after a press of the middle button.
//!
//! # What it is
//!
//! Press the wheel and a mark appears where the pointer was; move away from it
//! and the document scrolls, faster the further away the pointer goes; press
//! any button, or a key, and it stops. Windows has done this since the wheel
//! was invented and Word does it too, so a hand that has learnt it expects it
//! everywhere.
//!
//! # How it moves
//!
//! The distance from the mark decides the speed, with a few pixels of nothing
//! around the mark so that a hand resting still does not creep down the page.
//! The clock decides when: the same tick the caret blinks on, which is often
//! enough to look smooth and costs nothing while nothing is happening.

use wp_shell::Response;

use super::Editor;

/// How far the pointer must be from the mark before anything moves.
const DEAD: f32 = 12.0;
/// How much of the distance is taken each tick.
const SPEED: f32 = 0.06;
/// How big the mark is.
const MARK: f32 = 16.0;

impl Editor {
    /// Starts the scroll, or stops one that is already running.
    pub(super) fn toggle_autoscroll(&mut self, x: i32, y: i32) -> Response {
        self.autoscroll = match self.autoscroll {
            Some(_) => None,
            // Only over the pages: the wheel pressed on the ribbon is not
            // somebody asking to scroll the document.
            None if (y as f32) > self.content_top() && (y as f32) < self.window_bottom() => {
                Some((x, y))
            }
            None => None,
        };
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Whether it is running.
    #[must_use]
    pub(super) fn autoscrolling(&self) -> bool {
        self.autoscroll.is_some()
    }

    /// Stops it. Returns whether it was running.
    pub(super) fn stop_autoscroll(&mut self) -> bool {
        if self.autoscroll.take().is_some() {
            self.needs_redraw = true;
            return true;
        }
        false
    }

    /// One tick of it: moves the document by however far the pointer is from
    /// the mark.
    pub(super) fn autoscroll_tick(&mut self) -> Response {
        let Some((_, anchor_y)) = self.autoscroll else { return Response::Ignored };
        let away = self.pointer_y - anchor_y as f32;
        if away.abs() < DEAD {
            return Response::Ignored;
        }
        // The dead zone is taken off the distance, so the document creeps
        // rather than jumping the moment the pointer leaves it.
        let past = away.signum() * (away.abs() - DEAD);
        self.scroll_by(past * SPEED)
    }

    /// Draws the mark the scroll is measured from.
    pub(super) fn draw_autoscroll_mark(&mut self) {
        let Some((x, y)) = self.autoscroll else { return };
        let colour = self.theme.accent;
        let half = MARK / 2.0;

        // A ring with a dot in it, which is what Windows draws: something to
        // aim back at when the page has gone far enough.
        for step in 0..(MARK as i32) {
            let along = step as f32 - half;
            let across = (half * half - along * along).max(0.0).sqrt();
            self.canvas.fill_rect(
                (x as f32 + along) as i32,
                (y as f32 - across) as i32,
                1,
                1,
                colour,
            );
            self.canvas.fill_rect(
                (x as f32 + along) as i32,
                (y as f32 + across) as i32,
                1,
                1,
                colour,
            );
        }
        self.canvas.fill_rect(x - 1, y - 1, 3, 3, colour);
    }
}
