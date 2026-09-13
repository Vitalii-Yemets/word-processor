//! Reading the Windows metafile, to MS-WMF.
//!
//! # The shape of the format
//!
//! The older of the two, and smaller in every way: records of a length and a
//! function number, coordinates of sixteen bits, and a handful of objects. What
//! a document carries one of is clip art, a chart pasted out of a spreadsheet,
//! or a drawing somebody made in 1995.
//!
//! # The header that is not always there
//!
//! A metafile as Windows kept it in memory had no size: it was played back onto
//! whatever the program chose. A metafile in a file needs one, so the *placeable*
//! header was bolted on the front — twenty-two bytes giving the rectangle the
//! drawing covers and how many of its units go to the inch. Almost every
//! metafile in a document has one, and a metafile without one is played back at
//! the size its window says instead.
//!
//! # Why the parameters read backwards
//!
//! Because they were pushed on a stack. A rectangle is written bottom, right,
//! top, left — the order the arguments were in when the call was recorded, which
//! is the reverse of the order they were written in the program. It is the one
//! thing about this format that catches everybody.

use wp_raster::{Path, Point, Rule, Transform};

use crate::metafile::{
    brush_of, colour_of, ellipse, pen_of, rounded, Alignment, Faces, LogFont, Object, State,
};
use crate::{check_size, Error, Image};

/// The four bytes a placeable metafile begins with.
pub const PLACEABLE: [u8; 4] = [0xD7, 0xCD, 0xC6, 0x9A];

/// How many pixels an inch is played back at.
const PER_INCH: f32 = 96.0;

/// Half a pixel: a coordinate names a pixel, not the corner between four.
const HALF: f32 = 0.5;

fn short(data: &[u8], at: usize) -> Option<i16> {
    let bytes = data.get(at..at + 2)?;
    Some(i16::from_le_bytes([bytes[0], bytes[1]]))
}

fn word(data: &[u8], at: usize) -> Option<u16> {
    let bytes = data.get(at..at + 2)?;
    Some(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn long(data: &[u8], at: usize) -> Option<u32> {
    let bytes = data.get(at..at + 4)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// Whether these bytes are a Windows metafile.
#[must_use]
pub fn is_wmf(data: &[u8]) -> bool {
    if data.starts_with(&PLACEABLE) {
        return true;
    }
    // Without the placeable header there is only the metafile header to go on:
    // a kind of one or two, a header of nine words, and a version of one of
    // the two there ever were.
    matches!(word(data, 0), Some(1 | 2))
        && word(data, 2) == Some(9)
        && matches!(word(data, 4), Some(0x0100 | 0x0300))
}

/// Plays a Windows metafile back into pixels.
pub fn decode(data: &[u8]) -> Result<Image, Error> {
    decode_with(data, &crate::metafile::NoFaces)
}

/// The same, with somewhere to get letter shapes from.
///
/// A caller with no fonts to offer gets the picture without its words. See
/// [`crate::metafile::Faces`].
pub fn decode_with(data: &[u8], faces: &dyn Faces) -> Result<Image, Error> {
    if !is_wmf(data) {
        return Err(Error::UnknownFormat);
    }

    // The placeable header, when there is one: the rectangle the drawing
    // covers, in units of which it says how many go to the inch.
    let (mut at, bounds) = if data.starts_with(&PLACEABLE) {
        let left = short(data, 6).ok_or(Error::Truncated)?;
        let top = short(data, 8).ok_or(Error::Truncated)?;
        let right = short(data, 10).ok_or(Error::Truncated)?;
        let bottom = short(data, 12).ok_or(Error::Truncated)?;
        let inch = word(data, 14).unwrap_or(1440).max(1);
        (22usize, Some((left, top, right, bottom, inch)))
    } else {
        (0usize, None)
    };

    // How big to play it back. With a placeable header the answer is the
    // rectangle at ninety-six pixels to the inch, which is what a screen has;
    // without one, there is nothing to go on until the window is stated, so a
    // square of a size nobody will mistake for a picture is used and the
    // window sets the mapping inside it.
    let (width, height, scale) = match bounds {
        Some((left, top, right, bottom, inch)) => {
            let across = (i32::from(right) - i32::from(left)).unsigned_abs() as f32;
            let down = (i32::from(bottom) - i32::from(top)).unsigned_abs() as f32;
            let scale = PER_INCH / f32::from(inch);
            (
                (across * scale).round().max(1.0) as usize,
                (down * scale).round().max(1.0) as usize,
                scale,
            )
        }
        None => (512, 512, 1.0),
    };
    check_size(width, height)?;

    // Past the metafile header, which is nine words of nothing this needs.
    at += 18;

    let mut state = State::new(width, height);
    let origin = bounds.map_or((0.0, 0.0), |(left, top, ..)| (f32::from(left), f32::from(top)));
    state.transform = Transform::translate(-origin.0, -origin.1)
        .then(&Transform::scale(scale, scale))
        .then(&Transform::translate(HALF, HALF));

    let mut window = (origin.0, origin.1, width as f32 / scale, height as f32 / scale);
    let mut viewport = (0.0f32, 0.0f32, width as f32, height as f32);
    let mut mapped = false;

    while at + 6 <= data.len() {
        // The length is in words and takes in the length and the function.
        let size = long(data, at).ok_or(Error::Truncated)? as usize * 2;
        let function = word(data, at + 4).ok_or(Error::Truncated)?;
        if size < 6 || at + size > data.len() {
            break;
        }
        // The parameters, which begin after the length and the function.
        let body = &data[at + 6..at + size];

        match function {
            // The end of the metafile.
            0x0000 => break,
            // The two rectangles that map logical coordinates onto the page.
            // Both are written the far number first.
            0x020B => {
                window.1 = f32::from(short(body, 0).unwrap_or(0));
                window.0 = f32::from(short(body, 2).unwrap_or(0));
                mapped = true;
            }
            0x020C => {
                window.3 = f32::from(short(body, 0).unwrap_or(1));
                window.2 = f32::from(short(body, 2).unwrap_or(1));
                mapped = true;
            }
            0x020D => {
                viewport.1 = f32::from(short(body, 0).unwrap_or(0));
                viewport.0 = f32::from(short(body, 2).unwrap_or(0));
                mapped = true;
            }
            0x020E => {
                viewport.3 = f32::from(short(body, 0).unwrap_or(1));
                viewport.2 = f32::from(short(body, 2).unwrap_or(1));
                mapped = true;
            }
            // Which parts of a shape are inside it.
            0x0106 => {
                state.rule = if word(body, 0) == Some(1) { Rule::Nonzero } else { Rule::EvenOdd };
            }
            // A pen: a style, a width given as a point, and a colour.
            0x02FA => {
                let style = u32::from(word(body, 0).unwrap_or(0));
                let width = f32::from(short(body, 2).unwrap_or(1)).abs().max(1.0);
                let colour = colour_of(long(body, 6).unwrap_or(0));
                // The width is in logical units, and is brought into pixels
                // when it is drawn with rather than here: the mapping may
                // change between now and then.
                state.add(Object::Pen(pen_of(style, width, colour)));
            }
            // The colour words are drawn in.
            0x0209 => {
                state.words.colour = colour_of(long(body, 0).unwrap_or(0));
            }
            // Where the point a word record gives sits against the words.
            0x012E => {
                state.words.align = Alignment(word(body, 0).unwrap_or(0));
            }
            // A face: how tall, how far round, how heavy, and its name. The
            // name is the last of it and is as long as it is, up to the
            // thirty-two bytes the format allows.
            0x02FB => {
                let height = f32::from(short(body, 0).unwrap_or(0));
                let escapement = i32::from(short(body, 4).unwrap_or(0));
                let weight = short(body, 8).unwrap_or(400);
                let italic = body.get(10).copied().unwrap_or(0) != 0;
                let family: String = body
                    .get(18..)
                    .unwrap_or_default()
                    .iter()
                    .take_while(|byte| **byte != 0)
                    .map(|byte| char::from(*byte))
                    .collect();
                state.add(Object::Font(LogFont {
                    family,
                    // A height the file states as a positive number is the
                    // whole line and not the letter; the letter is what a font
                    // is asked for in, and four fifths of a line is near
                    // enough what the difference comes to.
                    size: if height < 0.0 { -height } else { height * 0.8 },
                    bold: weight >= 600,
                    italic,
                    escapement,
                }));
            }
            // Words at a point: how many of them, then the words themselves.
            0x0521 => {
                let count = usize::from(word(body, 0).unwrap_or(0));
                let text = latin(body.get(2..2 + count).unwrap_or_default());
                // The point comes after the words, and is written the second
                // number first as every point in this format is.
                let at = 2 + count + (count & 1);
                let y = f32::from(short(body, at).unwrap_or(0));
                let x = f32::from(short(body, at + 2).unwrap_or(0));
                state.draw_words(faces, x, y, &text);
            }
            // The same with a rectangle round it and a list of widths, neither
            // of which changes where the words go.
            0x0A32 => {
                let y = f32::from(short(body, 0).unwrap_or(0));
                let x = f32::from(short(body, 2).unwrap_or(0));
                let count = usize::from(word(body, 4).unwrap_or(0));
                let options = word(body, 6).unwrap_or(0);
                // The rectangle is there only when the options ask for one.
                let at = 8 + if options & 0x0006 == 0 { 0 } else { 8 };
                let text = latin(body.get(at..at + count).unwrap_or_default());
                state.draw_words(faces, x, y, &text);
            }
            // A brush: a style, a colour, and a hatch nothing here draws.
            0x02FC => {
                let style = u32::from(word(body, 0).unwrap_or(0));
                let colour = colour_of(long(body, 2).unwrap_or(0));
                state.add(Object::Brush(brush_of(style, colour)));
            }
            0x012D => state.select(usize::from(word(body, 0).unwrap_or(0))),
            0x01F0 => {
                let index = usize::from(word(body, 0).unwrap_or(0));
                if let Some(slot) = state.objects.get_mut(index) {
                    *slot = Object::Empty;
                }
            }
            // Where a line starts, and a line from there to somewhere. Both
            // are written down first and across second.
            0x0214 => {
                state.at = Point::new(
                    f32::from(short(body, 2).unwrap_or(0)),
                    f32::from(short(body, 0).unwrap_or(0)),
                );
            }
            0x0213 => {
                let to = Point::new(
                    f32::from(short(body, 2).unwrap_or(0)),
                    f32::from(short(body, 0).unwrap_or(0)),
                );
                let mut path = Path::new();
                path.move_to(state.place(state.at.x, state.at.y));
                path.line_to(state.place(to.x, to.y));
                state.stroke_only(&path);
                state.at = to;
            }
            // Runs of points, which here are written the ordinary way round.
            0x0325 | 0x0324 => {
                let count = usize::from(word(body, 0).unwrap_or(0)).min(1 << 16);
                let mut path = Path::new();
                for index in 0..count {
                    let at = 2 + index * 4;
                    let (Some(x), Some(y)) = (short(body, at), short(body, at + 2)) else { break };
                    let point = state.place(f32::from(x), f32::from(y));
                    if index == 0 {
                        path.move_to(point);
                    } else {
                        path.line_to(point);
                    }
                }
                if function == 0x0324 {
                    path.close();
                    state.draw(&path);
                } else {
                    state.stroke_only(&path);
                }
            }
            // Several shapes at once, which is how one with a hole is written.
            0x0538 => {
                let shapes = usize::from(word(body, 0).unwrap_or(0)).min(4096);
                let counts: Vec<usize> = (0..shapes)
                    .filter_map(|index| word(body, 2 + index * 2).map(usize::from))
                    .collect();
                let start = 2 + shapes * 2;
                let mut taken = 0usize;
                let mut path = Path::new();
                for count in counts {
                    for index in 0..count {
                        let at = start + (taken + index) * 4;
                        let (Some(x), Some(y)) = (short(body, at), short(body, at + 2)) else {
                            break;
                        };
                        let point = state.place(f32::from(x), f32::from(y));
                        if index == 0 {
                            path.move_to(point);
                        } else {
                            path.line_to(point);
                        }
                    }
                    path.close();
                    taken += count;
                }
                state.draw(&path);
            }
            // The shapes that are a rectangle and something done to it. All
            // written bottom, right, top, left.
            0x041B => {
                let (left, top, right, bottom) = corners(&state, body, 0);
                let mut path = Path::new();
                path.move_to(Point::new(left, top));
                path.line_to(Point::new(right, top));
                path.line_to(Point::new(right, bottom));
                path.line_to(Point::new(left, bottom));
                path.close();
                state.draw(&path);
            }
            0x0418 => {
                let (left, top, right, bottom) = corners(&state, body, 0);
                state.draw(&ellipse(left, top, right, bottom));
            }
            0x061C => {
                // The two that say how round the corners are come first, and
                // the rectangle after them.
                let across = f32::from(short(body, 2).unwrap_or(0)) * scale;
                let down = f32::from(short(body, 0).unwrap_or(0)) * scale;
                let (left, top, right, bottom) = corners(&state, body, 4);
                state.draw(&rounded(left, top, right, bottom, across, down));
            }
            // One pixel of one colour, which a drawing uses for a dot.
            0x041F => {
                let colour = colour_of(long(body, 0).unwrap_or(0));
                let point = state.place(
                    f32::from(short(body, 6).unwrap_or(0)),
                    f32::from(short(body, 4).unwrap_or(0)),
                );
                state.canvas.fill_rect(point.x as i32, point.y as i32, 1, 1, colour);
            }
            // Anything else is something this does not draw.
            _ => {}
        }

        if mapped {
            state.transform = mapping(window, viewport, origin, scale);
            mapped = false;
        }

        at += size;
    }

    let canvas = state.canvas;
    Ok(Image { width: canvas.width(), height: canvas.height(), pixels: canvas.pixels().to_vec() })
}

/// The rectangle a record states, where it lands on the canvas.
///
/// Written bottom, right, top, left — the parameters in the order they were
/// pushed, which is the reverse of the order they were written.
fn corners(state: &State, body: &[u8], at: usize) -> (f32, f32, f32, f32) {
    let bottom = f32::from(short(body, at).unwrap_or(0));
    let right = f32::from(short(body, at + 2).unwrap_or(0));
    let top = f32::from(short(body, at + 4).unwrap_or(0));
    let left = f32::from(short(body, at + 6).unwrap_or(0));
    let one = state.place(left, top);
    let other = state.place(right, bottom);
    (one.x, one.y, other.x, other.y)
}

/// The mapping from logical coordinates onto the canvas.
fn mapping(
    window: (f32, f32, f32, f32),
    viewport: (f32, f32, f32, f32),
    origin: (f32, f32),
    scale: f32,
) -> Transform {
    let across = if window.2 == 0.0 { 1.0 } else { viewport.2 / window.2 };
    let down = if window.3 == 0.0 { 1.0 } else { viewport.3 / window.3 };
    let _ = origin;
    Transform::translate(-window.0, -window.1)
        .then(&Transform::scale(across * scale, down * scale))
        .then(&Transform::translate(viewport.0 * scale, viewport.1 * scale))
        .then(&Transform::translate(HALF, HALF))
}

/// The letters of a string the older format wrote, which states them one byte
/// each.
///
/// Read as Latin-1, which is what a byte in a metafile means when nothing says
/// which code page it was written in — and nothing usually does.
fn latin(bytes: &[u8]) -> String {
    bytes.iter().take_while(|byte| **byte != 0).map(|byte| char::from(*byte)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a placeable metafile covering a rectangle of logical units, at a
    /// thousand four hundred and forty of them to the inch.
    fn wmf(across: i16, down: i16, inch: u16, records: &[Vec<u8>]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&PLACEABLE);
        out.extend_from_slice(&0u16.to_le_bytes()); // The handle, which is nothing.
        out.extend_from_slice(&0i16.to_le_bytes()); // The rectangle:
        out.extend_from_slice(&0i16.to_le_bytes());
        out.extend_from_slice(&across.to_le_bytes());
        out.extend_from_slice(&down.to_le_bytes());
        out.extend_from_slice(&inch.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes()); // Reserved.
        out.extend_from_slice(&0u16.to_le_bytes()); // The checksum, unchecked.

        out.extend_from_slice(&1u16.to_le_bytes()); // A metafile in memory,
        out.extend_from_slice(&9u16.to_le_bytes()); // nine words of header,
        out.extend_from_slice(&0x0300u16.to_le_bytes()); // version three.
        out.extend_from_slice(&0u32.to_le_bytes()); // Its size, unread.
        out.extend_from_slice(&0u16.to_le_bytes()); // How many objects.
        out.extend_from_slice(&0u32.to_le_bytes()); // The longest record.
        out.extend_from_slice(&0u16.to_le_bytes()); // How many members.

        for record in records {
            out.extend_from_slice(record);
        }
        out.extend_from_slice(&record(0x0000, &[]));
        out
    }

    /// One record: a length in words, a function, and the parameters.
    fn record(function: u16, body: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let words = (body.len() + 6) / 2;
        out.extend_from_slice(&(words as u32).to_le_bytes());
        out.extend_from_slice(&function.to_le_bytes());
        out.extend_from_slice(body);
        out
    }

    fn shorts(values: &[i16]) -> Vec<u8> {
        values.iter().flat_map(|value| value.to_le_bytes()).collect()
    }

    fn pixel(image: &Image, x: usize, y: usize) -> [u8; 4] {
        let at = (y * image.width + x) * 4;
        [image.pixels[at], image.pixels[at + 1], image.pixels[at + 2], image.pixels[at + 3]]
    }

    #[test]
    fn anything_else_is_not_a_metafile() {
        assert_eq!(decode(b"not a metafile at all, no"), Err(Error::UnknownFormat));
    }

    #[test]
    fn the_placeable_header_says_how_big_the_picture_is() {
        // Ninety-six units to the inch and a rectangle of 96 by 48: one inch
        // by half an inch, which at ninety-six pixels to the inch is 96 by 48.
        let data = wmf(96, 48, 96, &[]);
        let image = decode(&data).expect("a picture");
        assert_eq!((image.width, image.height), (96, 48));
    }

    #[test]
    fn the_units_to_the_inch_are_believed() {
        // The same rectangle at twice as many units to the inch is half the
        // size on the page.
        let data = wmf(96, 48, 192, &[]);
        let image = decode(&data).expect("a picture");
        assert_eq!((image.width, image.height), (48, 24));
    }

    #[test]
    fn a_rectangle_is_written_backwards_and_read_that_way() {
        // Bottom, right, top, left — the order they were pushed in.
        let brush = record(0x02FC, &[0, 0, 0xFF, 0, 0, 0, 0, 0]);
        let take = record(0x012D, &shorts(&[0]));
        let shape = record(0x041B, &shorts(&[14, 14, 2, 2]));
        let data = wmf(16, 16, 96, &[brush, take, shape]);

        let image = decode(&data).expect("a picture");
        assert_eq!(pixel(&image, 8, 8), [255, 0, 0, 255], "the fill");
        assert_eq!(pixel(&image, 0, 0), [255, 255, 255, 255], "and the paper round it");
    }

    #[test]
    fn a_pen_and_a_brush_go_into_slots_of_their_own() {
        let brush = record(0x02FC, &[0, 0, 0xFF, 0, 0, 0, 0, 0]);
        let pen = record(0x02FA, &[0, 0, 1, 0, 0, 0, 0, 0, 0xFF, 0, 0, 0]);
        let take_brush = record(0x012D, &shorts(&[0]));
        let take_pen = record(0x012D, &shorts(&[1]));
        let shape = record(0x041B, &shorts(&[14, 14, 2, 2]));
        let data = wmf(16, 16, 96, &[brush, pen, take_brush, take_pen, shape]);

        let image = decode(&data).expect("a picture");
        assert_eq!(pixel(&image, 8, 8), [255, 0, 0, 255], "the brush fills");
        assert_eq!(pixel(&image, 2, 8), [0, 0, 255, 255], "and the pen outlines");
    }

    #[test]
    fn a_line_goes_from_where_the_last_one_ended() {
        let pen = record(0x02FA, &[0, 0, 1, 0, 0, 0, 0xFF, 0, 0, 0, 0, 0]);
        let take = record(0x012D, &shorts(&[0]));
        // Down first, across second.
        let start = record(0x0214, &shorts(&[8, 2]));
        let line = record(0x0213, &shorts(&[8, 14]));
        let data = wmf(16, 16, 96, &[pen, take, start, line]);

        let image = decode(&data).expect("a picture");
        assert_eq!(pixel(&image, 8, 8), [255, 0, 0, 255], "the line");
        assert_eq!(pixel(&image, 8, 2), [255, 255, 255, 255], "and not above it");
    }

    #[test]
    fn a_polygon_is_filled() {
        let brush = record(0x02FC, &[0, 0, 0xFF, 0, 0, 0, 0, 0]);
        let take = record(0x012D, &shorts(&[0]));
        let mut body = shorts(&[3]);
        body.extend(shorts(&[2, 2, 14, 2, 8, 14]));
        let shape = record(0x0324, &body);
        let data = wmf(16, 16, 96, &[brush, take, shape]);

        let image = decode(&data).expect("a picture");
        assert_eq!(pixel(&image, 8, 6), [255, 0, 0, 255], "inside the triangle");
        assert_eq!(pixel(&image, 1, 13), [255, 255, 255, 255], "and outside it");
    }

    #[test]
    fn one_pixel_can_be_set_on_its_own() {
        let dot = record(0x041F, &[0, 0xFF, 0, 0, 6, 0, 4, 0]);
        let data = wmf(16, 16, 96, &[dot]);
        let image = decode(&data).expect("a picture");
        assert_eq!(pixel(&image, 4, 6), [0, 255, 0, 255]);
    }

    #[test]
    fn the_end_of_the_metafile_stops_the_reading() {
        // A record after the end is never reached.
        let ending = record(0x0000, &[]);
        let brush = record(0x02FC, &[0, 0, 0xFF, 0, 0, 0, 0, 0]);
        let take = record(0x012D, &shorts(&[0]));
        let shape = record(0x041B, &shorts(&[14, 14, 2, 2]));
        let data = wmf(16, 16, 96, &[ending, brush, take, shape]);

        let image = decode(&data).expect("a picture");
        assert_eq!(pixel(&image, 8, 8), [255, 255, 255, 255], "nothing after the end");
    }

    #[test]
    fn a_metafile_with_no_placeable_header_is_still_read() {
        let mut out = Vec::new();
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&9u16.to_le_bytes());
        out.extend_from_slice(&0x0300u16.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&record(0x0000, &[]));

        assert!(is_wmf(&out), "it should be recognised without the header");
        let image = decode(&out).expect("a picture");
        assert!(image.width > 0 && image.height > 0);
    }
}
