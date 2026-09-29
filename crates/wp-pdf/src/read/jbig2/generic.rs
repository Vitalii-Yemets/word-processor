//! Generic regions and their refinements: a bitmap a pixel at a time.
//!
//! [T.88] 6.2 and 6.3. A generic region codes each pixel with the MQ coder
//! in a context of the pixels already decoded around it — one of four
//! templates of them, some of whose pixels may be moved — or is a fax
//! coding outright. "Typical prediction" lets a row say it is the same as
//! the one above and be skipped. A refinement codes a bitmap against
//! another it is like, the context taking in pixels of both.

use super::super::mq::{Context, Decoder};
use super::bitmap::Bitmap;

/// What a generic region's coding needs.
#[derive(Clone, Copy, Debug)]
pub struct Generic {
    pub template: u8,
    pub typical: bool,
    /// Where the moveable pixels are.
    pub at: [(i64, i64); 4],
}

impl Generic {
    /// The moveable pixels where they are unless moved.
    #[must_use]
    pub fn with_template(template: u8) -> Self {
        let at = match template {
            0 => [(3, -1), (-3, -1), (2, -2), (-2, -2)],
            1 => [(3, -1), (0, 0), (0, 0), (0, 0)],
            _ => [(2, -1), (0, 0), (0, 0), (0, 0)],
        };
        Self { template, typical: false, at }
    }
}

/// Contexts enough for a template.
#[must_use]
pub fn generic_contexts(template: u8) -> Vec<Context> {
    vec![Context::default(); 1 << generic_bits(template)]
}

fn generic_bits(template: u8) -> u32 {
    match template {
        0 => 16,
        1 => 13,
        _ => 10,
    }
}

/// A pixel's context, its bits in the standard's order.
fn generic_context(bitmap: &Bitmap, x: i64, y: i64, parameters: &Generic) -> usize {
    let p = |dx: i64, dy: i64| usize::from(bitmap.get(x + dx, y + dy));
    let a = |index: usize| {
        let (dx, dy) = parameters.at[index];
        p(dx, dy)
    };
    match parameters.template {
        0 => {
            p(-1, 0)
                | p(-2, 0) << 1
                | p(-3, 0) << 2
                | p(-4, 0) << 3
                | a(0) << 4
                | p(2, -1) << 5
                | p(1, -1) << 6
                | p(0, -1) << 7
                | p(-1, -1) << 8
                | p(-2, -1) << 9
                | a(1) << 10
                | a(2) << 11
                | p(1, -2) << 12
                | p(0, -2) << 13
                | p(-1, -2) << 14
                | a(3) << 15
        }
        1 => {
            p(-1, 0)
                | p(-2, 0) << 1
                | p(-3, 0) << 2
                | a(0) << 3
                | p(2, -1) << 4
                | p(1, -1) << 5
                | p(0, -1) << 6
                | p(-1, -1) << 7
                | p(-2, -1) << 8
                | p(2, -2) << 9
                | p(1, -2) << 10
                | p(0, -2) << 11
                | p(-1, -2) << 12
        }
        2 => {
            p(-1, 0)
                | p(-2, 0) << 1
                | a(0) << 2
                | p(1, -1) << 3
                | p(0, -1) << 4
                | p(-1, -1) << 5
                | p(-2, -1) << 6
                | p(1, -2) << 7
                | p(0, -2) << 8
                | p(-1, -2) << 9
        }
        _ => {
            p(-1, 0)
                | p(-2, 0) << 1
                | p(-3, 0) << 2
                | p(-4, 0) << 3
                | a(0) << 4
                | p(1, -1) << 5
                | p(0, -1) << 6
                | p(-1, -1) << 7
                | p(-2, -1) << 8
                | p(-3, -1) << 9
        }
    }
}

/// Decodes a generic region with the MQ coder.
pub fn decode_generic(
    decoder: &mut Decoder<'_>,
    contexts: &mut [Context],
    width: usize,
    height: usize,
    parameters: &Generic,
) -> Bitmap {
    decode_generic_with(decoder, contexts, width, height, parameters, None)
}

/// Decodes a generic region, leaving white and uncoded the pixels a skip
/// bitmap marks.
pub fn decode_generic_with(
    decoder: &mut Decoder<'_>,
    contexts: &mut [Context],
    width: usize,
    height: usize,
    parameters: &Generic,
    skip: Option<&Bitmap>,
) -> Bitmap {
    let mut bitmap = Bitmap::new(width, height);
    let typical_context = match parameters.template {
        0 => 0x9B25,
        1 => 0x0795,
        2 => 0x00E5,
        _ => 0x0195,
    };
    let mut same_as_above = false;
    for y in 0..height {
        if parameters.typical {
            same_as_above ^= decoder.bit(&mut contexts[typical_context]) == 1;
            if same_as_above {
                if y > 0 {
                    let (above, here) = bitmap.pixels.split_at_mut(y * width);
                    here[..width].copy_from_slice(&above[(y - 1) * width..]);
                }
                continue;
            }
        }
        for x in 0..width {
            if skip.is_some_and(|skip| skip.get(x as i64, y as i64) == 1) {
                continue;
            }
            let context = generic_context(&bitmap, x as i64, y as i64, parameters);
            let pixel = decoder.bit(&mut contexts[context]);
            bitmap.pixels[y * width + x] = pixel;
        }
    }
    bitmap
}

/// Decodes a generic region coded as a Group 4 fax, and says how many
/// bytes it took.
#[must_use]
pub fn decode_mmr(data: &[u8], width: usize, height: usize) -> Option<(Bitmap, usize)> {
    if width == 0 || height == 0 {
        return Some((Bitmap::new(width, height), 0));
    }
    let (rows, used) =
        wp_image::fax::decode_counting(data, width, height, wp_image::fax::Kind::Group4).ok()?;
    Some((Bitmap::from_packed(&rows, width, height), used))
}

/// What a refinement's coding needs.
#[derive(Clone, Copy, Debug)]
pub struct Refinement {
    pub template: u8,
    pub typical: bool,
    pub at: [(i64, i64); 2],
    /// Where the reference lies against the bitmap being decoded.
    pub dx: i64,
    pub dy: i64,
}

impl Refinement {
    #[must_use]
    pub fn with_template(template: u8) -> Self {
        Self { template, typical: false, at: [(-1, -1), (-1, -1)], dx: 0, dy: 0 }
    }
}

#[must_use]
pub fn refinement_contexts(template: u8) -> Vec<Context> {
    vec![Context::default(); if template == 0 { 1 << 13 } else { 1 << 10 }]
}

fn refinement_context(
    bitmap: &Bitmap,
    reference: &Bitmap,
    x: i64,
    y: i64,
    parameters: &Refinement,
) -> usize {
    let p = |dx: i64, dy: i64| usize::from(bitmap.get(x + dx, y + dy));
    let (rx, ry) = (x - parameters.dx, y - parameters.dy);
    let r = |dx: i64, dy: i64| usize::from(reference.get(rx + dx, ry + dy));
    if parameters.template == 0 {
        let [(a1x, a1y), (a2x, a2y)] = parameters.at;
        p(-1, 0)
            | p(1, -1) << 1
            | p(0, -1) << 2
            | p(a1x, a1y) << 3
            | r(1, 1) << 4
            | r(0, 1) << 5
            | r(-1, 1) << 6
            | r(1, 0) << 7
            | r(0, 0) << 8
            | r(-1, 0) << 9
            | r(1, -1) << 10
            | r(0, -1) << 11
            | r(a2x, a2y) << 12
    } else {
        p(-1, 0)
            | p(1, -1) << 1
            | p(0, -1) << 2
            | p(-1, -1) << 3
            | r(1, 1) << 4
            | r(0, 1) << 5
            | r(1, 0) << 6
            | r(0, 0) << 7
            | r(-1, 0) << 8
            | r(0, -1) << 9
    }
}

/// Decodes a refinement of `reference`.
pub fn decode_refinement(
    decoder: &mut Decoder<'_>,
    contexts: &mut [Context],
    width: usize,
    height: usize,
    reference: &Bitmap,
    parameters: &Refinement,
) -> Bitmap {
    let mut bitmap = Bitmap::new(width, height);
    let typical_context = if parameters.template == 0 { 0x0010 } else { 0x0008 };
    let mut typical = false;
    for y in 0..height {
        if parameters.typical {
            typical ^= decoder.bit(&mut contexts[typical_context]) == 1;
        }
        for x in 0..width {
            let (xi, yi) = (x as i64, y as i64);
            if typical {
                // Where the reference is all one colour around the pixel,
                // the pixel is that colour.
                let (rx, ry) = (xi - parameters.dx, yi - parameters.dy);
                let first = reference.get(rx - 1, ry - 1);
                let uniform =
                    (-1..=1).all(|dy| (-1..=1).all(|dx| reference.get(rx + dx, ry + dy) == first));
                if uniform {
                    bitmap.pixels[y * width + x] = first;
                    continue;
                }
            }
            let context = refinement_context(&bitmap, reference, xi, yi, parameters);
            bitmap.pixels[y * width + x] = decoder.bit(&mut contexts[context]);
        }
    }
    bitmap
}
