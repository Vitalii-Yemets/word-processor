//! A picture out of an image object.
//!
//! A JPEG is carried as itself and comes out as itself. Anything else is
//! rows of samples in a colour space — grey, RGB, CMYK, or an index into
//! a palette — with a mask of its own for what is see-through, and comes
//! out as a PNG made from them. JPEG 2000, fax and JBIG2 pictures are not
//! read, and named in the roadmap.

use super::file::File;
use super::object::{Object, Stream};

/// The picture's bytes and their kind, if the picture can be read.
pub fn picture_of(
    file: &File<'_>,
    stream: &Stream,
    fill: [f32; 3],
) -> Option<(Vec<u8>, &'static str)> {
    let (data, filter) = file.decode(stream);
    match filter.as_deref() {
        Some("DCTDecode" | "DCT") => return Some((data, "jpeg")),
        Some(_) => return None,
        None => {}
    }
    let dictionary = &stream.dictionary;
    let width = file
        .get(dictionary, "Width")
        .as_integer()
        .or_else(|| file.get(dictionary, "W").as_integer())?;
    let height = file
        .get(dictionary, "Height")
        .as_integer()
        .or_else(|| file.get(dictionary, "H").as_integer())?;
    let (width, height) = (usize::try_from(width).ok()?, usize::try_from(height).ok()?);
    if width == 0 || height == 0 || width * height > 64 * 1024 * 1024 {
        return None;
    }
    let is_mask = matches!(file.get(dictionary, "ImageMask"), Object::Bool(true))
        || matches!(file.get(dictionary, "IM"), Object::Bool(true));
    let bits = if is_mask {
        1
    } else {
        file.get(dictionary, "BitsPerComponent")
            .as_integer()
            .or_else(|| file.get(dictionary, "BPC").as_integer())
            .unwrap_or(8) as usize
    };
    let decode_inverts = file
        .get(dictionary, "Decode")
        .as_array()
        .and_then(|d| d.first())
        .and_then(Object::as_number)
        .is_some_and(|first| first == 1.0);

    let mut pixels = vec![0u8; width * height * 4];
    if is_mask {
        // A stencil: one bit a pixel, painted in the fill colour where the
        // bit is clear (or set, when the decode array says so).
        let row_bytes = width.div_ceil(8);
        let colour = [(fill[0] * 255.0) as u8, (fill[1] * 255.0) as u8, (fill[2] * 255.0) as u8];
        for y in 0..height {
            for x in 0..width {
                let byte = data.get(y * row_bytes + x / 8).copied().unwrap_or(0xFF);
                let bit = (byte >> (7 - (x % 8))) & 1;
                let painted = (bit == 0) != decode_inverts;
                let at = (y * width + x) * 4;
                pixels[at..at + 3].copy_from_slice(&colour);
                pixels[at + 3] = if painted { 255 } else { 0 };
            }
        }
    } else {
        let space = file.get(dictionary, "ColorSpace");
        let space = if space.is_null() { file.get(dictionary, "CS") } else { space };
        let space = Space::of(file, &space)?;
        let components = space.components();
        let row_bits = width * components * bits;
        let row_bytes = row_bits.div_ceil(8);
        let max = ((1u32 << bits) - 1) as f32;
        for y in 0..height {
            let row = data.get(y * row_bytes..).unwrap_or(&[]);
            for x in 0..width {
                let mut samples = [0f32; 4];
                let mut raw = [0u32; 4];
                for (component, slot) in samples.iter_mut().enumerate().take(components) {
                    let value = sample(row, (x * components + component) * bits, bits);
                    raw[component] = value;
                    *slot =
                        if decode_inverts { 1.0 - value as f32 / max } else { value as f32 / max };
                }
                let rgb = space.rgb(&samples, raw[0] as usize);
                let at = (y * width + x) * 4;
                pixels[at] = rgb[0];
                pixels[at + 1] = rgb[1];
                pixels[at + 2] = rgb[2];
                pixels[at + 3] = 255;
            }
        }
        // The soft mask: a grey picture whose values are the alpha.
        if let Object::Stream(mask) = file.get(dictionary, "SMask") {
            apply_soft_mask(file, &mask, &mut pixels, width, height);
        }
    }
    let mut canvas = wp_raster::Canvas::new(width, height);
    canvas.paste_rect(0, 0, width as i32, height as i32, &pixels);
    Some((wp_raster::encode_png(&canvas), "png"))
}

fn apply_soft_mask(file: &File<'_>, mask: &Stream, pixels: &mut [u8], width: usize, height: usize) {
    let (data, filter) = file.decode(mask);
    if filter.is_some() {
        return;
    }
    let mask_width = file.get(&mask.dictionary, "Width").as_integer().unwrap_or(0).max(0) as usize;
    let mask_height =
        file.get(&mask.dictionary, "Height").as_integer().unwrap_or(0).max(0) as usize;
    let bits = file.get(&mask.dictionary, "BitsPerComponent").as_integer().unwrap_or(8).clamp(1, 16)
        as usize;
    if mask_width == 0 || mask_height == 0 {
        return;
    }
    let row_bytes = (mask_width * bits).div_ceil(8);
    let max = ((1u32 << bits) - 1) as f32;
    for y in 0..height {
        let my = y * mask_height / height;
        let row = data.get(my * row_bytes..).unwrap_or(&[]);
        for x in 0..width {
            let mx = x * mask_width / width;
            let value = sample(row, mx * bits, bits) as f32 / max;
            pixels[(y * width + x) * 4 + 3] = (value * 255.0).round() as u8;
        }
    }
}

/// One sample of `bits` bits at a bit offset in a row.
fn sample(row: &[u8], bit_offset: usize, bits: usize) -> u32 {
    match bits {
        8 => u32::from(row.get(bit_offset / 8).copied().unwrap_or(0)),
        16 => {
            let at = bit_offset / 8;
            u32::from(row.get(at).copied().unwrap_or(0)) << 8
                | u32::from(row.get(at + 1).copied().unwrap_or(0))
        }
        1 | 2 | 4 => {
            let byte = row.get(bit_offset / 8).copied().unwrap_or(0);
            let shift = 8 - bits - (bit_offset % 8);
            u32::from(byte >> shift) & ((1 << bits) - 1)
        }
        _ => 0,
    }
}

/// A colour space, as far as turning samples into RGB goes.
enum Space {
    Gray,
    Rgb,
    Cmyk,
    /// A palette: the base space's samples for each index, `base
    /// components` bytes each.
    Indexed {
        base: Box<Space>,
        table: Vec<u8>,
    },
    /// A separation or DeviceN: one tint, dark where it is full.
    Tint(usize),
}

impl Space {
    fn of(file: &File<'_>, object: &Object) -> Option<Self> {
        Self::of_depth(file, object, 0)
    }

    fn of_depth(file: &File<'_>, object: &Object, depth: usize) -> Option<Self> {
        if depth > 4 {
            return None;
        }
        match object {
            Object::Name(name) => Some(match name.as_str() {
                "DeviceGray" | "G" | "CalGray" => Self::Gray,
                "DeviceRGB" | "RGB" | "CalRGB" | "Lab" => Self::Rgb,
                "DeviceCMYK" | "CMYK" => Self::Cmyk,
                "Pattern" => return None,
                _ => Self::Gray,
            }),
            Object::Array(items) => {
                let family = items.first().and_then(Object::as_name)?;
                match family {
                    "ICCBased" => {
                        let profile = file.resolve(items.get(1)?);
                        let n = profile
                            .as_dictionary()
                            .map(|d| file.get(d, "N").as_integer().unwrap_or(3))
                            .unwrap_or(3);
                        Some(match n {
                            1 => Self::Gray,
                            4 => Self::Cmyk,
                            _ => Self::Rgb,
                        })
                    }
                    "CalRGB" | "Lab" => Some(Self::Rgb),
                    "CalGray" => Some(Self::Gray),
                    "Indexed" | "I" => {
                        let base = Self::of_depth(file, &file.resolve(items.get(1)?), depth + 1)?;
                        let table = match file.resolve(items.get(3)?) {
                            Object::String(bytes) => bytes,
                            Object::Stream(stream) => file.decode(&stream).0,
                            _ => return None,
                        };
                        Some(Self::Indexed { base: Box::new(base), table })
                    }
                    "Separation" => Some(Self::Tint(1)),
                    "DeviceN" => {
                        let names = file.resolve(items.get(1)?);
                        Some(Self::Tint(names.as_array().map_or(1, <[Object]>::len).max(1)))
                    }
                    "DeviceGray" | "DeviceRGB" | "DeviceCMYK" => {
                        Self::of_depth(file, &Object::Name(family.to_owned()), depth + 1)
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn components(&self) -> usize {
        match self {
            Self::Gray | Self::Indexed { .. } => 1,
            Self::Rgb => 3,
            Self::Cmyk => 4,
            Self::Tint(n) => (*n).clamp(1, 4),
        }
    }

    /// The colour of one pixel's samples, 0 to 1 each — or, for a
    /// palette, the raw index.
    fn rgb(&self, samples: &[f32; 4], index: usize) -> [u8; 3] {
        let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        match self {
            Self::Gray => [byte(samples[0]); 3],
            Self::Rgb => [byte(samples[0]), byte(samples[1]), byte(samples[2])],
            Self::Cmyk => {
                let k = samples[3];
                [
                    byte((1.0 - samples[0]) * (1.0 - k)),
                    byte((1.0 - samples[1]) * (1.0 - k)),
                    byte((1.0 - samples[2]) * (1.0 - k)),
                ]
            }
            Self::Indexed { base, table } => {
                let n = base.components();
                let mut inner = [0f32; 4];
                for (component, slot) in inner.iter_mut().enumerate().take(n) {
                    *slot =
                        f32::from(table.get(index * n + component).copied().unwrap_or(0)) / 255.0;
                }
                base.rgb(&inner, 0)
            }
            Self::Tint(_) => [byte(1.0 - samples[0]); 3],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_come_out_of_packed_rows() {
        let row = [0b1010_1100, 0x12, 0x34];
        assert_eq!(sample(&row, 0, 1), 1);
        assert_eq!(sample(&row, 1, 1), 0);
        assert_eq!(sample(&row, 4, 4), 0xC);
        assert_eq!(sample(&row, 8, 8), 0x12);
        assert_eq!(sample(&row, 8, 16), 0x1234);
    }
}
