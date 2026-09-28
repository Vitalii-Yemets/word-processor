//! The newer kinds of colour glyph, drawn: a tree of paints painted, an
//! Apple picture decoded, and each held at points where the format says what
//! colour it is.
//!
//! The fonts are written by fontTools from `tools/make-colour-fonts.py` when
//! the build image is built; what each glyph is was written down there.

use wp_font::{Font, GlyphId};
use wp_layout::colourglyph::{picture_of, GlyphPicture};
use wp_raster::Color;

const FOLDER: &str = "/usr/share/fonts/truetype/wp-colour";
const SIZE: f32 = 100.0;
const TEXT: Color = Color { red: 10, green: 20, blue: 200, alpha: 255 };

fn read(name: &str) -> Vec<u8> {
    let path = format!("{FOLDER}/{name}");
    std::fs::read(&path).unwrap_or_else(|error| panic!("cannot read {path}: {error}"))
}

fn glyph(font: &Font<'_>, code: u32) -> GlyphId {
    font.glyph_for(char::from_u32(code).unwrap()).expect("mapped")
}

/// The colour of a picture at a point of the font's own grid, in units of a
/// thousand to the em, y up.
fn at(picture: &GlyphPicture, x: f32, y: f32) -> [u8; 4] {
    let scale = picture.pixels_per_em / 1000.0;
    let column = (x * scale - picture.left).floor();
    let row = (picture.top - y * scale).floor();
    if column < 0.0 || row < 0.0 {
        return [0; 4];
    }
    let (column, row) = (column as usize, row as usize);
    if column >= picture.width || row >= picture.height {
        return [0; 4];
    }
    let at = (row * picture.width + column) * 4;
    [picture.pixels[at], picture.pixels[at + 1], picture.pixels[at + 2], picture.pixels[at + 3]]
}

fn near(colour: [u8; 4], wanted: [u8; 4], by: u8) -> bool {
    colour.iter().zip(wanted).all(|(got, want)| got.abs_diff(want) <= by)
}

/// Where in the font's grid the middle of the pixel is that [`at`] reads
/// for a point: what a gradient is actually asked about.
fn centre(picture: &GlyphPicture, x: f32, y: f32) -> (f32, f32) {
    let scale = picture.pixels_per_em / 1000.0;
    let column = (x * scale - picture.left).floor();
    let row = (picture.top - y * scale).floor();
    ((column + 0.5 + picture.left) / scale, (picture.top - row - 0.5) / scale)
}

/// Two opaque colours mixed the way the format says a gradient mixes them:
/// in linear light.
fn mixed(from: [u8; 3], to: [u8; 3], t: f32) -> [u8; 4] {
    let linear = |value: u8| {
        let value = f32::from(value) / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    let encoded = |value: f32| {
        let value = if value <= 0.003_130_8 {
            value * 12.92
        } else {
            1.055 * value.powf(1.0 / 2.4) - 0.055
        };
        (value.clamp(0.0, 1.0) * 255.0).round() as u8
    };
    let channel = |one: u8, other: u8| encoded(linear(one) * (1.0 - t) + linear(other) * t);
    [channel(from[0], to[0]), channel(from[1], to[1]), channel(from[2], to[2]), 255]
}

const RED: [u8; 3] = [255, 0, 0];
const BLUE: [u8; 3] = [0, 0, 255];

fn painted(code: u32) -> GlyphPicture {
    let data = read("wp-colour-one.ttf");
    let font = Font::parse(&data).unwrap();
    picture_of(&font, glyph(&font, code), SIZE, TEXT).expect("a picture")
}

#[test]
fn a_linear_gradient_runs_left_to_right_mixed_in_linear_light() {
    let picture = painted(0xE000);
    // The clip box, a pixel of room each side.
    assert_eq!((picture.width, picture.height), (102, 82));
    // Everywhere across, the colour the distance across says, mixed in linear
    // light: half red and half blue is 188 of each in the ordinary scale,
    // where mixing the ordinary numbers would give 128.
    for x in [5.0, 250.0, 500.0, 750.0, 995.0] {
        let (along, _) = centre(&picture, x, 400.0);
        let wanted = mixed(RED, BLUE, along / 1000.0);
        assert!(
            near(at(&picture, x, 400.0), wanted, 2),
            "at {x}: {:?} not {wanted:?}",
            at(&picture, x, 400.0)
        );
    }
    assert!(near(at(&picture, 500.0, 400.0), [188, 0, 188, 255], 3));
    // The same all the way up.
    assert_eq!(at(&picture, 500.0, 100.0), at(&picture, 500.0, 700.0));
}

#[test]
fn a_radial_gradient_is_white_in_the_middle_and_green_at_the_edge() {
    let picture = painted(0xE001);
    for x in [500.0, 600.0, 700.0, 880.0] {
        let (cx, cy) = centre(&picture, x, 400.0);
        let out = ((cx - 500.0).powi(2) + (cy - 400.0).powi(2)).sqrt() / 400.0;
        let wanted = mixed([255, 255, 255], [0, 128, 0], out);
        assert!(
            near(at(&picture, x, 400.0), wanted, 2),
            "at {x}: {:?} not {wanted:?}",
            at(&picture, x, 400.0)
        );
    }
    // Outside the disc it is seen through, nothing.
    assert_eq!(at(&picture, 60.0, 60.0)[3], 0);
}

#[test]
fn a_sweep_goes_round_anticlockwise_from_its_first_angle() {
    let picture = painted(0xE002);
    // Round the centre anticlockwise from the right: red, yellow half way
    // round, and red again — each place the colour its angle says.
    for (x, y) in [(900.0, 405.0), (100.0, 400.0), (500.0, 750.0), (500.0, 50.0), (800.0, 700.0)] {
        let (cx, cy) = centre(&picture, x, y);
        let turned = (cy - 400.0).atan2(cx - 500.0).to_degrees().rem_euclid(360.0) / 360.0;
        let wanted = if turned <= 0.5 {
            mixed(RED, [255, 255, 0], turned * 2.0)
        } else {
            mixed([255, 255, 0], RED, turned * 2.0 - 1.0)
        };
        assert!(
            near(at(&picture, x, y), wanted, 2),
            "at {x},{y}: {:?} not {wanted:?}",
            at(&picture, x, y)
        );
    }
    // Straight up and straight down are the same distance round from red.
    assert!(near(at(&picture, 500.0, 750.0), at(&picture, 500.0, 50.0), 3));
}

#[test]
fn a_turn_about_a_centre_stands_the_bar_upright() {
    let picture = painted(0xE003);
    assert!(near(at(&picture, 500.0, 50.0), [0, 0, 255, 255], 8));
    assert!(near(at(&picture, 500.0, 750.0), [0, 0, 255, 255], 8));
    // Where the bar was before it was turned, nothing.
    assert_eq!(at(&picture, 150.0, 400.0)[3], 0);
}

#[test]
fn a_composite_multiplies_where_both_are_and_keeps_the_backdrop_elsewhere() {
    let picture = painted(0xE004);
    // Red multiplied onto yellow is red.
    assert!(near(at(&picture, 500.0, 400.0), [255, 0, 0, 255], 8));
    // In the corner only the square is: yellow.
    assert!(near(at(&picture, 30.0, 30.0), [255, 255, 0, 255], 8));
    // Kept only where the disc is: blue in the middle, nothing in the corner.
    let kept = painted(0xE00A);
    assert!(near(at(&kept, 500.0, 400.0), [0, 0, 255, 255], 8));
    assert_eq!(at(&kept, 30.0, 30.0)[3], 0);
}

#[test]
fn a_layer_in_the_colour_of_the_text_takes_the_text_s_colour() {
    let picture = painted(0xE005);
    assert!(near(at(&picture, 50.0, 50.0), [10, 20, 200, 255], 4));
    assert!(near(at(&picture, 500.0, 400.0), [0, 128, 0, 255], 4));
}

#[test]
fn a_borrowed_glyph_is_moved_and_clipped_by_its_own_box() {
    let picture = painted(0xE006);
    // Moved up a hundred units, so the bottom hundred are empty…
    assert_eq!(at(&picture, 500.0, 50.0)[3], 0);
    // …and the gradient above is the linear glyph's, which a move up does
    // not change.
    let linear = painted(0xE000);
    assert_eq!(at(&picture, 500.0, 400.0), at(&linear, 500.0, 300.0));
    assert!(near(at(&picture, 500.0, 400.0), [188, 0, 188, 255], 3));
}

#[test]
fn a_run_repeated_starts_again_and_a_run_reflected_comes_back() {
    // Every quarter of the width is the whole run again; reflected, every
    // second quarter runs back.
    let repeated = painted(0xE007);
    let reflected = painted(0xE008);
    for x in [100.0, 245.0, 260.0, 400.0, 600.0, 875.0] {
        let (along, _) = centre(&repeated, x, 400.0);
        let t = along / 250.0;
        let once = t.rem_euclid(1.0);
        let back =
            if t.rem_euclid(2.0) > 1.0 { 2.0 - t.rem_euclid(2.0) } else { t.rem_euclid(2.0) };
        let wanted = mixed(RED, BLUE, once);
        assert!(
            near(at(&repeated, x, 400.0), wanted, 2),
            "repeated at {x}: {:?} not {wanted:?}",
            at(&repeated, x, 400.0)
        );
        let wanted = mixed(RED, BLUE, back);
        assert!(
            near(at(&reflected, x, 400.0), wanted, 2),
            "reflected at {x}: {:?} not {wanted:?}",
            at(&reflected, x, 400.0)
        );
    }
}

#[test]
fn a_skew_leans_the_shape_over() {
    let picture = painted(0xE009);
    // The left half leant by forty-five degrees along x: at the height of
    // 400 it runs from -400 to 100.
    assert!(
        near(at(&picture, -200.0, 400.0), [0, 128, 0, 255], 8),
        "{:?}",
        at(&picture, -200.0, 400.0)
    );
    assert_eq!(at(&picture, 300.0, 400.0)[3], 0);
}

#[test]
fn half_transparent_black_is_half_transparent() {
    let picture = painted(0xE00B);
    let colour = at(&picture, 750.0, 400.0);
    assert!(colour[3].abs_diff(128) <= 2, "{colour:?}");
    assert_eq!(at(&picture, 250.0, 400.0)[3], 0);
}

#[test]
fn an_apple_picture_is_decoded_and_stood_on_the_baseline() {
    let data = read("wp-colour-pictures.ttf");
    let font = Font::parse(&data).unwrap();
    let picture = picture_of(&font, glyph(&font, 0xE000), 40.0, TEXT).expect("a picture");
    assert_eq!((picture.width, picture.height), (40, 40));
    // Placed by its bottom edge at the baseline, so its top is an em up.
    assert_eq!((picture.left, picture.top), (0.0, 40.0));
    let pixel = |x: usize, y: usize| &picture.pixels[(y * 40 + x) * 4..(y * 40 + x) * 4 + 4];
    assert_eq!(pixel(5, 5), &[0, 0, 255, 255], "the top half is blue");
    assert_eq!(pixel(5, 35), &[255, 0, 0, 255], "the bottom half is red");

    // The TIFF one, moved off the corner.
    let tiled = picture_of(&font, glyph(&font, 0xE002), 40.0, TEXT).expect("a picture");
    assert_eq!((tiled.left, tiled.top), (10.0, 30.0));
    assert_eq!(&tiled.pixels[..4], &[0, 128, 0, 255]);
}

#[test]
fn an_ordinary_glyph_is_not_a_picture() {
    let data = std::fs::read("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf").unwrap();
    let font = Font::parse(&data).unwrap();
    assert!(picture_of(&font, font.glyph_for('A').unwrap(), SIZE, TEXT).is_none());
}

fn drawing(code: u32) -> GlyphPicture {
    let data = read("wp-colour-drawings.ttf");
    let font = Font::parse(&data).unwrap();
    picture_of(&font, glyph(&font, code), SIZE, TEXT).expect("a picture")
}

#[test]
fn an_svg_glyph_is_drawn_with_its_gradient_mixed_the_way_svg_mixes() {
    let picture = drawing(0xE000);
    // The whole em above the baseline.
    assert_eq!((picture.width, picture.height), (102, 82));
    assert!(near(at(&picture, 250.0, 400.0), [255, 0, 0, 255], 1));
    // The right half runs from yellow to blue, mixed in the ordinary scale,
    // which is what SVG does unless it is told otherwise: grey in the middle.
    for x in [505.0, 750.0, 995.0] {
        let (across, _) = centre(&picture, x, 400.0);
        let t = (across - 500.0) / 500.0;
        let channel = |from: f32, to: f32| (from + (to - from) * t).round() as u8;
        let wanted = [channel(255.0, 0.0), channel(255.0, 0.0), channel(0.0, 255.0), 255];
        assert!(
            near(at(&picture, x, 400.0), wanted, 2),
            "at {x}: {:?} not {wanted:?}",
            at(&picture, x, 400.0)
        );
    }
}

#[test]
fn an_svg_glyph_draws_its_groups_moved_and_its_uses_faded() {
    let picture = drawing(0xE001);
    // The circle, moved into place by its group.
    assert!(near(at(&picture, 300.0, 400.0), [0, 128, 0, 255], 1));
    // Half black over it on the right, from a rectangle defined elsewhere.
    assert!(
        near(at(&picture, 700.0, 400.0), [0, 64, 0, 255], 2),
        "{:?}",
        at(&picture, 700.0, 400.0)
    );
    let shade = at(&picture, 950.0, 750.0);
    assert!(shade[3].abs_diff(128) <= 1 && shade[..3] == [0, 0, 0], "{shade:?}");
    assert_eq!(at(&picture, 250.0, 750.0)[3], 0);
}

#[test]
fn a_compressed_svg_glyph_is_drawn_like_any_other() {
    let picture = drawing(0xE002);
    // White at the middle to red at the edge, mixed in the ordinary scale.
    for x in [500.0, 650.0, 800.0] {
        let (cx, cy) = centre(&picture, x, 400.0);
        let out = ((cx - 500.0).powi(2) + (cy - 400.0).powi(2)).sqrt() / 400.0;
        let fade = (255.0 * (1.0 - out)).round() as u8;
        assert!(
            near(at(&picture, x, 400.0), [255, fade, fade, 255], 2),
            "at {x}: {:?}",
            at(&picture, x, 400.0)
        );
    }
    let edge = at(&picture, 880.0, 400.0);
    assert!(edge[0] == 255 && edge[1] < 40, "{edge:?}");
    assert_eq!(at(&picture, 120.0, 20.0)[3], 0, "outside the disc");
}
