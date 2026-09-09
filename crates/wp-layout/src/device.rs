//! What a page is being drawn for: a screen, or a printer.
//!
//! Two things differ between them and nothing else should.
//!
//! **How fine it draws.** A screen is about ninety-six dots to the inch, a
//! printer six hundred or twelve hundred. The layout is worked out in points
//! and scaled by this, so the same document must break its lines and its pages
//! in the same places on both — otherwise print preview is a picture of a
//! different document from the one that comes out of the printer.
//!
//! **How much of the paper it can reach.** A printer holds the sheet by its
//! edges and cannot draw in the band it holds — a quarter of an inch on a
//! typical office printer, more at the end the paper is gripped by. Word warns
//! when a document's margins are inside that band, because the text there will
//! simply be missing.

use crate::PageMetrics;

/// The band of paper a device cannot draw in, in points.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Unprintable {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Unprintable {
    /// The same band on all four sides, which is what a printer that reports
    /// one number means.
    #[must_use]
    pub fn all(points: f32) -> Self {
        Self { left: points, top: points, right: points, bottom: points }
    }
}

/// A device pages are laid out and drawn for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Device {
    /// Dots per inch.
    pub dpi: f32,
    /// What it cannot reach.
    pub unprintable: Unprintable,
}

impl Default for Device {
    fn default() -> Self {
        Self::screen()
    }
}

impl Device {
    /// A screen: ninety-six dots to the inch, and every pixel of the page is
    /// reachable.
    #[must_use]
    pub fn screen() -> Self {
        Self { dpi: 96.0, unprintable: Unprintable::default() }
    }

    /// A printer at a given resolution, with nothing yet known about the band
    /// it cannot draw in — which is what a device that reports none means.
    #[must_use]
    pub fn printer(dpi: f32) -> Self {
        Self { dpi, unprintable: Unprintable::default() }
    }

    /// The same device, told what it cannot reach.
    #[must_use]
    pub fn with_unprintable(mut self, unprintable: Unprintable) -> Self {
        self.unprintable = unprintable;
        self
    }

    /// How many device dots one point is.
    #[must_use]
    pub fn dots_per_point(&self) -> f32 {
        self.dpi / 72.0
    }

    /// A length in points, in the device's own dots.
    #[must_use]
    pub fn dots(&self, points: f32) -> f32 {
        points * self.dots_per_point()
    }

    /// The part of the paper the device can draw in, in points, as left, top,
    /// width and height.
    #[must_use]
    pub fn printable(&self, paper: &PageMetrics) -> (f32, f32, f32, f32) {
        let left = self.unprintable.left;
        let top = self.unprintable.top;
        let width = (paper.width - left - self.unprintable.right).max(0.0);
        let height = (paper.height - top - self.unprintable.bottom).max(0.0);
        (left, top, width, height)
    }

    /// Whether the document's margins keep its text inside what the device can
    /// reach.
    ///
    /// Word asks this before printing and offers to widen the margins; a
    /// document written for one printer and printed on another is the ordinary
    /// way of arriving at a page whose text runs into the band the paper is
    /// held by.
    #[must_use]
    pub fn holds(&self, paper: &PageMetrics) -> bool {
        paper.margin_left >= self.unprintable.left
            && paper.margin_top >= self.unprintable.top
            && paper.margin_right >= self.unprintable.right
            && paper.margin_bottom >= self.unprintable.bottom
    }

    /// The same page setup with its margins widened to what the device can
    /// reach, which is what Word's "Fix" does.
    #[must_use]
    pub fn widened(&self, paper: &PageMetrics) -> PageMetrics {
        PageMetrics {
            margin_left: paper.margin_left.max(self.unprintable.left),
            margin_top: paper.margin_top.max(self.unprintable.top),
            margin_right: paper.margin_right.max(self.unprintable.right),
            margin_bottom: paper.margin_bottom.max(self.unprintable.bottom),
            ..*paper
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Device, Unprintable};
    use crate::PageMetrics;

    #[test]
    fn a_screen_reaches_the_whole_page() {
        let paper = PageMetrics::default();
        let screen = Device::screen();
        let (left, top, width, height) = screen.printable(&paper);
        assert_eq!((left, top), (0.0, 0.0));
        assert_eq!((width, height), (paper.width, paper.height));
        assert!(screen.holds(&paper));
    }

    #[test]
    fn a_printer_cannot_reach_the_edge_it_holds_the_paper_by() {
        // A quarter of an inch, which is eighteen points.
        let printer = Device::printer(600.0).with_unprintable(Unprintable::all(18.0));
        let paper = PageMetrics::default();
        let (left, top, width, height) = printer.printable(&paper);
        assert_eq!((left, top), (18.0, 18.0));
        assert!((width - (paper.width - 36.0)).abs() < 0.01);
        assert!((height - (paper.height - 36.0)).abs() < 0.01);
    }

    #[test]
    fn margins_inside_that_band_are_noticed_and_can_be_widened() {
        let printer = Device::printer(600.0).with_unprintable(Unprintable::all(18.0));
        let narrow = PageMetrics { margin_left: 5.0, ..PageMetrics::default() };
        assert!(!printer.holds(&narrow), "the text would run off the paper unnoticed");

        let fixed = printer.widened(&narrow);
        assert_eq!(fixed.margin_left, 18.0);
        assert!(printer.holds(&fixed));
        // Only what had to move, moved.
        assert_eq!(fixed.margin_top, narrow.margin_top);
    }

    #[test]
    fn a_length_in_points_becomes_the_devices_own_dots() {
        assert_eq!(Device::screen().dots(72.0), 96.0);
        assert_eq!(Device::printer(600.0).dots(72.0), 600.0);
    }
}
