//! Reading GIF, to the 89a specification.
//!
//! # The shape of the format
//!
//! A header saying how big the whole picture is, usually a palette, and then a
//! stream of blocks: extensions carrying things that are not pixels, images,
//! and a byte that says the file has ended. An animation is several images in
//! that stream and nothing more — there is no separate kind of file for one.
//!
//! # Why only the first frame
//!
//! Because a document shows a picture, not a film. Word draws the first frame
//! of an animated GIF and so does every other word processor; a page that
//! animated while it was being read would be a page nobody could read. The rest
//! of the frames are left where they are, in the file, which is carried through
//! whole and saved back unchanged.
//!
//! # How the pixels are written
//!
//! LZW, in a variant of its own: the codes are packed least significant bit
//! first, the width of a code grows as the table fills, and two codes are
//! reserved — one that empties the table and one that ends the picture. The
//! table is rebuilt as it is read, which is what makes the format decodable
//! without anything being written down but the codes themselves.

use crate::{check_size, Error, Image};

/// The six bytes every GIF begins with, in its two versions.
pub const SIGNATURE_87: [u8; 6] = *b"GIF87a";
pub const SIGNATURE_89: [u8; 6] = *b"GIF89a";

/// Decodes a GIF into eight-bit RGBA.
///
/// An animation decodes to its first frame, drawn where the file says it goes
/// on a canvas the size of the whole picture.
pub fn decode(data: &[u8]) -> Result<Image, Error> {
    if !(data.starts_with(&SIGNATURE_87) || data.starts_with(&SIGNATURE_89)) {
        return Err(Error::UnknownFormat);
    }

    let screen = data.get(6..13).ok_or(Error::Truncated)?;
    let width = usize::from(u16::from_le_bytes([screen[0], screen[1]]));
    let height = usize::from(u16::from_le_bytes([screen[2], screen[3]]));
    check_size(width, height)?;

    let flags = screen[4];
    let mut at = 13usize;
    let global = if flags & 0x80 != 0 {
        let entries = 2usize << (flags & 0x07);
        let table = read_table(data, at, entries)?;
        at += entries * 3;
        table
    } else {
        Vec::new()
    };

    // What is not covered by the first frame stays as it is, which for a
    // picture in a document means nothing at all rather than a background
    // colour: a GIF laid over a page shows the page through its edges.
    let mut image = Image::empty(width, height);
    // The transparent colour, from the last graphic control extension before
    // the frame. It belongs to the frame that follows it and to no other.
    let mut transparent: Option<u8> = None;

    loop {
        let block = *data.get(at).ok_or(Error::Truncated)?;
        at += 1;
        match block {
            // An extension: a kind byte, then sub-blocks.
            0x21 => {
                let kind = *data.get(at).ok_or(Error::Truncated)?;
                at += 1;
                let start = at;
                at = past_blocks(data, at)?;
                if kind == 0xF9 {
                    // The graphic control extension: four bytes of body inside
                    // one sub-block, of which the fourth is the transparent
                    // colour and the first says whether to believe it.
                    let body = data.get(start + 1..start + 5).ok_or(Error::Truncated)?;
                    transparent = (body[0] & 0x01 != 0).then_some(body[3]);
                }
            }
            // An image, which is the one this reader is looking for.
            0x2C => {
                read_frame(data, at, &global, transparent, &mut image)?;
                return Ok(image);
            }
            // The end of the file, before any picture in it.
            0x3B => return Err(Error::Malformed("a file that ends before its picture")),
            _ => return Err(Error::Malformed("a block of no known kind")),
        }
    }
}

/// Reads one image block onto the canvas.
fn read_frame(
    data: &[u8],
    at: usize,
    global: &[[u8; 4]],
    transparent: Option<u8>,
    image: &mut Image,
) -> Result<(), Error> {
    let header = data.get(at..at + 9).ok_or(Error::Truncated)?;
    let left = usize::from(u16::from_le_bytes([header[0], header[1]]));
    let top = usize::from(u16::from_le_bytes([header[2], header[3]]));
    let width = usize::from(u16::from_le_bytes([header[4], header[5]]));
    let height = usize::from(u16::from_le_bytes([header[6], header[7]]));
    let flags = header[8];
    let mut at = at + 9;

    // A frame may bring a palette of its own, which is the one it is drawn in.
    let local = if flags & 0x80 != 0 {
        let entries = 2usize << (flags & 0x07);
        let table = read_table(data, at, entries)?;
        at += entries * 3;
        table
    } else {
        Vec::new()
    };
    let palette = if local.is_empty() { global } else { &local };
    if palette.is_empty() {
        return Err(Error::Malformed("a picture with no palette anywhere"));
    }

    let least = *data.get(at).ok_or(Error::Truncated)?;
    at += 1;
    if least > 8 {
        return Err(Error::Malformed("a code wider than a palette can be"));
    }
    let pixels = unpack(data, at, least, width.saturating_mul(height))?;

    let interlaced = flags & 0x40 != 0;
    for (index, entry) in pixels.iter().enumerate() {
        let row = index / width.max(1);
        let column = index % width.max(1);
        let row = if interlaced { interlaced_row(row, height) } else { row };
        let (Some(x), Some(y)) = (left.checked_add(column), top.checked_add(row)) else {
            continue;
        };
        if x >= image.width || y >= image.height {
            continue;
        }
        let mut colour = palette.get(usize::from(*entry)).copied().unwrap_or([0, 0, 0, 255]);
        // The colour is kept under a pixel the file says not to draw, and only
        // its alpha is taken away. Nothing shows it, but anything that scales
        // the picture averages what is under its neighbours — and a colour
        // thrown away here would come back as a dark halo round every edge.
        // It is also what every other reader of the format leaves there.
        if Some(*entry) == transparent {
            colour[3] = 0;
        }
        let place = (y * image.width + x) * 4;
        if let Some(slice) = image.pixels.get_mut(place..place + 4) {
            slice.copy_from_slice(&colour);
        }
    }

    Ok(())
}

/// Which row of the picture a row of an interlaced frame is.
///
/// Four passes: every eighth row, then the eighth rows offset by four, then
/// every fourth row offset by two, then every second. It is what lets a picture
/// arriving down a slow line show its shape before it has all arrived, and it
/// is still written.
fn interlaced_row(row: usize, height: usize) -> usize {
    let eighths = height.div_ceil(8);
    let quarters = height.div_ceil(4) - height.div_ceil(8);
    let halves = height.div_ceil(2) - height.div_ceil(4);
    if row < eighths {
        return row * 8;
    }
    if row < eighths + quarters {
        return (row - eighths) * 8 + 4;
    }
    if row < eighths + quarters + halves {
        return (row - eighths - quarters) * 4 + 2;
    }
    (row - eighths - quarters - halves) * 2 + 1
}

/// Reads a palette of a given number of entries, as opaque RGBA.
fn read_table(data: &[u8], at: usize, entries: usize) -> Result<Vec<[u8; 4]>, Error> {
    let bytes = data.get(at..at + entries * 3).ok_or(Error::Truncated)?;
    Ok(bytes.chunks_exact(3).map(|entry| [entry[0], entry[1], entry[2], 255]).collect())
}

/// Steps over a chain of sub-blocks, and says where it ended.
///
/// Each is a length byte and that many bytes; a length of zero ends the chain.
fn past_blocks(data: &[u8], mut at: usize) -> Result<usize, Error> {
    loop {
        let length = usize::from(*data.get(at).ok_or(Error::Truncated)?);
        at += 1;
        if length == 0 {
            return Ok(at);
        }
        at = at.checked_add(length).ok_or(Error::Truncated)?;
        if at > data.len() {
            return Err(Error::Truncated);
        }
    }
}

/// Decodes the LZW stream of one frame into palette indices.
///
/// # How the table is built
///
/// Every code stands for a string of indices. The first few stand for
/// themselves; the rest are added as the stream is read, each being the string
/// the last code stood for with one more index on the end. Which index that is
/// cannot be known until the next code arrives, which is why a code can arrive
/// that is not in the table yet — the one case the specification names, and the
/// one every implementation has to handle by hand.
fn unpack(data: &[u8], at: usize, least: u8, wanted: usize) -> Result<Vec<u8>, Error> {
    let clear = 1u16 << least;
    let end = clear + 1;

    // Each entry is the code it extends and the index it adds, which is enough
    // to write any string out backwards without storing the strings themselves.
    let mut previous = vec![0u16; 4096];
    let mut added = vec![0u8; 4096];
    // The codes below the clear code stand for themselves, and are the only
    // ones in the table before anything has been read.
    for (index, entry) in added.iter_mut().enumerate().take(usize::from(clear)) {
        *entry = index as u8;
    }

    let mut next = end + 1;
    let mut width = least + 1;
    let mut last: Option<u16> = None;
    let mut out = Vec::with_capacity(wanted);
    let mut bits = Bits::new(data, at);
    let mut run = Vec::new();

    while out.len() < wanted {
        let Some(code) = bits.take(width)? else { break };
        if code == clear {
            next = end + 1;
            width = least + 1;
            last = None;
            continue;
        }
        if code == end {
            break;
        }

        // The one case the specification names: a code for a string that is
        // about to be added. It can only ever be the last string with its own
        // first index on the end.
        let known = code < next;
        if !known && last.is_none() {
            return Err(Error::Malformed("a code before anything it could extend"));
        }

        run.clear();
        let mut walk = if known { code } else { last.unwrap_or(0) };
        loop {
            run.push(added[usize::from(walk)]);
            if walk < clear {
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
                // The codes grow a bit wider whenever the table fills, up to
                // twelve bits, after which it stays full until it is cleared.
                if next == (1 << width) && width < 12 {
                    width += 1;
                }
            }
        }
        last = Some(code);
    }

    out.truncate(wanted);
    // A frame that stopped early is not an error: what is missing is left
    // transparent, which is better than refusing a picture that is nearly all
    // there. The palette's first colour would be a lie about what is in it.
    out.resize(wanted, 0);
    Ok(out)
}

/// Reads codes out of the chain of sub-blocks a frame's pixels are written in.
///
/// The codes are packed least significant bit first and run straight across the
/// boundary between one sub-block and the next, so the two cannot be separated:
/// the bits are read as though the sub-block lengths were not there.
struct Bits<'a> {
    data: &'a [u8],
    /// Where the current sub-block's bytes begin and end.
    at: usize,
    end: usize,
    /// Where the next sub-block's length byte is.
    next_block: usize,
    held: u32,
    count: u8,
    done: bool,
}

impl<'a> Bits<'a> {
    fn new(data: &'a [u8], at: usize) -> Self {
        Self { data, at: 0, end: 0, next_block: at, held: 0, count: 0, done: false }
    }

    /// The next code, or nothing when the sub-blocks have run out.
    fn take(&mut self, width: u8) -> Result<Option<u16>, Error> {
        while self.count < width {
            let Some(byte) = self.byte()? else {
                return Ok(None);
            };
            self.held |= u32::from(byte) << self.count;
            self.count += 8;
        }
        let code = (self.held & ((1u32 << width) - 1)) as u16;
        self.held >>= width;
        self.count -= width;
        Ok(Some(code))
    }

    /// The next byte of pixel data, stepping into the next sub-block as needed.
    fn byte(&mut self) -> Result<Option<u8>, Error> {
        if self.at == self.end {
            if self.done {
                return Ok(None);
            }
            let length = usize::from(*self.data.get(self.next_block).ok_or(Error::Truncated)?);
            if length == 0 {
                self.done = true;
                return Ok(None);
            }
            self.at = self.next_block + 1;
            self.end = self.at + length;
            if self.end > self.data.len() {
                return Err(Error::Truncated);
            }
            self.next_block = self.end;
        }
        let byte = self.data[self.at];
        self.at += 1;
        Ok(Some(byte))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A picture of four pixels: a palette of four and one code each.
    ///
    /// Written the way a program with no compressor writes one — a clear code,
    /// then a code for every pixel — which is what makes it short enough to
    /// write out here by hand.
    fn four_pixels(interlaced: bool, transparent: Option<u8>) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&SIGNATURE_89);
        out.extend_from_slice(&2u16.to_le_bytes()); // Two across.
        out.extend_from_slice(&2u16.to_le_bytes()); // Two down.
        out.push(0x80 | 0x01); // A global palette of four.
        out.push(0); // The background colour.
        out.push(0); // The aspect ratio.
        out.extend_from_slice(&[0xFF, 0x00, 0x00]); // Red.
        out.extend_from_slice(&[0x00, 0xFF, 0x00]); // Green.
        out.extend_from_slice(&[0x00, 0x00, 0xFF]); // Blue.
        out.extend_from_slice(&[0xFF, 0xFF, 0x00]); // Yellow.

        if let Some(index) = transparent {
            out.extend_from_slice(&[0x21, 0xF9, 0x04, 0x01, 0x00, 0x00, index, 0x00]);
        }

        out.push(0x2C);
        out.extend_from_slice(&0u16.to_le_bytes()); // At the left.
        out.extend_from_slice(&0u16.to_le_bytes()); // And the top.
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.push(if interlaced { 0x40 } else { 0x00 });

        // Three bits to a code: four colours, a clear code and an end code.
        //
        // A clear code every second pixel, which is what a program with no
        // compressor writes: the table never reaches eight entries, so the
        // codes never grow past three bits and the stream can be written out
        // by hand. It is a real stream for all that — a decoder cannot tell
        // this from one a compressor produced.
        out.push(2);
        let codes = [4u16, 0, 1, 4, 2, 3, 5];
        let mut held = 0u32;
        let mut count = 0u8;
        let mut bytes = Vec::new();
        for code in codes {
            held |= u32::from(code) << count;
            count += 3;
            while count >= 8 {
                bytes.push((held & 0xFF) as u8);
                held >>= 8;
                count -= 8;
            }
        }
        if count > 0 {
            bytes.push((held & 0xFF) as u8);
        }
        out.push(bytes.len() as u8);
        out.extend_from_slice(&bytes);
        out.push(0);

        out.push(0x3B);
        out
    }

    fn pixel(image: &Image, x: usize, y: usize) -> [u8; 4] {
        let at = (y * image.width + x) * 4;
        [image.pixels[at], image.pixels[at + 1], image.pixels[at + 2], image.pixels[at + 3]]
    }

    #[test]
    fn anything_else_is_not_a_gif() {
        assert_eq!(decode(b"not a gif at all!!"), Err(Error::UnknownFormat));
    }

    #[test]
    fn both_versions_are_read() {
        let mut older = four_pixels(false, None);
        older[..6].copy_from_slice(&SIGNATURE_87);
        assert!(decode(&older).is_ok(), "the older version was refused");
        assert!(decode(&four_pixels(false, None)).is_ok());
    }

    #[test]
    fn every_pixel_comes_back_its_own_colour() {
        let image = decode(&four_pixels(false, None)).expect("a picture");
        assert_eq!((image.width, image.height), (2, 2));
        assert_eq!(pixel(&image, 0, 0), [0xFF, 0, 0, 255]);
        assert_eq!(pixel(&image, 1, 0), [0, 0xFF, 0, 255]);
        assert_eq!(pixel(&image, 0, 1), [0, 0, 0xFF, 255]);
        assert_eq!(pixel(&image, 1, 1), [0xFF, 0xFF, 0, 255]);
    }

    #[test]
    fn the_colour_a_file_calls_transparent_is_not_drawn() {
        // The second colour, which is the pixel at the top right.
        let image = decode(&four_pixels(false, Some(1))).expect("a picture");
        assert_eq!(pixel(&image, 1, 0)[3], 0, "it should not have been drawn");
        assert_eq!(pixel(&image, 0, 0)[3], 255, "and the rest should have been");
    }

    #[test]
    fn an_interlaced_frame_puts_its_rows_where_they_belong() {
        // Two rows: the first pass takes every eighth, which for a picture two
        // rows tall is the first, and the last pass takes the second.
        let image = decode(&four_pixels(true, None)).expect("a picture");
        assert_eq!(pixel(&image, 0, 0), [0xFF, 0, 0, 255]);
        assert_eq!(pixel(&image, 0, 1), [0, 0, 0xFF, 255]);
    }

    #[test]
    fn the_rows_of_an_interlaced_picture_are_each_reached_once() {
        for height in [1usize, 2, 5, 8, 9, 16, 17, 33] {
            let mut seen = vec![false; height];
            for row in 0..height {
                let real = interlaced_row(row, height);
                assert!(real < height, "row {row} of {height} landed at {real}");
                assert!(!seen[real], "row {real} of {height} was written twice");
                seen[real] = true;
            }
        }
    }

    #[test]
    fn the_first_pass_takes_every_eighth_row() {
        assert_eq!(interlaced_row(0, 16), 0);
        assert_eq!(interlaced_row(1, 16), 8);
        assert_eq!(interlaced_row(2, 16), 4);
        assert_eq!(interlaced_row(3, 16), 12);
        assert_eq!(interlaced_row(4, 16), 2);
    }

    #[test]
    fn a_file_that_ends_before_its_picture_says_so() {
        let mut out = Vec::new();
        out.extend_from_slice(&SIGNATURE_89);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&[0x00, 0x00, 0x00]);
        out.push(0x3B);
        assert_eq!(decode(&out), Err(Error::Malformed("a file that ends before its picture")));
    }

    #[test]
    fn a_size_of_nothing_is_refused_before_anything_is_allocated() {
        let mut out = Vec::new();
        out.extend_from_slice(&SIGNATURE_89);
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&[0x00, 0x00, 0x00]);
        assert!(decode(&out).is_err());
    }

    /// The same picture with a second frame after the first, and a frame that
    /// covers only part of the screen.
    fn two_frames(left: u16, top: u16, size: u16) -> Vec<u8> {
        let mut out = four_pixels(false, None);
        // Everything but the trailer, then a second frame and the trailer.
        out.pop();
        out.push(0x2C);
        out.extend_from_slice(&left.to_le_bytes());
        out.extend_from_slice(&top.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.push(0x00);
        out.push(2);
        // One pixel of the fourth colour, which is nowhere in the first frame.
        out.push(2);
        // A clear code, the fourth colour, and the end code: three bits each,
        // packed from the bottom of the byte up.
        out.extend_from_slice(&[0b0101_1100, 0b0000_0001]);
        out.push(0);
        out.push(0x3B);
        out
    }

    #[test]
    fn an_animation_is_drawn_as_its_first_frame() {
        // The second frame would put yellow at the top left; the picture shows
        // what the first frame put there.
        let image = decode(&two_frames(0, 0, 1)).expect("a picture");
        assert_eq!(pixel(&image, 0, 0), [0xFF, 0, 0, 255], "the second frame was drawn");
    }

    #[test]
    fn a_frame_smaller_than_the_screen_leaves_the_rest_alone() {
        // A picture whose only frame is one pixel in the middle: the rest of
        // the screen is nothing at all, so a page shows through it.
        let mut out = Vec::new();
        out.extend_from_slice(&SIGNATURE_89);
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.push(0x80 | 0x01);
        out.push(0);
        out.push(0);
        out.extend_from_slice(&[0xFF, 0x00, 0x00]);
        out.extend_from_slice(&[0x00, 0xFF, 0x00]);
        out.extend_from_slice(&[0x00, 0x00, 0xFF]);
        out.extend_from_slice(&[0xFF, 0xFF, 0x00]);
        out.push(0x2C);
        out.extend_from_slice(&1u16.to_le_bytes()); // One along,
        out.extend_from_slice(&1u16.to_le_bytes()); // and one down.
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.push(0x00);
        out.push(2);
        out.push(2);
        // A clear code, the fourth colour, and the end code: three bits each,
        // packed from the bottom of the byte up.
        out.extend_from_slice(&[0b0101_1100, 0b0000_0001]);
        out.push(0);
        out.push(0x3B);

        let image = decode(&out).expect("a picture");
        assert_eq!(pixel(&image, 1, 1), [0xFF, 0xFF, 0, 255], "the frame is not where it says");
        assert_eq!(pixel(&image, 0, 0)[3], 0, "the rest should be nothing at all");
    }

    #[test]
    fn codes_are_read_from_the_bottom_of_each_byte_up() {
        // Two bytes holding three-bit codes: 0b101, 0b010, 0b110, ...
        let data = [0x02u8, 0b0101_0101, 0b0000_0010];
        let mut bits = Bits::new(&data, 0);
        assert_eq!(bits.take(3).unwrap(), Some(0b101));
        assert_eq!(bits.take(3).unwrap(), Some(0b010));
    }

    #[test]
    fn the_codes_run_across_the_join_between_sub_blocks() {
        // One byte in each of two sub-blocks, and a code that spans them.
        let data = [0x01u8, 0xFF, 0x01, 0x01, 0x00];
        let mut bits = Bits::new(&data, 0);
        assert_eq!(bits.take(8).unwrap(), Some(0xFF));
        assert_eq!(bits.take(8).unwrap(), Some(0x01), "the next sub-block should follow on");
        assert_eq!(bits.take(8).unwrap(), None, "and then the chain has ended");
    }
}
