//! Reading the enhanced metafile, to MS-EMF.
//!
//! # The shape of the format
//!
//! A list of records, each a type and a length and nothing else to find it by.
//! The first is the header, which says how big the picture is twice over — in
//! pixels of whatever screen recorded it, and in hundredths of a millimetre of
//! the paper it stands for. The rest are the drawing.
//!
//! # Why the coordinates take three transforms
//!
//! Because there are three. A record states a point in *logical* units; the
//! world transform moves it, the window and viewport map it onto the device,
//! and the device is a screen whose size the header states. Any of the three
//! may be the identity, and in most files two of them are — but a file that
//! sets a world transform and is read as though it had not is a file drawn
//! somewhere else entirely.
//!
//! # What is not read
//!
//! Text, which is its own piece of work and is the roadmap's **D11**. Clipping
//! regions, which a diagram rarely uses. And the records for things that are
//! not drawing at all: the palette, the colour space, the comments a program
//! leaves for itself.

use wp_raster::{Color, Path, Point, Rule, Transform};

use crate::metafile::{
    brush_of, colour_of, ellipse, pen_of, rounded, Alignment, Faces, LogFont, Object, State,
};
use crate::{check_size, Error, Image};

/// The four bytes the header carries to say what it is.
pub const SIGNATURE: [u8; 4] = *b" EMF";

/// Where the signature sits in the file: forty bytes into the first record.
const SIGNATURE_AT: usize = 40;

/// Half a pixel, which is the difference between naming a pixel and naming the
/// corner between four of them. See where the transform is set up.
const HALF: f32 = 0.5;

/// How many pixels a metafile is played back at to the inch.
const PER_INCH: f32 = 96.0;

/// Hundredths of a millimetre to the inch, which is the unit the frame is in.
const MM_PER_INCH_HUNDREDTHS: f32 = 2540.0;

/// The four edges of a rectangle the header states.
#[derive(Clone, Copy, Debug)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

/// Whether these bytes are an enhanced metafile.
#[must_use]
pub fn is_emf(data: &[u8]) -> bool {
    // The first record is the header, whose type is one; the signature four
    // words in is what tells it from anything else that starts with a one.
    data.len() > SIGNATURE_AT + 4
        && number(data, 0) == Some(1)
        && data[SIGNATURE_AT..SIGNATURE_AT + 4] == SIGNATURE
}

/// A little-endian word, or nothing when the file ends first.
fn number(data: &[u8], at: usize) -> Option<u32> {
    let bytes = data.get(at..at + 4)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn signed(data: &[u8], at: usize) -> Option<i32> {
    number(data, at).map(|value| value as i32)
}

fn short(data: &[u8], at: usize) -> Option<i16> {
    let bytes = data.get(at..at + 2)?;
    Some(i16::from_le_bytes([bytes[0], bytes[1]]))
}

fn float(data: &[u8], at: usize) -> Option<f32> {
    number(data, at).map(f32::from_bits)
}

/// A run of two-byte letters, which is how the newer format writes a name and
/// the words it draws.
///
/// Stops at the first nothing, because a name is written into a field of a
/// fixed size and what follows the name is that.
fn wide(data: &[u8], at: usize, count: usize) -> String {
    let mut out = String::new();
    for index in 0..count {
        let Some(bytes) = data.get(at + index * 2..at + index * 2 + 2) else {
            break;
        };
        let value = u16::from_le_bytes([bytes[0], bytes[1]]);
        if value == 0 {
            break;
        }
        out.push(char::from_u32(u32::from(value)).unwrap_or('?'));
    }
    out
}

/// Plays an enhanced metafile back into pixels.
pub fn decode(data: &[u8]) -> Result<Image, Error> {
    decode_with(data, &crate::metafile::NoFaces)
}

/// The same, with somewhere to get letter shapes from.
///
/// A caller with no fonts to offer gets the picture without its words. See
/// [`crate::metafile::Faces`].
pub fn decode_with(data: &[u8], faces: &dyn Faces) -> Result<Image, Error> {
    if !is_emf(data) {
        return Err(Error::UnknownFormat);
    }

    // How big the picture is. The header says it twice: the *bounds* are where
    // the drawing reached, in the pixels of whatever screen recorded it, and
    // the *frame* is the piece of paper it stands for, in hundredths of a
    // millimetre. The frame is the one to believe — a drawing that leaves a
    // margin round itself has bounds smaller than the picture, and playing it
    // back at the bounds would crop the margin off.
    let frame = Rect {
        left: signed(data, 24).ok_or(Error::Truncated)?,
        top: signed(data, 28).ok_or(Error::Truncated)?,
        right: signed(data, 32).ok_or(Error::Truncated)?,
        bottom: signed(data, 36).ok_or(Error::Truncated)?,
    };
    let width = ((frame.right - frame.left).abs() as f32 * PER_INCH / MM_PER_INCH_HUNDREDTHS)
        .round()
        .max(1.0) as usize;
    let height = ((frame.bottom - frame.top).abs() as f32 * PER_INCH / MM_PER_INCH_HUNDREDTHS)
        .round()
        .max(1.0) as usize;
    check_size(width, height)?;

    // A record's coordinates come out in the device's pixels, and the device
    // was a screen of its own size. How many of its pixels go to the inch is
    // what the header's last two pairs say, and that is what turns them into
    // pixels of the canvas.
    let device_across = signed(data, 72).unwrap_or(0) as f32;
    let device_down = signed(data, 76).unwrap_or(0) as f32;
    let millimetres_across = signed(data, 80).unwrap_or(0) as f32;
    let millimetres_down = signed(data, 84).unwrap_or(0) as f32;
    let per_device = |pixels: f32, millimetres: f32| -> f32 {
        if pixels <= 0.0 || millimetres <= 0.0 {
            return 1.0;
        }
        // One device pixel is this many of ours.
        PER_INCH * millimetres / (pixels * 25.4)
    };
    let scale_x = per_device(device_across, millimetres_across);
    let scale_y = per_device(device_down, millimetres_down);

    // Where the frame's corner is in the device's own pixels, which is what
    // the drawing is measured from.
    let corner_x = frame.left as f32 / MM_PER_INCH_HUNDREDTHS * PER_INCH / scale_x;
    let corner_y = frame.top as f32 / MM_PER_INCH_HUNDREDTHS * PER_INCH / scale_y;

    let mut state = State::new(width, height);
    // And half a pixel besides: a coordinate in these formats names a pixel,
    // where a coordinate in a path names the corner between four of them.
    // Without the half, a line one pixel wide straddles two rows and comes out
    // grey in both.
    let device = Transform::translate(-corner_x, -corner_y)
        .then(&Transform::scale(scale_x, scale_y))
        .then(&Transform::translate(HALF, HALF));
    state.transform = device;

    // Where the window and the viewport stand, which together are the other
    // mapping a file can state.
    let mut window = (0.0f32, 0.0f32, 1.0f32, 1.0f32);
    let mut viewport = (0.0f32, 0.0f32, 1.0f32, 1.0f32);
    let mut mapped = false;

    let mut at = 0usize;
    while at + 8 <= data.len() {
        let kind = number(data, at).ok_or(Error::Truncated)?;
        let size = number(data, at + 4).ok_or(Error::Truncated)? as usize;
        if size < 8 || at + size > data.len() {
            break;
        }
        let body = &data[at..at + size];

        match kind {
            // The header, which has been read already.
            1 => {}
            // Where the drawing goes: the two rectangles that map logical
            // coordinates onto the device.
            9 => {
                window.2 = signed(body, 8).unwrap_or(1) as f32;
                window.3 = signed(body, 12).unwrap_or(1) as f32;
                mapped = true;
            }
            10 => {
                window.0 = signed(body, 8).unwrap_or(0) as f32;
                window.1 = signed(body, 12).unwrap_or(0) as f32;
                mapped = true;
            }
            11 => {
                viewport.2 = signed(body, 8).unwrap_or(1) as f32;
                viewport.3 = signed(body, 12).unwrap_or(1) as f32;
                mapped = true;
            }
            12 => {
                viewport.0 = signed(body, 8).unwrap_or(0) as f32;
                viewport.1 = signed(body, 12).unwrap_or(0) as f32;
                mapped = true;
            }
            // The colour words are drawn in, and where the point a word record
            // gives sits against them.
            24 => state.words.colour = colour_of(number(body, 8).unwrap_or(0)),
            22 => state.words.align = Alignment(number(body, 8).unwrap_or(0) as u16),
            // A face. The newer format writes the same description the older
            // one does, in the same order, but with room for a name of
            // thirty-two characters of two bytes each.
            82 => {
                let height = signed(body, 12).unwrap_or(0) as f32;
                let escapement = signed(body, 20).unwrap_or(0);
                let weight = signed(body, 28).unwrap_or(400);
                let italic = body.get(32).copied().unwrap_or(0) != 0;
                let family = wide(body, 40, 32);
                state.add(Object::Font(LogFont {
                    family,
                    // Negative is the height of a letter and positive the
                    // height of the whole line; the letter is what a font is
                    // asked for in.
                    size: if height < 0.0 { -height } else { height * 0.8 },
                    bold: weight >= 600,
                    italic,
                    escapement,
                }));
            }
            // Words at a point, written one byte a letter or two. Everything
            // about the two records is the same but that.
            83 | 84 => {
                // The record holds a description of the run, which holds where
                // the words are and how many: the offsets in it are from the
                // start of the record.
                let x = signed(body, 36).unwrap_or(0) as f32;
                let y = signed(body, 40).unwrap_or(0) as f32;
                let count = number(body, 44).unwrap_or(0) as usize;
                let offset = number(body, 48).unwrap_or(0) as usize;
                let text = if kind == 84 {
                    wide(body, offset, count)
                } else {
                    body.get(offset..offset + count)
                        .unwrap_or_default()
                        .iter()
                        .map(|byte| char::from(*byte))
                        .collect()
                };
                state.draw_words(faces, x, y, &text);
            }
            // The transform the file states on top of everything else.
            35 => state.world = read_transform(body, 8),
            36 => {
                let stated = read_transform(body, 8);
                // What to do with it: forget everything, apply it before what
                // is there, apply it after, or replace what is there outright.
                state.world = match number(body, 32).unwrap_or(4) {
                    1 => Transform::IDENTITY,
                    2 => stated.then(&state.world),
                    3 => state.world.then(&stated),
                    _ => stated,
                };
            }
            // Which parts of a shape are inside it.
            19 => {
                state.rule = if number(body, 8) == Some(2) { Rule::Nonzero } else { Rule::EvenOdd };
            }
            // The objects, and taking one up.
            38 => {
                let index = number(body, 8).unwrap_or(0) as usize;
                let style = number(body, 12).unwrap_or(0);
                let width = signed(body, 16).unwrap_or(1) as f32;
                let colour = colour_of(number(body, 24).unwrap_or(0));
                put(&mut state, index, Object::Pen(pen_of(style, width, colour)));
            }
            // The same pen said at greater length, which is what a program
            // that draws through the newer interface writes. Five words come
            // between the number and the pen itself: they are for a pen made
            // out of a picture, which this does not draw.
            95 => {
                let index = number(body, 8).unwrap_or(0) as usize;
                let style = number(body, 28).unwrap_or(0);
                let width = number(body, 32).unwrap_or(1) as f32;
                let colour = colour_of(number(body, 40).unwrap_or(0));
                put(&mut state, index, Object::Pen(pen_of(style, width, colour)));
            }
            39 => {
                let index = number(body, 8).unwrap_or(0) as usize;
                let style = number(body, 12).unwrap_or(0);
                let colour = colour_of(number(body, 16).unwrap_or(0));
                put(&mut state, index, Object::Brush(brush_of(style, colour)));
            }
            37 => {
                let index = number(body, 8).unwrap_or(0);
                // The numbers with the top bit set are the handful of objects
                // the system provides rather than the file: the black and white
                // brushes, the hollow one, and the black and white pens.
                if index & 0x8000_0000 != 0 {
                    stock(&mut state, index & 0x7FFF_FFFF);
                } else {
                    state.select(index as usize);
                }
            }
            40 => {
                let index = number(body, 8).unwrap_or(0) as usize;
                if let Some(slot) = state.objects.get_mut(index) {
                    *slot = Object::Empty;
                }
            }
            // Where a line starts, and a line from there to somewhere.
            27 => {
                state.at = Point::new(
                    signed(body, 8).unwrap_or(0) as f32,
                    signed(body, 12).unwrap_or(0) as f32,
                );
            }
            54 => {
                let to = Point::new(
                    signed(body, 8).unwrap_or(0) as f32,
                    signed(body, 12).unwrap_or(0) as f32,
                );
                let mut path = Path::new();
                path.move_to(state.place(state.at.x, state.at.y));
                path.line_to(state.place(to.x, to.y));
                state.stroke_only(&path);
                state.at = to;
            }
            // Runs of points: a line through them, a shape of them, or several
            // shapes at once — each in whole words or in halves.
            2 | 4 | 85 | 87 => {
                let points = read_points(body, kind == 85 || kind == 87);
                let curved = kind == 2 || kind == 85;
                let path = through(&state, &points, curved, false);
                state.stroke_only(&path);
            }
            3 | 86 => {
                let points = read_points(body, kind == 86);
                let path = through(&state, &points, false, true);
                state.draw(&path);
            }
            8 | 91 => {
                let path = read_many(&state, body, kind == 91);
                state.draw(&path);
            }
            // The shapes that are a rectangle and something done to it.
            43 => {
                let path = rectangle_of(&state, body);
                state.draw(&path);
            }
            42 => {
                let (left, top, right, bottom) = corners(&state, body);
                state.draw(&ellipse(left, top, right, bottom));
            }
            44 => {
                let (left, top, right, bottom) = corners(&state, body);
                // How round the corners are, which the record states as the
                // width and height of the ellipse they are a quarter of.
                let across = signed(body, 24).unwrap_or(0) as f32;
                let down = signed(body, 28).unwrap_or(0) as f32;
                let scale = scale_of(&state);
                state.draw(&rounded(left, top, right, bottom, across * scale.0, down * scale.1));
            }
            // Building a path rather than drawing one, and then what to do
            // with what was built.
            59 => state.path = Some(Path::new()),
            60 => {}
            61 => {
                if let Some(path) = &mut state.path {
                    path.close();
                }
            }
            62 => {
                if let Some(path) = state.path.take() {
                    let brush = state.brush;
                    let pen = state.pen;
                    state.pen = None;
                    state.fill_and_stroke(&path);
                    state.brush = brush;
                    state.pen = pen;
                }
            }
            63 => {
                if let Some(path) = state.path.take() {
                    state.fill_and_stroke(&path);
                }
            }
            64 => {
                if let Some(path) = state.path.take() {
                    let brush = state.brush;
                    state.brush = None;
                    state.fill_and_stroke(&path);
                    state.brush = brush;
                }
            }
            // Anything else is something this does not draw.
            _ => {}
        }

        // The mapping is worked out afresh whenever either rectangle moves.
        if mapped {
            state.transform = mapping(window, viewport).then(&device);
            mapped = false;
        }

        at += size;
    }

    let canvas = state.canvas;
    Ok(Image {
        width: canvas.pixel_width(),
        height: canvas.pixel_height(),
        pixels: canvas.pixels().to_vec(),
    })
}

/// Puts an object in the slot the file names.
///
/// The newer format numbers its objects outright rather than by where they
/// went, so a slot may be filled long before the ones before it are.
fn put(state: &mut State, index: usize, object: Object) {
    if index > 4096 {
        return;
    }
    if state.objects.len() <= index {
        state.objects.resize(index + 1, Object::Empty);
    }
    state.objects[index] = object;
}

/// Takes up one of the objects the system provides rather than the file.
fn stock(state: &mut State, index: u32) {
    match index {
        0 => state.brush = Some(Color::WHITE),
        1 => state.brush = Some(Color::rgb(0xC0, 0xC0, 0xC0)),
        2 => state.brush = Some(Color::rgb(0x80, 0x80, 0x80)),
        3 => state.brush = Some(Color::rgb(0x40, 0x40, 0x40)),
        4 => state.brush = Some(Color::BLACK),
        5 => state.brush = None,
        6 => state.pen = Some((Color::WHITE, 1.0)),
        7 => state.pen = Some((Color::BLACK, 1.0)),
        8 => state.pen = None,
        _ => {}
    }
}

/// The transform a record states: six numbers, in the order the format writes
/// them.
fn read_transform(body: &[u8], at: usize) -> Transform {
    Transform {
        a: float(body, at).unwrap_or(1.0),
        b: float(body, at + 4).unwrap_or(0.0),
        c: float(body, at + 8).unwrap_or(0.0),
        d: float(body, at + 12).unwrap_or(1.0),
        e: float(body, at + 16).unwrap_or(0.0),
        f: float(body, at + 20).unwrap_or(0.0),
    }
}

/// How far a logical unit goes on the canvas, across and down.
fn scale_of(state: &State) -> (f32, f32) {
    let origin = state.place(0.0, 0.0);
    let along = state.place(1.0, 0.0);
    let down = state.place(0.0, 1.0);
    ((along.x - origin.x).abs().max(0.0001), (down.y - origin.y).abs().max(0.0001))
}

/// The mapping from logical coordinates onto the device's own pixels.
///
/// What turns those into pixels of the canvas is the transform worked out from
/// the header, which this is followed by.
fn mapping(window: (f32, f32, f32, f32), viewport: (f32, f32, f32, f32)) -> Transform {
    let across = if window.2 == 0.0 { 1.0 } else { viewport.2 / window.2 };
    let down = if window.3 == 0.0 { 1.0 } else { viewport.3 / window.3 };
    Transform::translate(-window.0, -window.1)
        .then(&Transform::scale(across, down))
        .then(&Transform::translate(viewport.0, viewport.1))
}

/// The four corners a record states, where they land on the canvas.
fn corners(state: &State, body: &[u8]) -> (f32, f32, f32, f32) {
    let one =
        state.place(signed(body, 8).unwrap_or(0) as f32, signed(body, 12).unwrap_or(0) as f32);
    let other =
        state.place(signed(body, 16).unwrap_or(0) as f32, signed(body, 20).unwrap_or(0) as f32);
    (one.x, one.y, other.x, other.y)
}

fn rectangle_of(state: &State, body: &[u8]) -> Path {
    let (left, top, right, bottom) = corners(state, body);
    let mut path = Path::new();
    path.move_to(Point::new(left, top));
    path.line_to(Point::new(right, top));
    path.line_to(Point::new(right, bottom));
    path.line_to(Point::new(left, bottom));
    path.close();
    path
}

/// The points a record carries, in whole words or in halves.
fn read_points(body: &[u8], halves: bool) -> Vec<(f32, f32)> {
    // The bounds come first, then how many points, then the points.
    let count = number(body, 24).unwrap_or(0) as usize;
    let start = 28usize;
    let step = if halves { 4 } else { 8 };
    (0..count.min(1 << 20))
        .filter_map(|index| {
            let at = start + index * step;
            if halves {
                Some((f32::from(short(body, at)?), f32::from(short(body, at + 2)?)))
            } else {
                Some((signed(body, at)? as f32, signed(body, at + 4)? as f32))
            }
        })
        .collect()
}

/// A path through a run of points, straight or curved, open or closed.
fn through(state: &State, points: &[(f32, f32)], curved: bool, closed: bool) -> Path {
    let mut path = Path::new();
    let mut placed = points.iter().map(|(x, y)| state.place(*x, *y));
    let Some(first) = placed.next() else { return path };
    path.move_to(first);

    if curved {
        // The points after the first come in threes: two that pull the curve
        // and one it passes through.
        let rest: Vec<Point> = placed.collect();
        for group in rest.chunks(3) {
            if let [one, two, three] = group {
                path.cubic_to(*one, *two, *three);
            } else if let Some(last) = group.last() {
                path.line_to(*last);
            }
        }
    } else {
        for point in placed {
            path.line_to(point);
        }
    }
    if closed {
        path.close();
    }
    path
}

/// Several shapes in one record, which is how a shape with a hole is written.
fn read_many(state: &State, body: &[u8], halves: bool) -> Path {
    let shapes = number(body, 24).unwrap_or(0) as usize;
    let total = number(body, 28).unwrap_or(0) as usize;
    let counts: Vec<usize> = (0..shapes.min(4096))
        .filter_map(|index| number(body, 32 + index * 4).map(|value| value as usize))
        .collect();

    let start = 32 + shapes * 4;
    let step = if halves { 4 } else { 8 };
    let mut path = Path::new();
    let mut taken = 0usize;

    for count in counts {
        let mut first = true;
        for index in 0..count.min(total) {
            let at = start + (taken + index) * step;
            let point = if halves {
                let (Some(x), Some(y)) = (short(body, at), short(body, at + 2)) else { break };
                state.place(f32::from(x), f32::from(y))
            } else {
                let (Some(x), Some(y)) = (signed(body, at), signed(body, at + 4)) else { break };
                state.place(x as f32, y as f32)
            };
            if first {
                path.move_to(point);
                first = false;
            } else {
                path.line_to(point);
            }
        }
        path.close();
        taken += count;
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds an enhanced metafile whose frame is a given size in pixels.
    ///
    /// The device fields are left at nothing, which means one device pixel to
    /// one of the canvas — the simplest thing for a test to reason about, and
    /// what a file that says nothing about its device gets.
    fn emf(width: i32, height: i32, records: &[Vec<u8>]) -> Vec<u8> {
        // The frame is in hundredths of a millimetre, and the picture is played
        // back at ninety-six pixels to the inch.
        let across = (width as f32 * 2540.0 / 96.0).round() as i32;
        let down = (height as f32 * 2540.0 / 96.0).round() as i32;

        let mut header = Vec::new();
        header.extend_from_slice(&1u32.to_le_bytes()); // The header's own type.
        header.extend_from_slice(&88u32.to_le_bytes()); // And its length.
        header.extend_from_slice(&0i32.to_le_bytes()); // The bounds:
        header.extend_from_slice(&0i32.to_le_bytes());
        header.extend_from_slice(&(width - 1).to_le_bytes());
        header.extend_from_slice(&(height - 1).to_le_bytes());
        header.extend_from_slice(&0i32.to_le_bytes()); // The frame:
        header.extend_from_slice(&0i32.to_le_bytes());
        header.extend_from_slice(&across.to_le_bytes());
        header.extend_from_slice(&down.to_le_bytes());
        header.extend_from_slice(&SIGNATURE);
        while header.len() < 88 {
            header.push(0);
        }

        let mut out = header;
        for record in records {
            out.extend_from_slice(record);
        }
        out
    }

    /// One record: a type, a length, and a body.
    fn record(kind: u32, body: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&kind.to_le_bytes());
        out.extend_from_slice(&((body.len() + 8) as u32).to_le_bytes());
        out.extend_from_slice(body);
        out
    }

    fn numbers(values: &[i32]) -> Vec<u8> {
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
    fn a_metafile_is_played_back_onto_paper() {
        // Nothing but a header: a blank sheet the size it says.
        let data = emf(16, 12, &[]);
        let image = decode(&data).expect("a picture");
        assert_eq!((image.width, image.height), (16, 12));
        assert_eq!(pixel(&image, 8, 6), [255, 255, 255, 255], "and white, not nothing");
    }

    #[test]
    fn a_rectangle_is_filled_with_the_brush_and_outlined_with_the_pen() {
        let brush = record(39, &numbers(&[0, 0, 0x00_00_00_FF, 0]));
        let take_brush = record(37, &numbers(&[0]));
        let pen = record(38, &numbers(&[1, 0, 1, 0, 0x00_FF_00_00]));
        let take_pen = record(37, &numbers(&[1]));
        let shape = record(43, &numbers(&[2, 2, 14, 14]));
        let data = emf(16, 16, &[brush, take_brush, pen, take_pen, shape]);

        let image = decode(&data).expect("a picture");
        assert_eq!(pixel(&image, 8, 8), [255, 0, 0, 255], "the fill");
        assert_eq!(pixel(&image, 2, 8), [0, 0, 255, 255], "the outline");
        assert_eq!(pixel(&image, 0, 0), [255, 255, 255, 255], "and the paper");
    }

    #[test]
    fn the_brush_that_fills_nothing_leaves_the_paper_showing() {
        let brush = record(39, &numbers(&[0, 1, 0x00_00_00_FF, 0]));
        let take = record(37, &numbers(&[0]));
        let shape = record(43, &numbers(&[2, 2, 14, 14]));
        let data = emf(16, 16, &[brush, take, shape]);

        let image = decode(&data).expect("a picture");
        assert_eq!(pixel(&image, 8, 8), [255, 255, 255, 255], "nothing should fill it");
    }

    #[test]
    fn a_line_goes_from_where_the_last_one_ended() {
        let pen = record(38, &numbers(&[0, 0, 1, 0, 0x00_00_00_FF]));
        let take = record(37, &numbers(&[0]));
        let start = record(27, &numbers(&[2, 8]));
        let line = record(54, &numbers(&[14, 8]));
        let data = emf(16, 16, &[pen, take, start, line]);

        let image = decode(&data).expect("a picture");
        assert_eq!(pixel(&image, 8, 8), [255, 0, 0, 255], "the line");
        assert_eq!(pixel(&image, 8, 2), [255, 255, 255, 255], "and not above it");
    }

    #[test]
    fn a_polygon_of_half_words_is_the_same_shape_as_one_of_whole_ones() {
        let brush = record(39, &numbers(&[0, 0, 0x00_00_00_FF, 0]));
        let take = record(37, &numbers(&[0]));

        let mut whole = numbers(&[0, 0, 16, 16, 3]);
        whole.extend(numbers(&[2, 2, 14, 2, 8, 14]));
        let one = decode(&emf(16, 16, &[brush.clone(), take.clone(), record(3, &whole)]))
            .expect("a picture");

        let mut halves = numbers(&[0, 0, 16, 16, 3]);
        for value in [2i16, 2, 14, 2, 8, 14] {
            halves.extend_from_slice(&value.to_le_bytes());
        }
        let other = decode(&emf(16, 16, &[brush, take, record(86, &halves)])).expect("a picture");

        assert_eq!(one.pixels, other.pixels, "the two ways of writing a point differ");
        assert_eq!(pixel(&one, 8, 6), [255, 0, 0, 255], "and the shape is filled");
    }

    #[test]
    fn the_window_and_the_viewport_move_the_drawing() {
        // A window twice the size of the viewport draws everything at half
        // size, which is what the mapping is for.
        let brush = record(39, &numbers(&[0, 0, 0x00_00_00_FF, 0]));
        let take = record(37, &numbers(&[0]));
        let window_extent = record(9, &numbers(&[32, 32]));
        let viewport_extent = record(11, &numbers(&[16, 16]));
        let shape = record(43, &numbers(&[0, 0, 32, 32]));
        let data = emf(16, 16, &[brush, take, window_extent, viewport_extent, shape]);

        let image = decode(&data).expect("a picture");
        assert_eq!(pixel(&image, 8, 8), [255, 0, 0, 255], "the shape reaches the middle");
    }

    #[test]
    fn the_world_transform_moves_it_too() {
        let brush = record(39, &numbers(&[0, 0, 0x00_00_00_FF, 0]));
        let take = record(37, &numbers(&[0]));
        // Everything moved eight along.
        let mut transform = Vec::new();
        for value in [1.0f32, 0.0, 0.0, 1.0, 8.0, 0.0] {
            transform.extend_from_slice(&value.to_bits().to_le_bytes());
        }
        let world = record(35, &transform);
        let shape = record(43, &numbers(&[0, 0, 6, 16]));
        let data = emf(16, 16, &[brush, take, world, shape]);

        let image = decode(&data).expect("a picture");
        assert_eq!(pixel(&image, 10, 8), [255, 0, 0, 255], "the shape moved along");
        assert_eq!(pixel(&image, 2, 8), [255, 255, 255, 255], "and is no longer where it was");
    }

    #[test]
    fn a_path_is_collected_and_then_drawn() {
        let brush = record(39, &numbers(&[0, 0, 0x00_00_00_FF, 0]));
        let take = record(37, &numbers(&[0]));
        let begin = record(59, &[]);
        let shape = record(43, &numbers(&[2, 2, 14, 14]));
        let end = record(60, &[]);
        let data = emf(16, 16, &[brush.clone(), take.clone(), begin, shape.clone(), end]);

        // Between the two records the shape is collected and not drawn.
        let image = decode(&data).expect("a picture");
        assert_eq!(pixel(&image, 8, 8), [255, 255, 255, 255], "nothing should be drawn yet");

        // And then filling the path draws it.
        let data = emf(
            16,
            16,
            &[
                brush,
                take,
                record(59, &[]),
                shape,
                record(60, &[]),
                record(62, &numbers(&[0, 0, 16, 16])),
            ],
        );
        let image = decode(&data).expect("a picture");
        assert_eq!(pixel(&image, 8, 8), [255, 0, 0, 255], "and now it should");
    }

    #[test]
    fn a_record_that_says_it_is_shorter_than_its_own_head_ends_the_reading() {
        let broken = [43u32.to_le_bytes(), 4u32.to_le_bytes()].concat();
        let data = emf(8, 8, &[broken]);
        assert!(decode(&data).is_ok(), "it should stop, not refuse");
    }

    #[test]
    fn the_objects_the_system_provides_are_known_by_their_numbers() {
        let mut state = State::new(1, 1);
        stock(&mut state, 5);
        assert_eq!(state.brush, None, "the hollow brush fills nothing");
        stock(&mut state, 4);
        assert_eq!(state.brush, Some(Color::BLACK));
        stock(&mut state, 8);
        assert_eq!(state.pen, None, "and the null pen draws nothing");
    }
}
