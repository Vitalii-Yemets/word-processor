//! Pattern dictionaries and halftone regions.
//!
//! [T.88] 6.6 and 6.7. A halftone is a grid of cells, each one of a few
//! patterns — a pattern dictionary holds them, side by side in one
//! generic region — and a halftone region codes which pattern each cell
//! is as a grey-scale picture, a bit-plane at a time, each plane a generic
//! region and each after the first the difference from the one above it.

use super::super::mq::Decoder;
use super::bitmap::{Bitmap, Combine};
use super::generic::{self, Generic};

/// The patterns of a pattern dictionary segment's data.
#[must_use]
pub fn decode_patterns(data: &[u8]) -> Option<Vec<Bitmap>> {
    let flags = *data.first()?;
    let mmr = flags & 1 != 0;
    let template = (flags >> 1) & 3;
    let width = usize::from(*data.get(1)?);
    let height = usize::from(*data.get(2)?);
    let most = u32::from_be_bytes(data.get(3..7)?.try_into().ok()?) as usize;
    let count = most.checked_add(1)?;
    let total_width = count.checked_mul(width)?;
    if width == 0 || height == 0 || total_width * height > 1 << 28 {
        return None;
    }
    let coded = &data[7..];
    let collective = if mmr {
        generic::decode_mmr(coded, total_width, height)?.0
    } else {
        let mut parameters = Generic::with_template(template);
        parameters.at[0] = (-(width as i64), 0);
        let mut contexts = generic::generic_contexts(template);
        let mut decoder = Decoder::new(coded, 0, coded.len());
        generic::decode_generic(&mut decoder, &mut contexts, total_width, height, &parameters)
    };
    Some(
        (0..count).map(|index| collective.part((index * width) as i64, 0, width, height)).collect(),
    )
}

/// A halftone region's bitmap, from its data after the region's own
/// information, drawn with `patterns`.
#[must_use]
pub fn decode_halftone(
    data: &[u8],
    width: usize,
    height: usize,
    patterns: &[Bitmap],
) -> Option<Bitmap> {
    let flags = *data.first()?;
    let mmr = flags & 1 != 0;
    let template = (flags >> 1) & 3;
    let skipping = flags & 8 != 0;
    let combine = Combine::of((flags >> 4) & 7);
    let default_pixel = (flags >> 7) & 1;
    let word = |at: usize| Some(u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?));
    let grid_width = word(1)? as usize;
    let grid_height = word(5)? as usize;
    let grid_x = i64::from(word(9)? as i32);
    let grid_y = i64::from(word(13)? as i32);
    let step_x = i64::from(u16::from_be_bytes(data.get(17..19)?.try_into().ok()?));
    let step_y = i64::from(u16::from_be_bytes(data.get(19..21)?.try_into().ok()?));
    let coded = &data[21..];
    let pattern = patterns.first()?;
    let (pattern_width, pattern_height) = (pattern.width as i64, pattern.height as i64);
    if grid_width * grid_height > 1 << 24 || width * height > 1 << 28 {
        return None;
    }
    let mut region = Bitmap::filled(width, height, default_pixel);
    // The cells wholly off the region, which the planes then skip.
    let skip = skipping.then(|| {
        let mut skip = Bitmap::new(grid_width, grid_height);
        for m in 0..grid_height as i64 {
            for n in 0..grid_width as i64 {
                let x = (grid_x + m * step_y + n * step_x) >> 8;
                let y = (grid_y + m * step_x - n * step_y) >> 8;
                if x + pattern_width <= 0
                    || x >= width as i64
                    || y + pattern_height <= 0
                    || y >= height as i64
                {
                    skip.set(n as usize, m as usize, 1);
                }
            }
        }
        skip
    });
    let planes_count = super::integers::bits_for(patterns.len());
    // The planes, most significant first, each after the first the
    // difference from the one before it.
    let mut planes: Vec<Bitmap> = Vec::with_capacity(planes_count as usize);
    let mut contexts = generic::generic_contexts(template);
    let mut decoder = Decoder::new(coded, 0, coded.len());
    let mut used = 0usize;
    let mut parameters = Generic::with_template(template);
    parameters.at = [(if template <= 1 { 3 } else { 2 }, -1), (-3, -1), (2, -2), (-2, -2)];
    for _ in 0..planes_count {
        let mut plane = if mmr {
            let (plane, consumed) =
                generic::decode_mmr(coded.get(used..).unwrap_or(&[]), grid_width, grid_height)?;
            used += consumed;
            plane
        } else {
            generic::decode_generic_with(
                &mut decoder,
                &mut contexts,
                grid_width,
                grid_height,
                &parameters,
                skip.as_ref(),
            )
        };
        if let Some(above) = planes.last() {
            for (pixel, &previous) in plane.pixels.iter_mut().zip(&above.pixels) {
                *pixel ^= previous;
            }
        }
        planes.push(plane);
    }
    for m in 0..grid_height {
        for n in 0..grid_width {
            let mut value = 0usize;
            for plane in &planes {
                value = (value << 1) | usize::from(plane.pixels[m * grid_width + n]);
            }
            let Some(pattern) = patterns.get(value.min(patterns.len() - 1)) else { continue };
            let (m, n) = (m as i64, n as i64);
            let x = (grid_x + m * step_y + n * step_x) >> 8;
            let y = (grid_y + m * step_x - n * step_y) >> 8;
            region.compose(pattern, x, y, combine);
        }
    }
    Some(region)
}
