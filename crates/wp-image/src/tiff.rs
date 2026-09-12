//! Reading TIFF, to the 6.0 specification.
//!
//! # The shape of the format
//!
//! Eight bytes saying which way round the numbers are written and where the
//! directory is, and then a directory: a count, and that many twelve-byte
//! entries, each a tag, a type, how many values, and either the values
//! themselves or where to find them. Everything else in the file is reached
//! through that directory, and nothing is where it is because of what came
//! before it. A TIFF is a little filing system with a picture in it.
//!
//! That is also why the format is so wide. There is no one layout: the pixels
//! may be in strips or in tiles, one channel at a time or all together, at one
//! bit each or at sixteen, compressed five different ways or not at all, and
//! meaning grey, colour, ink, or numbers into a palette. A reader that assumed
//! any of it would be a reader that worked on the files it was written against
//! and no others.
//!
//! # Why the predictor is undone here
//!
//! Because it is not compression, though it sits beside it. A picture with a
//! predictor has had each sample replaced by its difference from the one to its
//! left before being compressed — which compresses far better and means nothing
//! until it is added back up. So it is undone on each row as the row comes out,
//! and the rest of this file never knows it happened.
//!
//! # What is not read
//!
//! The two fax codings, which are a coding of their own and belong to scanned
//! pages: see the roadmap's **D10**. Anything else this does not understand
//! says so rather than drawing a picture that is subtly wrong.

use crate::{check_size, Error, Image};

/// The four bytes a TIFF begins with, one way round or the other.
pub const LITTLE: [u8; 4] = [b'I', b'I', 42, 0];
pub const BIG: [u8; 4] = [b'M', b'M', 0, 42];

/// Which way round the numbers in this file are written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Order {
    little: bool,
}

impl Order {
    fn short(self, data: &[u8], at: usize) -> Result<u16, Error> {
        let bytes = data.get(at..at + 2).ok_or(Error::Truncated)?;
        Ok(if self.little {
            u16::from_le_bytes([bytes[0], bytes[1]])
        } else {
            u16::from_be_bytes([bytes[0], bytes[1]])
        })
    }

    fn long(self, data: &[u8], at: usize) -> Result<u32, Error> {
        let bytes = data.get(at..at + 4).ok_or(Error::Truncated)?;
        Ok(if self.little {
            u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
        } else {
            u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
        })
    }
}

/// One entry of the directory: a tag, and the values under it.
#[derive(Clone, Debug)]
struct Entry {
    kind: u16,
    count: u32,
    /// Where the values are, which for four bytes or fewer is the entry itself.
    at: usize,
}

/// How wide one value of a given type is.
fn width_of(kind: u16) -> usize {
    match kind {
        1 | 2 | 6 | 7 => 1,
        3 | 8 => 2,
        5 | 10 => 8,
        12 => 8,
        _ => 4,
    }
}

/// Everything the directory says about the picture.
#[derive(Clone, Debug, Default)]
struct Fields {
    width: usize,
    height: usize,
    /// How many bits each sample of a pixel takes.
    bits: Vec<u16>,
    samples: usize,
    photometric: u16,
    compression: u16,
    predictor: u16,
    /// One means every sample of a pixel together, two means one channel at a
    /// time in blocks of its own.
    planar: u16,
    rows_per_strip: usize,
    offsets: Vec<u32>,
    counts: Vec<u32>,
    tile_width: usize,
    tile_height: usize,
    /// The palette, as three runs of sixteen-bit values: every red, then every
    /// green, then every blue.
    palette: Vec<u16>,
    /// What the samples past the ones the colour needs are for.
    extra: Vec<u16>,
}

/// Decodes a TIFF into eight-bit RGBA.
///
/// The first picture in the file: a TIFF may hold several — the pages of a
/// scanned document, or a thumbnail beside the picture — and a document shows
/// one.
pub fn decode(data: &[u8]) -> Result<Image, Error> {
    let order = if data.starts_with(&LITTLE) {
        Order { little: true }
    } else if data.starts_with(&BIG) {
        Order { little: false }
    } else {
        return Err(Error::UnknownFormat);
    };

    let directory = order.long(data, 4)? as usize;
    let fields = read_fields(data, order, directory)?;
    check_size(fields.width, fields.height)?;

    let mut image = Image::empty(fields.width, fields.height);
    read_pixels(data, order, &fields, &mut image)?;
    Ok(image)
}

/// Reads the directory and everything it says.
fn read_fields(data: &[u8], order: Order, directory: usize) -> Result<Fields, Error> {
    let count = usize::from(order.short(data, directory)?);
    let mut entries: Vec<(u16, Entry)> = Vec::with_capacity(count);
    for index in 0..count {
        let at = directory + 2 + index * 12;
        let tag = order.short(data, at)?;
        let kind = order.short(data, at + 2)?;
        let values = order.long(data, at + 4)?;
        // Four bytes or fewer sit in the entry itself; anything longer is
        // somewhere else in the file and the entry says where.
        let size = width_of(kind).saturating_mul(values as usize);
        let where_they_are = if size <= 4 { at + 8 } else { order.long(data, at + 8)? as usize };
        entries.push((tag, Entry { kind, count: values, at: where_they_are }));
    }

    let numbers = |tag: u16| -> Vec<u32> {
        let Some((_, entry)) = entries.iter().find(|(known, _)| *known == tag) else {
            return Vec::new();
        };
        (0..entry.count as usize)
            .filter_map(|index| {
                let at = entry.at + index * width_of(entry.kind);
                match entry.kind {
                    1 | 2 | 6 | 7 => data.get(at).map(|byte| u32::from(*byte)),
                    3 | 8 => order.short(data, at).ok().map(u32::from),
                    _ => order.long(data, at).ok(),
                }
            })
            .collect()
    };
    let first =
        |tag: u16, fallback: u32| -> u32 { numbers(tag).first().copied().unwrap_or(fallback) };

    let mut fields = Fields {
        width: first(256, 0) as usize,
        height: first(257, 0) as usize,
        photometric: first(262, 1) as u16,
        compression: first(259, 1) as u16,
        predictor: first(317, 1) as u16,
        planar: first(284, 1) as u16,
        samples: first(277, 1) as usize,
        tile_width: first(322, 0) as usize,
        tile_height: first(323, 0) as usize,
        bits: numbers(258).iter().map(|value| *value as u16).collect(),
        palette: numbers(320).iter().map(|value| *value as u16).collect(),
        extra: numbers(338).iter().map(|value| *value as u16).collect(),
        ..Fields::default()
    };

    // Tiles and strips say the same things under different tags.
    if fields.tile_width > 0 {
        fields.offsets = numbers(324);
        fields.counts = numbers(325);
    } else {
        fields.offsets = numbers(273);
        fields.counts = numbers(279);
        // A file that does not say puts the whole picture in one strip.
        fields.rows_per_strip = match first(278, u32::MAX) {
            u32::MAX => fields.height,
            rows => rows as usize,
        };
        if fields.rows_per_strip == 0 {
            fields.rows_per_strip = fields.height.max(1);
        }
    }

    if fields.bits.is_empty() {
        fields.bits = vec![1; fields.samples.max(1)];
    }
    fields.samples = fields.samples.max(fields.bits.len());
    if fields.offsets.is_empty() {
        return Err(Error::Malformed("a picture that says nowhere its pixels are"));
    }
    Ok(fields)
}

/// Reads every strip or tile onto the canvas.
fn read_pixels(data: &[u8], order: Order, fields: &Fields, image: &mut Image) -> Result<(), Error> {
    // A block is a tile or a strip: the same thing at different sizes, and the
    // only difference between them is how many there are across.
    let (block_width, block_height) = if fields.tile_width > 0 {
        (fields.tile_width, fields.tile_height.max(1))
    } else {
        (fields.width, fields.rows_per_strip.max(1))
    };
    let across = fields.width.div_ceil(block_width.max(1));
    let down = fields.height.div_ceil(block_height);

    // With one channel to a block there are as many groups of blocks as there
    // are channels, the first group being the first channel throughout.
    let planar = fields.planar == 2;
    let planes = if planar { fields.samples.max(1) } else { 1 };
    let per_plane = across * down;

    let bits = fields.bits.first().copied().unwrap_or(8);
    let samples_in_block = if planar { 1 } else { fields.samples };
    let row_bytes = (block_width * samples_in_block * usize::from(bits)).div_ceil(8);

    // Every sample of the picture, eight bits each, in the order the pixels are
    // in. Gathered first and turned into colour afterwards, because what a
    // sample means depends on the others beside it.
    let mut gathered = vec![0u8; fields.width * fields.height * fields.samples];

    for plane in 0..planes {
        for index in 0..per_plane {
            let block = plane * per_plane + index;
            let Some(start) = fields.offsets.get(block).copied() else { continue };
            let length = fields.counts.get(block).copied().unwrap_or(0) as usize;
            let start = start as usize;
            let Some(written) = data.get(start..start + length) else { continue };

            let wanted = row_bytes * block_height;
            let mut body = expand(written, fields.compression, wanted)?;
            undo_predictor(&mut body, fields, block_width, samples_in_block, row_bytes);

            let left = (index % across) * block_width;
            let top = (index / across) * block_height;
            place(
                &body,
                fields,
                image.width,
                (left, top, block_width, block_height),
                (row_bytes, samples_in_block, plane),
                &mut gathered,
            );
        }
    }

    colour(&gathered, fields, image);
    let _ = order;
    Ok(())
}

/// Undoes whatever the pixels were compressed with.
fn expand(written: &[u8], compression: u16, wanted: usize) -> Result<Vec<u8>, Error> {
    match compression {
        1 => Ok(written.to_vec()),
        5 => unpack_lzw(written, wanted),
        8 | 32_946 => wp_deflate::inflate_zlib(written, wanted.max(1) * 2)
            .map_err(|_| Error::Malformed("a strip that will not decompress")),
        32_773 => Ok(unpack_runs(written, wanted)),
        2..=4 => Err(Error::Unsupported("one of the fax codings")),
        6 | 7 => Err(Error::Unsupported("a picture whose strips are JPEG")),
        _ => Err(Error::Unsupported("a compression this decoder does not read")),
    }
}

/// PackBits: a length byte and then either a run or a stretch of literals.
///
/// A byte under 128 means that many plus one literal bytes; over 128 means the
/// next byte repeated as many times; 128 exactly means nothing at all.
fn unpack_runs(written: &[u8], wanted: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(wanted);
    let mut at = 0usize;
    while at < written.len() && out.len() < wanted {
        let length = written[at] as i8;
        at += 1;
        if length >= 0 {
            let run = length as usize + 1;
            let Some(bytes) = written.get(at..at + run) else { break };
            out.extend_from_slice(bytes);
            at += run;
        } else if length != -128 {
            let run = (1 - i32::from(length)) as usize;
            let Some(byte) = written.get(at).copied() else { break };
            at += 1;
            out.extend(std::iter::repeat_n(byte, run));
        }
    }
    out.resize(wanted, 0);
    out
}

/// LZW, in the variant TIFF uses.
///
/// # How it differs from the one GIF uses
///
/// The codes are packed the other way round — most significant bit first — and
/// they grow a code early: the width goes up when one code is left rather than
/// when none are. That early change was a mistake in the first writers and is
/// now what every reader expects, so it is what this does.
fn unpack_lzw(written: &[u8], wanted: usize) -> Result<Vec<u8>, Error> {
    const CLEAR: u16 = 256;
    const END: u16 = 257;

    let mut previous = vec![0u16; 4096];
    let mut added = vec![0u8; 4096];
    for (index, entry) in added.iter_mut().enumerate().take(256) {
        *entry = index as u8;
    }

    let mut next = 258u16;
    let mut width = 9u8;
    let mut last: Option<u16> = None;
    let mut out = Vec::with_capacity(wanted);
    let mut held = 0u32;
    let mut count = 0u8;
    let mut at = 0usize;
    let mut run: Vec<u8> = Vec::new();

    while out.len() < wanted {
        while count < width {
            let Some(byte) = written.get(at).copied() else {
                out.resize(wanted, 0);
                return Ok(out);
            };
            at += 1;
            held = (held << 8) | u32::from(byte);
            count += 8;
        }
        let code = ((held >> (count - width)) & ((1 << width) - 1)) as u16;
        count -= width;

        if code == CLEAR {
            next = 258;
            width = 9;
            last = None;
            continue;
        }
        if code == END {
            break;
        }

        let known = code < next;
        if !known && last.is_none() {
            return Err(Error::Malformed("a code before anything it could extend"));
        }

        run.clear();
        let mut walk = if known { code } else { last.unwrap_or(0) };
        loop {
            run.push(added[usize::from(walk)]);
            if walk < CLEAR {
                break;
            }
            walk = previous[usize::from(walk)];
        }
        run.reverse();
        if !known {
            let front = *run.first().unwrap_or(&0);
            run.push(front);
        }
        out.extend_from_slice(&run);

        if let Some(last) = last {
            if next < 4096 {
                previous[usize::from(next)] = last;
                added[usize::from(next)] = *run.first().unwrap_or(&0);
                next += 1;
            }
        }
        last = Some(code);

        // One code early, which is the quirk this variant is known for.
        if next + 1 >= (1 << width) && width < 12 {
            width += 1;
        }
    }

    out.resize(wanted, 0);
    Ok(out)
}

/// Adds each sample back onto the one to its left.
///
/// The predictor is not compression: it is what makes a photograph compress, by
/// replacing every sample with how much it differs from its neighbour. Undoing
/// it is a running total along each row, one channel at a time — the channels
/// of a pixel are interleaved, so red follows red and not green.
fn undo_predictor(
    body: &mut [u8],
    fields: &Fields,
    block_width: usize,
    samples: usize,
    row_bytes: usize,
) {
    if fields.predictor != 2 {
        return;
    }
    let bits = fields.bits.first().copied().unwrap_or(8);
    // Only the two widths the predictor is defined for; at anything narrower
    // it would be adding up parts of bytes, which the format does not ask for.
    if bits != 8 && bits != 16 {
        return;
    }

    for row in body.chunks_mut(row_bytes) {
        if bits == 8 {
            for index in samples..block_width * samples {
                if index < row.len() {
                    row[index] = row[index].wrapping_add(row[index - samples]);
                }
            }
        } else {
            for index in samples..block_width * samples {
                let (here, there) = (index * 2, (index - samples) * 2);
                let Some(before) = row.get(there..there + 2) else { continue };
                let before = u16::from_be_bytes([before[0], before[1]]);
                let Some(now) = row.get(here..here + 2) else { continue };
                let sum = u16::from_be_bytes([now[0], now[1]]).wrapping_add(before);
                row[here..here + 2].copy_from_slice(&sum.to_be_bytes());
            }
        }
    }
}

/// Copies one block's samples into the picture's, eight bits each.
fn place(
    body: &[u8],
    fields: &Fields,
    width: usize,
    block: (usize, usize, usize, usize),
    layout: (usize, usize, usize),
    gathered: &mut [u8],
) {
    let (left, top, block_width, block_height) = block;
    let (row_bytes, samples_in_block, plane) = layout;
    let bits = usize::from(fields.bits.first().copied().unwrap_or(8));
    let most = ((1u32 << bits.min(16)) - 1) as f32;

    for row in 0..block_height {
        let y = top + row;
        if y >= fields.height {
            break;
        }
        let Some(line) = body.get(row * row_bytes..) else { break };
        for column in 0..block_width {
            let x = left + column;
            if x >= width {
                break;
            }
            for sample in 0..samples_in_block {
                let index = column * samples_in_block + sample;
                let value = match bits {
                    16 => {
                        let at = index * 2;
                        line.get(at).copied().unwrap_or(0)
                    }
                    8 => line.get(index).copied().unwrap_or(0),
                    _ => {
                        let taken = read_bits(line, index, bits);
                        // A narrow sample is stretched over the whole range, so
                        // one bit of black and white really is black and white
                        // — but a number into a palette is a number, and
                        // stretching it would look up the wrong colour.
                        if fields.photometric == 3 {
                            taken as u8
                        } else {
                            ((taken as f32 / most) * 255.0).round() as u8
                        }
                    }
                };
                let channel = if fields.planar == 2 { plane } else { sample };
                let place = (y * width + x) * fields.samples + channel;
                if let Some(slot) = gathered.get_mut(place) {
                    *slot = value;
                }
            }
        }
    }
}

/// One sample of a row written several to the byte.
fn read_bits(line: &[u8], index: usize, bits: usize) -> u32 {
    let start = index * bits;
    let mut value = 0u32;
    for step in 0..bits {
        let at = start + step;
        let byte = line.get(at / 8).copied().unwrap_or(0);
        let bit = (byte >> (7 - at % 8)) & 1;
        value = (value << 1) | u32::from(bit);
    }
    value
}

/// Turns the gathered samples into colour.
fn colour(gathered: &[u8], fields: &Fields, image: &mut Image) {
    let samples = fields.samples.max(1);
    // A sample past the ones the colour itself needs, which the file says is
    // transparency. Anything else it could be is not transparency, so the
    // picture is opaque.
    let alpha_at = {
        let needed = match fields.photometric {
            2 => 3,
            5 => 4,
            _ => 1,
        };
        let says = fields.extra.first().copied().unwrap_or(0);
        (samples > needed && matches!(says, 1 | 2)).then_some(needed)
    };

    for (pixel, place) in image.pixels.chunks_exact_mut(4).enumerate() {
        let at = pixel * samples;
        let sample = |index: usize| gathered.get(at + index).copied().unwrap_or(0);

        let (red, green, blue) = match fields.photometric {
            // Zero is white, which is how a scanned page is written.
            0 => {
                let grey = 255 - sample(0);
                (grey, grey, grey)
            }
            1 => {
                let grey = sample(0);
                (grey, grey, grey)
            }
            2 => (sample(0), sample(1), sample(2)),
            3 => {
                from_palette(&fields.palette, sample(0), fields.bits.first().copied().unwrap_or(8))
            }
            // Ink on paper, and written as ink here rather than inverted the
            // way a JPEG writes it.
            5 => {
                let black = u32::from(255 - sample(3));
                let ink = |value: u8| ((255 - u32::from(value)) * black / 255) as u8;
                (ink(sample(0)), ink(sample(1)), ink(sample(2)))
            }
            _ => {
                let grey = sample(0);
                (grey, grey, grey)
            }
        };

        place[0] = red;
        place[1] = green;
        place[2] = blue;
        place[3] = alpha_at.map_or(255, sample);
    }
}

/// A palette entry, brought down from sixteen bits to eight.
///
/// The three colours are three runs one after another rather than three bytes
/// together, which is the one thing about a TIFF palette that surprises
/// everybody who writes one of these.
fn from_palette(palette: &[u16], index: u8, bits: u16) -> (u8, u8, u8) {
    let entries = 1usize << bits.min(8);
    let at = usize::from(index);
    let take = |run: usize| -> u8 {
        let value = palette.get(run * entries + at).copied().unwrap_or(0);
        (value >> 8) as u8
    };
    (take(0), take(1), take(2))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a little-endian TIFF of one strip from a list of tags.
    ///
    /// Every value is short enough to sit in its own entry, which keeps the
    /// arithmetic here to the offsets of the pixels themselves.
    fn tiff(tags: &[(u16, u16, u32)], pixels: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&LITTLE);
        out.extend_from_slice(&8u32.to_le_bytes());

        let count = tags.len() as u16 + 1;
        // The directory, then the pixels after it.
        let pixels_at = 8 + 2 + usize::from(count) * 12 + 4;
        out.extend_from_slice(&count.to_le_bytes());

        let mut entries: Vec<(u16, u16, u32)> = tags.to_vec();
        entries.push((273, 4, pixels_at as u32)); // Where the strip is.
        entries.sort_by_key(|(tag, _, _)| *tag);

        for (tag, kind, value) in &entries {
            out.extend_from_slice(&tag.to_le_bytes());
            out.extend_from_slice(&kind.to_le_bytes());
            out.extend_from_slice(&1u32.to_le_bytes());
            match kind {
                3 => {
                    out.extend_from_slice(&(*value as u16).to_le_bytes());
                    out.extend_from_slice(&[0, 0]);
                }
                _ => out.extend_from_slice(&value.to_le_bytes()),
            }
        }
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(pixels);
        out
    }

    fn pixel(image: &Image, x: usize, y: usize) -> [u8; 4] {
        let at = (y * image.width + x) * 4;
        [image.pixels[at], image.pixels[at + 1], image.pixels[at + 2], image.pixels[at + 3]]
    }

    /// A two-by-one picture in twenty-four-bit colour.
    fn colour_tags(compression: u16, count: u32) -> Vec<(u16, u16, u32)> {
        vec![
            (256, 3, 2), // Two across.
            (257, 3, 1), // One down.
            (258, 3, 8), // Eight bits a sample.
            (259, 3, u32::from(compression)),
            (262, 3, 2),     // Red, green and blue.
            (277, 3, 3),     // Three samples.
            (279, 4, count), // How long the strip is.
        ]
    }

    #[test]
    fn anything_else_is_not_a_tiff() {
        assert_eq!(decode(b"not a tiff at all"), Err(Error::UnknownFormat));
    }

    #[test]
    fn both_ways_round_are_read() {
        // The same picture written the other way round: every number in the
        // file reverses, and the picture does not.
        let little = tiff(&colour_tags(1, 6), &[0xFF, 0x00, 0x00, 0x00, 0xFF, 0x00]);
        let one = decode(&little).expect("a picture");

        let mut big = Vec::new();
        big.extend_from_slice(&BIG);
        big.extend_from_slice(&8u32.to_be_bytes());
        let tags = colour_tags(1, 6);
        let count = tags.len() as u16 + 1;
        let pixels_at = 8 + 2 + usize::from(count) * 12 + 4;
        big.extend_from_slice(&count.to_be_bytes());
        let mut entries = tags;
        entries.push((273, 4, pixels_at as u32));
        entries.sort_by_key(|(tag, _, _)| *tag);
        for (tag, kind, value) in &entries {
            big.extend_from_slice(&tag.to_be_bytes());
            big.extend_from_slice(&kind.to_be_bytes());
            big.extend_from_slice(&1u32.to_be_bytes());
            match kind {
                // A value shorter than the field sits at the front of it, which
                // is so whichever way round the numbers are written.
                3 => {
                    big.extend_from_slice(&(*value as u16).to_be_bytes());
                    big.extend_from_slice(&[0, 0]);
                }
                _ => big.extend_from_slice(&value.to_be_bytes()),
            }
        }
        big.extend_from_slice(&0u32.to_be_bytes());
        big.extend_from_slice(&[0xFF, 0x00, 0x00, 0x00, 0xFF, 0x00]);

        let other = decode(&big).expect("a picture");
        assert_eq!(one.pixels, other.pixels);
        assert_eq!(pixel(&one, 0, 0), [0xFF, 0, 0, 255]);
        assert_eq!(pixel(&one, 1, 0), [0, 0xFF, 0, 255]);
    }

    #[test]
    fn a_picture_of_runs_is_unpacked() {
        // PackBits: a run of three, then two literals.
        let written = [0xFEu8, 0x11, 0x01, 0x22, 0x33];
        assert_eq!(unpack_runs(&written, 5), vec![0x11, 0x11, 0x11, 0x22, 0x33]);
    }

    #[test]
    fn a_run_of_nothing_is_stepped_over() {
        // The one length that means neither a run nor literals.
        let written = [0x80u8, 0x00, 0x44];
        assert_eq!(unpack_runs(&written, 1), vec![0x44]);
    }

    #[test]
    fn zero_is_white_when_the_file_says_so() {
        let mut tags = colour_tags(1, 2);
        tags.retain(|(tag, _, _)| !matches!(tag, 262 | 277));
        tags.push((262, 3, 0)); // Zero is white.
        tags.push((277, 3, 1));
        let data = tiff(&tags, &[0x00, 0xFF]);
        let image = decode(&data).expect("a picture");
        assert_eq!(pixel(&image, 0, 0), [255, 255, 255, 255]);
        assert_eq!(pixel(&image, 1, 0), [0, 0, 0, 255]);
    }

    #[test]
    fn one_bit_is_stretched_to_black_and_white() {
        let mut tags = colour_tags(1, 1);
        tags.retain(|(tag, _, _)| !matches!(tag, 258 | 262 | 277));
        tags.push((258, 3, 1)); // One bit a sample.
        tags.push((262, 3, 1)); // Zero is black.
        tags.push((277, 3, 1));
        let data = tiff(&tags, &[0b0100_0000]);
        let image = decode(&data).expect("a picture");
        assert_eq!(pixel(&image, 0, 0), [0, 0, 0, 255]);
        assert_eq!(pixel(&image, 1, 0), [255, 255, 255, 255], "one bit should reach the top");
    }

    #[test]
    fn the_predictor_is_a_running_total_along_each_row() {
        let mut body = [10u8, 20, 30, 1, 1, 1];
        let fields = Fields { predictor: 2, bits: vec![8], ..Fields::default() };
        undo_predictor(&mut body, &fields, 2, 3, 6);
        assert_eq!(body, [10, 20, 30, 11, 21, 31], "each channel follows its own");
    }

    #[test]
    fn the_predictor_is_left_alone_when_it_is_not_asked_for() {
        let mut body = [10u8, 1];
        let fields = Fields { predictor: 1, bits: vec![8], ..Fields::default() };
        undo_predictor(&mut body, &fields, 2, 1, 2);
        assert_eq!(body, [10, 1]);
    }

    #[test]
    fn a_palette_is_three_runs_and_not_three_bytes() {
        // Two entries: the reds, then the greens, then the blues.
        let palette = vec![0xFFFF, 0x0000, 0x0000, 0xFFFF, 0x0000, 0x0000];
        assert_eq!(from_palette(&palette, 0, 1), (0xFF, 0x00, 0x00));
        assert_eq!(from_palette(&palette, 1, 1), (0x00, 0xFF, 0x00));
    }

    #[test]
    fn a_strip_written_in_deflate_is_read() {
        // Deflate itself is proved against an outside encoder by the PNG
        // fixtures; what this proves is that a TIFF reaches it, under either
        // of the two numbers the format has given it over the years.
        let pixels = [0xFFu8, 0x00, 0x00, 0x00, 0xFF, 0x00];
        let squeezed = wp_deflate::compress_zlib(&pixels);
        for number in [8u16, 32_946] {
            let mut tags = colour_tags(number, squeezed.len() as u32);
            tags.retain(|(tag, _, _)| *tag != 279);
            tags.push((279, 4, squeezed.len() as u32));
            let data = tiff(&tags, &squeezed);
            let image = decode(&data).expect("a picture");
            assert_eq!(pixel(&image, 0, 0), [0xFF, 0, 0, 255], "under {number}");
            assert_eq!(pixel(&image, 1, 0), [0, 0xFF, 0, 255]);
        }
    }

    #[test]
    fn a_compression_this_decoder_does_not_read_says_so() {
        assert_eq!(
            expand(&[], 999, 1),
            Err(Error::Unsupported("a compression this decoder does not read"))
        );
    }

    #[test]
    fn a_fax_coding_says_so_rather_than_drawing_nonsense() {
        assert_eq!(expand(&[], 4, 1), Err(Error::Unsupported("one of the fax codings")));
    }

    #[test]
    fn a_picture_that_says_nowhere_its_pixels_are_is_refused() {
        let data = tiff(&[(256, 3, 2), (257, 3, 1)], &[]);
        // The strip offset is in every picture the helper builds, so take it
        // out by hand: a directory without one says nothing about its pixels.
        let mut without = data.clone();
        let count = u16::from_le_bytes([without[8], without[9]]);
        without[8..10].copy_from_slice(&(count - 1).to_le_bytes());
        assert!(decode(&without).is_err());
    }
}
