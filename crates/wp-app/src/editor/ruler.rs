//! Dragging the markers and the margins on the rulers.
//!
//! # Why a drag and not a box to type in
//!
//! An indent is a distance, and a distance is easier to judge against the text
//! it moves than to name in twentieths of a point. Dragging is not a shortcut
//! for the dialog — it is the better way round, which is why Word puts the
//! markers on the ruler and hides the numbers in a dialog nobody opens.
//!
//! # What is being dragged
//!
//! Four markers and four margins. The markers change the paragraph the caret is
//! in; the margins change the whole section, because a margin belongs to the
//! page and not to any paragraph on it.

use wp_docx::page::MARGIN_PRESETS;
use wp_shell::{Modifiers, Response};

use wp_docx::model::{TabAlignment, TabLeader, TabStop};

use crate::chrome::rulers::{self, Hit, PlacedStop, VerticalHit};

use super::Editor;

/// Twentieths of a point in an inch.
const TWIPS_PER_INCH: f32 = 1440.0;

/// What a drag lands on: the ruler's ticks, an eighth of an inch apart.
///
/// Word snaps to the same marks, and for the same reason — a paragraph indented
/// by 0.37 of an inch is nobody's intention. Holding Alt turns it off, which is
/// also Word's arrangement.
const SNAP: f32 = TWIPS_PER_INCH / 8.0;

/// The least a page may have between its text and its edge, in twips.
///
/// A quarter of an inch: nearer than that and most printers cannot print it, so
/// dragging a margin further would be dragging text off the paper.
const LEAST_MARGIN: i32 = 360;

/// The least room the text may be left, so the two margins cannot meet.
const LEAST_TEXT: i32 = 720;

/// How far from a stop a press or a drag still counts as that stop, in twips.
///
/// An eighth of an inch: about the reach of the marker on screen, and less
/// than the distance two stops would sensibly be put apart.
const STOP_SLACK: i32 = 180;

/// How far past the ruler the pointer must go for a dragged stop to be let go
/// of altogether, in pixels.
const STOP_ESCAPE: f32 = 12.0;

/// What is being dragged, and what it was when the drag began.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Grab {
    Horizontal(Hit),
    Vertical(VerticalHit),
}

impl Editor {
    /// Takes hold of whatever on the rulers a press landed on.
    ///
    /// Returns whether it took hold of anything, which is what tells the press
    /// handler to stop looking.
    pub(super) fn press_on_ruler(&mut self, x: i32, y: i32) -> bool {
        if !self.show_rulers {
            return false;
        }
        let (horizontal, vertical) = self.ruler_measurements();
        let stops = self.ruler_stops();

        let grab =
            rulers::hit_horizontal(horizontal, &stops, x, y).map(Grab::Horizontal).or_else(|| {
                rulers::hit_vertical(self.pane_width(), vertical, x, y).map(Grab::Vertical)
            });
        let Some(grab) = grab else { return false };

        // Two of them are done with on the press and never dragged: the box
        // that chooses the kind of stop, and the face of the ruler, where a
        // press puts one down.
        match grab {
            Grab::Horizontal(Hit::StopSelector) => {
                self.cycle_tab_kind();
                return true;
            }
            Grab::Horizontal(Hit::Face) => {
                self.place_tab_stop(x);
                return true;
            }
            Grab::Horizontal(Hit::TabStop(index)) => {
                // Remembered in twips rather than by its place in the list,
                // because the list is sorted and a stop dragged past another
                // changes places with it.
                self.ruler_stop_at = self.document.tab_stops_here().get(index).map(|s| s.position);
            }
            _ => {}
        }

        // The whole drag is one change to undo, however many times the pointer
        // moves while it is under way.
        self.document.begin_gesture();
        self.ruler_drag = Some(grab);
        true
    }

    /// Carries on a drag that is already under way.
    pub(super) fn drag_ruler(&mut self, x: i32, y: i32, modifiers: Modifiers) -> Response {
        let Some(grab) = self.ruler_drag else { return Response::Ignored };
        // Alt drags freely; without it the marker lands on a tick.
        let snap = !modifiers.alt;

        let changed = match grab {
            Grab::Horizontal(Hit::TabStop(_)) => self.drag_tab_stop(x, snap),
            Grab::Horizontal(Hit::StopSelector | Hit::Face) => false,
            Grab::Horizontal(hit) => self.drag_horizontal(hit, x, snap),
            Grab::Vertical(hit) => self.drag_vertical(hit, y, snap),
        };
        if !changed {
            return Response::Ignored;
        }

        self.relayout();
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Lets go at the end of a drag.
    pub(super) fn release_ruler(&mut self) {
        // A stop dragged clear of the ruler is being thrown away.
        self.drop_tab_stop();
        if self.ruler_drag.take().is_some() {
            self.document.end_gesture();
            self.update_title();
        }
    }

    /// Whether a drag is under way, so a move is not read as anything else.
    #[must_use]
    pub(super) fn dragging_ruler(&self) -> bool {
        self.ruler_drag.is_some()
    }

    /// The tab stops of the caret's paragraph, where the ruler draws them.
    ///
    /// A stop that only cancels one a style put there is left out: it has no
    /// place on the ruler, because there is nothing at that place to see.
    #[must_use]
    pub(super) fn ruler_stops(&self) -> Vec<PlacedStop> {
        let (horizontal, _) = self.ruler_measurements();
        let text_left = horizontal.page_left + horizontal.margin_left;
        let per_twip = horizontal.pixels_per_inch / TWIPS_PER_INCH;
        self.document
            .tab_stops_here()
            .into_iter()
            .filter(|stop| stop.alignment != TabAlignment::Clear)
            .map(|stop| PlacedStop {
                x: text_left + stop.position as f32 * per_twip,
                alignment: stop.alignment,
            })
            .collect()
    }

    /// Steps round the kinds of stop, which is what clicking the box at the
    /// left end of the ruler does.
    ///
    /// Word's order, and Word's box. The two indent kinds it also offers are
    /// left out here, because both indents already have a marker of their own
    /// on the ruler that does the same thing more plainly.
    fn cycle_tab_kind(&mut self) -> Response {
        self.tab_kind = match self.tab_kind {
            TabAlignment::Start => TabAlignment::Center,
            TabAlignment::Center => TabAlignment::End,
            TabAlignment::End => TabAlignment::Decimal,
            TabAlignment::Decimal => TabAlignment::Bar,
            _ => TabAlignment::Start,
        };
        self.needs_redraw = true;
        let name = match self.tab_kind {
            TabAlignment::Center => "Center tab",
            TabAlignment::End => "Right tab",
            TabAlignment::Decimal => "Decimal tab",
            TabAlignment::Bar => "Bar tab",
            _ => "Left tab",
        };
        self.report(name)
    }

    /// Puts a stop of the chosen kind where the ruler was clicked.
    fn place_tab_stop(&mut self, x: i32) -> Response {
        let (horizontal, _) = self.ruler_measurements();
        let position =
            self.twips_from_pixels(x as f32 - horizontal.page_left - horizontal.margin_left, true);
        if position <= 0 {
            return Response::Ignored;
        }
        let stop = TabStop { position, alignment: self.tab_kind, leader: TabLeader::None };
        let changed = self.document.add_tab_stop_here(stop);
        self.relayout();
        self.edited(changed, "Tab stop")
    }

    /// Moves the stop a drag has hold of.
    fn drag_tab_stop(&mut self, x: i32, snap: bool) -> bool {
        let Some(from) = self.ruler_stop_at else { return false };
        let (horizontal, _) = self.ruler_measurements();
        let to = self
            .twips_from_pixels(x as f32 - horizontal.page_left - horizontal.margin_left, snap)
            .max(0);
        if to == from {
            return false;
        }
        // The one being dragged is the one nearest where it was last put, not
        // where the drag began: it has been moving with the pointer.
        if !self.document.move_tab_stop_here(from, to, STOP_SLACK) {
            return false;
        }
        self.ruler_stop_at = Some(to);
        true
    }

    /// Takes the dragged stop away if the pointer left the ruler with it.
    ///
    /// Dragging a marker off a ruler to be rid of it is what every program with
    /// a ruler has always meant, and it is the only way to remove a stop with
    /// the mouse.
    fn drop_tab_stop(&mut self) {
        let Some(position) = self.ruler_stop_at.take() else { return };
        let top = self.ruler_top();
        let below = self.pointer_y > top + rulers::HORIZONTAL_HEIGHT + STOP_ESCAPE;
        let above = self.pointer_y < top - STOP_ESCAPE;
        if !(below || above) {
            return;
        }
        if self.document.remove_tab_stop_here(position, STOP_SLACK) {
            self.relayout();
            self.needs_redraw = true;
            self.status = "Tab stop removed".to_owned();
        }
    }

    /// Moves one of the markers or margins on the top ruler.
    fn drag_horizontal(&mut self, hit: Hit, x: i32, snap: bool) -> bool {
        let (horizontal, _) = self.ruler_measurements();
        let (start, first_line, end) = self.document.indents_here();
        let (top_margin, right, bottom, left) = self.document.page_margins();

        // Where the pointer is, measured from the left margin outwards, which
        // is what every indent is measured from.
        let from_text_left =
            self.twips_from_pixels(x as f32 - horizontal.page_left - horizontal.margin_left, snap);
        // And from the right margin, which is what the right indent is.
        let from_text_right = self.twips_from_pixels(
            horizontal.page_left + horizontal.page_width - horizontal.margin_right - x as f32,
            snap,
        );
        // Margins are measured from the edge of the paper instead.
        let from_left_edge = self.twips_from_pixels(x as f32 - horizontal.page_left, snap);
        let from_right_edge =
            self.twips_from_pixels(horizontal.page_left + horizontal.page_width - x as f32, snap);
        let page_width = self.twips_from_pixels(horizontal.page_width, false);

        match hit {
            // The top triangle moves the first line alone, so what changes is
            // its offset from where the rest of the paragraph starts.
            Hit::FirstLine => self.document.set_indents_here(start, from_text_left - start, end),
            // The bottom triangle moves every line but the first, which means
            // the indent moves and the first line stays where it was.
            Hit::Hanging => {
                let moved = from_text_left;
                self.document.set_indents_here(moved, first_line + start - moved, end)
            }
            // The square moves the whole paragraph: the first line keeps its
            // offset, so the shape of the paragraph is unchanged.
            Hit::LeftIndent => self.document.set_indents_here(from_text_left, first_line, end),
            Hit::RightIndent => self.document.set_indents_here(start, first_line, from_text_right),
            Hit::LeftMargin => {
                let wanted = held_between(from_left_edge, page_width - right);
                self.set_margins_reporting(top_margin, right, bottom, wanted)
            }
            Hit::RightMargin => {
                let wanted = held_between(from_right_edge, page_width - left);
                self.set_margins_reporting(top_margin, wanted, bottom, left)
            }
            // The three that a press deals with and a drag never sees: the
            // box that chooses the kind of stop, the face where one is put
            // down, and a stop being carried, which moves through
            // [`Editor::drag_tab_stop`].
            Hit::StopSelector | Hit::TabStop(_) | Hit::Face => false,
        }
    }

    /// Moves one of the margins on the side ruler.
    fn drag_vertical(&mut self, hit: VerticalHit, y: i32, snap: bool) -> bool {
        let (_, vertical) = self.ruler_measurements();
        let (top, right, bottom, left) = self.document.page_margins();
        let page_height = self.twips_from_pixels(vertical.page_height, false);

        match hit {
            VerticalHit::TopMargin => {
                let dragged = self.twips_from_pixels(y as f32 - vertical.page_top, snap);
                let wanted = held_between(dragged, page_height - bottom);
                self.set_margins_reporting(wanted, right, bottom, left)
            }
            VerticalHit::BottomMargin => {
                let dragged = self
                    .twips_from_pixels(vertical.page_top + vertical.page_height - y as f32, snap);
                let wanted = held_between(dragged, page_height - top);
                self.set_margins_reporting(top, right, wanted, left)
            }
        }
    }

    /// Sets the margins and says in the status bar what they became.
    ///
    /// A margin has no marker to read a measurement off, so the number is the
    /// only feedback there is — and a drag without feedback is a guess.
    fn set_margins_reporting(&mut self, top: i32, right: i32, bottom: i32, left: i32) -> bool {
        if !self.document.set_page_margins(top, right, bottom, left) {
            return false;
        }
        let named = MARGIN_PRESETS
            .iter()
            .find(|(_, preset_top, preset_right, preset_bottom, preset_left)| {
                (*preset_top, *preset_right, *preset_bottom, *preset_left)
                    == (top, right, bottom, left)
            })
            .map(|(name, ..)| *name);
        self.status = match named {
            Some(name) => format!("Margins: {name}"),
            None => format!(
                "Margins: {} top, {} bottom, {} left, {} right",
                inches(top),
                inches(bottom),
                inches(left),
                inches(right)
            ),
        };
        true
    }

    /// Turns a distance on screen into one in the document.
    fn twips_from_pixels(&self, pixels: f32, snap: bool) -> i32 {
        let per_inch = self.pixels_per_inch();
        if per_inch <= 0.0 {
            return 0;
        }
        let twips = pixels / per_inch * TWIPS_PER_INCH;
        if snap {
            return (twips / SNAP).round() as i32 * SNAP as i32;
        }
        twips.round() as i32
    }

    /// Whether a point is on either ruler.
    ///
    /// The strip across the top and the one down the side, which are the two
    /// places a double click means "show me the page setup".
    #[must_use]
    pub(super) fn on_a_ruler(&self, x: i32, y: i32) -> bool {
        let (px, py) = (x as f32, y as f32);
        let top = self.ruler_top();
        if py >= top && py < top + crate::chrome::HORIZONTAL_HEIGHT {
            return px >= self.pane_width();
        }
        py >= self.whole_content_top()
            && px >= self.pane_width()
            && px < self.pane_width() + crate::chrome::VERTICAL_WIDTH
    }

    /// The top of the horizontal ruler, which is where its hit-testing starts.
    #[must_use]
    pub(super) fn ruler_top(&self) -> f32 {
        self.ribbon_bottom() + self.find_bar_height()
    }
}

/// Keeps a dragged margin on the paper.
///
/// `across` is the room the page has left once the opposite margin is taken
/// off. The upper limit is held at or above the lower one so that a page too
/// small to hold both margins clamps rather than panics — which a page of a
/// few millimetres, or one being dragged while the zoom changes, otherwise
/// would.
fn held_between(wanted: i32, across: i32) -> i32 {
    let most = (across - LEAST_TEXT).max(LEAST_MARGIN);
    wanted.clamp(LEAST_MARGIN, most)
}

/// A measurement written the way a ruler is read.
fn inches(twips: i32) -> String {
    // Rounded here rather than by the formatting, which breaks a tie towards
    // the even digit and would write an eighth of an inch as 0.12.
    let value = (twips as f32 / TWIPS_PER_INCH * 100.0).round() / 100.0;
    // Two places is as fine as the ruler can be read; a trailing zero is
    // noise, so 1.50 is written 1.5 and 1.00 is written 1.
    let text = format!("{value:.2}");
    let text = text.trim_end_matches('0').trim_end_matches('.').to_owned();
    format!("{text}\"")
}

impl Editor {
    /// Opens the Tabs dialog on the stop a double click landed on.
    ///
    /// What Word does. There was a menu here instead, offering the same
    /// choices, because the dialog did not exist: see **C3**. It does now, and
    /// a menu Word does not have is a menu somebody who knows Word will not
    /// look for.
    pub(super) fn open_tab_stop_menu(&mut self, index: usize, _x: i32, _y: i32) -> Response {
        let stops = self.document.tab_stops_here();
        let Some(stop) = stops.get(index).copied() else { return Response::Ignored };
        let dialog = self.tabs_dialog(&stops, Some(stop));
        self.ask(super::dialogs::Asking::TabStops, dialog)
    }
}

#[cfg(test)]
mod tests {
    use super::inches;

    #[test]
    fn a_whole_inch_is_written_without_a_fraction() {
        assert_eq!(inches(1440), "1\"");
    }

    #[test]
    fn a_half_is_written_with_one_place_rather_than_two() {
        assert_eq!(inches(720), "0.5\"");
    }

    #[test]
    fn an_eighth_is_written_with_two() {
        assert_eq!(inches(180), "0.13\"");
    }

    #[test]
    fn nothing_is_written_as_nothing() {
        assert_eq!(inches(0), "0\"");
    }
}
