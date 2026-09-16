//! Word's Document Recovery pane: what was open when the program stopped.
//!
//! # Why a pane and not a dialog
//!
//! Because the person has to choose, and choosing takes looking. A dialog
//! would stand in front of the document and have to be answered before
//! anything else could happen; the pane sits beside it, so a recovered
//! version can be opened, read, compared with the one on disk, saved or
//! thrown away, in whatever order makes sense. That is Word's pane, in
//! Word's place, doing Word's job.
//!
//! # What each row says
//!
//! The name of the document, when the copy was taken, and where it came
//! from. The name alone would not be enough: after a crash there may be two
//! of the same document, and the time is what tells them apart.

use wp_layout::{LayoutEngine, Renderer, TextStyle};
use wp_raster::{Canvas, Color};

use super::theme::Theme;
use crate::editor::autorecover::Recovered;

/// How wide the pane is drawn. Word's is about this, and the width matters:
/// the rows hold a file name and a time.
pub const WIDTH: f32 = 280.0;

/// How tall one row is: two lines of text and room around them.
const ROW: f32 = 46.0;

/// The room round everything.
const PADDING: f32 = 10.0;

/// The strip at the foot, which holds the Close button.
const FOOTER: f32 = 44.0;

/// What a press in the pane landed on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    /// Open the recovered document at this place in the list.
    Open(usize),
    /// Throw that one away.
    Delete(usize),
    /// Shut the pane, which is what says the person is done with all of them.
    Close,
}

/// The pane, and where it last drew what.
#[derive(Clone, Debug, Default)]
pub struct RecoveryPane {
    /// The copies found, newest first.
    pub entries: Vec<Recovered>,
    /// Which row the pointer is over.
    hovered: Option<Hit>,
    /// Which row was opened, so it can be shown as the one being looked at.
    pub opened: Option<usize>,
    /// How far down the list has been scrolled, in rows.
    scroll: usize,
    /// How many rows fitted last time it was drawn.
    rows: usize,
    placed: Vec<(Hit, f32, f32, f32, f32)>,
}

impl RecoveryPane {
    #[must_use]
    pub fn new(entries: Vec<Recovered>) -> Self {
        Self { entries, ..Self::default() }
    }

    #[must_use]
    pub fn width(&self) -> f32 {
        WIDTH
    }

    /// What is under a point, if anything.
    #[must_use]
    pub fn hit(&self, x: i32, y: i32) -> Option<Hit> {
        let (x, y) = (x as f32, y as f32);
        self.placed
            .iter()
            .find(|(_, left, top, width, height)| {
                x >= *left && x < left + width && y >= *top && y < top + height
            })
            .map(|(hit, ..)| *hit)
    }

    /// Follows the pointer. Whether the window has to be drawn again.
    pub fn hover(&mut self, x: i32, y: i32) -> bool {
        let over = self.hit(x, y);
        if over == self.hovered {
            return false;
        }
        self.hovered = over;
        true
    }

    /// Scrolls the list. Whether anything moved.
    pub fn scroll_by(&mut self, rows: i32) -> bool {
        let most = self.entries.len().saturating_sub(self.rows);
        let wanted = (self.scroll as i32 + rows).clamp(0, most as i32) as usize;
        if wanted == self.scroll {
            return false;
        }
        self.scroll = wanted;
        true
    }

    /// Takes one copy off the list, the file itself having gone.
    pub fn remove(&mut self, index: usize) {
        if index >= self.entries.len() {
            return;
        }
        self.entries.remove(index);
        self.opened = match self.opened {
            Some(opened) if opened == index => None,
            Some(opened) if opened > index => Some(opened - 1),
            other => other,
        };
        self.scroll = self.scroll.min(self.entries.len().saturating_sub(1));
    }

    /// Draws the pane down the left of the window, where Word puts it.
    pub fn draw(
        &mut self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        (top, bottom): (f32, f32),
        theme: &Theme,
    ) {
        self.placed.clear();
        let width = WIDTH;
        canvas.fill_rect(0, top as i32, width as i32, (bottom - top) as i32, theme.pane);
        canvas.fill_rect(
            (width - 1.0) as i32,
            top as i32,
            1,
            (bottom - top) as i32,
            theme.pane_edge,
        );

        // The caption, with the cross that shuts the pane.
        let caption = engine.styled_line(
            "Document Recovery",
            PADDING,
            top + 24.0,
            10.0,
            theme.text,
            TextStyle { bold: true, ..TextStyle::default() },
        );
        renderer.draw_onto(canvas, &caption, 0.0, 0.0);
        let close_left = width - 26.0;
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

        // Word's own sentence, which says the one thing a person needs to
        // know: these are copies, and saving is what keeps them.
        let said = engine.simple_line(
            "The files below were recovered.",
            PADDING,
            top + 44.0,
            9.0,
            theme.dim_text,
        );
        renderer.draw_within(canvas, &said, 0.0, top + 30.0, width - PADDING * 2.0, 20.0);

        let list_top = top + 56.0;
        let list_bottom = bottom - FOOTER;
        self.rows = ((list_bottom - list_top) / ROW).floor().max(1.0) as usize;

        let mut y = list_top;
        for (index, entry) in self.entries.iter().enumerate().skip(self.scroll) {
            if y + ROW > list_bottom {
                break;
            }
            let open = Hit::Open(index);
            if self.opened == Some(index) {
                canvas.fill_rect(4, y as i32, (width - 8.0) as i32, ROW as i32, theme.hover);
                canvas.fill_rect(4, y as i32, 3, ROW as i32, theme.accent);
            } else if matches!(self.hovered, Some(Hit::Open(at) | Hit::Delete(at)) if at == index) {
                canvas.fill_rect(4, y as i32, (width - 8.0) as i32, ROW as i32, theme.hover);
            }

            let name = engine.styled_line(
                &entry.name,
                PADDING + 4.0,
                y + 18.0,
                9.5,
                theme.text,
                TextStyle { bold: true, ..TextStyle::default() },
            );
            renderer.draw_within(canvas, &name, 0.0, y, width - 40.0, ROW);

            // What Word writes under the name: that it is a copy, and when.
            let said = format!("Autosaved {}", entry.when());
            let when = engine.simple_line(&said, PADDING + 4.0, y + 34.0, 8.5, theme.dim_text);
            renderer.draw_within(canvas, &when, 0.0, y + 20.0, width - 40.0, ROW - 20.0);

            // The cross that throws that copy away, shown when the row is
            // under the pointer, as Word shows its menu arrow.
            let delete = Hit::Delete(index);
            let delete_left = width - 28.0;
            if matches!(self.hovered, Some(Hit::Open(at) | Hit::Delete(at)) if at == index) {
                if self.hovered == Some(delete) {
                    canvas.fill_rect(
                        delete_left as i32 - 4,
                        (y + ROW / 2.0 - 11.0) as i32,
                        24,
                        22,
                        theme.danger,
                    );
                }
                super::icons::draw_sized(
                    canvas,
                    super::icons::Icon::Close,
                    delete_left,
                    y + ROW / 2.0 - 5.0,
                    10.0,
                    theme.text,
                );
            }
            self.placed.push((delete, delete_left - 4.0, y + ROW / 2.0 - 11.0, 24.0, 22.0));
            self.placed.push((open, 4.0, y, width - 40.0, ROW));
            y += ROW;
        }

        self.draw_footer(canvas, engine, renderer, list_bottom, width, theme);
    }

    /// The strip at the foot: the Close button, and nothing else. Word's
    /// pane has the same one, and it means "I am done with these".
    fn draw_footer(
        &mut self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        top: f32,
        width: f32,
        theme: &Theme,
    ) {
        canvas.fill_rect(0, top as i32, width as i32, 1, theme.pane_edge);
        let (left, button_top, button_width, height) = (width - 86.0, top + 10.0, 76.0, 26.0);
        let face = if self.hovered == Some(Hit::Close) { theme.hover } else { theme.field };
        canvas.fill_rect(left as i32, button_top as i32, button_width as i32, height as i32, face);
        outline(canvas, left, button_top, button_width, height, theme.field_edge);
        let label = engine.simple_line("Close", left + 22.0, button_top + 17.0, 9.0, theme.text);
        renderer.draw_within(canvas, &label, left, button_top, button_width, height);
    }
}

/// A one-pixel line round a rectangle.
fn outline(canvas: &mut Canvas, x: f32, y: f32, width: f32, height: f32, color: Color) {
    let (x, y, width, height) = (x as i32, y as i32, width as i32, height as i32);
    canvas.fill_rect(x, y, width, 1, color);
    canvas.fill_rect(x, y + height - 1, width, 1, color);
    canvas.fill_rect(x, y, 1, height, color);
    canvas.fill_rect(x + width - 1, y, 1, height, color);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn entry(name: &str) -> Recovered {
        Recovered {
            name: name.to_owned(),
            original: Some(PathBuf::from("/tmp").join(name)),
            saved: "2026-09-16T11:22:33Z".to_owned(),
            copy: PathBuf::from("/tmp/recovery").join(name),
        }
    }

    #[test]
    fn a_copy_taken_away_leaves_the_others_where_they_were() {
        let mut pane = RecoveryPane::new(vec![entry("a.docx"), entry("b.docx"), entry("c.docx")]);
        pane.opened = Some(2);
        pane.remove(0);
        assert_eq!(pane.entries.len(), 2);
        assert_eq!(pane.entries[0].name, "b.docx");
        assert_eq!(pane.opened, Some(1), "the one being looked at is still that one");
        pane.remove(1);
        assert_eq!(pane.opened, None, "and when it goes, nothing is being looked at");
    }

    #[test]
    fn nothing_is_under_a_point_before_the_pane_has_been_drawn() {
        let pane = RecoveryPane::new(vec![entry("a.docx")]);
        assert_eq!(pane.hit(10, 10), None);
    }
}
