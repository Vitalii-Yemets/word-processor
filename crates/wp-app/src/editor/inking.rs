//! Word's Draw tab: the pens that draw on the page, the erasers, and what the
//! ink can be turned into afterwards.
//!
//! # How a stroke becomes ink
//!
//! The pointer goes down on a page and the points it passes are kept in
//! pixels until it comes up. Then they are turned into the ink's own units,
//! the corner of what the pen covered is found, and the stroke is written as
//! Word writes one: a part of its own, in a drawing that floats — across from
//! the page's edge, down from the paragraph the stroke started beside. That
//! is why a stroke stays where it was drawn: the paragraph moves and the
//! ink moves with it, as Word's does.
//!
//! # Why every stroke is a drawing of its own
//!
//! Because that is how Word keeps them, and because it is what makes the
//! erasers simple: rubbing out a stroke is taking a drawing out, and rubbing
//! out part of one is writing that drawing's part again with the rest.
//!
//! # What the ink can become
//!
//! A shape, when the strokes chosen make one this program can see — a line,
//! a rectangle, an ellipse, a triangle, a diamond, a pentagon or a hexagon —
//! and text, when they make block capitals or digits. See
//! [`super::handwriting`] for the second; the first is here.

use wp_docx::anchor::{Anchor, Placement, Relative, Wrap};
use wp_docx::colour::Colour;
use wp_docx::ink::{Ink, Stroke};
use wp_docx::shapes::{Shape, EMU_PER_POINT};
use wp_docx::TextPosition;
use wp_layout::inking::Fit;
use wp_shell::Response;

use crate::chrome::{Choice, Command, Popup};
use crate::messages::t;

use super::Editor;

/// The three pens Word's Draw tab starts with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PenKind {
    Pen,
    Pencil,
    Highlighter,
}

impl PenKind {
    /// The button that takes the pen up.
    pub(crate) fn command(self) -> Command {
        match self {
            Self::Pen => Command::DrawPen,
            Self::Pencil => Command::DrawPencil,
            Self::Highlighter => Command::DrawHighlighter,
        }
    }

    /// And the list its arrow drops: the thicknesses and the colours.
    pub(crate) fn choice(self) -> Choice {
        match self {
            Self::Pen => Choice::PenLook,
            Self::Pencil => Choice::PencilLook,
            Self::Highlighter => Choice::HighlighterLook,
        }
    }

    fn index(self) -> usize {
        match self {
            Self::Pen => 0,
            Self::Pencil => 1,
            Self::Highlighter => 2,
        }
    }

    /// How thick the pen may be, in hundredths of a millimetre: Word's own
    /// list for a pen, and the wider one for a highlighter.
    fn widths(self) -> &'static [u16] {
        match self {
            Self::Highlighter => &[200, 400, 600, 800, 1200],
            _ => &[25, 35, 50, 100, 200, 350],
        }
    }

    /// And the colours it may be, as Word's gallery offers them.
    fn colours(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::Highlighter => HIGHLIGHTER_COLOURS,
            _ => PEN_COLOURS,
        }
    }

    /// What a pen is when nothing has been chosen for it.
    fn usual(self) -> PenLook {
        match self {
            Self::Pen => PenLook { colour: "000000", width: 50 },
            Self::Pencil => PenLook { colour: "595959", width: 50 },
            Self::Highlighter => PenLook { colour: "FFFF00", width: 600 },
        }
    }
}

/// The colours a pen may be, each with its name.
pub(crate) const PEN_COLOURS: &[(&str, &str)] = &[
    ("Black", "000000"),
    ("Dark Gray", "595959"),
    ("Red", "C00000"),
    ("Orange", "ED7D31"),
    ("Gold", "FFC000"),
    ("Green", "70AD47"),
    ("Blue", "4472C4"),
    ("Purple", "7030A0"),
    ("Pink", "FF66CC"),
];

/// And a highlighter, which is the light ones the words show through.
pub(crate) const HIGHLIGHTER_COLOURS: &[(&str, &str)] = &[
    ("Yellow", "FFFF00"),
    ("Lime", "92D050"),
    ("Turquoise", "00FFFF"),
    ("Pink", "FF66CC"),
    ("Orange", "FFC000"),
];

/// What a pen is set to: its colour and its thickness in hundredths of a
/// millimetre.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PenLook {
    pub colour: &'static str,
    pub width: u16,
}

impl PenLook {
    /// The thickness in English metric units.
    fn width_emu(self) -> i64 {
        i64::from(self.width) * 360
    }
}

/// What is in hand on the Draw tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InkTool {
    Draw(PenKind),
    /// Takes a whole stroke away at a touch.
    StrokeEraser,
    /// Rubs out what it passes over and leaves the rest of the stroke.
    PointEraser,
}

/// A stroke being drawn: which page it is on, and where the pointer has
/// been, in pixels of the window.
#[derive(Clone, Debug, PartialEq)]
struct StrokeInHand {
    page: usize,
    points: Vec<(f32, f32)>,
}

/// The state of the Draw tab.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Inking {
    tool: Option<InkTool>,
    looks: [PenLook; 3],
    stroke: Option<StrokeInHand>,
    /// Whether an eraser is down, so that moving the pointer goes on erasing.
    erasing: bool,
}

impl Default for Inking {
    fn default() -> Self {
        Self {
            tool: None,
            looks: [PenKind::Pen.usual(), PenKind::Pencil.usual(), PenKind::Highlighter.usual()],
            stroke: None,
            erasing: false,
        }
    }
}

/// How far a point eraser reaches, in pixels.
const ERASER_REACH: f32 = 8.0;

/// How near a stroke the pointer has to be to touch it, in pixels, for a
/// stroke thinner than that.
const TOUCH: f32 = 4.0;

impl Editor {
    /// Which tool the Draw tab has in hand, if any.
    #[must_use]
    pub(crate) fn ink_tool(&self) -> Option<InkTool> {
        self.inking.tool
    }

    /// The pen's colour and thickness.
    #[must_use]
    pub(crate) fn pen_look(&self, kind: PenKind) -> PenLook {
        self.inking.looks[kind.index()]
    }

    /// Takes a pen up, or puts it down again if it was the one in hand.
    pub(super) fn take_pen(&mut self, kind: PenKind) -> Response {
        self.set_ink_tool(InkTool::Draw(kind))
    }

    /// Takes an eraser up, or puts it down again.
    pub(super) fn take_eraser(&mut self, whole: bool) -> Response {
        self.set_ink_tool(if whole { InkTool::StrokeEraser } else { InkTool::PointEraser })
    }

    fn set_ink_tool(&mut self, tool: InkTool) -> Response {
        self.popup = None;
        if self.inking.tool == Some(tool) {
            return self.put_ink_tool_down();
        }
        // A pen and Select Objects are both modes, and only one of them
        // takes the press.
        if self.choosing_drawings() {
            self.toggle_choosing_drawings();
        }
        self.inking.tool = Some(tool);
        self.inking.stroke = None;
        self.needs_redraw = true;
        let note = match tool {
            InkTool::Draw(kind) => match kind {
                PenKind::Pen => t("Pen: draw on the page. Escape to stop"),
                PenKind::Pencil => t("Pencil: draw on the page. Escape to stop"),
                PenKind::Highlighter => t("Highlighter: draw over the words. Escape to stop"),
            },
            InkTool::StrokeEraser => t("Eraser: touch a stroke to take it away. Escape to stop"),
            InkTool::PointEraser => t("Eraser: rub out part of a stroke. Escape to stop"),
        };
        self.report(note)
    }

    /// The Draw tab's Select: Word's Select Objects, with the pen put down
    /// first, since a press cannot both draw and choose.
    pub(super) fn draw_select(&mut self) -> Response {
        if self.inking.tool.is_some() {
            self.inking.tool = None;
            self.inking.stroke = None;
            self.inking.erasing = false;
        }
        self.toggle_choosing_drawings()
    }

    /// Puts whatever the Draw tab had in hand down. Escape, and the button
    /// pressed again.
    pub(super) fn put_ink_tool_down(&mut self) -> Response {
        if self.inking.tool.is_none() {
            return Response::Ignored;
        }
        self.inking.tool = None;
        self.inking.stroke = None;
        self.inking.erasing = false;
        self.needs_redraw = true;
        self.report(t("Pen put down"))
    }

    /// Whether a pen or an eraser is in hand, which is when a press on the
    /// page draws rather than putting the caret there.
    #[must_use]
    pub(super) fn inking(&self) -> bool {
        self.inking.tool.is_some()
    }

    /// Drops the list of what a pen may be: its thicknesses, then its colours.
    pub(super) fn open_pen_look(&mut self, kind: PenKind) -> Response {
        if self.close_popup_if(kind.choice()) {
            return Response::Redraw;
        }
        let Some((left, top, _)) = self.ribbon.command_rect(kind.command()) else {
            return Response::Ignored;
        };
        let look = self.pen_look(kind);
        let mut items: Vec<String> =
            kind.widths().iter().map(|width| format!("{} mm", mm(*width))).collect();
        items.extend(kind.colours().iter().map(|(name, _)| t(name).to_owned()));
        // Whichever of them is in force: the thickness, since a list can
        // light one entry, and the thickness is the one a person changes
        // most.
        let current = kind.widths().iter().position(|width| *width == look.width);
        self.popup = Some(Popup::new(kind.choice(), items, current, left, top, 200.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// One entry of that list chosen: the pen takes it, and is taken up.
    pub(super) fn choose_pen_look(&mut self, kind: PenKind, index: usize) -> Response {
        self.popup = None;
        let widths = kind.widths();
        let look = &mut self.inking.looks[kind.index()];
        if let Some(width) = widths.get(index) {
            look.width = *width;
        } else if let Some((_, colour)) = kind.colours().get(index - widths.len()) {
            look.colour = colour;
        } else {
            return Response::Ignored;
        }
        if self.inking.tool != Some(InkTool::Draw(kind)) {
            return self.take_pen(kind);
        }
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Drops the two erasers.
    pub(super) fn open_erasers(&mut self) -> Response {
        if self.close_popup_if(Choice::EraserKind) {
            return Response::Redraw;
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::DrawEraser) else {
            return Response::Ignored;
        };
        let items = vec![t("Stroke Eraser").to_owned(), t("Point Eraser").to_owned()];
        let current = match self.inking.tool {
            Some(InkTool::StrokeEraser) => Some(0),
            Some(InkTool::PointEraser) => Some(1),
            _ => None,
        };
        self.popup = Some(Popup::new(Choice::EraserKind, items, current, left, top, 180.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    pub(super) fn choose_eraser(&mut self, index: usize) -> Response {
        self.popup = None;
        let tool = if index == 0 { InkTool::StrokeEraser } else { InkTool::PointEraser };
        if self.inking.tool == Some(tool) {
            self.needs_redraw = true;
            return Response::Redraw;
        }
        self.set_ink_tool(tool)
    }

    // --- the pointer ------------------------------------------------------------

    /// Takes a press while something from the Draw tab is in hand: a pen
    /// starts a stroke, an eraser starts rubbing out. A press off the page
    /// is left alone.
    pub(super) fn ink_press(&mut self, x: i32, y: i32) -> bool {
        let Some(tool) = self.inking.tool else { return false };
        let Some(page) = self.page_index_under(x, y) else { return false };
        match tool {
            InkTool::Draw(_) => {
                self.inking.stroke =
                    Some(StrokeInHand { page, points: vec![(x as f32, y as f32)] });
            }
            InkTool::StrokeEraser | InkTool::PointEraser => {
                self.inking.erasing = true;
                self.erase_at(x, y, tool == InkTool::PointEraser);
            }
        }
        self.needs_redraw = true;
        true
    }

    /// Carries a stroke or an eraser on as the pointer moves.
    pub(super) fn ink_move(&mut self, x: i32, y: i32, held: bool) -> Option<Response> {
        if !held {
            if self.inking.stroke.is_some() || self.inking.erasing {
                self.ink_release(x, y);
                return Some(Response::Redraw);
            }
            return None;
        }
        if let Some(stroke) = &mut self.inking.stroke {
            let (px, py) = (x as f32, y as f32);
            // A point a pixel or less from the last says nothing new, and a
            // stroke of a thousand such points is a stroke nobody wanted.
            let moved = stroke
                .points
                .last()
                .is_none_or(|(lx, ly)| (px - lx).abs() >= 1.0 || (py - ly).abs() >= 1.0);
            if moved {
                stroke.points.push((px, py));
                self.needs_redraw = true;
            }
            return Some(Response::Redraw);
        }
        if self.inking.erasing {
            let whole = self.inking.tool == Some(InkTool::StrokeEraser);
            self.erase_at(x, y, !whole);
            return Some(Response::Redraw);
        }
        None
    }

    /// Ends the stroke, writing it into the document, or lifts the eraser.
    pub(super) fn ink_release(&mut self, x: i32, y: i32) -> bool {
        if self.inking.erasing {
            self.inking.erasing = false;
            return true;
        }
        let Some(mut stroke) = self.inking.stroke.take() else { return false };
        let (px, py) = (x as f32, y as f32);
        if stroke.points.last().is_none_or(|(lx, ly)| *lx != px || *ly != py) {
            stroke.points.push((px, py));
        }
        self.needs_redraw = true;
        // A tap is not a stroke: the pen went down and came up in the same
        // place, and Word draws a dot for that which this does not.
        if stroke.points.len() < 2 {
            return true;
        }
        let Some(InkTool::Draw(kind)) = self.inking.tool else { return true };
        let look = self.pen_look(kind);
        self.write_stroke(&stroke, kind, look);
        true
    }

    /// Where the stroke being drawn is, for drawing it over the page.
    #[must_use]
    pub(super) fn stroke_in_hand(&self) -> Option<StrokeToDraw<'_>> {
        let stroke = self.inking.stroke.as_ref()?;
        let InkTool::Draw(kind) = self.inking.tool? else { return None };
        let look = self.pen_look(kind);
        let mut colour = wp_raster::Color::from_hex(look.colour)?;
        if kind == PenKind::Highlighter {
            colour.alpha = 127;
        }
        let pixels = look.width_emu() as f32 / EMU_PER_POINT as f32 * self.pixels_per_inch() / 72.0;
        Some(StrokeToDraw { points: &stroke.points, colour, width: pixels.max(1.0) })
    }

    /// Writes a finished stroke into the document as ink that floats where
    /// it was drawn.
    fn write_stroke(&mut self, stroke: &StrokeInHand, kind: PenKind, look: PenLook) {
        let scale = self.pixels_per_inch() / 72.0;
        if scale <= 0.0 {
            return;
        }
        let (origin_x, origin_y) = self.page_origin(stroke.page);
        let page_top = self.content_top() + origin_y - self.scroll_down();
        let emu = |pixels: f32| (f64::from(pixels / scale) * EMU_PER_POINT as f64) as i64;

        let points: Vec<(i64, i64)> =
            stroke.points.iter().map(|(x, y)| (emu(x - origin_x), emu(y - page_top))).collect();
        let highlighter = kind == PenKind::Highlighter;
        let ink = Ink {
            strokes: vec![Stroke {
                colour: look.colour.to_owned(),
                width_emu: look.width_emu(),
                transparency: if highlighter { 128 } else { 0 },
                flat: highlighter,
                points,
                pressure: Vec::new(),
            }],
        };
        let Some((left, top, _, _)) = ink.bounds() else { return };

        // Across from the page's edge, and down from the paragraph the
        // stroke began beside — the paragraph whose band the top of the
        // stroke is in, which is what the stroke hangs from.
        let top_pixels = page_top + top as f32 / EMU_PER_POINT as f32 * scale;
        let Some((paragraph, first_line)) = self.paragraph_under(stroke.page, top_pixels) else {
            return;
        };
        let anchor = Anchor {
            wrap: Wrap::None,
            horizontal_from: Relative::Page,
            horizontal: Placement::Offset(left),
            vertical_from: Relative::Paragraph,
            vertical: Placement::Offset(top - emu(first_line - page_top)),
            allow_overlap: true,
            ..Anchor::default()
        };

        // The run goes at the start of that paragraph, and the caret goes
        // back to where it was: drawing is not typing.
        let caret = self.document.caret();
        self.document.set_caret(TextPosition::new(paragraph, 0));
        let written = self.document.insert_ink_floating(&ink, &anchor).unwrap_or(false);
        let back = if written && caret.paragraph == paragraph {
            TextPosition::new(caret.paragraph, caret.offset + 1)
        } else {
            caret
        };
        self.document.set_caret(back);
        if written {
            self.relayout();
            self.edited(true, "");
        }
    }

    // --- the erasers ----------------------------------------------------------------

    /// Rubs out at a point: the whole of the stroke under it, or the part of
    /// it within the eraser's reach.
    fn erase_at(&mut self, x: i32, y: i32, part: bool) {
        let Some(found) = self.ink_under(x, y) else { return };
        let InkUnder { at, mut ink, stroke, fit, box_x, box_y } = found;
        if !part {
            ink.strokes.remove(stroke);
        } else {
            let reach = ERASER_REACH / fit.scale.max(f32::EPSILON);
            let (ink_x, ink_y) = fit.from_box(x as f32 - box_x, y as f32 - box_y);
            let old = ink.strokes.remove(stroke);
            let pieces = rubbed_out(&old, (ink_x as f32, ink_y as f32), reach);
            for (offset, piece) in pieces.into_iter().enumerate() {
                ink.strokes.insert(stroke + offset, piece);
            }
        }
        let done = self.document.replace_ink_at(at, &ink);
        if done {
            // A stroke rubbed out was chosen, perhaps: it is not there to be.
            self.chosen_drawings.retain(|held| *held != at || self.document.ink_at(at).is_some());
            self.relayout();
            self.edited(true, "");
        }
    }

    /// The stroke under a point of the window, if the point is on one.
    fn ink_under(&self, x: i32, y: i32) -> Option<InkUnder> {
        let (px, py) = (x as f32, y as f32);
        for placed in self.placed_inks() {
            let PlacedInkOnScreen { at, left, top, width, height } = placed;
            if px < left - TOUCH
                || px > left + width + TOUCH
                || py < top - TOUCH
                || py > top + height + TOUCH
            {
                continue;
            }
            let Some(reference) = self.document.ink_at(at) else { continue };
            let Some(ink) = self.document.ink(&reference.relationship) else { continue };
            let Some(fit) = Fit::of(&ink, width, height) else { continue };
            for (index, stroke) in ink.strokes.iter().enumerate() {
                let half = (stroke.width_emu as f32 * fit.scale / 2.0).max(TOUCH);
                let near = stroke.points.windows(2).any(|pair| {
                    let from = fit.to_box(pair[0].0, pair[0].1);
                    let to = fit.to_box(pair[1].0, pair[1].1);
                    distance_to_segment((px - left, py - top), (from.x, from.y), (to.x, to.y))
                        <= half
                });
                if near {
                    return Some(InkUnder { at, ink, stroke: index, fit, box_x: left, box_y: top });
                }
            }
        }
        None
    }

    /// Every lot of ink on the pages, where it is on the screen.
    fn placed_inks(&self) -> Vec<PlacedInkOnScreen> {
        let mut out = Vec::new();
        for index in 0..self.pages.len() {
            let (origin_x, origin_y) = self.page_origin(index);
            let top = self.content_top() + origin_y - self.scroll_down();
            for ink in &self.pages[index].inks {
                let Some(at) = ink.at else { continue };
                out.push(PlacedInkOnScreen {
                    at,
                    left: origin_x + ink.x,
                    top: top + ink.y,
                    width: ink.width,
                    height: ink.height,
                });
            }
        }
        // The one drawn last is on top, so it is the one a press means.
        out.reverse();
        out
    }

    /// Which page a point of the window is on, if it is on one.
    fn page_index_under(&self, x: i32, y: i32) -> Option<usize> {
        let (px, py) = (x as f32, y as f32);
        (0..self.pages.len()).find(|index| {
            let (origin_x, origin_y) = self.page_origin(*index);
            let top = self.content_top() + origin_y - self.scroll_down();
            let page = &self.pages[*index];
            px >= origin_x && px < origin_x + page.width && py >= top && py < top + page.height
        })
    }

    // --- what the ink becomes ----------------------------------------------------------

    /// The ink chosen, as strokes in pixels of the window, with where each
    /// lot is in the document and what pen drew it.
    fn chosen_ink(&self) -> Vec<ChosenInk> {
        let placed = self.placed_inks();
        let mut out = Vec::new();
        for at in &self.chosen_drawings {
            let Some(on_screen) = placed.iter().find(|ink| ink.at == *at) else { continue };
            let Some(reference) = self.document.ink_at(*at) else { continue };
            let Some(ink) = self.document.ink(&reference.relationship) else { continue };
            let Some(fit) = Fit::of(&ink, on_screen.width, on_screen.height) else { continue };
            for stroke in &ink.strokes {
                let points: Vec<(f32, f32)> = stroke
                    .points
                    .iter()
                    .map(|(x, y)| {
                        let point = fit.to_box(*x, *y);
                        (on_screen.left + point.x, on_screen.top + point.y)
                    })
                    .collect();
                out.push(ChosenInk {
                    at: *at,
                    page: self.page_index_under(points[0].0 as i32, points[0].1 as i32),
                    colour: stroke.colour.clone(),
                    width_emu: stroke.width_emu,
                    points,
                });
            }
        }
        out
    }

    /// Turns the ink chosen into the shape it draws, if this program can see
    /// one in it.
    pub(super) fn ink_to_shape(&mut self) -> Response {
        let chosen = self.chosen_ink();
        if chosen.is_empty() {
            return self.report(t("Choose the ink first: Select, then drag round it"));
        }
        let all: Vec<(f32, f32)> =
            chosen.iter().flat_map(|ink| ink.points.iter().copied()).collect();
        let Some(seen) = recognise_shape(&all) else {
            return self.report(t("That ink is not a shape this program can see"));
        };
        let Some(page) = chosen[0].page else { return Response::Ignored };

        let scale = self.pixels_per_inch() / 72.0;
        let (origin_x, _) = self.page_origin(page);
        let emu = |pixels: f32| (f64::from(pixels / scale) * EMU_PER_POINT as f64) as i64;
        let (left, top, right, bottom) = bounds_of(&all);
        let Some((paragraph, first_line)) = self.paragraph_under(page, top) else {
            return Response::Ignored;
        };

        let shape = Shape {
            preset: seen.preset.to_owned(),
            width_emu: emu(right - left).max(EMU_PER_POINT),
            height_emu: emu(bottom - top).max(EMU_PER_POINT),
            fill: wp_docx::fills::Fill::None,
            outline: Some(Colour::rgb(&chosen[0].colour)),
            outline_emu: chosen[0].width_emu,
            flipped_down: seen.flipped_down,
            name: seen.name.to_owned(),
            anchor: Some(Anchor {
                wrap: Wrap::None,
                horizontal_from: Relative::Page,
                horizontal: Placement::Offset(emu(left - origin_x)),
                vertical_from: Relative::Paragraph,
                vertical: Placement::Offset(emu(top - first_line)),
                allow_overlap: true,
                ..Anchor::default()
            }),
            ..Shape::default()
        };

        self.document.begin_gesture();
        self.take_chosen_ink_out(&chosen);
        self.document.set_caret(TextPosition::new(paragraph, 0));
        let done = self.document.insert_shape(&shape);
        self.document.end_gesture();
        self.relayout();
        self.choose_drawing_here();
        self.edited(done, &format!("{}: {}", t("Ink to Shape"), t(seen.name)))
    }

    /// Turns the ink chosen into the letters it spells, as far as this
    /// program can read them.
    pub(super) fn ink_to_text(&mut self) -> Response {
        let chosen = self.chosen_ink();
        if chosen.is_empty() {
            return self.report(t("Choose the ink first: Select, then drag round it"));
        }
        let strokes: Vec<Vec<(f32, f32)>> = chosen.iter().map(|ink| ink.points.clone()).collect();
        let text = super::handwriting::read(&strokes);
        if text.is_empty() {
            return self.report(t("That ink is not writing this program can read"));
        }
        // The words go where the first stroke hung: at that paragraph, in
        // place of the ink.
        let first = chosen.iter().map(|ink| ink.at).min().unwrap_or(chosen[0].at);
        self.document.begin_gesture();
        self.take_chosen_ink_out(&chosen);
        self.document.set_caret(first);
        let done = self.document.type_text(&text);
        self.document.end_gesture();
        self.relayout();
        self.edited(done, &format!("{}: {text}", t("Ink to Text")))
    }

    /// Takes every lot of ink chosen out of the document, last first so
    /// that the places of the rest stay true.
    fn take_chosen_ink_out(&mut self, chosen: &[ChosenInk]) {
        let mut places: Vec<TextPosition> = chosen.iter().map(|ink| ink.at).collect();
        places.sort();
        places.dedup();
        for at in places.into_iter().rev() {
            self.document.remove_drawing_at(at);
        }
        self.chosen_drawings.clear();
    }
}

/// The stroke being drawn, as the window draws it: its points in pixels,
/// its colour, and how wide it is in pixels.
pub(super) struct StrokeToDraw<'a> {
    pub points: &'a [(f32, f32)],
    pub colour: wp_raster::Color,
    pub width: f32,
}

/// Ink on the screen: where a lot of it is, and where it is in the text.
struct PlacedInkOnScreen {
    at: TextPosition,
    left: f32,
    top: f32,
    width: f32,
    height: f32,
}

/// A stroke found under the pointer: the ink it is in, which of its strokes,
/// and how the ink is fitted into its box on the screen.
struct InkUnder {
    at: TextPosition,
    ink: Ink,
    stroke: usize,
    fit: Fit,
    box_x: f32,
    box_y: f32,
}

/// One stroke of the ink chosen, in pixels of the window.
struct ChosenInk {
    at: TextPosition,
    page: Option<usize>,
    colour: String,
    width_emu: i64,
    points: Vec<(f32, f32)>,
}

/// A stroke with everything within the eraser's reach of a point taken out:
/// the pieces that are left, each with the pressure that was on it.
fn rubbed_out(stroke: &Stroke, at: (f32, f32), reach: f32) -> Vec<Stroke> {
    let mut pieces = Vec::new();
    let mut points = Vec::new();
    let mut pressure = Vec::new();
    let mut finish = |points: &mut Vec<(i64, i64)>, pressure: &mut Vec<f32>| {
        if points.len() >= 2 {
            pieces.push(Stroke {
                colour: stroke.colour.clone(),
                width_emu: stroke.width_emu,
                transparency: stroke.transparency,
                flat: stroke.flat,
                points: std::mem::take(points),
                pressure: std::mem::take(pressure),
            });
        } else {
            points.clear();
            pressure.clear();
        }
    };
    for (index, (x, y)) in stroke.points.iter().enumerate() {
        let far = (*x as f32 - at.0).hypot(*y as f32 - at.1);
        if far <= reach {
            finish(&mut points, &mut pressure);
            continue;
        }
        points.push((*x, *y));
        if !stroke.pressure.is_empty() {
            pressure.push(stroke.pressure_at(index));
        }
    }
    finish(&mut points, &mut pressure);
    pieces
}

/// How far a point is from a straight piece of a stroke.
fn distance_to_segment(point: (f32, f32), from: (f32, f32), to: (f32, f32)) -> f32 {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let length = dx * dx + dy * dy;
    let along = if length <= 0.0 {
        0.0
    } else {
        (((point.0 - from.0) * dx + (point.1 - from.1) * dy) / length).clamp(0.0, 1.0)
    };
    let (nx, ny) = (from.0 + dx * along, from.1 + dy * along);
    (point.0 - nx).hypot(point.1 - ny)
}

/// The rectangle round some points: left, top, right, bottom.
fn bounds_of(points: &[(f32, f32)]) -> (f32, f32, f32, f32) {
    points.iter().fold((f32::MAX, f32::MAX, f32::MIN, f32::MIN), |(l, t, r, b), (x, y)| {
        (l.min(*x), t.min(*y), r.max(*x), b.max(*y))
    })
}

/// A thickness in hundredths of a millimetre, written for a person.
fn mm(width: u16) -> String {
    let whole = width / 100;
    let rest = width % 100;
    if rest == 0 {
        whole.to_string()
    } else if rest % 10 == 0 {
        format!("{whole}.{}", rest / 10)
    } else {
        format!("{whole}.{rest:02}")
    }
}

/// What the shapes seen in ink are called, for the note that says which
/// was seen.
pub(crate) const SEEN_SHAPES: &[&str] =
    &["Line", "Oval", "Triangle", "Diamond", "Rectangle", "Pentagon", "Hexagon"];

/// A shape seen in some ink.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SeenShape {
    pub preset: &'static str,
    pub name: &'static str,
    /// A line drawn upwards is the line preset turned over.
    pub flipped_down: bool,
}

/// The shape some strokes draw, if they draw one this program can see.
///
/// A stroke that ends far from where it began is a line when it never
/// strays far from the straight one between its ends. A stroke that comes
/// back to where it began is an ellipse when its points sit on the ellipse
/// in its box, and otherwise whatever the corners left after the wobble is
/// taken out say: three is a triangle, four a rectangle — or a diamond when
/// the corners are at the middles of the box's sides — five a pentagon and
/// six a hexagon.
#[must_use]
pub(crate) fn recognise_shape(points: &[(f32, f32)]) -> Option<SeenShape> {
    if points.len() < 3 {
        return None;
    }
    let (left, top, right, bottom) = bounds_of(points);
    let (width, height) = (right - left, bottom - top);
    let size = width.max(height);
    if size <= 0.0 {
        return None;
    }
    let (first, last) = (points[0], points[points.len() - 1]);
    let closes = (first.0 - last.0).hypot(first.1 - last.1) < size * 0.25;

    if !closes {
        // How far the stroke strays from the straight line between its ends,
        // as a share of that line's length.
        let chord = (last.0 - first.0).hypot(last.1 - first.1);
        let strays = points
            .iter()
            .map(|point| distance_to_segment(*point, first, last))
            .fold(0.0f32, f32::max);
        if chord > 0.0 && strays / chord < 0.08 {
            let rising = (last.1 < first.1) != (last.0 < first.0);
            return Some(SeenShape { preset: "line", name: "Line", flipped_down: rising });
        }
        return None;
    }

    // Round: every point about as far from the middle as the ellipse in the
    // box is, which is nought when it is on it and one at the middle.
    let (cx, cy) = ((left + right) / 2.0, (top + bottom) / 2.0);
    let (rx, ry) = ((width / 2.0).max(1.0), (height / 2.0).max(1.0));
    let off = points
        .iter()
        .map(|(x, y)| {
            let (u, v) = ((x - cx) / rx, (y - cy) / ry);
            ((u * u + v * v).sqrt() - 1.0).abs()
        })
        .sum::<f32>()
        / points.len() as f32;
    // Under six hundredths: a hexagon's points sit nine hundredths off the
    // ellipse in its box on average, a pentagon's thirteen, and a hand-drawn
    // oval is closer than either.
    if off < 0.06 {
        return Some(SeenShape { preset: "ellipse", name: "Oval", flipped_down: false });
    }

    let corners = simplified(points, size * 0.08);
    // The first and last of a closed stroke are the same corner.
    let count = corners.len().saturating_sub(1);
    Some(match count {
        3 => SeenShape { preset: "triangle", name: "Triangle", flipped_down: false },
        4 => {
            // Corners at the middles of the sides rather than at the corners
            // of the box make a diamond.
            let at_middles = corners.iter().take(4).all(|(x, y)| {
                let across = ((x - cx).abs() / rx).min(1.0);
                let down = ((y - cy).abs() / ry).min(1.0);
                (across < 0.35 && down > 0.65) || (down < 0.35 && across > 0.65)
            });
            if at_middles {
                SeenShape { preset: "diamond", name: "Diamond", flipped_down: false }
            } else {
                SeenShape { preset: "rect", name: "Rectangle", flipped_down: false }
            }
        }
        5 => SeenShape { preset: "pentagon", name: "Pentagon", flipped_down: false },
        6 => SeenShape { preset: "hexagon", name: "Hexagon", flipped_down: false },
        _ => return None,
    })
}

/// The points that matter of a stroke: the ends and every turn that strays
/// further than the tolerance from the line between its neighbours, which
/// is the Douglas–Peucker simplification.
fn simplified(points: &[(f32, f32)], tolerance: f32) -> Vec<(f32, f32)> {
    fn keep(points: &[(f32, f32)], tolerance: f32, out: &mut Vec<(f32, f32)>) {
        if points.len() < 3 {
            out.extend(points.iter().skip(1));
            return;
        }
        let (first, last) = (points[0], points[points.len() - 1]);
        let (index, strays) = points
            .iter()
            .enumerate()
            .skip(1)
            .take(points.len() - 2)
            .map(|(index, point)| (index, distance_to_segment(*point, first, last)))
            .fold((0, 0.0f32), |best, next| if next.1 > best.1 { next } else { best });
        if strays > tolerance {
            keep(&points[..=index], tolerance, out);
            keep(&points[index..], tolerance, out);
        } else {
            out.push(last);
        }
    }
    let mut out = vec![points[0]];
    keep(points, tolerance, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::Document;
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event, Modifiers};

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    fn editor() -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("Some words on the page")));
        body.blocks.push(Block::Paragraph(Paragraph::text("And a second paragraph")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor
    }

    /// A point of the window that many pixels in from the first page's corner.
    fn on_page(editor: &Editor, across: f32, down: f32) -> (i32, i32) {
        let (origin_x, origin_y) = editor.page_origin(0);
        let top = editor.content_top() + origin_y - editor.scroll_down();
        ((origin_x + across) as i32, (top + down) as i32)
    }

    /// The pointer pressed at the first point, dragged through the rest and
    /// let go at the last.
    fn drag(editor: &mut Editor, points: &[(f32, f32)]) {
        let held: Vec<(i32, i32)> = points.iter().map(|(x, y)| on_page(editor, *x, *y)).collect();
        let (x, y) = held[0];
        editor.handle(Event::MouseDown { x, y, modifiers: Modifiers::default() });
        for (x, y) in &held[1..] {
            editor.handle(Event::MouseMove {
                x: *x,
                y: *y,
                held: true,
                modifiers: Modifiers::default(),
            });
        }
        let (x, y) = held[held.len() - 1];
        editor.handle(Event::MouseUp { x, y });
        // The first movement after the button comes up is what lets a band go.
        editor.handle(Event::MouseMove { x, y, held: false, modifiers: Modifiers::default() });
    }

    fn inks(editor: &Editor) -> Vec<wp_layout::PlacedInk> {
        editor.pages.iter().flat_map(|page| page.inks.iter().cloned()).collect()
    }

    #[test]
    fn a_stroke_drawn_with_the_pen_becomes_ink_that_floats_where_it_was_drawn() {
        let mut editor = editor();
        let caret = editor.document.caret();
        editor.run(Command::DrawPen);
        assert_eq!(editor.ink_tool(), Some(InkTool::Draw(PenKind::Pen)));
        drag(&mut editor, &[(120.0, 150.0), (160.0, 160.0), (200.0, 150.0), (240.0, 170.0)]);

        let placed = inks(&editor);
        assert_eq!(placed.len(), 1, "the stroke did not become ink");
        let at = placed[0].at.expect("a place in the text");
        let reference = editor.document.ink_at(at).expect("ink in the document");
        let anchor = reference.anchor.expect("the ink floats");
        assert_eq!(anchor.horizontal_from, Relative::Page);
        assert_eq!(anchor.vertical_from, Relative::Paragraph);
        // Drawn back where the pen went: the box starts where the stroke
        // started, less half the pen's width.
        let (origin_x, _) = editor.page_origin(0);
        assert!(
            (placed[0].x + origin_x - (on_page(&editor, 120.0, 0.0).0 as f32)).abs() < 3.0,
            "{}",
            placed[0].x
        );
        let ink = editor.document.ink(&reference.relationship).expect("the strokes");
        assert_eq!(ink.strokes.len(), 1);
        assert_eq!(ink.strokes[0].colour, "000000");
        assert_eq!(ink.strokes[0].width_emu, 18_000, "half a millimetre");
        // Drawing is not typing: the caret stayed where it was.
        assert_eq!(editor.document.caret(), caret);
        // And it is one undo.
        assert!(editor.document.undo());
        editor.relayout();
        assert!(inks(&editor).is_empty());
    }

    #[test]
    fn the_highlighter_is_wide_and_see_through_and_the_look_can_be_changed() {
        let mut editor = editor();
        editor.run(Command::DrawHighlighter);
        // The first entry of its list is its thinnest, the last its last colour.
        editor.choose_pen_look(PenKind::Highlighter, 0);
        assert_eq!(editor.pen_look(PenKind::Highlighter).width, 200);
        let colours =
            PenKind::Highlighter.widths().len() + PenKind::Highlighter.colours().len() - 1;
        editor.choose_pen_look(PenKind::Highlighter, colours);
        assert_eq!(editor.pen_look(PenKind::Highlighter).colour, "FFC000");
        drag(&mut editor, &[(120.0, 150.0), (300.0, 150.0)]);
        let placed = inks(&editor);
        let reference = editor.document.ink_at(placed[0].at.unwrap()).expect("ink");
        let ink = editor.document.ink(&reference.relationship).expect("the strokes");
        assert_eq!(ink.strokes[0].transparency, 128);
        assert!(ink.strokes[0].flat);
        assert_eq!(ink.strokes[0].width_emu, 72_000, "two millimetres");
        assert_eq!(ink.strokes[0].colour, "FFC000");
    }

    #[test]
    fn the_stroke_eraser_takes_a_stroke_away_and_the_point_eraser_leaves_the_rest() {
        let mut editor = editor();
        editor.run(Command::DrawPen);
        drag(
            &mut editor,
            &[(100.0, 150.0), (150.0, 150.0), (200.0, 150.0), (250.0, 150.0), (300.0, 150.0)],
        );
        assert_eq!(inks(&editor).len(), 1);

        editor.run(Command::DrawEraser);
        assert_eq!(editor.ink_tool(), Some(InkTool::StrokeEraser));
        drag(&mut editor, &[(200.0, 150.0), (201.0, 150.0)]);
        assert!(inks(&editor).is_empty(), "the stroke was not rubbed out");
        assert!(editor.document.undo());
        editor.relayout();
        assert_eq!(inks(&editor).len(), 1);

        editor.choose_eraser(1);
        assert_eq!(editor.ink_tool(), Some(InkTool::PointEraser));
        drag(&mut editor, &[(200.0, 150.0), (201.0, 150.0)]);
        let placed = inks(&editor);
        assert_eq!(placed.len(), 1, "the drawing went with the piece");
        let reference = editor.document.ink_at(placed[0].at.unwrap()).expect("ink");
        let ink = editor.document.ink(&reference.relationship).expect("the strokes");
        assert_eq!(ink.strokes.len(), 2, "the stroke was not cut in two");
        // Rubbed out far from any stroke, nothing happens.
        drag(&mut editor, &[(100.0, 400.0), (101.0, 400.0)]);
        let ink = editor.document.ink(&reference.relationship).expect("the strokes");
        assert_eq!(ink.strokes.len(), 2);
    }

    #[test]
    fn escape_puts_the_pen_down_and_select_puts_it_down_too() {
        let mut editor = editor();
        editor.run(Command::DrawPencil);
        assert!(editor.inking());
        editor
            .handle(Event::KeyDown { key: wp_shell::Key::Escape, modifiers: Modifiers::default() });
        assert!(!editor.inking());
        editor.run(Command::DrawPen);
        editor.run(Command::DrawSelect);
        assert!(!editor.inking());
        assert!(editor.choosing_drawings());
        editor.run(Command::DrawPen);
        assert!(!editor.choosing_drawings(), "a pen and Select cannot both take the press");
    }

    #[test]
    fn ink_to_shape_turns_a_drawn_box_into_a_rectangle() {
        let mut editor = editor();
        editor.run(Command::DrawPen);
        let mut box_path = Vec::new();
        for step in 0..=10 {
            box_path.push((120.0 + step as f32 * 10.0, 150.0));
        }
        for step in 0..=6 {
            box_path.push((220.0, 150.0 + step as f32 * 10.0));
        }
        for step in 0..=10 {
            box_path.push((220.0 - step as f32 * 10.0, 210.0));
        }
        for step in 0..=6 {
            box_path.push((120.0, 210.0 - step as f32 * 10.0));
        }
        drag(&mut editor, &box_path);
        assert_eq!(inks(&editor).len(), 1);

        // Gathered up with the band, then converted.
        editor.run(Command::DrawSelect);
        drag(&mut editor, &[(100.0, 130.0), (250.0, 240.0)]);
        assert_eq!(editor.chosen_drawings.len(), 1, "the band did not take the ink");
        editor.run(Command::InkToShape);
        assert!(inks(&editor).is_empty(), "the ink is still there");
        let shapes = editor.document.shapes();
        assert_eq!(shapes.len(), 1);
        assert_eq!(shapes[0].preset, "rect");
        assert_eq!(shapes[0].outline, Some(Colour::rgb("000000")));
        assert!(shapes[0].anchor.is_some(), "the shape does not float where the ink was");
        // One undo brings the ink back and takes the shape away.
        assert!(editor.document.undo());
        editor.relayout();
        assert_eq!(inks(&editor).len(), 1);
        assert!(editor.document.shapes().is_empty());
    }

    #[test]
    fn ink_to_text_reads_the_capitals_drawn_and_puts_the_word_in() {
        let mut editor = editor();
        editor.run(Command::DrawPen);
        // H in three strokes, then I in one, a little apart.
        drag(&mut editor, &[(100.0, 150.0), (100.0, 170.0), (100.0, 190.0)]);
        drag(&mut editor, &[(130.0, 150.0), (130.0, 170.0), (130.0, 190.0)]);
        drag(&mut editor, &[(100.0, 170.0), (115.0, 170.0), (130.0, 170.0)]);
        drag(&mut editor, &[(145.0, 150.0), (145.0, 170.0), (145.0, 190.0)]);
        assert_eq!(inks(&editor).len(), 4);

        editor.run(Command::DrawSelect);
        drag(&mut editor, &[(90.0, 140.0), (170.0, 200.0)]);
        assert_eq!(editor.chosen_drawings.len(), 4);
        editor.run(Command::InkToText);
        assert!(inks(&editor).is_empty());
        let text = editor.document.paragraph_text(0).unwrap_or_default()
            + &editor.document.paragraph_text(1).unwrap_or_default();
        assert!(text.contains("HI"), "read as {text:?}");
    }

    fn ring(count: usize, rx: f32, ry: f32) -> Vec<(f32, f32)> {
        (0..=count)
            .map(|step| {
                let angle = step as f32 / count as f32 * core::f32::consts::TAU;
                (100.0 + rx * angle.cos(), 100.0 + ry * angle.sin())
            })
            .collect()
    }

    fn polygon(corners: &[(f32, f32)]) -> Vec<(f32, f32)> {
        let mut out = Vec::new();
        for pair in corners.iter().chain(corners.first()).collect::<Vec<_>>().windows(2) {
            for step in 0..10 {
                let along = step as f32 / 10.0;
                out.push((
                    pair[0].0 + (pair[1].0 - pair[0].0) * along,
                    pair[0].1 + (pair[1].1 - pair[0].1) * along,
                ));
            }
        }
        out.push(corners[0]);
        out
    }

    #[test]
    fn a_stroke_that_goes_straight_is_a_line_and_one_that_wanders_is_not() {
        let line: Vec<(f32, f32)> =
            (0..20).map(|i| (i as f32 * 5.0, 100.0 - i as f32 * 2.0)).collect();
        let seen = recognise_shape(&line).expect("a line");
        assert_eq!(seen.preset, "line");
        assert!(seen.flipped_down, "a line drawn upwards is the preset turned over");
        let wander: Vec<(f32, f32)> =
            (0..40).map(|i| (i as f32 * 5.0, 100.0 + (i as f32).sin() * 40.0)).collect();
        assert_eq!(recognise_shape(&wander), None);
    }

    #[test]
    fn the_closed_shapes_are_told_apart_by_their_corners() {
        assert_eq!(recognise_shape(&ring(40, 50.0, 30.0)).map(|s| s.preset), Some("ellipse"));
        let square = polygon(&[(0.0, 0.0), (100.0, 0.0), (100.0, 60.0), (0.0, 60.0)]);
        assert_eq!(recognise_shape(&square).map(|s| s.preset), Some("rect"));
        let triangle = polygon(&[(50.0, 0.0), (100.0, 80.0), (0.0, 80.0)]);
        assert_eq!(recognise_shape(&triangle).map(|s| s.preset), Some("triangle"));
        let diamond = polygon(&[(50.0, 0.0), (100.0, 50.0), (50.0, 100.0), (0.0, 50.0)]);
        assert_eq!(recognise_shape(&diamond).map(|s| s.preset), Some("diamond"));
        let pentagon =
            polygon(&[(50.0, 0.0), (100.0, 38.0), (81.0, 100.0), (19.0, 100.0), (0.0, 38.0)]);
        assert_eq!(recognise_shape(&pentagon).map(|s| s.preset), Some("pentagon"));
    }

    #[test]
    fn the_point_eraser_leaves_the_pieces_either_side_of_where_it_rubbed() {
        let stroke = Stroke {
            colour: "000000".to_owned(),
            width_emu: 9_000,
            transparency: 0,
            flat: false,
            points: (0..10).map(|i| (i * 1000, 0)).collect(),
            pressure: (0..10).map(|i| i as f32 / 10.0).collect(),
        };
        let pieces = rubbed_out(&stroke, (4500.0, 0.0), 800.0);
        assert_eq!(pieces.len(), 2);
        assert_eq!(pieces[0].points, vec![(0, 0), (1000, 0), (2000, 0), (3000, 0)]);
        assert_eq!(pieces[0].pressure.len(), 4);
        assert_eq!(pieces[1].points[0], (6000, 0));
        // Rubbed out at the end, one piece is left; rubbed out entirely, none.
        assert_eq!(rubbed_out(&stroke, (9000.0, 0.0), 1500.0).len(), 1);
        assert!(rubbed_out(&stroke, (4500.0, 0.0), 10_000.0).is_empty());
    }

    #[test]
    fn a_thickness_is_written_in_millimetres_for_a_person() {
        assert_eq!(mm(25), "0.25");
        assert_eq!(mm(50), "0.5");
        assert_eq!(mm(100), "1");
        assert_eq!(mm(350), "3.5");
    }
}
