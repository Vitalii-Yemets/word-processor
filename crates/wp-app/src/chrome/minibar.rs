//! The little bar of formatting buttons that floats over a selection.
//!
//! # What Word does
//!
//! Select some text with the mouse and a small toolbar appears just above where
//! the button was let go, so faint it is nearly not there. Move towards it and
//! it becomes solid; move away and it goes. Right-click and it comes up solid,
//! over the menu. It holds the handful of things somebody formatting a piece of
//! text reaches for, so that the commonest job in a word processor does not
//! need a trip to the ribbon and back.
//!
//! All of that is behaviour rather than decoration, and all of it is here: the
//! fade, the waking on approach, the going away when the pointer leaves, the
//! place it appears, and what a click on it does.
//!
//! # What is different
//!
//! Word's is two rows deep. This is one row holding the same commands, because
//! two rows of a bar this size is a layout decision rather than a behaviour,
//! and one row is less to reach across.

use wp_docx::CharacterFormat;
use wp_layout::{LayoutEngine, Renderer, TextStyle};
use wp_raster::{Canvas, Color};

use super::icons::{self, Icon};
use super::theme::Theme;
use super::{Choice, Command, ToolbarState};

/// How high the bar is, in pixels.
pub const HEIGHT: f32 = 30.0;
/// The gap kept between it and the pointer, so it never lands under the hand.
const OFFSET: f32 = 14.0;
/// How much of its colour it keeps before the pointer reaches it.
const FAINT: u8 = 120;
/// How far the pointer may wander before the bar gives up and goes.
const REACH: f32 = 70.0;

const PADDING: f32 = 3.0;
const BUTTON: f32 = 24.0;
const GAP: f32 = 7.0;

const BOLD: TextStyle = TextStyle { bold: true, italic: false, underline: false, strike: false };
const ITALIC: TextStyle = TextStyle { bold: false, italic: true, underline: false, strike: false };
const UNDERLINE: TextStyle =
    TextStyle { bold: false, italic: false, underline: true, strike: false };

/// One thing on the bar.
enum Part {
    /// A box showing what is in force, which drops a list open.
    Field(Command, Choice, f32),
    Button(Command, Icon),
    /// A letter set in the formatting it applies, as B, I and U are.
    Letter(Command, &'static str, TextStyle),
    /// A space between two groups of buttons.
    Space,
}

/// What the bar holds, in the order Word holds it.
const PARTS: &[Part] = &[
    Part::Field(Command::ChooseFont, Choice::Font, 112.0),
    Part::Field(Command::ChooseSize, Choice::Size, 44.0),
    Part::Button(Command::GrowFont, Icon::LetterUp),
    Part::Button(Command::ShrinkFont, Icon::LetterDown),
    Part::Space,
    Part::Letter(Command::Format(CharacterFormat::Bold), "B", BOLD),
    Part::Letter(Command::Format(CharacterFormat::Italic), "I", ITALIC),
    Part::Letter(Command::Format(CharacterFormat::Underline), "U", UNDERLINE),
    Part::Button(Command::TextColor, Icon::TextColor),
    Part::Button(Command::Highlight, Icon::Highlight),
    Part::Space,
    Part::Button(Command::FormatPainter, Icon::Brush),
    Part::Button(Command::Bullets, Icon::Bullets),
    Part::Button(Command::Numbering, Icon::Numbering),
    Part::Button(Command::IndentLess, Icon::IndentLess),
    Part::Button(Command::IndentMore, Icon::IndentMore),
    Part::Space,
    Part::Field(Command::ChooseStyle, Choice::Style, 92.0),
];

/// The bar, where it is and how awake it is.
#[derive(Clone, Debug)]
pub struct MiniBar {
    left: f32,
    top: f32,
    /// Whether the pointer has reached it, which is what makes it solid.
    awake: bool,
    hovered: Option<Command>,
}

impl MiniBar {
    /// Puts one over a point, kept inside the room it is given.
    ///
    /// Above the point, which is where Word puts it: below would cover the line
    /// that has just been selected, and the whole purpose of the thing is to
    /// stay out of the way of what is being read.
    #[must_use]
    pub fn new(x: f32, y: f32, room: (f32, f32, f32, f32)) -> Self {
        let (room_left, room_top, room_right, room_bottom) = room;
        let width = Self::width();

        let mut left = x + 4.0;
        if left + width > room_right {
            left = room_right - width;
        }
        left = left.max(room_left);

        // Above the pointer, unless there is no room up there — near the top of
        // the window it goes below instead, rather than under the ribbon.
        let mut top = y - HEIGHT - OFFSET;
        if top < room_top {
            top = y + OFFSET;
        }
        top = top.min(room_bottom - HEIGHT).max(room_top);

        Self { left, top, awake: false, hovered: None }
    }

    /// How wide the bar is, which is the same however it is drawn.
    #[must_use]
    pub fn width() -> f32 {
        let mut width = PADDING;
        for part in PARTS {
            width += part_width(part);
        }
        width + PADDING
    }

    /// Makes it solid at once, which is what the right button does.
    pub fn wake(&mut self) {
        self.awake = true;
    }

    #[must_use]
    pub fn covers(&self, x: i32, y: i32) -> bool {
        let (x, y) = (x as f32, y as f32);
        x >= self.left && x < self.left + Self::width() && y >= self.top && y < self.top + HEIGHT
    }

    /// Which command a point is on, if any.
    #[must_use]
    pub fn hit(&self, x: i32, y: i32) -> Option<Command> {
        if !self.covers(x, y) {
            return None;
        }
        let mut left = self.left + PADDING;
        for part in PARTS {
            let width = part_width(part);
            if (x as f32) < left + width {
                return match part {
                    Part::Field(command, ..)
                    | Part::Button(command, _)
                    | Part::Letter(command, ..) => Some(*command),
                    Part::Space => None,
                };
            }
            left += width;
        }
        None
    }

    /// Where a list dropped from one of its boxes hangs: the left edge of the
    /// box, the bottom of the bar, and the width of the box.
    #[must_use]
    pub fn anchor(&self, command: Command) -> Option<(f32, f32, f32)> {
        let mut left = self.left + PADDING;
        for part in PARTS {
            let width = part_width(part);
            let found = match part {
                Part::Field(found, ..) | Part::Button(found, _) | Part::Letter(found, ..) => {
                    Some(*found)
                }
                Part::Space => None,
            };
            if found == Some(command) {
                return Some((left, self.top + HEIGHT, width.max(120.0)));
            }
            left += width;
        }
        None
    }

    /// Follows the pointer. Returns whether anything about the bar changed.
    pub fn hover(&mut self, x: i32, y: i32) -> bool {
        let found = self.hit(x, y);
        let inside = self.covers(x, y);
        let changed = found != self.hovered || (inside && !self.awake);
        self.hovered = found;
        if inside {
            self.awake = true;
        }
        changed
    }

    /// Which of its buttons the pointer is on, if any.
    #[must_use]
    pub fn hovered(&self) -> Option<Command> {
        self.hovered
    }

    /// Whether the pointer has gone far enough away for the bar to give up.
    ///
    /// Word's fades out as the pointer leaves rather than the moment it steps
    /// off the last button, which is what stops the bar flickering away when a
    /// hand travelling towards it passes a pixel outside.
    #[must_use]
    pub fn abandoned(&self, x: i32, y: i32) -> bool {
        let (x, y) = (x as f32, y as f32);
        x < self.left - REACH
            || x > self.left + Self::width() + REACH
            || y < self.top - REACH
            || y > self.top + HEIGHT + REACH
    }

    pub fn draw(
        &self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        state: &ToolbarState,
        theme: &Theme,
    ) {
        let alpha = if self.awake { 255 } else { FAINT };
        let fade = |color: Color| Color { alpha, ..color };
        let width = Self::width();

        canvas.fill_rect(
            self.left as i32 - 1,
            self.top as i32 - 1,
            width as i32 + 2,
            HEIGHT as i32 + 2,
            fade(theme.field_edge),
        );
        canvas.fill_rect(
            self.left as i32,
            self.top as i32,
            width as i32,
            HEIGHT as i32,
            fade(theme.pane),
        );

        let mut left = self.left + PADDING;
        for part in PARTS {
            let part_width = part_width(part);
            let command = match part {
                Part::Field(command, ..) | Part::Button(command, _) | Part::Letter(command, ..) => {
                    Some(*command)
                }
                Part::Space => None,
            };

            // The same two backgrounds the ribbon uses, so a button that is on
            // looks the same wherever it is met.
            if let Some(command) = command {
                let active = super::is_active(command, state);
                let background = if active {
                    Some(fade(theme.accent))
                } else if self.hovered == Some(command) {
                    Some(fade(theme.hover))
                } else {
                    None
                };
                if let Some(fill) = background {
                    canvas.fill_rect(
                        left as i32,
                        (self.top + PADDING) as i32,
                        part_width as i32,
                        (HEIGHT - PADDING * 2.0) as i32,
                        fill,
                    );
                }
                let color = if active { fade(theme.on_accent()) } else { fade(theme.text) };
                self.draw_part(canvas, engine, renderer, part, left, color, state, theme, alpha);
            }
            left += part_width;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_part(
        &self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        part: &Part,
        left: f32,
        color: Color,
        state: &ToolbarState,
        theme: &Theme,
        alpha: u8,
    ) {
        let fade = |color: Color| Color { alpha, ..color };
        let middle = self.top + HEIGHT / 2.0;

        match part {
            Part::Field(_, choice, width) => {
                canvas.fill_rect(
                    left as i32,
                    (self.top + PADDING + 1.0) as i32,
                    *width as i32,
                    (HEIGHT - PADDING * 2.0 - 2.0) as i32,
                    fade(theme.field),
                );
                let text = match choice {
                    Choice::Font => state.font.clone().unwrap_or_else(|| "(default)".to_owned()),
                    Choice::Size => super::format_size(state.size),
                    Choice::Style => state.style.clone().unwrap_or_else(|| "Normal".to_owned()),
                    _ => String::new(),
                };
                let line = engine.simple_line(&text, left + 5.0, middle + 4.0, 8.5, color);
                renderer.draw_onto(canvas, &line, 0.0, 0.0);
                chevron(canvas, left + width - 11.0, middle, color);
            }
            Part::Button(_, icon) => {
                let inset = (BUTTON - icons::SIZE) / 2.0;
                let top = middle - icons::SIZE / 2.0;
                icons::draw(canvas, *icon, left + inset, top, color);
                // The two coloured buttons carry the colour they would apply,
                // which is the only way to tell what pressing them will do.
                match *icon {
                    Icon::TextColor => icons::draw_color_band(
                        canvas,
                        left + inset,
                        top,
                        icons::SIZE,
                        fade(state.text_color),
                    ),
                    Icon::Highlight => icons::draw_color_band(
                        canvas,
                        left + inset,
                        top,
                        icons::SIZE,
                        fade(state.highlight_color),
                    ),
                    _ => {}
                }
            }
            Part::Letter(_, text, style) => {
                let measured = engine.styled_line(text, 0.0, 0.0, 10.0, color, *style);
                let line = engine.styled_line(
                    text,
                    left + (BUTTON - measured.width) / 2.0,
                    middle + 5.0,
                    10.0,
                    color,
                    *style,
                );
                renderer.draw_onto(canvas, &line, 0.0, 0.0);
            }
            Part::Space => {}
        }
    }
}

/// How wide one thing on the bar is.
fn part_width(part: &Part) -> f32 {
    match part {
        Part::Field(_, _, width) => *width,
        Part::Button(..) | Part::Letter(..) => BUTTON,
        Part::Space => GAP,
    }
}

/// The small triangle that says a list drops from here.
fn chevron(canvas: &mut Canvas, x: f32, centre_y: f32, color: Color) {
    for step in 0..4 {
        canvas.fill_rect(
            (x + step as f32) as i32,
            (centre_y - 2.0 + step as f32) as i32,
            (7 - step * 2).max(1),
            1,
            color,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOM: (f32, f32, f32, f32) = (0.0, 100.0, 1000.0, 700.0);

    #[test]
    fn it_sits_above_the_point_it_was_opened_at() {
        let bar = MiniBar::new(400.0, 400.0, ROOM);
        assert!(bar.top < 400.0, "the bar covers the selection");
        assert!(bar.covers(410, (bar.top + 4.0) as i32));
    }

    #[test]
    fn near_the_top_of_the_window_it_goes_below_instead() {
        let bar = MiniBar::new(400.0, 110.0, ROOM);
        assert!(bar.top > 110.0, "the bar was pushed under the ribbon");
    }

    #[test]
    fn it_is_kept_inside_the_window() {
        let bar = MiniBar::new(990.0, 400.0, ROOM);
        assert!(bar.left + MiniBar::width() <= 1000.0, "the bar hangs off the edge");
    }

    #[test]
    fn a_press_on_a_button_is_the_command_it_carries() {
        let bar = MiniBar::new(400.0, 400.0, ROOM);
        let (left, _, _) = bar.anchor(Command::ChooseFont).expect("the font box");
        assert_eq!(bar.hit(left as i32 + 2, (bar.top + 10.0) as i32), Some(Command::ChooseFont));
    }

    #[test]
    fn a_press_outside_it_is_nothing() {
        let bar = MiniBar::new(400.0, 400.0, ROOM);
        assert_eq!(bar.hit(10, 10), None);
    }

    #[test]
    fn the_pointer_reaching_it_wakes_it_up() {
        let mut bar = MiniBar::new(400.0, 400.0, ROOM);
        assert!(!bar.awake);
        assert!(bar.hover((bar.left + 4.0) as i32, (bar.top + 4.0) as i32));
        assert!(bar.awake);
    }

    #[test]
    fn it_is_given_up_only_once_the_pointer_is_well_away() {
        let bar = MiniBar::new(400.0, 400.0, ROOM);
        assert!(!bar.abandoned((bar.left + 4.0) as i32, (bar.top - 10.0) as i32));
        assert!(bar.abandoned((bar.left - 200.0) as i32, bar.top as i32));
    }

    #[test]
    fn every_button_on_it_can_be_pressed() {
        // Nothing may be laid out where a click cannot reach it.
        let bar = MiniBar::new(400.0, 400.0, ROOM);
        for part in PARTS {
            let command = match part {
                Part::Field(command, ..) | Part::Button(command, _) | Part::Letter(command, ..) => {
                    *command
                }
                Part::Space => continue,
            };
            let (left, _, _) = bar.anchor(command).expect("a place");
            assert_eq!(
                bar.hit(left as i32 + 2, (bar.top + HEIGHT / 2.0) as i32),
                Some(command),
                "{command:?} cannot be pressed"
            );
        }
    }
}
