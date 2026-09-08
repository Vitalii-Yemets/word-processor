//! The navigation pane: the document's headings, as a way of moving about it.
//!
//! # Why headings
//!
//! A long document is navigated by its structure, not by its scrollbar. The
//! headings are that structure, and they are already in the document — a
//! paragraph whose style is a heading style is one. Nothing has to be built or
//! maintained: the outline is read out of the document every time it changes,
//! and is therefore never stale.
//!
//! # Why it closes and resizes
//!
//! Because Word's does, and because a pane that cannot be got rid of is not a
//! pane, it is a permanent loss of a quarter of the window. It closes by its own
//! button, and its right edge is a handle that can be dragged.

use wp_layout::{LayoutEngine, Renderer};
use wp_raster::Canvas;

use super::theme::Theme;

/// How wide the pane is until someone drags it.
pub const NAVIGATION_WIDTH: f32 = 250.0;
/// How narrow and how wide dragging can make it.
const MIN_WIDTH: f32 = 150.0;
const MAX_WIDTH: f32 = 480.0;
/// How far either side of the edge counts as grabbing the handle.
pub const SPLITTER_GRAB: f32 = 4.0;

const ROW_HEIGHT: f32 = 21.0;
const SEARCH_HEIGHT: f32 = 24.0;
const HEADER_HEIGHT: f32 = 26.0;
const TAB_HEIGHT: f32 = 22.0;
/// The side of the close button in the pane's header.
const CLOSE: f32 = 18.0;

/// Which of the pane's three tabs is showing.
///
/// Word's own three: the outline, the pages, and what a search turned up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Headings,
    Pages,
    Results,
    /// The notes people have left on the document.
    Comments,
}

impl Section {
    pub const ALL: &'static [Section] =
        &[Section::Headings, Section::Pages, Section::Results, Section::Comments];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Headings => "Headings",
            Self::Pages => "Pages",
            Self::Results => "Results",
            Self::Comments => "Comments",
        }
    }
}

/// One heading of the document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Heading {
    pub text: String,
    /// How deep it is, counted from zero, which is what indents it.
    pub level: u8,
    /// Which paragraph it is, so pressing it can move the caret there.
    pub paragraph: usize,
}

/// One comment, as the pane lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    pub author: String,
    pub text: String,
    pub paragraph: usize,
    pub offset: usize,
}

/// One thing a search turned up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    /// The line it was found on, with the match somewhere in it.
    pub context: String,
    pub paragraph: usize,
    pub offset: usize,
}

/// What the pane is showing: the outline, the pages, and the search results.
#[derive(Clone, Debug, Default)]
pub struct Contents {
    pub headings: Vec<Heading>,
    pub pages: usize,
    pub current_page: usize,
    pub found: Vec<Found>,
    pub notes: Vec<Note>,
}

/// What was pressed in the pane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Close,
    Tab(Section),
    SearchBox,
    /// A row of whichever list is showing, by its index in that list.
    Row(usize),
}

/// The pane, and what has been typed into its search box.
#[derive(Debug)]
pub struct Navigation {
    pub search: String,
    pub section: Section,
    /// How wide it has been dragged to.
    width: f32,
    /// Whether the search box has the keyboard.
    pub searching: bool,
    /// Which row the pointer is over.
    hovered: Option<usize>,
    hovered_close: bool,
    /// The first row drawn, so a long outline can be scrolled.
    scroll: usize,
    rows: usize,
}

impl Default for Navigation {
    fn default() -> Self {
        Self {
            search: String::new(),
            section: Section::Headings,
            width: NAVIGATION_WIDTH,
            searching: false,
            hovered: None,
            hovered_close: false,
            scroll: 0,
            rows: 0,
        }
    }
}

impl Navigation {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn width(&self) -> f32 {
        self.width
    }

    /// Drags the right edge to a new place. Returns whether anything moved.
    pub fn resize_to(&mut self, x: i32) -> bool {
        let wanted = (x as f32).clamp(MIN_WIDTH, MAX_WIDTH);
        let changed = (wanted - self.width).abs() >= 1.0;
        self.width = wanted;
        changed
    }

    /// Whether a point is on the handle down the pane's right edge.
    #[must_use]
    pub fn on_splitter(&self, x: i32, y: i32, top: f32, bottom: f32) -> bool {
        (y as f32) >= top && (y as f32) < bottom && (x as f32 - self.width).abs() <= SPLITTER_GRAB
    }

    /// Where the list begins, under the header, the tabs and the search box.
    fn list_top(top: f32) -> f32 {
        top + HEADER_HEIGHT + TAB_HEIGHT + SEARCH_HEIGHT + 12.0
    }

    fn close_rect(&self, top: f32) -> (f32, f32) {
        (self.width - CLOSE - 6.0, top + 4.0)
    }

    /// What a point in the pane stands for, if anything.
    #[must_use]
    pub fn hit(&self, x: i32, y: i32, top: f32, bottom: f32) -> Option<Hit> {
        let (x, y) = (x as f32, y as f32);
        if x >= self.width || y < top || y >= bottom {
            return None;
        }

        let (close_x, close_y) = self.close_rect(top);
        if x >= close_x && x < close_x + CLOSE && y >= close_y && y < close_y + CLOSE {
            return Some(Hit::Close);
        }

        let tabs_top = top + HEADER_HEIGHT;
        if y >= tabs_top && y < tabs_top + TAB_HEIGHT {
            let each = self.width / Section::ALL.len() as f32;
            let index = (x / each) as usize;
            return Section::ALL.get(index).copied().map(Hit::Tab);
        }

        let box_top = tabs_top + TAB_HEIGHT + 4.0;
        if y >= box_top && y < box_top + SEARCH_HEIGHT {
            return Some(Hit::SearchBox);
        }

        let list_top = Self::list_top(top);
        if y >= list_top {
            let row = ((y - list_top) / ROW_HEIGHT) as usize;
            return (row < self.rows).then_some(Hit::Row(self.scroll + row));
        }
        None
    }

    /// Lights up whatever the pointer is over. Returns whether that changed.
    pub fn hover(&mut self, x: i32, y: i32, top: f32, bottom: f32) -> bool {
        let found = self.hit(x, y, top, bottom);
        let row = match found {
            Some(Hit::Row(index)) => Some(index),
            _ => None,
        };
        let close = found == Some(Hit::Close);
        let changed = row != self.hovered || close != self.hovered_close;
        self.hovered = row;
        self.hovered_close = close;
        changed
    }

    /// Scrolls the list. Returns whether it moved.
    pub fn scroll_by(&mut self, rows: i32, total: usize) -> bool {
        let limit = total.saturating_sub(self.rows);
        let wanted = (self.scroll as i32 + rows).clamp(0, limit as i32) as usize;
        let changed = wanted != self.scroll;
        self.scroll = wanted;
        changed
    }

    /// Draws the pane down the left of the window.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        contents: &Contents,
        current: Option<usize>,
        bounds: (f32, f32),
        theme: &Theme,
    ) {
        let (top, bottom) = bounds;
        let height = (bottom - top).max(0.0);
        let width = self.width;
        canvas.fill_rect(0, top as i32, width as i32, height as i32, theme.pane);
        canvas.fill_rect((width - 1.0) as i32, top as i32, 1, height as i32, theme.pane_edge);

        let title = engine.simple_line("Navigation", 12.0, top + 18.0, 10.0, theme.text);
        renderer.draw_onto(canvas, &title, 0.0, 0.0);

        // The button that closes the pane, where every pane keeps it.
        let (close_x, close_y) = self.close_rect(top);
        if self.hovered_close {
            canvas.fill_rect(
                close_x as i32,
                close_y as i32,
                CLOSE as i32,
                CLOSE as i32,
                theme.hover,
            );
        }
        cross(canvas, close_x + CLOSE / 2.0, close_y + CLOSE / 2.0, 4.0, theme.text);

        self.draw_tabs(canvas, engine, renderer, top, theme);
        self.draw_search(canvas, engine, renderer, top, theme);

        let list_top = Self::list_top(top);
        self.rows = ((bottom - list_top) / ROW_HEIGHT).max(0.0) as usize;

        match self.section {
            Section::Headings => {
                self.draw_rows(
                    canvas,
                    engine,
                    renderer,
                    list_top,
                    contents.headings.len(),
                    current,
                    theme,
                    "No headings in this document",
                    |index| {
                        let heading = &contents.headings[index];
                        (12.0 + f32::from(heading.level) * 12.0, heading.text.clone())
                    },
                );
            }
            Section::Pages => {
                let current_page = contents.current_page.checked_sub(1);
                self.draw_rows(
                    canvas,
                    engine,
                    renderer,
                    list_top,
                    contents.pages,
                    current_page,
                    theme,
                    "This document has no pages",
                    |index| (12.0, format!("Page {}", index + 1)),
                );
            }
            Section::Comments => {
                self.draw_rows(
                    canvas,
                    engine,
                    renderer,
                    list_top,
                    contents.notes.len(),
                    None,
                    theme,
                    "Nobody has commented on this document",
                    |index| {
                        let note = &contents.notes[index];
                        (12.0, format!("{}: {}", note.author, note.text))
                    },
                );
            }
            Section::Results => {
                let empty = if self.search.is_empty() {
                    "Type in the box above to search"
                } else {
                    "Nothing found"
                };
                self.draw_rows(
                    canvas,
                    engine,
                    renderer,
                    list_top,
                    contents.found.len(),
                    None,
                    theme,
                    empty,
                    |index| (12.0, contents.found[index].context.clone()),
                );
            }
        }
    }

    fn draw_tabs(
        &self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        top: f32,
        theme: &Theme,
    ) {
        let tabs_top = top + HEADER_HEIGHT;
        let each = self.width / Section::ALL.len() as f32;
        for (index, section) in Section::ALL.iter().enumerate() {
            let x = index as f32 * each;
            let chosen = *section == self.section;
            let colour = if chosen { theme.text } else { theme.dim_text };
            let measured = engine.simple_line(section.label(), 0.0, 0.0, 8.0, colour);
            let line = engine.simple_line(
                section.label(),
                x + (each - measured.width) / 2.0,
                tabs_top + 15.0,
                8.0,
                colour,
            );
            renderer.draw_onto(canvas, &line, 0.0, 0.0);
            if chosen {
                canvas.fill_rect(
                    x as i32 + 6,
                    (tabs_top + TAB_HEIGHT - 2.0) as i32,
                    each as i32 - 12,
                    2,
                    theme.emphasis,
                );
            }
        }
        canvas.fill_rect(
            0,
            (tabs_top + TAB_HEIGHT - 1.0) as i32,
            self.width as i32,
            1,
            theme.pane_edge,
        );
    }

    fn draw_search(
        &self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        top: f32,
        theme: &Theme,
    ) {
        let box_top = top + HEADER_HEIGHT + TAB_HEIGHT + 4.0;
        canvas.fill_rect(
            10,
            box_top as i32,
            (self.width - 20.0) as i32,
            SEARCH_HEIGHT as i32,
            theme.field,
        );
        let edge = if self.searching { theme.emphasis } else { theme.field_edge };
        outline(canvas, 10, box_top as i32, (self.width - 20.0) as i32, SEARCH_HEIGHT as i32, edge);

        let empty = self.search.is_empty();
        let text = if empty { "Search document" } else { self.search.as_str() };
        let colour = if empty { theme.dim_text } else { theme.text };
        let line = engine.simple_line(text, 16.0, box_top + 16.0, 8.5, colour);
        renderer.draw_onto(canvas, &line, 0.0, 0.0);

        // The caret, so a box with the keyboard looks like one.
        if self.searching {
            let measured = engine.simple_line(text, 0.0, 0.0, 8.5, colour).width;
            let x = if empty { 16.0 } else { 16.0 + measured };
            canvas.fill_rect(x as i32, (box_top + 4.0) as i32, 1, 16, theme.text);
        }
    }

    /// Draws whichever list is showing, which differ only in what a row says.
    #[allow(clippy::too_many_arguments)]
    fn draw_rows(
        &mut self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        list_top: f32,
        total: usize,
        current: Option<usize>,
        theme: &Theme,
        when_empty: &str,
        row_text: impl Fn(usize) -> (f32, String),
    ) {
        self.scroll = self.scroll.min(total.saturating_sub(self.rows));

        if total == 0 {
            let line = engine.simple_line(when_empty, 12.0, list_top + 16.0, 8.0, theme.dim_text);
            renderer.draw_onto(canvas, &line, 0.0, 0.0);
            return;
        }

        for row in 0..self.rows {
            let index = self.scroll + row;
            if index >= total {
                break;
            }
            let y = list_top + row as f32 * ROW_HEIGHT;

            let background = if current == Some(index) {
                Some(theme.accent)
            } else if self.hovered == Some(index) {
                Some(theme.hover)
            } else {
                None
            };
            if let Some(fill) = background {
                canvas.fill_rect(4, y as i32, (self.width - 8.0) as i32, ROW_HEIGHT as i32, fill);
            }

            let (indent, text) = row_text(index);
            let colour = if current == Some(index) { theme.on_accent() } else { theme.text };
            let text = trim_to_width(engine, &text, self.width - indent - 12.0, colour);
            let line = engine.simple_line(&text, indent, y + 15.0, 8.5, colour);
            renderer.draw_onto(canvas, &line, 0.0, 0.0);
        }
    }
}

fn outline(canvas: &mut Canvas, x: i32, y: i32, width: i32, height: i32, colour: wp_raster::Color) {
    canvas.fill_rect(x, y, width, 1, colour);
    canvas.fill_rect(x, y + height - 1, width, 1, colour);
    canvas.fill_rect(x, y, 1, height, colour);
    canvas.fill_rect(x + width - 1, y, 1, height, colour);
}

/// A small ×, drawn as two lines rather than set as a letter.
fn cross(canvas: &mut Canvas, centre_x: f32, centre_y: f32, reach: f32, colour: wp_raster::Color) {
    let steps = (reach * 2.0) as i32;
    for step in 0..=steps {
        let along = step as f32;
        canvas.fill_rect(
            (centre_x - reach + along) as i32,
            (centre_y - reach + along) as i32,
            1,
            1,
            colour,
        );
        canvas.fill_rect(
            (centre_x - reach + along) as i32,
            (centre_y + reach - along) as i32,
            1,
            1,
            colour,
        );
    }
}

/// Cuts a row down to what fits, ending it with an ellipsis.
fn trim_to_width(
    engine: &mut LayoutEngine<'_>,
    text: &str,
    width: f32,
    colour: wp_raster::Color,
) -> String {
    if engine.simple_line(text, 0.0, 0.0, 8.5, colour).width <= width {
        return text.to_owned();
    }
    let mut kept = String::new();
    for character in text.chars() {
        let mut candidate = kept.clone();
        candidate.push(character);
        candidate.push('…');
        if engine.simple_line(&candidate, 0.0, 0.0, 8.5, colour).width > width {
            break;
        }
        kept.push(character);
    }
    kept.push('…');
    kept
}
