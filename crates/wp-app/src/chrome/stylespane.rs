//! Word's Styles pane: every style the document has, down the right-hand side.
//!
//! # Why a pane and not a list
//!
//! The gallery on the ribbon shows a dozen styles across; a document made from
//! a template has a hundred. The pane is where the rest of them live, and where
//! a style is made, changed, or looked at rather than merely applied.
//!
//! # Each style drawn in its own formatting
//!
//! The same reason the gallery does it and the same reason the ribbon's bold
//! button is a bold letter B: a style shown in what it does needs no
//! explaining. Word puts a tick box under the list to turn that off, because a
//! hundred styles each at their own size is a list that cannot be scanned, and
//! that tick box is here too.
//!
//! # What the pane holds
//!
//! The list, a tick box for the preview, the three buttons along the bottom —
//! New Style, Style Inspector, Manage Styles — and the Options link that says
//! which styles to show. That is Word's pane, in Word's order.

use wp_layout::{LayoutEngine, Renderer, TextStyle};
use wp_raster::{Canvas, Color};

use super::theme::Theme;

/// How wide the pane is drawn.
pub const WIDTH: f32 = 250.0;

/// One row of the list.
const ROW: f32 = 26.0;

/// The strip along the bottom holding the three buttons.
const FOOTER: f32 = 76.0;

/// The room round everything.
const PADDING: f32 = 10.0;

/// One style, as the pane shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// The identifier, or `None` for the body style every document has.
    pub id: Option<String>,
    pub name: String,
    /// What it looks like, for the preview.
    pub style: TextStyle,
    pub size: f32,
    /// Whether the paragraph the caret is in has it.
    pub current: bool,
    /// Whether anything in the document has it.
    pub in_use: bool,
}

/// Which styles the pane lists.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Showing {
    /// Every style the document defines.
    #[default]
    All,
    /// Only the ones something in the document has.
    InUse,
}

impl Showing {
    /// What the Options row says it is showing.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All styles",
            Self::InUse => "In use",
        }
    }

    /// The other one, which is what pressing the row does.
    #[must_use]
    pub fn other(self) -> Self {
        match self {
            Self::All => Self::InUse,
            Self::InUse => Self::All,
        }
    }
}

/// Where a press on the pane can land.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    /// One of the styles, by its place in the list the pane was given.
    Style(usize),
    /// The tick box that turns the preview off.
    ShowPreview,
    /// The row that says which styles are listed.
    Options,
    NewStyle,
    Inspector,
    Manage,
    Close,
}

/// The pane, and what it remembers between one drawing and the next.
#[derive(Clone, Debug)]
pub struct StylesPane {
    pub showing: Showing,
    /// Whether each style is drawn in its own formatting.
    pub preview: bool,
    /// The first row drawn, so a long list can be scrolled.
    scroll: usize,
    hovered: Option<Hit>,
    /// Where everything ended up when it was last drawn.
    placed: Vec<(Hit, f32, f32, f32, f32)>,
    /// How many rows the list had room for.
    rows: usize,
}

impl Default for StylesPane {
    fn default() -> Self {
        Self::new()
    }
}

impl StylesPane {
    #[must_use]
    pub fn new() -> Self {
        Self {
            showing: Showing::default(),
            preview: true,
            scroll: 0,
            hovered: None,
            placed: Vec::new(),
            rows: 0,
        }
    }

    /// Moves the list, and says whether it moved.
    pub fn scroll_by(&mut self, rows: i32, total: usize) -> bool {
        let last = total.saturating_sub(self.rows.max(1));
        let wanted = (self.scroll as i32 + rows).clamp(0, last as i32) as usize;
        if wanted == self.scroll {
            return false;
        }
        self.scroll = wanted;
        true
    }

    /// What a point is on, if anything.
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

    /// Draws the pane down the right-hand side of the window.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        entries: &[Entry],
        left: f32,
        top: f32,
        bottom: f32,
        theme: &Theme,
    ) {
        self.placed.clear();
        let width = WIDTH;
        canvas.fill_rect(left as i32, top as i32, width as i32, (bottom - top) as i32, theme.pane);
        canvas.fill_rect(left as i32, top as i32, 1, (bottom - top) as i32, theme.pane_edge);

        // The caption, with the cross that shuts the pane.
        let line = engine.simple_line("Styles", left + PADDING, top + 24.0, 10.0, theme.text);
        renderer.draw_onto(canvas, &line, 0.0, 0.0);
        let close_left = left + width - 26.0;
        if self.hovered == Some(Hit::Close) {
            canvas.fill_rect(close_left as i32 - 4, top as i32 + 10, 24, 22, theme.hover);
        }
        super::icons::draw_sized(
            canvas,
            super::icons::Icon::Close,
            close_left,
            top + 14.0,
            12.0,
            theme.text,
        );
        self.placed.push((Hit::Close, close_left - 4.0, top + 10.0, 24.0, 22.0));

        // The list, between the caption and the strip at the foot.
        let list_top = top + 40.0;
        let list_bottom = bottom - FOOTER;
        self.rows = ((list_bottom - list_top) / ROW).floor().max(1.0) as usize;

        let mut y = list_top;
        for (index, entry) in entries.iter().enumerate().skip(self.scroll) {
            if y + ROW > list_bottom {
                break;
            }
            let hit = Hit::Style(index);
            if entry.current {
                canvas.fill_rect(
                    (left + 4.0) as i32,
                    y as i32,
                    (width - 8.0) as i32,
                    ROW as i32,
                    theme.hover,
                );
                // A bar down the left of the one in force, as Word draws it.
                canvas.fill_rect((left + 4.0) as i32, y as i32, 3, ROW as i32, theme.accent);
            } else if self.hovered == Some(hit) {
                canvas.fill_rect(
                    (left + 4.0) as i32,
                    y as i32,
                    (width - 8.0) as i32,
                    ROW as i32,
                    theme.hover,
                );
            }

            // Drawn in its own formatting, or as a plain name when the tick box
            // below says so.
            let (size, style) = if self.preview {
                (entry.size.clamp(7.0, 13.0), entry.style)
            } else {
                (9.0, TextStyle::default())
            };
            let line = engine.styled_line(
                &entry.name,
                left + PADDING + 4.0,
                y + ROW - 8.0,
                size,
                theme.text,
                style,
            );
            renderer.draw_within(canvas, &line, left, y, width - PADDING * 2.0 - 20.0, ROW);

            // A mark against the ones the document actually uses, so a list of
            // a hundred still says which six matter.
            if entry.in_use {
                canvas.fill_rect(
                    (left + width - 16.0) as i32,
                    (y + ROW / 2.0 - 2.0) as i32,
                    4,
                    4,
                    theme.dim_text,
                );
            }
            self.placed.push((hit, left + 4.0, y, width - 8.0, ROW));
            y += ROW;
        }

        self.draw_footer(canvas, engine, renderer, left, list_bottom, width, theme);
    }

    /// The tick box, the Options row and the three buttons.
    #[allow(clippy::too_many_arguments)]
    fn draw_footer(
        &mut self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        left: f32,
        top: f32,
        width: f32,
        theme: &Theme,
    ) {
        canvas.fill_rect(left as i32, top as i32, width as i32, 1, theme.pane_edge);

        // Show Preview, which is Word's own wording.
        let box_left = left + PADDING;
        let box_top = top + 8.0;
        canvas.fill_rect(box_left as i32, box_top as i32, 14, 14, theme.field);
        outline(canvas, box_left, box_top, 14.0, 14.0, theme.field_edge);
        if self.preview {
            canvas.fill_rect((box_left + 3.0) as i32, (box_top + 3.0) as i32, 8, 8, theme.accent);
        }
        let line =
            engine.simple_line("Show Preview", box_left + 22.0, box_top + 12.0, 8.5, theme.text);
        renderer.draw_onto(canvas, &line, 0.0, 0.0);
        self.placed.push((Hit::ShowPreview, box_left, box_top, width - PADDING * 2.0, 16.0));

        // The row that says which styles are listed, which Word calls Options.
        let options_y = top + 28.0;
        if self.hovered == Some(Hit::Options) {
            canvas.fill_rect(
                (left + 4.0) as i32,
                options_y as i32 - 2,
                (width - 8.0) as i32,
                16,
                theme.hover,
            );
        }
        let line = engine.simple_line(
            &format!("Options: {}", self.showing.label()),
            box_left,
            options_y + 10.0,
            8.5,
            theme.accent,
        );
        renderer.draw_onto(canvas, &line, 0.0, 0.0);
        self.placed.push((Hit::Options, left + 4.0, options_y - 2.0, width - 8.0, 16.0));

        // And Word's three buttons along the very bottom.
        let buttons: &[(Hit, &str)] =
            &[(Hit::NewStyle, "New"), (Hit::Inspector, "Inspect"), (Hit::Manage, "Manage")];
        let room = width - PADDING * 2.0;
        let each = (room - 8.0) / buttons.len() as f32;
        let button_y = top + 48.0;
        for (index, (hit, label)) in buttons.iter().enumerate() {
            let button_left = left + PADDING + (each + 4.0) * index as f32;
            let background = if self.hovered == Some(*hit) { theme.hover } else { theme.field };
            canvas.fill_rect(button_left as i32, button_y as i32, each as i32, 20, background);
            outline(canvas, button_left, button_y, each, 20.0, theme.field_edge);

            let measured = engine.simple_line(label, 0.0, 0.0, 8.0, theme.text).width;
            let line = engine.simple_line(
                label,
                button_left + (each - measured) / 2.0,
                button_y + 14.0,
                8.0,
                theme.text,
            );
            renderer.draw_onto(canvas, &line, 0.0, 0.0);
            self.placed.push((*hit, button_left, button_y, each, 20.0));
        }
    }
}

fn outline(canvas: &mut Canvas, x: f32, y: f32, width: f32, height: f32, colour: Color) {
    let (x, y, width, height) = (x as i32, y as i32, width as i32, height as i32);
    canvas.fill_rect(x, y, width, 1, colour);
    canvas.fill_rect(x, y + height - 1, width, 1, colour);
    canvas.fill_rect(x, y, 1, height, colour);
    canvas.fill_rect(x + width - 1, y, 1, height, colour);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(count: usize) -> Vec<Entry> {
        (0..count)
            .map(|index| Entry {
                id: Some(format!("Style{index}")),
                name: format!("Style {index}"),
                style: TextStyle::default(),
                size: 11.0,
                current: false,
                in_use: index < 2,
            })
            .collect()
    }

    #[test]
    fn the_list_scrolls_only_as_far_as_there_is_list() {
        let mut pane = StylesPane::new();
        pane.rows = 10;
        assert!(pane.scroll_by(3, 40), "the list did not move");
        assert!(pane.scroll_by(-99, 40), "it did not come back to the top");
        assert_eq!(pane.scroll, 0);
        // Never past the last screenful: a list scrolled off the end is a pane
        // showing nothing.
        pane.scroll_by(999, 40);
        assert_eq!(pane.scroll, 30);
    }

    #[test]
    fn a_list_shorter_than_the_pane_does_not_scroll_at_all() {
        let mut pane = StylesPane::new();
        pane.rows = 20;
        assert!(!pane.scroll_by(5, 6), "a short list moved");
    }

    #[test]
    fn showing_flips_between_the_two_word_offers() {
        assert_eq!(Showing::All.other(), Showing::InUse);
        assert_eq!(Showing::InUse.other(), Showing::All);
        assert_eq!(Showing::All.label(), "All styles");
    }

    #[test]
    fn nothing_is_hit_before_the_pane_has_been_drawn() {
        // Everything the pane knows about where it is comes from drawing it.
        let pane = StylesPane::new();
        assert_eq!(pane.at(10, 10), None);
        let _ = entries(3);
    }
}
