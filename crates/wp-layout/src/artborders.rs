//! The art borders that are a pattern rather than a picture.
//!
//! # What is drawn here and what is not
//!
//! Word's Art gallery is about a hundred and sixty repeating pictures. Most of
//! them are artwork it ships — apples, hearts, gingerbread men — and this
//! program draws nothing it did not make, so those are not offered and a
//! document carrying one keeps it and is drawn with a plain line of its width.
//!
//! The rest are not pictures at all. `basicBlackSquares` is a row of black
//! squares, `triangles` is a row of triangles, `checkered` is a checkerboard,
//! `northwest` is hatching that leans one way. Those describe themselves, a row
//! of black squares drawn from rectangles is a row of black squares, and they
//! are what this file draws. Which names they are is [`wp_docx::art::DRAWN`].
//!
//! # Everything out of rectangles
//!
//! The same as the line styles beside them, and for the same reason: a page
//! carries rectangles of colour, and a triangle is a row of them getting
//! shorter. A slope is a ladder of one-pixel rungs, each tall enough to meet
//! the next, which is what makes it read as a line and not as a staircase.
//!
//! # The band, and the two directions
//!
//! An art border is wide — Word measures it in whole points, one to thirty-one,
//! where a line is in eighths of one — so the patterns are drawn to fill a band
//! rather than along a line. Everything here is written in terms of the band's
//! thickness, so the same pattern at four points and at thirty looks like
//! itself; and everything goes through [`Ink`], which already knows whether
//! this edge runs across the page or down it.
//!
//! # Grey without knowing the paper
//!
//! Word has `sawtoothGray` and `diamondsGray`, and grey is halfway between the
//! ink and the paper. Nothing here knows the colour of the paper — a page is
//! whatever the theme says it is. So grey is the ink at half its opacity, which
//! *is* halfway to the paper whatever the paper is, and stays right when the
//! window is dark.

use wp_raster::Color;

use crate::borders::{lightened, parallel, Ink};

/// How much of the ink is left in the patterns Word calls grey.
const GREY: u8 = 128;

/// How thick a drawn line is inside a pattern, as a part of the band.
const STROKE: f32 = 0.16;

/// Draws one of the art borders, if it is one this program knows.
///
/// Returns whether anything was drawn: a name that is art and is not a pattern
/// — one of Word's pictures — is answered `false`, and the caller draws a plain
/// line of the border's width instead, which is what keeps such a document
/// looking like a bordered document rather than like nothing at all.
pub(crate) fn draw(ink: &mut Ink<'_>, style: &str, run: f32, band: f32) -> bool {
    // Nothing sensible can be drawn in no room at all.
    if run <= 0.0 || band <= 0.0 {
        return false;
    }

    match style {
        // --- Rows of marks -------------------------------------------------
        "basicBlackDashes" => marks(ink, run, band, band * 1.4, band * 0.6, 1.0),
        "basicBlackDots" => marks(ink, run, band, band * 0.5, band * 0.5, 0.5),
        "basicBlackSquares" => marks(ink, run, band, band, band * 0.35, 1.0),

        // --- The same, cut out of a bar ------------------------------------
        // The marks are the paper showing through, so they are not drawn: what
        // is drawn is the bar around them. That way "white" is whatever colour
        // the page is, which is the only reading that survives a dark window.
        "basicWhiteDashes" => holes(ink, run, band, band * 1.4, band * 0.6, 0.7),
        "basicWhiteDots" => holes(ink, run, band, band * 0.5, band * 0.5, 0.5),
        "basicWhiteSquares" => holes(ink, run, band, band * 0.7, band * 0.35, 0.7),

        // --- Bands of lines along the edge ---------------------------------
        "basicThinLines" => parallel(ink, run, band, &[1.0, 2.0, 1.0, 2.0, 1.0]),
        "basicWideInline" => parallel(ink, run, band, &[9.0, 2.0, 9.0]),
        "basicWideMidline" => parallel(ink, run, band, &[6.0, 2.0, 4.0, 2.0, 6.0]),
        "basicWideOutline" => parallel(ink, run, band, &[3.0, 14.0, 3.0]),
        "gradient" => gradient(ink, run, band),

        // --- Squares -------------------------------------------------------
        "checkered" => checkerboard(ink, run, band, false),
        "checkedBarBlack" => checkerboard(ink, run, band, true),
        "quadrants" => quadrants(ink, run, band),
        "shadowedSquares" => shadowed_squares(ink, run, band),
        "eclipsingSquares1" => squares(ink, run, band, band * 0.55, 1),
        "eclipsingSquares2" => squares(ink, run, band, band * 1.1, 2),

        // --- Triangles and diamonds ----------------------------------------
        "triangles" => triangles(ink, run, band, false),
        "sharksTeeth" => triangles(ink, run, band, true),
        "diamondsGray" => diamonds(ink, run, band),

        // --- Lines that go up and down --------------------------------------
        "sawtooth" => zigzag(ink, run, band, 1.4, ink.colour, false),
        "sawtoothGray" => zigzag(ink, run, band, 1.4, grey(ink.colour), false),
        "zigZag" => zigzag(ink, run, band, 0.7, ink.colour, false),
        "zigZagStitch" => zigzag(ink, run, band, 0.7, ink.colour, true),
        "waveline" => waveline(ink, run, band),
        "classicalWave" => meander(ink, run, band),

        // --- Crossings and hatching -----------------------------------------
        "crossStitch" => cross_stitch(ink, run, band),
        "northwest" => hatching(ink, run, band, true),
        "southwest" => hatching(ink, run, band, false),

        // One of Word's pictures. Not drawn here, and not drawn wrongly.
        _ => return false,
    }
    true
}

/// The ink at half its opacity, which is grey over any paper.
fn grey(colour: Color) -> Color {
    Color::rgba(colour.red, colour.green, colour.blue, GREY)
}

/// One mark of a pattern, kept inside the band.
///
/// A rung of a slope is drawn tall enough to meet the next one, which at the
/// last rung of a stroke would reach past the edge of the band — and a border
/// that put ink outside its own width would creep off the paper. So the mark is
/// trimmed to the band, and one trimmed away to nothing is not drawn at all.
fn within(ink: &mut Ink<'_>, band: f32, along: f32, length: f32, offset: f32, weight: f32) {
    let half = band / 2.0;
    let top = offset.max(-half);
    let bottom = (offset + weight).min(half);
    ink.piece(along, length, top, bottom - top);
}

/// The same, in a colour of its own.
fn within_coloured(
    ink: &mut Ink<'_>,
    band: f32,
    along: f32,
    length: f32,
    offset: f32,
    weight: f32,
    colour: Color,
) {
    let half = band / 2.0;
    let top = offset.max(-half);
    let bottom = (offset + weight).min(half);
    ink.coloured(along, length, top, bottom - top, colour);
}

/// A row of marks along the band: how long each is, how far apart, and how much
/// of the band's thickness it takes.
fn marks(ink: &mut Ink<'_>, run: f32, band: f32, mark: f32, gap: f32, fill: f32) {
    let step = (mark + gap).max(1.0);
    let weight = (band * fill).max(1.0);
    let offset = -weight / 2.0;
    let mut along = 0.0;
    while along < run {
        ink.piece(along, mark.min(run - along), offset, weight);
        along += step;
    }
}

/// A bar with those marks cut out of it.
///
/// Drawn as the bar around the holes: a strip above them, a strip below, and a
/// piece of the full band in each gap. The holes themselves are never drawn, so
/// what shows through them is the page.
fn holes(ink: &mut Ink<'_>, run: f32, band: f32, hole: f32, gap: f32, fill: f32) {
    let rim = (band * (1.0 - fill) / 2.0).max(1.0);
    let half = band / 2.0;
    // The rims, which are what makes it a bar rather than a row of marks.
    ink.piece(0.0, run, -half, rim);
    ink.piece(0.0, run, half - rim, rim);

    let middle = (band - rim * 2.0).max(1.0);
    let step = (hole + gap).max(1.0);
    let mut along = 0.0;
    while along < run {
        // The gap between one hole and the next, at full height.
        let at = along + hole;
        if at < run {
            ink.piece(at, gap.min(run - at), -half + rim, middle);
        }
        along += step;
    }
}

/// A band that fades from the ink to the paper across its width.
///
/// Strips of falling opacity rather than of a lighter colour, because the
/// colour it fades into is the page's and nothing here knows what that is.
fn gradient(ink: &mut Ink<'_>, run: f32, band: f32) {
    let steps = (band.round() as usize).clamp(2, 24);
    let weight = band / steps as f32;
    let half = band / 2.0;
    for step in 0..steps {
        let left = 1.0 - step as f32 / steps as f32;
        let colour = Color::rgba(
            ink.colour.red,
            ink.colour.green,
            ink.colour.blue,
            (f32::from(ink.colour.alpha) * left) as u8,
        );
        within_coloured(ink, band, 0.0, run, -half + weight * step as f32, weight, colour);
    }
}

/// Squares in two rows, filled one row and then the other.
///
/// `bar` fills the near row solid instead, which is the difference between
/// Word's checkerboard and its checked bar.
fn checkerboard(ink: &mut Ink<'_>, run: f32, band: f32, bar: bool) {
    let half = (band / 2.0).max(1.0);
    if bar {
        ink.piece(0.0, run, -half, half);
    }

    let mut along = 0.0;
    let mut at = 0usize;
    while along < run {
        let length = half.min(run - along);
        // One row or the other, turn and turn about — and only the far row when
        // the near one is a solid bar.
        let offset = if at % 2 == 0 && !bar { -half } else { 0.0 };
        ink.piece(along, length, offset, half);
        along += half;
        at += 1;
    }
}

/// Squares quartered, with two quarters filled across the diagonal.
fn quadrants(ink: &mut Ink<'_>, run: f32, band: f32) {
    let half = (band / 2.0).max(1.0);
    let mut along = 0.0;
    while along < run {
        let length = half.min(run - along);
        ink.piece(along, length, -half, half);
        let second = along + half;
        if second < run {
            ink.piece(second, half.min(run - second), 0.0, half);
        }
        along += band;
    }
}

/// Squares each with a shadow falling away from it.
fn shadowed_squares(ink: &mut Ink<'_>, run: f32, band: f32) {
    let size = band * 0.62;
    let drop = band * 0.16;
    let step = (size + drop + band * 0.3).max(1.0);
    let top = -band / 2.0;
    let mut along = 0.0;
    while along < run {
        // The shadow first, under the square it belongs to, and lighter than it
        // so that it reads as a shadow and not as a second square.
        let shade = lightened(ink.colour, 0.55);
        ink.coloured(along + drop, (size).min(run - along), top + drop, size, shade);
        ink.piece(along, size.min(run - along), top, size);
        along += step;
    }
}

/// Outlined squares along the band.
///
/// `inside` is how many outlines each square has: one, and they are set close
/// enough to overlap their neighbours, or two, one inside the other.
fn squares(ink: &mut Ink<'_>, run: f32, band: f32, step: f32, inside: usize) {
    let stroke = (band * STROKE).max(1.0);
    let mut along = 0.0;
    while along < run {
        for ring in 0..inside.max(1) {
            let inset = band * 0.25 * ring as f32;
            let size = band - inset * 2.0;
            if size <= stroke * 2.0 {
                continue;
            }
            let top = -band / 2.0 + inset;
            let left = along + inset;
            if left >= run {
                continue;
            }
            let width = size.min(run - left);
            // Four sides: two along the band and two across it.
            ink.piece(left, width, top, stroke);
            ink.piece(left, width, top + size - stroke, stroke);
            ink.piece(left, stroke.min(width), top, size);
            let far = left + size - stroke;
            if far < run {
                ink.piece(far, stroke.min(run - far), top, size);
            }
        }
        along += step.max(1.0);
    }
}

/// A row of triangles.
///
/// `leaning` makes them right-angled and all leaning the same way, which is a
/// saw; otherwise they are even-sided and point away from the page.
fn triangles(ink: &mut Ink<'_>, run: f32, band: f32, leaning: bool) {
    let width = band.max(1.0);
    let mut along = 0.0;
    while along < run {
        let mut step = 0.0;
        while step < width && along + step < run {
            let across = step / width;
            // Even-sided: tallest in the middle. Leaning: tallest at the end.
            let part = if leaning { across } else { 1.0 - (across * 2.0 - 1.0).abs() };
            let height = (band * part).max(1.0);
            ink.piece(along + step, 1.0f32.min(run - along - step), band / 2.0 - height, height);
            step += 1.0;
        }
        along += width;
    }
}

/// A row of diamonds, in Word's grey.
fn diamonds(ink: &mut Ink<'_>, run: f32, band: f32) {
    let width = band.max(1.0);
    let colour = grey(ink.colour);
    let mut along = 0.0;
    while along < run {
        let mut step = 0.0;
        while step < width && along + step < run {
            let across = step / width;
            let part = 1.0 - (across * 2.0 - 1.0).abs();
            let height = (band * part).max(1.0);
            ink.coloured(
                along + step,
                1.0f32.min(run - along - step),
                -height / 2.0,
                height,
                colour,
            );
            step += 1.0;
        }
        along += width;
    }
}

/// A line that goes up and down as it travels.
///
/// `steepness` is how far along the band one rise takes, as a multiple of the
/// band: a short one is a zigzag, a long one a saw. `broken` lifts the pen
/// between one stroke and the next, which is what makes a row of stitches.
fn zigzag(ink: &mut Ink<'_>, run: f32, band: f32, steepness: f32, colour: Color, broken: bool) {
    let reach = (band * steepness).max(2.0);
    let stroke = (band * STROKE).max(1.0);
    let top = -band / 2.0;
    let travel = band - stroke;

    let mut along = 0.0;
    let mut up = true;
    while along < run {
        let length = reach.min(run - along);
        let mut step = 0.0;
        while step < length {
            let part = step / reach;
            let offset = if up { top + travel * part } else { top + travel * (1.0 - part) };
            // A stitch is the middle of each stroke, not the whole of it.
            let stitching = broken && (part < 0.2 || part > 0.8);
            if !stitching {
                // Tall enough to meet the next rung, or a steep line comes out
                // as a ladder.
                let rise = travel / reach;
                let width = 1.0f32.min(run - along - step);
                within_coloured(ink, band, along + step, width, offset, stroke + rise, colour);
            }
            step += 1.0;
        }
        along += reach;
        up = !up;
    }
}

/// A wave along the band.
fn waveline(ink: &mut Ink<'_>, run: f32, band: f32) {
    let wavelength = (band * 2.2).max(4.0);
    let amplitude = (band - band * STROKE) / 2.0;
    let stroke = (band * STROKE).max(1.0);

    let mut along = 0.0;
    while along < run {
        let phase = along / wavelength * std::f32::consts::TAU;
        let width = 1.0f32.min(run - along);
        within(ink, band, along, width, phase.sin() * amplitude - stroke / 2.0, stroke);
        along += 1.0;
    }
}

/// The Greek wave: a square spiral, repeated.
///
/// A meander is rails and risers and nothing else, which is why it is the one
/// of Word's waves that can be drawn exactly rather than approximately.
fn meander(ink: &mut Ink<'_>, run: f32, band: f32) {
    let stroke = (band * STROKE).max(1.0);
    let repeat = (band * 1.6).max(4.0);
    let half = band / 2.0;

    let mut along = 0.0;
    while along < run {
        let left = |part: f32| along + repeat * part;
        let piece = |ink: &mut Ink<'_>, from: f32, to: f32, offset: f32, weight: f32| {
            let (from, to) = (from.min(run), to.min(run));
            if to > from {
                ink.piece(from, to - from, offset, weight);
            }
        };
        // The rail along the bottom, the riser up the near end, the rail back
        // along the top, and the two shorter ones that turn it into a spiral.
        piece(ink, along, along + repeat, half - stroke, stroke);
        piece(ink, along, along + stroke, -half, band);
        piece(ink, along, left(0.75), -half, stroke);
        piece(ink, left(0.75) - stroke, left(0.75), -half, half);
        piece(ink, left(0.35), left(0.75), -stroke / 2.0, stroke);
        piece(ink, left(0.35), left(0.35) + stroke, -stroke / 2.0, half);
        along += repeat;
    }
}

/// A row of crosses.
fn cross_stitch(ink: &mut Ink<'_>, run: f32, band: f32) {
    let size = band.max(2.0);
    let stroke = (band * STROKE).max(1.0);
    let top = -band / 2.0;
    let rise = (band - stroke) / size;

    let mut along = 0.0;
    while along < run {
        let mut step = 0.0;
        while step < size && along + step < run {
            let part = step / size;
            let width = 1.0f32.min(run - along - step);
            for offset in [top + (band - stroke) * part, top + (band - stroke) * (1.0 - part)] {
                within(ink, band, along + step, width, offset, stroke + rise);
            }
            step += 1.0;
        }
        along += size * 1.25;
    }
}

/// Slanted strokes across the band, leaning one way or the other.
fn hatching(ink: &mut Ink<'_>, run: f32, band: f32, leaning: bool) {
    let stroke = (band * STROKE).max(1.0);
    let step = (band * 0.7).max(2.0);
    let top = -band / 2.0;
    let travel = band - stroke;
    let rise = travel / band.max(1.0);

    let mut along = 0.0;
    while along < run {
        let mut across = 0.0;
        while across < band && along + across < run {
            let part = across / band.max(1.0);
            let offset = if leaning { top + travel * (1.0 - part) } else { top + travel * part };
            let width = 1.0f32.min(run - along - across);
            within(ink, band, along + across, width, offset, stroke + rise);
            across += 1.0;
        }
        along += step;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{Decoration, Page};

    /// Draws one pattern along a hundred pixels and gives back the rectangles.
    fn drawn(style: &str, band: f32) -> Vec<Decoration> {
        let mut page = Page::default();
        let mut ink = Ink { page: &mut page, x: 0.0, y: 0.0, sideways: true, colour: Color::BLACK };
        assert!(draw(&mut ink, style, 100.0, band), "{style} drew nothing");
        page.decorations
    }

    #[test]
    fn every_pattern_offered_is_drawn() {
        // The list a person picks from and the list this file knows are the
        // same list. One offering a pattern nothing drew would be a row that
        // does nothing.
        for (label, name) in wp_docx::art::DRAWN {
            let mut page = Page::default();
            let mut ink =
                Ink { page: &mut page, x: 0.0, y: 0.0, sideways: true, colour: Color::BLACK };
            assert!(draw(&mut ink, name, 100.0, 16.0), "{label} ({name}) is offered and not drawn");
            assert!(!page.decorations.is_empty(), "{label} drew nothing at all");
        }
    }

    #[test]
    fn one_of_words_pictures_is_left_alone() {
        let mut page = Page::default();
        let mut ink = Ink { page: &mut page, x: 0.0, y: 0.0, sideways: true, colour: Color::BLACK };
        assert!(!draw(&mut ink, "apples", 100.0, 16.0), "apples was drawn as something");
        assert!(page.decorations.is_empty(), "something was drawn for apples");
    }

    #[test]
    fn nothing_is_drawn_outside_the_band() {
        // A border twenty points wide that put ink twenty-five points from the
        // line would be a border creeping off the paper.
        let band = 16.0f32;
        for (_, name) in wp_docx::art::DRAWN {
            for mark in drawn(name, band) {
                assert!(
                    mark.y >= -band / 2.0 - 0.51 && mark.y + mark.height <= band / 2.0 + 0.51,
                    "{name} put a mark at {}..{} outside a band of {band}",
                    mark.y,
                    mark.y + mark.height
                );
            }
        }
    }

    #[test]
    fn nothing_is_drawn_past_the_end_of_the_run() {
        for (_, name) in wp_docx::art::DRAWN {
            for mark in drawn(name, 16.0) {
                assert!(mark.x >= -0.01, "{name} began at {}", mark.x);
                assert!(
                    mark.x + mark.width <= 100.51,
                    "{name} reached {} of a hundred",
                    mark.x + mark.width
                );
            }
        }
    }

    #[test]
    fn a_row_of_marks_repeats() {
        let dots = drawn("basicBlackDots", 8.0);
        assert!(dots.len() > 6, "a row of dots came out as {} marks", dots.len());
        assert!(dots.iter().all(|dot| (dot.width - dots[0].width).abs() < 0.01));
    }

    #[test]
    fn squares_are_bigger_than_dots() {
        let dots = drawn("basicBlackDots", 8.0);
        let squares = drawn("basicBlackSquares", 8.0);
        assert!(squares[0].height > dots[0].height, "the squares are no taller than the dots");
    }

    #[test]
    fn the_white_patterns_are_a_bar_with_nothing_in_the_holes() {
        // The first two rectangles are the rims of the bar, which run the whole
        // length; what is between them is drawn only where there is no hole.
        let bar = drawn("basicWhiteDots", 12.0);
        assert!((bar[0].width - 100.0).abs() < 0.01, "the top rim is not the whole run");
        assert!((bar[1].width - 100.0).abs() < 0.01, "the bottom rim is not the whole run");
        assert!(bar.len() > 4, "nothing was drawn between the holes");
    }

    #[test]
    fn a_triangle_rises_to_a_point_and_falls_again() {
        let marks = drawn("triangles", 12.0);
        let tallest = marks.iter().map(|mark| mark.height).fold(0.0f32, f32::max);
        assert!(tallest > 8.0, "the triangles are flat");
        // The first slice is the shortest, and one in the middle is the tallest.
        assert!(marks[0].height < tallest);
    }

    #[test]
    fn a_saw_leans_all_one_way() {
        let marks = drawn("sharksTeeth", 12.0);
        // Each tooth rises from nothing to its full height and starts again, so
        // the slices get taller and taller until the tooth ends.
        let rising = marks.windows(2).filter(|pair| pair[1].height > pair[0].height).count();
        let falling = marks.windows(2).filter(|pair| pair[1].height < pair[0].height).count();
        assert!(rising > falling * 3, "the teeth do not lean: {rising} up against {falling} down");
    }

    #[test]
    fn the_grey_patterns_are_the_ink_half_way_to_the_paper() {
        for name in ["sawtoothGray", "diamondsGray"] {
            let marks = drawn(name, 12.0);
            assert!(marks.iter().all(|mark| mark.color.alpha == GREY), "{name} is not grey");
        }
        // And the black ones are not.
        assert!(drawn("sawtooth", 12.0).iter().all(|mark| mark.color.alpha == 255));
    }

    #[test]
    fn a_gradient_fades() {
        let strips = drawn("gradient", 16.0);
        assert!(strips.len() > 3);
        assert!(
            strips[0].color.alpha > strips[strips.len() - 1].color.alpha,
            "the gradient does not fade"
        );
    }

    #[test]
    fn a_pattern_drawn_down_the_page_turns_with_it() {
        // The same pattern on a side edge: the band is now the width of each
        // mark and the run is its height, which is what turning it means.
        let band = 12.0f32;
        let mut page = Page::default();
        let mut ink =
            Ink { page: &mut page, x: 0.0, y: 0.0, sideways: false, colour: Color::BLACK };
        draw(&mut ink, "basicBlackSquares", 100.0, band);
        assert!(!page.decorations.is_empty());
        assert!(
            page.decorations.iter().all(|mark| mark.width <= band + 0.51),
            "a mark is wider than the band it is drawn in"
        );
        // And across the page, the band is the height instead.
        let mut page = Page::default();
        let mut ink = Ink { page: &mut page, x: 0.0, y: 0.0, sideways: true, colour: Color::BLACK };
        draw(&mut ink, "basicBlackSquares", 100.0, band);
        assert!(page.decorations.iter().all(|mark| mark.height <= band + 0.51));
    }

    #[test]
    fn a_band_of_nothing_draws_nothing() {
        let mut page = Page::default();
        let mut ink = Ink { page: &mut page, x: 0.0, y: 0.0, sideways: true, colour: Color::BLACK };
        assert!(!draw(&mut ink, "triangles", 0.0, 12.0));
        assert!(!draw(&mut ink, "triangles", 100.0, 0.0));
        assert!(page.decorations.is_empty());
    }
}
