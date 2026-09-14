//! Running a page's content stream for where its text and pictures land.
//!
//! The stream is a stack language: operands, then an operator. The
//! operators that matter here move the pen and set the font, show text,
//! draw pictures, and — for the lines under words and round table cells —
//! fill and stroke rectangles. Everything else is stepped over. Each glyph
//! comes out with where its baseline starts on the page, in points, with
//! the page's bottom-left corner as the origin, the way the format has it.

use std::collections::HashMap;
use std::rc::Rc;

use super::file::{File, PageInfo};
use super::font::LoadedFont;
use super::object::{Dictionary, Lexer, Object};

/// A transformation: `[a b c d e f]`, mapping (x, y) to
/// (a·x + c·y + e, b·x + d·y + f).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Matrix(pub [f64; 6]);

impl Matrix {
    pub const IDENTITY: Self = Self([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    /// `self` then `other`.
    #[must_use]
    pub fn then(self, other: Self) -> Self {
        let [a, b, c, d, e, f] = self.0;
        let [a2, b2, c2, d2, e2, f2] = other.0;
        Self([
            a * a2 + b * c2,
            a * b2 + b * d2,
            c * a2 + d * c2,
            c * b2 + d * d2,
            e * a2 + f * c2 + e2,
            e * b2 + f * d2 + f2,
        ])
    }

    #[must_use]
    pub fn apply(self, x: f64, y: f64) -> (f64, f64) {
        let [a, b, c, d, e, f] = self.0;
        (a * x + c * y + e, b * x + d * y + f)
    }

    #[must_use]
    pub fn translate(x: f64, y: f64) -> Self {
        Self([1.0, 0.0, 0.0, 1.0, x, y])
    }

    /// How much a length along the x axis is scaled.
    #[must_use]
    pub fn x_scale(self) -> f64 {
        (self.0[0] * self.0[0] + self.0[1] * self.0[1]).sqrt()
    }

    /// How much a length along the y axis is scaled.
    #[must_use]
    pub fn y_scale(self) -> f64 {
        (self.0[2] * self.0[2] + self.0[3] * self.0[3]).sqrt()
    }
}

/// One glyph on the page.
#[derive(Clone, Debug)]
pub struct Glyph {
    /// Where the baseline starts, in points from the bottom-left.
    pub x: f64,
    pub y: f64,
    /// Where the next glyph would start.
    pub end_x: f64,
    /// The font size as it comes out on the page.
    pub size: f64,
    pub text: String,
    pub font: Rc<LoadedFont>,
    /// Red, green, blue, each 0 to 1.
    pub colour: [f32; 3],
    /// The rise above the baseline, in points on the page.
    pub rise: f64,
    /// The glyph was drawn slanted by the text matrix: an italic made
    /// from an upright font, which is how a program without the italic
    /// face draws one.
    pub slanted: bool,
}

/// A picture drawn on the page.
#[derive(Clone, Debug)]
pub struct PlacedPicture {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
    pub bytes: Vec<u8>,
    pub extension: &'static str,
}

/// A filled or stroked rectangle: a rule under text, a table's border,
/// or a cell's shading.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rectangle {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
    pub filled: bool,
}

impl Rectangle {
    #[must_use]
    pub fn width(&self) -> f64 {
        self.x1 - self.x0
    }

    #[must_use]
    pub fn height(&self) -> f64 {
        self.y1 - self.y0
    }
}

/// What a page's content amounts to.
#[derive(Debug, Default)]
pub struct Drawn {
    pub glyphs: Vec<Glyph>,
    pub pictures: Vec<PlacedPicture>,
    pub rectangles: Vec<Rectangle>,
}

#[derive(Clone)]
struct State {
    ctm: Matrix,
    font: Option<Rc<LoadedFont>>,
    size: f64,
    char_spacing: f64,
    word_spacing: f64,
    horizontal_scale: f64,
    leading: f64,
    rise: f64,
    fill: [f32; 3],
    fill_components: usize,
    line_width: f64,
}

impl Default for State {
    fn default() -> Self {
        Self {
            ctm: Matrix::IDENTITY,
            font: None,
            size: 0.0,
            char_spacing: 0.0,
            word_spacing: 0.0,
            horizontal_scale: 1.0,
            leading: 0.0,
            rise: 0.0,
            fill: [0.0; 3],
            fill_components: 1,
            line_width: 1.0,
        }
    }
}

/// The interpreter for one page.
pub struct Interpreter<'f, 'a> {
    file: &'f File<'a>,
    fonts: HashMap<String, Rc<LoadedFont>>,
    state: State,
    stack: Vec<State>,
    text_matrix: Matrix,
    line_matrix: Matrix,
    /// The path being built: subpaths of points, in page space.
    path: Vec<Vec<(f64, f64)>>,
    depth: usize,
    pub drawn: Drawn,
}

impl<'f, 'a> Interpreter<'f, 'a> {
    /// Runs a page's content, with the page turned the way it is shown.
    #[must_use]
    pub fn run_page(file: &'f File<'a>, page: &PageInfo) -> Drawn {
        let [left, bottom, right, top] = page.media_box;
        let (width, height) = (right - left, top - bottom);
        // Moved so the page's own corner is the origin, then turned.
        let base = Matrix::translate(-left, -bottom).then(match page.rotate {
            90 => Matrix([0.0, -1.0, 1.0, 0.0, 0.0, width]),
            180 => Matrix([-1.0, 0.0, 0.0, -1.0, width, height]),
            270 => Matrix([0.0, 1.0, -1.0, 0.0, height, 0.0]),
            _ => Matrix::IDENTITY,
        });
        let mut interpreter = Self {
            file,
            fonts: HashMap::new(),
            state: State { ctm: base, ..State::default() },
            stack: Vec::new(),
            text_matrix: Matrix::IDENTITY,
            line_matrix: Matrix::IDENTITY,
            path: Vec::new(),
            depth: 0,
            drawn: Drawn::default(),
        };
        let content = file.content_of(&page.dictionary);
        interpreter.run(&content, &page.resources);
        interpreter.drawn
    }

    fn run(&mut self, content: &[u8], resources: &Dictionary) {
        let mut lexer = Lexer::new(content);
        let mut operands: Vec<Object> = Vec::new();
        while let Some(object) = lexer.next_object() {
            let Object::Operator(operator) = object else {
                operands.push(object);
                if operands.len() > 64 {
                    operands.remove(0);
                }
                continue;
            };
            match operator.as_str() {
                "BI" => {
                    skip_inline_image(&mut lexer);
                }
                _ => self.operate(&operator, &operands, resources),
            }
            operands.clear();
        }
    }

    fn number(operands: &[Object], index: usize) -> f64 {
        operands.get(index).and_then(Object::as_number).unwrap_or(0.0)
    }

    fn operate(&mut self, operator: &str, operands: &[Object], resources: &Dictionary) {
        let n = |index: usize| Self::number(operands, index);
        match operator {
            "q" => {
                self.stack.push(self.state.clone());
                if self.stack.len() > 256 {
                    self.stack.remove(0);
                }
            }
            "Q" => {
                if let Some(state) = self.stack.pop() {
                    self.state = state;
                }
            }
            "cm" if operands.len() >= 6 => {
                let matrix = Matrix([n(0), n(1), n(2), n(3), n(4), n(5)]);
                self.state.ctm = matrix.then(self.state.ctm);
            }
            "w" => self.state.line_width = n(0),
            "BT" => {
                self.text_matrix = Matrix::IDENTITY;
                self.line_matrix = Matrix::IDENTITY;
            }
            "ET" => {}
            "Tc" => self.state.char_spacing = n(0),
            "Tw" => self.state.word_spacing = n(0),
            "Tz" => self.state.horizontal_scale = n(0) / 100.0,
            "TL" => self.state.leading = n(0),
            "Ts" => self.state.rise = n(0),
            "Tf" => {
                self.state.size = n(1);
                if let Some(Object::Name(name)) = operands.first() {
                    self.state.font = self.font(name, resources);
                }
            }
            "Td" => {
                self.line_matrix = Matrix::translate(n(0), n(1)).then(self.line_matrix);
                self.text_matrix = self.line_matrix;
            }
            "TD" => {
                self.state.leading = -n(1);
                self.line_matrix = Matrix::translate(n(0), n(1)).then(self.line_matrix);
                self.text_matrix = self.line_matrix;
            }
            "Tm" if operands.len() >= 6 => {
                self.line_matrix = Matrix([n(0), n(1), n(2), n(3), n(4), n(5)]);
                self.text_matrix = self.line_matrix;
            }
            "T*" => self.next_line(),
            "Tj" => {
                if let Some(Object::String(bytes)) = operands.last() {
                    self.show(bytes);
                }
            }
            "'" => {
                self.next_line();
                if let Some(Object::String(bytes)) = operands.last() {
                    self.show(bytes);
                }
            }
            "\"" => {
                self.state.word_spacing = n(0);
                self.state.char_spacing = n(1);
                self.next_line();
                if let Some(Object::String(bytes)) = operands.last() {
                    self.show(bytes);
                }
            }
            "TJ" => {
                if let Some(Object::Array(items)) = operands.last() {
                    for item in items {
                        match item {
                            Object::String(bytes) => self.show(bytes),
                            Object::Number(adjust) => {
                                let tx = -adjust / 1000.0
                                    * self.state.size
                                    * self.state.horizontal_scale;
                                self.text_matrix =
                                    Matrix::translate(tx, 0.0).then(self.text_matrix);
                            }
                            _ => {}
                        }
                    }
                }
            }
            // Colour, for the text's colour: grey, RGB, CMYK, and named
            // spaces by their component count.
            "g" => self.state.fill = [n(0) as f32; 3],
            "rg" => self.state.fill = [n(0) as f32, n(1) as f32, n(2) as f32],
            "k" => self.state.fill = cmyk(n(0), n(1), n(2), n(3)),
            "cs" => {
                self.state.fill_components = match operands.first().and_then(Object::as_name) {
                    Some("DeviceRGB" | "CalRGB" | "Lab") => 3,
                    Some("DeviceCMYK") => 4,
                    Some("DeviceGray" | "CalGray") => 1,
                    Some(name) => self.colour_space_components(name, resources),
                    None => 1,
                };
                self.state.fill = [0.0; 3];
            }
            "sc" | "scn" => {
                let numbers: Vec<f64> = operands.iter().filter_map(Object::as_number).collect();
                match (self.state.fill_components, numbers.len()) {
                    (_, 0) => {}
                    (4, 4) => {
                        self.state.fill = cmyk(numbers[0], numbers[1], numbers[2], numbers[3])
                    }
                    (3, 3) => {
                        self.state.fill = [numbers[0] as f32, numbers[1] as f32, numbers[2] as f32]
                    }
                    (1, 1) => self.state.fill = [numbers[0] as f32; 3],
                    // A separation's tint: full is the ink, none is paper.
                    (0, 1) => self.state.fill = [1.0 - numbers[0] as f32; 3],
                    _ => {}
                }
            }
            // Paths, for the rectangles among them.
            "m" => self.path.push(vec![self.state.ctm.apply(n(0), n(1))]),
            "l" => {
                let point = self.state.ctm.apply(n(0), n(1));
                match self.path.last_mut() {
                    Some(subpath) => subpath.push(point),
                    None => self.path.push(vec![point]),
                }
            }
            "c" | "v" | "y" => {
                let (x, y) = match operator {
                    "c" => (n(4), n(5)),
                    "v" => (n(2), n(3)),
                    _ => (n(2), n(3)),
                };
                let point = self.state.ctm.apply(x, y);
                // A curve is not a rule; its end point keeps the path
                // going but marks the subpath as curved.
                match self.path.last_mut() {
                    Some(subpath) => {
                        subpath.push((f64::NAN, f64::NAN));
                        subpath.push(point);
                    }
                    None => self.path.push(vec![point]),
                }
            }
            "re" => {
                let (x, y, w, h) = (n(0), n(1), n(2), n(3));
                let corners = [(x, y), (x + w, y), (x + w, y + h), (x, y + h), (x, y)];
                self.path.push(corners.iter().map(|&(x, y)| self.state.ctm.apply(x, y)).collect());
            }
            "h" => {
                if let Some(subpath) = self.path.last_mut() {
                    if let Some(&first) = subpath.first() {
                        subpath.push(first);
                    }
                }
            }
            "f" | "F" | "f*" | "b" | "b*" | "B" | "B*" => {
                let stroked = operator.starts_with('b') || operator.starts_with('B');
                self.paint(true, stroked);
            }
            "S" | "s" => self.paint(false, true),
            "n" | "W" | "W*" => {
                if operator == "n" {
                    self.path.clear();
                }
            }
            "Do" => {
                if let Some(Object::Name(name)) = operands.first() {
                    self.draw_xobject(name, resources);
                }
            }
            "d0" | "d1" => {}
            _ => {}
        }
    }

    fn next_line(&mut self) {
        self.line_matrix = Matrix::translate(0.0, -self.state.leading).then(self.line_matrix);
        self.text_matrix = self.line_matrix;
    }

    /// Shows a string: one glyph per character the font finds in it, each
    /// advancing the text matrix.
    fn show(&mut self, bytes: &[u8]) {
        let Some(font) = self.state.font.clone() else { return };
        let size = self.state.size;
        let vertical = font.is_vertical();
        for shown in font.decode(bytes) {
            let placement =
                Matrix([size * self.state.horizontal_scale, 0.0, 0.0, size, 0.0, self.state.rise])
                    .then(self.text_matrix)
                    .then(self.state.ctm);
            let (x, y) = placement.apply(0.0, 0.0);
            let mut advance = shown.width * size + self.state.char_spacing;
            if shown.is_space_byte {
                advance += self.state.word_spacing;
            }
            let (tx, ty) = if vertical {
                (0.0, -(shown.width.max(0.5) * size + self.state.char_spacing))
            } else {
                (advance * self.state.horizontal_scale, 0.0)
            };
            let after = Matrix::translate(tx, ty).then(self.text_matrix).then(self.state.ctm);
            let (end_x, _) = after.apply(0.0, 0.0);
            let on_page = placement.y_scale();
            // A slant: the y axis leaning to the right by more than a few
            // degrees relative to the x axis.
            let [a, b, c, d, ..] = placement.0;
            let slanted = (a * c + b * d).abs() > 0.15 * (a * a + b * b).sqrt() * on_page;
            if !shown.text.is_empty() && on_page > 0.0 {
                self.drawn.glyphs.push(Glyph {
                    x,
                    y: y - self.state.rise * self.text_matrix.then(self.state.ctm).y_scale(),
                    end_x,
                    size: on_page,
                    text: shown.text,
                    font: Rc::clone(&font),
                    colour: self.state.fill,
                    rise: self.state.rise * self.text_matrix.then(self.state.ctm).y_scale(),
                    slanted,
                });
            }
            self.text_matrix = Matrix::translate(tx, ty).then(self.text_matrix);
        }
    }

    /// A font resource, loaded once.
    fn font(&mut self, name: &str, resources: &Dictionary) -> Option<Rc<LoadedFont>> {
        let key = format!("{name}@{:p}", resources);
        if let Some(font) = self.fonts.get(&key) {
            return Some(Rc::clone(font));
        }
        let fonts = self.file.get(resources, "Font");
        let dictionary = fonts.as_dictionary()?;
        let reference = dictionary.get(name)?;
        // The same font object under any name is the same font.
        let identity = match reference {
            Object::Reference(number, _) => format!("#{number}"),
            _ => key.clone(),
        };
        if let Some(font) = self.fonts.get(&identity) {
            let font = Rc::clone(font);
            self.fonts.insert(key, Rc::clone(&font));
            return Some(font);
        }
        let font = self.file.resolve(reference);
        let font = Rc::new(LoadedFont::load(self.file, font.as_dictionary()?));
        self.fonts.insert(identity, Rc::clone(&font));
        self.fonts.insert(key, Rc::clone(&font));
        Some(font)
    }

    /// How many components a named colour space takes; nothing known
    /// gives a separation's one tint.
    fn colour_space_components(&self, name: &str, resources: &Dictionary) -> usize {
        let spaces = self.file.get(resources, "ColorSpace");
        let Some(spaces) = spaces.as_dictionary() else { return 1 };
        let space = self.file.get(spaces, name);
        components_of(self.file, &space, 0)
    }

    /// Records the rectangles a painted path holds, then drops it.
    fn paint(&mut self, filled: bool, stroked: bool) {
        let path = std::mem::take(&mut self.path);
        for subpath in path {
            if subpath.iter().any(|(x, _)| x.is_nan()) {
                continue;
            }
            let Some(rectangle) = rectangle_of(&subpath) else {
                // A straight line stroked is a rule too.
                if stroked && subpath.len() == 2 {
                    let (x0, y0) = subpath[0];
                    let (x1, y1) = subpath[1];
                    let half = (self.state.line_width * self.state.ctm.x_scale() / 2.0).max(0.25);
                    if (y0 - y1).abs() < 0.01 || (x0 - x1).abs() < 0.01 {
                        self.drawn.rectangles.push(Rectangle {
                            x0: x0.min(x1) - if (y0 - y1).abs() < 0.01 { 0.0 } else { half },
                            y0: y0.min(y1) - if (x0 - x1).abs() < 0.01 { 0.0 } else { half },
                            x1: x0.max(x1) + if (y0 - y1).abs() < 0.01 { 0.0 } else { half },
                            y1: y0.max(y1) + if (x0 - x1).abs() < 0.01 { 0.0 } else { half },
                            filled: false,
                        });
                    }
                }
                continue;
            };
            if filled {
                self.drawn.rectangles.push(Rectangle { filled: true, ..rectangle });
            }
            if stroked {
                // The four sides, as rules.
                let half = (self.state.line_width * self.state.ctm.x_scale() / 2.0).max(0.25);
                let Rectangle { x0, y0, x1, y1, .. } = rectangle;
                for (a, b, c, d) in [
                    (x0 - half, y0 - half, x1 + half, y0 + half),
                    (x0 - half, y1 - half, x1 + half, y1 + half),
                    (x0 - half, y0 - half, x0 + half, y1 + half),
                    (x1 - half, y0 - half, x1 + half, y1 + half),
                ] {
                    self.drawn.rectangles.push(Rectangle {
                        x0: a,
                        y0: b,
                        x1: c,
                        y1: d,
                        filled: false,
                    });
                }
            }
        }
    }

    /// Draws a form by running it, or places a picture.
    fn draw_xobject(&mut self, name: &str, resources: &Dictionary) {
        let xobjects = self.file.get(resources, "XObject");
        let Some(xobjects) = xobjects.as_dictionary() else { return };
        let object = self.file.get(xobjects, name);
        let Some(stream) = object.as_stream() else { return };
        match self.file.get(&stream.dictionary, "Subtype").as_name() {
            Some("Form") => {
                if self.depth >= 12 {
                    return;
                }
                self.depth += 1;
                let saved_state = self.state.clone();
                let saved_stack = self.stack.len();
                if let Some(matrix) = self.file.get(&stream.dictionary, "Matrix").as_array() {
                    if matrix.len() >= 6 {
                        let values: Vec<f64> = matrix
                            .iter()
                            .map(|m| self.file.resolve(m).as_number().unwrap_or(0.0))
                            .collect();
                        let matrix = Matrix([
                            values[0], values[1], values[2], values[3], values[4], values[5],
                        ]);
                        self.state.ctm = matrix.then(self.state.ctm);
                    }
                }
                let own = self.file.get(&stream.dictionary, "Resources");
                let resources = own.as_dictionary().cloned().unwrap_or_else(|| resources.clone());
                let content = self.file.decode(stream).0;
                let saved_text = (self.text_matrix, self.line_matrix);
                self.run(&content, &resources);
                self.text_matrix = saved_text.0;
                self.line_matrix = saved_text.1;
                self.stack.truncate(saved_stack);
                self.state = saved_state;
                self.depth -= 1;
            }
            Some("Image") => {
                let corners = [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)]
                    .map(|(x, y)| self.state.ctm.apply(x, y));
                let x0 = corners.iter().map(|c| c.0).fold(f64::INFINITY, f64::min);
                let x1 = corners.iter().map(|c| c.0).fold(f64::NEG_INFINITY, f64::max);
                let y0 = corners.iter().map(|c| c.1).fold(f64::INFINITY, f64::min);
                let y1 = corners.iter().map(|c| c.1).fold(f64::NEG_INFINITY, f64::max);
                if x1 - x0 < 1.0 || y1 - y0 < 1.0 {
                    return;
                }
                if let Some((bytes, extension)) =
                    super::images::picture_of(self.file, stream, self.state.fill)
                {
                    self.drawn.pictures.push(PlacedPicture { x0, y0, x1, y1, bytes, extension });
                }
            }
            _ => {}
        }
    }
}

/// How many components a colour space object takes.
fn components_of(file: &File<'_>, space: &Object, depth: usize) -> usize {
    if depth > 4 {
        return 1;
    }
    match space {
        Object::Name(name) => match name.as_str() {
            "DeviceRGB" | "CalRGB" | "Lab" => 3,
            "DeviceCMYK" => 4,
            "Pattern" => 0,
            _ => 1,
        },
        Object::Array(items) => {
            let family = items.first().and_then(Object::as_name).unwrap_or("");
            match family {
                "ICCBased" => items
                    .get(1)
                    .map(|s| file.resolve(s))
                    .and_then(|s| {
                        s.as_dictionary().map(|d| file.get(d, "N").as_integer().unwrap_or(3))
                    })
                    .unwrap_or(3) as usize,
                "CalRGB" | "Lab" => 3,
                "CalGray" => 1,
                "Indexed" | "I" => 1,
                "Separation" => 0,
                "DeviceN" => 0,
                "Pattern" => 0,
                _ => 1,
            }
        }
        _ => 1,
    }
}

fn cmyk(c: f64, m: f64, y: f64, k: f64) -> [f32; 3] {
    [((1.0 - c) * (1.0 - k)) as f32, ((1.0 - m) * (1.0 - k)) as f32, ((1.0 - y) * (1.0 - k)) as f32]
}

/// The rectangle a subpath is, if it is one: four or five points with
/// each side level or upright.
fn rectangle_of(points: &[(f64, f64)]) -> Option<Rectangle> {
    let mut points = points.to_vec();
    if points.len() == 5 {
        let (first, last) = (points[0], points[4]);
        if (first.0 - last.0).abs() > 0.01 || (first.1 - last.1).abs() > 0.01 {
            return None;
        }
        points.pop();
    }
    if points.len() != 4 {
        return None;
    }
    for index in 0..4 {
        let (x0, y0) = points[index];
        let (x1, y1) = points[(index + 1) % 4];
        if (x0 - x1).abs() > 0.01 && (y0 - y1).abs() > 0.01 {
            return None;
        }
    }
    let x0 = points.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
    let x1 = points.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
    let y0 = points.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    let y1 = points.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
    Some(Rectangle { x0, y0, x1, y1, filled: false })
}

/// Steps over an inline picture: its dictionary up to `ID`, one byte of
/// white space, then the data up to an `EI` standing on its own.
fn skip_inline_image(lexer: &mut Lexer<'_>) {
    let mut length: Option<usize> = None;
    let mut key: Option<String> = None;
    while let Some(object) = lexer.next_object() {
        match object {
            Object::Operator(word) if word == "ID" => break,
            Object::Operator(_) => {}
            Object::Name(name) if key.is_none() => key = Some(name),
            value => {
                if let Some(name) = key.take() {
                    if name == "L" || name == "Length" {
                        length = value.as_integer().and_then(|l| usize::try_from(l).ok());
                    }
                }
            }
        }
    }
    lexer.at = (lexer.at + 1).min(lexer.bytes.len());
    if let Some(length) = length {
        lexer.at = (lexer.at + length).min(lexer.bytes.len());
    }
    let bytes = lexer.bytes;
    let mut at = lexer.at;
    while at + 1 < bytes.len() {
        if bytes[at] == b'E'
            && bytes[at + 1] == b'I'
            && (at == 0 || super::object::is_whitespace(bytes[at - 1]))
            && bytes
                .get(at + 2)
                .is_none_or(|b| super::object::is_whitespace(*b) || super::object::is_delimiter(*b))
        {
            lexer.at = at + 2;
            return;
        }
        at += 1;
    }
    lexer.at = bytes.len();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrices_compose_in_order() {
        let scale = Matrix([2.0, 0.0, 0.0, 2.0, 0.0, 0.0]);
        let shift = Matrix::translate(10.0, 5.0);
        assert_eq!(scale.then(shift).apply(1.0, 1.0), (12.0, 7.0));
        assert_eq!(shift.then(scale).apply(1.0, 1.0), (22.0, 12.0));
    }

    #[test]
    fn a_rectangle_is_known_from_its_corners() {
        let points = [(0.0, 0.0), (10.0, 0.0), (10.0, 2.0), (0.0, 2.0), (0.0, 0.0)];
        let rectangle = rectangle_of(&points).unwrap();
        assert_eq!((rectangle.x0, rectangle.y0, rectangle.x1, rectangle.y1), (0.0, 0.0, 10.0, 2.0));
        assert!(rectangle_of(&[(0.0, 0.0), (10.0, 3.0), (10.0, 2.0), (0.0, 2.0)]).is_none());
    }

    #[test]
    fn an_inline_image_is_stepped_over() {
        let content = b"BI /W 2 /H 2 /BPC 8 /CS /G ID \x00\xFFEI\x80 EI Q";
        let mut lexer = Lexer::new(content);
        assert!(matches!(lexer.next_object(), Some(Object::Operator(word)) if word == "BI"));
        skip_inline_image(&mut lexer);
        assert!(matches!(lexer.next_object(), Some(Object::Operator(word)) if word == "Q"));
    }
}
