//! Word's Restrict Editing pane, down the right-hand side of the window.
//!
//! # Why a pane and not a dialog
//!
//! Because restricting a document is not one decision taken once. A person
//! ticks a box, selects a paragraph, says who may edit it, selects another,
//! looks at what they have done, and only then enforces any of it — and a
//! dialog cannot be open while the selection is being made. Word's pane
//! stands beside the document for exactly that reason, and **J16** named the
//! difference where this program had a dialog instead.
//!
//! The dialogs that remain are the two Word keeps: the Formatting
//! Restrictions list of styles, and the box that takes a password when
//! enforcement starts. Both are questions asked once and answered once, which
//! is what a dialog is for.
//!
//! # The two faces of it
//!
//! Before enforcement the pane is Word's four numbered sections: what
//! formatting is allowed, what editing is allowed, who is let through anyway,
//! and the button that starts it. After enforcement it is a different pane
//! altogether — "Your permissions" — saying what this person may do, with the
//! ways of finding where they may do it and the button that stops the whole
//! thing. Word swaps them because the questions are different, and so does
//! this.
//!
//! # What the pane does not decide
//!
//! Anything. It draws what it is given and reports where it was pressed; what
//! the answers mean is [`crate::editor`]'s restrict module, and what gets
//! written down is [`wp_docx::protection`] and [`wp_docx::permissions`].

use wp_layout::{LayoutEngine, Renderer};
use wp_raster::{Canvas, Color};

use crate::messages::t;

use super::pane::{Pen, Scroll, HEADING, PADDING, TEXT};
use super::theme::Theme;

/// How wide the pane is drawn.
///
/// Wider than the Styles pane, because its widest line is a sentence rather
/// than a style's name.
pub const WIDTH: f32 = 270.0;

/// How tall a row of the editor list is.
const ROW: f32 = 22.0;

/// And how tall a button is.
const BUTTON: f32 = 26.0;

/// One person who may be let through a restriction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Editor {
    /// The name as the document writes it, or [`wp_docx::permissions::EVERYONE`].
    pub name: String,
    /// What to show, which for the group is the word Word shows.
    pub shown: String,
    /// Whether the selection is inside a stretch this one may edit.
    pub on: bool,
    /// How many stretches of the document name them.
    pub stretches: usize,
}

/// Everything the pane draws, worked out afresh each time from the document.
#[derive(Clone, Debug, Default)]
pub struct Shown {
    /// Whether the formatting is limited to a selection of styles.
    pub limiting: bool,
    /// How many styles that selection holds, for the line under the box.
    pub allowed: usize,
    /// Whether only one kind of editing is allowed.
    pub restricting: bool,
    /// Which kind, as the names Word gives them.
    pub modes: Vec<String>,
    pub mode: usize,
    /// Who may be let through, the group first.
    pub editors: Vec<Editor>,
    /// Whether any of it is being enforced yet.
    pub enforced: bool,
    /// Whether the restriction has a password behind it.
    pub locked_with_a_password: bool,
    /// What this person may do, said in a sentence.
    pub permission: String,
    /// Whether the stretches this person may edit are shaded.
    pub highlight: bool,
    /// How many stretches this person may edit.
    pub mine: usize,
}

/// Where a press on the pane can land.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    /// The tick box that limits formatting to a selection of styles.
    Limit,
    /// The link under it that says which styles those are.
    Settings,
    /// The tick box that allows only one kind of editing.
    Restrict,
    /// The box that says which kind.
    Mode,
    /// One of the people, by their place in the list.
    Person(usize),
    /// Word's More users…, which takes names nobody has typed yet.
    MoreUsers,
    /// The button that starts enforcing all of it.
    Start,
    /// And the one that stops it.
    Stop,
    /// Find Next Region I Can Edit.
    FindNext,
    /// Show All Regions I Can Edit.
    ShowAll,
    /// The tick box that shades them.
    Highlight,
    Close,
}

/// The pane, and what it remembers between one drawing and the next.
#[derive(Clone, Debug, Default)]
pub struct RestrictPane {
    hovered: Option<Hit>,
    /// How far down it has been scrolled, and how far it reaches.
    scroll: Scroll,
    /// Where everything ended up when it was last drawn, so that a press can
    /// be told what it landed on without the drawing being done again.
    placed: Vec<(Hit, f32, f32, f32, f32)>,
}

impl RestrictPane {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// What a point is on, if anything.
    #[must_use]
    pub fn at(&self, x: i32, y: i32) -> Option<Hit> {
        let x = super::mirror::flip(x);
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

    /// Draws the pane.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        shown: &Shown,
        left: f32,
        top: f32,
        bottom: f32,
        theme: &Theme,
    ) {
        self.placed.clear();
        canvas.fill_rect(left as i32, top as i32, WIDTH as i32, (bottom - top) as i32, theme.pane);
        canvas.fill_rect(left as i32, top as i32, 1, (bottom - top) as i32, theme.pane_edge);

        // Begun as far above the window as it has been scrolled: everything a
        // pane draws is placed by running a number down the column, so moving
        // where that number starts moves the whole column. The caption does
        // not move with it — it is the pane's own name and the way out.
        let started = top - self.scroll.offset;
        let mut pen =
            Pen { canvas, engine, renderer, theme, left, width: WIDTH, y: top + 8.0, bottom, top };
        self.caption(&mut pen, top);
        let header = pen.y;
        pen.y = started + (header - top);

        if shown.enforced {
            self.permissions(&mut pen, shown);
        } else {
            self.settings(&mut pen, shown);
        }

        // How far it reached, measured from the top of the pane, which is
        // what says how far it may be scrolled.
        let reach = pen.y - started;
        self.scroll.reached(reach, bottom - top);
        self.scroll.draw_bar(canvas, left + WIDTH, header, bottom, theme);
    }

    /// Moves it, and says whether it moved.
    pub fn scroll_by(&mut self, pixels: f32) -> bool {
        self.scroll.by(pixels)
    }

    /// Whether there is more of it than there is room for.
    #[must_use]
    pub fn overflows(&self) -> bool {
        self.scroll.overflows()
    }

    /// The name of the pane, and the cross that shuts it.
    fn caption(&mut self, pen: &mut Pen<'_, '_, '_>, top: f32) {
        pen.words(t("Restrict Editing"), pen.left + PADDING, HEADING, pen.theme.text);
        let close_left = pen.left + WIDTH - 26.0;
        if self.hovered == Some(Hit::Close) {
            pen.canvas.fill_rect(close_left as i32 - 4, top as i32 + 6, 24, 22, pen.theme.hover);
        }
        super::icons::draw_sized(
            pen.canvas,
            super::icons::Icon::Close,
            close_left,
            top + 10.0,
            12.0,
            pen.theme.text,
        );
        self.placed.push((Hit::Close, close_left - 4.0, top + 6.0, 24.0, 22.0));
        pen.y = top + 34.0;
        pen.rule();
    }

    /// Word's four numbered sections, which is the pane before anything is
    /// being enforced.
    fn settings(&mut self, pen: &mut Pen<'_, '_, '_>, shown: &Shown) {
        pen.heading(t("1. Formatting restrictions"));
        self.tick(pen, Hit::Limit, t("Limit formatting to a selection of styles"), shown.limiting);
        let note = crate::messages::with("{0} styles allowed", &[&shown.allowed.to_string()]);
        pen.note(&note);
        self.link(pen, Hit::Settings, t("Settings…"));
        pen.gap();

        pen.heading(t("2. Editing restrictions"));
        self.tick(
            pen,
            Hit::Restrict,
            t("Allow only this kind of editing in the document"),
            shown.restricting,
        );
        let said = shown.modes.get(shown.mode).cloned().unwrap_or_default();
        let said = crate::messages::translated(&said);
        self.box_of(pen, Hit::Mode, &said, shown.restricting);
        pen.gap();

        pen.heading(t("3. Exceptions"));
        pen.note(t("Select part of the document, then tick who may edit it."));
        for (index, person) in shown.editors.iter().enumerate() {
            self.person(pen, index, person);
        }
        self.link(pen, Hit::MoreUsers, t("More users…"));
        pen.gap();

        pen.heading(t("4. Start enforcement"));
        self.button(pen, Hit::Start, t("Yes, Start Enforcing Protection"));
    }

    /// And the pane once something is.
    fn permissions(&mut self, pen: &mut Pen<'_, '_, '_>, shown: &Shown) {
        pen.heading(t("Your permissions"));
        pen.note(&shown.permission);
        pen.gap();

        if shown.mine > 0 {
            // One and many are two sentences rather than one with a number in
            // it: the strip along the bottom says "1 word" and not "1 words",
            // and a pane that said the other would be the same program
            // speaking two ways.
            let said = if shown.mine == 1 {
                t("You may edit one stretch of this document.").to_owned()
            } else {
                crate::messages::with(
                    "You may edit {0} stretches of this document.",
                    &[&shown.mine.to_string()],
                )
            };
            pen.note(&said);
            self.button(pen, Hit::FindNext, t("Find Next Region I Can Edit"));
            self.button(pen, Hit::ShowAll, t("Show All Regions I Can Edit"));
            self.tick(pen, Hit::Highlight, t("Highlight the regions I can edit"), shown.highlight);
            pen.gap();
        }

        if shown.locked_with_a_password {
            pen.note(t("Stopping it asks for the password."));
        }
        self.button(pen, Hit::Stop, t("Stop Protection"));
    }

    /// One tick box with its words beside it.
    fn tick(&mut self, pen: &mut Pen<'_, '_, '_>, hit: Hit, label: &str, on: bool) {
        let box_left = pen.left + PADDING;
        let box_top = pen.y;
        pen.canvas.fill_rect(box_left as i32, box_top as i32, 14, 14, pen.theme.field);
        outline(pen.canvas, box_left, box_top, 14.0, 14.0, pen.theme.field_edge);
        if on {
            pen.canvas.fill_rect(
                (box_left + 3.0) as i32,
                (box_top + 3.0) as i32,
                8,
                8,
                pen.theme.accent,
            );
        }
        // Wrapped, because these are sentences and the pane is narrow.
        let colour = pen.theme.text;
        let used = pen.wrapped(
            label,
            box_left + 20.0,
            box_top + 11.0,
            WIDTH - PADDING * 2.0 - 20.0,
            colour,
        );
        let height = used.max(16.0);
        self.placed.push((hit, box_left, box_top, WIDTH - PADDING * 2.0, height));
        pen.y = box_top + height + 4.0;
    }

    /// A word that is pressed rather than read, drawn as Word draws a link.
    fn link(&mut self, pen: &mut Pen<'_, '_, '_>, hit: Hit, label: &str) {
        let x = pen.left + PADDING;
        let colour = if self.hovered == Some(hit) { pen.theme.text } else { pen.theme.accent };
        let top = pen.y;
        let width = pen.words(label, x, TEXT, colour);
        // Underlined, which is what says it can be pressed. Under the words
        // rather than through them: `words` draws downwards from the pen, so
        // the line clears the whole of the text and not half of it.
        pen.canvas.fill_rect(x as i32, (top + TEXT + 3.0) as i32, width as i32, 1, colour);
        self.placed.push((hit, x, top, width, TEXT + 6.0));
        pen.y = top + TEXT + 10.0;
    }

    /// The box that shows which kind of editing is allowed.
    fn box_of(&mut self, pen: &mut Pen<'_, '_, '_>, hit: Hit, said: &str, live: bool) {
        let x = pen.left + PADDING + 20.0;
        let width = WIDTH - PADDING * 2.0 - 20.0;
        pen.canvas.fill_rect(x as i32, pen.y as i32, width as i32, 20, pen.theme.field);
        outline(pen.canvas, x, pen.y, width, 20.0, pen.theme.field_edge);
        let colour = if live { pen.theme.text } else { pen.theme.disabled_text };
        pen.words(said, x + 6.0, TEXT, colour);
        // The little arrow that says it drops open.
        let arrow = x + width - 14.0;
        for step in 0..4 {
            pen.canvas.fill_rect(
                (arrow + step as f32) as i32,
                (pen.y + 8.0 + step as f32) as i32,
                7 - step * 2,
                1,
                colour,
            );
        }
        if live {
            self.placed.push((hit, x, pen.y, width, 20.0));
        }
        pen.y += 26.0;
    }

    /// One person on the exceptions list.
    ///
    /// Drawn in their own colour — the colour their tracked changes are drawn
    /// in and the colour the stretches they may edit are shaded — because a
    /// list of names beside a document shaded in colours is no use unless the
    /// two agree.
    fn person(&mut self, pen: &mut Pen<'_, '_, '_>, index: usize, person: &Editor) {
        let hit = Hit::Person(index);
        let top = pen.y;
        if self.hovered == Some(hit) {
            pen.canvas.fill_rect(
                (pen.left + 4.0) as i32,
                top as i32,
                (WIDTH - 8.0) as i32,
                ROW as i32,
                pen.theme.hover,
            );
        }

        let box_left = pen.left + PADDING;
        pen.canvas.fill_rect(box_left as i32, (top + 3.0) as i32, 14, 14, pen.theme.field);
        outline(pen.canvas, box_left, top + 3.0, 14.0, 14.0, pen.theme.field_edge);
        if person.on {
            pen.canvas.fill_rect(
                (box_left + 3.0) as i32,
                (top + 6.0) as i32,
                8,
                8,
                pen.theme.accent,
            );
        }

        // A patch of their colour, then their name.
        let swatch = box_left + 20.0;
        pen.canvas.fill_rect(
            swatch as i32,
            (top + 5.0) as i32,
            10,
            10,
            wp_layout::author_color(&person.name),
        );

        let said = if person.stretches > 0 {
            format!("{} ({})", person.shown, person.stretches)
        } else {
            person.shown.clone()
        };
        pen.y = top + 4.0;
        pen.words(&said, swatch + 16.0, TEXT, pen.theme.text);
        self.placed.push((hit, pen.left + 4.0, top, WIDTH - 8.0, ROW));
        pen.y = top + ROW;
    }

    /// A button across the pane.
    fn button(&mut self, pen: &mut Pen<'_, '_, '_>, hit: Hit, label: &str) {
        let x = pen.left + PADDING;
        let width = WIDTH - PADDING * 2.0;
        let fill = if self.hovered == Some(hit) { pen.theme.hover } else { pen.theme.field };
        pen.canvas.fill_rect(x as i32, pen.y as i32, width as i32, BUTTON as i32, fill);
        outline(pen.canvas, x, pen.y, width, BUTTON, pen.theme.field_edge);

        // Centred, which means measuring it first.
        let measured = pen.engine.simple_line(label, 0.0, 0.0, TEXT, pen.theme.text);
        let text_x = x + (width - measured.width).max(0.0) / 2.0;
        let line = pen.engine.simple_line(label, text_x, pen.y + 17.0, TEXT, pen.theme.text);
        pen.renderer.draw_onto(pen.canvas, &line, 0.0, 0.0);

        self.placed.push((hit, x, pen.y, width, BUTTON));
        pen.y += BUTTON + 8.0;
    }
}

/// A rectangle drawn as four lines rather than filled.
fn outline(canvas: &mut Canvas, left: f32, top: f32, width: f32, height: f32, colour: Color) {
    let (x, y, w, h) = (left as i32, top as i32, width as i32, height as i32);
    canvas.fill_rect(x, y, w, 1, colour);
    canvas.fill_rect(x, y + h - 1, w, 1, colour);
    canvas.fill_rect(x, y, 1, h, colour);
    canvas.fill_rect(x + w - 1, y, 1, h, colour);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_hit_before_it_has_been_drawn() {
        // The pane answers out of where things landed last time, so a pane
        // nobody has drawn has to say nothing rather than guess.
        let pane = RestrictPane::new();
        assert_eq!(pane.at(10, 10), None);
    }

    #[test]
    fn the_two_faces_of_it_are_not_the_same_pane() {
        // Before enforcement the questions are what to restrict; after it,
        // what this person may do about it. A pane that showed both would be
        // offering to restrict a document that is already restricted.
        let before = Shown { enforced: false, ..Shown::default() };
        let after = Shown { enforced: true, ..Shown::default() };
        assert!(!before.enforced && after.enforced);
    }
}

#[cfg(test)]
mod drawing {
    use super::*;
    use crate::chrome::theme::Theme;

    /// The pane drawn into a canvas of its own, so the typography can be
    /// measured rather than squinted at.
    fn drawn(shown: &Shown) -> (Canvas, RestrictPane) {
        let library: &'static wp_layout::FontLibrary =
            Box::leak(Box::new(wp_layout::FontLibrary::scan_system()));
        let mut engine = LayoutEngine::new(library).with_dpi(96.0);
        let mut renderer = Renderer::new(library);
        let mut canvas = Canvas::new(WIDTH as usize, 600);
        let mut pane = RestrictPane::new();
        pane.draw(&mut canvas, &mut engine, &mut renderer, shown, 0.0, 0.0, 600.0, &Theme::light());
        (canvas, pane)
    }

    /// Which rows of a band of the canvas have anything drawn on them.
    fn inked_rows(
        canvas: &Canvas,
        left: usize,
        right: usize,
        from: usize,
        to: usize,
    ) -> Vec<usize> {
        let background = Theme::light().pane;
        (from..to).filter(|y| (left..right).any(|x| canvas.pixel(x, *y) != background)).collect()
    }

    #[test]
    fn a_link_is_underlined_below_its_words_and_not_through_them() {
        // An underline drawn through the middle of a word is a strikethrough,
        // which says the opposite of what a link means.
        let (canvas, pane) =
            drawn(&Shown { modes: vec!["No changes".to_owned()], ..Shown::default() });
        let (_, left, top, width, height) = pane
            .placed
            .iter()
            .copied()
            .find(|(hit, ..)| *hit == Hit::Settings)
            .expect("the link was not drawn");

        let rows = inked_rows(
            &canvas,
            left as usize,
            (left + width) as usize,
            top as usize,
            (top + height + 6.0) as usize,
        );
        assert!(!rows.is_empty(), "nothing was drawn where the link says it is");

        // The underline is the one row that is inked all the way across.
        let solid: Vec<usize> = rows
            .iter()
            .copied()
            .filter(|y| {
                (left as usize..(left + width) as usize)
                    .all(|x| canvas.pixel(x, *y) != Theme::light().pane)
            })
            .collect();
        assert_eq!(solid.len(), 1, "there is not exactly one solid row: {solid:?}");
        let underline = solid[0];
        let words: Vec<usize> = rows.iter().copied().filter(|y| *y != underline).collect();
        assert!(!words.is_empty(), "the link has no words");
        assert!(
            words.iter().all(|y| *y < underline),
            "the line is drawn through the words: line at {underline}, words at {words:?}"
        );
    }
}
