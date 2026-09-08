//! The rulers along the top and the left of the page.
//!
//! # What a ruler is for
//!
//! Two things at once. It says how wide the paper is and where its margins
//! fall, so a person can see at a glance that the text sits an inch from the
//! edge. And it carries the markers that set a paragraph's indents, so those
//! measurements can be changed by dragging rather than typed into a box.
//!
//! The numbers count outwards from the left margin, positive to the right and
//! positive again to the left — which looks strange written down and is exactly
//! what a person expects, because both directions are measured *from the text*.

use wp_layout::{LayoutEngine, Renderer};
use wp_raster::{Canvas, Color};

use super::theme::Theme;

/// Height of the ruler across the top.
pub const HORIZONTAL_HEIGHT: f32 = 24.0;

// Where each part of the strip sits, measured down from its top. They are
// constants rather than numbers written twice because the drawing and the
// hit-testing have to agree about them exactly: a marker drawn a pixel from
// where it can be grabbed is a marker that does not work.

/// The first-line marker, which points down from the top.
const FIRST_LINE_TOP: f32 = 1.0;
/// How tall either triangle is.
const MARKER_HEIGHT: f32 = 5.0;
/// The band showing the paper and its margins.
const BAND_TOP: f32 = 5.0;
const BAND_HEIGHT: f32 = 10.0;
/// The hanging marker and the right one, which point up from below the band.
const HANGING_TOP: f32 = 13.0;
/// The square under the hanging marker, which moves both indents together.
const SQUARE_TOP: f32 = 18.0;
const SQUARE_HEIGHT: f32 = 4.0;
const SQUARE_WIDTH: f32 = 9.0;
/// Width of the ruler down the left side.
pub const VERTICAL_WIDTH: f32 = 20.0;

/// Where a paragraph's indents sit, in pixels from the left of the text area.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Indents {
    pub first_line: f32,
    pub start: f32,
    pub end: f32,
}

/// What the horizontal ruler needs to know.
#[derive(Clone, Copy, Debug)]
pub struct Measurements {
    /// Where the page sits on screen and how wide it is, in pixels.
    pub page_left: f32,
    pub page_width: f32,
    /// The margins, in pixels.
    pub margin_left: f32,
    pub margin_right: f32,
    /// How many pixels there are to the inch at the current zoom.
    pub pixels_per_inch: f32,
    pub indents: Indents,
}

/// Draws the ruler across the top of the page area.
pub fn draw_horizontal(
    canvas: &mut Canvas,
    engine: &mut LayoutEngine<'_>,
    renderer: &mut Renderer<'_>,
    top: f32,
    left_edge: f32,
    measurements: Measurements,
    theme: &Theme,
) {
    let width = canvas.width() as i32;
    canvas.fill_rect(left_edge as i32, top as i32, width, HORIZONTAL_HEIGHT as i32, theme.ribbon);
    canvas.fill_rect(
        left_edge as i32,
        (top + HORIZONTAL_HEIGHT - 1.0) as i32,
        width,
        1,
        theme.ribbon_edge,
    );

    if measurements.page_width <= 0.0 {
        return;
    }

    let band_top = top + BAND_TOP;
    let band_height = BAND_HEIGHT;
    let text_left = measurements.page_left + measurements.margin_left;
    let text_width =
        (measurements.page_width - measurements.margin_left - measurements.margin_right).max(0.0);

    canvas.fill_rect(
        measurements.page_left as i32,
        band_top as i32,
        measurements.page_width as i32,
        band_height as i32,
        theme.ruler_margin,
    );
    canvas.fill_rect(
        text_left as i32,
        band_top as i32,
        text_width as i32,
        band_height as i32,
        theme.ruler_paper,
    );

    // Marks every eighth of an inch, a taller one every half, and a number at
    // every whole inch — counting outwards from the left margin in both
    // directions, because both are measured from the text.
    let step = measurements.pixels_per_inch / 8.0;
    if step >= 2.0 {
        let mut index = -((text_left - measurements.page_left) / step).floor() as i32;
        let last = ((measurements.page_left + measurements.page_width - text_left) / step) as i32;

        while index <= last {
            let x = text_left + index as f32 * step;
            if x < measurements.page_left || x > measurements.page_left + measurements.page_width {
                index += 1;
                continue;
            }

            let inches = index.abs();
            if inches % 8 == 0 && inches != 0 {
                let text = (inches / 8).to_string();
                let measured = engine.simple_line(&text, 0.0, 0.0, 6.5, theme.ruler_tick);
                let line = engine.simple_line(
                    &text,
                    x - measured.width / 2.0,
                    band_top + 8.0,
                    6.5,
                    theme.ruler_tick,
                );
                renderer.draw_onto(canvas, &line, 0.0, 0.0);
            } else if inches % 4 == 0 {
                canvas.fill_rect(x as i32, (band_top + 3.0) as i32, 1, 5, theme.ruler_tick);
            } else {
                canvas.fill_rect(x as i32, (band_top + 4.0) as i32, 1, 3, theme.ruler_tick);
            }
            index += 1;
        }
    }

    // The indent markers: a triangle pointing down for the first line, one
    // pointing up for the rest, and one at the right edge.
    let first = text_left + measurements.indents.first_line;
    let start = text_left + measurements.indents.start;
    let end = measurements.page_left + measurements.page_width
        - measurements.margin_right
        - measurements.indents.end;

    marker(canvas, first, top + FIRST_LINE_TOP, true, theme.text);
    marker(canvas, start, top + HANGING_TOP, false, theme.text);
    marker(canvas, end, top + HANGING_TOP, false, theme.text);

    // The square under the hanging marker. Dragging it moves the whole
    // paragraph, first line and all, which is the one thing the two triangles
    // cannot do between them.
    canvas.fill_rect(
        (start - SQUARE_WIDTH / 2.0) as i32,
        (top + SQUARE_TOP) as i32,
        SQUARE_WIDTH as i32,
        SQUARE_HEIGHT as i32,
        theme.text,
    );
}

/// Draws the ruler down the left of the page area.
/// Where the page sits down the window, for the ruler beside it.
#[derive(Clone, Copy, Debug)]
pub struct Vertical {
    /// The band of the window the ruler covers.
    pub top: f32,
    pub bottom: f32,
    /// Where the page sits within it, and how tall it is.
    pub page_top: f32,
    pub page_height: f32,
    pub margin_top: f32,
    pub margin_bottom: f32,
    pub pixels_per_inch: f32,
}

pub fn draw_vertical(
    canvas: &mut Canvas,
    engine: &mut LayoutEngine,
    renderer: &mut Renderer,
    left: f32,
    measurements: Vertical,
    theme: &Theme,
) {
    let Vertical { top, bottom, page_top, page_height, margin_top, margin_bottom, pixels_per_inch } =
        measurements;
    canvas.fill_rect(
        left as i32,
        top as i32,
        VERTICAL_WIDTH as i32,
        (bottom - top) as i32,
        theme.ribbon,
    );
    canvas.fill_rect(
        (left + VERTICAL_WIDTH - 1.0) as i32,
        top as i32,
        1,
        (bottom - top) as i32,
        theme.ribbon_edge,
    );

    if page_height <= 0.0 {
        return;
    }

    let band_left = left + 5.0;
    let band_width = 10.0;
    let text_top = page_top + margin_top;
    let text_height = (page_height - margin_top - margin_bottom).max(0.0);

    // Only the part of the page that is on screen.
    let clip = |y: f32, height: f32| -> Option<(f32, f32)> {
        let start = y.max(top);
        let finish = (y + height).min(bottom);
        (finish > start).then_some((start, finish - start))
    };

    if let Some((y, height)) = clip(page_top, page_height) {
        canvas.fill_rect(
            band_left as i32,
            y as i32,
            band_width as i32,
            height as i32,
            theme.ruler_margin,
        );
    }
    if let Some((y, height)) = clip(text_top, text_height) {
        canvas.fill_rect(
            band_left as i32,
            y as i32,
            band_width as i32,
            height as i32,
            theme.ruler_paper,
        );
    }

    let step = pixels_per_inch / 8.0;
    if step < 2.0 {
        return;
    }
    // The numbers count outwards from the top margin, the same way the ones
    // across the top count from the left margin: what a ruler measures is the
    // text, not the paper.
    let mut index = -((text_top - page_top) / step).floor() as i32;
    let last = ((page_top + page_height - text_top) / step) as i32;

    while index <= last {
        let y = text_top + index as f32 * step;
        index += 1;
        if y < top || y > bottom || y < page_top || y > page_top + page_height {
            continue;
        }

        let inches = (index - 1).abs();
        if inches % 8 == 0 && inches != 0 {
            let text = (inches / 8).to_string();
            let measured = engine.simple_line(&text, 0.0, 0.0, 6.5, theme.ruler_tick);
            let line = engine.simple_line(
                &text,
                band_left + (band_width - measured.width) / 2.0,
                y + 2.5,
                6.5,
                theme.ruler_tick,
            );
            renderer.draw_onto(canvas, &line, 0.0, 0.0);
        } else if inches % 4 == 0 {
            canvas.fill_rect((band_left + 2.0) as i32, y as i32, 6, 1, theme.ruler_tick);
        } else {
            canvas.fill_rect((band_left + 3.0) as i32, y as i32, 4, 1, theme.ruler_tick);
        }
    }
}

/// One indent marker: a small triangle.
fn marker(canvas: &mut Canvas, x: f32, y: f32, pointing_down: bool, colour: Color) {
    let rows = MARKER_HEIGHT as i32;
    for step in 0..rows {
        let row = if pointing_down { y + step as f32 } else { y + (rows - 1 - step) as f32 };
        let half = rows - 1 - step;
        canvas.fill_rect((x - half as f32) as i32, row as i32, half * 2 + 1, 1, colour);
    }
}

/// What on the horizontal ruler a press landed on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    /// The triangle at the top: where the first line of the paragraph begins.
    FirstLine,
    /// The triangle at the bottom: where every line but the first begins.
    Hanging,
    /// The square under it, which moves both together.
    LeftIndent,
    /// The triangle at the right: where the lines end.
    RightIndent,
    /// The join between the grey band and the white one, on either side.
    LeftMargin,
    RightMargin,
}

/// What on the vertical ruler a press landed on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerticalHit {
    TopMargin,
    BottomMargin,
}

/// How near a marker a press has to be, in pixels either side.
///
/// Wider than the marker is drawn, because a triangle nine pixels across is
/// easy to see and hard to hit.
const GRAB: f32 = 6.0;

/// How near a margin boundary a press has to be.
///
/// Narrower than a marker, because a margin has no handle to aim at and a
/// generous reach here would steal presses meant for the markers beside it.
const MARGIN_GRAB: f32 = 4.0;

/// Where the three markers sit, left to right, on the strip.
fn marker_positions(measurements: Measurements) -> (f32, f32, f32) {
    let text_left = measurements.page_left + measurements.margin_left;
    let first = text_left + measurements.indents.first_line;
    let start = text_left + measurements.indents.start;
    let end = measurements.page_left + measurements.page_width
        - measurements.margin_right
        - measurements.indents.end;
    (first, start, end)
}

/// What a press at a point on the horizontal ruler is aiming at.
///
/// The markers are tested before the margins: where a paragraph has no indent
/// they sit on top of one another, and the marker is the thing a person means —
/// dragging a margin is the rarer act, and the one with somewhere else to be
/// asked for (Layout ▸ Margins).
#[must_use]
pub fn hit_horizontal(top: f32, measurements: Measurements, x: i32, y: i32) -> Option<Hit> {
    let (x, y) = (x as f32, y as f32);
    if y < top || y >= top + HORIZONTAL_HEIGHT || measurements.page_width <= 0.0 {
        return None;
    }

    let (first, start, end) = marker_positions(measurements);
    let near = |at: f32, reach: f32| (x - at).abs() <= reach;

    if y < top + FIRST_LINE_TOP + MARKER_HEIGHT + 2.0 && near(first, GRAB) {
        return Some(Hit::FirstLine);
    }
    if y >= top + HANGING_TOP - 1.0 {
        if near(end, GRAB) {
            return Some(Hit::RightIndent);
        }
        if y >= top + SQUARE_TOP - 1.0 && near(start, GRAB) {
            return Some(Hit::LeftIndent);
        }
        if near(start, GRAB) {
            return Some(Hit::Hanging);
        }
    }

    // The margins, on the band between the two rows of markers.
    let text_left = measurements.page_left + measurements.margin_left;
    let text_right = measurements.page_left + measurements.page_width - measurements.margin_right;
    if (top + BAND_TOP..top + HANGING_TOP - 1.0).contains(&y) {
        if near(text_left, MARGIN_GRAB) {
            return Some(Hit::LeftMargin);
        }
        if near(text_right, MARGIN_GRAB) {
            return Some(Hit::RightMargin);
        }
    }
    None
}

/// What a press at a point on the vertical ruler is aiming at.
#[must_use]
pub fn hit_vertical(left: f32, measurements: Vertical, x: i32, y: i32) -> Option<VerticalHit> {
    let (x, y) = (x as f32, y as f32);
    if x < left || x >= left + VERTICAL_WIDTH || measurements.page_height <= 0.0 {
        return None;
    }
    if y < measurements.top || y >= measurements.bottom {
        return None;
    }

    let text_top = measurements.page_top + measurements.margin_top;
    let text_bottom = measurements.page_top + measurements.page_height - measurements.margin_bottom;
    if (y - text_top).abs() <= GRAB {
        return Some(VerticalHit::TopMargin);
    }
    if (y - text_bottom).abs() <= GRAB {
        return Some(VerticalHit::BottomMargin);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{
        hit_horizontal, hit_vertical, Hit, Indents, Measurements, Vertical, VerticalHit,
        HORIZONTAL_HEIGHT, VERTICAL_WIDTH,
    };

    /// A page 8.5 inches across at 96 pixels to the inch, with inch margins,
    /// sitting 100 pixels from the left of the window.
    fn page() -> Measurements {
        Measurements {
            page_left: 100.0,
            page_width: 816.0,
            margin_left: 96.0,
            margin_right: 96.0,
            pixels_per_inch: 96.0,
            indents: Indents::default(),
        }
    }

    /// The same page down the side ruler, starting 50 pixels down the window.
    fn side() -> Vertical {
        Vertical {
            top: 40.0,
            bottom: 700.0,
            page_top: 50.0,
            page_height: 1056.0,
            margin_top: 96.0,
            margin_bottom: 96.0,
            pixels_per_inch: 96.0,
        }
    }

    const TOP: f32 = 60.0;

    #[test]
    fn nothing_outside_the_strip_is_hit() {
        assert_eq!(hit_horizontal(TOP, page(), 196, TOP as i32 - 1), None);
        assert_eq!(hit_horizontal(TOP, page(), 196, (TOP + HORIZONTAL_HEIGHT) as i32), None);
    }

    #[test]
    fn the_first_line_marker_is_at_the_top_of_the_text_edge() {
        // Text begins at 100 + 96 = 196.
        assert_eq!(hit_horizontal(TOP, page(), 196, TOP as i32 + 3), Some(Hit::FirstLine));
    }

    #[test]
    fn the_hanging_marker_is_below_the_band_at_the_same_place() {
        assert_eq!(hit_horizontal(TOP, page(), 196, TOP as i32 + 15), Some(Hit::Hanging));
    }

    #[test]
    fn the_square_is_below_the_hanging_marker() {
        assert_eq!(hit_horizontal(TOP, page(), 196, TOP as i32 + 20), Some(Hit::LeftIndent));
    }

    #[test]
    fn the_right_marker_is_at_the_other_edge_of_the_text() {
        // The text ends at 100 + 816 − 96 = 820.
        assert_eq!(hit_horizontal(TOP, page(), 820, TOP as i32 + 15), Some(Hit::RightIndent));
    }

    #[test]
    fn the_margins_are_on_the_band_between_the_two_rows_of_markers() {
        assert_eq!(hit_horizontal(TOP, page(), 196, TOP as i32 + 8), Some(Hit::LeftMargin));
        assert_eq!(hit_horizontal(TOP, page(), 820, TOP as i32 + 8), Some(Hit::RightMargin));
    }

    #[test]
    fn the_middle_of_the_ruler_is_nothing_at_all() {
        assert_eq!(hit_horizontal(TOP, page(), 500, TOP as i32 + 8), None);
        assert_eq!(hit_horizontal(TOP, page(), 500, TOP as i32 + 20), None);
    }

    #[test]
    fn a_marker_can_be_grabbed_a_few_pixels_either_side() {
        assert_eq!(hit_horizontal(TOP, page(), 191, TOP as i32 + 3), Some(Hit::FirstLine));
        assert_eq!(hit_horizontal(TOP, page(), 201, TOP as i32 + 3), Some(Hit::FirstLine));
        assert_eq!(hit_horizontal(TOP, page(), 210, TOP as i32 + 3), None);
    }

    #[test]
    fn an_indented_paragraph_moves_its_markers() {
        // Half an inch in at 96 pixels to the inch is 48 pixels.
        let measurements =
            Measurements { indents: Indents { first_line: 48.0, start: 48.0, end: 0.0 }, ..page() };
        assert_eq!(hit_horizontal(TOP, measurements, 244, TOP as i32 + 3), Some(Hit::FirstLine));
        assert_eq!(hit_horizontal(TOP, measurements, 196, TOP as i32 + 3), None);
        // The margin has not moved with them.
        assert_eq!(hit_horizontal(TOP, measurements, 196, TOP as i32 + 8), Some(Hit::LeftMargin));
    }

    #[test]
    fn a_hanging_indent_puts_the_two_triangles_in_different_places() {
        let measurements =
            Measurements { indents: Indents { first_line: 0.0, start: 48.0, end: 0.0 }, ..page() };
        assert_eq!(hit_horizontal(TOP, measurements, 196, TOP as i32 + 3), Some(Hit::FirstLine));
        assert_eq!(hit_horizontal(TOP, measurements, 244, TOP as i32 + 15), Some(Hit::Hanging));
    }

    #[test]
    fn a_page_of_no_width_has_nothing_to_hit() {
        let measurements = Measurements { page_width: 0.0, ..page() };
        assert_eq!(hit_horizontal(TOP, measurements, 196, TOP as i32 + 3), None);
    }

    #[test]
    fn the_side_ruler_has_a_margin_at_each_end_of_the_text() {
        // The text begins at 50 + 96 = 146 and ends at 50 + 1056 − 96 = 1010,
        // which is past the bottom of the window and so cannot be hit.
        assert_eq!(hit_vertical(10.0, side(), 15, 146), Some(VerticalHit::TopMargin));
        assert_eq!(hit_vertical(10.0, side(), 15, 1010), None);
    }

    #[test]
    fn the_bottom_margin_can_be_hit_once_it_is_scrolled_into_view() {
        let measurements = Vertical { page_top: -400.0, ..side() };
        // Now the text ends at −400 + 1056 − 96 = 560.
        assert_eq!(hit_vertical(10.0, measurements, 15, 560), Some(VerticalHit::BottomMargin));
    }

    #[test]
    fn nothing_beside_the_side_ruler_is_hit() {
        assert_eq!(hit_vertical(10.0, side(), 9, 146), None);
        assert_eq!(hit_vertical(10.0, side(), (10.0 + VERTICAL_WIDTH) as i32, 146), None);
    }

    #[test]
    fn the_middle_of_the_side_ruler_is_nothing_at_all() {
        assert_eq!(hit_vertical(10.0, side(), 15, 400), None);
    }
}
