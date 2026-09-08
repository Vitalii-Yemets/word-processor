//! The document as an outline: headings alone, or headings and what is under
//! them, a level at a time.
//!
//! # Why this needs no map between the outline and the document
//!
//! Word's outline view is the document, laid out differently: each paragraph
//! indented to the depth of its heading, and anything below the level being
//! shown left out. Nothing is renumbered and nothing is copied, so a caret in
//! the outline is a caret in the document and typing is typing — which is why
//! there is no map here to go wrong.
//!
//! The one thing that has to be looked after is a caret in a paragraph the
//! level has just hidden. It is moved to the nearest paragraph still on show,
//! because a caret nobody can see is a caret typing into the dark.

use wp_shell::Response;

use crate::chrome::{Choice, Command, Popup};

use super::views::View;
use super::Editor;

/// Every level a heading can be, and then everything.
///
/// Ten is what the engine reads as "all levels": nine headings and the body
/// text under them.
pub(super) const ALL_LEVELS: u8 = 10;

impl Editor {
    /// Drops open the levels the outline can be shown down to.
    pub(super) fn open_outline(&mut self) -> Response {
        if self.close_popup_if(Choice::OutlineLevel) {
            return Response::Redraw;
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::OutlineView) else {
            return Response::Ignored;
        };

        let mut items: Vec<String> = (1..=9).map(|level| format!("Level {level}")).collect();
        items.push("All levels".to_owned());
        if self.view == View::Outline {
            items.push("Close outline view".to_owned());
        }
        let current = (self.view == View::Outline)
            .then(|| usize::from(self.outline_depth.min(ALL_LEVELS)) - 1);

        self.popup = Some(Popup::new(Choice::OutlineLevel, items, current, left, top, 220.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Shows the outline down to the level that was chosen.
    pub(super) fn choose_outline_level(&mut self, index: usize) -> Response {
        self.popup = None;

        // Past the ten levels is the way out, which is only offered while the
        // outline is being shown.
        if index >= usize::from(ALL_LEVELS) {
            self.set_view(View::Print);
            return self.report("Print layout");
        }

        self.outline_depth = (index + 1) as u8;
        if self.view == View::Outline {
            // Already in the outline, so only the depth changes.
            self.relayout();
            self.snap_caret_into_view();
            self.needs_redraw = true;
        } else {
            self.set_view(View::Outline);
            self.snap_caret_into_view();
        }

        let named = if self.outline_depth >= ALL_LEVELS {
            "all levels".to_owned()
        } else {
            format!("level {}", self.outline_depth)
        };
        self.report(&format!("Outline: {named}"))
    }

    /// How deep the outline goes, for the engine to be told.
    ///
    /// Nothing at all unless the outline is what is being looked at.
    #[must_use]
    pub(super) fn outline_for_layout(&self) -> Option<u8> {
        (self.view == View::Outline).then_some(self.outline_depth.clamp(1, ALL_LEVELS))
    }

    /// Moves the caret to the nearest paragraph still on show.
    ///
    /// Only ever does anything in the outline, where a level can hide the
    /// paragraph the caret was in.
    pub(super) fn snap_caret_into_view(&mut self) {
        if self.caret_is_visible() {
            return;
        }
        let caret = self.document.caret();
        let mut best: Option<wp_docx::TextPosition> = None;
        for page in &self.pages {
            for line in &page.lines {
                let position = wp_docx::TextPosition::new(line.paragraph, line.start_offset);
                // The last paragraph at or before the caret, or failing that
                // the first one there is.
                if line.paragraph <= caret.paragraph || best.is_none() {
                    best = Some(position);
                }
            }
        }
        if let Some(position) = best {
            self.document.set_caret(position);
            self.reveal_caret();
        }
    }

    /// Whether the caret is in a paragraph that is being drawn.
    #[must_use]
    fn caret_is_visible(&self) -> bool {
        let caret = self.document.caret();
        self.pages
            .iter()
            .any(|page| page.lines.iter().any(|line| line.paragraph == caret.paragraph))
    }
}
