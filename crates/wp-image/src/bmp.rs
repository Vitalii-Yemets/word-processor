//! Reading Windows bitmaps, in the several forms a document carries.
//!
//! # The shape of the format
//!
//! Fourteen bytes saying where the pixels begin, then a header describing them,
//! then — usually — a palette, then the rows. Nothing is compressed in the
//! ordinary case: a bitmap is the pixels written down, which is why it is the
//! format a program reaches for when it has no encoder and why documents from
//! the nineteen-nineties are full of them.
//!
//! # Why there are so many headers
//!
//! Because the format was extended four times and every version is still
//! written. The first is twelve bytes and cannot say anything but the size and
//! the depth; the second, forty bytes, added compression and a palette count;
//! the fourth and fifth added colour masks, a colour space and an alpha
//! channel. A reader that understood only one of them would be a reader that
//! failed on half the bitmaps in the world, so the length of the header is read
//! first and everything after it is taken as far as that length allows.
//!
//! # Which way up
//!
//! Upwards, unless the height is negative. A bitmap is a picture of what is in
//! video memory, and video memory of the time had the bottom row first; a
//! negative height was the later convention for saying it is stored the way it
//! is read. Both appear, so both are read, and what comes out of here is always
//! the top row first.
//!
//! # What is not read
//!
//! Nothing that a bitmap in a document is written in. A bitmap whose pixels are
//! a whole PNG or a whole JPEG — which the format allows, for printer drivers —
//! is handed to the decoder for that format rather than refused.

use crate::{check_size, Error, Image};

/// The two bytes every bitmap begins with.
pub const SIGNATURE: [u8; 2] = *b"BM";

/// How the pixels are written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Compression {
    /// Not compressed at all: the pixels, row by row.
    None,
    /// Runs of one colour, eight bits to the pixel.
    Rle8,
    /// The same, four bits to the pixel.
    Rle4,
    /// Not compressed, but the bits of each pixel are divided up by masks the
    /// file states rather than in the usual places.
    Fields,
    /// The pixels are a picture in another format altogether.
    Embedded,
}

/// Everything the header says about the pixels.
#[derive(Clone, Copy, Debug)]
struct Header {
    width: usize,
    height: usize,
    /// Whether the bottom row is written first, which is the older convention
    /// and still the usual one.
    upwards: bool,
    bits_per_pixel: usize,
    compression: Compression,
    /// Which bits of a pixel are which colour. Only for [`Compression::Fields`]
    /// and for the headers that state them outright.
    masks: [u32; 4],
    /// How many entries the palette has, and how many bytes each takes.
    palette_entries: usize,
    palette_entry_size: usize,
    /// Where the palette and the pixels begin, from the start of the file.
    palette_at: usize,
    pixels_at: usize,
}

/// Decodes a Windows bitmap into eight-bit RGBA.
pub fn decode(data: &[u8]) -> Result<Image, Error> {
    if !data.starts_with(&SIGNATURE) {
        return Err(Error::UnknownFormat);
    }
    let header = read_header(data)?;
    check_size(header.width, header.height)?;

    if header.compression == Compression::Embedded {
        // A bitmap whose pixels are a whole picture in another format. The
        // header says how big it is and the picture inside says so as well;
        // the picture inside is the one that knows.
        //
        // Only the two the format means by it. A bitmap inside a bitmap is not
        // something the format allows, and following one would be following
        // whatever depth of them a damaged file asked for.
        let inside = data.get(header.pixels_at..).ok_or(Error::Truncated)?;
        return match crate::Format::detect(inside) {
            Some(crate::Format::Png) => crate::png::decode(inside),
            Some(crate::Format::Jpeg) => crate::jpeg::decode(inside),
            _ => Err(Error::Malformed("a bitmap whose pixels are not a picture")),
        };
    }

    let palette = read_palette(data, &header);
    let mut image = Image::empty(header.width, header.height);
    let pixels = data.get(header.pixels_at..).ok_or(Error::Truncated)?;

    match header.compression {
        Compression::Rle8 | Compression::Rle4 => {
            read_runs(pixels, &header, &palette, &mut image)?;
        }
        _ => read_rows(pixels, &header, &palette, &mut image)?,
    }

    Ok(image)
}

/// Reads the file header and whichever of the five bitmap headers follows it.
fn read_header(data: &[u8]) -> Result<Header, Error> {
    let pixels_at = usize::try_from(number(data, 10, 4)?).unwrap_or(0);
    let size = usize::try_from(number(data, 14, 4)?).unwrap_or(0);

    // The oldest header says only the size and the depth, and says the size in
    // half as many bits.
    if size == 12 {
        let width = usize::try_from(number(data, 18, 2)?).unwrap_or(0);
        let height = usize::try_from(number(data, 20, 2)?).unwrap_or(0);
        let bits_per_pixel = usize::try_from(number(data, 24, 2)?).unwrap_or(0);
        let palette_entries = if bits_per_pixel <= 8 { 1usize << bits_per_pixel } else { 0 };
        return Ok(Header {
            width,
            height,
            upwards: true,
            bits_per_pixel,
            compression: Compression::None,
            masks: [0; 4],
            palette_entries,
            // Three bytes to an entry and not four: the older header's palette
            // has no fourth byte at all.
            palette_entry_size: 3,
            palette_at: 26,
            pixels_at: if pixels_at == 0 { 26 + palette_entries * 3 } else { pixels_at },
        });
    }
    if size < 40 {
        return Err(Error::Malformed("a bitmap header of no known length"));
    }

    let width = usize::try_from(number(data, 18, 4)? as i32).map_err(|_| Error::TooLarge)?;
    let stated_height = number(data, 22, 4)? as i32;
    let upwards = stated_height >= 0;
    let height = usize::try_from(stated_height.unsigned_abs()).unwrap_or(0);
    let bits_per_pixel = usize::try_from(number(data, 28, 2)?).unwrap_or(0);
    let stated_compression = number(data, 30, 4)?;
    let stated_colours = usize::try_from(number(data, 46, 4)?).unwrap_or(0);

    let compression = match stated_compression {
        0 => Compression::None,
        1 => Compression::Rle8,
        2 => Compression::Rle4,
        3 | 6 => Compression::Fields,
        4 | 5 => Compression::Embedded,
        _ => return Err(Error::Unsupported("a compression this decoder does not read")),
    };

    // The masks are in the header from the fourth version onwards, and in the
    // place the palette would be for the second when it says the bits are
    // divided up by masks at all.
    let mut masks = [0u32; 4];
    let mut after_masks = 14 + size;
    if size >= 108 {
        for (index, mask) in masks.iter_mut().enumerate() {
            *mask = number(data, 54 + index * 4, 4)?;
        }
    } else if compression == Compression::Fields {
        // Three masks, or four when the file says it states one for the fourth
        // channel as well.
        let count = if stated_compression == 6 { 4 } else { 3 };
        for (index, mask) in masks.iter_mut().enumerate().take(count) {
            *mask = number(data, 14 + size + index * 4, 4)?;
        }
        after_masks += count * 4;
    }

    let palette_entries = if bits_per_pixel <= 8 {
        if stated_colours > 0 {
            stated_colours.min(1usize << bits_per_pixel)
        } else {
            1usize << bits_per_pixel
        }
    } else {
        // A deep bitmap may still carry a palette, for screens that cannot show
        // one. Nothing here reads it: the pixels say their own colours.
        0
    };

    Ok(Header {
        width,
        height,
        upwards,
        bits_per_pixel,
        compression,
        masks,
        palette_entries,
        palette_entry_size: 4,
        palette_at: after_masks,
        pixels_at: if pixels_at == 0 { after_masks + palette_entries * 4 } else { pixels_at },
    })
}

/// Reads the palette, as opaque RGBA.
///
/// Short of what the header promised is not an error: a bitmap that says it has
/// 256 colours and carries 200 is one Word opens, and the rest are black.
fn read_palette(data: &[u8], header: &Header) -> Vec<[u8; 4]> {
    let mut out = vec![[0, 0, 0, 255]; header.palette_entries];
    for (index, entry) in out.iter_mut().enumerate() {
        let at = header.palette_at + index * header.palette_entry_size;
        let Some(bytes) = data.get(at..at + 3) else { break };
        // Blue, green, red, in that order: the same order the pixels are in.
        *entry = [bytes[2], bytes[1], bytes[0], 255];
    }
    out
}

/// Reads the rows of a bitmap that is not compressed.
fn read_rows(
    pixels: &[u8],
    header: &Header,
    palette: &[[u8; 4]],
    image: &mut Image,
) -> Result<(), Error> {
    // Every row is padded out to a multiple of four bytes, whatever the depth.
    let bits = header.width.checked_mul(header.bits_per_pixel).ok_or(Error::TooLarge)?;
    let stride = bits.div_ceil(8).div_ceil(4) * 4;
    let channels = Channels::of(header);

    for row in 0..header.height {
        let source = if header.upwards { header.height - 1 - row } else { row };
        let Some(line) = pixels.get(source * stride..) else { break };

        for column in 0..header.width {
            let colour = match header.bits_per_pixel {
                1 | 2 | 4 | 8 => {
                    let index = packed(line, column, header.bits_per_pixel);
                    palette.get(index).copied().unwrap_or([0, 0, 0, 255])
                }
                16 => {
                    let at = column * 2;
                    let Some(bytes) = line.get(at..at + 2) else { break };
                    channels.split(u32::from(u16::from_le_bytes([bytes[0], bytes[1]])))
                }
                24 => {
                    let at = column * 3;
                    let Some(bytes) = line.get(at..at + 3) else { break };
                    [bytes[2], bytes[1], bytes[0], 255]
                }
                32 => {
                    let at = column * 4;
                    let Some(bytes) = line.get(at..at + 4) else { break };
                    channels.split(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
                }
                _ => return Err(Error::Unsupported("a depth this decoder does not read")),
            };
            put(image, column, row, colour);
        }
    }

    // A bitmap of 32 bits whose fourth byte is zero everywhere is not a picture
    // nobody can see: it is one written by something that left the byte alone.
    // Word shows such a bitmap; so does everything else.
    if header.bits_per_pixel == 32 && !channels.states_alpha {
        make_opaque(image);
    }
    Ok(())
}

/// Which bits of a pixel are which colour.
#[derive(Clone, Copy, Debug)]
struct Channels {
    masks: [u32; 4],
    /// Whether the file said anything about the fourth channel at all.
    states_alpha: bool,
}

impl Channels {
    /// The masks for a depth, from the header or from the usual places.
    fn of(header: &Header) -> Self {
        let stated = header.masks[0] | header.masks[1] | header.masks[2];
        if stated != 0 {
            return Self { masks: header.masks, states_alpha: header.masks[3] != 0 };
        }
        // What the format means by sixteen bits when it does not say: five bits
        // each of red, green and blue, and one bit left over.
        let masks = match header.bits_per_pixel {
            16 => [0x7C00, 0x03E0, 0x001F, 0],
            _ => [0x00FF_0000, 0x0000_FF00, 0x0000_00FF, 0xFF00_0000],
        };
        Self { masks, states_alpha: false }
    }

    /// Pulls one pixel apart into eight-bit channels.
    fn split(self, value: u32) -> [u8; 4] {
        let mut out = [0u8, 0, 0, 255];
        for (index, mask) in self.masks.iter().copied().enumerate() {
            if mask == 0 {
                continue;
            }
            out[index] = scale(value & mask, mask);
        }
        out
    }
}

/// Brings a channel of any width up to eight bits.
///
/// Five bits of red must become eight, and how that is done matters twice over.
/// The brightest value has to come out as 255 and not as 248, or a white
/// picture would be drawn slightly grey — and the answer has to be the same one
/// every other reader of these formats gives, or a bitmap read here and the
/// same bitmap read by Windows would be two different pictures.
///
/// Both come out of repeating the value's own top bits into the space below it,
/// which is what every one of them does. Dividing instead is out by one here
/// and there, and that one is visible when two readings are compared.
fn scale(value: u32, mask: u32) -> u8 {
    let shift = mask.trailing_zeros();
    let width = mask.count_ones();
    if width == 0 {
        return 0;
    }
    let raw = value >> shift;
    // Wider than a byte: there is nothing to fill in, only room to give up.
    if width >= 8 {
        return (raw >> (width - 8)) as u8;
    }

    let mut out = raw << (8 - width);
    let mut filled = width;
    while filled < 8 {
        out |= out >> filled;
        filled *= 2;
    }
    (out & 0xFF) as u8
}

/// The palette index of one pixel of a row written several pixels to the byte.
fn packed(line: &[u8], column: usize, depth: usize) -> usize {
    let per_byte = 8 / depth;
    let byte = line.get(column / per_byte).copied().unwrap_or(0);
    let within = column % per_byte;
    // The leftmost pixel is in the top bits, which is the way every packed
    // format of the era writes them.
    let shift = 8 - depth * (within + 1);
    usize::from((byte >> shift) & ((1u8 << depth) - 1))
}

/// Reads the rows of a bitmap written as runs.
///
/// # What the runs say
///
/// A pair of bytes at a time. A count and a colour is a run of that colour; a
/// zero and a small number is one of three instructions — end of the row, end
/// of the picture, or move the cursor — and a zero and anything larger is that
/// many pixels written out one by one.
///
/// A bitmap written this way need not fill its own rectangle. Everything not
/// reached stays as it was, which for this decoder is transparent: that is what
/// the format means by it, and it is how an irregular shape is stored.
fn read_runs(
    pixels: &[u8],
    header: &Header,
    palette: &[[u8; 4]],
    image: &mut Image,
) -> Result<(), Error> {
    let four_bit = header.compression == Compression::Rle4;
    let (mut x, mut y) = (0usize, 0usize);
    let mut at = 0usize;

    while at + 1 < pixels.len() {
        let count = usize::from(pixels[at]);
        let value = pixels[at + 1];
        at += 2;

        if count > 0 {
            for step in 0..count {
                let index = if four_bit {
                    // Two colours alternating, which is how a run of four-bit
                    // pixels holds a chequer.
                    if step % 2 == 0 {
                        usize::from(value >> 4)
                    } else {
                        usize::from(value & 0x0F)
                    }
                } else {
                    usize::from(value)
                };
                paint(image, header, x + step, y, palette.get(index).copied());
            }
            x += count;
            continue;
        }

        match value {
            // The end of a row.
            0 => {
                x = 0;
                y += 1;
            }
            // The end of the picture.
            1 => break,
            // A jump: two more bytes saying how far along and how far down.
            2 => {
                let Some(step) = pixels.get(at..at + 2) else { break };
                at += 2;
                x += usize::from(step[0]);
                y += usize::from(step[1]);
            }
            // That many pixels written out one by one, padded to an even
            // number of bytes.
            length => {
                let length = usize::from(length);
                let bytes = if four_bit { length.div_ceil(2) } else { length };
                let Some(run) = pixels.get(at..at + bytes) else { break };
                for step in 0..length {
                    let index = if four_bit {
                        let byte = run[step / 2];
                        if step % 2 == 0 {
                            usize::from(byte >> 4)
                        } else {
                            usize::from(byte & 0x0F)
                        }
                    } else {
                        usize::from(run[step])
                    };
                    paint(image, header, x + step, y, palette.get(index).copied());
                }
                x += length;
                at += bytes + bytes % 2;
            }
        }
    }

    Ok(())
}

/// Puts one pixel of a run-length bitmap down, the right way up.
fn paint(image: &mut Image, header: &Header, x: usize, y: usize, colour: Option<[u8; 4]>) {
    let Some(colour) = colour else { return };
    if x >= header.width || y >= header.height {
        return;
    }
    let row = if header.upwards { header.height - 1 - y } else { y };
    put(image, x, row, colour);
}

fn put(image: &mut Image, x: usize, y: usize, colour: [u8; 4]) {
    let at = (y * image.width + x) * 4;
    let Some(slice) = image.pixels.get_mut(at..at + 4) else { return };
    slice.copy_from_slice(&colour);
}

/// Makes every pixel opaque, whatever its fourth byte said.
fn make_opaque(image: &mut Image) {
    for pixel in image.pixels.chunks_exact_mut(4) {
        pixel[3] = 255;
    }
}

/// A little-endian number of one, two or four bytes.
fn number(data: &[u8], at: usize, length: usize) -> Result<u32, Error> {
    let bytes = data.get(at..at + length).ok_or(Error::Truncated)?;
    let mut value = 0u32;
    for (index, byte) in bytes.iter().enumerate() {
        value |= u32::from(*byte) << (index * 8);
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bitmap with a forty-byte header, built to order.
    fn bitmap(width: i32, height: i32, depth: u16, compression: u32, body: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&SIGNATURE);
        out.extend_from_slice(&0u32.to_le_bytes()); // The size, which nothing reads.
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&54u32.to_le_bytes()); // Where the pixels begin.

        out.extend_from_slice(&40u32.to_le_bytes());
        out.extend_from_slice(&width.to_le_bytes());
        out.extend_from_slice(&height.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&depth.to_le_bytes());
        out.extend_from_slice(&compression.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes()); // The size of the pixels.
        out.extend_from_slice(&0u32.to_le_bytes()); // Pixels per metre, across.
        out.extend_from_slice(&0u32.to_le_bytes()); // And down.
        out.extend_from_slice(&0u32.to_le_bytes()); // Colours used.
        out.extend_from_slice(&0u32.to_le_bytes()); // Colours that matter.
        out.extend_from_slice(body);
        out
    }

    fn pixel(image: &Image, x: usize, y: usize) -> [u8; 4] {
        let at = (y * image.width + x) * 4;
        [image.pixels[at], image.pixels[at + 1], image.pixels[at + 2], image.pixels[at + 3]]
    }

    #[test]
    fn anything_else_is_not_a_bitmap() {
        assert_eq!(decode(b"not a bitmap"), Err(Error::UnknownFormat));
    }

    #[test]
    fn twenty_four_bits_are_read_blue_first() {
        // One row of two pixels, padded out to four bytes.
        let body = [0x01, 0x02, 0x03, 0xF0, 0xF1, 0xF2, 0x00, 0x00];
        let image = decode(&bitmap(2, 1, 24, 0, &body)).expect("a bitmap");
        assert_eq!(pixel(&image, 0, 0), [0x03, 0x02, 0x01, 255]);
        assert_eq!(pixel(&image, 1, 0), [0xF2, 0xF1, 0xF0, 255]);
    }

    #[test]
    fn the_bottom_row_comes_first_unless_the_height_is_negative() {
        // Two rows of one pixel each, each padded to four bytes.
        let body = [0xFF, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF, 0x00];
        let upwards = decode(&bitmap(1, 2, 24, 0, &body)).expect("a bitmap");
        assert_eq!(pixel(&upwards, 0, 0), [0xFF, 0, 0, 255], "the last row is at the top");
        assert_eq!(pixel(&upwards, 0, 1), [0, 0, 0xFF, 255]);

        let downwards = decode(&bitmap(1, -2, 24, 0, &body)).expect("a bitmap");
        assert_eq!(pixel(&downwards, 0, 0), [0, 0, 0xFF, 255], "the first row is at the top");
        assert_eq!(pixel(&downwards, 0, 1), [0xFF, 0, 0, 255]);
    }

    #[test]
    fn thirty_two_bits_with_no_alpha_written_are_opaque() {
        // Every fourth byte zero: a picture nobody could see, if it were read
        // as transparency.
        let body = [0x10, 0x20, 0x30, 0x00, 0x40, 0x50, 0x60, 0x00];
        let image = decode(&bitmap(2, 1, 32, 0, &body)).expect("a bitmap");
        assert_eq!(pixel(&image, 0, 0), [0x30, 0x20, 0x10, 255]);
        assert_eq!(pixel(&image, 1, 0), [0x60, 0x50, 0x40, 255]);
    }

    #[test]
    fn sixteen_bits_are_five_of_each_colour_by_default() {
        // 0b0_11111_00000_00000: red at its brightest and nothing else.
        let body = [0x00, 0x7C, 0x1F, 0x00];
        let image = decode(&bitmap(2, 1, 16, 0, &body)).expect("a bitmap");
        assert_eq!(pixel(&image, 0, 0), [255, 0, 0, 255], "five bits should reach the top");
        assert_eq!(pixel(&image, 1, 0), [0, 0, 255, 255]);
    }

    #[test]
    fn a_palette_is_looked_up_and_its_entries_are_blue_first() {
        let mut out = Vec::new();
        out.extend_from_slice(&SIGNATURE);
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&(54u32 + 8).to_le_bytes());

        out.extend_from_slice(&40u32.to_le_bytes());
        out.extend_from_slice(&4i32.to_le_bytes());
        out.extend_from_slice(&1i32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&4u16.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&2u32.to_le_bytes()); // Two colours used.
        out.extend_from_slice(&0u32.to_le_bytes());
        // The palette: blue, green, red, nothing.
        out.extend_from_slice(&[0x11, 0x22, 0x33, 0x00]);
        out.extend_from_slice(&[0x44, 0x55, 0x66, 0x00]);
        // Two bytes of pixels: 0, 1, 1, 0.
        out.extend_from_slice(&[0x01, 0x10, 0x00, 0x00]);

        let image = decode(&out).expect("a bitmap");
        assert_eq!(pixel(&image, 0, 0), [0x33, 0x22, 0x11, 255]);
        assert_eq!(pixel(&image, 1, 0), [0x66, 0x55, 0x44, 255]);
        assert_eq!(pixel(&image, 2, 0), [0x66, 0x55, 0x44, 255]);
        assert_eq!(pixel(&image, 3, 0), [0x33, 0x22, 0x11, 255]);
    }

    #[test]
    fn a_run_of_one_colour_fills_that_many_pixels() {
        let mut out = Vec::new();
        out.extend_from_slice(&SIGNATURE);
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&(54u32 + 1024).to_le_bytes());

        out.extend_from_slice(&40u32.to_le_bytes());
        out.extend_from_slice(&4i32.to_le_bytes());
        out.extend_from_slice(&1i32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&8u16.to_le_bytes());
        out.extend_from_slice(&1u32.to_le_bytes()); // Runs, eight bits.
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        // A palette of 256, of which two say anything.
        let mut palette = vec![0u8; 1024];
        palette[4..8].copy_from_slice(&[0x11, 0x22, 0x33, 0x00]);
        out.extend_from_slice(&palette);
        // Four pixels of colour one, then the end.
        out.extend_from_slice(&[0x04, 0x01, 0x00, 0x01]);

        let image = decode(&out).expect("a bitmap");
        for x in 0..4 {
            assert_eq!(pixel(&image, x, 0), [0x33, 0x22, 0x11, 255], "pixel {x}");
        }
    }

    #[test]
    fn what_a_run_never_reaches_is_left_transparent() {
        let mut out = Vec::new();
        out.extend_from_slice(&SIGNATURE);
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&(54u32 + 1024).to_le_bytes());

        out.extend_from_slice(&40u32.to_le_bytes());
        out.extend_from_slice(&4i32.to_le_bytes());
        out.extend_from_slice(&1i32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&8u16.to_le_bytes());
        out.extend_from_slice(&1u32.to_le_bytes());
        // The size of the pixels, the two resolutions, and the two colour
        // counts: five numbers, which is what makes the header forty bytes.
        for _ in 0..5 {
            out.extend_from_slice(&0u32.to_le_bytes());
        }
        let mut palette = vec![0u8; 1024];
        palette[4..8].copy_from_slice(&[0xFF, 0xFF, 0xFF, 0x00]);
        out.extend_from_slice(&palette);
        // Two pixels, then the end: the other two are never written.
        out.extend_from_slice(&[0x02, 0x01, 0x00, 0x01]);

        let image = decode(&out).expect("a bitmap");
        assert_eq!(pixel(&image, 1, 0)[3], 255, "what was written is there");
        assert_eq!(pixel(&image, 3, 0)[3], 0, "what was not is not");
    }

    #[test]
    fn channels_of_any_width_reach_the_top() {
        // The brightest a channel can be comes out as the brightest a byte can
        // be, whatever width it was written in.
        assert_eq!(scale(0x7C00, 0x7C00), 255);
        assert_eq!(scale(0x001F, 0x001F), 255);
        assert_eq!(scale(0x00FF, 0x00FF), 255);
        assert_eq!(scale(0x07E0, 0x07E0), 255, "six bits as well");
        assert_eq!(scale(0x8000, 0x8000), 255, "and one bit");
        assert_eq!(scale(0, 0x7C00), 0);
        // And the middle comes out near the middle.
        assert_eq!(scale(0x4000, 0x7C00), 132);
    }

    #[test]
    fn a_channel_is_widened_the_way_every_other_reader_widens_it() {
        // Six bits of green holding 50 is 203 and not 202: the top two bits are
        // repeated at the bottom rather than the value being divided out. One
        // off is one off, and it shows the moment two readings are compared.
        assert_eq!(scale(50 << 5, 0x07E0), 203);
        // Five bits holding 25 is 206 by the same rule.
        assert_eq!(scale(25, 0x001F), 206);
    }

    #[test]
    fn pixels_packed_several_to_a_byte_are_read_left_to_right() {
        let line = [0b1011_0001u8];
        assert_eq!(packed(&line, 0, 4), 0b1011);
        assert_eq!(packed(&line, 1, 4), 0b0001);
        assert_eq!(packed(&line, 0, 1), 1);
        assert_eq!(packed(&line, 1, 1), 0);
        assert_eq!(packed(&line, 2, 1), 1);
        assert_eq!(packed(&line, 3, 1), 1);
    }

    #[test]
    fn a_header_of_no_known_length_is_refused() {
        let mut data = vec![0u8; 40];
        data[0..2].copy_from_slice(&SIGNATURE);
        data[14..18].copy_from_slice(&20u32.to_le_bytes());
        assert!(decode(&data).is_err());
    }

    #[test]
    fn a_compression_this_decoder_does_not_read_says_so() {
        let body = [0u8; 8];
        assert_eq!(
            decode(&bitmap(2, 1, 24, 11, &body)),
            Err(Error::Unsupported("a compression this decoder does not read"))
        );
    }

    #[test]
    fn a_picture_of_nothing_is_refused_rather_than_allocated() {
        let body = [0u8; 4];
        assert!(decode(&bitmap(0, 1, 24, 0, &body)).is_err());
    }
}
