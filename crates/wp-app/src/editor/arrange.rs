//! Where a drawing sits and what the text does about it, from the Layout tab.
//!
//! # What these commands act on
//!
//! The drawing nearest the caret. Word acts on the one that is selected, and
//! selecting a drawing means clicking it — which needs handles, a selection
//! that is not a stretch of text, and a way to drag. None of that is here yet,
//! so the caret stands in for it: put the caret beside a drawing and these
//! commands are about that drawing.

use wp_docx::anchor::{Anchor, Placement, Wrap};
use wp_shell::Response;

use crate::chrome::{Choice, Command, Popup};

use super::Editor;

/// Where a drawing can be put across the page.
const POSITIONS: &[(&str, &str)] = &[("Left", "left"), ("Centre", "center"), ("Right", "right")];

impl Editor {
    /// Drops open what the text can do about a drawing.
    pub(super) fn open_wrapping(&mut self) -> Response {
        if self.popup.as_ref().is_some_and(|popup| popup.choice == Choice::Wrap) {
            self.popup = None;
            self.needs_redraw = true;
            return Response::Redraw;
        }
        if self.document.shape_here().is_none() {
            return self.report("Put the caret beside a shape or a picture first");
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::WrapText) else {
            return Response::Ignored;
        };

        let here = self.document.shape_here().and_then(|shape| shape.anchor);
        let mut items = vec!["In Line with Text".to_owned()];
        items.extend(Wrap::ALL.iter().map(|wrap| wrap.label().to_owned()));

        let current = match here {
            None => Some(0),
            Some(anchor) => Wrap::ALL.iter().position(|wrap| *wrap == anchor.wrap).map(|at| at + 1),
        };
        self.popup = Some(Popup::new(Choice::Wrap, items, current, left, top, 240.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Sets what the text does about the drawing at the caret.
    pub(super) fn choose_wrapping(&mut self, index: usize) -> Response {
        self.popup = None;
        let Some(mut shape) = self.document.shape_here() else { return Response::Ignored };

        // The first line puts the drawing back in the line of text; the rest
        // are the ways of floating.
        let (anchor, note) = match index.checked_sub(1) {
            None => (None, "In line with text".to_owned()),
            Some(at) => {
                let Some(wrap) = Wrap::ALL.get(at).copied() else { return Response::Ignored };
                let anchor = Anchor {
                    wrap,
                    // Behind and in front are the same wrapping — none — and
                    // differ only in which is drawn over which.
                    behind_text: wrap == Wrap::None,
                    ..shape.anchor.clone().unwrap_or_default()
                };
                (Some(anchor), format!("Wrap: {}", wrap.label()))
            }
        };

        shape.anchor = anchor;
        let changed = self.document.replace_shape_here(&shape);
        self.relayout();
        self.edited(changed, &note)
    }

    /// Drops open where a drawing can sit across the page.
    pub(super) fn open_position(&mut self) -> Response {
        if self.popup.as_ref().is_some_and(|popup| popup.choice == Choice::Position) {
            self.popup = None;
            self.needs_redraw = true;
            return Response::Redraw;
        }
        if self.document.shape_here().is_none() {
            return self.report("Put the caret beside a shape or a picture first");
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::Position) else {
            return Response::Ignored;
        };

        let items = POSITIONS.iter().map(|(label, _)| (*label).to_owned()).collect();
        self.popup = Some(Popup::new(Choice::Position, items, None, left, top, 200.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Moves the drawing at the caret across the page.
    pub(super) fn choose_position(&mut self, index: usize) -> Response {
        self.popup = None;
        let Some(mut shape) = self.document.shape_here() else { return Response::Ignored };
        let Some((label, edge)) = POSITIONS.get(index).copied() else { return Response::Ignored };

        // A drawing has to float before it can be put anywhere, so one that was
        // in the line starts floating with the wrapping Word gives it.
        let mut anchor = shape.anchor.clone().unwrap_or_default();
        anchor.horizontal = Placement::Aligned(edge.to_owned());
        shape.anchor = Some(anchor);

        let changed = self.document.replace_shape_here(&shape);
        self.relayout();
        self.edited(changed, &format!("Position: {label}"))
    }

    /// Puts the drawing at the caret in front of the text, or behind it.
    pub(super) fn set_shape_depth(&mut self, behind: bool) -> Response {
        let Some(mut shape) = self.document.shape_here() else {
            return self.report("Put the caret beside a shape or a picture first");
        };
        let mut anchor = shape.anchor.clone().unwrap_or_default();
        anchor.wrap = Wrap::None;
        anchor.behind_text = behind;
        shape.anchor = Some(anchor);

        let changed = self.document.replace_shape_here(&shape);
        self.relayout();
        self.edited(changed, if behind { "Behind text" } else { "In front of text" })
    }
}
