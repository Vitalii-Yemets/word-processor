//! A `UserForm` on the screen.
//!
//! # What this is
//!
//! A form is a window a macro designed: boxes, buttons and labels at the
//! places its author put them, in points, with the caption they were given.
//! Word draws one with the operating system's controls. This draws it with
//! its own, at the same places and sizes, in the same face as the rest of
//! the program's dialogs — which is what the [`super::dialog`] does for the
//! program's own dialogs, and the form is drawn to look like one of those.
//!
//! # Who owns what
//!
//! The macro owns the form: what each control holds is the macro's, and the
//! window is shown a copy of it each time the macro waits for something to
//! happen. What a person does to the copy — types into a box, ticks a box,
//! presses a button — is sent back as one [`Happening`], with what every
//! control holds now, and the macro's own code decides what that means.
//! Between happenings the window keeps the copy, the caret and which control
//! has the keyboard, so that typing feels like typing and not like a
//! conversation.
//!
//! # Units
//!
//! A form is designed in points and Word shows it at the screen's points,
//! which at the usual ninety-six dots to the inch is four pixels to three
//! points. That is the one number here.

use wp_layout::{LayoutEngine, Renderer};
use wp_raster::{Canvas, Color};
use wp_shell::Key;
use wp_vba::forms::{Form, Happening, Kind};

use super::icons::{self, Icon};
use super::theme::Theme;

/// Pixels to a point, at the ninety-six dots to the inch a form is designed
/// for.
const SCALE: f32 = 96.0 / 72.0;

/// The caption bar, the same height as a dialog's.
const TITLE_HEIGHT: f32 = 34.0;

/// Word's forms are set in Tahoma at eight points.
const TEXT: f32 = 8.0;

/// How tall one row of a list is.
const ROW: f32 = 15.0 * SCALE;

/// Where the mouse can land.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Hit {
    Control(usize),
    /// One item of a list, whether standing open or dropped open.
    Item(usize, usize),
    Close,
}

/// What the keyboard or a click did.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// Nothing the form knows about.
    Ignored,
    /// Something to draw again, and nothing to tell the macro.
    Changed,
    /// Something the macro is told.
    Happened(Happening),
}

/// The form as it is being shown.
#[derive(Clone, Debug)]
pub struct FormWindow {
    pub form: Form,
    /// Which control has the keyboard.
    focus: Option<usize>,
    /// Which combo box is dropped open.
    open_list: Option<usize>,
    placed: Vec<(Hit, f32, f32, f32, f32)>,
    hovered: Option<Hit>,
}

impl FormWindow {
    /// The form, with the keyboard on the first control in its tab order.
    #[must_use]
    pub fn new(form: Form) -> Self {
        let focus = form.tab_order().first().copied();
        Self { form, focus, open_list: None, placed: Vec::new(), hovered: None }
    }

    /// The form as the macro has left it, keeping where the keyboard was.
    pub fn replace(&mut self, form: Form) {
        let name =
            self.focus.and_then(|at| self.form.controls.get(at)).map(|held| held.name.clone());
        self.form = form;
        self.focus = name
            .and_then(|name| self.form.controls.iter().position(|held| held.name == name))
            .or_else(|| self.form.tab_order().first().copied());
        if self.open_list.is_some_and(|at| at >= self.form.controls.len()) {
            self.open_list = None;
        }
    }

    /// What every control holds, which goes with every happening.
    fn values(&self) -> Vec<(String, String, i32)> {
        self.form
            .controls
            .iter()
            .map(|control| (control.name.clone(), control.value.clone(), control.list_index))
            .collect()
    }

    fn happened(&self, at: usize, event: &str) -> Outcome {
        Outcome::Happened(Happening::On {
            control: self.form.controls[at].name.clone(),
            event: event.to_owned(),
            values: self.values(),
        })
    }

    /// The size of the window, caption bar and all.
    fn size(&self) -> (f32, f32) {
        (self.form.width * SCALE + 2.0, self.form.height * SCALE + TITLE_HEIGHT + 1.0)
    }

    // --- Doing things to controls ------------------------------------

    /// Presses a control as its kind is pressed: a button is clicked, a tick
    /// box ticked, an option chosen.
    fn act(&mut self, at: usize) -> Outcome {
        let kind = self.form.controls[at].kind;
        if !self.form.controls[at].enabled {
            return Outcome::Ignored;
        }
        match kind {
            Kind::CommandButton => self.happened(at, "Click"),
            Kind::CheckBox | Kind::ToggleButton => {
                let control = &mut self.form.controls[at];
                control.value = if control.ticked() { "0".to_owned() } else { "1".to_owned() };
                self.happened(at, "Click")
            }
            Kind::OptionButton => {
                // One of a group at a time: the others in the same group go
                // off, and a group is the option buttons with one name.
                let group = self.form.controls[at].group.clone();
                for control in &mut self.form.controls {
                    if control.kind == Kind::OptionButton && control.group == group {
                        control.value = "0".to_owned();
                    }
                }
                self.form.controls[at].value = "1".to_owned();
                self.happened(at, "Click")
            }
            Kind::ComboBox => {
                self.open_list = if self.open_list == Some(at) { None } else { Some(at) };
                Outcome::Changed
            }
            _ => Outcome::Changed,
        }
    }

    /// Chooses an item of a list.
    fn choose(&mut self, at: usize, item: usize) -> Outcome {
        let control = &mut self.form.controls[at];
        let Some(text) = control.items.get(item).cloned() else { return Outcome::Ignored };
        control.value = text;
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        {
            control.list_index = item as i32;
        }
        let event = if control.kind == Kind::ComboBox { "Change" } else { "Click" };
        self.open_list = None;
        self.happened(at, event)
    }

    /// Moves the keyboard along the tab order.
    fn move_focus(&mut self, back: bool) {
        let order = self.form.tab_order();
        if order.is_empty() {
            return;
        }
        let here = self.focus.and_then(|at| order.iter().position(|held| *held == at));
        let next = match (here, back) {
            (Some(here), false) => (here + 1) % order.len(),
            (Some(here), true) => (here + order.len() - 1) % order.len(),
            (None, _) => 0,
        };
        self.focus = Some(order[next]);
        self.open_list = None;
    }

    /// The button Enter presses, and the one Escape presses.
    fn default_button(&self) -> Option<usize> {
        self.form.controls.iter().position(|control| {
            control.kind == Kind::CommandButton
                && control.default
                && control.visible
                && control.enabled
        })
    }

    fn cancel_button(&self) -> Option<usize> {
        self.form.controls.iter().position(|control| {
            control.kind == Kind::CommandButton
                && control.cancel
                && control.visible
                && control.enabled
        })
    }

    // --- Events --------------------------------------------------------

    /// A key.
    pub fn key(&mut self, key: Key, shift: bool) -> Outcome {
        match key {
            Key::Tab => {
                self.move_focus(shift);
                Outcome::Changed
            }
            Key::Escape => {
                if self.open_list.take().is_some() {
                    return Outcome::Changed;
                }
                match self.cancel_button() {
                    Some(at) => self.act(at),
                    None => Outcome::Happened(Happening::Closed),
                }
            }
            Key::Enter => {
                if let (Some(at), Some(open)) = (self.focus, self.open_list) {
                    if at == open {
                        let item = self.form.controls[at].list_index.max(0) as usize;
                        return self.choose(at, item);
                    }
                }
                match self.focus {
                    Some(at) if self.form.controls[at].kind == Kind::CommandButton => self.act(at),
                    _ => match self.default_button() {
                        Some(at) => self.act(at),
                        None => Outcome::Ignored,
                    },
                }
            }
            Key::Space => match self.focus {
                Some(at)
                    if self.form.controls[at].kind.is_tick()
                        || self.form.controls[at].kind == Kind::CommandButton =>
                {
                    self.act(at)
                }
                Some(at) => self.typed(at, ' '),
                None => Outcome::Ignored,
            },
            Key::Backspace => match self.focus {
                Some(at) => {
                    let control = &mut self.form.controls[at];
                    if !matches!(control.kind, Kind::TextBox | Kind::ComboBox) || !control.enabled {
                        return Outcome::Ignored;
                    }
                    if control.value.pop().is_none() {
                        return Outcome::Ignored;
                    }
                    self.happened(at, "Change")
                }
                None => Outcome::Ignored,
            },
            Key::Up | Key::Down => match self.focus {
                Some(at) if self.form.controls[at].kind.has_list() => {
                    let control = &self.form.controls[at];
                    if control.items.is_empty() {
                        return Outcome::Ignored;
                    }
                    #[allow(clippy::cast_possible_wrap)]
                    let last = control.items.len() as i32 - 1;
                    let next = if key == Key::Down {
                        (control.list_index + 1).min(last)
                    } else {
                        (control.list_index - 1).max(0)
                    };
                    if next == control.list_index {
                        return Outcome::Ignored;
                    }
                    if control.kind == Kind::ComboBox && self.open_list != Some(at) {
                        // Walking a closed combo box changes it straight away.
                        return self.choose(at, next as usize);
                    }
                    if control.kind == Kind::ListBox {
                        return self.choose(at, next as usize);
                    }
                    self.form.controls[at].list_index = next;
                    Outcome::Changed
                }
                _ => Outcome::Ignored,
            },
            _ => Outcome::Ignored,
        }
    }

    /// A character typed.
    pub fn character(&mut self, character: char) -> Outcome {
        if character.is_control() {
            return Outcome::Ignored;
        }
        // A space on a button presses it, as a space on a tick box ticks it.
        match self.focus {
            Some(at)
                if character == ' '
                    && (self.form.controls[at].kind.is_tick()
                        || self.form.controls[at].kind == Kind::CommandButton) =>
            {
                self.act(at)
            }
            Some(at) => self.typed(at, character),
            None => Outcome::Ignored,
        }
    }

    fn typed(&mut self, at: usize, character: char) -> Outcome {
        let control = &mut self.form.controls[at];
        if !matches!(control.kind, Kind::TextBox | Kind::ComboBox) || !control.enabled {
            return Outcome::Ignored;
        }
        control.value.push(character);
        if control.kind == Kind::ComboBox {
            control.list_index = -1;
        }
        self.happened(at, "Change")
    }

    /// A press of the mouse.
    pub fn press(&mut self, x: i32, y: i32) -> Outcome {
        let hit = self.hit(x, y);
        // A list dropped open takes the press, or is shut by it.
        if let Some(open) = self.open_list {
            match hit {
                Some(Hit::Item(at, item)) if at == open => return self.choose(at, item),
                Some(Hit::Control(at)) if at == open => {
                    self.open_list = None;
                    return Outcome::Changed;
                }
                _ => {
                    self.open_list = None;
                }
            }
        }
        match hit {
            Some(Hit::Close) => Outcome::Happened(Happening::Closed),
            Some(Hit::Item(at, item)) => {
                self.focus = Some(at);
                self.choose(at, item)
            }
            Some(Hit::Control(at)) => {
                let control = &self.form.controls[at];
                if control.kind != Kind::Label && control.enabled {
                    self.focus = Some(at);
                }
                self.act(at)
            }
            None => Outcome::Changed,
        }
    }

    /// The pointer moving: whether anything under it changed.
    pub fn hover(&mut self, x: i32, y: i32) -> bool {
        let hit = self.hit(x, y);
        if hit == self.hovered {
            return false;
        }
        self.hovered = hit;
        true
    }

    fn hit(&self, x: i32, y: i32) -> Option<Hit> {
        let (x, y) = (x as f32, y as f32);
        // The last placed is the topmost: an open list over a control.
        self.placed
            .iter()
            .rev()
            .find(|(_, left, top, width, height)| {
                x >= *left && x < left + width && y >= *top && y < top + height
            })
            .map(|(hit, ..)| *hit)
    }

    // --- Drawing ----------------------------------------------------------

    /// Draws the form over everything, in the middle of the window.
    pub fn draw(
        &mut self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        theme: &Theme,
    ) {
        self.placed.clear();
        let (window_width, window_height) = (canvas.width() as f32, canvas.height() as f32);
        canvas.fill_rect(0, 0, window_width as i32, window_height as i32, Color::rgba(0, 0, 0, 90));

        let (width, height) = self.size();
        let left = ((window_width - width) / 2.0).max(0.0).floor();
        let top = ((window_height - height) / 2.0).max(0.0).floor();

        canvas.fill_rect(
            (left + 4.0) as i32,
            (top + 4.0) as i32,
            width as i32,
            height as i32,
            Color::rgba(0, 0, 0, 60),
        );
        canvas.fill_rect(left as i32, top as i32, width as i32, height as i32, theme.pane);
        outline(canvas, left, top, width, height, theme.pane_edge);

        // The caption bar, as a dialog's.
        canvas.fill_rect(
            left as i32,
            (top + TITLE_HEIGHT) as i32 - 1,
            width as i32,
            1,
            theme.pane_edge,
        );
        let line =
            engine.simple_line(&self.form.caption, left + 12.0, top + 22.0, 10.0, theme.text);
        renderer.draw_within(canvas, &line, left, top, width - 32.0, TITLE_HEIGHT);
        let close_left = left + width - 28.0;
        if self.hovered == Some(Hit::Close) {
            canvas.fill_rect(close_left as i32 - 4, top as i32 + 6, 24, 22, theme.hover);
        }
        icons::draw_sized(canvas, Icon::Close, close_left, top + 10.0, 14.0, theme.text);
        self.placed.push((Hit::Close, close_left - 4.0, top + 6.0, 24.0, 22.0));

        // The controls, at their places, the way they were designed.
        let origin = (left + 1.0, top + TITLE_HEIGHT);
        for at in 0..self.form.controls.len() {
            if self.form.controls[at].visible {
                self.draw_control(canvas, engine, renderer, theme, origin, at);
            }
        }

        // A list dropped open goes over the controls under it.
        if let Some(at) = self.open_list {
            self.draw_open_list(canvas, engine, renderer, theme, origin, at);
        }
    }

    #[allow(clippy::too_many_lines)]
    fn draw_control(
        &mut self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        theme: &Theme,
        origin: (f32, f32),
        at: usize,
    ) {
        let control = self.form.controls[at].clone();
        let x = origin.0 + control.left * SCALE;
        let y = origin.1 + control.top * SCALE;
        let width = (control.width * SCALE).max(1.0);
        let height = (control.height * SCALE).max(1.0);
        let focused = self.focus == Some(at);
        let ink = if control.enabled { theme.text } else { theme.disabled_text };
        let baseline = y + height / 2.0 + TEXT * SCALE * 0.35;

        match control.kind {
            Kind::Label => {
                let line = engine.simple_line(&control.caption, x, y + TEXT * SCALE, TEXT, ink);
                renderer.draw_within(canvas, &line, x, y, width, height);
            }
            Kind::TextBox | Kind::ComboBox => {
                canvas.fill_rect(x as i32, y as i32, width as i32, height as i32, theme.field);
                outline(
                    canvas,
                    x,
                    y,
                    width,
                    height,
                    if focused { theme.accent } else { theme.field_edge },
                );
                let room = if control.kind == Kind::ComboBox { width - 18.0 } else { width };
                let line = engine.simple_line(&control.value, x + 4.0, baseline, TEXT, ink);
                renderer.draw_within(canvas, &line, x + 1.0, y + 1.0, room - 2.0, height - 2.0);
                if focused && control.enabled {
                    // A line's width is where its pen stopped, from the
                    // window's edge, which is where the caret goes.
                    let caret = line.width.min(x + room - 3.0);
                    canvas.fill_rect(
                        caret as i32,
                        (y + 4.0) as i32,
                        1,
                        (height - 8.0) as i32,
                        theme.caret,
                    );
                }
                if control.kind == Kind::ComboBox {
                    let button_left = x + width - 17.0;
                    canvas.fill_rect(
                        button_left as i32,
                        (y + 1.0) as i32,
                        16,
                        (height - 2.0) as i32,
                        theme.pane,
                    );
                    chevron(canvas, button_left + 4.0, y + height / 2.0, ink);
                }
            }
            Kind::CheckBox | Kind::OptionButton => {
                let size = 13.0f32.min(height);
                let box_top = y + (height - size) / 2.0;
                if control.kind == Kind::CheckBox {
                    tick_box(canvas, x, box_top, size, control.ticked(), theme);
                } else {
                    option_dot(canvas, x, box_top, size, control.ticked(), theme);
                }
                let line =
                    engine.simple_line(&control.caption, x + size + 5.0, baseline, TEXT, ink);
                renderer.draw_within(canvas, &line, x + size + 4.0, y, width - size - 4.0, height);
                if focused {
                    outline(canvas, x - 1.0, y, width + 1.0, height, theme.accent);
                }
            }
            Kind::ToggleButton | Kind::CommandButton => {
                let down = control.kind == Kind::ToggleButton && control.ticked();
                let background = if down || (control.default && control.kind == Kind::CommandButton)
                {
                    theme.accent
                } else if self.hovered == Some(Hit::Control(at)) && control.enabled {
                    theme.hover
                } else {
                    theme.field
                };
                canvas.fill_rect(x as i32, y as i32, width as i32, height as i32, background);
                outline(
                    canvas,
                    x,
                    y,
                    width,
                    height,
                    if focused { theme.accent } else { theme.field_edge },
                );
                if focused {
                    outline(canvas, x + 2.0, y + 2.0, width - 4.0, height - 4.0, theme.accent);
                }
                let colour = if background == theme.accent { theme.on_accent() } else { ink };
                let line = engine.simple_line(&control.caption, 0.0, 0.0, TEXT, colour);
                let text_left = x + ((width - line.width) / 2.0).max(2.0);
                let line = engine.simple_line(&control.caption, text_left, baseline, TEXT, colour);
                renderer.draw_within(canvas, &line, x + 1.0, y + 1.0, width - 2.0, height - 2.0);
            }
            Kind::ListBox => {
                canvas.fill_rect(x as i32, y as i32, width as i32, height as i32, theme.field);
                outline(
                    canvas,
                    x,
                    y,
                    width,
                    height,
                    if focused { theme.accent } else { theme.field_edge },
                );
                let mut row_top = y + 1.0;
                for (item, text) in control.items.iter().enumerate() {
                    if row_top + ROW > y + height {
                        break;
                    }
                    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
                    let chosen = control.list_index == item as i32;
                    if chosen {
                        canvas.fill_rect(
                            (x + 1.0) as i32,
                            row_top as i32,
                            (width - 2.0) as i32,
                            ROW as i32,
                            theme.accent,
                        );
                    }
                    let colour = if chosen { theme.on_accent() } else { ink };
                    let line =
                        engine.simple_line(text, x + 4.0, row_top + ROW * 0.72, TEXT, colour);
                    renderer.draw_within(canvas, &line, x + 1.0, row_top, width - 2.0, ROW);
                    self.placed.push((Hit::Item(at, item), x, row_top, width, ROW));
                    row_top += ROW;
                }
            }
            Kind::Frame | Kind::Other => {
                outline(canvas, x, y + 6.0, width, (height - 6.0).max(1.0), theme.control_edge);
                if !control.caption.is_empty() {
                    let line =
                        engine.simple_line(&control.caption, x + 6.0, y + TEXT * SCALE, TEXT, ink);
                    canvas.fill_rect(
                        (x + 4.0) as i32,
                        y as i32,
                        (line.width + 4.0) as i32,
                        12,
                        theme.pane,
                    );
                    renderer.draw_onto(canvas, &line, 0.0, 0.0);
                }
            }
        }
        // A list's rows were placed above; everything else is one place.
        if control.kind != Kind::ListBox {
            self.placed.push((Hit::Control(at), x, y, width, height));
        }
    }

    fn draw_open_list(
        &mut self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        theme: &Theme,
        origin: (f32, f32),
        at: usize,
    ) {
        let control = self.form.controls[at].clone();
        let x = origin.0 + control.left * SCALE;
        let top = origin.1 + (control.top + control.height) * SCALE;
        let width = (control.width * SCALE).max(1.0);
        let rows = control.items.len().clamp(1, 8);
        let height = rows as f32 * ROW + 2.0;
        canvas.fill_rect(x as i32, top as i32, width as i32, height as i32, theme.field);
        outline(canvas, x, top, width, height, theme.field_edge);
        let mut row_top = top + 1.0;
        for (item, text) in control.items.iter().enumerate().take(rows) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
            let chosen = control.list_index == item as i32;
            let hovered = self.hovered == Some(Hit::Item(at, item));
            if chosen || hovered {
                canvas.fill_rect(
                    (x + 1.0) as i32,
                    row_top as i32,
                    (width - 2.0) as i32,
                    ROW as i32,
                    if chosen { theme.accent } else { theme.hover },
                );
            }
            let colour = if chosen { theme.on_accent() } else { theme.text };
            let line = engine.simple_line(text, x + 4.0, row_top + ROW * 0.72, TEXT, colour);
            renderer.draw_within(canvas, &line, x + 1.0, row_top, width - 2.0, ROW);
            self.placed.push((Hit::Item(at, item), x, row_top, width, ROW));
            row_top += ROW;
        }
    }
}

fn outline(canvas: &mut Canvas, x: f32, y: f32, width: f32, height: f32, colour: Color) {
    let (x, y, width, height) = (x as i32, y as i32, width as i32, height as i32);
    canvas.fill_rect(x, y, width, 1, colour);
    canvas.fill_rect(x, y + height - 1, width, 1, colour);
    canvas.fill_rect(x, y, 1, height, colour);
    canvas.fill_rect(x + width - 1, y, 1, height, colour);
}

fn tick_box(canvas: &mut Canvas, x: f32, y: f32, size: f32, on: bool, theme: &Theme) {
    canvas.fill_rect(x as i32, y as i32, size as i32, size as i32, theme.field);
    outline(canvas, x, y, size, size, theme.field_edge);
    if on {
        canvas.fill_rect(
            (x + 3.0) as i32,
            (y + 3.0) as i32,
            (size - 6.0) as i32,
            (size - 6.0) as i32,
            theme.accent,
        );
    }
}

/// An option button: a ring, drawn as the rows of a circle, with a dot in
/// it when it is the one chosen.
fn option_dot(canvas: &mut Canvas, x: f32, y: f32, size: f32, on: bool, theme: &Theme) {
    let radius = size / 2.0;
    let (centre_x, centre_y) = (x + radius, y + radius);
    let mut row = 0.0f32;
    while row < size {
        let dy = row + 0.5 - radius;
        let half = (radius * radius - dy * dy).max(0.0).sqrt();
        let inner = ((radius - 1.0) * (radius - 1.0) - dy * dy).max(0.0).sqrt();
        canvas.fill_rect(
            (centre_x - half) as i32,
            (y + row) as i32,
            (half * 2.0).max(1.0) as i32,
            1,
            theme.field_edge,
        );
        if inner > 0.0 {
            canvas.fill_rect(
                (centre_x - inner) as i32,
                (y + row) as i32,
                (inner * 2.0).max(1.0) as i32,
                1,
                theme.field,
            );
        }
        if on {
            let dot = ((radius - 4.0) * (radius - 4.0) - dy * dy).max(0.0).sqrt();
            if dot > 0.0 {
                canvas.fill_rect(
                    (centre_x - dot) as i32,
                    (y + row) as i32,
                    (dot * 2.0).max(1.0) as i32,
                    1,
                    theme.accent,
                );
            }
        }
        row += 1.0;
    }
    let _ = centre_y;
}

/// The small triangle that says a list drops from here.
fn chevron(canvas: &mut Canvas, x: f32, centre_y: f32, colour: Color) {
    for step in 0..4 {
        canvas.fill_rect(
            (x + step as f32) as i32,
            (centre_y - 2.0 + step as f32) as i32,
            (7 - step * 2).max(1),
            1,
            colour,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_vba::forms::Control;

    fn window() -> FormWindow {
        let mut form = Form::new("UserForm1");
        form.controls
            .push(Control::new("Label1", Kind::Label, 10.0, 10.0, 80.0, 12.0).captioned("Name"));
        let mut box_ = Control::new("TextBox1", Kind::TextBox, 10.0, 30.0, 100.0, 18.0);
        box_.tab_index = 0;
        form.controls.push(box_);
        let mut tick = Control::new("CheckBox1", Kind::CheckBox, 10.0, 60.0, 100.0, 18.0);
        tick.tab_index = 1;
        form.controls.push(tick);
        let mut ok =
            Control::new("OK", Kind::CommandButton, 150.0, 30.0, 60.0, 24.0).captioned("OK");
        ok.default = true;
        ok.tab_index = 2;
        form.controls.push(ok);
        let mut list = Control::new("ComboBox1", Kind::ComboBox, 10.0, 90.0, 100.0, 18.0);
        list.items = vec!["One".to_owned(), "Two".to_owned()];
        list.tab_index = 3;
        form.controls.push(list);
        FormWindow::new(form)
    }

    fn event(outcome: &Outcome) -> (String, String) {
        match outcome {
            Outcome::Happened(Happening::On { control, event, .. }) => {
                (control.clone(), event.clone())
            }
            other => panic!("not an event: {other:?}"),
        }
    }

    #[test]
    fn typing_goes_into_the_box_with_the_keyboard_and_is_reported() {
        let mut window = window();
        assert_eq!(window.focus, Some(1), "the keyboard starts on the first in the tab order");
        let outcome = window.character('B');
        assert_eq!(event(&outcome), ("TextBox1".to_owned(), "Change".to_owned()));
        window.character('o');
        assert_eq!(window.form.controls[1].value, "Bo");
        if let Outcome::Happened(Happening::On { values, .. }) = window.key(Key::Backspace, false) {
            assert!(values.iter().any(|(name, value, _)| name == "TextBox1" && value == "B"));
        } else {
            panic!("backspace was not reported");
        }
    }

    #[test]
    fn tab_walks_the_order_and_enter_presses_the_default_button() {
        let mut window = window();
        window.key(Key::Tab, false);
        assert_eq!(window.focus, Some(2));
        assert_eq!(event(&window.character(' ')), ("CheckBox1".to_owned(), "Click".to_owned()));
        assert!(window.form.controls[2].ticked());
        window.key(Key::Tab, true);
        assert_eq!(window.focus, Some(1));
        assert_eq!(event(&window.key(Key::Enter, false)), ("OK".to_owned(), "Click".to_owned()));
    }

    #[test]
    fn escape_with_no_cancel_button_shuts_the_form() {
        let mut window = window();
        assert_eq!(window.key(Key::Escape, false), Outcome::Happened(Happening::Closed));
    }

    #[test]
    fn a_combo_box_walks_its_list_with_the_arrows() {
        let mut window = window();
        window.focus = Some(4);
        assert_eq!(
            event(&window.key(Key::Down, false)),
            ("ComboBox1".to_owned(), "Change".to_owned())
        );
        assert_eq!(window.form.controls[4].value, "One");
        window.key(Key::Down, false);
        assert_eq!(window.form.controls[4].list_index, 1);
    }

    #[test]
    fn the_macros_copy_replaces_the_windows_and_keeps_the_keyboard_where_it_was() {
        let mut window = window();
        window.focus = Some(3);
        let mut form = window.form.clone();
        form.controls[1].value = "from the macro".to_owned();
        window.replace(form);
        assert_eq!(window.focus, Some(3));
        assert_eq!(window.form.controls[1].value, "from the macro");
    }
}
