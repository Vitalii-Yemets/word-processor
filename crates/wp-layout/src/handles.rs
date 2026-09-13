//! The yellow handles: where a shape's adjustments can be taken hold of.
//!
//! # One description, both ways round
//!
//! A handle has to answer two questions — where it sits for the value the
//! document gives, and what value it means when somebody drags it somewhere
//! else. Two answers written separately are two that can disagree, and a handle
//! that jumps out from under the pointer as it is taken hold of is exactly that
//! disagreement. So each handle says the *line it slides along* and what the
//! far end of that line is worth, and both answers follow from it.
//!
//! # What the handles are worth
//!
//! The same value the geometry reads, in the format's own unit. A handle whose
//! value when the document says nothing is not the value the shape is drawn at
//! would move the shape the moment it was touched, which is why there is a test
//! that writes each handle's own fallback into a document and checks the shape
//! comes out unchanged.

use wp_raster::Point;

use crate::geometry::{fallback, Adjusts, Preset};

/// One of a shape's handles.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Handle {
    /// The name the format gives this adjustment: `adj` for a shape with one
    /// handle and `adj1`, `adj2` and so on for a shape with several.
    pub name: &'static str,
    /// Where it slides.
    pub slide: Slide,
    /// What it is worth now: the document's value, or the one the format falls
    /// back on.
    pub value: i32,
}

/// The way a handle moves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Slide {
    /// Along a straight line. `zero` is where the handle sits when the value is
    /// nothing, and `full` where it sits when the value is `scale`.
    Line { zero: Point, full: Point, scale: i32 },
    /// Round a middle, where the value is the angle itself in sixtieths of a
    /// degree, which is the unit the format states an angle in.
    Turn { middle: Point, radii: (f32, f32) },
}

impl Handle {
    /// Where it sits.
    #[must_use]
    pub fn at(self) -> Point {
        match self.slide {
            Slide::Line { zero, full, scale } => {
                let share = self.value as f32 / scale.max(1) as f32;
                Point::new(zero.x + (full.x - zero.x) * share, zero.y + (full.y - zero.y) * share)
            }
            Slide::Turn { middle, radii } => {
                let angle = (self.value as f32 / 60_000.0).to_radians();
                Point::new(middle.x + radii.0 * angle.cos(), middle.y + radii.1 * angle.sin())
            }
        }
    }

    /// What it would be worth if it were dragged to a place.
    ///
    /// A handle slides along its own line, so a drag across it counts for
    /// nothing and a drag past the end of it counts as the end: what comes back
    /// is how far along the line the pointer has reached.
    #[must_use]
    pub fn value_at(self, to: Point) -> i32 {
        match self.slide {
            Slide::Line { zero, full, scale } => {
                let (dx, dy) = (full.x - zero.x, full.y - zero.y);
                let length = dx * dx + dy * dy;
                if length <= 0.0 {
                    return self.value;
                }
                let along = ((to.x - zero.x) * dx + (to.y - zero.y) * dy) / length;
                (along * scale as f32).round() as i32
            }
            Slide::Turn { middle, radii } => {
                let (dx, dy) = (
                    (to.x - middle.x) / radii.0.max(0.001),
                    (to.y - middle.y) / radii.1.max(0.001),
                );
                let mut degrees = dy.atan2(dx).to_degrees();
                if degrees < 0.0 {
                    degrees += 360.0;
                }
                (degrees * 60_000.0).round() as i32
            }
        }
    }
}

/// Every handle a shape has, in the box it is drawn in.
///
/// Nothing for a shape with no adjustment, which is most of them: a rectangle
/// is a rectangle however it is dragged.
#[must_use]
pub fn handles_in(
    preset: Preset,
    adjusts: &Adjusts,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
) -> Vec<Handle> {
    let point = Point::new;
    let shorter = width.min(height);
    let (mx, my) = (x + width / 2.0, y + height / 2.0);
    let (right, bottom) = (x + width, y + height);
    // A handle that slides along a line, with what it is worth now.
    let line = |name: &'static str, zero: Point, full: Point, scale: i32, fallback: i32| Handle {
        name,
        slide: Slide::Line { zero, full, scale },
        value: value_of(adjusts, name, fallback),
    };
    let turn = |name: &'static str, fallback: i32| Handle {
        name,
        slide: Slide::Turn { middle: point(mx, my), radii: (width / 2.0, height / 2.0) },
        value: value_of(adjusts, name, fallback),
    };

    match preset {
        // The corner of a rectangle, taken round or cut off: the handle slides
        // along the top edge from the corner towards the middle, and half the
        // shorter side is as far as a corner can reach.
        Preset::RoundedRectangle
        | Preset::SnipOneCorner
        | Preset::SnipTwoSame
        | Preset::SnipTwoDiagonal
        | Preset::SnipAndRound
        | Preset::RoundOneCorner
        | Preset::RoundTwoSame
        | Preset::RoundTwoDiagonal => {
            vec![line("adj", point(x, y), point(x + shorter / 2.0, y), 50_000, fallback::CORNER)]
        }
        // A star's dip, along the line from the middle out to its topmost
        // point: the format states it over half the radius, so the point
        // itself is worth 50,000.
        Preset::Star
        | Preset::Star4
        | Preset::Star6
        | Preset::Star7
        | Preset::Star8
        | Preset::Star10
        | Preset::Star12
        | Preset::Star16
        | Preset::Star24
        | Preset::Star32 => {
            let dip = crate::banners::dip_of(preset).unwrap_or(0.5);
            vec![line("adj", point(mx, my), point(mx, y), 50_000, (dip * 50_000.0).round() as i32)]
        }
        // An arrow: how thick the shaft is, and how long the head.
        Preset::Arrow | Preset::LeftArrow | Preset::LeftRightArrow => {
            let head_from = if preset == Preset::LeftArrow { x } else { right };
            let head_to = if preset == Preset::LeftArrow { x + shorter } else { right - shorter };
            vec![
                line("adj1", point(x, my), point(x, my - shorter / 2.0), 100_000, fallback::HALF),
                line("adj2", point(head_from, my), point(head_to, my), 100_000, fallback::HALF),
            ]
        }
        Preset::UpArrow | Preset::DownArrow | Preset::UpDownArrow => {
            let head_from = if preset == Preset::UpArrow { y } else { bottom };
            let head_to = if preset == Preset::UpArrow { y + shorter } else { bottom - shorter };
            vec![
                line("adj1", point(mx, y), point(mx - shorter / 2.0, y), 100_000, fallback::HALF),
                line("adj2", point(mx, head_from), point(mx, head_to), 100_000, fallback::HALF),
            ]
        }
        // The shapes made of arcs, whose handles are the angles themselves.
        Preset::Pie | Preset::Chord => {
            vec![turn("adj1", 0), turn("adj2", 16_200_000)]
        }
        Preset::Arc => vec![turn("adj1", 16_200_000), turn("adj2", 0)],
        // And the ones whose handle is a thickness or an arm, each sliding in
        // from the edge it is measured from.
        Preset::Can => {
            vec![line("adj", point(mx, y), point(mx, y + shorter / 2.0), 100_000, fallback::RING)]
        }
        Preset::Donut => {
            vec![line("adj", point(x, my), point(x + shorter, my), 100_000, fallback::RING)]
        }
        // The "no" symbol is thinner in the rim than a donut, because the bar
        // across it is the same thickness and a thick one would fill it.
        Preset::NoSymbol => {
            vec![line("adj", point(x, my), point(x + shorter, my), 100_000, fallback::BAR)]
        }
        Preset::Frame => {
            vec![line("adj", point(x, y), point(x + shorter, y), 100_000, fallback::FRAME)]
        }
        Preset::Cross | Preset::LShape | Preset::HalfFrame => {
            vec![line("adj", point(x, y), point(x + shorter, y), 100_000, fallback::ARM)]
        }
        Preset::Plaque => {
            vec![line("adj", point(x, y), point(x + shorter, y), 100_000, fallback::CORNER)]
        }
        Preset::BlockArc => {
            vec![
                turn("adj1", 10_800_000),
                turn("adj2", 21_600_000),
                line("adj3", point(x, my), point(x + shorter, my), 100_000, fallback::RING),
            ]
        }
        _ => Vec::new(),
    }
}

/// What a handle is worth: the document's value, or the fallback.
///
/// A shape with one handle is written `adj` by some programs and `adj1` by
/// others, and [`Adjusts`] already knows they are the same one.
fn value_of(adjusts: &Adjusts, name: &str, fallback: i32) -> i32 {
    let index = match name {
        "adj" | "adj1" => 1,
        other => other.strip_prefix("adj").and_then(|digits| digits.parse().ok()).unwrap_or(1),
    };
    adjusts.value(index).unwrap_or(fallback)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_handle_sits_where_its_value_says() {
        // The corner of a rounded rectangle, at the format's own sixth: a
        // sixth of the shorter side along the top edge from the corner.
        let handles = handles_in(Preset::RoundedRectangle, &Adjusts::NONE, 0.0, 0.0, 120.0, 60.0);
        let at = handles[0].at();
        assert!((at.x - 10.0).abs() < 0.1, "it sits at {at:?}");
        assert!(at.y.abs() < 0.1, "and on the top edge");
    }

    #[test]
    fn dragging_a_handle_says_what_it_is_worth() {
        let handles = handles_in(Preset::RoundedRectangle, &Adjusts::NONE, 0.0, 0.0, 120.0, 60.0);
        // Half way along the line it slides on, which is half of what the far
        // end is worth.
        assert_eq!(handles[0].value_at(Point::new(15.0, 0.0)), 25_000);
        // And across it counts for nothing: a handle slides along its own line.
        assert_eq!(handles[0].value_at(Point::new(15.0, 40.0)), 25_000);
    }

    #[test]
    fn a_handle_dragged_past_the_end_of_its_line_says_so() {
        // What it says is the caller's to hold to what the shape can do: a
        // corner further than half the box is a corner that crosses the one
        // opposite, and the geometry holds it back.
        let handles = handles_in(Preset::RoundedRectangle, &Adjusts::NONE, 0.0, 0.0, 120.0, 60.0);
        assert!(handles[0].value_at(Point::new(90.0, 0.0)) > 50_000);
    }

    #[test]
    fn every_handle_is_worth_what_the_shape_is_drawn_at() {
        // A handle whose value when the document says nothing is not the value
        // the shape is drawn at would move the shape the moment it was touched.
        // So: writing each handle's own value into a document draws the same
        // shape as writing nothing at all.
        for preset in Preset::all() {
            let handles = handles_in(preset, &Adjusts::NONE, 0.0, 0.0, 120.0, 60.0);
            if handles.is_empty() {
                continue;
            }
            let pairs: Vec<(String, i32)> =
                handles.iter().map(|handle| (handle.name.to_owned(), handle.value)).collect();
            let said = Adjusts::from_pairs(&pairs);
            let plain = crate::geometry::path_in(preset, &Adjusts::NONE, 0.0, 0.0, 120.0, 60.0);
            let told = crate::geometry::path_in(preset, &said, 0.0, 0.0, 120.0, 60.0);
            assert_eq!(plain, told, "{} is drawn differently by its own handles", preset.label());
        }
    }
}
