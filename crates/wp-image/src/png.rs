//! Reading PNG, to RFC 2083.
//!
//! # The shape of the format
//!
//! A signature, then a sequence of chunks. `IHDR` says how big the picture is
//! and how its pixels are stored; `PLTE` holds a palette; `IDAT` carries the
//! pixels, zlib-compressed and split across as many chunks as the writer felt
//! like; `IEND` ends it. Anything else is skipped, which is what the format is
//! designed for.
//!
//! # Why the rows are filtered
//!
//! Compression works far better on differences than on values, so each row is
//! stored as a difference from its neighbours — above, to the left, or an
//! average of the two. Undoing that is most of the work here, and it has to be
//! done in order: a row cannot be reconstructed before the row above it.

use wp_deflate::inflate_zlib;

use crate::{check_size, Error, Image, MAX_PIXELS};

/// The eight bytes every PNG begins with.
pub const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// How the pixels of a picture are stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ColorType {
    Grey,
    Rgb,
    Palette,
    GreyAlpha,
    Rgba,
}

impl ColorType {
    fn from_byte(value: u8) -> Result<Self, Error> {
        Ok(match value {
            0 => Self::Grey,
            2 => Self::Rgb,
            3 => Self::Palette,
            4 => Self::GreyAlpha,
            6 => Self::Rgba,
            _ => return Err(Error::Malformed("an unknown colour type")),
        })
    }

    /// How many values each pixel is written as.
    fn channels(self) -> usize {
        match self {
            Self::Grey | Self::Palette => 1,
            Self::GreyAlpha => 2,
            Self::Rgb => 3,
            Self::Rgba => 4,
        }
    }
}

/// What `IHDR` says.
#[derive(Clone, Copy, Debug)]
struct Header {
    width: usize,
    height: usize,
    depth: u8,
    color: ColorType,
    interlaced: bool,
}

impl Header {
    /// How many bits one pixel takes.
    fn bits_per_pixel(&self) -> usize {
        usize::from(self.depth) * self.color.channels()
    }

    /// How many bytes one row takes, rounded up to whole bytes.
    fn row_bytes(&self) -> usize {
        (self.width * self.bits_per_pixel()).div_ceil(8)
    }

    /// The distance between a byte and the same byte of the pixel before it,
    /// which is what the filters subtract. Never less than one byte.
    fn filter_step(&self) -> usize {
        self.bits_per_pixel().div_ceil(8).max(1)
    }
}

/// Decodes a PNG into eight-bit RGBA.
pub fn decode(data: &[u8]) -> Result<Image, Error> {
    if !data.starts_with(&SIGNATURE) {
        return Err(Error::UnknownFormat);
    }

    let mut header: Option<Header> = None;
    let mut palette: Vec<[u8; 3]> = Vec::new();
    let mut transparency: Vec<u8> = Vec::new();
    let mut compressed: Vec<u8> = Vec::new();

    let mut offset = SIGNATURE.len();
    loop {
        // Length, type, data, checksum. The checksum is not verified: a picture
        // that decodes is worth showing, and refusing one over a stale CRC
        // would lose the user their picture for nothing.
        let Some(length) = read_u32(data, offset) else {
            return Err(Error::Truncated);
        };
        let length = length as usize;
        let kind_at = offset + 4;
        let body_at = kind_at + 4;
        let Some(kind) = data.get(kind_at..body_at) else {
            return Err(Error::Truncated);
        };
        let Some(body) = data.get(body_at..body_at + length) else {
            return Err(Error::Truncated);
        };

        match kind {
            b"IHDR" => header = Some(read_header(body)?),
            b"PLTE" => {
                palette =
                    body.chunks_exact(3).map(|entry| [entry[0], entry[1], entry[2]]).collect();
            }
            b"tRNS" => transparency = body.to_vec(),
            b"IDAT" => compressed.extend_from_slice(body),
            b"IEND" => break,
            _ => {}
        }

        offset = body_at + length + 4;
        if offset >= data.len() {
            break;
        }
    }

    let Some(header) = header else {
        return Err(Error::Malformed("no header chunk"));
    };
    check_size(header.width, header.height)?;
    if header.interlaced {
        // Adam7 is rare outside the web and not worth guessing at.
        return Err(Error::Unsupported("an interlaced layout"));
    }
    if !matches!(header.depth, 1 | 2 | 4 | 8 | 16) {
        return Err(Error::Malformed("an impossible bit depth"));
    }
    if header.color == ColorType::Palette && palette.is_empty() {
        return Err(Error::Malformed("a palettised picture with no palette"));
    }

    // Every row carries one byte saying how it was filtered.
    let expected = (header.row_bytes() + 1) * header.height;
    let raw = inflate_zlib(&compressed, expected.min(MAX_PIXELS * 8) + 1)
        .map_err(|_| Error::Malformed("the pixel data does not decompress"))?;
    if raw.len() < expected {
        return Err(Error::Truncated);
    }

    let rows = unfilter(&raw, &header)?;
    Ok(to_rgba(&rows, &header, &palette, &transparency))
}

fn read_u32(data: &[u8], at: usize) -> Option<u32> {
    let bytes = data.get(at..at + 4)?;
    Some(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn read_header(body: &[u8]) -> Result<Header, Error> {
    if body.len() < 13 {
        return Err(Error::Malformed("a short header chunk"));
    }
    Ok(Header {
        width: read_u32(body, 0).unwrap_or(0) as usize,
        height: read_u32(body, 4).unwrap_or(0) as usize,
        depth: body[8],
        color: ColorType::from_byte(body[9])?,
        interlaced: body[12] != 0,
    })
}

/// Undoes the per-row filtering, giving the raw rows back.
///
/// Each row is reconstructed from the one above it, so this cannot be done out
/// of order or in parallel — the format is defined as a chain.
fn unfilter(raw: &[u8], header: &Header) -> Result<Vec<u8>, Error> {
    let row_bytes = header.row_bytes();
    let step = header.filter_step();
    let mut out = vec![0u8; row_bytes * header.height];

    for row in 0..header.height {
        let source = row * (row_bytes + 1);
        let filter = raw[source];
        let start = row * row_bytes;

        for index in 0..row_bytes {
            let value = raw[source + 1 + index];
            let left = if index >= step { out[start + index - step] } else { 0 };
            let above = if row > 0 { out[start - row_bytes + index] } else { 0 };
            let above_left =
                if row > 0 && index >= step { out[start - row_bytes + index - step] } else { 0 };

            out[start + index] = match filter {
                0 => value,
                1 => value.wrapping_add(left),
                2 => value.wrapping_add(above),
                3 => value.wrapping_add(((u16::from(left) + u16::from(above)) / 2) as u8),
                4 => value.wrapping_add(paeth(left, above, above_left)),
                _ => return Err(Error::Malformed("an unknown row filter")),
            };
        }
    }

    Ok(out)
}

/// The predictor from the specification: whichever neighbour the gradient of
/// the three is closest to.
fn paeth(left: u8, above: u8, above_left: u8) -> u8 {
    let estimate = i16::from(left) + i16::from(above) - i16::from(above_left);
    let distance_left = (estimate - i16::from(left)).abs();
    let distance_above = (estimate - i16::from(above)).abs();
    let distance_corner = (estimate - i16::from(above_left)).abs();

    if distance_left <= distance_above && distance_left <= distance_corner {
        left
    } else if distance_above <= distance_corner {
        above
    } else {
        above_left
    }
}

/// Turns unfiltered rows into eight-bit RGBA.
fn to_rgba(rows: &[u8], header: &Header, palette: &[[u8; 3]], transparency: &[u8]) -> Image {
    let mut pixels = Vec::with_capacity(header.width * header.height * 4);
    let row_bytes = header.row_bytes();
    let channels = header.color.channels();

    for row in 0..header.height {
        let line = &rows[row * row_bytes..(row + 1) * row_bytes];
        for column in 0..header.width {
            let mut sample = [0u16; 4];
            for (channel, value) in sample.iter_mut().enumerate().take(channels) {
                *value = read_sample(line, column * channels + channel, header.depth);
            }

            let (red, green, blue, alpha) = match header.color {
                ColorType::Grey => {
                    let grey = scale(sample[0], header.depth);
                    // A greyscale picture may name one shade as transparent.
                    let clear = transparency
                        .get(0..2)
                        .is_some_and(|bytes| u16::from_be_bytes([bytes[0], bytes[1]]) == sample[0]);
                    (grey, grey, grey, if clear { 0 } else { 255 })
                }
                ColorType::GreyAlpha => {
                    let grey = scale(sample[0], header.depth);
                    (grey, grey, grey, scale(sample[1], header.depth))
                }
                ColorType::Rgb => {
                    let clear = transparency.get(0..6).is_some_and(|bytes| {
                        u16::from_be_bytes([bytes[0], bytes[1]]) == sample[0]
                            && u16::from_be_bytes([bytes[2], bytes[3]]) == sample[1]
                            && u16::from_be_bytes([bytes[4], bytes[5]]) == sample[2]
                    });
                    (
                        scale(sample[0], header.depth),
                        scale(sample[1], header.depth),
                        scale(sample[2], header.depth),
                        if clear { 0 } else { 255 },
                    )
                }
                ColorType::Rgba => (
                    scale(sample[0], header.depth),
                    scale(sample[1], header.depth),
                    scale(sample[2], header.depth),
                    scale(sample[3], header.depth),
                ),
                ColorType::Palette => {
                    let index = sample[0] as usize;
                    let entry = palette.get(index).copied().unwrap_or([0, 0, 0]);
                    // The transparency chunk gives one alpha per palette entry,
                    // for as many entries as it lists; the rest are opaque.
                    let alpha = transparency.get(index).copied().unwrap_or(255);
                    (entry[0], entry[1], entry[2], alpha)
                }
            };

            pixels.extend_from_slice(&[red, green, blue, alpha]);
        }
    }

    Image { width: header.width, height: header.height, pixels }
}

/// Reads one value out of a row, whatever width the values are.
fn read_sample(line: &[u8], index: usize, depth: u8) -> u16 {
    match depth {
        16 => {
            let at = index * 2;
            line.get(at..at + 2).map_or(0, |bytes| u16::from_be_bytes([bytes[0], bytes[1]]))
        }
        8 => u16::from(line.get(index).copied().unwrap_or(0)),
        // Below a byte, several values share one: the first is in the high bits.
        bits => {
            let per_byte = 8 / usize::from(bits);
            let byte = line.get(index / per_byte).copied().unwrap_or(0);
            let position = index % per_byte;
            let shift = 8 - usize::from(bits) * (position + 1);
            let mask = (1u16 << bits) - 1;
            (u16::from(byte) >> shift) & mask
        }
    }
}

/// Brings a value of any depth up to the full eight-bit range.
///
/// Not a shift: a four-bit 15 has to become 255, not 240, or every white in the
/// picture would come out slightly grey.
fn scale(value: u16, depth: u8) -> u8 {
    match depth {
        8 => value as u8,
        16 => (value >> 8) as u8,
        1 => {
            if value != 0 {
                255
            } else {
                0
            }
        }
        bits => {
            let maximum = (1u32 << bits) - 1;
            ((u32::from(value) * 255 + maximum / 2) / maximum) as u8
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_deflate::{compress_zlib, crc32};

    /// Builds a PNG out of a header and rows, so the decoder can be tested
    /// against pictures whose every pixel is known.
    fn build(width: u32, height: u32, depth: u8, color: u8, rows: &[Vec<u8>]) -> Vec<u8> {
        let mut out = SIGNATURE.to_vec();

        let mut header = Vec::new();
        header.extend_from_slice(&width.to_be_bytes());
        header.extend_from_slice(&height.to_be_bytes());
        header.extend_from_slice(&[depth, color, 0, 0, 0]);
        push_chunk(&mut out, b"IHDR", &header);

        // Every row is written unfiltered, which the format calls filter zero.
        let mut raw = Vec::new();
        for row in rows {
            raw.push(0);
            raw.extend_from_slice(row);
        }
        push_chunk(&mut out, b"IDAT", &compress_zlib(&raw));
        push_chunk(&mut out, b"IEND", &[]);
        out
    }

    fn push_chunk(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(body);

        let mut checked = kind.to_vec();
        checked.extend_from_slice(body);
        out.extend_from_slice(&crc32(&checked).to_be_bytes());
    }

    #[test]
    fn anything_else_is_not_a_png() {
        assert_eq!(decode(b"nowhere near a png"), Err(Error::UnknownFormat));
    }

    #[test]
    fn a_truncated_file_is_refused_rather_than_guessed_at() {
        let png = build(1, 1, 8, 2, &[vec![255, 0, 0]]);
        assert!(decode(&png[..png.len() - 8]).is_err());
    }

    #[test]
    fn a_single_red_pixel_comes_back_red() {
        let png = build(1, 1, 8, 2, &[vec![255, 0, 0]]);
        let image = decode(&png).unwrap();

        assert_eq!((image.width, image.height), (1, 1));
        assert_eq!(image.pixels, vec![255, 0, 0, 255]);
    }

    #[test]
    fn colour_with_alpha_keeps_its_alpha() {
        let png = build(2, 1, 8, 6, &[vec![10, 20, 30, 40, 50, 60, 70, 80]]);
        let image = decode(&png).unwrap();

        assert_eq!(image.pixels, vec![10, 20, 30, 40, 50, 60, 70, 80]);
    }

    #[test]
    fn greyscale_becomes_the_same_value_in_every_channel() {
        let png = build(3, 1, 8, 0, &[vec![0, 128, 255]]);
        let image = decode(&png).unwrap();

        assert_eq!(image.pixels, vec![0, 0, 0, 255, 128, 128, 128, 255, 255, 255, 255, 255]);
    }

    #[test]
    fn a_row_is_reconstructed_from_the_one_above_it() {
        // Two rows: the second is written as a difference from the first.
        let mut out = SIGNATURE.to_vec();
        let mut header = Vec::new();
        header.extend_from_slice(&1u32.to_be_bytes());
        header.extend_from_slice(&2u32.to_be_bytes());
        header.extend_from_slice(&[8, 2, 0, 0, 0]);
        push_chunk(&mut out, b"IHDR", &header);

        // Row one plain, row two filtered "up" with a difference of ten.
        let raw = vec![0, 100, 110, 120, 2, 10, 10, 10];
        push_chunk(&mut out, b"IDAT", &compress_zlib(&raw));
        push_chunk(&mut out, b"IEND", &[]);

        let image = decode(&out).unwrap();
        assert_eq!(image.pixels, vec![100, 110, 120, 255, 110, 120, 130, 255]);
    }

    #[test]
    fn the_paeth_predictor_picks_the_nearest_neighbour() {
        // The estimate is left + above - corner; whichever of the three is
        // nearest to it wins.
        assert_eq!(paeth(50, 10, 10), 50, "the estimate lands on the left");
        assert_eq!(paeth(10, 50, 10), 50, "and here on the one above");
        assert_eq!(paeth(10, 20, 15), 15, "and here on the corner");
    }

    #[test]
    fn the_paeth_predictor_breaks_ties_the_way_the_specification_says() {
        // Left first, then above, then the corner — an order that matters,
        // because a decoder that broke ties differently would drift away from
        // the encoder row by row.
        assert_eq!(paeth(10, 10, 10), 10);
        assert_eq!(paeth(10, 10, 30), 10, "left and above tie, and left wins");
    }

    #[test]
    fn four_bit_white_comes_out_fully_white() {
        // A shift would give 240, which would tint every white in the picture.
        assert_eq!(scale(15, 4), 255);
        assert_eq!(scale(0, 4), 0);
        assert_eq!(scale(1, 1), 255);
        assert_eq!(scale(3, 2), 255);
    }

    #[test]
    fn values_narrower_than_a_byte_are_read_from_within_one() {
        // Two four-bit values packed into one byte, high half first.
        let line = [0xF0u8];
        assert_eq!(read_sample(&line, 0, 4), 15);
        assert_eq!(read_sample(&line, 1, 4), 0);
    }

    #[test]
    fn sixteen_bit_values_are_brought_down_to_eight() {
        assert_eq!(scale(0xFFFF, 16), 255);
        assert_eq!(scale(0x8000, 16), 128);
    }

    #[test]
    fn a_picture_with_no_header_is_refused() {
        let mut out = SIGNATURE.to_vec();
        push_chunk(&mut out, b"IEND", &[]);
        assert_eq!(decode(&out), Err(Error::Malformed("no header chunk")));
    }

    #[test]
    fn an_impossible_size_is_refused_before_anything_is_allocated() {
        let mut out = SIGNATURE.to_vec();
        let mut header = Vec::new();
        header.extend_from_slice(&u32::MAX.to_be_bytes());
        header.extend_from_slice(&u32::MAX.to_be_bytes());
        header.extend_from_slice(&[8, 2, 0, 0, 0]);
        push_chunk(&mut out, b"IHDR", &header);
        push_chunk(&mut out, b"IEND", &[]);

        assert_eq!(decode(&out), Err(Error::TooLarge));
    }

    #[test]
    fn an_interlaced_picture_says_so_rather_than_drawing_nonsense() {
        let mut out = SIGNATURE.to_vec();
        let mut header = Vec::new();
        header.extend_from_slice(&4u32.to_be_bytes());
        header.extend_from_slice(&4u32.to_be_bytes());
        header.extend_from_slice(&[8, 2, 0, 0, 1]);
        push_chunk(&mut out, b"IHDR", &header);
        push_chunk(&mut out, b"IEND", &[]);

        assert_eq!(decode(&out), Err(Error::Unsupported("an interlaced layout")));
    }

    #[test]
    fn a_chunk_nobody_understands_is_stepped_over() {
        // Which is what the format is designed for: an unknown chunk is data,
        // not an error.
        let mut png = build(1, 1, 8, 2, &[vec![1, 2, 3]]);
        let end = png.len() - 12;
        let mut with_extra = png[..end].to_vec();
        push_chunk(&mut with_extra, b"tEXt", b"a comment nobody reads");
        with_extra.extend_from_slice(&png.split_off(end));

        assert_eq!(decode(&with_extra).unwrap().pixels, vec![1, 2, 3, 255]);
    }
}
