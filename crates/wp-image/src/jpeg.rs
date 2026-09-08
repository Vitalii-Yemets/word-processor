//! Reading baseline JPEG, to ITU-T T.81.
//!
//! # The shape of the format
//!
//! A sequence of marker segments. `DQT` carries quantisation tables, `DHT`
//! Huffman tables, `SOF0` the size and how the components are sampled, and
//! `SOS` introduces the entropy-coded data — one continuous stream of bits with
//! no lengths in it, which is why everything else has to be read first.
//!
//! # How a picture is put back together
//!
//! The stream holds blocks of eight by eight frequency coefficients. Each is
//! multiplied back up by its quantisation table, put back in raster order from
//! the zig-zag it was written in, and turned into samples by an inverse cosine
//! transform. Colour is stored as brightness and two colour differences, with
//! the colour usually sampled at half resolution — the eye notices detail in
//! brightness far more than in colour, and the format is built around that.
//!
//! # What is not read
//!
//! Progressive JPEG, which spreads the coefficients over several scans, and
//! arithmetic coding, which almost nothing produces. Both say so rather than
//! producing a picture that is subtly wrong.

use crate::{check_size, Error, Image};

/// Where each coefficient of a block sits, unzig-zagged.
///
/// The coefficients are written in a diagonal order that puts the coarse detail
/// first, so a block has to be put back into raster order before anything can
/// be done with it.
const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20,
    13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59,
    52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

/// One Huffman table, in the form the specification's decoder wants.
#[derive(Clone, Debug, Default)]
struct HuffmanTable {
    /// The smallest and largest code of each length, and where its values start.
    min_code: [i32; 17],
    max_code: [i32; 17],
    value_index: [usize; 17],
    values: Vec<u8>,
}

impl HuffmanTable {
    /// Builds a table from the counts and values a `DHT` segment carries.
    ///
    /// The codes themselves are never written down: they are canonical, so
    /// knowing how many codes there are of each length is enough to work out
    /// what every one of them is.
    fn build(counts: &[u8; 16], values: Vec<u8>) -> Self {
        let mut table = Self { values, ..Self::default() };
        let mut code = 0i32;
        let mut index = 0usize;

        for length in 1..=16usize {
            table.value_index[length] = index;
            table.min_code[length] = code;
            code += i32::from(counts[length - 1]);
            index += usize::from(counts[length - 1]);
            // A length with no codes is marked as matching nothing.
            table.max_code[length] = if counts[length - 1] == 0 { -1 } else { code - 1 };
            code <<= 1;
        }

        table
    }

    /// Reads one value, a bit at a time until a code is complete.
    fn decode(&self, bits: &mut BitReader<'_>) -> Result<u8, Error> {
        let mut code = 0i32;
        for length in 1..=16usize {
            code = (code << 1) | i32::from(bits.bit()?);
            if self.max_code[length] >= code && code >= self.min_code[length] {
                let offset = self.value_index[length] + (code - self.min_code[length]) as usize;
                return self
                    .values
                    .get(offset)
                    .copied()
                    .ok_or(Error::Malformed("a Huffman code with no value"));
            }
        }
        Err(Error::Malformed("a Huffman code longer than the format allows"))
    }
}

/// Reads the entropy-coded data one bit at a time.
///
/// A byte of 0xFF inside the stream is written as 0xFF 0x00, because 0xFF
/// begins a marker; the stuffed zero is stepped over here so that nothing above
/// has to know about it.
struct BitReader<'a> {
    data: &'a [u8],
    position: usize,
    current: u8,
    remaining: u8,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, position: 0, current: 0, remaining: 0 }
    }

    fn bit(&mut self) -> Result<u8, Error> {
        if self.remaining == 0 {
            let Some(byte) = self.data.get(self.position).copied() else {
                return Err(Error::Truncated);
            };
            self.position += 1;

            if byte == 0xFF {
                match self.data.get(self.position).copied() {
                    // The stuffed zero that makes an 0xFF an ordinary byte.
                    Some(0x00) => self.position += 1,
                    // Anything else is a marker: the scan has ended.
                    Some(_) | None => return Err(Error::Truncated),
                }
            }

            self.current = byte;
            self.remaining = 8;
        }

        self.remaining -= 1;
        Ok((self.current >> self.remaining) & 1)
    }

    /// Reads a number written as a given count of bits.
    fn receive(&mut self, length: u8) -> Result<i32, Error> {
        let mut value = 0i32;
        for _ in 0..length {
            value = (value << 1) | i32::from(self.bit()?);
        }
        Ok(value)
    }

    /// Throws away the rest of the current byte, as a restart marker requires.
    fn align(&mut self) {
        self.remaining = 0;
    }

    /// Steps over a restart marker, which is byte-aligned in the stream.
    fn skip_restart(&mut self) {
        self.align();
        while self.position + 1 < self.data.len() {
            if self.data[self.position] == 0xFF
                && (0xD0..=0xD7).contains(&self.data[self.position + 1])
            {
                self.position += 2;
                return;
            }
            self.position += 1;
        }
        self.position = self.data.len();
    }
}

/// Turns a value written in a given number of bits into a signed coefficient.
///
/// The format writes magnitudes: the top half of the range is positive and the
/// bottom half negative, which is what this undoes.
fn extend(value: i32, length: u8) -> i32 {
    if length == 0 {
        return 0;
    }
    if value < (1 << (length - 1)) {
        value - (1 << length) + 1
    } else {
        value
    }
}

/// One colour component of the picture.
#[derive(Clone, Debug)]
struct Component {
    id: u8,
    /// How many blocks of this component there are per group, across and down.
    horizontal: usize,
    vertical: usize,
    quantisation: usize,
    dc_table: usize,
    ac_table: usize,
    /// The decoded samples, at this component's own resolution.
    samples: Vec<u8>,
    width: usize,
    height: usize,
}

/// Decodes a baseline JPEG into eight-bit RGBA.
pub fn decode(data: &[u8]) -> Result<Image, Error> {
    if !data.starts_with(&[0xFF, 0xD8]) {
        return Err(Error::UnknownFormat);
    }

    let mut quantisation = [[1u16; 64]; 4];
    let mut dc_tables: Vec<HuffmanTable> = vec![HuffmanTable::default(); 4];
    let mut ac_tables: Vec<HuffmanTable> = vec![HuffmanTable::default(); 4];
    let mut components: Vec<Component> = Vec::new();
    let mut width = 0usize;
    let mut height = 0usize;
    let mut restart_interval = 0usize;

    let mut offset = 2usize;
    loop {
        // Markers are 0xFF followed by a code; fill bytes of 0xFF may precede.
        while data.get(offset).copied() == Some(0xFF) && data.get(offset + 1).copied() == Some(0xFF)
        {
            offset += 1;
        }
        let Some(&0xFF) = data.get(offset) else {
            return Err(Error::Malformed("a segment that does not start with a marker"));
        };
        let Some(marker) = data.get(offset + 1).copied() else {
            return Err(Error::Truncated);
        };
        offset += 2;

        match marker {
            // Start of frame, baseline.
            0xC0 | 0xC1 => {
                let body = segment(data, &mut offset)?;
                if body.len() < 6 {
                    return Err(Error::Truncated);
                }
                height = usize::from(u16::from_be_bytes([body[1], body[2]]));
                width = usize::from(u16::from_be_bytes([body[3], body[4]]));
                check_size(width, height)?;

                let count = usize::from(body[5]);
                if !matches!(count, 1 | 3) {
                    return Err(Error::Unsupported("a colour model other than grey or YCbCr"));
                }
                components.clear();
                for index in 0..count {
                    let at = 6 + index * 3;
                    let entry =
                        body.get(at..at + 3).ok_or(Error::Malformed("a short frame header"))?;
                    components.push(Component {
                        id: entry[0],
                        horizontal: usize::from(entry[1] >> 4).max(1),
                        vertical: usize::from(entry[1] & 0x0F).max(1),
                        quantisation: usize::from(entry[2]).min(3),
                        dc_table: 0,
                        ac_table: 0,
                        samples: Vec::new(),
                        width: 0,
                        height: 0,
                    });
                }
            }
            // Progressive, and the other frame types nothing here reads.
            0xC2 => return Err(Error::Unsupported("a progressive layout")),
            0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF => {
                return Err(Error::Unsupported("a coding this decoder does not read"));
            }
            // Huffman tables.
            0xC4 => {
                let body = segment(data, &mut offset)?;
                read_huffman_tables(body, &mut dc_tables, &mut ac_tables)?;
            }
            // Quantisation tables.
            0xDB => {
                let body = segment(data, &mut offset)?;
                read_quantisation_tables(body, &mut quantisation)?;
            }
            // Restart interval.
            0xDD => {
                let body = segment(data, &mut offset)?;
                if body.len() >= 2 {
                    restart_interval = usize::from(u16::from_be_bytes([body[0], body[1]]));
                }
            }
            // Start of scan: everything after it is entropy-coded data.
            0xDA => {
                let body = segment(data, &mut offset)?;
                read_scan_header(body, &mut components)?;
                if components.is_empty() || width == 0 || height == 0 {
                    return Err(Error::Malformed("a scan before the frame it belongs to"));
                }
                decode_scan(
                    &data[offset..],
                    &mut components,
                    &quantisation,
                    &dc_tables,
                    &ac_tables,
                    width,
                    height,
                    restart_interval,
                )?;
                return Ok(to_rgba(&components, width, height));
            }
            0xD9 => return Err(Error::Malformed("a picture that ends before its pixels")),
            // Standalone markers carry no length.
            0x01 | 0xD0..=0xD7 => {}
            // Everything else is a segment to step over: thumbnails, comments,
            // colour profiles, the maker's notes.
            _ => {
                segment(data, &mut offset)?;
            }
        }
    }
}

/// Reads a segment's body and moves past it.
fn segment<'a>(data: &'a [u8], offset: &mut usize) -> Result<&'a [u8], Error> {
    let bytes = data.get(*offset..*offset + 2).ok_or(Error::Truncated)?;
    let length = usize::from(u16::from_be_bytes([bytes[0], bytes[1]]));
    if length < 2 {
        return Err(Error::Malformed("a segment shorter than its own length"));
    }
    let body = data.get(*offset + 2..*offset + length).ok_or(Error::Truncated)?;
    *offset += length;
    Ok(body)
}

fn read_quantisation_tables(body: &[u8], tables: &mut [[u16; 64]; 4]) -> Result<(), Error> {
    let mut at = 0usize;
    while at < body.len() {
        let descriptor = body[at];
        let precision = descriptor >> 4;
        let index = usize::from(descriptor & 0x0F);
        at += 1;
        if index >= tables.len() {
            return Err(Error::Malformed("a quantisation table with no place to go"));
        }

        for entry in 0..64usize {
            let value = if precision == 0 {
                let byte = *body.get(at).ok_or(Error::Truncated)?;
                at += 1;
                u16::from(byte)
            } else {
                let bytes = body.get(at..at + 2).ok_or(Error::Truncated)?;
                at += 2;
                u16::from_be_bytes([bytes[0], bytes[1]])
            };
            // Stored in the same zig-zag order as the coefficients.
            tables[index][ZIGZAG[entry]] = value;
        }
    }
    Ok(())
}

fn read_huffman_tables(
    body: &[u8],
    dc: &mut [HuffmanTable],
    ac: &mut [HuffmanTable],
) -> Result<(), Error> {
    let mut at = 0usize;
    while at < body.len() {
        let descriptor = body[at];
        let is_ac = descriptor >> 4 == 1;
        let index = usize::from(descriptor & 0x0F);
        at += 1;
        if index >= 4 {
            return Err(Error::Malformed("a Huffman table with no place to go"));
        }

        let counts_slice = body.get(at..at + 16).ok_or(Error::Truncated)?;
        let mut counts = [0u8; 16];
        counts.copy_from_slice(counts_slice);
        at += 16;

        let total: usize = counts.iter().map(|count| usize::from(*count)).sum();
        let values = body.get(at..at + total).ok_or(Error::Truncated)?.to_vec();
        at += total;

        let table = HuffmanTable::build(&counts, values);
        if is_ac {
            ac[index] = table;
        } else {
            dc[index] = table;
        }
    }
    Ok(())
}

fn read_scan_header(body: &[u8], components: &mut [Component]) -> Result<(), Error> {
    let count = usize::from(*body.first().ok_or(Error::Truncated)?);
    for index in 0..count {
        let at = 1 + index * 2;
        let entry = body.get(at..at + 2).ok_or(Error::Truncated)?;
        let Some(component) = components.iter_mut().find(|c| c.id == entry[0]) else {
            continue;
        };
        component.dc_table = usize::from(entry[1] >> 4).min(3);
        component.ac_table = usize::from(entry[1] & 0x0F).min(3);
    }
    Ok(())
}

/// Decodes the entropy-coded data into each component's samples.
#[allow(clippy::too_many_arguments)]
fn decode_scan(
    data: &[u8],
    components: &mut [Component],
    quantisation: &[[u16; 64]; 4],
    dc_tables: &[HuffmanTable],
    ac_tables: &[HuffmanTable],
    width: usize,
    height: usize,
    restart_interval: usize,
) -> Result<(), Error> {
    let max_horizontal = components.iter().map(|c| c.horizontal).max().unwrap_or(1);
    let max_vertical = components.iter().map(|c| c.vertical).max().unwrap_or(1);

    // A group of blocks covering one patch of the picture: the unit the stream
    // is written in, so the picture is decoded a group at a time and not a
    // component at a time.
    let group_width = 8 * max_horizontal;
    let group_height = 8 * max_vertical;
    let across = width.div_ceil(group_width);
    let down = height.div_ceil(group_height);

    for component in components.iter_mut() {
        component.width = across * component.horizontal * 8;
        component.height = down * component.vertical * 8;
        check_size(component.width, component.height)?;
        component.samples = vec![128; component.width * component.height];
    }

    let mut bits = BitReader::new(data);
    let mut predictions = vec![0i32; components.len()];
    let mut block = [0i32; 64];
    let mut samples = [0u8; 64];
    let mut since_restart = 0usize;

    for group_y in 0..down {
        for group_x in 0..across {
            if restart_interval > 0 && since_restart == restart_interval {
                bits.skip_restart();
                predictions.iter_mut().for_each(|value| *value = 0);
                since_restart = 0;
            }
            since_restart += 1;

            for (index, component) in components.iter_mut().enumerate() {
                for row in 0..component.vertical {
                    for column in 0..component.horizontal {
                        decode_block(
                            &mut bits,
                            &dc_tables[component.dc_table],
                            &ac_tables[component.ac_table],
                            &quantisation[component.quantisation],
                            &mut predictions[index],
                            &mut block,
                        )?;
                        inverse_transform(&block, &mut samples);

                        let left = (group_x * component.horizontal + column) * 8;
                        let top = (group_y * component.vertical + row) * 8;
                        for y in 0..8 {
                            let destination = (top + y) * component.width + left;
                            let Some(slice) =
                                component.samples.get_mut(destination..destination + 8)
                            else {
                                continue;
                            };
                            slice.copy_from_slice(&samples[y * 8..y * 8 + 8]);
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

/// Reads one block of coefficients and multiplies them back up.
fn decode_block(
    bits: &mut BitReader<'_>,
    dc: &HuffmanTable,
    ac: &HuffmanTable,
    quantisation: &[u16; 64],
    prediction: &mut i32,
    block: &mut [i32; 64],
) -> Result<(), Error> {
    block.fill(0);

    // The first coefficient is written as a difference from the block before,
    // because neighbouring blocks of a photograph are rarely far apart.
    let length = dc.decode(bits)?;
    let difference = extend(bits.receive(length)?, length);
    *prediction += difference;
    block[0] = *prediction * i32::from(quantisation[0]);

    // The rest are written as a run of zeros and then a value, because most of
    // them are zero.
    let mut index = 1usize;
    while index < 64 {
        let symbol = ac.decode(bits)?;
        let run = usize::from(symbol >> 4);
        let size = symbol & 0x0F;

        if size == 0 {
            // Sixteen zeros, or the end of the block.
            if run == 15 {
                index += 16;
                continue;
            }
            break;
        }

        index += run;
        if index >= 64 {
            break;
        }
        let value = extend(bits.receive(size)?, size);
        let position = ZIGZAG[index];
        block[position] = value * i32::from(quantisation[position]);
        index += 1;
    }

    Ok(())
}

/// The cosines the transform is built from, worked out once.
fn cosine_table() -> &'static [[f32; 8]; 8] {
    use std::sync::OnceLock;
    static TABLE: OnceLock<[[f32; 8]; 8]> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table = [[0.0f32; 8]; 8];
        for (x, row) in table.iter_mut().enumerate() {
            for (u, entry) in row.iter_mut().enumerate() {
                let scale = if u == 0 { (0.5f32).sqrt() } else { 1.0 };
                *entry = scale
                    * ((2.0 * x as f32 + 1.0) * u as f32 * core::f32::consts::PI / 16.0).cos();
            }
        }
        table
    })
}

/// Turns a block of frequency coefficients back into samples.
///
/// Done as two passes of a one-dimensional transform rather than one
/// two-dimensional one: the transform separates, and eight rows followed by
/// eight columns is a great deal less arithmetic than sixty-four sums of
/// sixty-four terms.
fn inverse_transform(block: &[i32; 64], out: &mut [u8; 64]) {
    let cosines = cosine_table();
    let mut intermediate = [0.0f32; 64];

    for row in 0..8 {
        for x in 0..8 {
            let mut total = 0.0f32;
            for u in 0..8 {
                total += cosines[x][u] * block[row * 8 + u] as f32;
            }
            intermediate[row * 8 + x] = total * 0.5;
        }
    }

    for column in 0..8 {
        for y in 0..8 {
            let mut total = 0.0f32;
            for v in 0..8 {
                total += cosines[y][v] * intermediate[v * 8 + column];
            }
            // Samples are stored with the midpoint at zero, so the level is
            // shifted back up before it becomes a byte.
            let value = total * 0.5 + 128.0;
            out[y * 8 + column] = value.clamp(0.0, 255.0) as u8;
        }
    }
}

/// Turns the decoded components into RGBA at the picture's own size.
fn to_rgba(components: &[Component], width: usize, height: usize) -> Image {
    let mut pixels = Vec::with_capacity(width * height * 4);
    let max_horizontal = components.iter().map(|c| c.horizontal).max().unwrap_or(1);
    let max_vertical = components.iter().map(|c| c.vertical).max().unwrap_or(1);

    // A component sampled at half resolution covers two pixels with one sample,
    // so its samples are spread back out as the picture is assembled.
    let sample = |component: &Component, x: usize, y: usize| -> f32 {
        let sx = x * component.horizontal / max_horizontal;
        let sy = y * component.vertical / max_vertical;
        let at = sy.min(component.height.saturating_sub(1)) * component.width
            + sx.min(component.width.saturating_sub(1));
        f32::from(component.samples.get(at).copied().unwrap_or(128))
    };

    for y in 0..height {
        for x in 0..width {
            let (red, green, blue) = if components.len() == 1 {
                let grey = sample(&components[0], x, y);
                (grey, grey, grey)
            } else {
                // Brightness and two colour differences, as the format stores
                // colour.
                let luma = sample(&components[0], x, y);
                let blue_difference = sample(&components[1], x, y) - 128.0;
                let red_difference = sample(&components[2], x, y) - 128.0;
                (
                    luma + 1.402 * red_difference,
                    luma - 0.344_136 * blue_difference - 0.714_136 * red_difference,
                    luma + 1.772 * blue_difference,
                )
            };

            pixels.push(red.clamp(0.0, 255.0) as u8);
            pixels.push(green.clamp(0.0, 255.0) as u8);
            pixels.push(blue.clamp(0.0, 255.0) as u8);
            // A JPEG has no transparency at all.
            pixels.push(255);
        }
    }

    Image { width, height, pixels }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anything_else_is_not_a_jpeg() {
        assert_eq!(decode(b"not a jpeg at all"), Err(Error::UnknownFormat));
    }

    #[test]
    fn a_progressive_picture_says_so_rather_than_drawing_nonsense() {
        // Start of image, then a progressive frame header.
        let data = [0xFF, 0xD8, 0xFF, 0xC2, 0x00, 0x02];
        assert_eq!(decode(&data), Err(Error::Unsupported("a progressive layout")));
    }

    #[test]
    fn a_truncated_file_is_refused() {
        let data = [0xFF, 0xD8, 0xFF, 0xC0, 0x00];
        assert!(decode(&data).is_err());
    }

    #[test]
    fn magnitudes_become_signed_coefficients() {
        // The top half of each range is positive, the bottom half negative.
        assert_eq!(extend(0, 0), 0);
        assert_eq!(extend(1, 1), 1);
        assert_eq!(extend(0, 1), -1);
        assert_eq!(extend(3, 2), 3);
        assert_eq!(extend(1, 2), -2);
        assert_eq!(extend(0, 2), -3);
    }

    #[test]
    fn the_zigzag_covers_every_place_exactly_once() {
        let mut seen = [false; 64];
        for position in ZIGZAG {
            assert!(!seen[position], "position {position} appears twice");
            seen[position] = true;
        }
        assert!(seen.iter().all(|hit| *hit));
    }

    #[test]
    fn a_flat_block_transforms_to_a_flat_patch() {
        // Only the first coefficient set: the block has no detail, so every
        // sample of it is the same.
        let mut block = [0i32; 64];
        block[0] = 8 * 16;
        let mut out = [0u8; 64];
        inverse_transform(&block, &mut out);

        let first = out[0];
        assert!(out.iter().all(|value| *value == first), "the patch should be flat");
        assert!(first > 128, "a positive coefficient should be brighter than the midpoint");
    }

    #[test]
    fn a_block_of_nothing_is_the_midpoint_grey() {
        let block = [0i32; 64];
        let mut out = [0u8; 64];
        inverse_transform(&block, &mut out);
        assert!(out.iter().all(|value| *value == 128));
    }

    #[test]
    fn a_canonical_huffman_table_decodes_its_own_codes() {
        // Two codes of length two: 00 and 01.
        let mut counts = [0u8; 16];
        counts[1] = 2;
        let table = HuffmanTable::build(&counts, vec![0xAA, 0xBB]);

        // 00 then 01, packed into one byte with the rest unused.
        let data = [0b0001_0000u8];
        let mut bits = BitReader::new(&data);
        assert_eq!(table.decode(&mut bits).unwrap(), 0xAA);
        assert_eq!(table.decode(&mut bits).unwrap(), 0xBB);
    }

    #[test]
    fn a_stuffed_byte_is_stepped_over() {
        // 0xFF inside the data is written as 0xFF 0x00.
        let data = [0xFF, 0x00, 0b1000_0000];
        let mut bits = BitReader::new(&data);
        for _ in 0..8 {
            assert_eq!(bits.bit().unwrap(), 1, "the 0xFF byte itself");
        }
        assert_eq!(bits.bit().unwrap(), 1, "and then the byte after the stuffing");
    }

    #[test]
    fn a_real_marker_ends_the_scan() {
        // 0xFF followed by anything but zero is a marker, not data.
        let data = [0xFF, 0xD9];
        let mut bits = BitReader::new(&data);
        assert_eq!(bits.bit(), Err(Error::Truncated));
    }

    #[test]
    fn bits_are_read_from_the_top_of_each_byte_down() {
        let data = [0b1010_0000u8];
        let mut bits = BitReader::new(&data);
        assert_eq!(bits.receive(4).unwrap(), 0b1010);
    }
}
