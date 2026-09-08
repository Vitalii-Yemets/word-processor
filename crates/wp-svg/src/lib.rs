//! Just enough SVG to draw an icon.
//!
//! # Why only enough
//!
//! SVG is an enormous format — filters, gradients, clipping, text, animation,
//! scripting. None of that is wanted here. What is wanted is the one thing an
//! icon set is made of: a viewBox and some filled outlines. So that is what
//! this reads, and it says so rather than pretending to be an SVG renderer.
//!
//! The document is parsed by `wp-xml`, the same parser the `.docx` parts go
//! through; only the path data needs a reader of its own, and that is in
//! [`path`].
//!
//! # Example
//!
//! ```
//! let drawing = wp_svg::Drawing::parse(
//!     r#"<svg viewBox="0 0 20 20"><path d="M0 0H20V20H0Z"/></svg>"#,
//! )?;
//! assert_eq!(drawing.view_box, (0.0, 0.0, 20.0, 20.0));
//! assert_eq!(drawing.shapes.len(), 1);
//! # Ok::<(), wp_svg::Error>(())
//! ```

#![forbid(unsafe_code)]

pub mod path;

use wp_raster::{Path, Transform};
use wp_xml::tree::{Element, XmlTree};

pub use path::parse as parse_path;

/// Why a drawing could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The file is not valid XML.
    Xml(wp_xml::Error),
    /// The root element is not `<svg>`.
    NotAnSvg,
    /// A `d` attribute could not be read.
    Path(path::Error),
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Xml(error) => write!(f, "not valid XML: {error}"),
            Self::NotAnSvg => f.write_str("the root element is not <svg>"),
            Self::Path(error) => write!(f, "bad path data: {error}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<wp_xml::Error> for Error {
    fn from(error: wp_xml::Error) -> Self {
        Self::Xml(error)
    }
}

impl From<path::Error> for Error {
    fn from(error: path::Error) -> Self {
        Self::Path(error)
    }
}

/// One filled outline of a drawing.
#[derive(Clone, Debug)]
pub struct Shape {
    pub outline: Path,
    /// The colour the file asks for, when it asks for one that is not the
    /// default. An icon meant to be tinted says `currentColor` or nothing, and
    /// this is `None` for both.
    pub fill: Option<wp_raster::Color>,
    /// Whether the file asked for the even-odd fill rule.
    ///
    /// Kept because throwing it away would silently fill the holes in of any
    /// shape that relies on it; the renderer decides what to do about it.
    pub even_odd: bool,
}

/// A drawing: the box it is composed in, and the shapes in it.
#[derive(Clone, Debug)]
pub struct Drawing {
    /// `(x, y, width, height)`, as the `viewBox` attribute gives them.
    pub view_box: (f32, f32, f32, f32),
    pub shapes: Vec<Shape>,
}

impl Drawing {
    /// Reads an SVG file.
    pub fn parse(text: &str) -> Result<Self, Error> {
        let tree = XmlTree::parse(text)?;
        if tree.root.local_name() != "svg" {
            return Err(Error::NotAnSvg);
        }

        let view_box = read_view_box(&tree.root);
        let mut shapes = Vec::new();
        collect(&tree.root, &mut shapes)?;
        Ok(Self { view_box, shapes })
    }

    /// The drawing scaled to fit a square of the given side, at a point.
    ///
    /// Aspect ratio is kept and the result is centred, which is what
    /// `preserveAspectRatio` defaults to and what an icon in a button wants.
    #[must_use]
    pub fn placed(&self, x: f32, y: f32, size: f32) -> Vec<Shape> {
        let (box_x, box_y, box_width, box_height) = self.view_box;
        if box_width <= 0.0 || box_height <= 0.0 {
            return Vec::new();
        }
        let scale = (size / box_width).min(size / box_height);
        let offset_x = x + (size - box_width * scale) / 2.0;
        let offset_y = y + (size - box_height * scale) / 2.0;

        let transform = Transform {
            a: scale,
            b: 0.0,
            c: 0.0,
            d: scale,
            e: offset_x - box_x * scale,
            f: offset_y - box_y * scale,
        };

        self.shapes
            .iter()
            .map(|shape| Shape {
                outline: shape.outline.transformed(&transform),
                fill: shape.fill,
                even_odd: shape.even_odd,
            })
            .collect()
    }
}

/// The `viewBox`, or the unit square when there is not one.
fn read_view_box(root: &Element) -> (f32, f32, f32, f32) {
    let numbers: Option<Vec<f32>> = root.attribute_by_name("viewBox").map(|text| {
        text.split(|c: char| c.is_ascii_whitespace() || c == ',')
            .filter(|part| !part.is_empty())
            .filter_map(|part| part.parse().ok())
            .collect()
    });

    match numbers.as_deref() {
        Some([x, y, width, height]) => (*x, *y, *width, *height),
        // Without a viewBox the width and height attributes stand in for one.
        _ => {
            let width = root.attribute_by_name("width").and_then(|v| v.parse().ok()).unwrap_or(1.0);
            let height =
                root.attribute_by_name("height").and_then(|v| v.parse().ok()).unwrap_or(1.0);
            (0.0, 0.0, width, height)
        }
    }
}

/// Walks the tree collecting every `<path>`, however deeply grouped.
fn collect(element: &Element, shapes: &mut Vec<Shape>) -> Result<(), Error> {
    for child in element.child_elements() {
        if child.local_name() == "path" {
            let Some(data) = child.attribute_by_name("d") else { continue };
            let fill = child.attribute_by_name("fill");
            // "none" means the shape is not filled at all, so there is nothing
            // to draw; the icons here are all fills, never strokes.
            if fill == Some("none") {
                continue;
            }
            shapes.push(Shape {
                outline: path::parse(data)?,
                fill: fill
                    .filter(|value| *value != "currentColor")
                    .and_then(wp_raster::Color::from_hex),
                even_odd: child.attribute_by_name("fill-rule") == Some("evenodd"),
            });
        }
        collect(child, shapes)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SQUARE: &str = r##"<svg width="20" height="20" viewBox="0 0 20 20" fill="none"
        xmlns="http://www.w3.org/2000/svg"><path d="M2 2H18V18H2Z" fill="#212121"/></svg>"##;

    #[test]
    fn a_drawing_carries_its_view_box_and_its_shapes() {
        let drawing = Drawing::parse(SQUARE).expect("a drawing");
        assert_eq!(drawing.view_box, (0.0, 0.0, 20.0, 20.0));
        assert_eq!(drawing.shapes.len(), 1);
    }

    #[test]
    fn the_root_fill_of_none_is_not_mistaken_for_a_shape() {
        // The <svg> element says fill="none"; only the <path> is a shape.
        assert_eq!(Drawing::parse(SQUARE).expect("a drawing").shapes.len(), 1);
    }

    #[test]
    fn a_path_that_is_not_filled_is_skipped() {
        let svg = r#"<svg viewBox="0 0 10 10"><path d="M0 0H10" fill="none"/></svg>"#;
        assert!(Drawing::parse(svg).expect("a drawing").shapes.is_empty());
    }

    #[test]
    fn paths_inside_groups_are_found_too() {
        let svg = r#"<svg viewBox="0 0 10 10"><g><g><path d="M0 0H10V10Z"/></g></g></svg>"#;
        assert_eq!(Drawing::parse(svg).expect("a drawing").shapes.len(), 1);
    }

    #[test]
    fn placing_scales_the_drawing_to_the_size_asked_for() {
        let drawing = Drawing::parse(SQUARE).expect("a drawing");
        let placed = drawing.placed(100.0, 200.0, 40.0);
        let wp_raster::Command::MoveTo(first) = placed[0].outline.commands[0] else {
            panic!("a move")
        };
        // (2, 2) in a twenty-unit box, drawn at forty pixels, is four pixels in.
        assert_eq!((first.x, first.y), (104.0, 204.0));
    }

    #[test]
    fn something_that_is_not_an_svg_is_refused() {
        assert!(matches!(Drawing::parse("<html/>"), Err(Error::NotAnSvg)));
    }

    #[test]
    fn a_missing_view_box_falls_back_to_the_width_and_height() {
        let svg = r#"<svg width="24" height="24"><path d="M0 0H24V24Z"/></svg>"#;
        assert_eq!(Drawing::parse(svg).expect("a drawing").view_box, (0.0, 0.0, 24.0, 24.0));
    }
}
