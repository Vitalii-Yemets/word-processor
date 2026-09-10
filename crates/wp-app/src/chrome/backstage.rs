//! Word's backstage: what the File tab opens onto.
//!
//! # Why a window rather than a menu
//!
//! Word replaced the File menu with this in 2010, and the reason holds: the
//! things under File are not commands to be picked off a list but places to
//! look at — what this document is, which ones were open lately, where this one
//! would go if it were saved. A menu shows a name; a page shows the answer.
//!
//! # The shape
//!
//! A coloured rail down the left with the places on it, the chosen one filling
//! the rest, and an arrow at the top that goes back to the document. The Print
//! page has been this shape since it was built; this is the rail around it, and
//! Print on that rail opens the page that already exists rather than a second
//! copy of it. See [`super::printpane`].
//!
//! # What is drawn here and what is decided elsewhere
//!
//! Nothing here knows what a document is. The editor works out what the chosen
//! place has to say, hands it over as [`Contents`], and is told back which line
//! was pressed — the same division as every other pane in the program.

use wp_layout::{LayoutEngine, Renderer};
use wp_raster::Canvas;

use super::icons::{self, Icon};
use super::theme::Theme;

/// How wide the rail down the left is.
pub const RAIL_WIDTH: f32 = 190.0;

/// The height of one place on the rail.
const RAIL_ROW: f32 = 32.0;

/// The height of one line of the page on the right.
const ROW_HEIGHT: f32 = 46.0;

/// The room round everything.
const PADDING: f32 = 24.0;

/// How far one notch of the wheel moves the page.
const SCROLL_PER_NOTCH: f32 = ROW_HEIGHT;

/// One of the places the rail lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// What this document is: what it says about itself, and how much of it
    /// there is.
    Info,
    New,
    Open,
    Save,
    SaveAs,
    Print,
    /// Word's Export: a copy in another format, which here means a PDF.
    Export,
    Close,
    Options,
}

impl Place {
    /// Every one, in Word's order.
    pub const ALL: &'static [Self] = &[
        Self::Info,
        Self::New,
        Self::Open,
        Self::Save,
        Self::SaveAs,
        Self::Print,
        Self::Export,
        Self::Close,
        Self::Options,
    ];

    /// What the rail calls it.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Info => "Info",
            Self::New => "New",
            Self::Open => "Open",
            Self::Save => "Save",
            Self::SaveAs => "Save As",
            Self::Print => "Print",
            Self::Export => "Export",
            Self::Close => "Close",
            Self::Options => "Options",
        }
    }

    /// Whether choosing it shows a page, or simply does the thing and leaves.
    ///
    /// Word draws a page for Info, New, Open, Save As and Export; Save, Print,
    /// Close and Options do their work and the backstage goes away. A page that
    /// said "Save" and had a Save button on it would be one press too many, and
    /// Word does not draw one either.
    #[must_use]
    pub fn has_a_page(self) -> bool {
        matches!(self, Self::Info | Self::New | Self::Open | Self::SaveAs | Self::Export)
    }
}

/// A line of a page: something to press, with a line under it saying what it is.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Row {
    pub title: String,
    /// Under the title: where it lives, or what pressing it would do.
    pub note: String,
}

impl Row {
    #[must_use]
    pub fn new(title: impl Into<String>, note: impl Into<String>) -> Self {
        Self { title: title.into(), note: note.into() }
    }
}

/// What the editor gives the backstage to show.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Contents {
    /// The heading of the page that is showing.
    pub heading: String,
    /// Lines that are told rather than pressed: a label and its value.
    pub facts: Vec<(String, String)>,
    /// The heading over the rows, when they want one.
    pub rows_heading: String,
    /// Which row the heading goes above.
    ///
    /// Word's Open page has Browse at the top and *then* the heading "Recent"
    /// over the documents themselves, because Browse is not one of them. The
    /// rows before the heading stay put; the ones after it are the list, and
    /// the list is what winds.
    pub rows_heading_at: usize,
    /// Lines that can be pressed.
    pub rows: Vec<Row>,
    /// What to say when there are no rows at all.
    pub nothing: String,
}

/// Where a press on the backstage lands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    /// The arrow that goes back to the document.
    Back,
    /// One of the places on the rail.
    Place(Place),
    /// One of the lines of the page that is showing.
    Row(usize),
}

/// The backstage, and what it remembers between one drawing and the next.
#[derive(Clone, Debug)]
pub struct Backstage {
    /// Which place is showing. Word opens on Info.
    pub place: Place,
    /// How far down the lines of the page have been wound.
    scroll: f32,
    /// How much of the page did not fit, worked out as it was drawn.
    overflow: f32,
    hovered: Option<Hit>,
    placed: Vec<(Hit, f32, f32, f32, f32)>,
}

impl Default for Backstage {
    fn default() -> Self {
        Self::new()
    }
}

impl Backstage {
    #[must_use]
    pub fn new() -> Self {
        Self { place: Place::Info, scroll: 0.0, overflow: 0.0, hovered: None, placed: Vec::new() }
    }

    /// Goes to a place, starting its page at the top.
    ///
    /// The scroll belongs to the page and not to the pane: arriving at Open
    /// half way down the list because Info was long is nobody's idea of what
    /// should happen.
    pub fn show(&mut self, place: Place) {
        self.place = place;
        self.scroll = 0.0;
    }

    /// What is under a point, if anything.
    #[must_use]
    pub fn at(&self, x: i32, y: i32) -> Option<Hit> {
        let (x, y) = (x as f32, y as f32);
        self.placed
            .iter()
            .find(|(_, left, top, width, height)| {
                x >= *left && x < left + width && y >= *top && y < top + height
            })
            .map(|(hit, ..)| *hit)
    }

    /// Follows the pointer. True when something has to be drawn again.
    pub fn hover(&mut self, x: i32, y: i32) -> bool {
        let over = self.at(x, y);
        if over == self.hovered {
            return false;
        }
        self.hovered = over;
        true
    }

    /// Winds the page. True when something has to be drawn again.
    ///
    /// A page whose lines all fit does not move at all, so the wheel over it
    /// does nothing rather than sliding the text out of the window.
    pub fn scroll_by(&mut self, notches: f32) -> bool {
        let wanted = (self.scroll + notches * SCROLL_PER_NOTCH).clamp(0.0, self.overflow);
        if (wanted - self.scroll).abs() < 0.5 {
            return false;
        }
        self.scroll = wanted;
        true
    }

    /// Draws the whole window.
    pub fn draw(
        &mut self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        contents: &Contents,
        top: f32,
        theme: &Theme,
    ) {
        self.placed.clear();
        let width = canvas.width() as f32;
        let height = canvas.height() as f32;

        canvas.fill_rect(0, top as i32, width as i32, (height - top) as i32, theme.ribbon);
        canvas.fill_rect(
            0,
            top as i32,
            RAIL_WIDTH as i32,
            (height - top) as i32,
            theme.backstage(),
        );

        self.draw_rail(canvas, engine, renderer, top, theme);
        self.draw_page(canvas, engine, renderer, contents, top, width, height, theme);
    }

    /// The arrow and the places, down the left.
    fn draw_rail(
        &mut self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        top: f32,
        theme: &Theme,
    ) {
        // The arrow back to the document. It is the only way out of Word's
        // backstage that is drawn anywhere, so it goes first.
        let back = top + 14.0;
        if self.hovered == Some(Hit::Back) {
            canvas.fill_rect(12, back as i32 - 6, 32, 32, theme.bar_hover());
        }
        icons::draw_sized(canvas, Icon::Previous, 18.0, back, 20.0, theme.bar_text());
        self.placed.push((Hit::Back, 12.0, back - 6.0, 32.0, 32.0));

        let mut y = top + 60.0;
        for place in Place::ALL {
            let hit = Hit::Place(*place);
            // Only a place with a page can be the one showing: pressing Save
            // saves and leaves, so nothing is left lit up behind it.
            let chosen = *place == self.place && place.has_a_page();
            if chosen {
                canvas.fill_rect(0, y as i32, RAIL_WIDTH as i32, RAIL_ROW as i32, theme.ribbon);
            } else if self.hovered == Some(hit) {
                canvas.fill_rect(
                    0,
                    y as i32,
                    RAIL_WIDTH as i32,
                    RAIL_ROW as i32,
                    theme.bar_hover(),
                );
            }

            let colour = if chosen { theme.text } else { theme.bar_text() };
            let line = engine.simple_line(place.label(), PADDING, y + 21.0, 9.5, colour);
            renderer.draw_onto(canvas, &line, 0.0, 0.0);
            self.placed.push((hit, 0.0, y, RAIL_WIDTH, RAIL_ROW));

            y += RAIL_ROW;
            // Word leaves a gap before Close, which separates what is done to
            // this document from what is done with the program.
            if *place == Place::Export {
                y += 14.0;
            }
        }
    }

    /// Whatever the chosen place has to show, filling the rest of the window.
    #[allow(clippy::too_many_arguments)]
    fn draw_page(
        &mut self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        contents: &Contents,
        top: f32,
        width: f32,
        height: f32,
        theme: &Theme,
    ) {
        let left = RAIL_WIDTH + PADDING * 1.5;
        let room = (width - left - PADDING * 1.5).max(1.0);
        let mut y = top + 52.0;

        let line = engine.simple_line(&contents.heading, left, y, 20.0, theme.text);
        renderer.draw_onto(canvas, &line, 0.0, 0.0);
        y += 30.0;

        // What the page is telling rather than offering.
        for (label, value) in &contents.facts {
            let line = engine.simple_line(label, left, y + 13.0, 9.0, theme.dim_text);
            renderer.draw_within(canvas, &line, left, y, 150.0, 20.0);
            let shown = if value.is_empty() { "—" } else { value.as_str() };
            let line = engine.simple_line(shown, left + 160.0, y + 13.0, 9.0, theme.text);
            renderer.draw_within(canvas, &line, left + 160.0, y, room - 160.0, 20.0);
            y += 22.0;
        }

        if !contents.facts.is_empty() {
            y += 14.0;
        }

        // The rows before the heading are not part of the list under it: Word's
        // Browse sits above "Recent" because browsing is not a recent document.
        let fixed = contents.rows_heading_at.min(contents.rows.len());
        for (index, row) in contents.rows.iter().enumerate().take(fixed) {
            self.draw_row(canvas, engine, renderer, index, row, left, y, room, theme);
            y += ROW_HEIGHT;
        }

        if !contents.rows_heading.is_empty() {
            y += 10.0;
            let line =
                engine.simple_line(&contents.rows_heading, left, y + 12.0, 9.0, theme.emphasis);
            renderer.draw_onto(canvas, &line, 0.0, 0.0);
            canvas.fill_rect(left as i32, (y + 18.0) as i32, room as i32, 1, theme.pane_edge);
            y += 26.0;
        }

        if fixed >= contents.rows.len() {
            self.overflow = 0.0;
            if !contents.nothing.is_empty() {
                let line =
                    engine.simple_line(&contents.nothing, left, y + 14.0, 9.0, theme.dim_text);
                renderer.draw_onto(canvas, &line, 0.0, 0.0);
            }
            return;
        }

        // The list can be longer than the window holds, so it winds — and how
        // far it can wind is worked out from where it actually ended up rather
        // than guessed at, which keeps the two in step when the window is
        // resized.
        let band_top = y;
        let band_height = (height - band_top - PADDING).max(ROW_HEIGHT);
        let listed = contents.rows.len() - fixed;
        self.overflow = (listed as f32 * ROW_HEIGHT - band_height).max(0.0);
        self.scroll = self.scroll.min(self.overflow);

        let clip = canvas.set_clip(0, band_top as i32, width as i32, band_height as i32);
        for (place, row) in contents.rows.iter().skip(fixed).enumerate() {
            let row_top = band_top + place as f32 * ROW_HEIGHT - self.scroll;
            if row_top + ROW_HEIGHT < band_top || row_top > band_top + band_height {
                continue;
            }
            let index = fixed + place;
            self.draw_row(canvas, engine, renderer, index, row, left, row_top, room, theme);
        }
        canvas.restore_clip(clip);
    }

    /// One line of a page, wherever it has ended up.
    #[allow(clippy::too_many_arguments)]
    fn draw_row(
        &mut self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        index: usize,
        row: &Row,
        left: f32,
        top: f32,
        room: f32,
        theme: &Theme,
    ) {
        let hit = Hit::Row(index);
        if self.hovered == Some(hit) {
            canvas.fill_rect(
                (left - 10.0) as i32,
                top as i32,
                (room + 20.0) as i32,
                ROW_HEIGHT as i32,
                theme.hover,
            );
        }
        let line = engine.simple_line(&row.title, left, top + 19.0, 10.0, theme.text);
        renderer.draw_within(canvas, &line, left, top, room, 24.0);
        if !row.note.is_empty() {
            let line = engine.simple_line(&row.note, left, top + 35.0, 8.0, theme.dim_text);
            renderer.draw_within(canvas, &line, left, top + 24.0, room, 18.0);
        }
        self.placed.push((hit, left - 10.0, top, room + 20.0, ROW_HEIGHT));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_place_word_has_is_on_the_rail() {
        // Info first, because that is what Word's File tab opens onto.
        assert_eq!(Place::ALL.first().copied(), Some(Place::Info));
        assert_eq!(Place::ALL.len(), 9);
    }

    #[test]
    fn the_places_that_simply_do_something_have_no_page() {
        assert!(!Place::Save.has_a_page());
        assert!(!Place::Print.has_a_page());
        assert!(!Place::Close.has_a_page());
        assert!(!Place::Options.has_a_page());
        assert!(Place::Info.has_a_page());
        assert!(Place::Open.has_a_page());
    }

    #[test]
    fn nothing_is_hit_before_it_has_been_drawn() {
        assert_eq!(Backstage::new().at(10, 10), None);
    }

    #[test]
    fn it_opens_on_info() {
        assert_eq!(Backstage::new().place, Place::Info);
    }

    #[test]
    fn a_page_that_fits_does_not_wind() {
        // Nothing has been drawn, so nothing overflows, so the wheel over it is
        // not to slide the text out of the window.
        let mut backstage = Backstage::new();
        assert!(!backstage.scroll_by(-3.0));
        assert!(!backstage.scroll_by(3.0));
    }

    #[test]
    fn going_to_a_place_starts_its_page_at_the_top() {
        let mut backstage = Backstage::new();
        backstage.overflow = 500.0;
        assert!(backstage.scroll_by(3.0));
        backstage.show(Place::Open);
        assert_eq!(backstage.scroll, 0.0);
    }
}
