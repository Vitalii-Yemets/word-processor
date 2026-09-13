//! What the two metafile formats have in common.
//!
//! # What a metafile is
//!
//! Not a picture but a recording of how one was drawn: a list of the calls a
//! program made — take this pen, draw a line here, fill this polygon — written
//! down and played back. Which is why a document that was going to hold a
//! diagram held one of these: it draws at any size, and in 1990 that mattered
//! more than anything.
//!
//! # Why it is turned into pixels here
//!
//! Because everything above this draws pictures, and a picture is pixels. A
//! metafile is played back onto a canvas of its own at the size it says it is,
//! and what comes out is a picture like any other. Something is lost by that —
//! a metafile scaled up afterwards is no sharper than the canvas it was played
//! onto — and that is the price of one road through the program rather than
//! two. It is named in the roadmap rather than hidden.
//!
//! # The state
//!
//! Both formats are stacks of the same few things: a pen, a brush, where the
//! last line ended, how logical coordinates map onto the page, and which of the
//! two fill rules is in force. The records change one of those or draw with
//! them, and nothing else.

use wp_raster::{Canvas, Color, Path, Point, Rule, Transform};

/// A pen, a brush, a font, or a slot that holds none of them.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Object {
    /// A slot that has been made and not filled, or one that was deleted.
    #[default]
    Empty,
    /// The line round a shape: a colour and a width, or nothing at all.
    Pen(Option<(Color, f32)>),
    /// What fills a shape, or nothing at all.
    Brush(Option<Color>),
    /// What words are drawn in.
    Font(LogFont),
}

/// The face a file asks for, as the file describes it.
///
/// What the file says and not what this machine has: a metafile recorded in
/// 1994 asks for "MS Sans Serif", and which of the fonts actually here is
/// nearest to that is somebody else's business. See [`Faces`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LogFont {
    pub family: String,
    /// How tall, in logical units. The file states this as the height of a
    /// letter when it is negative and of the whole line when it is positive;
    /// what is kept here is always the letter, because that is what a font is
    /// asked for in.
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    /// How far round the line of words is turned, in tenths of a degree and
    /// anticlockwise — which is how both formats measure it, and the other way
    /// round from a canvas.
    pub escapement: i32,
}

/// Where a metafile's words get their letter shapes from.
///
/// # Why this is asked for rather than looked up
///
/// Because what fonts a machine has is not something a picture decoder can
/// know. A metafile names a face the way the program that recorded it knew it,
/// and matching that against what is actually installed is the business of
/// whatever opened the document — which has a list of them already, and a rule
/// for what to fall back on. So the player asks, and a caller that has no fonts
/// to offer gets a picture with its shapes and none of its words: see
/// [`crate::decode`].
pub trait Faces {
    /// The face nearest what a record asked for, or nothing.
    fn face(&self, family: &str, bold: bool, italic: bool) -> Option<wp_font::Font<'_>>;
}

/// A caller with no fonts to offer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NoFaces;

impl Faces for NoFaces {
    fn face(&self, _family: &str, _bold: bool, _italic: bool) -> Option<wp_font::Font<'_>> {
        None
    }
}

/// How a run of words is placed against the point a record gives.
///
/// The flags both formats share: which end of the words the point is, and
/// whether it is their top, their bottom or the line they stand on.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Alignment(pub u16);

impl Alignment {
    /// How far back from the point the words start, as a share of their width.
    #[must_use]
    pub fn back(self) -> f32 {
        match self.0 & 6 {
            2 => 1.0,
            6 => 0.5,
            _ => 0.0,
        }
    }

    /// Whether the point is the line the words stand on, rather than the top or
    /// the bottom of them.
    #[must_use]
    pub fn on_the_baseline(self) -> bool {
        self.0 & 24 == 24
    }

    /// Whether it is the bottom of them.
    #[must_use]
    pub fn at_the_bottom(self) -> bool {
        self.0 & 24 == 8
    }
}

/// Everything the records that draw words work from.
#[derive(Clone, Debug, PartialEq)]
pub struct Words {
    pub colour: Color,
    pub align: Alignment,
    pub font: LogFont,
}

impl Default for Words {
    fn default() -> Self {
        // Black, which is what both formats draw words in until a record says
        // otherwise.
        Self { colour: Color::BLACK, align: Alignment::default(), font: LogFont::default() }
    }
}

/// Everything a record can change or draw with.
#[derive(Clone, Debug)]
pub struct State {
    /// The canvas the records are played back onto.
    pub canvas: Canvas,
    /// The objects the file has made, by the numbers it refers to them by.
    pub objects: Vec<Object>,
    pub pen: Option<(Color, f32)>,
    pub brush: Option<Color>,
    /// Where the last line ended, in logical coordinates.
    pub at: Point,
    /// How a logical coordinate becomes a place on the canvas.
    pub transform: Transform,
    /// The transform the file states on top of that, which only the newer
    /// format has.
    pub world: Transform,
    pub rule: Rule,
    /// The path being built between the records that begin and end one, when
    /// the file is building one.
    pub path: Option<Path>,
    /// What the records that draw words work from: the colour, where the point
    /// they are given sits against them, and the face they are drawn in.
    pub words: Words,
}

impl State {
    /// A state ready to play a file back onto a canvas of a given size.
    #[must_use]
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            // White, because a metafile draws on paper and says nothing about
            // what is under it. A transparent canvas would leave every gap in
            // a diagram showing the page through it, which is not what the
            // program that recorded it drew.
            canvas: Canvas::filled(width, height, Color::WHITE),
            objects: Vec::new(),
            pen: Some((Color::BLACK, 1.0)),
            brush: Some(Color::WHITE),
            at: Point::new(0.0, 0.0),
            transform: Transform::IDENTITY,
            world: Transform::IDENTITY,
            rule: Rule::EvenOdd,
            path: None,
            words: Words::default(),
        }
    }

    /// Puts an object in the first empty slot, or at the end.
    ///
    /// Which slot a file's objects land in matters: the records that select one
    /// name it by its number, and the number is the place it went into.
    pub fn add(&mut self, object: Object) {
        if let Some(slot) = self.objects.iter_mut().find(|slot| **slot == Object::Empty) {
            *slot = object;
            return;
        }
        self.objects.push(object);
    }

    /// Takes up whichever object a record names.
    pub fn select(&mut self, index: usize) {
        match self.objects.get(index).cloned().unwrap_or_default() {
            Object::Pen(pen) => self.pen = pen,
            Object::Brush(brush) => self.brush = brush,
            Object::Font(font) => self.words.font = font,
            Object::Empty => {}
        }
    }

    /// A logical point, where it lands on the canvas.
    #[must_use]
    pub fn place(&self, x: f32, y: f32) -> Point {
        let point = self.world.apply(Point::new(x, y));
        self.transform.apply(point)
    }

    /// How far one logical unit goes on the canvas.
    ///
    /// A pen's width is stated in logical units like everything else, and has
    /// to be brought into pixels before it is drawn with — a file that draws at
    /// sixteen times the size with a pen sixteen times as wide means a line of
    /// the same thickness, and a reader that missed that would black the
    /// picture out.
    #[must_use]
    pub fn scale(&self) -> f32 {
        let origin = self.place(0.0, 0.0);
        let along = self.place(1.0, 0.0);
        let down = self.place(0.0, 1.0);
        let across = (along.x - origin.x).hypot(along.y - origin.y);
        let downwards = (down.x - origin.x).hypot(down.y - origin.y);
        ((across + downwards) / 2.0).max(0.0001)
    }

    /// Draws a run of words at a point, the way the file asks for it.
    ///
    /// # What settles where they go
    ///
    /// Three things, and all of them are the file's. The point itself, which
    /// may be the left end of the words or their right or their middle. The
    /// line they stand on, which may be that point or the top of them or the
    /// bottom. And how far round they are turned, which the file states in
    /// tenths of a degree anticlockwise — the other way from a canvas, so the
    /// sign changes here.
    ///
    /// Nothing is drawn when there is no face to draw with. A picture missing
    /// its words is a worse picture; a picture with its words in a face nobody
    /// asked for would be a wrong one.
    pub fn draw_words(&mut self, faces: &dyn Faces, x: f32, y: f32, text: &str) {
        if text.is_empty() {
            return;
        }
        let asked = &self.words.font;
        let Some(font) = faces
            .face(&asked.family, asked.bold, asked.italic)
            .or_else(|| faces.face("", asked.bold, asked.italic))
        else {
            return;
        };

        // How big the letters are on the canvas: the size the file states, in
        // logical units, through the mapping in force.
        let size = asked.size.abs().max(1.0) * self.scale();
        let units = f32::from(font.units_per_em().max(1));
        let scale = size / units;

        // How wide the run is, which is what the alignment is measured from.
        let glyphs: Vec<_> = text.chars().filter_map(|letter| font.glyph_for(letter)).collect();
        if glyphs.len() != text.chars().count() {
            // A face that cannot draw every letter of the run is a face that
            // would draw the run wrong. Nothing rather than something wrong.
            return;
        }
        let width: f32 = glyphs.iter().map(|glyph| f32::from(font.advance(*glyph)) * scale).sum();

        let metrics = font.vertical_metrics();
        let ascent = metrics.ascender as f32 * scale;
        let descent = metrics.descender as f32 * scale;
        let at = self.place(x, y);
        // Where the pen starts: back from the point by as much of the run as
        // the alignment asks for, and down to the line the letters stand on.
        let start_x = at.x - width * self.words.align.back();
        let start_y = if self.words.align.on_the_baseline() {
            at.y
        } else if self.words.align.at_the_bottom() {
            at.y + descent
        } else {
            at.y + ascent
        };

        let turn = -(asked.escapement as f32 / 10.0).to_radians();
        let colour = self.words.colour;
        let mut pen = 0.0;
        for glyph in glyphs {
            if let Ok(Some(outline)) = font.outline(glyph) {
                let mut path = Path::new();
                for command in &outline.commands {
                    match *command {
                        wp_font::PathCommand::MoveTo(point) => {
                            path.move_to(Point::new(point.x, point.y));
                        }
                        wp_font::PathCommand::LineTo(point) => {
                            path.line_to(Point::new(point.x, point.y));
                        }
                        wp_font::PathCommand::QuadTo(control, point) => {
                            path.quad_to(
                                Point::new(control.x, control.y),
                                Point::new(point.x, point.y),
                            );
                        }
                        wp_font::PathCommand::Close => {
                            path.close();
                        }
                    }
                }
                // The letter itself, then where it stands, then how far round
                // the whole run is turned about where it began.
                let placed = Transform::glyph(scale, start_x + pen, start_y)
                    .then(&Transform::rotate_about(turn, start_x, start_y));
                self.canvas.fill_path(&path.transformed(&placed), colour);
            }
            pen += f32::from(font.advance(glyph)) * scale;
        }
    }

    /// Draws a shape: filled with the brush, then outlined with the pen.
    ///
    /// In that order, because that is the order the formats draw in and the
    /// difference shows: a wide pen on a filled shape covers half its own width
    /// of the fill, and drawn the other way round it would not.
    pub fn draw(&mut self, path: &Path) {
        // A path being built is collected rather than drawn: the record that
        // ends it says what to do with the whole of it.
        if let Some(building) = &mut self.path {
            for command in &path.commands {
                building.commands.push(*command);
            }
            return;
        }
        self.fill_and_stroke(path);
    }

    /// The same, for a path the file has finished building.
    pub fn fill_and_stroke(&mut self, path: &Path) {
        if let Some(brush) = self.brush {
            self.canvas.fill_path_by(path, brush, self.rule);
        }
        if let Some((colour, width)) = self.pen {
            let outline = stroke(path, (width * self.scale()).max(1.0));
            self.canvas.fill_path(&outline, colour);
        }
    }

    /// A line, which is drawn with the pen and never filled.
    pub fn stroke_only(&mut self, path: &Path) {
        if let Some(building) = &mut self.path {
            for command in &path.commands {
                building.commands.push(*command);
            }
            return;
        }
        if let Some((colour, width)) = self.pen {
            let outline = stroke(path, (width * self.scale()).max(1.0));
            self.canvas.fill_path(&outline, colour);
        }
    }
}

/// Turns a path into the shape its own outline covers.
///
/// # Why it is done this way
///
/// Because the rasterizer fills shapes and does not draw lines. A line of a
/// given width is a long thin rectangle, and a run of them is a rectangle
/// apiece with a square at each join to fill the wedge between them. That is
/// not what a drawing program means by a round or a mitred join, but at the
/// widths a metafile uses — one pixel, two, occasionally five — the difference
/// is a corner pixel, and the alternative is a line-joining geometry nothing
/// else here needs.
pub fn stroke(path: &Path, width: f32) -> Path {
    let half = (width / 2.0).max(0.5);
    let mut out = Path::new();
    let mut start = Point::new(0.0, 0.0);
    let mut current = start;
    let mut down = false;

    let segment = |out: &mut Path, from: Point, to: Point| {
        let (dx, dy) = (to.x - from.x, to.y - from.y);
        let length = (dx * dx + dy * dy).sqrt();
        if length < 0.0001 {
            // A line going nowhere still leaves the pen's mark where it is.
            out.move_to(Point::new(from.x - half, from.y - half));
            out.line_to(Point::new(from.x + half, from.y - half));
            out.line_to(Point::new(from.x + half, from.y + half));
            out.line_to(Point::new(from.x - half, from.y + half));
            out.close();
            return;
        }
        // Out to the side of the line by half the pen's width, each way.
        let (nx, ny) = (-dy / length * half, dx / length * half);
        out.move_to(Point::new(from.x + nx, from.y + ny));
        out.line_to(Point::new(to.x + nx, to.y + ny));
        out.line_to(Point::new(to.x - nx, to.y - ny));
        out.line_to(Point::new(from.x - nx, from.y - ny));
        out.close();

        // A square at the far end, which fills the wedge a turn leaves open.
        out.move_to(Point::new(to.x - half, to.y - half));
        out.line_to(Point::new(to.x + half, to.y - half));
        out.line_to(Point::new(to.x + half, to.y + half));
        out.line_to(Point::new(to.x - half, to.y + half));
        out.close();
    };

    for command in &path.commands {
        match *command {
            wp_raster::Command::MoveTo(point) => {
                start = point;
                current = point;
                down = false;
            }
            wp_raster::Command::LineTo(point) => {
                segment(&mut out, current, point);
                current = point;
                down = true;
            }
            wp_raster::Command::QuadTo(control, point) => {
                for step in 1..=8 {
                    let t = step as f32 / 8.0;
                    let next = quadratic(current, control, point, t);
                    segment(&mut out, current, next);
                    current = next;
                }
                down = true;
            }
            wp_raster::Command::CubicTo(first, second, point) => {
                for step in 1..=12 {
                    let t = step as f32 / 12.0;
                    let next = cubic(current, first, second, point, t);
                    segment(&mut out, current, next);
                    current = next;
                }
                down = true;
            }
            wp_raster::Command::Close => {
                if down {
                    segment(&mut out, current, start);
                }
                current = start;
            }
        }
    }

    out
}

fn quadratic(from: Point, control: Point, to: Point, t: f32) -> Point {
    let inverse = 1.0 - t;
    Point::new(
        inverse * inverse * from.x + 2.0 * inverse * t * control.x + t * t * to.x,
        inverse * inverse * from.y + 2.0 * inverse * t * control.y + t * t * to.y,
    )
}

fn cubic(from: Point, first: Point, second: Point, to: Point, t: f32) -> Point {
    let inverse = 1.0 - t;
    let (a, b, c, d) = (
        inverse * inverse * inverse,
        3.0 * inverse * inverse * t,
        3.0 * inverse * t * t,
        t * t * t,
    );
    Point::new(
        a * from.x + b * first.x + c * second.x + d * to.x,
        a * from.y + b * first.y + c * second.y + d * to.y,
    )
}

/// An ellipse inside a rectangle, as four curves.
///
/// The magic number is how far along each side a control point has to sit for
/// a cubic to pass for a quarter of a circle. It is not exact and cannot be —
/// no cubic is a circle — but it is out by less than a thousandth of the
/// radius, which is a great deal less than a pixel.
#[must_use]
pub fn ellipse(left: f32, top: f32, right: f32, bottom: f32) -> Path {
    const PULL: f32 = 0.552_284_8;
    let (middle_x, middle_y) = ((left + right) / 2.0, (top + bottom) / 2.0);
    let (radius_x, radius_y) = ((right - left) / 2.0, (bottom - top) / 2.0);
    let (pull_x, pull_y) = (radius_x * PULL, radius_y * PULL);

    let mut path = Path::new();
    path.move_to(Point::new(middle_x, top));
    path.cubic_to(
        Point::new(middle_x + pull_x, top),
        Point::new(right, middle_y - pull_y),
        Point::new(right, middle_y),
    );
    path.cubic_to(
        Point::new(right, middle_y + pull_y),
        Point::new(middle_x + pull_x, bottom),
        Point::new(middle_x, bottom),
    );
    path.cubic_to(
        Point::new(middle_x - pull_x, bottom),
        Point::new(left, middle_y + pull_y),
        Point::new(left, middle_y),
    );
    path.cubic_to(
        Point::new(left, middle_y - pull_y),
        Point::new(middle_x - pull_x, top),
        Point::new(middle_x, top),
    );
    path.close();
    path
}

/// A rectangle with its corners rounded off.
#[must_use]
pub fn rounded(left: f32, top: f32, right: f32, bottom: f32, across: f32, down: f32) -> Path {
    let across = (across / 2.0).abs().min((right - left).abs() / 2.0);
    let down = (down / 2.0).abs().min((bottom - top).abs() / 2.0);
    if across <= 0.0 || down <= 0.0 {
        return Path::rectangle(
            left.min(right),
            top.min(bottom),
            (right - left).abs(),
            (bottom - top).abs(),
        );
    }
    const PULL: f32 = 0.552_284_8;
    let (pull_x, pull_y) = (across * PULL, down * PULL);

    let mut path = Path::new();
    path.move_to(Point::new(left + across, top));
    path.line_to(Point::new(right - across, top));
    path.cubic_to(
        Point::new(right - across + pull_x, top),
        Point::new(right, top + down - pull_y),
        Point::new(right, top + down),
    );
    path.line_to(Point::new(right, bottom - down));
    path.cubic_to(
        Point::new(right, bottom - down + pull_y),
        Point::new(right - across + pull_x, bottom),
        Point::new(right - across, bottom),
    );
    path.line_to(Point::new(left + across, bottom));
    path.cubic_to(
        Point::new(left + across - pull_x, bottom),
        Point::new(left, bottom - down + pull_y),
        Point::new(left, bottom - down),
    );
    path.line_to(Point::new(left, top + down));
    path.cubic_to(
        Point::new(left, top + down - pull_y),
        Point::new(left + across - pull_x, top),
        Point::new(left + across, top),
    );
    path.close();
    path
}

/// A colour as the metafile formats write one: blue, green, red, and a byte
/// that says nothing.
#[must_use]
pub fn colour_of(value: u32) -> Color {
    Color::rgb((value & 0xFF) as u8, ((value >> 8) & 0xFF) as u8, ((value >> 16) & 0xFF) as u8)
}

/// Whether a pen of a given style draws anything.
///
/// Style five is the one that means no line at all. The dashes and dots are
/// drawn solid: at the widths a metafile uses the difference is a few pixels,
/// and a dashed line drawn solid is a line, where one drawn as nothing is a
/// shape with a piece missing.
#[must_use]
pub fn pen_of(style: u32, width: f32, colour: Color) -> Option<(Color, f32)> {
    if style & 0xFF == 5 {
        return None;
    }
    Some((colour, width))
}

/// Whether a brush of a given style fills anything.
///
/// Style one is the hollow brush. The hatched styles are filled solid, for the
/// same reason the dashes are drawn solid.
#[must_use]
pub fn brush_of(style: u32, colour: Color) -> Option<Color> {
    if style == 1 {
        return None;
    }
    Some(colour)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_colour_is_written_blue_first() {
        assert_eq!(colour_of(0x00_11_22_33), Color::rgb(0x33, 0x22, 0x11));
    }

    #[test]
    fn the_pen_that_draws_nothing_draws_nothing() {
        assert_eq!(pen_of(5, 1.0, Color::BLACK), None);
        assert_eq!(pen_of(0, 2.0, Color::BLACK), Some((Color::BLACK, 2.0)));
        // A dashed pen is a pen: drawn solid, but drawn.
        assert!(pen_of(1, 1.0, Color::BLACK).is_some());
    }

    #[test]
    fn the_brush_that_fills_nothing_fills_nothing() {
        assert_eq!(brush_of(1, Color::BLACK), None);
        assert_eq!(brush_of(0, Color::BLACK), Some(Color::BLACK));
        // A hatched brush fills: solid, but it fills.
        assert!(brush_of(2, Color::BLACK).is_some());
    }

    #[test]
    fn an_object_goes_into_the_first_empty_slot() {
        // Which slot matters: the records that select one name it by number.
        let mut state = State::new(1, 1);
        state.add(Object::Pen(None));
        state.add(Object::Brush(None));
        state.objects[0] = Object::Empty;
        state.add(Object::Pen(Some((Color::BLACK, 1.0))));
        assert_eq!(state.objects[0], Object::Pen(Some((Color::BLACK, 1.0))));
        assert_eq!(state.objects.len(), 2, "it should not have gone on the end");
    }

    #[test]
    fn selecting_an_object_takes_it_up() {
        let mut state = State::new(1, 1);
        state.add(Object::Pen(Some((Color::rgb(1, 2, 3), 4.0))));
        state.add(Object::Brush(Some(Color::rgb(5, 6, 7))));
        state.select(0);
        state.select(1);
        assert_eq!(state.pen, Some((Color::rgb(1, 2, 3), 4.0)));
        assert_eq!(state.brush, Some(Color::rgb(5, 6, 7)));
    }

    #[test]
    fn selecting_a_slot_that_holds_nothing_changes_nothing() {
        let mut state = State::new(1, 1);
        let pen = state.pen;
        state.select(99);
        assert_eq!(state.pen, pen);
    }

    #[test]
    fn a_stroked_line_covers_its_own_width() {
        let mut path = Path::new();
        path.move_to(Point::new(2.0, 5.0));
        path.line_to(Point::new(8.0, 5.0));
        let outline = stroke(&path, 4.0);

        let mut canvas = Canvas::filled(10, 10, Color::WHITE);
        canvas.fill_path(&outline, Color::BLACK);
        assert_eq!(canvas.pixel(5, 5), Color::BLACK, "the middle of the line");
        assert_eq!(canvas.pixel(5, 1), Color::WHITE, "and well above it");
    }

    #[test]
    fn an_ellipse_fills_its_middle_and_misses_its_corners() {
        let path = ellipse(0.0, 0.0, 20.0, 20.0);
        let mut canvas = Canvas::filled(20, 20, Color::WHITE);
        canvas.fill_path(&path, Color::BLACK);
        assert_eq!(canvas.pixel(10, 10), Color::BLACK, "the middle");
        assert_eq!(canvas.pixel(0, 0), Color::WHITE, "and not the corner");
    }

    #[test]
    fn a_rounded_rectangle_with_no_rounding_is_a_rectangle() {
        let path = rounded(0.0, 0.0, 10.0, 10.0, 0.0, 0.0);
        let mut canvas = Canvas::filled(10, 10, Color::WHITE);
        canvas.fill_path(&path, Color::BLACK);
        assert_eq!(canvas.pixel(0, 0), Color::BLACK, "the corner should be square");
    }

    #[test]
    fn a_rounded_rectangle_loses_its_corners() {
        let path = rounded(0.0, 0.0, 20.0, 20.0, 16.0, 16.0);
        let mut canvas = Canvas::filled(20, 20, Color::WHITE);
        canvas.fill_path(&path, Color::BLACK);
        assert_eq!(canvas.pixel(10, 10), Color::BLACK, "the middle is filled");
        assert_eq!(canvas.pixel(0, 0), Color::WHITE, "and the corner is not");
    }
}
