//! Text drawn turned: up the page, down it, at a slant, or in columns as
//! vertical writing is.
//!
//! Read across the page, a line going up it is a letter on each line of
//! the text beside it. So the glyphs that run another way are gathered by
//! the way they run, turned back to run across, and read as lines and
//! paragraphs of their own, after the page's text.

use super::super::content::Glyph;
use super::{blocks_of, decorate, lines_of, paragraphs_of, Built};

pub(super) fn turned_paragraphs(glyphs: Vec<Glyph>, body_size: f64) -> Vec<Built> {
    let mut groups: Vec<(f64, Vec<Glyph>)> = Vec::new();
    for glyph in glyphs {
        match groups.iter_mut().find(|(angle, _)| (angle - glyph.angle).abs() < 0.05) {
            Some((_, group)) => group.push(glyph),
            None => groups.push((glyph.angle, vec![glyph])),
        }
    }
    let mut out = Vec::new();
    for (angle, group) in groups {
        let (sin, cos) = (-angle).sin_cos();
        let turn = |x: f64, y: f64| (x * cos - y * sin, x * sin + y * cos);
        let turned: Vec<Glyph> = group
            .into_iter()
            .map(|mut glyph| {
                let (x, y) = turn(glyph.x, glyph.y);
                let (end_x, end_y) = turn(glyph.end_x, glyph.end_y);
                glyph.x = x;
                glyph.y = y;
                glyph.end_x = end_x;
                glyph.end_y = end_y;
                glyph.angle = 0.0;
                glyph
            })
            .collect();
        let pieces = decorate(&turned, &[], &[], 0);
        for block in blocks_of(lines_of(pieces, &mut [], &mut Vec::new())) {
            let left = block.iter().map(|l| l.x0).fold(f64::INFINITY, f64::min);
            let right = block.iter().map(|l| l.x1).fold(f64::NEG_INFINITY, f64::max);
            // Never carried on from or onto the next page's text: they are
            // put after the page's own, not where they were.
            out.extend(
                paragraphs_of(block, left, right, body_size, (left + right) / 2.0).into_iter().map(
                    |mut built| {
                        built.ends_full = false;
                        built
                    },
                ),
            );
        }
    }
    out
}
