//! Laying an equation out: where every letter and every rule goes.
//!
//! # Why this is separate from the rest of the layout
//!
//! Because it is a different problem. A paragraph is a line of things one after
//! another, broken where they will not fit. An equation is a tree of boxes
//! nested inside one another — a numerator over a denominator, a power beside
//! what it is a power of — and it is never broken. So it is measured whole and
//! then put on the line as one item, the same way a picture is.
//!
//! # The one measurement everything hangs off
//!
//! The *axis*: the height a fraction bar sits at, and the height a minus sign
//! is drawn at. Typography puts it about a quarter of the way up the em, and
//! everything here is placed relative to it — which is why a fraction inside a
//! fraction still lines up with the text beside it.

use wp_docx::math::Math;
use wp_raster::Color;

use crate::layout::{Decoration, PositionedGlyph};

/// Where the fraction bar sits, as a share of the size.
const AXIS: f32 = 0.25;
/// How thick a rule is, as a share of the size.
const RULE: f32 = 0.05;
/// The room left round a rule, as a share of the size.
const GAP: f32 = 0.14;
/// How much smaller a power or an index is set.
const SMALLER: f32 = 0.72;
/// How far a power is lifted and an index dropped, as shares of the size.
const RAISE: f32 = 0.45;
/// And how far an index is dropped.
const DROP: f32 = 0.18;
/// The room left either side of a bracket or a root sign.
const PAD: f32 = 0.08;

/// An equation measured and laid out, ready to be put on a line.
///
/// Everything inside is placed against a baseline at zero, with the same
/// screen coordinates the rest of the layout uses: `x` to the right, `y` down.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MathBox {
    pub width: f32,
    /// How far it reaches above the baseline.
    pub ascent: f32,
    /// And below it.
    pub descent: f32,
    pub glyphs: Vec<PositionedGlyph>,
    /// The rules: fraction bars and the stroke over a root.
    pub rules: Vec<Decoration>,
}

impl MathBox {
    /// Moves everything in the box by an offset.
    fn shifted(mut self, dx: f32, dy: f32) -> Self {
        for glyph in &mut self.glyphs {
            glyph.x += dx;
            glyph.baseline += dy;
        }
        for rule in &mut self.rules {
            rule.x += dx;
            rule.y += dy;
        }
        // The extents move with it: what was `ascent` above the old baseline is
        // `ascent - dy` above the new one.
        self.ascent -= dy;
        self.descent += dy;
        self
    }

    /// Puts another box's contents into this one, already positioned.
    fn absorb(&mut self, other: Self) {
        self.glyphs.extend(other.glyphs);
        self.rules.extend(other.rules);
        self.ascent = self.ascent.max(other.ascent);
        self.descent = self.descent.max(other.descent);
    }

    /// The box moved onto a line: its glyphs and rules in page coordinates.
    #[must_use]
    pub fn placed(&self, x: f32, baseline: f32) -> (Vec<PositionedGlyph>, Vec<Decoration>) {
        let glyphs = self
            .glyphs
            .iter()
            .map(|glyph| PositionedGlyph {
                x: glyph.x + x,
                baseline: glyph.baseline + baseline,
                ..*glyph
            })
            .collect();
        let rules = self
            .rules
            .iter()
            .map(|rule| Decoration { x: rule.x + x, y: rule.y + baseline, ..*rule })
            .collect();
        (glyphs, rules)
    }
}

/// What one piece of an equation needs from the engine to be laid out.
///
/// The engine owns the fonts, so it shapes the text; everything else about
/// where a piece goes is worked out here.
pub trait MathShaper {
    /// Turns text into placed glyphs on a baseline at zero, and says how far
    /// the pen moved, how far the letters reach up, and how far down.
    fn shape_math(
        &mut self,
        text: &str,
        size: f32,
        color: Color,
    ) -> (Vec<PositionedGlyph>, f32, f32, f32);
}

/// Lays an equation out at a size.
#[must_use]
pub fn layout(shaper: &mut dyn MathShaper, math: &Math, size: f32, color: Color) -> MathBox {
    match math {
        Math::Text(text) => {
            let (glyphs, width, ascent, descent) = shaper.shape_math(text, size, color);
            MathBox { width, ascent, descent, glyphs, rules: Vec::new() }
        }
        Math::Row(pieces) => {
            let mut out = MathBox::default();
            for piece in pieces {
                let laid = layout(shaper, piece, size, color);
                let width = laid.width;
                out.absorb(laid.shifted(out.width, 0.0));
                out.width += width;
            }
            out
        }
        Math::Fraction(top, bottom) => fraction(shaper, top, bottom, size, color),
        Math::Superscript(base, power) => script(shaper, base, power, size, color, true),
        Math::Subscript(base, index) => script(shaper, base, index, size, color, false),
        Math::Radical(inside) => radical(shaper, inside, size, color),
        Math::Delimited(inside) => delimited(shaper, inside, size, color),
    }
}

/// A numerator over a denominator, with the bar between them.
fn fraction(
    shaper: &mut dyn MathShaper,
    top: &Math,
    bottom: &Math,
    size: f32,
    color: Color,
) -> MathBox {
    let above = layout(shaper, top, size, color);
    let below = layout(shaper, bottom, size, color);

    let pad = size * PAD;
    let width = above.width.max(below.width) + pad * 2.0;
    let thickness = (size * RULE).max(1.0);
    let gap = size * GAP;
    // The bar sits at the axis, above the baseline — so upwards is negative.
    let bar_y = -size * AXIS;

    // The numerator's foot rests a gap above the bar; the denominator's head
    // hangs a gap below it.
    let shift_top = bar_y - gap - above.descent;
    let shift_bottom = bar_y + thickness + gap + below.ascent;

    let mut out = MathBox {
        width,
        ascent: 0.0,
        descent: 0.0,
        glyphs: Vec::new(),
        rules: vec![Decoration { x: 0.0, y: bar_y, width, height: thickness, color }],
    };
    out.absorb(above.clone().shifted((width - above.width) / 2.0, shift_top));
    out.absorb(below.clone().shifted((width - below.width) / 2.0, shift_bottom));
    // The bar itself is part of what the box covers.
    out.ascent = out.ascent.max(-bar_y);
    out.descent = out.descent.max(bar_y + thickness);
    out
}

/// A power above, or an index below.
fn script(
    shaper: &mut dyn MathShaper,
    base: &Math,
    script: &Math,
    size: f32,
    color: Color,
    above: bool,
) -> MathBox {
    let body = layout(shaper, base, size, color);
    let small = layout(shaper, script, size * SMALLER, color);

    let shift = if above { -size * RAISE } else { size * DROP };
    let width = body.width + small.width;

    let mut out = MathBox { width, ..MathBox::default() };
    let body_width = body.width;
    out.absorb(body);
    out.absorb(small.shifted(body_width, shift));
    out
}

/// A square root: the sign, and the rule over what is under it.
fn radical(shaper: &mut dyn MathShaper, inside: &Math, size: f32, color: Color) -> MathBox {
    let body = layout(shaper, inside, size, color);
    let pad = size * PAD;
    let thickness = (size * RULE).max(1.0);
    let gap = size * GAP * 0.6;

    // The sign is the character the font has for it, set tall enough to reach
    // over what is under the root.
    let sign_size = size * ((body.ascent + body.descent) / size).clamp(1.0, 2.5);
    let (sign_glyphs, sign_width, sign_ascent, sign_descent) =
        shaper.shape_math("√", sign_size, color);

    let mut out = MathBox {
        width: sign_width + pad + body.width + pad,
        ascent: sign_ascent,
        descent: sign_descent,
        glyphs: sign_glyphs,
        rules: Vec::new(),
    };

    let body_left = sign_width + pad;
    let body_width = body.width;
    let body_ascent = body.ascent;
    out.absorb(body.shifted(body_left, 0.0));

    // The rule runs from the top of the sign across the top of what is inside.
    let rule_y = -(body_ascent + gap + thickness);
    out.rules.push(Decoration {
        x: body_left - pad / 2.0,
        y: rule_y,
        width: body_width + pad,
        height: thickness,
        color,
    });
    out.ascent = out.ascent.max(-rule_y);
    out
}

/// Something in brackets.
fn delimited(shaper: &mut dyn MathShaper, inside: &Math, size: f32, color: Color) -> MathBox {
    let body = layout(shaper, inside, size, color);
    // Brackets grow with what is between them, up to a point: a bracket three
    // times the height of the text is a bracket that has stopped helping.
    let tall = ((body.ascent + body.descent) / size).clamp(1.0, 2.5);
    let bracket_size = size * tall;

    let (open, open_width, open_ascent, open_descent) = shaper.shape_math("(", bracket_size, color);
    let (close, close_width, close_ascent, close_descent) =
        shaper.shape_math(")", bracket_size, color);

    let mut out = MathBox {
        width: open_width,
        ascent: open_ascent.max(close_ascent),
        descent: open_descent.max(close_descent),
        glyphs: open,
        rules: Vec::new(),
    };

    let body_width = body.width;
    out.absorb(body.shifted(open_width, 0.0));
    out.width += body_width;

    let closing = MathBox {
        width: close_width,
        ascent: close_ascent,
        descent: close_descent,
        glyphs: close,
        rules: Vec::new(),
    };
    out.absorb(closing.shifted(out.width, 0.0));
    out.width += close_width;
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_docx::math::parse;
    use wp_raster::Color;

    /// A shaper that knows nothing about fonts: every character is a square of
    /// the size, sitting on the baseline. Enough to check the arithmetic, which
    /// is what this module is.
    struct Squares;

    impl MathShaper for Squares {
        fn shape_math(
            &mut self,
            text: &str,
            size: f32,
            color: Color,
        ) -> (Vec<PositionedGlyph>, f32, f32, f32) {
            let mut glyphs = Vec::new();
            let mut x = 0.0;
            for _ in text.chars() {
                glyphs.push(PositionedGlyph {
                    face: 0,
                    glyph: wp_font::GlyphId(0),
                    x,
                    baseline: 0.0,
                    advance: size,
                    size,
                    stretch: 1.0,
                    color,
                    effect: None,
                    source: wp_docx::TextPosition::new(0, 0),
                    source_length: 0,
                    invisible: false,
                });
                x += size;
            }
            (glyphs, x, size * 0.7, size * 0.2)
        }
    }

    fn laid(typed: &str) -> MathBox {
        layout(&mut Squares, &parse(typed), 20.0, Color::BLACK)
    }

    #[test]
    fn a_letter_is_one_square_wide() {
        let box_ = laid("x");
        assert!((box_.width - 20.0).abs() < 0.01, "got {}", box_.width);
        assert_eq!(box_.glyphs.len(), 1);
    }

    #[test]
    fn a_row_is_as_wide_as_its_pieces_together() {
        let box_ = laid("abc");
        assert!((box_.width - 60.0).abs() < 0.01, "got {}", box_.width);
    }

    #[test]
    fn a_fraction_has_a_bar() {
        let box_ = laid("a/b");
        assert_eq!(box_.rules.len(), 1, "{:?}", box_.rules);
        // Above the baseline, which is where the axis is.
        assert!(box_.rules[0].y < 0.0, "the bar is at {}", box_.rules[0].y);
    }

    #[test]
    fn a_fraction_is_taller_than_the_text_beside_it() {
        let plain = laid("a");
        let fraction = laid("a/b");
        assert!(fraction.ascent > plain.ascent, "it does not reach higher");
        assert!(fraction.descent > plain.descent, "it does not reach lower");
    }

    #[test]
    fn the_numerator_sits_above_the_denominator() {
        let box_ = laid("a/b");
        let baselines: Vec<f32> = box_.glyphs.iter().map(|glyph| glyph.baseline).collect();
        assert_eq!(baselines.len(), 2);
        assert!(baselines[0] < baselines[1], "{baselines:?}");
    }

    #[test]
    fn a_power_is_smaller_and_higher_than_what_it_is_a_power_of() {
        let box_ = laid("x^2");
        assert_eq!(box_.glyphs.len(), 2);
        assert!(box_.glyphs[1].size < box_.glyphs[0].size, "the power is not smaller");
        assert!(box_.glyphs[1].baseline < box_.glyphs[0].baseline, "the power is not higher");
    }

    #[test]
    fn an_index_is_smaller_and_lower() {
        let box_ = laid("x_1");
        assert!(box_.glyphs[1].size < box_.glyphs[0].size);
        assert!(box_.glyphs[1].baseline > box_.glyphs[0].baseline, "the index is not lower");
    }

    #[test]
    fn a_root_has_a_sign_and_a_rule_over_what_is_under_it() {
        let box_ = laid("sqrt(x)");
        assert!(!box_.rules.is_empty(), "no rule over the root");
        // The sign and the letter under it.
        assert_eq!(box_.glyphs.len(), 2);
        assert!(box_.glyphs[1].x > box_.glyphs[0].x, "the letter is not after the sign");
    }

    #[test]
    fn brackets_go_round_what_is_inside_them() {
        let box_ = laid("(x)");
        assert_eq!(box_.glyphs.len(), 3);
        let x: Vec<f32> = box_.glyphs.iter().map(|glyph| glyph.x).collect();
        assert!(x[0] < x[1] && x[1] < x[2], "{x:?}");
    }

    #[test]
    fn brackets_grow_with_what_is_between_them() {
        let plain = laid("(x)");
        let tall = laid("((a)/(b))");
        assert!(tall.glyphs[0].size > plain.glyphs[0].size, "the bracket did not grow");
    }

    #[test]
    fn a_box_moved_takes_everything_in_it_along() {
        let moved = laid("a/b").shifted(10.0, 5.0);
        assert!(moved.glyphs.iter().all(|glyph| glyph.x >= 10.0), "a glyph was left behind");
        assert!(moved.rules.iter().all(|rule| rule.x >= 10.0), "a rule was left behind");
    }

    #[test]
    fn placing_a_box_puts_it_where_it_was_asked_for() {
        let box_ = laid("a/b");
        let (glyphs, rules) = box_.placed(100.0, 50.0);
        assert!(glyphs.iter().all(|glyph| glyph.x >= 100.0));
        assert!(rules.iter().all(|rule| rule.y > 0.0 && rule.y < 50.0));
    }

    #[test]
    fn nothing_lays_out_as_nothing() {
        let box_ = laid("");
        assert_eq!(box_.width, 0.0);
        assert!(box_.glyphs.is_empty());
    }
}
