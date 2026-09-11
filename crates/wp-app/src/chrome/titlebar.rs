//! The window's own title bar.
//!
//! # Why the program draws its own
//!
//! Word does, and for a reason worth copying: the strip across the top is the
//! most valuable space in the window, and a caption the system draws can hold
//! nothing but a name. Drawing it here puts the commands a person reaches for
//! most — save, undo, redo — permanently in reach, and puts the document's name
//! where everyone looks for it.
//!
//! The cost is that the window has to say for itself which parts of that strip
//! can be dragged and which are buttons, and where its resize edges are. That
//! is what `is_caption` answers, and it is what keeps dragging, snapping,
//! double-click-to-maximise and every resize edge behaving as they do for any
//! other window.

use wp_layout::{LayoutEngine, Renderer};
use wp_raster::{Canvas, Color};

use super::icons;
use super::theme::Theme;
use super::{Command, ToolbarState};

/// Height of the title bar. Windows uses 32 for its own, and a bar that is a
/// different height from every other window's looks wrong beside them.
pub const HEIGHT: f32 = 32.0;

/// How wide each of the window buttons is. 46 is what Windows uses.
const BUTTON_WIDTH: f32 = 46.0;
/// How wide each quick-access button is.
const QUICK_WIDTH: f32 = 30.0;
/// The gap kept between the title and whatever is beside it.
const TITLE_GAP: f32 = 16.0;

/// What pressing one of the window's own buttons does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowButton {
    Minimise,
    Maximise,
    Close,
}

/// The title bar, and where everything on it ended up.
#[derive(Debug, Default)]
pub struct TitleBar {
    quick: Vec<(Command, f32, f32)>,
    windows: Vec<(WindowButton, f32, f32)>,
    /// Which button the pointer is over.
    hovered_window: Option<WindowButton>,
    hovered_quick: Option<Command>,
}

/// How many buttons the toolbar will draw, whatever is on it.
///
/// The title bar holds the document's name as well, and a toolbar allowed to
/// grow without limit would push it off the bar altogether. Word's answer is to
/// move the toolbar under the ribbon once it is long; this one keeps it where
/// it is and stops.
const QUICK_MOST: usize = 12;

impl TitleBar {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether a point is on the part of the bar that drags the window.
    ///
    /// Everything except the buttons: pressing a button must press it, not
    /// start dragging the window across the desk.
    #[must_use]
    pub fn is_caption(&self, x: i32, y: i32) -> bool {
        if (y as f32) >= HEIGHT {
            return false;
        }
        self.quick_at(x, y).is_none() && self.window_button_at(x, y).is_none()
    }

    /// The quick access command at a point, if there is one.
    #[must_use]
    pub fn quick_at(&self, x: i32, y: i32) -> Option<Command> {
        if (y as f32) >= HEIGHT {
            return None;
        }
        self.quick
            .iter()
            .find(|(_, left, width)| x as f32 >= *left && (x as f32) < left + width)
            .map(|(command, _, _)| *command)
    }

    /// The window button at a point, if there is one.
    #[must_use]
    pub fn window_button_at(&self, x: i32, y: i32) -> Option<WindowButton> {
        if (y as f32) >= HEIGHT {
            return None;
        }
        self.windows
            .iter()
            .find(|(_, left, width)| x as f32 >= *left && (x as f32) < left + width)
            .map(|(button, _, _)| *button)
    }

    /// Lights up whatever the pointer is over. Returns whether that changed.
    pub fn hover(&mut self, x: i32, y: i32) -> bool {
        let window = self.window_button_at(x, y);
        let quick = self.quick_at(x, y);
        let changed = window != self.hovered_window || quick != self.hovered_quick;
        self.hovered_window = window;
        self.hovered_quick = quick;
        changed
    }

    /// Draws the bar across the top of the window.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        title: &str,
        quick: &[Command],
        state: &ToolbarState,
        theme: &Theme,
    ) {
        self.quick.clear();
        self.windows.clear();

        let width = canvas.width() as f32;
        canvas.fill_rect(0, 0, width as i32, HEIGHT as i32, theme.title_bar);

        let text = theme.bar_text();
        let dim = theme.bar_dim_text();

        // The quick access buttons, on the left where Word puts them.
        let mut x = 8.0f32;
        for command in quick.iter().take(QUICK_MOST) {
            let enabled = super::is_enabled(*command, state);
            if self.hovered_quick == Some(*command) && enabled {
                rounded(canvas, x, 3.0, QUICK_WIDTH, HEIGHT - 6.0, theme.bar_hover());
            }
            icons::draw(
                canvas,
                super::ribbon::icon_of(*command),
                x + (QUICK_WIDTH - icons::SIZE) / 2.0,
                (HEIGHT - icons::SIZE) / 2.0,
                if enabled { text } else { dim },
            );
            self.quick.push((*command, x, QUICK_WIDTH));
            x += QUICK_WIDTH;
        }

        // The window's own buttons, on the right where every window puts them.
        let buttons = [WindowButton::Minimise, WindowButton::Maximise, WindowButton::Close];
        let mut right = width - BUTTON_WIDTH * buttons.len() as f32;
        let buttons_left = right;
        for button in buttons {
            let hovered = self.hovered_window == Some(button);
            if hovered {
                let fill =
                    if button == WindowButton::Close { theme.danger } else { theme.bar_hover() };
                canvas.fill_rect(right as i32, 0, BUTTON_WIDTH as i32, HEIGHT as i32, fill);
            }
            // The mark on the close button turns white on the red, whatever the
            // theme, because red is red in both.
            let mark = if hovered && button == WindowButton::Close { Color::WHITE } else { text };
            draw_window_button(canvas, button, right, BUTTON_WIDTH, mark);
            self.windows.push((button, right, BUTTON_WIDTH));
            right += BUTTON_WIDTH;
        }

        // The document's name, centred in the whole bar if it fits between the
        // two sets of buttons, and pushed along if it does not.
        let quick_right = x + TITLE_GAP;
        let measured = engine.simple_line(title, 0.0, 0.0, 9.0, text).width;
        let mut left = (width - measured) / 2.0;
        if left < quick_right {
            left = quick_right;
        }
        let room = buttons_left - TITLE_GAP - left;
        if room > 40.0 {
            let shown = trim_to_width(engine, title, room, text);
            let line = engine.simple_line(&shown, left, HEIGHT / 2.0 + 3.5, 9.0, text);
            renderer.draw_onto(canvas, &line, 0.0, 0.0);
        }
    }
}

/// The mark on one of the window's own buttons.
///
/// Windows draws these at ten pixels square, and the eye notices when they are
/// not: a close button one pixel out reads as a different program's.
fn draw_window_button(
    canvas: &mut Canvas,
    button: WindowButton,
    left: f32,
    width: f32,
    colour: Color,
) {
    let centre_x = (left + width / 2.0).round();
    let centre_y = (HEIGHT / 2.0).round();

    match button {
        WindowButton::Minimise => {
            canvas.fill_rect((centre_x - 5.0) as i32, centre_y as i32, 10, 1, colour);
        }
        WindowButton::Maximise => {
            // A hollow square, or two overlapping ones when already maximised.
            let (x, y, size) = (centre_x - 5.0, centre_y - 5.0, 10.0);
            if wp_shell::is_maximised() {
                hollow(canvas, x - 1.0, y + 2.0, size - 1.0, size - 1.0, colour);
                hollow(canvas, x + 2.0, y - 1.0, size - 1.0, size - 1.0, colour);
            } else {
                hollow(canvas, x, y, size, size, colour);
            }
        }
        WindowButton::Close => {
            for step in 0..10 {
                let along = step as f32;
                canvas.fill_rect(
                    (centre_x - 5.0 + along) as i32,
                    (centre_y - 5.0 + along) as i32,
                    1,
                    1,
                    colour,
                );
                canvas.fill_rect(
                    (centre_x - 5.0 + along) as i32,
                    (centre_y + 4.0 - along) as i32,
                    1,
                    1,
                    colour,
                );
            }
        }
    }
}

fn hollow(canvas: &mut Canvas, x: f32, y: f32, width: f32, height: f32, colour: Color) {
    canvas.fill_rect(x as i32, y as i32, width as i32, 1, colour);
    canvas.fill_rect(x as i32, (y + height - 1.0) as i32, width as i32, 1, colour);
    canvas.fill_rect(x as i32, y as i32, 1, height as i32, colour);
    canvas.fill_rect((x + width - 1.0) as i32, y as i32, 1, height as i32, colour);
}

/// A filled rectangle with its four corner pixels left out, which at this size
/// is all a rounded corner is.
fn rounded(canvas: &mut Canvas, x: f32, y: f32, width: f32, height: f32, colour: Color) {
    let (x, y, width, height) = (x as i32, y as i32, width as i32, height as i32);
    canvas.fill_rect(x + 1, y, width - 2, 1, colour);
    canvas.fill_rect(x, y + 1, width, height - 2, colour);
    canvas.fill_rect(x + 1, y + height - 1, width - 2, 1, colour);
}

/// Cuts a caption down to what fits, ending it with an ellipsis.
fn trim_to_width(engine: &mut LayoutEngine<'_>, text: &str, width: f32, colour: Color) -> String {
    if engine.simple_line(text, 0.0, 0.0, 9.0, colour).width <= width {
        return text.to_owned();
    }
    let mut kept = String::new();
    for character in text.chars() {
        let mut candidate = kept.clone();
        candidate.push(character);
        candidate.push('…');
        if engine.simple_line(&candidate, 0.0, 0.0, 9.0, colour).width > width {
            break;
        }
        kept.push(character);
    }
    kept.push('…');
    kept
}
