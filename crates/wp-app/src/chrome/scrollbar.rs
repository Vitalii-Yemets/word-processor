//! The bar down the right of the document, and the one across the bottom.
//!
//! # Why a program with a wheel still needs one
//!
//! Because it says where you are. A wheel moves the page but tells you nothing
//! about how much of the document is above and below what you can see, and a
//! long document with no bar is one nobody can judge the length of. It is also
//! the only way to move a long way at once with a mouse, and the only thing on
//! screen that a person can grab to do it.
//!
//! # What it is made of
//!
//! A track, a thumb sized by how much of the document fits on screen, and an
//! arrow at each end. Pressing the track above or below the thumb moves a
//! screenful, which is what every scroll bar has done since they were invented
//! and what a person expects without being told.

use wp_raster::Canvas;

use super::theme::Theme;

/// How wide the bar is, and how tall the horizontal one is.
pub const THICKNESS: f32 = 14.0;
/// The arrow at each end is square, so this is its length too.
const ARROW: f32 = THICKNESS;
/// The thumb never gets shorter than this, however long the document is.
const SHORTEST_THUMB: f32 = 24.0;

/// Where a bar is and what it is showing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollBar {
    /// The track, in window coordinates.
    pub left: f32,
    pub top: f32,
    pub length: f32,
    /// Whether it runs down the side rather than across the bottom.
    pub vertical: bool,
    /// How far the scroll has run, and how far it may.
    pub position: f32,
    pub limit: f32,
    /// How much of the document is on screen at once.
    pub visible: f32,
    pub extent: f32,
}

/// What a press on the bar landed on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    /// The arrow at the top or the left.
    Back,
    /// The arrow at the bottom or the right.
    Forward,
    /// The thumb itself, which is dragged.
    Thumb,
    /// The track before the thumb: a screenful backwards.
    PageBack,
    /// And after it.
    PageForward,
}

impl ScrollBar {
    /// Whether there is anything to scroll.
    #[must_use]
    pub fn is_needed(&self) -> bool {
        self.limit > 0.5 && self.length > ARROW * 2.0 + SHORTEST_THUMB
    }

    /// How long the thumb is: the share of the document that is on screen.
    #[must_use]
    fn thumb_length(&self) -> f32 {
        let room = self.room();
        if self.extent <= 0.0 {
            return room;
        }
        (room * (self.visible / self.extent)).clamp(SHORTEST_THUMB.min(room), room)
    }

    /// The room the thumb slides in, between the two arrows.
    #[must_use]
    fn room(&self) -> f32 {
        (self.length - ARROW * 2.0).max(1.0)
    }

    /// Where the thumb starts, measured along the track.
    #[must_use]
    fn thumb_start(&self) -> f32 {
        let travel = (self.room() - self.thumb_length()).max(0.0);
        let share =
            if self.limit > 0.0 { (self.position / self.limit).clamp(0.0, 1.0) } else { 0.0 };
        ARROW + travel * share
    }

    /// What a point on the bar is.
    #[must_use]
    pub fn hit(&self, x: i32, y: i32) -> Option<Hit> {
        if !self.covers(x, y) {
            return None;
        }
        let along = if self.vertical { y as f32 - self.top } else { x as f32 - self.left };

        if along < ARROW {
            return Some(Hit::Back);
        }
        if along > self.length - ARROW {
            return Some(Hit::Forward);
        }
        let start = self.thumb_start();
        if along < start {
            return Some(Hit::PageBack);
        }
        if along > start + self.thumb_length() {
            return Some(Hit::PageForward);
        }
        Some(Hit::Thumb)
    }

    /// Whether a point is on the bar at all.
    #[must_use]
    pub fn covers(&self, x: i32, y: i32) -> bool {
        let (x, y) = (x as f32, y as f32);
        let (width, height) =
            if self.vertical { (THICKNESS, self.length) } else { (self.length, THICKNESS) };
        x >= self.left && x < self.left + width && y >= self.top && y < self.top + height
    }

    /// Where the scroll should go for the thumb to be under a point.
    ///
    /// `grab` is how far down the thumb the person took hold of it, so the
    /// thumb does not jump under the pointer when the drag starts.
    #[must_use]
    pub fn position_at(&self, x: i32, y: i32, grab: f32) -> f32 {
        let along = if self.vertical { y as f32 - self.top } else { x as f32 - self.left };
        let travel = (self.room() - self.thumb_length()).max(1.0);
        let share = ((along - grab - ARROW) / travel).clamp(0.0, 1.0);
        share * self.limit
    }

    /// How far down the thumb a point is, for a drag to hold on to.
    #[must_use]
    pub fn grab_offset(&self, x: i32, y: i32) -> f32 {
        let along = if self.vertical { y as f32 - self.top } else { x as f32 - self.left };
        (along - self.thumb_start()).clamp(0.0, self.thumb_length())
    }

    /// Draws it.
    pub fn draw(&self, canvas: &mut Canvas, theme: &Theme, hovered: bool) {
        if !self.is_needed() {
            return;
        }
        let (width, height) =
            if self.vertical { (THICKNESS, self.length) } else { (self.length, THICKNESS) };
        canvas.fill_rect(
            self.left as i32,
            self.top as i32,
            width as i32,
            height as i32,
            theme.ribbon,
        );

        let start = self.thumb_start();
        let thumb = self.thumb_length();
        // A little narrower than the track, which is what leaves the thumb
        // looking like something sitting in a groove rather than filling it.
        let inset = 3.0;
        // Light against a light track and dark against a dark one, and one
        // step stronger under the pointer so it reads as something to take
        // hold of.
        let colour = if hovered { theme.dim_text } else { theme.disabled_text };
        if self.vertical {
            canvas.fill_rect(
                (self.left + inset) as i32,
                (self.top + start) as i32,
                (THICKNESS - inset * 2.0) as i32,
                thumb as i32,
                colour,
            );
        } else {
            canvas.fill_rect(
                (self.left + start) as i32,
                (self.top + inset) as i32,
                thumb as i32,
                (THICKNESS - inset * 2.0) as i32,
                colour,
            );
        }

        self.draw_arrow(canvas, theme, true);
        self.draw_arrow(canvas, theme, false);
    }

    /// One of the two arrows, pointing away from the middle.
    fn draw_arrow(&self, canvas: &mut Canvas, theme: &Theme, back: bool) {
        let along = if back { 0.0 } else { self.length - ARROW };
        let (x, y) = if self.vertical {
            (self.left + THICKNESS / 2.0, self.top + along + ARROW / 2.0)
        } else {
            (self.left + along + ARROW / 2.0, self.top + THICKNESS / 2.0)
        };

        // Four rows of a triangle, which at this size is as much of one as
        // there is room to draw.
        let colour = theme.disabled_text;
        for step in 0..4i32 {
            let half = if back { step } else { 3 - step };
            let offset = step as f32 - 1.5;
            if self.vertical {
                canvas.fill_rect(
                    (x - half as f32) as i32,
                    (y + offset) as i32,
                    half * 2 + 1,
                    1,
                    colour,
                );
            } else {
                canvas.fill_rect(
                    (x + offset) as i32,
                    (y - half as f32) as i32,
                    1,
                    half * 2 + 1,
                    colour,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(position: f32, limit: f32, visible: f32, extent: f32) -> ScrollBar {
        ScrollBar {
            left: 100.0,
            top: 50.0,
            length: 300.0,
            vertical: true,
            position,
            limit,
            visible,
            extent,
        }
    }

    #[test]
    fn a_document_that_fits_needs_no_bar() {
        assert!(!bar(0.0, 0.0, 500.0, 400.0).is_needed());
    }

    #[test]
    fn a_document_longer_than_the_view_needs_one() {
        assert!(bar(0.0, 600.0, 400.0, 1000.0).is_needed());
    }

    #[test]
    fn the_thumb_shows_how_much_of_the_document_is_on_screen() {
        let half = bar(0.0, 400.0, 400.0, 800.0).thumb_length();
        let quarter = bar(0.0, 1200.0, 400.0, 1600.0).thumb_length();
        assert!(half > quarter, "{half} is not longer than {quarter}");
    }

    #[test]
    fn the_thumb_never_disappears_however_long_the_document_is() {
        let thumb = bar(0.0, 100_000.0, 400.0, 100_400.0).thumb_length();
        assert!(thumb >= SHORTEST_THUMB, "the thumb is {thumb} long");
    }

    #[test]
    fn the_thumb_is_at_the_top_at_the_start_and_the_foot_at_the_end() {
        let bar = bar(0.0, 600.0, 400.0, 1000.0);
        assert!((bar.thumb_start() - ARROW).abs() < 0.01);

        let ended = ScrollBar { position: 600.0, ..bar };
        let expected = ARROW + (ended.room() - ended.thumb_length());
        assert!((ended.thumb_start() - expected).abs() < 0.01);
    }

    #[test]
    fn every_part_of_the_bar_is_something() {
        let bar = bar(0.0, 600.0, 400.0, 1000.0);
        assert_eq!(bar.hit(105, 55), Some(Hit::Back), "the top arrow");
        assert_eq!(bar.hit(105, 340), Some(Hit::Forward), "the bottom arrow");
        assert_eq!(bar.hit(105, 70), Some(Hit::Thumb), "the thumb");
        assert_eq!(bar.hit(105, 300), Some(Hit::PageForward), "the track below it");
    }

    #[test]
    fn a_point_off_the_bar_is_nothing() {
        let bar = bar(0.0, 600.0, 400.0, 1000.0);
        assert_eq!(bar.hit(50, 100), None);
        assert_eq!(bar.hit(105, 400), None);
    }

    #[test]
    fn dragging_the_thumb_to_the_foot_scrolls_to_the_end() {
        let bar = bar(0.0, 600.0, 400.0, 1000.0);
        assert!((bar.position_at(105, 400, 0.0) - 600.0).abs() < 0.01);
    }

    #[test]
    fn dragging_it_back_to_the_top_scrolls_to_the_start() {
        let bar = bar(300.0, 600.0, 400.0, 1000.0);
        assert!(bar.position_at(105, 0, 0.0).abs() < 0.01);
    }

    #[test]
    fn the_thumb_does_not_jump_under_the_pointer_when_it_is_grabbed() {
        let bar = bar(300.0, 600.0, 400.0, 1000.0);
        // Taken hold of halfway down the thumb, and not moved.
        let start = bar.top + bar.thumb_start() + bar.thumb_length() / 2.0;
        let grab = bar.grab_offset(105, start as i32);
        let after = bar.position_at(105, start as i32, grab);
        assert!((after - 300.0).abs() < 1.0, "the thumb jumped to {after}");
    }

    #[test]
    fn a_bar_across_the_bottom_measures_the_other_way() {
        let across = ScrollBar { vertical: false, ..bar(0.0, 600.0, 400.0, 1000.0) };
        assert_eq!(across.hit(105, 55), Some(Hit::Back), "the left arrow");
        assert_eq!(across.hit(390, 55), Some(Hit::Forward), "the right arrow");
        assert_eq!(across.hit(105, 100), None, "below the bar");
    }
}
