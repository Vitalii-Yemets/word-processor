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

use wp_layout::{Page, PositionedGlyph};
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
pub(crate) fn of(page: &Page, names: &BTreeMap<usize, String>) -> Drawing {
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

    let inside =
        page.shapes.iter().filter(|shape| !shape.over_text).flat_map(|shape| shape.text.iter());
    let glyphs: Vec<&PositionedGlyph> = page
        .glyphs
        .iter()
        .chain(inside)
        .filter(|glyph| !glyph.invisible && glyph.size > 0.0)
        .collect();
    write_text(&mut out, &glyphs, names, height, &mut drawing);

    // What a person put in front of the text, over it, with the words inside a
    // shape written after the shape so they are not painted over.
    for placed in page.drawings_over() {
        write_drawing(&mut out, placed, height, &mut drawing);
        if let wp_layout::Drawing::Shape(shape) = placed {
            let glyphs: Vec<&PositionedGlyph> =
                shape.text.iter().filter(|glyph| !glyph.invisible && glyph.size > 0.0).collect();
            write_text(&mut out, &glyphs, names, height, &mut drawing);
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
            if let Some((colour, distance)) = shape.shadow {
                let path = wp_layout::geometry::path_in(
                    shape.preset,
                    shape.x + distance,
                    shape.y + distance,
                    shape.width,
                    shape.height,
                );
                fill(out, &path, colour, height, drawing);
            }
            if let Some(colour) = shape.fill {
                let path = wp_layout::geometry::path_in(
                    shape.preset,
                    shape.x,
                    shape.y,
                    shape.width,
                    shape.height,
                );
                fill(out, &path, colour, height, drawing);
            }
            if let Some(colour) = shape.outline {
                let path = wp_layout::geometry::outline_in(
                    shape.preset,
                    shape.x,
                    shape.y,
                    shape.width,
                    shape.height,
                    shape.outline_weight,
                );
                fill(out, &path, colour, height, drawing);
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
    glyphs: &[&PositionedGlyph],
    names: &BTreeMap<usize, String>,
    height: f32,
    drawing: &mut Drawing,
) {
    let mut index = 0;
    while index < glyphs.len() {
        let first = glyphs[index];
        let Some(name) = names.get(&first.face) else {
            index += 1;
            continue;
        };
        let mut end = index + 1;
        while end < glyphs.len() {
            let next = glyphs[end];
            let same = next.face == first.face
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

        let run = &glyphs[index..end];
        drawing.fonts.insert(name.clone());
        out.push_str(&paint(first.color, drawing));
        out.push_str(&format!(
            "BT /{name} {} Tf 1 0 0 1 {} {} Tm\n",
            number(first.size),
            number(first.x),
            number(height - first.baseline),
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
    use wp_layout::{Decoration, Page};
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
        let drawing = of(&page, &Default::default());
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
        let drawing = of(&page, &Default::default());
        assert!(drawing.stream.contains("1 0 0.502 rg"), "{}", drawing.stream);
    }

    #[test]
    fn an_empty_page_draws_nothing() {
        let page = Page { width: 10.0, height: 10.0, ..Page::default() };
        assert!(of(&page, &Default::default()).stream.is_empty());
    }
}
