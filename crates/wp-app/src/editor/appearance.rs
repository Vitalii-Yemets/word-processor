//! Line numbers, hyphenation and the colour of the page.

use wp_docx::appearance::{LineNumbers, Restart};
use wp_shell::Response;

use crate::chrome::{Choice, Command, Popup};

use super::Editor;

/// The ways of numbering the lines, in the order Word lists them.
const NUMBERING: &[(&str, Option<Restart>)] = &[
    ("None", None),
    ("Continuous", Some(Restart::Continuous)),
    ("Restart Each Page", Some(Restart::NewPage)),
    ("Restart Each Section", Some(Restart::NewSection)),
];

/// How strongly a marked stretch is washed over with its editor's colour.
const WASH: u8 = 0x30;

/// And how far the lips of a bracket reach into the stretch, in pixels.
const BRACKET_LIP: i32 = 3;

impl Editor {
    /// Drops open the ways of numbering the lines.
    pub(super) fn open_line_numbers(&mut self) -> Response {
        if self.close_popup_if(Choice::LineNumbers) {
            return Response::Redraw;
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::LineNumbers) else {
            return Response::Ignored;
        };

        let here = self.document.line_numbers().map(|numbers| numbers.restart);
        let current = NUMBERING.iter().position(|(_, restart)| *restart == here);
        let items = NUMBERING.iter().map(|(label, _)| (*label).to_owned()).collect();
        self.popup = Some(Popup::new(Choice::LineNumbers, items, current, left, top, 220.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Numbers the lines whichever way was chosen.
    pub(super) fn choose_line_numbers(&mut self, index: usize) -> Response {
        self.popup = None;
        let Some((label, restart)) = NUMBERING.get(index).copied() else {
            return Response::Ignored;
        };

        // Whatever else was set — how often to print a number, where to start —
        // is kept, so picking a different restart does not undo it.
        let wanted = restart.map(|restart| LineNumbers {
            restart,
            ..self.document.line_numbers().unwrap_or_default()
        });
        let changed = self.document.set_line_numbers(wanted);
        self.relayout();
        self.edited(changed, &format!("Line numbers: {label}"))
    }

    /// Drops open the two ways of hyphenating.
    pub(super) fn open_hyphenation(&mut self) -> Response {
        if self.close_popup_if(Choice::Hyphenation) {
            return Response::Redraw;
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::Hyphenation) else {
            return Response::Ignored;
        };

        let current = usize::from(self.document.automatic_hyphenation());
        let items = vec!["None".to_owned(), "Automatic".to_owned()];
        self.popup = Some(Popup::new(Choice::Hyphenation, items, Some(current), left, top, 200.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Turns hyphenation on or off.
    pub(super) fn choose_hyphenation(&mut self, index: usize) -> Response {
        self.popup = None;
        let on = index == 1;
        let changed = self.document.set_automatic_hyphenation(on);
        // Nothing about the layout changes yet — this program does not break
        // words itself — but the document says so, and Word will.
        self.edited(changed, if on { "Hyphenation: automatic" } else { "Hyphenation: none" })
    }

    /// Closes an open list if it is the one asked about.
    ///
    /// Pressing a button whose list is already open closes it, which is what
    /// every list in every ribbon does.
    pub(super) fn close_popup_if(&mut self, choice: Choice) -> bool {
        if self.popup.as_ref().is_some_and(|popup| popup.choice == choice) {
            self.popup = None;
            self.needs_redraw = true;
            return true;
        }
        false
    }

    /// The colour the paper is painted.
    ///
    /// A document that names its own page colour gets it, whatever the theme
    /// says: that colour is part of the document, and a dark window is not a
    /// reason to show a different one.
    pub(super) fn page_paint(&self) -> wp_raster::Color {
        self.document
            .page_color()
            .as_deref()
            .and_then(wp_raster::Color::from_hex)
            .unwrap_or(self.theme.page)
    }
}

impl Editor {
    /// Draws the faint tag at each end of every content control.
    ///
    /// Word's boundary, and the reason it has one: a control is a box
    /// somebody is meant to fill in, and a box nobody can see is a box
    /// nobody knows to fill in. Two short uprights with a lip at the top and
    /// the bottom, which is Word's shape and reads as a bracket without
    /// being a letter.
    pub(super) fn draw_control_edges(&mut self) {
        let controls = self.document.controls();
        if controls.is_empty() {
            return;
        }

        // Where each end is on the page, gathered before anything is drawn so
        // that the canvas is not borrowed while the pages are being read.
        let mut edges: Vec<(f32, f32, f32)> = Vec::new();
        for index in 0..self.pages.len() {
            let (origin_x, origin_y) = self.page_origin(index);
            let top = self.content_top() + origin_y - self.scroll_down();
            for control in &controls {
                for (at, ending) in [(control.start, false), (control.end, true)] {
                    // A range of no width at the position itself, which is
                    // what asks the layout where that position is.
                    let rects = self.pages[index].selection_rects(
                        at,
                        wp_docx::TextPosition::new(at.paragraph, at.offset + 1),
                    );
                    let Some((x, y, width, height)) = rects.first().copied() else { continue };
                    let edge = if ending { x + width } else { x };
                    let _ = ending;
                    edges.push((origin_x + edge, top + y, height));
                }
            }
        }

        let colour = self.theme.control_edge;
        for (x, y, height) in edges {
            let height = height.max(2.0);
            // The upright.
            self.canvas.fill_rect(x as i32, y as i32, 1, height.ceil() as i32, colour);
            // And the two lips, one at each end of it, which are what make it
            // a tag rather than a caret.
            for lip in [y, y + height - 1.0] {
                self.canvas.fill_rect(x as i32, lip as i32, 3, 1, colour);
            }
        }
    }

    /// Shades every stretch of the document that has its own rule about who
    /// may edit it, and puts a bracket at each end of it.
    ///
    /// Word does this and it is not decoration: a restricted document with an
    /// exception in it looks exactly like a restricted document without one,
    /// and a person would have to try typing in every paragraph to find the
    /// one they are allowed in. The shading is the answer to "where may I
    /// write".
    ///
    /// Each stretch is drawn in the colour of whoever may edit it — the same
    /// colour their tracked changes are drawn in, out of
    /// [`wp_layout::author_color`] — because a document three people may edit
    /// different parts of reads as three people that way and as one shaded
    /// mess otherwise. The brackets are what say where a stretch begins and
    /// ends when two of them touch, which shading alone cannot.
    ///
    /// The whole of it is behind Word's own tick box: shading is help while a
    /// person is looking for where they may type and clutter once they have
    /// found it.
    pub(super) fn draw_marked_regions(&mut self) {
        if !self.regions_are_highlighted() {
            return;
        }
        let marked = self.document.locked_regions();
        if marked.is_empty() {
            return;
        }

        let mut bands: Vec<(f32, f32, f32, f32, wp_raster::Color)> = Vec::new();
        let mut brackets: Vec<(f32, f32, f32, bool, wp_raster::Color)> = Vec::new();
        for index in 0..self.pages.len() {
            let (origin_x, origin_y) = self.page_origin(index);
            let top = self.content_top() + origin_y - self.scroll_down();
            for stretch in &marked {
                let colour = Self::region_colour(stretch);
                let rects = self.pages[index].selection_rects(stretch.start, stretch.end);
                for (at, (x, y, width, height)) in rects.iter().copied().enumerate() {
                    // The shading is the person's colour laid on thinly: at
                    // full strength it would be a highlighter pen over the
                    // words rather than a wash behind them.
                    let wash = wp_raster::Color::rgba(colour.red, colour.green, colour.blue, WASH);
                    bands.push((origin_x + x, top + y, width, height, wash));
                    // A bracket at each end of the stretch, which on a
                    // stretch running over several lines means the first
                    // line's start and the last line's end.
                    if at == 0 {
                        brackets.push((origin_x + x, top + y, height, true, colour));
                    }
                    if at + 1 == rects.len() {
                        brackets.push((origin_x + x + width, top + y, height, false, colour));
                    }
                }
            }
        }

        for (x, y, width, height, colour) in bands {
            self.canvas.fill_rect(
                x as i32,
                y as i32,
                width.ceil() as i32,
                height.ceil() as i32,
                colour,
            );
        }
        for (x, y, height, opening, colour) in brackets {
            self.draw_region_bracket(x, y, height, opening, colour);
        }
    }

    /// One of those brackets: an upright with a lip at each end of it, turned
    /// the way the stretch runs.
    ///
    /// Drawn rather than written for the reason the submenu arrow is: the
    /// characters for these are ordinary brackets, but a bracket set in the
    /// document's font at the document's size would sit on the baseline and
    /// be read as part of the words. This is a mark on the page, the height
    /// of the line, and nothing in the text.
    fn draw_region_bracket(
        &mut self,
        x: f32,
        y: f32,
        height: f32,
        opening: bool,
        colour: wp_raster::Color,
    ) {
        let height = height.max(2.0).ceil() as i32;
        let x = x.round() as i32;
        let y = y.round() as i32;
        self.canvas.fill_rect(x, y, 1, height, colour);
        // The lips point into the stretch, so an opening bracket's reach to
        // the right and a closing one's to the left.
        let lip = if opening { x } else { x - BRACKET_LIP + 1 };
        self.canvas.fill_rect(lip, y, BRACKET_LIP, 1, colour);
        self.canvas.fill_rect(lip, y + height - 1, BRACKET_LIP, 1, colour);
    }

    /// Locks the selection so that only this author may change it, or unlocks
    /// the stretch the caret is in.
    ///
    /// Word's Block Authors, which is one button that does both: in a document
    /// several people have open, you lock what you are working on and let it go
    /// again when you are done.
    /// Word's exception to a restriction: the stretch that stays editable
    /// while the rest of the document is shut.
    ///
    /// The same pair of markers Block Authors writes, naming everybody rather
    /// than one person — see [`wp_docx::permissions`], where the one rule
    /// behind both is set out.
    pub(super) fn toggle_everyone(&mut self) -> Response {
        if let Some(marked) = self.document.locked_here() {
            if marked.for_everyone() {
                let changed = self.document.unblock_authors();
                self.relayout();
                return self.edited(changed, "Exception taken off");
            }
            return self.report("This stretch already belongs to somebody");
        }

        let changed = self.document.allow_everyone();
        if !changed {
            return self.report("Select the text everyone is to be allowed to edit first");
        }
        self.relayout();
        self.edited(changed, "Everyone may edit this")
    }

    pub(super) fn toggle_block_authors(&mut self) -> Response {
        if self.document.locked_here().is_some() {
            let changed = self.document.unblock_authors();
            self.relayout();
            return self.edited(changed, "Unblocked");
        }

        // Who is doing the locking: whoever the document says is writing it,
        // which is the same name a tracked change is signed with.
        let author = super::files::user_name();
        let changed = self.document.block_authors(&author);
        if !changed {
            return self.report("Select the text to block other authors from first");
        }
        self.relayout();
        self.edited(changed, &format!("Blocked for everybody but {author}"))
    }
}
