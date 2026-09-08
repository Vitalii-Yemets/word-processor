//! The scroll bars: where they are, what they say, and what pressing them does.
//!
//! One down the side, and one across the bottom when the page is wider than
//! the window — at a big zoom, or on wide paper. Without the second, the right
//! edge of the page cannot be reached at all.
//!
//! Turned side to side the view scrolls across instead of down, and then the
//! bar along the bottom is that scroll rather than a second one. See
//! [`super::views::Movement`].

use wp_shell::Response;

use crate::chrome::scrollbar::{Hit, ScrollBar, THICKNESS};

use super::Editor;

impl Editor {
    /// Where the bar is and what it is showing, or nothing when the whole
    /// document is already on screen.
    #[must_use]
    pub(super) fn scroll_bar(&self) -> Option<ScrollBar> {
        let (extent, visible) = self.scroll_extent();
        let limit = (extent - visible).max(0.0);
        let (top, bottom) = self.whole_content_band();

        let bar = if self.is_side_to_side() {
            ScrollBar {
                left: self.content_left(),
                top: bottom - THICKNESS,
                length: (self.view_width as f32 - self.content_left()).max(1.0),
                vertical: false,
                position: self.scroll,
                limit,
                visible,
                extent,
            }
        } else {
            ScrollBar {
                left: (self.view_width as f32 - THICKNESS).max(0.0),
                top,
                length: (bottom - top).max(1.0),
                vertical: true,
                position: self.scroll,
                limit,
                visible,
                extent,
            }
        };
        bar.is_needed().then_some(bar)
    }

    /// The bar across the bottom, when the page is too wide to fit.
    ///
    /// Not there at all while the whole width is on screen, which is the usual
    /// case — a bar that says everything is visible is a bar in the way.
    #[must_use]
    pub(super) fn across_scroll_bar(&self) -> Option<ScrollBar> {
        if self.is_side_to_side() {
            return None;
        }
        let limit = self.across_limit();
        let visible = self.viewport_width();
        let (_, bottom) = self.whole_content_band();
        let bar = ScrollBar {
            left: self.content_left(),
            top: bottom - THICKNESS,
            length: (self.view_width as f32 - self.content_left() - THICKNESS).max(1.0),
            vertical: false,
            position: self.scroll_across,
            limit,
            visible,
            extent: visible + limit,
        };
        bar.is_needed().then_some(bar)
    }

    /// Draws whichever bars are needed.
    pub(super) fn draw_scrollbar(&mut self) {
        let theme = self.theme;
        let (pointer_x, pointer_y) = (self.pointer_x as i32, self.pointer_y as i32);

        match self.scroll_bar() {
            Some(bar) => {
                let hovered =
                    self.scroll_grab.is_some() || bar.hit(pointer_x, pointer_y) == Some(Hit::Thumb);
                bar.draw(&mut self.canvas, &theme, hovered);
                self.scrollbar = Some(bar);
            }
            None => self.scrollbar = None,
        }

        match self.across_scroll_bar() {
            Some(bar) => {
                let hovered =
                    self.across_grab.is_some() || bar.hit(pointer_x, pointer_y) == Some(Hit::Thumb);
                bar.draw(&mut self.canvas, &theme, hovered);
                self.across_bar = Some(bar);
            }
            None => self.across_bar = None,
        }
    }

    /// Whether a point is on either bar.
    #[must_use]
    pub(super) fn on_scrollbar(&self, x: i32, y: i32) -> bool {
        self.scrollbar.is_some_and(|bar| bar.covers(x, y))
            || self.across_bar.is_some_and(|bar| bar.covers(x, y))
    }

    /// Acts on a press.
    pub(super) fn press_on_scrollbar(&mut self, x: i32, y: i32) -> Response {
        // The one across the bottom is asked first: where the two meet, the
        // corner belongs to neither, and asking the bar the point is actually
        // inside settles it.
        if let Some(bar) = self.across_bar {
            if let Some(hit) = bar.hit(x, y) {
                let page = (bar.visible * 0.9).max(1.0);
                return match hit {
                    Hit::Back => self.scroll_across_by(-super::SCROLL_PER_NOTCH),
                    Hit::Forward => self.scroll_across_by(super::SCROLL_PER_NOTCH),
                    Hit::PageBack => self.scroll_across_by(-page),
                    Hit::PageForward => self.scroll_across_by(page),
                    Hit::Thumb => {
                        self.across_grab = Some(bar.grab_offset(x, y));
                        Response::Redraw
                    }
                };
            }
        }

        let Some(bar) = self.scrollbar else { return Response::Ignored };
        let Some(hit) = bar.hit(x, y) else { return Response::Ignored };

        // A screenful less a little, so a line stays on screen to read on from
        // — which is what every reader expects and nobody asks for.
        let page = (bar.visible * 0.9).max(1.0);
        match hit {
            Hit::Back => self.scroll_by(-super::SCROLL_PER_NOTCH),
            Hit::Forward => self.scroll_by(super::SCROLL_PER_NOTCH),
            Hit::PageBack => self.scroll_by(-page),
            Hit::PageForward => self.scroll_by(page),
            Hit::Thumb => {
                self.scroll_grab = Some(bar.grab_offset(x, y));
                Response::Redraw
            }
        }
    }

    /// Follows the pointer while a thumb is being dragged.
    pub(super) fn drag_scrollbar(&mut self, x: i32, y: i32) -> Response {
        if let (Some(bar), Some(grab)) = (self.across_bar, self.across_grab) {
            self.scroll_across = bar.position_at(x, y, grab);
            self.clamp_across();
            self.needs_redraw = true;
            return Response::Redraw;
        }
        let (Some(bar), Some(grab)) = (self.scrollbar, self.scroll_grab) else {
            return Response::Ignored;
        };
        self.scroll = bar.position_at(x, y, grab);
        self.clamp_scroll();
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Whether a thumb is being dragged.
    #[must_use]
    pub(super) fn dragging_scrollbar(&self) -> bool {
        self.scroll_grab.is_some() || self.across_grab.is_some()
    }

    /// Lets go of it.
    pub(super) fn release_scrollbar(&mut self) {
        self.scroll_grab = None;
        self.across_grab = None;
    }
}
