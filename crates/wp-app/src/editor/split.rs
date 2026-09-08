//! Two views of one document in one window.
//!
//! # What a split actually is
//!
//! Not two documents and not two carets: one document, shown twice, scrolled to
//! two different places. It is how a person quotes page forty in a footnote on
//! page one without losing their place.
//!
//! # How it is done here
//!
//! The window has one viewport, and everything — where a page is drawn, where a
//! click lands, how far a wheel scrolls — is measured from its top and bottom.
//! A split does not change any of that. It makes the viewport mean *the pane
//! that was last clicked in*, and draws the other pane by pretending, for the
//! length of one draw, that the other one is active.
//!
//! So there is exactly one new idea in the drawing and none at all in the
//! editing. The alternative — passing a pane through every function that
//! measures anything — would have touched thirty places to say the same thing.

use wp_shell::Response;

use super::Editor;

/// How thick the bar between the panes is.
pub(super) const BAR: f32 = 6.0;
/// How near the edge the bar may be dragged, as a share of the viewport.
const NEAREST_EDGE: f32 = 0.1;

impl Editor {
    /// Splits the window in two, or puts it back together.
    pub(super) fn toggle_split(&mut self) -> Response {
        if self.split.is_some() {
            self.split = None;
            self.active_pane = 0;
            self.needs_redraw = true;
            return self.report("Split removed");
        }

        // Halfway down, which is where Word puts it before anybody drags it.
        self.split = Some(0.5);
        self.other_scroll = self.scroll;
        self.active_pane = 0;
        self.clamp_scroll();
        self.needs_redraw = true;
        self.report("Split — drag the bar to move it, or press Split again")
    }

    /// Whether the window is showing two views.
    #[must_use]
    pub(super) fn is_split(&self) -> bool {
        self.split.is_some()
    }

    /// Where the bar sits, in window coordinates.
    ///
    /// Measured from the whole content band rather than from the active pane,
    /// because the bar is a property of the window and not of either pane.
    #[must_use]
    pub(super) fn split_bar_top(&self) -> Option<f32> {
        let share = self.split?;
        let (top, bottom) = self.whole_content_band();
        Some(top + (bottom - top) * share - BAR / 2.0)
    }

    /// The top and bottom of one pane, in window coordinates.
    #[must_use]
    pub(super) fn pane_band(&self, pane: usize) -> (f32, f32) {
        let (top, bottom) = self.whole_content_band();
        let Some(bar) = self.split_bar_top() else { return (top, bottom) };
        if pane == 0 {
            (top, bar)
        } else {
            (bar + BAR, bottom)
        }
    }

    /// Whether a point is on the bar between the panes.
    #[must_use]
    pub(super) fn on_split_bar(&self, y: f32) -> bool {
        self.split_bar_top().is_some_and(|top| y >= top && y < top + BAR)
    }

    /// Which pane a point is in.
    #[must_use]
    pub(super) fn pane_at(&self, y: f32) -> usize {
        match self.split_bar_top() {
            Some(bar) if y >= bar + BAR => 1,
            _ => 0,
        }
    }

    /// Makes the pane a click landed in the one that is being edited.
    ///
    /// The two panes' scroll positions swap places, because the active pane's
    /// is the one everything else calls "the scroll".
    pub(super) fn activate_pane(&mut self, pane: usize) {
        if !self.is_split() || pane == self.active_pane {
            return;
        }
        core::mem::swap(&mut self.scroll, &mut self.other_scroll);
        self.active_pane = pane;
        self.clamp_scroll();
        self.needs_redraw = true;
    }

    /// Starts dragging the bar.
    pub(super) fn start_split_drag(&mut self) -> Response {
        self.split_dragging = true;
        Response::Redraw
    }

    /// Moves the bar to where the pointer is.
    pub(super) fn drag_split_to(&mut self, y: f32) -> Response {
        if !self.split_dragging {
            return Response::Ignored;
        }
        let (top, bottom) = self.whole_content_band();
        let height = (bottom - top).max(1.0);
        let share = ((y - top) / height).clamp(NEAREST_EDGE, 1.0 - NEAREST_EDGE);
        self.split = Some(share);
        self.clamp_scroll();
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Draws the pane that is not being edited, and the bar between them.
    ///
    /// The other pane is drawn by making it the active one for as long as it
    /// takes to draw it, which is the whole trick: everything it needs already
    /// measures from whichever pane is active.
    pub(super) fn draw_other_pane(&mut self) {
        if !self.is_split() {
            return;
        }
        let other = 1 - self.active_pane;
        let (top, bottom) = self.pane_band(other);

        let previous_clip = self.canvas.set_clip(
            0,
            top as i32,
            self.view_width as i32,
            (bottom - top).max(0.0) as i32,
        );
        // The desk under it, so the active pane's overflow does not show
        // through where this pane's paper does not reach.
        let desk = self.theme.desk;
        self.canvas.fill_rect(
            0,
            top as i32,
            self.view_width as i32,
            (bottom - top).max(0.0) as i32,
            desk,
        );

        core::mem::swap(&mut self.scroll, &mut self.other_scroll);
        self.active_pane = other;
        self.draw_pages();
        self.draw_caret();
        self.active_pane = 1 - other;
        core::mem::swap(&mut self.scroll, &mut self.other_scroll);

        self.canvas.restore_clip(previous_clip);

        if let Some(bar) = self.split_bar_top() {
            let colour = self.theme.pane_edge;
            self.canvas.fill_rect(0, bar as i32, self.view_width as i32, BAR as i32, colour);
        }
    }
}
