//! Editing the header or the footer in place, as double-clicking one does.
//!
//! # How Word does it and how this does it
//!
//! In Word, double-clicking the top of a page opens the header for editing: the
//! body of the document goes grey, a dashed line marks the header, and the
//! caret is in the header's own text. Double-clicking the body, or pressing
//! Escape, comes back out.
//!
//! Here the same thing happens, and the machinery is the part swap in
//! [`wp_docx::Document::enter_part`]: a header is a body of its own in a part
//! of its own, so pointing the editor at that part makes every command work on
//! it — typing, styles, tables, pictures, all of them, with no second caret
//! model.
//!
//! The body the person was reading a moment ago is kept as it was last laid
//! out, and drawn behind in a paler colour. It is a picture, not a document:
//! clicking it does nothing except come back out, which is exactly what
//! clicking the greyed body does in Word.
//!
//! # What is different
//!
//! Word shows the header on every page while it is being edited. This shows it
//! on the first, because a header is one piece of text however many pages it
//! is printed on — editing it here changes it everywhere, which is the part
//! that matters.

use wp_docx::furniture::Furniture;
use wp_layout::Page;
use wp_raster::Color;
use wp_shell::Response;

use super::Editor;

/// How much of its colour the body keeps while something else is being edited.
const DIMMED: u8 = 90;

impl Editor {
    /// Opens the header or the footer for editing.
    pub(super) fn edit_furniture(&mut self, which: Furniture) -> Response {
        if self.document.part_being_edited().is_some() {
            return Response::Ignored;
        }
        let Some(part) = self.document.furniture_part(which) else {
            let name = if which == Furniture::Header { "header" } else { "footer" };
            return self.report(&format!("This document has no {name} — Insert ▸ {name} adds one"));
        };
        if !self.document.enter_part(&part) {
            return Response::Ignored;
        }

        // What was on screen a moment ago, kept as a picture of the document
        // to draw behind what is being edited.
        self.dimmed = dimmed(core::mem::take(&mut self.pages));
        self.editing_furniture = Some(which);
        self.relayout();
        self.reveal_caret();

        let name = if which == Furniture::Header { "Header" } else { "Footer" };
        self.report(&format!("{name} — double-click the document or press Escape to come back"))
    }

    /// Comes back out to the document.
    pub(super) fn leave_furniture(&mut self) -> Response {
        if self.editing_furniture.take().is_none() {
            return Response::Ignored;
        }
        self.document.leave_part();
        self.dimmed = Vec::new();
        self.relayout();
        self.reveal_caret();
        self.report("Document")
    }

    /// Whether a header or a footer is being edited.
    #[must_use]
    pub(super) fn in_furniture(&self) -> bool {
        self.editing_furniture.is_some()
    }

    /// Which piece of furniture a double click landed in, if either.
    ///
    /// The band above the top margin of a page is the header, and the one below
    /// the bottom margin is the footer — which is where they are printed and
    /// therefore where a person points at them.
    #[must_use]
    pub(super) fn furniture_at(&self, x: i32, y: i32) -> Option<Furniture> {
        let (px, py) = (x as f32, y as f32);
        let metrics = wp_layout::PageMetrics::from_document(&self.document);
        let scale = self.pixels_per_inch() / 72.0;

        for index in 0..self.pages.len() {
            let (origin_x, origin_y) = self.page_origin(index);
            let top = self.content_top() + origin_y - self.scroll_down();
            let page = &self.pages[index];
            if px < origin_x || px >= origin_x + page.width {
                continue;
            }
            if py >= top && py < top + metrics.margin_top * scale {
                return Some(Furniture::Header);
            }
            if py >= top + page.height - metrics.margin_bottom * scale && py < top + page.height {
                return Some(Furniture::Footer);
            }
        }
        None
    }

    /// Draws the document behind what is being edited.
    pub(super) fn draw_dimmed_document(&mut self) {
        if self.dimmed.is_empty() {
            return;
        }
        // Drawn the same way the real pages are, so it sits where it sat: the
        // page it is a picture of has not moved.
        let pages = core::mem::take(&mut self.dimmed);
        let mine = core::mem::replace(&mut self.pages, pages);
        self.draw_pages();
        self.dimmed = core::mem::replace(&mut self.pages, mine);
    }
}

/// The same pages with everything drawn fainter.
#[must_use]
fn dimmed(pages: Vec<Page>) -> Vec<Page> {
    let fade = |colour: Color| Color { alpha: DIMMED, ..colour };
    pages
        .into_iter()
        .map(|mut page| {
            for glyph in &mut page.glyphs {
                glyph.color = fade(glyph.color);
            }
            for decoration in &mut page.decorations {
                decoration.color = fade(decoration.color);
            }
            // The lines are dropped: a click in the picture of the document
            // must not land in text that is not being edited.
            page.lines.clear();
            page
        })
        .collect()
}
