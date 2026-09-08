//! The ways of looking at a document, from the View tab.
//!
//! # What a view mode actually changes
//!
//! Mostly how big the paper is. Print layout uses the paper the document says
//! it is on. Web layout uses one sheet as wide as the window and as long as the
//! text. Draft uses the same pages as print with the margins taken away, so the
//! text runs on without the furniture.
//!
//! Two are different. Reading changes what is shown round the document rather
//! than what the paper is. Outline changes where each paragraph sits and which
//! of them are drawn at all — see [`crate::editor::outline`].

use wp_layout::PageMetrics;
use wp_shell::Response;

use super::Editor;

/// Points to the inch, which is what the layout measures in.
const POINTS_PER_INCH: f32 = 72.0;

/// The margin left round the text where the document's own is not used.
///
/// Half an inch: enough that the text does not touch the window, little enough
/// that the window is not mostly margin.
const VIEW_MARGIN: f32 = 36.0;

/// How a document is being looked at.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum View {
    /// The paper the document says it is on, with margins and page edges.
    #[default]
    Print,
    /// One sheet as wide as the window, running on without page breaks.
    Web,
    /// The pages of print layout with the furniture taken away.
    Draft,
    /// The headings alone, each indented to its level, with the body text
    /// under them shown or hidden a level at a time.
    Outline,
    /// Print layout with nothing round it: no ribbon, no rulers, no pane.
    Reading,
}

impl View {
    /// What the status bar and the ribbon call it.
    #[must_use]
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Print => "Print layout",
            Self::Web => "Web layout",
            Self::Draft => "Draft",
            Self::Outline => "Outline",
            Self::Reading => "Read mode",
        }
    }

    /// Whether the window shows its ribbon, rulers and panes in this mode.
    #[must_use]
    pub(super) fn shows_furniture(self) -> bool {
        self != Self::Reading
    }

    /// Whether pages are drawn as sheets with edges and gaps between them.
    #[must_use]
    pub(super) fn shows_paper(self) -> bool {
        matches!(self, Self::Print | Self::Reading)
    }
}

impl Editor {
    /// Changes how the document is being looked at.
    pub(super) fn set_view(&mut self, view: View) -> Response {
        if self.view == view {
            return Response::Ignored;
        }
        self.view = view;
        // Reading mode takes the ribbon and the panes away and gives them back
        // on the way out, so leaving it looks like arriving.
        if view == View::Reading {
            self.remembered_rulers = self.show_rulers;
            self.remembered_navigation = self.show_navigation;
            self.show_rulers = false;
            self.show_navigation = false;
        } else {
            self.show_rulers = self.remembered_rulers;
            self.show_navigation = self.remembered_navigation;
        }

        self.relayout();
        self.clamp_scroll();
        self.reveal_caret();
        self.report(view.label())
    }

    /// The paper the current view lays the document out on.
    pub(super) fn view_metrics(&self) -> PageMetrics {
        let document = PageMetrics::from_document(&self.document);
        match self.view {
            View::Print | View::Reading => document,
            // One sheet as wide as the window and as long as it needs to be.
            // The height is not a guess at how much text there is: it is the
            // largest a page may be, so that nothing ever reaches the end of it
            // and breaks.
            View::Web => PageMetrics {
                width: self.viewport_points().max(document.width / 4.0),
                height: f32::MAX / 4.0,
                margin_top: VIEW_MARGIN,
                margin_right: VIEW_MARGIN,
                margin_bottom: VIEW_MARGIN,
                margin_left: VIEW_MARGIN,
                ..document
            },
            // The pages of print layout, with the margins taken away so the
            // text fills the sheet. The pages themselves are still the
            // document's, so a page break falls where it would when printed.
            // An outline has no pages: it is one long sheet as wide as the
            // window, with room down the left for the levels to step into.
            View::Outline => PageMetrics {
                width: self.viewport_points().max(document.width / 4.0),
                height: f32::MAX / 4.0,
                margin_top: VIEW_MARGIN,
                margin_right: VIEW_MARGIN,
                margin_bottom: VIEW_MARGIN,
                margin_left: VIEW_MARGIN,
                ..document
            },
            View::Draft => PageMetrics {
                margin_top: VIEW_MARGIN / 2.0,
                margin_right: VIEW_MARGIN,
                margin_bottom: VIEW_MARGIN / 2.0,
                margin_left: VIEW_MARGIN,
                ..document
            },
        }
    }

    /// How wide the window is, in points at the current zoom.
    fn viewport_points(&self) -> f32 {
        let per_inch = self.pixels_per_inch();
        if per_inch <= 0.0 {
            return 0.0;
        }
        (self.viewport_width() / per_inch * POINTS_PER_INCH).max(POINTS_PER_INCH)
    }
}

#[cfg(test)]
mod tests {
    use super::View;

    #[test]
    fn reading_is_the_only_mode_that_hides_the_ribbon() {
        for view in [View::Print, View::Web, View::Draft, View::Outline] {
            assert!(view.shows_furniture(), "{}", view.label());
        }
        assert!(!View::Reading.shows_furniture());
    }

    #[test]
    fn only_the_paper_modes_draw_paper() {
        assert!(View::Print.shows_paper());
        assert!(View::Reading.shows_paper());
        assert!(!View::Web.shows_paper());
        assert!(!View::Draft.shows_paper());
        assert!(!View::Outline.shows_paper());
    }

    #[test]
    fn every_mode_says_what_it_is_called() {
        for view in [View::Print, View::Web, View::Draft, View::Reading, View::Outline] {
            assert!(!view.label().is_empty());
        }
    }

    #[test]
    fn a_document_starts_in_print_layout() {
        assert_eq!(View::default(), View::Print);
    }
}

/// Which way the pages are laid out, and therefore which way the view scrolls.
///
/// Word calls this Page Movement, and offers it only in print layout: a web
/// page has no pages to turn and a draft has nothing to turn them into.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Movement {
    /// One page above the next, scrolling down. What a document does.
    #[default]
    Vertical,
    /// One page beside the next, scrolling across. What a book does.
    SideToSide,
}

impl Movement {
    #[must_use]
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Vertical => "Vertical",
            Self::SideToSide => "Side to Side",
        }
    }

    #[must_use]
    pub(super) fn other(self) -> Self {
        match self {
            Self::Vertical => Self::SideToSide,
            Self::SideToSide => Self::Vertical,
        }
    }
}

impl Editor {
    /// Turns the pages sideways, or puts them back one above the next.
    pub(super) fn toggle_movement(&mut self) -> Response {
        // Only print layout has paper to turn. Asking for it elsewhere puts the
        // view back into print layout first, which is what Word's button does.
        if !self.view.shows_paper() {
            self.set_view(View::Print);
        }
        // Which page is being looked at is asked before the axis changes, while
        // the scroll still measures what it was measuring.
        let looking_at = self.visible_page();

        self.movement = self.movement.other();
        self.scroll = 0.0;
        self.scroll_to_page(looking_at);
        self.needs_redraw = true;
        let label = self.movement.label();
        self.report(&format!("Page movement: {label}"))
    }

    /// Whether the pages run across the window rather than down it.
    #[must_use]
    pub(super) fn is_side_to_side(&self) -> bool {
        self.movement == Movement::SideToSide && self.view.shows_paper()
    }

    /// How much of the scroll goes downwards.
    ///
    /// All of it, until the pages are turned sideways — and then none, because
    /// [`Editor::page_origin`] has already taken it off the other axis.
    #[must_use]
    pub(super) fn scroll_down(&self) -> f32 {
        if self.is_side_to_side() {
            0.0
        } else {
            self.scroll
        }
    }
}
