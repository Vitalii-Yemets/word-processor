//! A whole SVG document, drawn: what a font's `SVG ` table holds.
//!
//! # Why this is not [`crate::Drawing`]
//!
//! An icon is a view box and some filled outlines, and that is all the
//! [`crate::Drawing`] reads. A glyph in a font's `SVG ` table is a real
//! drawing: shapes grouped and moved by transforms, filled with gradients
//! defined elsewhere in the document, drawn again where a `<use>` points at
//! them, faded as a group, clipped to another shape. And one document may hold
//! many glyphs, each the element whose id is `glyph` and its number. So this
//! reads the document as a tree and draws the part of it asked for, into a
//! picture of layers — see [`wp_raster::compose`] — where a group's opacity
//! and a clip are the layer operations they are.
//!
//! # What is drawn
//!
//! Filled shapes: `path`, `rect` (with rounded corners), `circle`, `ellipse`,
//! `polygon`, `polyline`. Fills of a colour — every way CSS writes one — of
//! the colour the text is (`currentColor`), or of a linear or radial gradient,
//! in the document's own space or the shape's box, transformed, padded,
//! repeated or reflected, inheriting stops and settings from the gradient it
//! names. Groups, `use`, `transform` of every kind, `opacity`,
//! `fill-opacity`, `fill-rule`, `clip-path`, the `style` attribute, and
//! `display:none`.
//!
//! Not drawn: strokes — which is every `line` — text, filters, masks,
//! patterns and images. Nothing in a colour font has been seen to need them
//! that a font made for the purpose does not avoid, and each is named in the
//! roadmap rather than half done here.

use std::collections::HashMap;

use wp_raster::compose::{self, Mode, Pixmap, Premultiplied, Spread};
use wp_raster::{Color, Path, Point, Rule, Transform};
use wp_xml::tree::{Element, XmlTree};

use crate::Error;

/// A parsed SVG document, ready to have parts of it drawn.
#[derive(Clone, Debug)]
pub struct Document {
    tree: XmlTree,
    /// How large the space percentages of the document's own units are of:
    /// the em square, for a glyph.
    viewport: f32,
}

/// What a shape is filled with, as the document says it.
#[derive(Clone, Debug, PartialEq)]
enum Fill {
    None,
    Colour(Color),
    Current,
    Url(String),
}

/// What a shape inherits from the elements round it.
#[derive(Clone, Debug)]
struct Style {
    fill: Fill,
    fill_opacity: f32,
    rule: Rule,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            fill: Fill::Colour(Color { red: 0, green: 0, blue: 0, alpha: 255 }),
            fill_opacity: 1.0,
            rule: Rule::Nonzero,
        }
    }
}

/// How deep `use` and gradient references are followed before the document
/// is taken for one that points into itself.
const DEEPEST: u8 = 16;

impl Document {
    /// Reads a document.
    pub fn parse(text: &str) -> Result<Self, Error> {
        let tree = XmlTree::parse(text)?;
        if tree.root.local_name() != "svg" {
            return Err(Error::NotAnSvg);
        }
        Ok(Self { tree, viewport: 1000.0 })
    }

    /// Says how large the space is that a percentage in the document's own
    /// units is a percentage of: the em, for a glyph.
    pub fn set_viewport(&mut self, size: f32) {
        self.viewport = size;
    }

    /// Whether the document has an element with this id.
    #[must_use]
    pub fn has(&self, id: &str) -> bool {
        self.find(id).is_some()
    }

    /// How far the element with an id reaches when drawn through a transform:
    /// the box round every shape of it that is filled. `None` for an element
    /// that is not there or draws nothing.
    #[must_use]
    pub fn reach(&self, id: &str, place: &Transform) -> Option<(f32, f32, f32, f32)> {
        let element = self.find(id)?;
        let mut out = None;
        self.reach_of(element, place, &self.inherited(id), &mut out, 0);
        out
    }

    /// Draws the element with an id into a picture, through a transform,
    /// with `currentColor` the colour given. Whether there was such an
    /// element.
    pub fn draw(&self, id: &str, into: &mut Pixmap, place: &Transform, current: Color) -> bool {
        let Some(element) = self.find(id) else { return false };
        let style = self.inherited(id);
        self.draw_element(element, into, place, &style, current, 0);
        true
    }

    /// The element with an id, anywhere in the document.
    fn find(&self, id: &str) -> Option<&Element> {
        fn search<'e>(element: &'e Element, id: &str) -> Option<&'e Element> {
            if element.attribute_by_name("id") == Some(id) {
                return Some(element);
            }
            element.child_elements().find_map(|child| search(child, id))
        }
        search(&self.tree.root, id)
    }

    /// What an element inherits from the elements it sits inside.
    fn inherited(&self, id: &str) -> Style {
        fn walk(element: &Element, id: &str, style: &Style) -> Option<Style> {
            let own = styled(element, style);
            if element.attribute_by_name("id") == Some(id) {
                return Some(style.clone());
            }
            element.child_elements().find_map(|child| walk(child, id, &own))
        }
        walk(&self.tree.root, id, &Style::default()).unwrap_or_default()
    }

    fn draw_element(
        &self,
        element: &Element,
        into: &mut Pixmap,
        place: &Transform,
        style: &Style,
        current: Color,
        depth: u8,
    ) {
        if depth > DEEPEST || declared(element, "display") == Some("none") {
            return;
        }
        let place = own_transform(element).then(place);
        let style = styled(element, style);
        let opacity = declared(element, "opacity").and_then(number).unwrap_or(1.0).clamp(0.0, 1.0);
        let clip = declared(element, "clip-path").and_then(url).and_then(|id| self.find(id));

        // Drawn on a layer of its own where it is faded or clipped as a
        // whole, and straight onto the picture otherwise.
        let apart = opacity < 1.0 || clip.is_some();
        let mut layer =
            if apart { Pixmap::new(into.width(), into.height()) } else { Pixmap::new(0, 0) };
        let target: &mut Pixmap = if apart { &mut layer } else { into };

        match element.local_name() {
            "g" | "svg" | "a" | "switch" => {
                for child in element.child_elements() {
                    self.draw_element(child, target, &place, &style, current, depth + 1);
                }
            }
            "use" => {
                let reference = element
                    .attribute_by_name("xlink:href")
                    .or_else(|| element.attribute_by_name("href"))
                    .and_then(|href| href.strip_prefix('#'));
                if let Some(used) = reference.and_then(|id| self.find(id)) {
                    let x = element.attribute_by_name("x").and_then(number).unwrap_or(0.0);
                    let y = element.attribute_by_name("y").and_then(number).unwrap_or(0.0);
                    let moved = Transform::translate(x, y).then(&place);
                    self.draw_element(used, target, &moved, &style, current, depth + 1);
                }
            }
            _ => {
                if let Some(shape) = shape_of(element) {
                    self.fill(&shape, target, &place, &style, current, depth);
                }
            }
        }

        if apart {
            if let Some(clip) = clip {
                let mut outline = Path::new();
                for child in clip.child_elements() {
                    if let Some(shape) = shape_of(child) {
                        let moved = own_transform(child).then(&own_transform(clip)).then(&place);
                        outline.commands.extend(shape.transformed(&moved).commands);
                    }
                }
                layer.keep_inside(&outline, Rule::Nonzero);
            }
            layer.fade(opacity);
            into.composite(&layer, Mode::SourceOver);
        }
    }

    /// Fills one shape.
    fn fill(
        &self,
        shape: &Path,
        into: &mut Pixmap,
        place: &Transform,
        style: &Style,
        current: Color,
        depth: u8,
    ) {
        let opacity = style.fill_opacity.clamp(0.0, 1.0);
        let solid = |colour: Color| {
            let mut colour =
                compose::premultiplied(colour.red, colour.green, colour.blue, colour.alpha);
            for channel in &mut colour {
                *channel *= opacity;
            }
            colour
        };
        let mut layer = match &style.fill {
            Fill::None => return,
            Fill::Colour(colour) => {
                let colour = solid(*colour);
                Pixmap::shaded(into.width(), into.height(), |_, _| colour)
            }
            Fill::Current => {
                let colour = solid(current);
                Pixmap::shaded(into.width(), into.height(), |_, _| colour)
            }
            Fill::Url(id) => {
                let Some(gradient) = self.gradient(id, depth) else { return };
                let Some(layer) = gradient.painted(shape, place, into, opacity, self.viewport)
                else {
                    return;
                };
                layer
            }
        };
        layer.keep_inside(&shape.transformed(place), style.rule);
        into.composite(&layer, Mode::SourceOver);
    }

    /// A gradient by its id, with what it inherits from the gradients it
    /// names filled in.
    fn gradient(&self, id: &str, depth: u8) -> Option<Gradient> {
        let element = self.find(id)?;
        let linear = match element.local_name() {
            "linearGradient" => true,
            "radialGradient" => false,
            _ => return None,
        };
        let mut gradient = Gradient { linear, ..Gradient::default() };
        // The chain of gradients named, nearest first: an attribute or the
        // stops of the nearest that says them win.
        let mut chain = vec![element];
        let mut next = element;
        for _ in 0..DEEPEST.saturating_sub(depth) {
            let named = next
                .attribute_by_name("xlink:href")
                .or_else(|| next.attribute_by_name("href"))
                .and_then(|href| href.strip_prefix('#'))
                .and_then(|id| self.find(id));
            match named {
                Some(found) => {
                    chain.push(found);
                    next = found;
                }
                None => break,
            }
        }
        for element in chain.iter().rev() {
            gradient.read(element);
        }
        gradient.stops = chain
            .iter()
            .map(|element| stops_of(element))
            .find(|stops| !stops.is_empty())
            .unwrap_or_default();
        Some(gradient)
    }

    fn reach_of(
        &self,
        element: &Element,
        place: &Transform,
        style: &Style,
        out: &mut Option<(f32, f32, f32, f32)>,
        depth: u8,
    ) {
        if depth > DEEPEST || declared(element, "display") == Some("none") {
            return;
        }
        let place = own_transform(element).then(place);
        let style = styled(element, style);
        match element.local_name() {
            "g" | "svg" | "a" | "switch" => {
                for child in element.child_elements() {
                    self.reach_of(child, &place, &style, out, depth + 1);
                }
            }
            "use" => {
                let reference = element
                    .attribute_by_name("xlink:href")
                    .or_else(|| element.attribute_by_name("href"))
                    .and_then(|href| href.strip_prefix('#'));
                if let Some(used) = reference.and_then(|id| self.find(id)) {
                    let x = element.attribute_by_name("x").and_then(number).unwrap_or(0.0);
                    let y = element.attribute_by_name("y").and_then(number).unwrap_or(0.0);
                    let moved = Transform::translate(x, y).then(&place);
                    self.reach_of(used, &moved, &style, out, depth + 1);
                }
            }
            _ => {
                if style.fill == Fill::None {
                    return;
                }
                let Some(shape) = shape_of(element) else { return };
                if let Some((x0, y0, x1, y1)) = wp_raster::bounds_of(&shape.transformed(&place)) {
                    *out = Some(match *out {
                        Some((a, b, c, d)) => (a.min(x0), b.min(y0), c.max(x1), d.max(y1)),
                        None => (x0, y0, x1, y1),
                    });
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Gradients
// ---------------------------------------------------------------------------

/// A gradient with everything it inherits filled in. Coordinates are kept as
/// written, with whether each was a percentage, because what a percentage is
/// of depends on the units the gradient is in.
#[derive(Clone, Debug, Default)]
struct Gradient {
    linear: bool,
    values: HashMap<&'static str, (f32, bool)>,
    bounding_box: Option<bool>,
    transform: Option<Transform>,
    spread: Spread,
    stops: Vec<(f32, Premultiplied)>,
}

impl Gradient {
    /// Takes what one element of the chain says, over what was there.
    fn read(&mut self, element: &Element) {
        for name in ["x1", "y1", "x2", "y2", "cx", "cy", "r", "fx", "fy", "fr"] {
            if let Some(value) = element.attribute_by_name(name).and_then(length) {
                self.values.insert(name, value);
            }
        }
        if let Some(units) = element.attribute_by_name("gradientUnits") {
            self.bounding_box = Some(units.trim() != "userSpaceOnUse");
        }
        if let Some(text) = element.attribute_by_name("gradientTransform") {
            self.transform = Some(transform_list(text));
        }
        if let Some(method) = element.attribute_by_name("spreadMethod") {
            self.spread = match method.trim() {
                "reflect" => Spread::Reflect,
                "repeat" => Spread::Repeat,
                _ => Spread::Pad,
            };
        }
    }

    /// A value, as a fraction of the box where the gradient is in the box's
    /// units and in the document's own units otherwise.
    fn value(&self, name: &str, default: (f32, bool), in_box: bool, viewport: f32) -> f32 {
        let (value, percent) = self.values.get(name).copied().unwrap_or(default);
        match (percent, in_box) {
            (true, true) => value / 100.0,
            (true, false) => value / 100.0 * viewport,
            (false, _) => value,
        }
    }

    /// The gradient over the whole picture, for a shape drawn through a
    /// transform: every pixel is taken back into the gradient's own space.
    fn painted(
        &self,
        shape: &Path,
        place: &Transform,
        into: &Pixmap,
        opacity: f32,
        viewport: f32,
    ) -> Option<Pixmap> {
        if self.stops.is_empty() {
            return None;
        }
        let in_box = self.bounding_box.unwrap_or(true);
        // Where the gradient's own space is on the page: its transform, then
        // the shape's box where it is in the box's units, then the shape's
        // own place.
        let mut space = self.transform.unwrap_or(Transform::translate(0.0, 0.0));
        if in_box {
            let (x0, y0, x1, y1) = wp_raster::bounds_of(shape)?;
            let (width, height) = (x1 - x0, y1 - y0);
            if width <= 0.0 || height <= 0.0 {
                return None;
            }
            space = space.then(&Transform { a: width, b: 0.0, c: 0.0, d: height, e: x0, f: y0 });
        }
        let back = inverse(&space.then(place))?;
        let stops: Vec<(f32, Premultiplied)> = self
            .stops
            .iter()
            .map(|(offset, colour)| (*offset, colour.map(|channel| channel * opacity)))
            .collect();
        let spread = self.spread;
        let value = |name: &str, default: (f32, bool)| self.value(name, default, in_box, viewport);
        let along: Box<dyn Fn(Point) -> Option<f32>> = if self.linear {
            let from = Point::new(value("x1", (0.0, true)), value("y1", (0.0, true)));
            let to = Point::new(value("x2", (100.0, true)), value("y2", (0.0, true)));
            Box::new(move |at| compose::along_line(at, from, to))
        } else {
            let centre = Point::new(value("cx", (50.0, true)), value("cy", (50.0, true)));
            let radius = value("r", (50.0, true));
            let focus = Point::new(
                self.values.get("fx").map_or(centre.x, |_| value("fx", (50.0, true))),
                self.values.get("fy").map_or(centre.y, |_| value("fy", (50.0, true))),
            );
            let focal_radius = value("fr", (0.0, true));
            Box::new(move |at| {
                compose::between_circles(at, (focus, focal_radius), (centre, radius))
            })
        };
        Some(Pixmap::shaded(into.width(), into.height(), |x, y| {
            let at = back.apply(Point::new(x, y));
            along(at).map_or([0.0; 4], |t| compose::colour_along(&stops, t, spread))
        }))
    }
}

/// The stops of one gradient element, each offset no less than the one
/// before it, as the format requires of a reader.
fn stops_of(element: &Element) -> Vec<(f32, Premultiplied)> {
    let mut out: Vec<(f32, Premultiplied)> = Vec::new();
    let mut floor = 0.0f32;
    for stop in element.child_elements().filter(|child| child.local_name() == "stop") {
        let offset = stop
            .attribute_by_name("offset")
            .and_then(length)
            .map_or(0.0, |(value, percent)| if percent { value / 100.0 } else { value })
            .clamp(0.0, 1.0)
            .max(floor);
        floor = offset;
        let colour = declared(stop, "stop-color").and_then(colour).unwrap_or(Color {
            red: 0,
            green: 0,
            blue: 0,
            alpha: 255,
        });
        let opacity =
            declared(stop, "stop-opacity").and_then(number).unwrap_or(1.0).clamp(0.0, 1.0);
        let mut premultiplied =
            compose::premultiplied(colour.red, colour.green, colour.blue, colour.alpha);
        for channel in &mut premultiplied {
            *channel *= opacity;
        }
        out.push((offset, premultiplied));
    }
    out
}

// ---------------------------------------------------------------------------
// Attributes
// ---------------------------------------------------------------------------

/// What an element says about one property: its `style` attribute first,
/// then the attribute of that name.
fn declared<'e>(element: &'e Element, name: &str) -> Option<&'e str> {
    if let Some(style) = element.attribute_by_name("style") {
        for declaration in style.split(';') {
            if let Some((key, value)) = declaration.split_once(':') {
                if key.trim() == name {
                    return Some(value.trim());
                }
            }
        }
    }
    element.attribute_by_name(name).map(str::trim)
}

/// The style an element passes on to what is inside it.
fn styled(element: &Element, inherited: &Style) -> Style {
    let mut style = inherited.clone();
    if let Some(fill) = declared(element, "fill") {
        style.fill = match fill {
            "none" | "transparent" => Fill::None,
            "currentColor" | "currentcolor" => Fill::Current,
            "inherit" => inherited.fill.clone(),
            other => match url(other) {
                Some(id) => Fill::Url(id.to_owned()),
                None => colour(other).map_or(inherited.fill.clone(), Fill::Colour),
            },
        };
    }
    if let Some(opacity) = declared(element, "fill-opacity").and_then(number) {
        style.fill_opacity = opacity;
    }
    if let Some(rule) = declared(element, "fill-rule") {
        style.rule = if rule == "evenodd" { Rule::EvenOdd } else { Rule::Nonzero };
    }
    style
}

/// The id a `url(#id)` names.
fn url(text: &str) -> Option<&str> {
    let inner = text.trim().strip_prefix("url(")?.split(')').next()?;
    inner.trim().trim_matches(['"', '\'']).strip_prefix('#')
}

/// A number, with a unit of pixels allowed and a percentage read as a number.
fn number(text: &str) -> Option<f32> {
    let text = text.trim();
    let text = text.strip_suffix("px").unwrap_or(text);
    if let Some(percent) = text.strip_suffix('%') {
        return percent.trim().parse::<f32>().ok().map(|value| value / 100.0);
    }
    text.parse().ok()
}

/// A length and whether it was written as a percentage.
fn length(text: &str) -> Option<(f32, bool)> {
    let text = text.trim();
    match text.strip_suffix('%') {
        Some(percent) => percent.trim().parse().ok().map(|value| (value, true)),
        None => text.strip_suffix("px").unwrap_or(text).parse().ok().map(|value| (value, false)),
    }
}

/// An element's own transform, or none.
fn own_transform(element: &Element) -> Transform {
    element.attribute_by_name("transform").map_or(Transform::translate(0.0, 0.0), transform_list)
}

/// A `transform` attribute: functions applied right to left, as written.
fn transform_list(text: &str) -> Transform {
    let mut out = Transform::translate(0.0, 0.0);
    let mut rest = text;
    while let Some(open) = rest.find('(') {
        let name = rest[..open].trim().trim_start_matches(',').trim();
        let Some(close) = rest[open..].find(')') else { break };
        let arguments: Vec<f32> = rest[open + 1..open + close]
            .split(|character: char| character == ',' || character.is_ascii_whitespace())
            .filter(|part| !part.is_empty())
            .filter_map(|part| part.parse().ok())
            .collect();
        rest = &rest[open + close + 1..];
        let function = match (name, arguments.as_slice()) {
            ("matrix", [a, b, c, d, e, f]) => {
                Transform { a: *a, b: *b, c: *c, d: *d, e: *e, f: *f }
            }
            ("translate", [x]) => Transform::translate(*x, 0.0),
            ("translate", [x, y]) => Transform::translate(*x, *y),
            ("scale", [s]) => Transform::scale(*s, *s),
            ("scale", [x, y]) => Transform::scale(*x, *y),
            ("rotate", [angle]) => Transform::rotate(angle.to_radians()),
            ("rotate", [angle, x, y]) => Transform::rotate_about(angle.to_radians(), *x, *y),
            ("skewX", [angle]) => {
                Transform { a: 1.0, b: 0.0, c: angle.to_radians().tan(), d: 1.0, e: 0.0, f: 0.0 }
            }
            ("skewY", [angle]) => {
                Transform { a: 1.0, b: angle.to_radians().tan(), c: 0.0, d: 1.0, e: 0.0, f: 0.0 }
            }
            _ => continue,
        };
        // Written left to right, applied right to left: each function
        // written later is applied to the point first.
        out = function.then(&out);
    }
    out
}

/// The transform that undoes one, when one does.
fn inverse(transform: &Transform) -> Option<Transform> {
    let Transform { a, b, c, d, e, f } = *transform;
    let determinant = a * d - b * c;
    if determinant.abs() <= 1e-12 {
        return None;
    }
    let (ia, ib, ic, id) = (d / determinant, -b / determinant, -c / determinant, a / determinant);
    Some(Transform { a: ia, b: ib, c: ic, d: id, e: -(ia * e + ic * f), f: -(ib * e + id * f) })
}

// ---------------------------------------------------------------------------
// Shapes
// ---------------------------------------------------------------------------

/// The outline of a shape element, in its own space.
fn shape_of(element: &Element) -> Option<Path> {
    let get = |name: &str| element.attribute_by_name(name).and_then(number);
    match element.local_name() {
        "path" => crate::path::parse(element.attribute_by_name("d")?).ok(),
        "rect" => {
            let (x, y) = (get("x").unwrap_or(0.0), get("y").unwrap_or(0.0));
            let (width, height) = (get("width")?, get("height")?);
            if width <= 0.0 || height <= 0.0 {
                return None;
            }
            let rx = get("rx").or_else(|| get("ry")).unwrap_or(0.0).clamp(0.0, width / 2.0);
            let ry = get("ry").or_else(|| get("rx")).unwrap_or(0.0).clamp(0.0, height / 2.0);
            Some(rounded(x, y, width, height, rx, ry))
        }
        "circle" => {
            let r = get("r")?;
            (r > 0.0).then(|| ellipse(get("cx").unwrap_or(0.0), get("cy").unwrap_or(0.0), r, r))
        }
        "ellipse" => {
            let (rx, ry) = (get("rx")?, get("ry")?);
            (rx > 0.0 && ry > 0.0)
                .then(|| ellipse(get("cx").unwrap_or(0.0), get("cy").unwrap_or(0.0), rx, ry))
        }
        "polygon" | "polyline" => {
            let numbers: Vec<f32> = element
                .attribute_by_name("points")?
                .split(|character: char| character == ',' || character.is_ascii_whitespace())
                .filter(|part| !part.is_empty())
                .filter_map(|part| part.parse().ok())
                .collect();
            let mut path = Path::new();
            for (at, pair) in numbers.chunks_exact(2).enumerate() {
                let point = Point::new(pair[0], pair[1]);
                if at == 0 {
                    path.move_to(point);
                } else {
                    path.line_to(point);
                }
            }
            // A polyline is filled as though it were closed, as SVG says.
            path.close();
            (path.commands.len() > 2).then_some(path)
        }
        _ => None,
    }
}

/// How far along a quarter circle's tangent its cubic's controls sit.
const KAPPA: f32 = 0.552_284_8;

fn ellipse(cx: f32, cy: f32, rx: f32, ry: f32) -> Path {
    let (kx, ky) = (rx * KAPPA, ry * KAPPA);
    let mut path = Path::new();
    path.move_to(Point::new(cx + rx, cy));
    path.cubic_to(
        Point::new(cx + rx, cy + ky),
        Point::new(cx + kx, cy + ry),
        Point::new(cx, cy + ry),
    );
    path.cubic_to(
        Point::new(cx - kx, cy + ry),
        Point::new(cx - rx, cy + ky),
        Point::new(cx - rx, cy),
    );
    path.cubic_to(
        Point::new(cx - rx, cy - ky),
        Point::new(cx - kx, cy - ry),
        Point::new(cx, cy - ry),
    );
    path.cubic_to(
        Point::new(cx + kx, cy - ry),
        Point::new(cx + rx, cy - ky),
        Point::new(cx + rx, cy),
    );
    path.close();
    path
}

fn rounded(x: f32, y: f32, width: f32, height: f32, rx: f32, ry: f32) -> Path {
    if rx <= 0.0 || ry <= 0.0 {
        return Path::rectangle(x, y, width, height);
    }
    let (kx, ky) = (rx * KAPPA, ry * KAPPA);
    let (right, bottom) = (x + width, y + height);
    let mut path = Path::new();
    path.move_to(Point::new(x + rx, y));
    path.line_to(Point::new(right - rx, y));
    path.cubic_to(
        Point::new(right - rx + kx, y),
        Point::new(right, y + ry - ky),
        Point::new(right, y + ry),
    );
    path.line_to(Point::new(right, bottom - ry));
    path.cubic_to(
        Point::new(right, bottom - ry + ky),
        Point::new(right - rx + kx, bottom),
        Point::new(right - rx, bottom),
    );
    path.line_to(Point::new(x + rx, bottom));
    path.cubic_to(
        Point::new(x + rx - kx, bottom),
        Point::new(x, bottom - ry + ky),
        Point::new(x, bottom - ry),
    );
    path.line_to(Point::new(x, y + ry));
    path.cubic_to(Point::new(x, y + ry - ky), Point::new(x + rx - kx, y), Point::new(x + rx, y));
    path.close();
    path
}

// ---------------------------------------------------------------------------
// Colours
// ---------------------------------------------------------------------------

/// A colour as CSS writes one: `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`,
/// `rgb()`, `rgba()`, or one of the names.
fn colour(text: &str) -> Option<Color> {
    let text = text.trim();
    if let Some(hex) = text.strip_prefix('#') {
        let digits: Vec<u8> = hex
            .chars()
            .map(|character| character.to_digit(16).map(|value| value as u8))
            .collect::<Option<Vec<u8>>>()?;
        let (red, green, blue, alpha) = match digits.as_slice() {
            [r, g, b] => (r * 17, g * 17, b * 17, 255),
            [r, g, b, a] => (r * 17, g * 17, b * 17, a * 17),
            [r1, r2, g1, g2, b1, b2] => (r1 * 16 + r2, g1 * 16 + g2, b1 * 16 + b2, 255),
            [r1, r2, g1, g2, b1, b2, a1, a2] => {
                (r1 * 16 + r2, g1 * 16 + g2, b1 * 16 + b2, a1 * 16 + a2)
            }
            _ => return None,
        };
        return Some(Color { red, green, blue, alpha });
    }
    let lower = text.to_ascii_lowercase();
    if let Some(inner) = lower.strip_prefix("rgba(").or_else(|| lower.strip_prefix("rgb(")) {
        let parts: Vec<&str> = inner
            .trim_end_matches(')')
            .split(|character: char| {
                character == ',' || character == '/' || character.is_ascii_whitespace()
            })
            .filter(|part| !part.is_empty())
            .collect();
        let channel = |part: &str| -> Option<u8> {
            let value = match part.strip_suffix('%') {
                Some(percent) => percent.parse::<f32>().ok()? * 2.55,
                None => part.parse::<f32>().ok()?,
            };
            Some(value.round().clamp(0.0, 255.0) as u8)
        };
        let alpha = match parts.get(3) {
            Some(part) => {
                let value = match part.strip_suffix('%') {
                    Some(percent) => percent.parse::<f32>().ok()? / 100.0,
                    None => part.parse::<f32>().ok()?,
                };
                (value.clamp(0.0, 1.0) * 255.0).round() as u8
            }
            None => 255,
        };
        return Some(Color {
            red: channel(parts.first()?)?,
            green: channel(parts.get(1)?)?,
            blue: channel(parts.get(2)?)?,
            alpha,
        });
    }
    let hex = NAMED.iter().find(|(name, _)| *name == lower)?.1;
    Some(Color { red: (hex >> 16) as u8, green: (hex >> 8) as u8, blue: hex as u8, alpha: 255 })
}

/// The colours CSS names.
const NAMED: &[(&str, u32)] = &[
    ("aliceblue", 0xF0F8FF),
    ("antiquewhite", 0xFAEBD7),
    ("aqua", 0x00FFFF),
    ("aquamarine", 0x7FFFD4),
    ("azure", 0xF0FFFF),
    ("beige", 0xF5F5DC),
    ("bisque", 0xFFE4C4),
    ("black", 0x000000),
    ("blanchedalmond", 0xFFEBCD),
    ("blue", 0x0000FF),
    ("blueviolet", 0x8A2BE2),
    ("brown", 0xA52A2A),
    ("burlywood", 0xDEB887),
    ("cadetblue", 0x5F9EA0),
    ("chartreuse", 0x7FFF00),
    ("chocolate", 0xD2691E),
    ("coral", 0xFF7F50),
    ("cornflowerblue", 0x6495ED),
    ("cornsilk", 0xFFF8DC),
    ("crimson", 0xDC143C),
    ("cyan", 0x00FFFF),
    ("darkblue", 0x00008B),
    ("darkcyan", 0x008B8B),
    ("darkgoldenrod", 0xB8860B),
    ("darkgray", 0xA9A9A9),
    ("darkgreen", 0x006400),
    ("darkgrey", 0xA9A9A9),
    ("darkkhaki", 0xBDB76B),
    ("darkmagenta", 0x8B008B),
    ("darkolivegreen", 0x556B2F),
    ("darkorange", 0xFF8C00),
    ("darkorchid", 0x9932CC),
    ("darkred", 0x8B0000),
    ("darksalmon", 0xE9967A),
    ("darkseagreen", 0x8FBC8F),
    ("darkslateblue", 0x483D8B),
    ("darkslategray", 0x2F4F4F),
    ("darkslategrey", 0x2F4F4F),
    ("darkturquoise", 0x00CED1),
    ("darkviolet", 0x9400D3),
    ("deeppink", 0xFF1493),
    ("deepskyblue", 0x00BFFF),
    ("dimgray", 0x696969),
    ("dimgrey", 0x696969),
    ("dodgerblue", 0x1E90FF),
    ("firebrick", 0xB22222),
    ("floralwhite", 0xFFFAF0),
    ("forestgreen", 0x228B22),
    ("fuchsia", 0xFF00FF),
    ("gainsboro", 0xDCDCDC),
    ("ghostwhite", 0xF8F8FF),
    ("gold", 0xFFD700),
    ("goldenrod", 0xDAA520),
    ("gray", 0x808080),
    ("green", 0x008000),
    ("greenyellow", 0xADFF2F),
    ("grey", 0x808080),
    ("honeydew", 0xF0FFF0),
    ("hotpink", 0xFF69B4),
    ("indianred", 0xCD5C5C),
    ("indigo", 0x4B0082),
    ("ivory", 0xFFFFF0),
    ("khaki", 0xF0E68C),
    ("lavender", 0xE6E6FA),
    ("lavenderblush", 0xFFF0F5),
    ("lawngreen", 0x7CFC00),
    ("lemonchiffon", 0xFFFACD),
    ("lightblue", 0xADD8E6),
    ("lightcoral", 0xF08080),
    ("lightcyan", 0xE0FFFF),
    ("lightgoldenrodyellow", 0xFAFAD2),
    ("lightgray", 0xD3D3D3),
    ("lightgreen", 0x90EE90),
    ("lightgrey", 0xD3D3D3),
    ("lightpink", 0xFFB6C1),
    ("lightsalmon", 0xFFA07A),
    ("lightseagreen", 0x20B2AA),
    ("lightskyblue", 0x87CEFA),
    ("lightslategray", 0x778899),
    ("lightslategrey", 0x778899),
    ("lightsteelblue", 0xB0C4DE),
    ("lightyellow", 0xFFFFE0),
    ("lime", 0x00FF00),
    ("limegreen", 0x32CD32),
    ("linen", 0xFAF0E6),
    ("magenta", 0xFF00FF),
    ("maroon", 0x800000),
    ("mediumaquamarine", 0x66CDAA),
    ("mediumblue", 0x0000CD),
    ("mediumorchid", 0xBA55D3),
    ("mediumpurple", 0x9370DB),
    ("mediumseagreen", 0x3CB371),
    ("mediumslateblue", 0x7B68EE),
    ("mediumspringgreen", 0x00FA9A),
    ("mediumturquoise", 0x48D1CC),
    ("mediumvioletred", 0xC71585),
    ("midnightblue", 0x191970),
    ("mintcream", 0xF5FFFA),
    ("mistyrose", 0xFFE4E1),
    ("moccasin", 0xFFE4B5),
    ("navajowhite", 0xFFDEAD),
    ("navy", 0x000080),
    ("oldlace", 0xFDF5E6),
    ("olive", 0x808000),
    ("olivedrab", 0x6B8E23),
    ("orange", 0xFFA500),
    ("orangered", 0xFF4500),
    ("orchid", 0xDA70D6),
    ("palegoldenrod", 0xEEE8AA),
    ("palegreen", 0x98FB98),
    ("paleturquoise", 0xAFEEEE),
    ("palevioletred", 0xDB7093),
    ("papayawhip", 0xFFEFD5),
    ("peachpuff", 0xFFDAB9),
    ("peru", 0xCD853F),
    ("pink", 0xFFC0CB),
    ("plum", 0xDDA0DD),
    ("powderblue", 0xB0E0E6),
    ("purple", 0x800080),
    ("rebeccapurple", 0x663399),
    ("red", 0xFF0000),
    ("rosybrown", 0xBC8F8F),
    ("royalblue", 0x4169E1),
    ("saddlebrown", 0x8B4513),
    ("salmon", 0xFA8072),
    ("sandybrown", 0xF4A460),
    ("seagreen", 0x2E8B57),
    ("seashell", 0xFFF5EE),
    ("sienna", 0xA0522D),
    ("silver", 0xC0C0C0),
    ("skyblue", 0x87CEEB),
    ("slateblue", 0x6A5ACD),
    ("slategray", 0x708090),
    ("slategrey", 0x708090),
    ("snow", 0xFFFAFA),
    ("springgreen", 0x00FF7F),
    ("steelblue", 0x4682B4),
    ("tan", 0xD2B48C),
    ("teal", 0x008080),
    ("thistle", 0xD8BFD8),
    ("tomato", 0xFF6347),
    ("turquoise", 0x40E0D0),
    ("violet", 0xEE82EE),
    ("wheat", 0xF5DEB3),
    ("white", 0xFFFFFF),
    ("whitesmoke", 0xF5F5F5),
    ("yellow", 0xFFFF00),
    ("yellowgreen", 0x9ACD32),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(picture: &Pixmap, x: usize, y: usize) -> [u8; 4] {
        let bytes = picture.to_rgba();
        let at = (y * picture.width() + x) * 4;
        [bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]
    }

    fn drawn(svg: &str, id: &str) -> Pixmap {
        let document = Document::parse(svg).expect("a document");
        let mut picture = Pixmap::new(100, 100);
        assert!(document.draw(
            id,
            &mut picture,
            &Transform::translate(0.0, 0.0),
            Color { red: 0, green: 0, blue: 255, alpha: 255 }
        ));
        picture
    }

    #[test]
    fn colours_are_read_every_way_css_writes_them() {
        assert_eq!(colour("#f00"), Some(Color { red: 255, green: 0, blue: 0, alpha: 255 }));
        assert_eq!(colour("#00800080"), Some(Color { red: 0, green: 128, blue: 0, alpha: 128 }));
        assert_eq!(
            colour("rgb(0, 128, 255)"),
            Some(Color { red: 0, green: 128, blue: 255, alpha: 255 })
        );
        assert_eq!(colour("rgba(255,0,0,0.5)").map(|c| c.alpha), Some(128));
        assert_eq!(
            colour("rebeccapurple"),
            Some(Color { red: 0x66, green: 0x33, blue: 0x99, alpha: 255 })
        );
        assert_eq!(colour("nonsense"), None);
    }

    #[test]
    fn transforms_apply_right_to_left() {
        // Scaled by two, then moved by ten: (1, 0) lands at (12, 0).
        let transform = transform_list("translate(10) scale(2)");
        let at = transform.apply(Point::new(1.0, 0.0));
        assert!((at.x - 12.0).abs() < 1e-4 && at.y.abs() < 1e-4);
    }

    #[test]
    fn a_group_s_transform_and_fill_reach_the_shapes_inside() {
        let picture = drawn(
            r#"<svg><g id="g" transform="translate(50 0)" fill="red"><rect width="10" height="10"/></g></svg>"#,
            "g",
        );
        assert_eq!(pixel(&picture, 55, 5), [255, 0, 0, 255]);
        assert_eq!(pixel(&picture, 5, 5)[3], 0);
    }

    #[test]
    fn the_colour_of_the_text_is_taken_where_it_is_asked_for() {
        let picture =
            drawn(r#"<svg><circle id="c" cx="50" cy="50" r="20" fill="currentColor"/></svg>"#, "c");
        assert_eq!(pixel(&picture, 50, 50), [0, 0, 255, 255]);
    }

    #[test]
    fn a_gradient_in_the_shape_s_box_runs_across_it() {
        let picture = drawn(
            r#"<svg><linearGradient id="l"><stop offset="0" stop-color="red"/><stop offset="100%" stop-color="blue"/></linearGradient>
               <rect id="r" x="0" y="0" width="100" height="10" fill="url(#l)"/></svg>"#,
            "r",
        );
        assert_eq!(pixel(&picture, 0, 5)[0], 254);
        assert!(pixel(&picture, 99, 5)[2] > 250);
        // Mixed in the ordinary scale, as SVG does by default.
        let middle = pixel(&picture, 50, 5);
        assert!(middle[0].abs_diff(126) <= 2 && middle[2].abs_diff(129) <= 2, "{middle:?}");
    }

    #[test]
    fn a_gradient_inherits_stops_from_the_one_it_names() {
        let picture = drawn(
            r##"<svg><linearGradient id="base"><stop offset="0" stop-color="#0f0"/><stop offset="1" stop-color="#0f0"/></linearGradient>
               <radialGradient id="round" xlink:href="#base" xmlns:xlink="http://www.w3.org/1999/xlink"/>
               <rect id="r" width="100" height="100" fill="url(#round)"/></svg>"##,
            "r",
        );
        assert_eq!(pixel(&picture, 50, 50), [0, 255, 0, 255]);
    }

    #[test]
    fn use_draws_what_it_names_moved_and_faded() {
        let picture = drawn(
            r##"<svg xmlns:xlink="http://www.w3.org/1999/xlink"><defs><rect id="d" width="10" height="10"/></defs>
               <use id="u" xlink:href="#d" x="20" y="20" fill="black" opacity="0.5"/></svg>"##,
            "u",
        );
        let at = pixel(&picture, 25, 25);
        assert!(at[3].abs_diff(128) <= 1, "{at:?}");
        assert_eq!(pixel(&picture, 5, 5)[3], 0, "the definition is not drawn where it stands");
    }

    #[test]
    fn a_clip_keeps_only_what_is_inside_it() {
        let picture = drawn(
            r#"<svg><clipPath id="k"><rect width="50" height="100"/></clipPath>
               <rect id="r" width="100" height="100" fill="red" clip-path="url(#k)"/></svg>"#,
            "r",
        );
        assert_eq!(pixel(&picture, 25, 50), [255, 0, 0, 255]);
        assert_eq!(pixel(&picture, 75, 50)[3], 0);
    }

    #[test]
    fn a_shape_filled_with_nothing_reaches_nowhere() {
        let document = Document::parse(
            r#"<svg><g id="g"><rect width="5" height="5" fill="none"/><rect x="10" y="10" width="5" height="5"/></g></svg>"#,
        )
        .unwrap();
        assert_eq!(
            document.reach("g", &Transform::translate(0.0, 0.0)),
            Some((10.0, 10.0, 15.0, 15.0))
        );
    }
}
