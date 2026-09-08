//! The strip along the bottom, and what it says.
//!
//! Word puts four things there and so does this: where you are in the document,
//! how much of it there is, what language it is in, and how far it is zoomed.
//! The first two are what a person glances down at while writing; the zoom is
//! the one control that belongs at the bottom rather than in the ribbon,
//! because it is about looking rather than about the document.

use wp_layout::{LayoutEngine, Renderer};
use wp_raster::{Canvas, Color};

use super::theme::Theme;
use super::Command;

/// Height of the strip.
pub const STATUS_HEIGHT: f32 = 24.0;

/// The smallest and largest zoom the slider reaches.
pub const MIN_ZOOM: f32 = 25.0;
pub const MAX_ZOOM: f32 = 400.0;

/// What the strip has to say.
#[derive(Clone, Debug)]
pub struct StatusState {
    /// What the last command did, shown at the left.
    pub note: String,
    pub page: usize,
    pub pages: usize,
    /// Which section the caret is in, counted from one.
    pub section: usize,
    pub words: usize,
    pub characters: usize,
    pub language: String,
    pub zoom: f32,
    pub modified: bool,
    pub selected_characters: usize,
    /// Which of those the person has asked to see.
    pub shows: Shows,
}

/// Which parts of the strip are showing.
///
/// Word lets a person choose, by right-clicking the strip and ticking things
/// off a list, and it remembers the choice. The defaults here are Word's: the
/// page, the word count, the language and the zoom, with the section and the
/// character count off until they are asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shows {
    pub page: bool,
    pub section: bool,
    pub words: bool,
    pub characters: bool,
    pub language: bool,
    pub zoom: bool,
    pub slider: bool,
}

impl Default for Shows {
    fn default() -> Self {
        Self {
            page: true,
            section: false,
            words: true,
            characters: false,
            language: true,
            zoom: true,
            slider: true,
        }
    }
}

/// One part of the strip: what it is called, and how to reach its switch.
type Part = (&'static str, fn(&mut Shows) -> &mut bool);

/// The parts, in the order the menu lists them, with the name Word gives each.
pub const PARTS: &[Part] = &[
    ("Page Number", |shows| &mut shows.page),
    ("Section", |shows| &mut shows.section),
    ("Word Count", |shows| &mut shows.words),
    ("Character Count", |shows| &mut shows.characters),
    ("Language", |shows| &mut shows.language),
    ("Zoom", |shows| &mut shows.zoom),
    ("Zoom Slider", |shows| &mut shows.slider),
];

impl Shows {
    /// Whether the part at a place in [`PARTS`] is showing.
    #[must_use]
    pub fn at(&self, index: usize) -> bool {
        let mut copy = *self;
        PARTS.get(index).is_some_and(|(_, reach)| *reach(&mut copy))
    }

    /// Turns one of them on or off.
    pub fn toggle(&mut self, index: usize) {
        if let Some((_, reach)) = PARTS.get(index) {
            let flag = reach(self);
            *flag = !*flag;
        }
    }

    /// The names of the parts that are switched off, for remembering.
    #[must_use]
    pub fn switched_off(&self) -> Vec<String> {
        PARTS
            .iter()
            .enumerate()
            .filter(|(index, _)| !self.at(*index))
            .map(|(_, (name, _))| (*name).to_owned())
            .collect()
    }

    /// The same read back.
    #[must_use]
    pub fn with_switched_off(names: &[String]) -> Self {
        let mut shows = Self::default();
        for (index, (name, _)) in PARTS.iter().enumerate() {
            let off = names.iter().any(|found| found.eq_ignore_ascii_case(name));
            if off == shows.at(index) {
                shows.toggle(index);
            }
        }
        shows
    }
}

/// Where the zoom slider is drawn, so a click can find it.
#[derive(Clone, Copy, Debug)]
pub struct SliderRect {
    pub left: f32,
    pub width: f32,
    pub top: f32,
    pub height: f32,
}

impl SliderRect {
    /// The zoom a point along the slider stands for.
    #[must_use]
    pub fn zoom_at(&self, x: i32) -> f32 {
        let along = ((x as f32 - self.left) / self.width).clamp(0.0, 1.0);
        // Geometric rather than linear: the useful zooms cluster near a hundred
        // per cent, and a linear slider spends half its length above two.
        let range = (MAX_ZOOM / MIN_ZOOM).ln();
        MIN_ZOOM * (along * range).exp()
    }

    /// Where along the slider a zoom sits.
    #[must_use]
    fn position_of(&self, zoom: f32) -> f32 {
        let range = (MAX_ZOOM / MIN_ZOOM).ln();
        let along = ((zoom.clamp(MIN_ZOOM, MAX_ZOOM) / MIN_ZOOM).ln() / range).clamp(0.0, 1.0);
        self.left + along * self.width
    }

    #[must_use]
    pub fn covers(&self, x: i32, y: i32) -> bool {
        let (x, y) = (x as f32, y as f32);
        x >= self.left - 6.0
            && x <= self.left + self.width + 6.0
            && y >= self.top
            && y <= self.top + self.height
    }
}

/// Draws the strip, returning where the zoom slider ended up.
pub fn draw(
    canvas: &mut Canvas,
    engine: &mut LayoutEngine<'_>,
    renderer: &mut Renderer<'_>,
    state: &StatusState,
    theme: &Theme,
) -> (Option<SliderRect>, [(Command, f32, f32); 2]) {
    // The strip is dark in both themes, the way Word keeps it, so its text is
    // the light sort in both.
    let text = theme.bar_text();
    let dim = theme.bar_dim_text();
    let track = theme.field_edge;
    let width = canvas.width() as f32;
    let top = canvas.height() as f32 - STATUS_HEIGHT;
    canvas.fill_rect(0, top as i32, width as i32, STATUS_HEIGHT as i32, theme.status);

    let baseline = top + 16.0;

    // The left side: where you are, and how much there is.
    let mut left = 10.0f32;
    let write = |canvas: &mut Canvas,
                 engine: &mut LayoutEngine<'_>,
                 renderer: &mut Renderer<'_>,
                 text: &str,
                 color: Color,
                 left: &mut f32| {
        let line = engine.simple_line(text, *left, baseline, 8.0, color);
        let measured = line.width - *left;
        renderer.draw_onto(canvas, &line, 0.0, 0.0);
        *left += measured + 16.0;
    };

    if state.shows.page {
        let position = format!("Page {} of {}", state.page, state.pages);
        write(canvas, engine, renderer, &position, text, &mut left);
    }
    if state.shows.section {
        let section = format!("Section: {}", state.section);
        write(canvas, engine, renderer, &section, text, &mut left);
    }

    if state.shows.words {
        let words =
            if state.words == 1 { "1 word".to_owned() } else { format!("{} words", state.words) };
        write(canvas, engine, renderer, &words, text, &mut left);
    }
    if state.shows.characters {
        let characters = format!("{} characters", state.characters);
        write(canvas, engine, renderer, &characters, dim, &mut left);
    }

    if state.selected_characters > 0 {
        let selected = format!("{} selected", state.selected_characters);
        write(canvas, engine, renderer, &selected, dim, &mut left);
    }
    if state.shows.language {
        write(canvas, engine, renderer, &state.language, dim, &mut left);
    }

    if state.modified {
        write(canvas, engine, renderer, "unsaved changes", dim, &mut left);
    }
    if !state.note.is_empty() {
        write(canvas, engine, renderer, &state.note, dim, &mut left);
    }

    // The right side: the zoom, with a slider and the buttons either end.
    // Either of them can be switched off, and what is left keeps its place.
    let percent = format!("{}%", state.zoom.round() as i32);
    let measured = engine.simple_line(&percent, 0.0, 0.0, 8.0, text).width;
    let percent_x = width - measured - 12.0;
    if state.shows.zoom {
        let line = engine.simple_line(&percent, percent_x, baseline, 8.0, text);
        renderer.draw_onto(canvas, &line, 0.0, 0.0);
    }

    let slider = SliderRect {
        left: percent_x - 108.0,
        width: 90.0,
        top: top + 4.0,
        height: STATUS_HEIGHT - 8.0,
    };
    let out_x = slider.left - 18.0;
    let in_x = slider.left + slider.width + 8.0;

    if !state.shows.slider {
        // Nothing drawn and nothing to press: the two buttons are put where no
        // click can reach them rather than left lying about the strip.
        return (None, [(Command::ZoomOut, -1000.0, top), (Command::ZoomIn, -1000.0, top)]);
    }

    canvas.fill_rect(
        slider.left as i32,
        (slider.top + slider.height / 2.0) as i32,
        slider.width as i32,
        1,
        track,
    );
    // The mark at a hundred per cent, which is where the eye goes back to.
    canvas.fill_rect(
        slider.position_of(100.0) as i32,
        (slider.top + 3.0) as i32,
        1,
        (slider.height - 6.0) as i32,
        track,
    );
    let knob = slider.position_of(state.zoom);
    canvas.fill_rect(
        (knob - 2.0) as i32,
        (slider.top + 2.0) as i32,
        5,
        (slider.height - 4.0) as i32,
        theme.bar_text(),
    );

    // A minus and a plus either side of it.
    canvas.fill_rect(out_x as i32, (top + 11.0) as i32, 9, 2, text);
    canvas.fill_rect(in_x as i32, (top + 11.0) as i32, 9, 2, text);
    canvas.fill_rect((in_x + 3.5) as i32, (top + 7.5) as i32, 2, 9, text);

    (Some(slider), [(Command::ZoomOut, out_x - 3.0, top), (Command::ZoomIn, in_x - 3.0, top)])
}
