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

use wp_docx::furniture::{Furniture, Which};
use wp_layout::Page;
use wp_raster::Color;
use wp_shell::Response;

use super::Editor;

/// How much of its colour the body keeps while something else is being edited.
const DIMMED: u8 = 90;

impl Editor {
    /// Opens the header or the footer for editing.
    pub(super) fn edit_furniture(&mut self, kind: Furniture) -> Response {
        if self.document.part_being_edited().is_some() {
            return Response::Ignored;
        }

        // Which of the section's three the caret's page is printed with: the
        // first page of a section can have its own, and so can the left-hand
        // pages of a book. Editing the header of the page being looked at is
        // the only thing a double click there can sensibly mean.
        let which = self.which_furniture_here();
        let section = self.document.section_here();
        let part = match self.document.furniture_part_for(kind, section, which) {
            Some(part) => part,
            None if which == Which::Default => {
                let name = if kind == Furniture::Header { "header" } else { "footer" };
                return self
                    .report(&format!("This document has no {name} — Insert ▸ {name} adds one"));
            }
            // A section that has asked for a different first page and never
            // written one has a blank page waiting for it; a double click there
            // is somebody about to fill it in.
            None => {
                if self
                    .document
                    .set_furniture_for(
                        kind,
                        which,
                        wp_docx::furniture::Preset::Blank,
                        wp_docx::model::Alignment::Start,
                        "",
                    )
                    .is_err()
                {
                    return Response::Ignored;
                }
                let Some(part) = self.document.furniture_part_for(kind, section, which) else {
                    return Response::Ignored;
                };
                part
            }
        };

        if !self.document.enter_part(&part) {
            return Response::Ignored;
        }

        // What was on screen a moment ago, kept as a picture of the document
        // to draw behind what is being edited.
        self.dimmed = dimmed(core::mem::take(&mut self.pages));
        self.editing_furniture = Some(kind);
        // The ribbon shows the tab that is about headers and footers, which is
        // what Word does the moment one is opened. Where it was is remembered,
        // because coming back out should not leave somebody somewhere else.
        self.tab_before_furniture = Some(self.ribbon.tab);
        self.ribbon.tab = crate::chrome::ribbon::Tab::HeaderFooter;
        self.relayout();
        self.reveal_caret();

        let name = if kind == Furniture::Header { "Header" } else { "Footer" };
        let of = match which {
            Which::First => " (first page)",
            Which::Even => " (even pages)",
            Which::Default => "",
        };
        self.report(&format!("{name}{of} — double-click the document or press Escape to come back"))
    }

    /// Which of a section's three headers the caret's page is printed with.
    #[must_use]
    pub(super) fn which_furniture_here(&self) -> Which {
        let page = self.caret_page();
        let section = self.document.section_here();
        // The first page of the section is the first page whose text belongs to
        // it, which the layout knows and nothing else does.
        let first_of_section = self.first_page_of_section(section) == page;
        self.document.which_for_page(section, first_of_section, page)
    }

    /// Which page a section's text begins on, counted from one.
    #[must_use]
    fn first_page_of_section(&self, section: usize) -> usize {
        for (index, page) in self.pages.iter().enumerate() {
            let Some(line) = page.lines.first() else { continue };
            if self.document.section_index_of(line.paragraph) == section {
                return index + 1;
            }
        }
        1
    }

    /// Comes back out to the document.
    pub(super) fn leave_furniture(&mut self) -> Response {
        if self.editing_furniture.take().is_none() {
            return Response::Ignored;
        }
        self.document.leave_part();
        self.dimmed = Vec::new();
        if let Some(tab) = self.tab_before_furniture.take() {
            self.ribbon.tab = tab;
        }
        self.relayout();
        self.reveal_caret();
        self.report("Document")
    }

    /// Moves between the header and the footer of the page being edited.
    ///
    /// Word's Go to Header and Go to Footer. Coming out and going back in
    /// rather than swapping the part underneath, because everything about the
    /// state — the picture of the document behind, which part the caret is in,
    /// what the strip says — is set up by going in.
    pub(super) fn go_to_furniture(&mut self, kind: Furniture) -> Response {
        if self.editing_furniture == Some(kind) {
            return Response::Ignored;
        }
        self.leave_furniture();
        self.edit_furniture(kind)
    }

    /// Whether the section this page belongs to takes its header from the
    /// section before it.
    #[must_use]
    pub(super) fn linked_to_previous(&self) -> bool {
        let Some(kind) = self.editing_furniture else { return false };
        let section = self.document.section_here();
        // The first section has nothing before it to be linked to.
        section > 0 && !self.document.has_own_furniture(kind, section, self.which_furniture_here())
    }

    /// Links this section's header to the one before it, or breaks the link.
    ///
    /// Breaking it copies what the section was showing into a header of its
    /// own, so that the page does not change the moment the link is broken —
    /// which is what Word does and what makes the button safe to press.
    pub(super) fn toggle_link_to_previous(&mut self) -> Response {
        let Some(kind) = self.editing_furniture else { return Response::Ignored };
        let section = self.document.section_here();
        if section == 0 {
            return self.report("The first section has nothing before it");
        }
        let which = self.which_furniture_here();

        // The part being edited has to be let go of first: it is about to stop
        // being the one this section uses.
        let linked = self.linked_to_previous();
        self.leave_furniture();

        let changed = if linked {
            let inherited = self.document.furniture_of_page(kind, section, which);
            match inherited {
                Some(body) => self.document.set_furniture_body(kind, which, &body).unwrap_or(false),
                None => false,
            }
        } else {
            self.document.unset_furniture(kind, which)
        };

        self.relayout();
        let said =
            if linked { "Same as previous section: off" } else { "Same as previous section" };
        let response = self.edited(changed, said);
        // Straight back into the header, because that is where the person was.
        self.edit_furniture(kind);
        response
    }

    /// Turns the section's different first page on or off.
    pub(super) fn toggle_different_first_page(&mut self) -> Response {
        let inside = self.editing_furniture;
        if inside.is_some() {
            self.leave_furniture();
        }
        let wanted = !self.document.different_first_page(self.document.section_here());
        let changed = self.document.set_different_first_page(wanted);
        self.relayout();
        let response =
            self.edited(changed, if wanted { "Different first page" } else { "Same first page" });
        if let Some(kind) = inside {
            self.edit_furniture(kind);
        }
        response
    }

    /// Turns different odd and even pages on or off, for the whole document.
    pub(super) fn toggle_different_odd_even(&mut self) -> Response {
        let inside = self.editing_furniture;
        if inside.is_some() {
            self.leave_furniture();
        }
        let wanted = !self.document.different_odd_and_even();
        let changed = self.document.set_different_odd_and_even(wanted);
        self.relayout();
        let response = self.edited(
            changed,
            if wanted { "Different odd and even pages" } else { "Same on every page" },
        );
        if let Some(kind) = inside {
            self.edit_furniture(kind);
        }
        response
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
