//! One page as a PDF draws it: a little stack language of instructions.
//!
//! `1 0 0 1 72 720 Tm` moves the pen, `<0041> Tj` draws a glyph, `re f` fills a
//! rectangle. There are no paragraphs and no lines in a PDF — by the time
//! anything is written here every decision has been taken, and what is left is
//! where each mark goes.
//!
//! # Which way up
//!
//! A PDF measures from the bottom left of the page, upwards. Everything in this
//! program measures from the top left, downwards, because that is how a screen
//! is addressed. So every vertical position is turned round here, once, rather
//! than the whole page being flipped — a flipped page would draw the text
//! upside down along with everything else.

use std::collections::{BTreeMap, BTreeSet};

use wp_layout::{FontLibrary, Page, PositionedGlyph, Turn};
use wp_raster::{Color, Command, Path};

use crate::writer::number;

/// A page's instructions, and what it needs to carry them out.
#[derive(Debug, Default)]
pub(crate) struct Drawing {
    pub stream: String,
    /// The fonts this page draws with, by the name they go by in the file.
    pub fonts: BTreeSet<String>,
    /// The pictures it draws, in the order it draws them.
    pub images: Vec<(String, Picture)>,
    /// The degrees of transparency it uses, which a PDF holds as a named state
    /// rather than as part of a colour.
    pub fades: Vec<(String, u8)>,
    /// Which faces draw in colours of their own, asked once a face rather
    /// than once a glyph: reading a font to ask is not free.
    pub colour: std::collections::HashMap<usize, bool>,
}

/// A picture, taken apart the way a PDF holds one: the colours, and the
/// transparency as a picture of its own.
#[derive(Debug)]
pub(crate) struct Picture {
    pub width: usize,
    pub height: usize,
    pub colours: Vec<u8>,
    /// `None` where every pixel is opaque, which is most of them.
    pub alpha: Option<Vec<u8>>,
}

/// Turns a page into instructions.
pub(crate) fn of(page: &Page, names: &BTreeMap<usize, String>, library: &FontLibrary) -> Drawing {
    let mut drawing = Drawing::default();
    let mut out = String::new();
    let height = page.height;

    // The same order the screen is drawn in, because the order is what decides
    // which of two overlapping things is seen.
    for placed in page.drawings_under() {
        write_drawing(&mut out, placed, height, &mut drawing);
    }

    for placed in &page.paths {
        fill(&mut out, &placed.path, placed.color, height, &mut drawing);
    }

    for decoration in &page.decorations {
        let top = height - decoration.y - decoration.height;
        out.push_str(&paint(decoration.color, &mut drawing));
        out.push_str(&format!(
            "{} {} {} {} re f\n",
            number(decoration.x),
            number(top),
            number(decoration.width),
            number(decoration.height),
        ));
    }

    // Each glyph with which way it is turned: a heading down a cell, the
    // ideographs of a section written down the page standing upright.
    let inside = page.shapes.iter().filter(|shape| !shape.over_text).flat_map(|shape| {
        shape.text.iter().enumerate().map(|(at, glyph)| (glyph, shape.text_turn_of(at)))
    });
    let glyphs: Vec<(&PositionedGlyph, Turn)> = page
        .glyphs
        .iter()
        .enumerate()
        .map(|(at, glyph)| (glyph, page.turn_of(at)))
        .chain(inside)
        .filter(|(glyph, _)| !glyph.invisible && glyph.size > 0.0)
        .collect();
    write_text(&mut out, &glyphs, names, library, height, &mut drawing);

    // What a person put in front of the text, over it, with the words inside a
    // shape written after the shape so they are not painted over.
    for placed in page.drawings_over() {
        write_drawing(&mut out, placed, height, &mut drawing);
        if let wp_layout::Drawing::Shape(shape) = placed {
            let glyphs: Vec<(&PositionedGlyph, Turn)> = shape
                .text
                .iter()
                .enumerate()
                .map(|(at, glyph)| (glyph, shape.text_turn_of(at)))
                .filter(|(glyph, _)| !glyph.invisible && glyph.size > 0.0)
                .collect();
            write_text(&mut out, &glyphs, names, library, height, &mut drawing);
        }
    }

    drawing.stream = out;
    drawing
}

/// Writes one drawing's body into the page's instructions.
fn write_drawing(
    out: &mut String,
    placed: wp_layout::Drawing<'_>,
    height: f32,
    drawing: &mut Drawing,
) {
    match placed {
        wp_layout::Drawing::Ink(ink) => {
            for (path, colour) in &ink.drawing.paths {
                fill(out, path, *colour, height, drawing);
            }
        }
        wp_layout::Drawing::Picture(picture) => {
            // Named by how many have gone in already, which is what the page's
            // resource dictionary will call it.
            let name = format!("Im{}", drawing.images.len());
            let top = height - picture.y - picture.height;
            out.push_str(&format!(
                "q {} 0 0 {} {} {} cm /{name} Do Q\n",
                number(picture.width),
                number(picture.height),
                number(picture.x),
                number(top),
            ));
            drawing.images.push((name, take_apart(&picture.image)));
        }
        wp_layout::Drawing::Shape(shape) => {
            // The shadow, flat: a PDF fills in one colour, so a blur would have
            // to be written as a picture of itself. What is drawn here is the
            // shape again, offset, in the shadow's colour — which is the shadow
            // without the blur, and is what the theme's own shadow amounts to
            // anyway. Drawing a blurred shadow properly is the roadmap's
            // F-series work on what a PDF can carry.
            let cast = shape.effects.outer_shadow.map_or(shape.shadow, |shadow| {
                Some((shadow.colour, shadow.across.max(shadow.down)))
            });
            if let Some((colour, distance)) = cast {
                let path = wp_layout::geometry::path_in(
                    shape.preset,
                    &shape.adjusts,
                    shape.x + distance,
                    shape.y + distance,
                    shape.width,
                    shape.height,
                );
                fill(out, &path, colour, height, drawing);
            }
            // A PDF fills a shape in one colour; a gradient is written as the
            // colour at its middle, which is the nearest one colour to it.
            // Drawing the shade itself is the roadmap's F-series work on what
            // a PDF can carry.
            if let Some(colour) = shape.fill.colour() {
                let path = wp_layout::geometry::path_in(
                    shape.preset,
                    &shape.adjusts,
                    shape.x,
                    shape.y,
                    shape.width,
                    shape.height,
                );
                fill(out, &path, colour, height, drawing);
            }
            if let Some(colour) = shape.outline {
                // A routed connector is drawn from its route, the same as on
                // the screen.
                let along = shape.route.clone().unwrap_or_else(|| {
                    wp_layout::geometry::path_in(
                        shape.preset,
                        &shape.adjusts,
                        shape.x,
                        shape.y,
                        shape.width,
                        shape.height,
                    )
                });
                let band = if shape.route.is_some() {
                    wp_layout::geometry::band_along(&along, shape.outline_weight)
                } else {
                    wp_layout::geometry::outline_in(
                        shape.preset,
                        &shape.adjusts,
                        shape.x,
                        shape.y,
                        shape.width,
                        shape.height,
                        shape.outline_weight,
                    )
                };
                fill(out, &band, colour, height, drawing);

                // And the arrowheads, which are drawn with the same line.
                for (tip, end) in [
                    (wp_layout::connectors::LineTip::Head, shape.head_end),
                    (wp_layout::connectors::LineTip::Tail, shape.tail_end),
                ] {
                    let head =
                        wp_layout::connectors::arrowhead(&along, tip, end, shape.outline_weight);
                    if !head.is_empty() {
                        fill(out, &head, colour, height, drawing);
                    }
                }
            }
        }
    }
}

/// Writes the text, a run at a time.
///
/// A run is glyphs that share a font, a size, a colour and a baseline — which
/// is what lets them be written as one string with the gaps between them given
/// as numbers, the way every PDF holds a line of text.
fn write_text(
    out: &mut String,
    glyphs: &[(&PositionedGlyph, Turn)],
    names: &BTreeMap<usize, String>,
    library: &FontLibrary,
    height: f32,
    drawing: &mut Drawing,
) {
    let mut index = 0;
    while index < glyphs.len() {
        let (first, turn) = glyphs[index];

        // A glyph a font keeps as a picture is drawn as a picture. There is no
        // outline to fill and no font to embed that would draw it: written as
        // text it would be nothing at all on the page.
        if write_picture_glyph(out, first, names, library, height, drawing) {
            index += 1;
            continue;
        }
        // And a glyph drawn as layers of other glyphs is drawn as those
        // layers, each in its own colour.
        if write_layered_glyph(out, first, turn, names, library, height, drawing) {
            index += 1;
            continue;
        }

        let Some(name) = names.get(&first.face) else {
            index += 1;
            continue;
        };
        // A glyph turned, or drawn beside the pen rather than at it, is
        // written on its own with a text matrix that says so: such glyphs
        // are few, and a run of them would need the gaps worked out along
        // the turned line.
        let alone = turn != Turn::None || first.shift_x != 0.0 || first.shift_y != 0.0;
        let mut end = index + 1;
        while end < glyphs.len() && !alone {
            let (next, next_turn) = glyphs[end];
            let same = next.face == first.face
                && next_turn == Turn::None
                && next.shift_x == 0.0
                && next.shift_y == 0.0
                && (next.size - first.size).abs() < 0.01
                && (next.stretch - first.stretch).abs() < 0.001
                && next.color == first.color
                && (next.baseline - first.baseline).abs() < 0.01
                // Only forwards: a glyph drawn to the left of the one before it
                // is a new run, or the gap between them would be a negative
                // that a reader takes for a space.
                && next.x >= first.x;
            if !same {
                break;
            }
            end += 1;
        }

        let run: Vec<&PositionedGlyph> =
            glyphs[index..end].iter().map(|(glyph, _)| *glyph).collect();
        let run = run.as_slice();
        drawing.fonts.insert(name.clone());
        out.push_str(&paint(first.color, drawing));
        // The text matrix: where the pen is, and which way the letter is
        // turned. A PDF's y runs up the page, so a quarter turn clockwise on
        // the page is a quarter turn the other way here.
        let (a, b, c, d) = match turn {
            Turn::None | Turn::Upright => (1.0, 0.0, 0.0, 1.0),
            Turn::Down => (0.0, -1.0, 1.0, 0.0),
            Turn::Up => (0.0, 1.0, -1.0, 0.0),
            Turn::Over => (-1.0, 0.0, 0.0, -1.0),
        };
        out.push_str(&format!(
            "BT /{name} {} Tf {} {} {} {} {} {} Tm\n",
            number(first.size),
            number(a),
            number(b),
            number(c),
            number(d),
            number(first.x + first.shift_x),
            number(height - first.baseline - first.shift_y),
        ));
        // Letters drawn wider or narrower than they are tall. `Tz` is part of
        // the text state and outlives the block that set it, so it is put back
        // afterwards rather than left standing for whatever is drawn next.
        let stretched = (first.stretch - 1.0).abs() > 0.001;
        if stretched {
            out.push_str(&format!("{} Tz\n", number(first.stretch * 100.0)));
        }
        out.push_str(&positions(run, first.size, first.stretch));
        out.push_str(" TJ\n");
        if stretched {
            out.push_str("100 Tz\n");
        }
        out.push_str("ET\n");
        index = end;
    }
}

/// The glyphs of one run, with the gaps between them.
///
/// A PDF draws a string and moves the pen by each glyph's own width. Where the
/// layout put a glyph somewhere else — because a space was stretched to justify
/// the line, or a tab reached a stop — the difference is written between the
/// two as a number, in thousandths of the text size, and negative because the
/// number moves the pen back.
///
/// The horizontal scale is divided out of it: `Tz` scales these numbers along
/// with everything else on the line, so a gap of a tenth of an em written into
/// text set at half width would come out a twentieth.
fn positions(run: &[&PositionedGlyph], size: f32, stretch: f32) -> String {
    let mut out = String::from("[");
    let mut hex = String::new();
    let mut pen = run[0].x;
    let stretch = if stretch.abs() < 0.001 { 1.0 } else { stretch };

    for glyph in run {
        let gap = (glyph.x - pen) / stretch;
        if gap.abs() > 0.01 && size > 0.0 {
            if !hex.is_empty() {
                out.push_str(&format!("<{hex}>"));
                hex.clear();
            }
            out.push_str(&number(-gap * 1000.0 / size));
        }
        hex.push_str(&format!("{:04X}", glyph.glyph.0));
        pen = glyph.x + glyph.advance;
    }

    if !hex.is_empty() {
        out.push_str(&format!("<{hex}>"));
    }
    out.push(']');
    out
}

/// Fills a path, turning it the right way up on the way.
fn fill(out: &mut String, path: &Path, colour: Color, height: f32, drawing: &mut Drawing) {
    if path.commands.is_empty() {
        return;
    }
    out.push_str(&paint(colour, drawing));

    for command in &path.commands {
        let up = |y: f32| number(height - y);
        match command {
            Command::MoveTo(point) => {
                out.push_str(&format!("{} {} m\n", number(point.x), up(point.y)));
            }
            Command::LineTo(point) => {
                out.push_str(&format!("{} {} l\n", number(point.x), up(point.y)));
            }
            // A PDF has no quadratic curve, so it is written as the cubic that
            // draws the same line: the two control points sit two thirds of the
            // way from each end towards the one the quadratic had.
            Command::QuadTo(control, end) => {
                out.push_str(&format!(
                    "{} {} {} {} {} {} c\n",
                    number(control.x),
                    up(control.y),
                    number(control.x),
                    up(control.y),
                    number(end.x),
                    up(end.y),
                ));
            }
            Command::CubicTo(first, second, end) => {
                out.push_str(&format!(
                    "{} {} {} {} {} {} c\n",
                    number(first.x),
                    up(first.y),
                    number(second.x),
                    up(second.y),
                    number(end.x),
                    up(end.y),
                ));
            }
            Command::Close => out.push_str("h\n"),
        }
    }
    // Filled the same way the rasterizer fills it: a hole inside a shape is a
    // hole because the two contours wind opposite ways.
    out.push_str("f\n");
}

/// Sets the colour to paint with, and the transparency if it is not opaque.
fn paint(colour: Color, drawing: &mut Drawing) -> String {
    let channel = |value: u8| number(f32::from(value) / 255.0);
    let mut out = String::new();

    if colour.alpha < 255 {
        let name = format!("Fade{}", colour.alpha);
        if !drawing.fades.iter().any(|(found, _)| *found == name) {
            drawing.fades.push((name.clone(), colour.alpha));
        }
        out.push_str(&format!("/{name} gs "));
    } else if drawing.fades.iter().any(|(_, alpha)| *alpha < 255) {
        out.push_str("/FadeNone gs ");
        if !drawing.fades.iter().any(|(found, _)| found == "FadeNone") {
            drawing.fades.push(("FadeNone".to_owned(), 255));
        }
    }

    out.push_str(&format!(
        "{} {} {} rg\n",
        channel(colour.red),
        channel(colour.green),
        channel(colour.blue)
    ));
    out
}

/// A glyph a font keeps as a picture, drawn as one.
///
/// An emoji in a font of this kind is not an outline and cannot be written as
/// text: there is nothing to fill. It goes into the file as a small picture in
/// the place the glyph would have stood, which is what it is.
fn write_picture_glyph(
    out: &mut String,
    glyph: &PositionedGlyph,
    names: &BTreeMap<usize, String>,
    library: &FontLibrary,
    height: f32,
    drawing: &mut Drawing,
) -> bool {
    if !has_colour(library, glyph.face, drawing) {
        return false;
    }
    let Some(font) = library.face(glyph.face).and_then(|face| face.font()) else { return false };
    // Painted or drawn four times finer than the text it stands in, which is
    // what a picture has to be to print as sharply as the letters round it;
    // a picture the font keeps comes at the nearest size it holds.
    let pixels = (glyph.size * 4.0).clamp(16.0, 512.0);
    let Some(picture) = wp_layout::colourglyph::picture_of(&font, glyph.glyph, pixels, glyph.color)
    else {
        return false;
    };
    if picture.width == 0 || picture.height == 0 {
        return false;
    }
    let image =
        wp_layout::Image { width: picture.width, height: picture.height, pixels: picture.pixels };

    let scale = glyph.size / picture.pixels_per_em.max(1.0);
    let width = image.width as f32 * scale;
    let tall = image.height as f32 * scale;
    let left = glyph.x + picture.left * scale;
    // A PDF measures up from the foot of the page and places a picture by its
    // bottom edge; the layout measures down from the top and places a glyph by
    // its baseline.
    let bottom = height - (glyph.baseline - picture.top * scale) - tall;

    // A picture is not text, and what it stands for is said beside it, so
    // that copying the emoji out of the page gives the emoji.
    let meant = actual_text(&font, glyph.glyph);
    if let Some(meant) = &meant {
        out.push_str(meant);
    }
    let name = format!("Im{}", drawing.images.len());
    out.push_str(&format!(
        "q {} 0 0 {} {} {} cm /{name} Do Q\n",
        number(width),
        number(tall),
        number(left),
        number(bottom),
    ));
    // And the glyph itself, where the font can be carried, as text drawn in
    // no colour at all: what the readers that ignore a stretch's own text
    // for a picture copy out instead, through the font's table of what each
    // glyph says. The mode is put back inside the same block, because it
    // outlives the block that set it.
    if font.has_outlines() {
        if let Some(face) = names.get(&glyph.face) {
            drawing.fonts.insert(face.clone());
            out.push_str(&format!(
                "BT 3 Tr /{face} {} Tf 1 0 0 1 {} {} Tm [<{:04X}>] TJ 0 Tr ET\n",
                number(glyph.size),
                number(glyph.x),
                number(height - glyph.baseline),
                glyph.glyph.0,
            ));
        }
    }
    if meant.is_some() {
        out.push_str("EMC\n");
    }
    drawing.images.push((name, take_apart(&image)));
    true
}

/// The opening of a stretch of the page that says what text it stands for:
/// the character the font maps to a glyph, for a glyph drawn some other way
/// than as a glyph of text. `None` where the font maps nothing to it.
fn actual_text(font: &wp_font::Font<'_>, glyph: wp_font::GlyphId) -> Option<String> {
    let character = font
        .character_map()
        .pairs()
        .into_iter()
        .find(|(_, mapped)| *mapped == glyph)
        .map(|(character, _)| character)?;
    let mut text = String::from("FEFF");
    let mut buffer = [0u16; 2];
    for unit in character.encode_utf16(&mut buffer) {
        text.push_str(&format!("{unit:04X}"));
    }
    Some(format!("/Span << /ActualText <{text}> >> BDC\n"))
}

/// Whether a face draws in colours of its own, asked of the font once.
fn has_colour(library: &FontLibrary, face: usize, drawing: &mut Drawing) -> bool {
    *drawing.colour.entry(face).or_insert_with(|| {
        library.face(face).and_then(|entry| entry.font()).is_some_and(|font| font.has_colour())
    })
}

/// A glyph drawn as layers of other glyphs, written as those layers: one
/// run of text for each, in the layer's colour or the text's.
///
/// The layer glyphs are in the font the file carries — the font is cut with
/// them in it — and they draw exactly as the screen draws them, in
/// outlines. What they spell is said once for all of them, as the text the
/// glyph stands for, so that copying the emoji out of the page gives the
/// emoji and not a run of shapes nobody typed.
#[allow(clippy::too_many_arguments)]
fn write_layered_glyph(
    out: &mut String,
    glyph: &PositionedGlyph,
    turn: Turn,
    names: &BTreeMap<usize, String>,
    library: &FontLibrary,
    height: f32,
    drawing: &mut Drawing,
) -> bool {
    if !has_colour(library, glyph.face, drawing) {
        return false;
    }
    let Some(font) = library.face(glyph.face).and_then(|face| face.font()) else { return false };
    let Some(layers) = font.colour_layers(glyph.glyph) else { return false };
    let Some(name) = names.get(&glyph.face) else { return false };
    if layers.is_empty() {
        return false;
    }

    let meant = actual_text(&font, glyph.glyph);
    if let Some(meant) = &meant {
        out.push_str(meant);
    }
    let (a, b, c, d) = match turn {
        Turn::None | Turn::Upright => (1.0, 0.0, 0.0, 1.0),
        Turn::Down => (0.0, -1.0, 1.0, 0.0),
        Turn::Up => (0.0, 1.0, -1.0, 0.0),
        Turn::Over => (-1.0, 0.0, 0.0, -1.0),
    };
    drawing.fonts.insert(name.clone());
    for layer in layers {
        let colour = layer.colour.map_or(glyph.color, |colour| Color {
            red: colour.red,
            green: colour.green,
            blue: colour.blue,
            alpha: colour.alpha,
        });
        out.push_str(&paint(colour, drawing));
        out.push_str(&format!(
            "BT /{name} {} Tf {} {} {} {} {} {} Tm [<{:04X}>] TJ ET\n",
            number(glyph.size),
            number(a),
            number(b),
            number(c),
            number(d),
            number(glyph.x + glyph.shift_x),
            number(height - glyph.baseline - glyph.shift_y),
            layer.glyph.0,
        ));
    }
    if meant.is_some() {
        out.push_str("EMC\n");
    }
    true
}

/// Splits a picture into the colours and the transparency, which a PDF holds
/// separately.
fn take_apart(image: &wp_layout::Image) -> Picture {
    let count = image.width * image.height;
    let mut colours = Vec::with_capacity(count * 3);
    let mut alpha = Vec::with_capacity(count);
    let mut clear = false;

    for pixel in image.pixels.chunks_exact(4) {
        colours.extend_from_slice(&pixel[..3]);
        alpha.push(pixel[3]);
        clear |= pixel[3] != 255;
    }

    Picture { width: image.width, height: image.height, colours, alpha: clear.then_some(alpha) }
}

#[cfg(test)]
mod tests {
    use wp_layout::{Decoration, FontLibrary, Page};
    use wp_raster::Color;

    use super::of;

    #[test]
    fn a_rectangle_is_turned_the_right_way_up() {
        // Ten points down from the top of a hundred-point page, and four tall:
        // eighty-six points up from the bottom.
        let page = Page {
            width: 100.0,
            height: 100.0,
            decorations: vec![Decoration {
                x: 5.0,
                y: 10.0,
                width: 20.0,
                height: 4.0,
                color: Color::rgb(0, 0, 0),
            }],
            ..Page::default()
        };
        let drawing = of(&page, &Default::default(), &FontLibrary::new());
        assert!(drawing.stream.contains("5 86 20 4 re f"), "{}", drawing.stream);
    }

    #[test]
    fn a_colour_is_written_as_three_fractions() {
        let page = Page {
            width: 10.0,
            height: 10.0,
            decorations: vec![Decoration {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
                color: Color::rgb(255, 0, 128),
            }],
            ..Page::default()
        };
        let drawing = of(&page, &Default::default(), &FontLibrary::new());
        assert!(drawing.stream.contains("1 0 0.502 rg"), "{}", drawing.stream);
    }

    #[test]
    fn an_empty_page_draws_nothing() {
        let page = Page { width: 10.0, height: 10.0, ..Page::default() };
        assert!(of(&page, &Default::default(), &FontLibrary::new()).stream.is_empty());
    }
}
