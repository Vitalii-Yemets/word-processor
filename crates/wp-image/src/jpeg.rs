//! Reading JPEG, to ITU-T T.81.
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
//! # Progressive
//!
//! The same coefficients, spread over several scans instead of written in one.
//! The first scans carry the top bits of the low frequencies, and each scan
//! after them adds either a band of higher frequencies or one more bit of what
//! is already there. So nothing can be transformed until every scan has been
//! read: the coefficients are gathered whole and turned into samples at the
//! end, which is why both kinds go the same way here and differ only in how
//! they fill the coefficients in.
//!
//! # Colour
//!
//! Almost always brightness and two colour differences. Three components that
//! Adobe's marker calls untransformed are red, green and blue outright; four
//! are ink — cyan, magenta, yellow and black — and Adobe writes those inverted,
//! which is why a scanned page opened by a reader that does not know it comes
//! out looking like a photographic negative.
//!
//! # What is not read
//!
//! Arithmetic coding, which almost nothing produces, and the lossless and
//! hierarchical modes, which nothing does. All say so rather than producing a
//! picture that is subtly wrong.

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
    /// How many blocks of this component the picture holds, counting the ones
    /// that hang over its edge: the coefficients are stored for all of them.
    blocks_wide: usize,
    blocks_high: usize,
    /// And how many a scan of this component alone covers, which is fewer —
    /// such a scan is written in this component's own blocks rather than in
    /// groups, and the blocks past the edge of the picture are not among them.
    scan_wide: usize,
    scan_high: usize,
    /// Every block's sixty-four coefficients, in the zig-zag order they are
    /// written in and still divided down by the quantisation table.
    ///
    /// Kept rather than transformed as they are read, because a progressive
    /// picture states them a few bits at a time and nothing can be transformed
    /// until the last scan has been read.
    coefficients: Vec<i32>,
    /// The decoded samples, at this component's own resolution.
    samples: Vec<u8>,
    width: usize,
    height: usize,
}

/// One scan, from the header that introduces it.
#[derive(Clone, Debug, Default)]
struct Scan {
    /// Which components it carries, as places in the frame's list.
    parts: Vec<usize>,
    /// The band of coefficients it carries, in zig-zag order. A baseline scan
    /// carries all of them.
    start: usize,
    end: usize,
    /// Which bit of each coefficient: the one already sent, and the one being
    /// sent now. Both are zero in a baseline scan.
    high: u8,
    low: u8,
}

/// How the components make a colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Colour {
    Grey,
    YCbCr,
    /// Red, green and blue outright, which Adobe's marker can say.
    Rgb,
    /// Cyan, magenta, yellow and black — and written inverted, as Adobe writes
    /// them.
    Cmyk,
    /// The same four, with the first three held as brightness and colour
    /// differences the way an ordinary photograph is.
    Ycck,
}

/// Decodes a JPEG into eight-bit RGBA, baseline or progressive.
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
    let mut progressive = false;
    // What Adobe's marker said, if the file carries one. Nothing else says how
    // four components are to be read.
    let mut adobe: Option<u8> = None;

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
            // Start of frame: baseline, extended sequential, or progressive.
            0xC0..=0xC2 => {
                progressive = marker == 0xC2;
                let body = segment(data, &mut offset)?;
                if body.len() < 6 {
                    return Err(Error::Truncated);
                }
                height = usize::from(u16::from_be_bytes([body[1], body[2]]));
                width = usize::from(u16::from_be_bytes([body[3], body[4]]));
                check_size(width, height)?;

                let count = usize::from(body[5]);
                if !matches!(count, 1 | 3 | 4) {
                    return Err(Error::Unsupported("a picture of more colours than any exist in"));
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
                        blocks_wide: 0,
                        blocks_high: 0,
                        scan_wide: 0,
                        scan_high: 0,
                        coefficients: Vec::new(),
                        samples: Vec::new(),
                        width: 0,
                        height: 0,
                    });
                }
                ready(&mut components, width, height)?;
            }
            // The other frame types, which nothing here reads.
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
            // Start of scan: everything after it is entropy-coded data. A
            // baseline picture has one; a progressive picture has as many as it
            // needs, and is not a picture until the last of them is read.
            0xDA => {
                let body = segment(data, &mut offset)?;
                if components.is_empty() || width == 0 || height == 0 {
                    return Err(Error::Malformed("a scan before the frame it belongs to"));
                }
                let scan = read_scan_header(body, &mut components, progressive)?;
                decode_scan(
                    &data[offset..],
                    &mut components,
                    &scan,
                    &dc_tables,
                    &ac_tables,
                    restart_interval,
                    progressive,
                )?;
                offset = end_of_scan(data, offset);
            }
            // The end of the picture, which is where a progressive one becomes
            // one. A file that ends without saying so is taken as it stands:
            // what has been read is a picture, and refusing it would throw away
            // everything that did arrive.
            0xD9 => {
                if components.is_empty() {
                    return Err(Error::Malformed("a picture that ends before its pixels"));
                }
                finish(&mut components, &quantisation);
                return Ok(to_rgba(&components, width, height, colour_of(&components, adobe)?));
            }
            // Standalone markers carry no length.
            0x01 | 0xD0..=0xD7 => {}
            // Adobe's own marker, which is the only thing that says how four
            // components are to be read.
            0xEE => {
                let body = segment(data, &mut offset)?;
                if body.starts_with(b"Adobe") && body.len() >= 12 {
                    adobe = Some(body[11]);
                }
            }
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

/// Works out how many blocks each component has, and makes room for them.
///
/// A picture is written in groups of blocks, one group covering the same patch
/// of the picture in every component — so a component sampled at half width has
/// half as many blocks across, and the count is rounded up to whole groups
/// whether or not the last of them hangs over the edge.
fn ready(components: &mut [Component], width: usize, height: usize) -> Result<(), Error> {
    let max_horizontal = components.iter().map(|c| c.horizontal).max().unwrap_or(1);
    let max_vertical = components.iter().map(|c| c.vertical).max().unwrap_or(1);
    let across = width.div_ceil(8 * max_horizontal);
    let down = height.div_ceil(8 * max_vertical);

    for component in components.iter_mut() {
        component.blocks_wide = across * component.horizontal;
        component.blocks_high = down * component.vertical;
        component.width = component.blocks_wide * 8;
        component.height = component.blocks_high * 8;
        check_size(component.width, component.height)?;

        // A scan of this component alone is written in its own blocks, and
        // only as many of them as the picture reaches into.
        let samples_across = (width * component.horizontal).div_ceil(max_horizontal);
        let samples_down = (height * component.vertical).div_ceil(max_vertical);
        component.scan_wide = samples_across.div_ceil(8);
        component.scan_high = samples_down.div_ceil(8);

        let blocks = component
            .blocks_wide
            .checked_mul(component.blocks_high)
            .and_then(|blocks| blocks.checked_mul(64))
            .ok_or(Error::TooLarge)?;
        if blocks > crate::MAX_PIXELS {
            return Err(Error::TooLarge);
        }
        component.coefficients = vec![0; blocks];
        component.samples = vec![128; component.width * component.height];
    }
    Ok(())
}

/// Reads the header that introduces a scan.
///
/// It says which components the scan carries and which tables each of them is
/// written with, and — for a progressive picture — which band of coefficients
/// and which bit of them.
fn read_scan_header(
    body: &[u8],
    components: &mut [Component],
    progressive: bool,
) -> Result<Scan, Error> {
    let count = usize::from(*body.first().ok_or(Error::Truncated)?);
    let mut scan = Scan { start: 0, end: 63, high: 0, low: 0, parts: Vec::new() };

    for index in 0..count {
        let at = 1 + index * 2;
        let entry = body.get(at..at + 2).ok_or(Error::Truncated)?;
        let Some(place) = components.iter().position(|c| c.id == entry[0]) else {
            continue;
        };
        components[place].dc_table = usize::from(entry[1] >> 4).min(3);
        components[place].ac_table = usize::from(entry[1] & 0x0F).min(3);
        scan.parts.push(place);
    }
    if scan.parts.is_empty() {
        return Err(Error::Malformed("a scan of no component of the picture"));
    }

    if progressive {
        let tail = body.get(1 + count * 2..1 + count * 2 + 3).ok_or(Error::Truncated)?;
        scan.start = usize::from(tail[0]).min(63);
        scan.end = usize::from(tail[1]).min(63);
        scan.high = tail[2] >> 4;
        scan.low = tail[2] & 0x0F;
        if scan.end < scan.start {
            return Err(Error::Malformed("a scan whose band ends before it begins"));
        }
        // A scan of several components carries only the first coefficient of
        // each. Anything else would have no order to be written in.
        if scan.parts.len() > 1 && scan.start != 0 {
            return Err(Error::Malformed(
                "a scan of several components past the first coefficient",
            ));
        }
    }
    Ok(scan)
}

/// Where the entropy-coded data of a scan ends.
///
/// At the first marker in it that is neither a stuffed zero nor a restart —
/// the only two things that look like a marker and are not one. The scan
/// carries no length of its own, which is why it has to be found this way.
fn end_of_scan(data: &[u8], from: usize) -> usize {
    let mut at = from;
    while at + 1 < data.len() {
        if data[at] == 0xFF {
            let next = data[at + 1];
            if next != 0x00 && !(0xD0..=0xD7).contains(&next) {
                return at;
            }
        }
        at += 1;
    }
    data.len()
}

/// Decodes one scan into the components' coefficients.
///
/// A scan of one component is written in that component's own blocks, one after
/// another; a scan of several is written in groups, each holding every block of
/// every component that covers one patch of the picture. Both orders are here
/// because a progressive picture uses both, often in the same file.
#[allow(clippy::too_many_arguments)]
fn decode_scan(
    data: &[u8],
    components: &mut [Component],
    scan: &Scan,
    dc_tables: &[HuffmanTable],
    ac_tables: &[HuffmanTable],
    restart_interval: usize,
    progressive: bool,
) -> Result<(), Error> {
    let mut bits = BitReader::new(data);
    let mut predictions = vec![0i32; components.len()];
    let mut eob_run = 0u32;
    let mut since_restart = 0usize;

    // How many units the scan is made of, and how many blocks each holds.
    let alone = scan.parts.len() == 1;
    let (across, down) = if alone {
        let part = &components[scan.parts[0]];
        (part.scan_wide, part.scan_high)
    } else {
        let first = &components[scan.parts[0]];
        (first.blocks_wide / first.horizontal, first.blocks_high / first.vertical)
    };

    for unit_y in 0..down {
        for unit_x in 0..across {
            if restart_interval > 0 && since_restart == restart_interval {
                bits.skip_restart();
                predictions.iter_mut().for_each(|value| *value = 0);
                eob_run = 0;
                since_restart = 0;
            }
            since_restart += 1;

            for (index, part) in scan.parts.iter().copied().enumerate() {
                let (rows, columns) = if alone {
                    (1, 1)
                } else {
                    (components[part].vertical, components[part].horizontal)
                };
                for row in 0..rows {
                    for column in 0..columns {
                        let (block_x, block_y) = if alone {
                            (unit_x, unit_y)
                        } else {
                            (
                                unit_x * components[part].horizontal + column,
                                unit_y * components[part].vertical + row,
                            )
                        };
                        let wide = components[part].blocks_wide;
                        let at = (block_y * wide + block_x) * 64;
                        let (dc, ac) = (
                            &dc_tables[components[part].dc_table],
                            &ac_tables[components[part].ac_table],
                        );
                        let Some(block) = components[part].coefficients.get_mut(at..at + 64) else {
                            continue;
                        };

                        let outcome = if progressive {
                            progressive_block(
                                &mut bits,
                                dc,
                                ac,
                                scan,
                                &mut predictions[index],
                                &mut eob_run,
                                block,
                            )
                        } else {
                            read_block(&mut bits, dc, ac, &mut predictions[index], block)
                        };
                        // A scan that ends before its last block is a scan that
                        // ended: what arrived is kept and the rest stays as it
                        // was. Refusing it would throw away a picture that is
                        // nearly all there.
                        match outcome {
                            Err(Error::Truncated) => return Ok(()),
                            other => other?,
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

/// Reads one block of a baseline scan.
///
/// The coefficients are kept in the zig-zag order they are written in and are
/// not multiplied back up here: that is done once, at the end, for both kinds
/// of picture alike.
fn read_block(
    bits: &mut BitReader<'_>,
    dc: &HuffmanTable,
    ac: &HuffmanTable,
    prediction: &mut i32,
    block: &mut [i32],
) -> Result<(), Error> {
    block.fill(0);

    // The first coefficient is written as a difference from the block before,
    // because neighbouring blocks of a photograph are rarely far apart.
    let length = dc.decode(bits)?;
    let difference = extend(bits.receive(length)?, length);
    *prediction += difference;
    block[0] = *prediction;

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
        block[index] = extend(bits.receive(size)?, size);
        index += 1;
    }

    Ok(())
}

/// Reads one block of a progressive scan, whichever of the four kinds it is.
///
/// A scan carries either the first coefficient or a band of the rest, and
/// either the first bits of them or one more bit of what was already sent.
fn progressive_block(
    bits: &mut BitReader<'_>,
    dc: &HuffmanTable,
    ac: &HuffmanTable,
    scan: &Scan,
    prediction: &mut i32,
    eob_run: &mut u32,
    block: &mut [i32],
) -> Result<(), Error> {
    if scan.start == 0 {
        if scan.high == 0 {
            let length = dc.decode(bits)?;
            let difference = extend(bits.receive(length)?, length);
            *prediction += difference;
            block[0] = *prediction << scan.low;
        } else if bits.bit()? == 1 {
            // One more bit of a coefficient already sent.
            block[0] |= 1 << scan.low;
        }
        return Ok(());
    }

    if scan.high == 0 {
        ac_first(bits, ac, scan, eob_run, block)
    } else {
        ac_refine(bits, ac, scan, eob_run, block)
    }
}

/// The first bits of a band of coefficients above the first.
fn ac_first(
    bits: &mut BitReader<'_>,
    ac: &HuffmanTable,
    scan: &Scan,
    eob_run: &mut u32,
    block: &mut [i32],
) -> Result<(), Error> {
    // A run of blocks with nothing left in this band, counted out rather than
    // written one by one: the whole point of spreading a picture over scans is
    // that most of each scan is empty.
    if *eob_run > 0 {
        *eob_run -= 1;
        return Ok(());
    }

    let mut index = scan.start;
    while index <= scan.end {
        let symbol = ac.decode(bits)?;
        let run = u32::from(symbol >> 4);
        let size = symbol & 0x0F;

        if size == 0 {
            if run < 15 {
                *eob_run = (1 << run) - 1;
                if run > 0 {
                    *eob_run += bits.receive(run as u8)? as u32;
                }
                break;
            }
            index += 16;
            continue;
        }

        index += run as usize;
        if index > scan.end {
            break;
        }
        block[index] = extend(bits.receive(size)?, size) << scan.low;
        index += 1;
    }
    Ok(())
}

/// One more bit of a band of coefficients above the first.
///
/// # Why this one is unlike the others
///
/// Because every coefficient already sent needs a bit whether or not this scan
/// has anything new to say about it, and those bits are written in the gaps
/// between the ones that do. So the run lengths count only the coefficients
/// that are still zero, and each step past a coefficient that is not reads one
/// bit to say whether it grows.
fn ac_refine(
    bits: &mut BitReader<'_>,
    ac: &HuffmanTable,
    scan: &Scan,
    eob_run: &mut u32,
    block: &mut [i32],
) -> Result<(), Error> {
    let positive = 1i32 << scan.low;
    let negative = -1i32 << scan.low;
    let mut index = scan.start;

    if *eob_run == 0 {
        while index <= scan.end {
            let symbol = ac.decode(bits)?;
            let mut run = i32::from(symbol >> 4);
            let size = symbol & 0x0F;
            let mut value = 0i32;

            if size == 0 {
                if run < 15 {
                    // The block this is read in is one of the run, and its own
                    // coefficients still want their bits — so the count is the
                    // whole run here, and one is taken off it below, after
                    // those bits have been read. A scan that took one off now
                    // would skip them, and every coefficient already sent
                    // would stop growing wherever the first empty band began.
                    *eob_run = 1u32 << run;
                    if run > 0 {
                        *eob_run += bits.receive(run as u8)? as u32;
                    }
                    break;
                }
            } else {
                // The only size a refining scan writes: one bit, which says
                // which way a coefficient that has just become non-zero goes.
                value = if bits.bit()? == 1 { positive } else { negative };
            }

            while index <= scan.end {
                if block[index] != 0 {
                    if bits.bit()? == 1 && (block[index] & positive) == 0 {
                        block[index] += if block[index] >= 0 { positive } else { negative };
                    }
                } else {
                    if run == 0 {
                        if value != 0 {
                            block[index] = value;
                        }
                        index += 1;
                        break;
                    }
                    run -= 1;
                }
                index += 1;
            }
        }
    }

    if *eob_run > 0 {
        // The rest of the band belongs to a run of empty blocks — but the
        // coefficients already sent still each need their bit.
        while index <= scan.end {
            if block[index] != 0 && bits.bit()? == 1 && (block[index] & positive) == 0 {
                block[index] += if block[index] >= 0 { positive } else { negative };
            }
            index += 1;
        }
        *eob_run -= 1;
    }

    Ok(())
}

/// Turns every block's coefficients into samples.
///
/// Done once, when every scan has been read: the coefficients are multiplied
/// back up by the quantisation table, put back into raster order from the
/// zig-zag they were written in, and transformed.
fn finish(components: &mut [Component], quantisation: &[[u16; 64]; 4]) {
    let mut block = [0i32; 64];
    let mut samples = [0u8; 64];

    for component in components.iter_mut() {
        let table = &quantisation[component.quantisation];
        for block_y in 0..component.blocks_high {
            for block_x in 0..component.blocks_wide {
                let at = (block_y * component.blocks_wide + block_x) * 64;
                let Some(written) = component.coefficients.get(at..at + 64) else { continue };
                for (index, value) in written.iter().enumerate() {
                    let place = ZIGZAG[index];
                    block[place] = value * i32::from(table[place]);
                }
                inverse_transform(&block, &mut samples);

                let left = block_x * 8;
                let top = block_y * 8;
                for y in 0..8 {
                    let destination = (top + y) * component.width + left;
                    let Some(slice) = component.samples.get_mut(destination..destination + 8)
                    else {
                        continue;
                    };
                    slice.copy_from_slice(&samples[y * 8..y * 8 + 8]);
                }
            }
        }
    }
}

/// What the components mean, from how many there are and what Adobe's marker
/// said about them.
fn colour_of(components: &[Component], adobe: Option<u8>) -> Result<Colour, Error> {
    match components.len() {
        1 => Ok(Colour::Grey),
        // Three components are brightness and two colour differences unless
        // something says otherwise. Two things can: Adobe's marker calling them
        // untransformed, and the components naming themselves after the three
        // colours, which is what a picture written by a printer driver does.
        3 => {
            let named =
                components[0].id == b'R' && components[1].id == b'G' && components[2].id == b'B';
            if adobe == Some(0) || named {
                Ok(Colour::Rgb)
            } else {
                Ok(Colour::YCbCr)
            }
        }
        4 => Ok(if adobe == Some(2) { Colour::Ycck } else { Colour::Cmyk }),
        _ => Err(Error::Unsupported("a picture of more colours than any exist in")),
    }
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
fn to_rgba(components: &[Component], width: usize, height: usize, colour: Colour) -> Image {
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
            let at = |index: usize| sample(&components[index], x, y);
            let (red, green, blue) = match colour {
                Colour::Grey => {
                    let grey = at(0);
                    (grey, grey, grey)
                }
                Colour::Rgb => (at(0), at(1), at(2)),
                Colour::YCbCr => from_differences(at(0), at(1), at(2)),
                Colour::Cmyk => from_ink(at(0), at(1), at(2), at(3)),
                Colour::Ycck => {
                    let (red, green, blue) = from_differences(at(0), at(1), at(2));
                    from_ink(red, green, blue, at(3))
                }
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

/// Ink, as Adobe writes it, turned into light.
///
/// Adobe writes the four inks inverted: what is stored is how much light gets
/// through rather than how much ink is on the page. So the three colours
/// multiply by the black rather than being subtracted from it — and a reader
/// that does not know this shows a scanned page as a photographic negative,
/// which is exactly what such a reader does.
fn from_ink(cyan: f32, magenta: f32, yellow: f32, black: f32) -> (f32, f32, f32) {
    let black = black / 255.0;
    (cyan * black, magenta * black, yellow * black)
}

/// Brightness and two colour differences, as the format stores colour.
///
/// The eye notices detail in brightness far more than in colour, and the whole
/// format is built round that: this is the arithmetic that undoes it.
fn from_differences(luma: f32, blue: f32, red: f32) -> (f32, f32, f32) {
    let blue_difference = blue - 128.0;
    let red_difference = red - 128.0;
    (
        luma + 1.402 * red_difference,
        luma - 0.344_136 * blue_difference - 0.714_136 * red_difference,
        luma + 1.772 * blue_difference,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anything_else_is_not_a_jpeg() {
        assert_eq!(decode(b"not a jpeg at all"), Err(Error::UnknownFormat));
    }

    /// Writes entropy-coded bits, most significant first, stuffing a zero
    /// after any byte of 0xFF.
    #[derive(Default)]
    struct Writer {
        bytes: Vec<u8>,
        held: u8,
        count: u8,
    }

    impl Writer {
        fn put(&mut self, value: u32, width: u8) {
            for step in (0..width).rev() {
                let bit = ((value >> step) & 1) as u8;
                self.held = (self.held << 1) | bit;
                self.count += 1;
                if self.count == 8 {
                    self.bytes.push(self.held);
                    if self.held == 0xFF {
                        self.bytes.push(0);
                    }
                    self.held = 0;
                    self.count = 0;
                }
            }
        }

        /// The last byte is padded with ones, which is what the format pads
        /// with.
        fn finish(mut self) -> Vec<u8> {
            while self.count != 0 {
                self.put(1, 1);
            }
            self.bytes
        }
    }

    fn segment(marker: u8, body: &[u8]) -> Vec<u8> {
        let mut out = vec![0xFF, marker];
        let length = body.len() + 2;
        out.push((length >> 8) as u8);
        out.push((length & 0xFF) as u8);
        out.extend_from_slice(body);
        out
    }

    /// A table of eight codes four bits long, standing for the symbols nought
    /// to seven. Canonical, so the code for a symbol is the symbol — and eight
    /// rather than sixteen because no table may use the code that is all ones.
    fn table(class: u8) -> Vec<u8> {
        let mut body = vec![class << 4];
        for length in 1..=16u8 {
            body.push(if length == 4 { 8 } else { 0 });
        }
        body.extend(0..8u8);
        body
    }

    fn scan_head(start: u8, end: u8, high: u8, low: u8) -> Vec<u8> {
        vec![1, 1, 0x00, start, end, (high << 4) | low]
    }

    /// One block of one component, written in up to four scans: the first bits
    /// of the first coefficient, the first bits of the rest, and — if asked —
    /// one more bit of each.
    fn progressive(refined: bool) -> Vec<u8> {
        let mut out = vec![0xFF, 0xD8];

        let mut quantisation = vec![0u8];
        quantisation.extend(std::iter::repeat_n(1u8, 64));
        out.extend(segment(0xDB, &quantisation));

        let frame = [8, 0, 8, 0, 8, 1, 1, 0x11, 0];
        out.extend(segment(0xC2, &frame));
        out.extend(segment(0xC4, &table(0)));
        out.extend(segment(0xC4, &table(1)));

        // The first coefficient at half precision: a difference of four, which
        // is three bits wide.
        out.extend(segment(0xDA, &scan_head(0, 0, 0, 1)));
        let mut bits = Writer::default();
        bits.put(3, 4);
        bits.put(4, 3);
        out.extend(bits.finish());

        // The rest of the band, also at half precision.
        out.extend(segment(0xDA, &scan_head(1, 63, 0, 1)));
        let mut bits = Writer::default();
        bits.put(2, 4); // No run, two bits,
        bits.put(3, 2); // which are three.
        bits.put(1, 4); // No run, one bit,
        bits.put(0, 1); // which is minus one.
        bits.put(0, 4); // And the end of the block.
        out.extend(bits.finish());

        if refined {
            out.extend(segment(0xDA, &scan_head(0, 0, 1, 0)));
            let mut bits = Writer::default();
            bits.put(1, 1);
            out.extend(bits.finish());

            out.extend(segment(0xDA, &scan_head(1, 63, 1, 0)));
            let mut bits = Writer::default();
            bits.put(0, 4); // The end of the block, a run of one.
            bits.put(1, 1); // The first coefficient grows.
            bits.put(0, 1); // The second does not.
            out.extend(bits.finish());
        }

        out.extend_from_slice(&[0xFF, 0xD9]);
        out
    }

    #[test]
    fn a_progressive_picture_is_read() {
        let image = decode(&progressive(true)).expect("a progressive picture");
        assert_eq!((image.width, image.height), (8, 8));
        // Not flat: the band above the first coefficient was read as well.
        let first = image.pixels[0];
        assert!(
            image.pixels.chunks_exact(4).any(|pixel| pixel[0] != first),
            "the picture came out flat, so only the first coefficient was read"
        );
    }

    #[test]
    fn the_scans_that_add_a_bit_are_read_too() {
        // The same picture with and without its refining scans. If they were
        // being stepped over, the two would come out identical.
        let coarse = decode(&progressive(false)).expect("a picture");
        let fine = decode(&progressive(true)).expect("a picture");
        assert_ne!(coarse.pixels, fine.pixels, "the refining scans changed nothing");
    }

    /// One component of a picture, as a frame header would have made it.
    fn part(id: u8) -> Component {
        Component {
            id,
            horizontal: 1,
            vertical: 1,
            quantisation: 0,
            dc_table: 0,
            ac_table: 0,
            blocks_wide: 1,
            blocks_high: 1,
            scan_wide: 1,
            scan_high: 1,
            coefficients: vec![0; 64],
            samples: vec![128; 64],
            width: 8,
            height: 8,
        }
    }

    #[test]
    fn a_scan_of_several_components_past_the_first_coefficient_is_refused() {
        // The format allows the higher coefficients only one component at a
        // time: several of them would have no order to be written in.
        let body = [2, 1, 0x00, 2, 0x00, 1, 63, 0x00];
        let mut components = vec![part(1), part(2)];
        assert!(read_scan_header(&body, &mut components, true).is_err());

        // The first coefficient is the one they may share.
        let body = [2, 1, 0x00, 2, 0x00, 0, 0, 0x00];
        assert!(read_scan_header(&body, &mut components, true).is_ok());
    }

    #[test]
    fn a_scan_of_no_component_of_the_picture_is_refused() {
        let body = [1, 9, 0x00, 0, 63, 0x00];
        let mut components = vec![part(1)];
        assert!(read_scan_header(&body, &mut components, true).is_err());
    }

    #[test]
    fn a_coding_this_decoder_does_not_read_says_so() {
        // Start of image, then a lossless frame header.
        let data = [0xFF, 0xD8, 0xFF, 0xC3, 0x00, 0x02];
        assert_eq!(decode(&data), Err(Error::Unsupported("a coding this decoder does not read")));
    }

    #[test]
    fn what_the_components_mean_is_read_from_how_many_and_what_adobe_said() {
        let parts = |count: usize, ids: &[u8]| -> Vec<Component> {
            (0..count)
                .map(|index| part(ids.get(index).copied().unwrap_or(index as u8 + 1)))
                .collect()
        };

        assert_eq!(colour_of(&parts(1, &[]), None), Ok(Colour::Grey));
        assert_eq!(colour_of(&parts(3, &[]), None), Ok(Colour::YCbCr));
        // Adobe saying nothing was transformed, and a picture whose components
        // name themselves after the three colours: both mean the same thing.
        assert_eq!(colour_of(&parts(3, &[]), Some(0)), Ok(Colour::Rgb));
        assert_eq!(colour_of(&parts(3, b"RGB"), None), Ok(Colour::Rgb));
        assert_eq!(colour_of(&parts(4, &[]), None), Ok(Colour::Cmyk));
        assert_eq!(colour_of(&parts(4, &[]), Some(2)), Ok(Colour::Ycck));
    }

    #[test]
    fn ink_at_its_blackest_is_black_and_at_its_lightest_is_the_paper() {
        // Adobe writes ink inverted: everything at 255 is a blank page, and
        // black at nothing is black.
        let white = from_ink(255.0, 255.0, 255.0, 255.0);
        assert_eq!(white, (255.0, 255.0, 255.0));
        let black = from_ink(255.0, 255.0, 255.0, 0.0);
        assert_eq!(black, (0.0, 0.0, 0.0));
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
