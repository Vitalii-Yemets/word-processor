//! Interoperability with an outside encoder.
//!
//! Decoding a picture this project also wrote would prove nothing: there is no
//! PNG or JPEG encoder here at all, and a mistake made symmetrically on both
//! sides slips straight through even where there is one. So the fixtures come
//! out of GDI+ — the encoder behind a great many of the pictures inside real
//! documents — and the expected colours are read back through GDI+ as well,
//! never computed here.
//!
//! The fixtures are produced by `tools/make-image-fixtures.ps1`, which needs
//! Windows. They are committed, so this runs anywhere.

use std::path::{Path, PathBuf};

use wp_image::{decode, Format};

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures")
}

/// One point of one picture, and the colour the encoder says is there.
struct Sample {
    name: String,
    width: usize,
    height: usize,
    x: usize,
    y: usize,
    red: u8,
    green: u8,
    blue: u8,
    alpha: u8,
    /// How far off a lossy format is allowed to be.
    tolerance: i32,
}

fn read_manifest() -> Vec<Sample> {
    let path = fixtures_dir().join("manifest.txt");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };

    text.lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() < 10 {
                return None;
            }
            Some(Sample {
                name: fields[0].to_owned(),
                width: fields[1].parse().ok()?,
                height: fields[2].parse().ok()?,
                x: fields[3].parse().ok()?,
                y: fields[4].parse().ok()?,
                red: fields[5].parse().ok()?,
                green: fields[6].parse().ok()?,
                blue: fields[7].parse().ok()?,
                alpha: fields[8].parse().ok()?,
                tolerance: fields[9].parse().ok()?,
            })
        })
        .collect()
}

#[test]
fn every_form_of_bitmap_is_among_the_fixtures() {
    // A bitmap is not one format but several, and the ones a decoder is most
    // likely to be wrong about are the ones an encoder will not write for it.
    // Each of these is assembled by the tool and read back through GDI+, so
    // what is checked is a second reader's answer and not this one's.
    let names: Vec<String> = read_manifest().into_iter().map(|sample| sample.name).collect();
    for wanted in [
        "square24.bmp", // Twenty-four bits, as any encoder writes them.
        "core24.bmp",   // The oldest header, twelve bytes of it.
        "bits1.bmp",    // One bit to the pixel, out of a palette of two.
        "bits4.bmp",    // Four bits, out of a palette of sixteen.
        "rle8.bmp",     // Runs of one colour, eight bits to the pixel.
        "rle4.bmp",     // The same, four bits.
        "rgb565.bmp",   // Sixteen bits divided up by masks the file states.
        "topdown.bmp",  // The rows the way they are read.
    ] {
        assert!(names.iter().any(|name| name == wanted), "{wanted} is not among the fixtures");
    }
}

#[test]
fn every_form_of_gif_is_among_the_fixtures() {
    let names: Vec<String> = read_manifest().into_iter().map(|sample| sample.name).collect();
    for wanted in [
        "square.gif",      // Written by GDI+, so a real compressor's stream.
        "interlaced.gif",  // The rows in the order interlacing puts them.
        "transparent.gif", // A colour the file says is not to be drawn.
        "animated.gif",    // Two frames, of which the first is the picture.
    ] {
        assert!(names.iter().any(|name| name == wanted), "{wanted} is not among the fixtures");
    }
}

#[test]
fn a_progressive_picture_is_among_the_fixtures() {
    // The one form no encoder to hand writes. It is assembled by the tool and
    // read back through GDI+, which does read progressive — so the proof still
    // comes from outside even though the file did not.
    let names: Vec<String> = read_manifest().into_iter().map(|sample| sample.name).collect();
    assert!(names.iter().any(|name| name == "progressive.jpg"));
}

#[test]
fn the_fixtures_are_there() {
    let samples = read_manifest();
    assert!(
        !samples.is_empty(),
        "no fixtures: run tools/make-image-fixtures.ps1 on Windows to produce them"
    );
}

#[test]
fn every_picture_decodes_to_the_colours_its_encoder_wrote() {
    let samples = read_manifest();
    assert!(!samples.is_empty(), "no fixtures to check");

    for sample in &samples {
        let path = fixtures_dir().join(&sample.name);
        let bytes = std::fs::read(&path).unwrap_or_else(|_| panic!("cannot read {:?}", path));
        let image =
            decode(&bytes).unwrap_or_else(|error| panic!("cannot decode {}: {error}", sample.name));

        assert_eq!(
            (image.width, image.height),
            (sample.width, sample.height),
            "{} came out the wrong size",
            sample.name
        );

        let at = (sample.y * image.width + sample.x) * 4;
        let found = &image.pixels[at..at + 4];
        let wanted = [sample.red, sample.green, sample.blue, sample.alpha];

        for (channel, (got, expected)) in found.iter().zip(&wanted).enumerate() {
            let difference = i32::from(*got) - i32::from(*expected);
            assert!(
                difference.abs() <= sample.tolerance,
                "{} at ({}, {}), channel {channel}: got {got}, expected {expected}",
                sample.name,
                sample.x,
                sample.y
            );
        }
    }
}

#[test]
fn each_fixture_is_recognised_from_its_own_bytes() {
    for sample in read_manifest() {
        let bytes = std::fs::read(fixtures_dir().join(&sample.name)).expect("a fixture");
        let wanted = match sample.name.rsplit_once('.').map(|(_, suffix)| suffix) {
            Some("png") => Format::Png,
            Some("bmp") => Format::Bmp,
            Some("gif") => Format::Gif,
            _ => Format::Jpeg,
        };
        assert_eq!(Format::detect(&bytes), Some(wanted), "{}", sample.name);
    }
}

#[test]
fn a_picture_with_transparency_keeps_it() {
    let path = fixtures_dir().join("alpha.png");
    let Ok(bytes) = std::fs::read(&path) else { return };
    let image = decode(&bytes).expect("a decodable picture");

    // The fixture varies alpha down the picture, so the top and the bottom
    // must differ — a decoder that dropped the channel would pass every other
    // check here.
    let top = image.pixels[3];
    let bottom = image.pixels[(image.height - 1) * image.width * 4 + 3];
    assert!(bottom > top, "alpha should increase down the picture, got {top} then {bottom}");
}

#[test]
fn a_flat_colour_survives_a_lossy_encoder_almost_exactly() {
    // Nothing for a lossy format to lose, so the two should be very close.
    let Ok(jpeg) = std::fs::read(fixtures_dir().join("flat.jpg")) else { return };
    let Ok(png) = std::fs::read(fixtures_dir().join("flat.png")) else { return };

    let from_jpeg = decode(&jpeg).expect("a decodable JPEG");
    let from_png = decode(&png).expect("a decodable PNG");
    assert_eq!((from_jpeg.width, from_jpeg.height), (from_png.width, from_png.height));

    let middle = (from_png.height / 2 * from_png.width + from_png.width / 2) * 4;
    for channel in 0..3 {
        let difference = i32::from(from_jpeg.pixels[middle + channel])
            - i32::from(from_png.pixels[middle + channel]);
        assert!(
            difference.abs() <= 4,
            "channel {channel} differs by {difference} between the two formats"
        );
    }
}

#[test]
fn a_gradient_really_is_a_gradient_after_decoding() {
    // Every row and column of the fixture changes, so a decoder that lost the
    // order of its rows or columns would produce something flat or scrambled.
    let Ok(bytes) = std::fs::read(fixtures_dir().join("gradient.png")) else { return };
    let image = decode(&bytes).expect("a decodable picture");

    let red_at = |x: usize, y: usize| image.pixels[(y * image.width + x) * 4];
    let green_at = |x: usize, y: usize| image.pixels[(y * image.width + x) * 4 + 1];

    assert!(red_at(0, 0) < red_at(image.width - 1, 0), "red should rise across");
    assert!(green_at(0, 0) < green_at(0, image.height - 1), "green should rise down");
    assert_eq!(red_at(5, 0), red_at(5, 9), "red should not depend on the row");
}
