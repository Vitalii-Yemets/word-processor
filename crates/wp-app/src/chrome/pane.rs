//! What every pane down the side of the window is drawn with.
//!
//! # Why there is one of these
//!
//! Because a pane is a column of things, each of which knows its own height,
//! and every one of them needs the same three tools and one number that moves
//! down. Passing those separately to a dozen little methods is how a drawing
//! function grows nine arguments; writing them out again in the next pane is
//! how two panes end up knowing different things.
//!
//! That second one is not hypothetical. The Styles pane could be scrolled and
//! the Restrict Editing pane could not, which was never decided — it was two
//! copies of the same idea, and only one of them had been taught. A pane that
//! runs off the bottom of the window with no way to reach the rest of it is a
//! pane hiding what a person asked to see.
//!
//! # How a pane is scrolled
//!
//! By where the pen starts. Everything a pane draws is placed by running a
//! number down the column, so beginning that number higher up the window
//! moves the whole column. What stops it running away is knowing how far the
//! column reached last time, which the pen remembers: see [`Pen::reach`].
//!
//! Drawing off the top or the bottom is not clipped, because nothing is drawn
//! there: a pen past either edge draws nothing at all and only moves on.

use wp_layout::{LayoutEngine, Renderer};
use wp_raster::{Canvas, Color};

use super::theme::Theme;

/// The room round everything a pane draws.
pub const PADDING: f32 = 10.0;

/// The size a pane's words are drawn at, and the size a heading is.
pub const TEXT: f32 = 8.5;
pub const HEADING: f32 = 9.0;

/// How far one line of a broken sentence sits below the last.
const LINE: f32 = 13.0;

/// How wide the bar down the edge of a scrolled pane is.
const BAR: f32 = 4.0;

/// Where the next thing goes, and what draws it.
pub struct Pen<'a, 'e, 'r> {
    pub canvas: &'a mut Canvas,
    pub engine: &'a mut LayoutEngine<'e>,
    pub renderer: &'a mut Renderer<'r>,
    pub theme: &'a Theme,
    /// The pane's left edge and how wide it is.
    pub left: f32,
    pub width: f32,
    /// Where the next thing goes.
    pub y: f32,
    /// The window's bottom, past which nothing is drawn.
    pub bottom: f32,
    /// The window's top, above which nothing is drawn either: a pane scrolled
    /// down has its first rows above the window, and they must not be painted
    /// over the ribbon.
    pub top: f32,
}

impl Pen<'_, '_, '_> {
    /// Whether the pen is somewhere worth drawing.
    ///
    /// A pane scrolled down has rows above the window and a tall one has rows
    /// below it; neither is drawn, and both still move the pen on, because
    /// how far the column reaches is what says how far it may be scrolled.
    #[must_use]
    pub fn showing(&self, height: f32) -> bool {
        self.y + height > self.top && self.y < self.bottom
    }

    /// One line of words at the pen, returning how wide it came out.
    pub fn words(&mut self, text: &str, x: f32, size: f32, colour: Color) -> f32 {
        // Measured either way, since a caller may want the width to put
        // something beside it, and measuring costs nothing to draw.
        let line = self.engine.simple_line(text, x, self.y + size, size, colour);
        let width = line.width;
        if self.showing(size * 2.0) {
            self.renderer.draw_onto(self.canvas, &line, 0.0, 0.0);
        }
        width
    }

    /// A sentence broken to fit a width, returning how tall it came out.
    ///
    /// Broken here rather than laid out: a pane's words are the program's own
    /// and short, and a paragraph engine to set three of them would be a
    /// second way of doing what the document already does.
    pub fn wrapped(&mut self, text: &str, x: f32, baseline: f32, width: f32, colour: Color) -> f32 {
        let mut y = baseline;
        let mut line = String::new();
        for word in text.split_whitespace() {
            let wanted = if line.is_empty() { word.to_owned() } else { format!("{line} {word}") };
            let measured = self.engine.simple_line(&wanted, 0.0, 0.0, TEXT, colour);
            if measured.width > width && !line.is_empty() {
                self.put(&line, x, y, colour);
                y += LINE;
                line = word.to_owned();
            } else {
                line = wanted;
            }
        }
        if !line.is_empty() {
            self.put(&line, x, y, colour);
        }
        y - baseline + 15.0
    }

    /// How tall a sentence broken to fit a width would come out, without
    /// drawing it: what a row that holds one needs to know before it draws
    /// what goes behind it.
    pub fn wrapped_height(&mut self, text: &str, width: f32) -> f32 {
        let mut lines = 0usize;
        let mut line = String::new();
        let colour = self.theme.text;
        for word in text.split_whitespace() {
            let wanted = if line.is_empty() { word.to_owned() } else { format!("{line} {word}") };
            let measured = self.engine.simple_line(&wanted, 0.0, 0.0, TEXT, colour);
            if measured.width > width && !line.is_empty() {
                lines += 1;
                line = word.to_owned();
            } else {
                line = wanted;
            }
        }
        if !line.is_empty() {
            lines += 1;
        }
        lines.saturating_sub(1) as f32 * LINE + 15.0
    }

    /// One line, drawn where it is asked for rather than at the pen.
    fn put(&mut self, text: &str, x: f32, baseline: f32, colour: Color) {
        if baseline < self.top || baseline > self.bottom + LINE {
            return;
        }
        let line = self.engine.simple_line(text, x, baseline, TEXT, colour);
        self.renderer.draw_onto(self.canvas, &line, 0.0, 0.0);
    }

    /// The name of a section.
    pub fn heading(&mut self, text: &str) {
        let colour = self.theme.text;
        self.words(text, self.left + PADDING, HEADING, colour);
        self.y += 20.0;
    }

    /// A sentence that is read and not pressed.
    pub fn note(&mut self, text: &str) {
        let colour = self.theme.dim_text;
        let x = self.left + PADDING;
        let width = self.width - PADDING * 2.0;
        let used = self.wrapped(text, x, self.y + TEXT, width, colour);
        self.y += used - 7.0;
    }

    /// A line across the pane.
    pub fn rule(&mut self) {
        if self.showing(1.0) {
            let colour = self.theme.pane_edge;
            self.canvas.fill_rect(
                (self.left + PADDING) as i32,
                self.y as i32,
                (self.width - PADDING * 2.0) as i32,
                1,
                colour,
            );
        }
        self.y += 10.0;
    }

    /// The room between one section and the next.
    pub fn gap(&mut self) {
        self.y += 8.0;
    }
}

/// How far down a pane has been scrolled, and how far it reaches.
///
/// Kept by the pane rather than worked out afresh, because how far a pane may
/// be scrolled depends on how tall what it drew turned out to be, and that is
/// only known once it has been drawn.
#[derive(Clone, Copy, Debug, Default)]
pub struct Scroll {
    /// How far down, in pixels.
    pub offset: f32,
    /// How far the column reached the last time it was drawn, measured from
    /// the top of the pane.
    reach: f32,
    /// And how tall the window was for it.
    room: f32,
}

impl Scroll {
    /// How far it could be scrolled: what is left below the window.
    #[must_use]
    pub fn most(&self) -> f32 {
        (self.reach - self.room).max(0.0)
    }

    /// Whether there is more than there is room for.
    #[must_use]
    pub fn overflows(&self) -> bool {
        self.most() > 0.0
    }

    /// Moves it, and says whether it moved.
    ///
    /// Clamped both ways: a pane cannot be scrolled above its first row, and
    /// scrolling past the last one would leave a person looking at nothing
    /// and wondering what they had done.
    pub fn by(&mut self, pixels: f32) -> bool {
        let wanted = (self.offset + pixels).clamp(0.0, self.most());
        if (wanted - self.offset).abs() < f32::EPSILON {
            return false;
        }
        self.offset = wanted;
        true
    }

    /// Remembers what the last drawing came to, and pulls the offset back if
    /// the pane has shrunk since — a person who deletes what they scrolled
    /// down to should not be left looking below the end of it.
    pub fn reached(&mut self, reach: f32, room: f32) {
        self.reach = reach;
        self.room = room;
        self.offset = self.offset.min(self.most());
    }

    /// Draws the bar that says how much there is and where in it we are.
    ///
    /// Only when there is more than there is room for: a bar down a pane that
    /// fits would be saying there is something below when there is not.
    pub fn draw_bar(&self, canvas: &mut Canvas, left: f32, top: f32, bottom: f32, theme: &Theme) {
        if !self.overflows() {
            return;
        }
        let room = bottom - top;
        if room <= 0.0 || self.reach <= 0.0 {
            return;
        }
        // As much of the bar as the window is of the whole, and never so
        // small that there is nothing to see.
        let height = (room * room / self.reach).max(20.0).min(room);
        let travel = room - height;
        let at = top + travel * (self.offset / self.most()).clamp(0.0, 1.0);
        canvas.fill_rect(
            (left - BAR - 2.0) as i32,
            at as i32,
            BAR as i32,
            height as i32,
            theme.control_edge,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scrolled(reach: f32, room: f32) -> Scroll {
        let mut scroll = Scroll::default();
        scroll.reached(reach, room);
        scroll
    }

    #[test]
    fn a_pane_that_fits_cannot_be_scrolled() {
        // And says so, so that nothing draws a bar beside it claiming there
        // is more below.
        let mut scroll = scrolled(300.0, 500.0);
        assert!(!scroll.overflows());
        assert_eq!(scroll.most(), 0.0);
        assert!(!scroll.by(40.0), "it moved");
        assert_eq!(scroll.offset, 0.0);
    }

    #[test]
    fn one_that_does_not_fit_scrolls_as_far_as_what_is_left_below() {
        let mut scroll = scrolled(800.0, 500.0);
        assert!(scroll.overflows());
        assert_eq!(scroll.most(), 300.0);

        assert!(scroll.by(100.0));
        assert_eq!(scroll.offset, 100.0);
        // And no further than the end: past it a person is looking at nothing
        // and wondering what they did.
        assert!(scroll.by(1000.0));
        assert_eq!(scroll.offset, 300.0);
        assert!(!scroll.by(1000.0), "it went past the end");
    }

    #[test]
    fn it_cannot_be_scrolled_above_its_first_row() {
        let mut scroll = scrolled(800.0, 500.0);
        scroll.by(100.0);
        assert!(scroll.by(-1000.0));
        assert_eq!(scroll.offset, 0.0);
    }

    #[test]
    fn the_bar_is_drawn_only_when_there_is_more_than_there_is_room_for() {
        // A bar down a pane that fits would be saying there is something
        // below when there is not, and one missing from a pane that does not
        // fit is the whole complaint this item was written about.
        let theme = Theme::light();
        let ink = |scroll: &Scroll| {
            let mut canvas = Canvas::new(300, 200);
            canvas.fill_rect(0, 0, 300, 200, Color::rgb(0xFF, 0xFF, 0xFF));
            scroll.draw_bar(&mut canvas, 280.0, 20.0, 180.0, &theme);
            (0..200)
                .filter(|y| (270..280).any(|x| canvas.pixel(x, *y) != Color::rgb(0xFF, 0xFF, 0xFF)))
                .count()
        };

        assert_eq!(ink(&scrolled(100.0, 160.0)), 0, "a pane that fits was given a bar");
        let tall = scrolled(800.0, 160.0);
        assert!(ink(&tall) > 0, "a pane that does not fit was given none");

        // And it moves down as the pane is scrolled.
        let mut at_the_foot = tall;
        at_the_foot.by(1000.0);
        let top_rows = |scroll: &Scroll| {
            let mut canvas = Canvas::new(300, 200);
            canvas.fill_rect(0, 0, 300, 200, Color::rgb(0xFF, 0xFF, 0xFF));
            scroll.draw_bar(&mut canvas, 280.0, 20.0, 180.0, &theme);
            (0..200)
                .find(|y| (270..280).any(|x| canvas.pixel(x, *y) != Color::rgb(0xFF, 0xFF, 0xFF)))
        };
        assert!(
            top_rows(&at_the_foot) > top_rows(&tall),
            "the bar did not move down with the pane"
        );
    }

    #[test]
    fn a_pane_that_shrinks_pulls_the_view_back_with_it() {
        // Somebody scrolled to the foot of a long list and then deleted most
        // of it. Leaving the view where it was would leave them looking below
        // the end of what is there.
        let mut scroll = scrolled(800.0, 500.0);
        scroll.by(300.0);
        assert_eq!(scroll.offset, 300.0);

        scroll.reached(550.0, 500.0);
        assert_eq!(scroll.offset, 50.0, "the view was left below the end");

        scroll.reached(200.0, 500.0);
        assert_eq!(scroll.offset, 0.0);
    }
}
