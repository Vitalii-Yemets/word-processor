//! Words in a metafile, drawn with the fonts of this machine.
//!
//! A metafile is a recording of drawing, and some of what it records is words:
//! a label on a diagram, the numbers up the side of a chart. The player has no
//! fonts of its own — what a machine has is the business of whatever opened the
//! document — so it is given them, and these tests are about what it does with
//! them.

use wp_layout::FontLibrary;

/// The fonts, read once per test.
fn library() -> &'static FontLibrary {
    Box::leak(Box::new(FontLibrary::scan_system()))
}

/// One record of the older format: its length in words, what it is, and its
/// parameters.
fn record(function: u16, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let words = (body.len() + 6) / 2;
    out.extend_from_slice(&(words as u32).to_le_bytes());
    out.extend_from_slice(&function.to_le_bytes());
    out.extend_from_slice(body);
    out
}

/// A metafile of a given size with whatever records are given inside it.
fn metafile(records: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::new();
    // The placeable header: what it is, the rectangle it covers, and how many
    // of its units go to the inch.
    out.extend_from_slice(&0x9AC6_CDD7_u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    for value in [0i16, 0, 120, 60] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out.extend_from_slice(&120u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    // Then the metafile header, which nothing here reads.
    out.extend_from_slice(&[0u8; 18]);
    for piece in records {
        out.extend_from_slice(piece);
    }
    out.extend_from_slice(&record(0x0000, &[]));
    out
}

/// A face of a given height and turn, made and then taken up.
///
/// Two records, because making an object and using it are two things in both
/// formats: a file that made a font and never selected it would draw its words
/// in whatever was in hand before. The name is one that is probably not on this
/// machine, so what is wanted is the fallback — which is what a real metafile
/// recorded thirty years ago gets as well.
fn font(height: i16, escapement: i16) -> Vec<Vec<u8>> {
    let mut body = Vec::new();
    for value in [height, 0i16, escapement, 0, 400] {
        body.extend_from_slice(&value.to_le_bytes());
    }
    // Italic, underline, strikeout, charset, and the four that follow.
    body.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0]);
    body.extend_from_slice(b"Arial\0");
    vec![record(0x02FB, &body), record(0x012D, &0u16.to_le_bytes())]
}

/// Words at a point.
fn words(x: i16, y: i16, text: &str) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&(text.len() as u16).to_le_bytes());
    body.extend_from_slice(text.as_bytes());
    if text.len() % 2 == 1 {
        body.push(0);
    }
    body.extend_from_slice(&y.to_le_bytes());
    body.extend_from_slice(&x.to_le_bytes());
    record(0x0521, &body)
}

/// How much of the picture is not white.
fn ink(image: &wp_image::Image) -> usize {
    image.pixels.chunks_exact(4).filter(|pixel| pixel[0] < 200).count()
}

/// The leftmost and topmost pixel of ink, and the rightmost and bottommost.
fn bounds(image: &wp_image::Image) -> (usize, usize, usize, usize) {
    let mut found = (usize::MAX, usize::MAX, 0, 0);
    for y in 0..image.height {
        for x in 0..image.width {
            let at = (y * image.width + x) * 4;
            if image.pixels[at] < 200 {
                found.0 = found.0.min(x);
                found.1 = found.1.min(y);
                found.2 = found.2.max(x);
                found.3 = found.3.max(y);
            }
        }
    }
    found
}

#[test]
fn a_metafile_with_words_draws_them() {
    let file = metafile(&[font(-24, 0), vec![words(10, 40, "Hi")]].concat());
    let drawn = wp_image::decode_with(&file, library()).expect("a picture");
    let plain = wp_image::decode(&file).expect("a picture");

    assert!(ink(&drawn) > 0, "the words were not drawn");
    assert_eq!(ink(&plain), 0, "a caller with no fonts should get no words");
}

#[test]
fn a_selected_font_is_the_one_the_words_are_drawn_in() {
    // The same words at two sizes: the bigger face covers more of the picture.
    let small = metafile(&[font(-12, 0), vec![words(10, 40, "Hi")]].concat());
    let large = metafile(&[font(-36, 0), vec![words(10, 40, "Hi")]].concat());
    let small = wp_image::decode_with(&small, library()).expect("a picture");
    let large = wp_image::decode_with(&large, library()).expect("a picture");
    assert!(ink(&large) > ink(&small) * 2, "the size was not read");
}

#[test]
fn the_alignment_says_which_end_of_the_words_the_point_is() {
    // Left is what a file falls back on, so the words run to the right of the
    // point; asked for the right, they run to the left of it.
    let align = |value: u16| record(0x012E, &value.to_le_bytes());
    let left = metafile(&[font(-24, 0), vec![align(0), words(60, 40, "Hi")]].concat());
    let right = metafile(&[font(-24, 0), vec![align(2), words(60, 40, "Hi")]].concat());
    let left = wp_image::decode_with(&left, library()).expect("a picture");
    let right = wp_image::decode_with(&right, library()).expect("a picture");
    // The point is at sixty of the hundred and twenty units across, which is
    // the middle of the picture whatever size it came out.
    let point = (left.width / 2) as i64;
    let (left, right) = (bounds(&left), bounds(&right));

    assert!(left.0 as i64 >= point - 2, "the words should start at the point: {left:?}");
    assert!(right.2 as i64 <= point + 2, "and end at it when asked: {right:?}");
    assert!(right.0 < left.0, "they should be on the other side of the point");
}

#[test]
fn the_escapement_turns_the_words_on_their_side() {
    // A quarter turn, which the file states in tenths of a degree: the same
    // words come out as tall as they were wide.
    let flat = metafile(&[font(-24, 0), vec![words(40, 40, "Hill")]].concat());
    let side = metafile(&[font(-24, 900), vec![words(40, 40, "Hill")]].concat());
    let flat = bounds(&wp_image::decode_with(&flat, library()).expect("a picture"));
    let side = bounds(&wp_image::decode_with(&side, library()).expect("a picture"));

    let wide = |at: (usize, usize, usize, usize)| at.2 - at.0;
    let tall = |at: (usize, usize, usize, usize)| at.3 - at.1;
    assert!(wide(flat) > tall(flat), "the words should be wider than they are tall");
    assert!(tall(side) > wide(side), "and turned, taller than they are wide");
}

/// One record of the newer format: a type, a length, and a body.
fn wide_record(kind: u32, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&kind.to_le_bytes());
    out.extend_from_slice(&((body.len() + 8) as u32).to_le_bytes());
    out.extend_from_slice(body);
    out
}

/// An enhanced metafile of a given size with whatever records are given.
fn enhanced(width: i32, height: i32, records: &[Vec<u8>]) -> Vec<u8> {
    // The frame is in hundredths of a millimetre, and the picture is played
    // back at ninety-six pixels to the inch.
    let across = (width as f32 * 2540.0 / 96.0).round() as i32;
    let down = (height as f32 * 2540.0 / 96.0).round() as i32;

    let mut header = Vec::new();
    header.extend_from_slice(&1u32.to_le_bytes());
    header.extend_from_slice(&88u32.to_le_bytes());
    for value in [0, 0, width - 1, height - 1, 0, 0, across, down] {
        header.extend_from_slice(&value.to_le_bytes());
    }
    header.extend_from_slice(b" EMF");
    while header.len() < 88 {
        header.push(0);
    }

    let mut out = header;
    for piece in records {
        out.extend_from_slice(piece);
    }
    out
}

#[test]
fn the_newer_format_draws_its_words_too() {
    // A face, taken up, and a run of words at a point. The newer format writes
    // both the name and the words two bytes a letter.
    let mut face = Vec::new();
    face.extend_from_slice(&0u32.to_le_bytes()); // Which slot it goes in.
    for value in [-24i32, 0, 0, 0, 400] {
        face.extend_from_slice(&value.to_le_bytes());
    }
    face.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0]);
    for letter in "Arial".encode_utf16() {
        face.extend_from_slice(&letter.to_le_bytes());
    }
    while face.len() < 40 + 64 {
        face.push(0);
    }

    let text = "Hi";
    let mut run = Vec::new();
    // The bounds, the graphics mode and the two scales, none of which changes
    // where the words go.
    for value in [0i32, 0, 200, 100, 1] {
        run.extend_from_slice(&value.to_le_bytes());
    }
    run.extend_from_slice(&1.0f32.to_le_bytes());
    run.extend_from_slice(&1.0f32.to_le_bytes());
    // Then the run itself: where it goes, how many letters, and where they are.
    run.extend_from_slice(&20i32.to_le_bytes());
    run.extend_from_slice(&60i32.to_le_bytes());
    run.extend_from_slice(&(text.len() as u32).to_le_bytes());
    run.extend_from_slice(&76u32.to_le_bytes());
    run.extend_from_slice(&0u32.to_le_bytes());
    for value in [0i32, 0, 0, 0] {
        run.extend_from_slice(&value.to_le_bytes());
    }
    run.extend_from_slice(&0u32.to_le_bytes());
    while run.len() + 8 < 76 {
        run.push(0);
    }
    for letter in text.encode_utf16() {
        run.extend_from_slice(&letter.to_le_bytes());
    }

    let file = enhanced(
        200,
        100,
        &[wide_record(82, &face), wide_record(37, &0u32.to_le_bytes()), wide_record(84, &run)],
    );
    let drawn = wp_image::decode_with(&file, library()).expect("a picture");
    assert!(ink(&drawn) > 0, "the words were not drawn");
    assert_eq!(ink(&wp_image::decode(&file).expect("a picture")), 0, "and none without fonts");
}
