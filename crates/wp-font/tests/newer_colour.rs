//! The three newer kinds of colour glyph, read out of fonts written by
//! fontTools: a `COLR` version 1 font, an `sbix` font and an `SVG ` font.
//!
//! No font of these kinds is packaged for the build image's Debian, so the
//! image has fontTools write them from `tools/make-colour-fonts.py` — an
//! implementation of the format that is not this one, the way LibreOffice
//! writes the `.doc` files. What each glyph is was written down there, and
//! is what these hold the reader to.

use wp_font::paint::{CompositeMode, Extend, Paint, Swatch};
use wp_font::{Font, GlyphId, ImageFormat, Rgba};

const FOLDER: &str = "/usr/share/fonts/truetype/wp-colour";

fn read(name: &str) -> Vec<u8> {
    let path = format!("{FOLDER}/{name}");
    std::fs::read(&path).unwrap_or_else(|error| {
        panic!("cannot read {path}: {error}\nthe build image writes it with tools/make-colour-fonts.py")
    })
}

fn glyph(font: &Font<'_>, code: u32) -> GlyphId {
    font.glyph_for(char::from_u32(code).unwrap()).expect("the character is mapped")
}

const RED: Rgba = Rgba { red: 255, green: 0, blue: 0, alpha: 255 };
const BLUE: Rgba = Rgba { red: 0, green: 0, blue: 255, alpha: 255 };

#[test]
fn a_linear_gradient_is_read_with_its_three_points_and_its_run_of_colours() {
    let data = read("wp-colour-one.ttf");
    let font = Font::parse(&data).unwrap();
    assert!(font.has_colour());
    let tree = font.colour_glyph(glyph(&font, 0xE000)).expect("a tree");
    assert_eq!(tree.clip, Some((0.0, 0.0, 1000.0, 800.0)));
    let Paint::Glyph { glyph: outline, paint } = tree.paint else { panic!("{:?}", tree.paint) };
    assert!(
        font.outline(outline).unwrap().is_some(),
        "the glyph it is seen through has an outline"
    );
    let Paint::Linear { line, from, to, rotation } = *paint else {
        panic!("not a linear gradient")
    };
    assert_eq!((from, to, rotation), ((0.0, 0.0), (1000.0, 0.0), (0.0, 800.0)));
    assert_eq!(line.extend, Extend::Pad);
    assert_eq!(line.stops, vec![(0.0, Swatch::Colour(RED)), (1.0, Swatch::Colour(BLUE))]);
}

#[test]
fn the_other_gradients_and_the_extend_modes_are_read() {
    let data = read("wp-colour-one.ttf");
    let font = Font::parse(&data).unwrap();
    let inner = |code: u32| match font.colour_glyph(glyph(&font, code)).unwrap().paint {
        Paint::Glyph { paint, .. } => *paint,
        other => panic!("{other:?}"),
    };
    let Paint::Radial { from, to, .. } = inner(0xE001) else { panic!() };
    assert_eq!((from, to), (((500.0, 400.0), 0.0), ((500.0, 400.0), 400.0)));
    let Paint::Sweep { centre, start, end, line } = inner(0xE002) else { panic!() };
    assert_eq!(centre, (500.0, 400.0));
    // The angles are written with a bias of a half turn, which is what lets
    // a whole turn be written at all.
    assert_eq!((start, end), (0.0, 360.0));
    assert_eq!(line.stops.len(), 3);
    let Paint::Linear { line, .. } = inner(0xE007) else { panic!() };
    assert_eq!(line.extend, Extend::Repeat);
    let Paint::Linear { line, .. } = inner(0xE008) else { panic!() };
    assert_eq!(line.extend, Extend::Reflect);
}

#[test]
fn transforms_are_read_as_matrices_about_their_centres() {
    let data = read("wp-colour-one.ttf");
    let font = Font::parse(&data).unwrap();
    // A quarter turn anticlockwise about (500, 400).
    let tree = font.colour_glyph(glyph(&font, 0xE003)).unwrap();
    let Paint::Transform { matrix, .. } = tree.paint else { panic!() };
    let expected = [0.0, 1.0, -1.0, 0.0, 900.0, -100.0];
    for (got, wanted) in matrix.iter().zip(expected) {
        assert!((got - wanted).abs() < 0.01, "{matrix:?}");
    }
    // Forty-five degrees of skew along x: the format's xy is minus the
    // tangent.
    let tree = font.colour_glyph(glyph(&font, 0xE009)).unwrap();
    let Paint::Transform { matrix, .. } = tree.paint else { panic!() };
    assert!((matrix[2] + 1.0).abs() < 0.01 && matrix[1].abs() < 0.01, "{matrix:?}");
    // A move, round a glyph borrowed from another colour glyph and clipped
    // by that glyph's box.
    let tree = font.colour_glyph(glyph(&font, 0xE006)).unwrap();
    let Paint::Transform { matrix, paint } = tree.paint else { panic!() };
    assert_eq!(matrix, [1.0, 0.0, 0.0, 1.0, 0.0, 100.0]);
    let Paint::Clip { clip, paint } = *paint else { panic!() };
    assert_eq!(clip, (0.0, 0.0, 1000.0, 800.0));
    assert!(matches!(*paint, Paint::Glyph { .. }));
}

#[test]
fn composites_layers_and_the_colour_of_the_text_are_read() {
    let data = read("wp-colour-one.ttf");
    let font = Font::parse(&data).unwrap();
    let tree = font.colour_glyph(glyph(&font, 0xE004)).unwrap();
    let Paint::Composite { mode, .. } = tree.paint else { panic!() };
    assert_eq!(mode, CompositeMode::Multiply);
    let tree = font.colour_glyph(glyph(&font, 0xE00A)).unwrap();
    let Paint::Composite { mode, .. } = tree.paint else { panic!() };
    assert_eq!(mode, CompositeMode::SourceIn);

    let tree = font.colour_glyph(glyph(&font, 0xE005)).unwrap();
    let Paint::Layers(layers) = tree.paint else { panic!() };
    assert_eq!(layers.len(), 2);
    let Paint::Glyph { paint, .. } = &layers[0] else { panic!() };
    assert_eq!(**paint, Paint::Solid(Swatch::Text { alpha: 1.0 }));

    // Half-transparent black: the palette's own opacity.
    let tree = font.colour_glyph(glyph(&font, 0xE00B)).unwrap();
    let Paint::Glyph { paint, .. } = tree.paint else { panic!() };
    let Paint::Solid(Swatch::Colour(colour)) = *paint else { panic!() };
    assert_eq!((colour.red, colour.green, colour.blue), (0, 0, 0));
    assert!((i32::from(colour.alpha) - 128).abs() <= 1, "{colour:?}");
}

#[test]
fn a_glyph_of_the_older_kind_in_the_same_table_is_still_a_list_of_layers() {
    let data = read("wp-colour-one.ttf");
    let font = Font::parse(&data).unwrap();
    let layered = glyph(&font, 0xE010);
    assert!(font.colour_glyph(layered).is_none());
    let layers = font.colour_layers(layered).expect("layers");
    assert_eq!(layers.len(), 2);
    assert_eq!(layers[0].colour, Some(RED));
    assert_eq!(layers[1].colour, Some(BLUE));
}

#[test]
fn apple_pictures_come_at_the_size_asked_for_placed_by_their_bottom_edge() {
    let data = read("wp-colour-pictures.ttf");
    let font = Font::parse(&data).unwrap();
    assert!(font.has_colour());
    let square = glyph(&font, 0xE000);
    let small = font.bitmap(square, 18).expect("a picture");
    let large = font.bitmap(square, 38).expect("a picture");
    assert_eq!((small.pixels_per_em, large.pixels_per_em), (20, 40));
    assert_eq!(large.format, ImageFormat::Png);
    assert!(large.from_bottom);
    assert_eq!((large.bearing_x, large.bearing_y), (0, 0));
    assert_eq!(&large.data[1..4], b"PNG");

    // A glyph that says it is the same picture as another is that picture.
    let same = font.bitmap(glyph(&font, 0xE001), 38).expect("a picture");
    assert_eq!(same.data, large.data);

    // And a picture of the other kind, placed off the corner.
    let tiled = font.bitmap(glyph(&font, 0xE002), 38).expect("a picture");
    assert_eq!(tiled.format, ImageFormat::Tiff);
    assert_eq!((tiled.bearing_x, tiled.bearing_y), (10, -10));
}

#[test]
fn a_drawing_is_found_for_its_glyph_compressed_or_not() {
    let data = read("wp-colour-drawings.ttf");
    let font = Font::parse(&data).unwrap();
    assert!(font.has_colour());
    let gradient = glyph(&font, 0xE000);
    let document = font.svg_document(gradient).expect("a document");
    assert_eq!((document.first, document.last), (gradient, gradient));
    let text = String::from_utf8_lossy(document.data);
    assert!(text.contains(&format!("id=\"glyph{}\"", gradient.0)), "{text}");

    let radial = font.svg_document(glyph(&font, 0xE002)).expect("a document");
    assert_eq!(&radial.data[..2], &[0x1F, 0x8B], "not compressed");
    assert!(font.svg_document(GlyphId(0)).is_none());
}
