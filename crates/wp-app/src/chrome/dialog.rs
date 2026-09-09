//! Dialogs: the box that asks a question a click cannot answer.
//!
//! # Why there was none
//!
//! Everything this program draws is drawn by this program, on one canvas, and
//! there was no machinery for a box that takes the keyboard, holds fields, and
//! comes back with an answer. So the questions Word asks in a dialog have been
//! asked in a strip along the top of the window — which works, and is not what
//! Word does, and cannot hold half of what a dialog holds.
//!
//! # What a dialog is here
//!
//! A panel drawn over the document, with the document dimmed behind it. Not a
//! window of the operating system: everything else in this program is drawn on
//! the one canvas, and a dialog drawn the same way works on any machine the
//! program is ever ported to and can be photographed for a test. What it gives
//! up is being dragged outside the window, which Word's can be.
//!
//! What it keeps is everything else a person expects: a title, fields that take
//! the keyboard in order, Tab and Shift+Tab between them, Space to tick a box,
//! the arrows inside a list, Enter for the button that is in bold, Escape to
//! cancel, and a click anywhere outside that does nothing at all — because a
//! modal dialog is modal.

use wp_layout::{LayoutEngine, Renderer};
use wp_raster::{Canvas, Color};
use wp_shell::Key;

use super::icons::{self, Icon};
use super::theme::Theme;

/// How wide a dialog is drawn, unless it says otherwise.
const WIDTH: f32 = 420.0;

/// The height of one row of the body.
const ROW: f32 = 30.0;

/// The height of a box that takes typing.
const BOX_HEIGHT: f32 = 24.0;

/// The room round everything inside the panel.
const PADDING: f32 = 16.0;

/// The bar along the top of the panel.
const TITLE_HEIGHT: f32 = 34.0;

/// The bar along the bottom, where the buttons are.
const FOOTER_HEIGHT: f32 = 48.0;

/// One thing a dialog asks about.
#[derive(Clone, Debug, PartialEq)]
pub enum Field {
    /// A heading inside the body, which nothing can land on.
    Heading(String),
    /// A line of text the dialog is telling rather than asking.
    Said { label: String, value: String },
    /// A box with words in it.
    Text { label: String, value: String },
    /// A box with a number in it, and what the number is measured in.
    Number { label: String, value: String, unit: &'static str },
    /// A box that is ticked or not.
    Check { label: String, on: bool },
    /// One of several, shown as a list that drops open.
    Choice { label: String, items: Vec<String>, current: usize },
}

impl Field {
    /// Whether the keyboard can land on it.
    #[must_use]
    pub fn takes_focus(&self) -> bool {
        !matches!(self, Self::Heading(_) | Self::Said { .. })
    }

    /// The word down the left-hand column, for the fields that have one.
    ///
    /// A heading spans the panel and a tick box labels itself, so neither takes
    /// room in that column.
    #[must_use]
    pub fn label(&self) -> Option<&str> {
        match self {
            Self::Heading(_) | Self::Check { .. } => None,
            Self::Said { label, .. }
            | Self::Text { label, .. }
            | Self::Number { label, .. }
            | Self::Choice { label, .. } => Some(label),
        }
    }
}

/// What a button does when it is pressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    /// Take what the fields say and act on it.
    Accept,
    /// Leave everything as it was.
    Cancel,
}

/// A button along the bottom of the dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Button {
    pub label: String,
    pub answer: Answer,
    /// Whether Enter presses it. Word draws that one with a ring round it.
    pub default: bool,
}

/// What the keyboard or a click did to the dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reaction {
    /// Nothing this dialog knows about.
    Ignored,
    /// Something changed and the window has to be drawn again.
    Changed,
    /// The dialog is finished with, one way or the other.
    Closed(Answer),
}

/// Where the mouse can land.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Hit {
    Field(usize),
    Button(usize),
    Close,
}

/// A dialog, and everything it is asking.
#[derive(Clone, Debug)]
pub struct Dialog {
    pub title: String,
    pub fields: Vec<Field>,
    pub buttons: Vec<Button>,
    /// Which field the keyboard is on, or, past the end of them, which button.
    focus: usize,
    /// Which list is dropped open, if any.
    open_list: Option<usize>,
    placed: Vec<(Hit, f32, f32, f32, f32)>,
    hovered: Option<Hit>,
    width: f32,
}

impl Dialog {
    /// A dialog with the two buttons every dialog has.
    #[must_use]
    pub fn new(title: &str, fields: Vec<Field>) -> Self {
        Self::with_buttons(
            title,
            fields,
            vec![
                Button { label: "OK".to_owned(), answer: Answer::Accept, default: true },
                Button { label: "Cancel".to_owned(), answer: Answer::Cancel, default: false },
            ],
        )
    }

    /// A dialog that is telling rather than asking, and so has one button.
    #[must_use]
    pub fn message(title: &str, fields: Vec<Field>) -> Self {
        Self::with_buttons(
            title,
            fields,
            vec![Button { label: "Close".to_owned(), answer: Answer::Accept, default: true }],
        )
    }

    #[must_use]
    pub fn with_buttons(title: &str, fields: Vec<Field>, buttons: Vec<Button>) -> Self {
        let mut dialog = Self {
            title: title.to_owned(),
            fields,
            buttons,
            focus: 0,
            open_list: None,
            placed: Vec::new(),
            hovered: None,
            width: WIDTH,
        };
        // The keyboard starts on the first thing that can take it, which is
        // where a person expects to start typing.
        dialog.focus = dialog.first_focus();
        dialog
    }

    /// Whether a box is ticked.
    #[must_use]
    pub fn ticked(&self, index: usize) -> bool {
        matches!(self.fields.get(index), Some(Field::Check { on: true, .. }))
    }

    /// What was typed into a box, or chosen from a list.
    #[must_use]
    pub fn said(&self, index: usize) -> String {
        match self.fields.get(index) {
            Some(Field::Text { value, .. } | Field::Number { value, .. }) => value.clone(),
            Some(Field::Choice { items, current, .. }) => {
                items.get(*current).cloned().unwrap_or_default()
            }
            _ => String::new(),
        }
    }

    /// Which of a list was chosen.
    #[must_use]
    pub fn chose(&self, index: usize) -> usize {
        match self.fields.get(index) {
            Some(Field::Choice { current, .. }) => *current,
            _ => 0,
        }
    }

    /// The first thing the keyboard can land on.
    fn first_focus(&self) -> usize {
        self.fields.iter().position(Field::takes_focus).unwrap_or(self.fields.len())
    }

    /// How many places the keyboard can be: the fields it can land on, and then
    /// the buttons.
    fn stops(&self) -> Vec<usize> {
        let mut out: Vec<usize> =
            (0..self.fields.len()).filter(|index| self.fields[*index].takes_focus()).collect();
        out.extend(self.fields.len()..self.fields.len() + self.buttons.len());
        out
    }

    /// Moves the keyboard on, or back.
    fn step_focus(&mut self, forwards: bool) {
        let stops = self.stops();
        if stops.is_empty() {
            return;
        }
        let at = stops.iter().position(|stop| *stop == self.focus).unwrap_or(0);
        let next =
            if forwards { (at + 1) % stops.len() } else { (at + stops.len() - 1) % stops.len() };
        self.focus = stops[next];
    }

    /// A key pressed while the dialog is up.
    pub fn key(&mut self, key: Key, shift: bool) -> Reaction {
        // A list that is dropped open takes the keyboard until it is done with.
        if let Some(index) = self.open_list {
            return match key {
                Key::Up | Key::Down => {
                    let step = if key == Key::Down { 1i32 } else { -1 };
                    self.move_choice(index, step);
                    Reaction::Changed
                }
                Key::Enter | Key::Escape => {
                    self.open_list = None;
                    Reaction::Changed
                }
                _ => Reaction::Ignored,
            };
        }

        match key {
            Key::Tab => {
                self.step_focus(!shift);
                Reaction::Changed
            }
            Key::Escape => Reaction::Closed(Answer::Cancel),
            Key::Enter => {
                // Enter on a button presses it; anywhere else it presses the
                // one in bold, which is what a dialog's Enter means.
                if let Some(button) = self.focused_button() {
                    return Reaction::Closed(self.buttons[button].answer);
                }
                let default = self.buttons.iter().find(|button| button.default);
                default.map_or(Reaction::Ignored, |button| Reaction::Closed(button.answer))
            }
            Key::Up | Key::Down => {
                // The arrows walk a list without dropping it open, as they do
                // in Word.
                let step = if key == Key::Down { 1i32 } else { -1 };
                if matches!(self.fields.get(self.focus), Some(Field::Choice { .. })) {
                    self.move_choice(self.focus, step);
                    return Reaction::Changed;
                }
                self.step_focus(key == Key::Down);
                Reaction::Changed
            }
            Key::Backspace => {
                if let Some(Field::Text { value, .. } | Field::Number { value, .. }) =
                    self.fields.get_mut(self.focus)
                {
                    value.pop();
                    return Reaction::Changed;
                }
                Reaction::Ignored
            }
            _ => Reaction::Ignored,
        }
    }

    /// A character typed while the dialog is up.
    pub fn character(&mut self, character: char) -> Reaction {
        // Space ticks the box the keyboard is on, as it does everywhere.
        if character == ' ' {
            if let Some(Field::Check { on, .. }) = self.fields.get_mut(self.focus) {
                *on = !*on;
                return Reaction::Changed;
            }
            if let Some(button) = self.focused_button() {
                return Reaction::Closed(self.buttons[button].answer);
            }
        }
        if character.is_control() {
            return Reaction::Ignored;
        }

        match self.fields.get_mut(self.focus) {
            Some(Field::Text { value, .. }) => {
                value.push(character);
                Reaction::Changed
            }
            // A number box takes numbers, a point, and a minus at the front.
            Some(Field::Number { value, .. }) => {
                let allowed = character.is_ascii_digit()
                    || (character == '.' && !value.contains('.'))
                    || (character == '-' && value.is_empty());
                if !allowed {
                    return Reaction::Ignored;
                }
                value.push(character);
                Reaction::Changed
            }
            _ => Reaction::Ignored,
        }
    }

    /// A press somewhere in the window.
    pub fn press(&mut self, x: i32, y: i32) -> Reaction {
        // A list dropped open takes the press, wherever it lands.
        if let Some(index) = self.open_list {
            let chosen = self.list_row_at(index, x, y);
            self.open_list = None;
            if let Some(row) = chosen {
                if let Some(Field::Choice { current, .. }) = self.fields.get_mut(index) {
                    *current = row;
                }
            }
            return Reaction::Changed;
        }

        match self.at(x, y) {
            Some(Hit::Close) => Reaction::Closed(Answer::Cancel),
            Some(Hit::Button(index)) => Reaction::Closed(self.buttons[index].answer),
            Some(Hit::Field(index)) => {
                self.focus = index;
                match self.fields.get_mut(index) {
                    Some(Field::Check { on, .. }) => *on = !*on,
                    Some(Field::Choice { .. }) => self.open_list = Some(index),
                    _ => {}
                }
                Reaction::Changed
            }
            // Inside the panel but on nothing, or outside it altogether: a
            // modal dialog swallows the press rather than letting it reach the
            // document behind.
            _ => Reaction::Changed,
        }
    }

    /// Follows the pointer. True when something has to be drawn again.
    pub fn hover(&mut self, x: i32, y: i32) -> bool {
        let over = self.at(x, y);
        let changed = over != self.hovered;
        self.hovered = over;
        changed
    }

    fn at(&self, x: i32, y: i32) -> Option<Hit> {
        let (x, y) = (x as f32, y as f32);
        self.placed
            .iter()
            .find(|(_, left, top, width, height)| {
                x >= *left && x < left + width && y >= *top && y < top + height
            })
            .map(|(hit, ..)| *hit)
    }

    fn focused_button(&self) -> Option<usize> {
        self.focus.checked_sub(self.fields.len()).filter(|index| *index < self.buttons.len())
    }

    fn move_choice(&mut self, index: usize, step: i32) {
        if let Some(Field::Choice { items, current, .. }) = self.fields.get_mut(index) {
            if items.is_empty() {
                return;
            }
            let last = items.len() as i32 - 1;
            let wanted = (*current as i32 + step).clamp(0, last);
            *current = wanted as usize;
        }
    }

    /// Which row of a dropped-open list a point is on.
    fn list_row_at(&self, index: usize, x: i32, y: i32) -> Option<usize> {
        let (left, top, width, _) = self.rect_of(Hit::Field(index))?;
        let Some(Field::Choice { items, .. }) = self.fields.get(index) else { return None };
        let (x, y) = (x as f32, y as f32);
        if x < left || x > left + width {
            return None;
        }
        let row = ((y - (top + BOX_HEIGHT)) / ROW).floor();
        if row < 0.0 {
            return None;
        }
        let row = row as usize;
        (row < items.len()).then_some(row)
    }

    fn rect_of(&self, hit: Hit) -> Option<(f32, f32, f32, f32)> {
        self.placed
            .iter()
            .find(|(found, ..)| *found == hit)
            .map(|(_, left, top, width, height)| (*left, *top, *width, *height))
    }

    /// How tall the panel is, from what is in it.
    fn height(&self) -> f32 {
        let body: f32 = self
            .fields
            .iter()
            .map(|field| match field {
                Field::Heading(_) => ROW,
                _ => ROW + 4.0,
            })
            .sum();
        TITLE_HEIGHT + PADDING + body + PADDING + FOOTER_HEIGHT
    }

    /// Draws the dialog over the window, with everything behind it dimmed.
    pub fn draw(
        &mut self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        theme: &Theme,
    ) {
        self.placed.clear();
        let (window_width, window_height) = (canvas.width() as f32, canvas.height() as f32);

        // The document behind is dimmed, which is what says that it cannot be
        // reached until this is answered.
        canvas.fill_rect(0, 0, window_width as i32, window_height as i32, Color::rgba(0, 0, 0, 90));

        let width = self.width;
        let height = self.height();
        let left = ((window_width - width) / 2.0).max(0.0);
        let top = ((window_height - height) / 2.0).max(0.0);

        // A shadow under the panel, so it reads as being in front rather than
        // painted on.
        canvas.fill_rect(
            (left + 4.0) as i32,
            (top + 4.0) as i32,
            width as i32,
            height as i32,
            Color::rgba(0, 0, 0, 60),
        );
        canvas.fill_rect(left as i32, top as i32, width as i32, height as i32, theme.pane);
        outline(canvas, left, top, width, height, theme.pane_edge);

        self.draw_title(canvas, engine, renderer, left, top, width, theme);
        let after_body = self.draw_body(canvas, engine, renderer, left, top, width, theme);
        self.draw_buttons(canvas, engine, renderer, left, top + height, width, theme);
        let _ = after_body;

        // The open list goes over everything, including the buttons.
        if let Some(index) = self.open_list {
            self.draw_open_list(canvas, engine, renderer, index, theme);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_title(
        &mut self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        left: f32,
        top: f32,
        width: f32,
        theme: &Theme,
    ) {
        // A dialog's caption is the same surface as the dialog itself, with a
        // hairline under it — which is how Word's dialogs come up on Windows,
        // where the caption belongs to the system rather than to the ribbon.
        canvas.fill_rect(left as i32, top as i32, width as i32, TITLE_HEIGHT as i32, theme.pane);
        canvas.fill_rect(
            left as i32,
            (top + TITLE_HEIGHT) as i32 - 1,
            width as i32,
            1,
            theme.pane_edge,
        );
        let line = engine.simple_line(&self.title, left + PADDING, top + 22.0, 10.0, theme.text);
        renderer.draw_onto(canvas, &line, 0.0, 0.0);

        // The cross at the right-hand end, which every dialog has.
        let close_left = left + width - 28.0;
        if self.hovered == Some(Hit::Close) {
            canvas.fill_rect(close_left as i32 - 4, top as i32 + 6, 24, 22, theme.hover);
        }
        icons::draw_sized(canvas, Icon::Close, close_left, top + 10.0, 14.0, theme.text);
        self.placed.push((Hit::Close, close_left - 4.0, top + 6.0, 24.0, 22.0));
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_body(
        &mut self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        left: f32,
        top: f32,
        width: f32,
        theme: &Theme,
    ) -> f32 {
        // The labels line up in one column, as wide as the widest of them.
        // A fixed column would either waste room or — with a label as long as
        // "Characters (no spaces)" — run into what stands beside it.
        let widest = self
            .fields
            .iter()
            .filter_map(Field::label)
            .map(|label| engine.simple_line(label, 0.0, 0.0, 9.0, theme.text).width)
            .fold(0.0f32, f32::max);
        let room = width - PADDING * 2.0;
        let label_width = (widest + PADDING).clamp(130.0, (room - 120.0).max(130.0));
        let mut y = top + TITLE_HEIGHT + PADDING;

        for index in 0..self.fields.len() {
            let field = self.fields[index].clone();
            let focused = self.focus == index;
            let box_left = left + PADDING + label_width;
            let box_width = width - PADDING * 2.0 - label_width;

            match field {
                Field::Heading(text) => {
                    let line = engine.simple_line(&text, left + PADDING, y + 16.0, 9.0, theme.text);
                    renderer.draw_onto(canvas, &line, 0.0, 0.0);
                    canvas.fill_rect(
                        (left + PADDING) as i32,
                        (y + 22.0) as i32,
                        (width - PADDING * 2.0) as i32,
                        1,
                        theme.pane_edge,
                    );
                    y += ROW;
                    continue;
                }
                Field::Said { label, value } => {
                    let line =
                        engine.simple_line(&label, left + PADDING, y + 16.0, 9.0, theme.text);
                    renderer.draw_onto(canvas, &line, 0.0, 0.0);
                    let line = engine.simple_line(&value, box_left, y + 16.0, 9.0, theme.text);
                    renderer.draw_onto(canvas, &line, 0.0, 0.0);
                    y += ROW + 4.0;
                    continue;
                }
                Field::Check { label, on } => {
                    // The box, then the label beside it: a tick box labels
                    // itself, which is why it has no label down the left.
                    let size = 16.0;
                    let boxes_left = left + PADDING;
                    canvas.fill_rect(
                        boxes_left as i32,
                        (y + 4.0) as i32,
                        size as i32,
                        size as i32,
                        theme.field,
                    );
                    outline(
                        canvas,
                        boxes_left,
                        y + 4.0,
                        size,
                        size,
                        if focused { theme.accent } else { theme.field_edge },
                    );
                    if on {
                        canvas.fill_rect(
                            (boxes_left + 4.0) as i32,
                            (y + 8.0) as i32,
                            (size - 8.0) as i32,
                            (size - 8.0) as i32,
                            theme.accent,
                        );
                    }
                    let line = engine.simple_line(
                        &label,
                        boxes_left + size + 8.0,
                        y + 16.0,
                        9.0,
                        theme.text,
                    );
                    renderer.draw_onto(canvas, &line, 0.0, 0.0);
                    self.placed.push((
                        Hit::Field(index),
                        boxes_left,
                        y,
                        width - PADDING * 2.0,
                        ROW,
                    ));
                    y += ROW + 4.0;
                    continue;
                }
                Field::Text { label, value } | Field::Number { label, value, .. } => {
                    let line =
                        engine.simple_line(&label, left + PADDING, y + 17.0, 9.0, theme.text);
                    renderer.draw_onto(canvas, &line, 0.0, 0.0);
                    canvas.fill_rect(
                        box_left as i32,
                        y as i32,
                        box_width as i32,
                        BOX_HEIGHT as i32,
                        theme.field,
                    );
                    outline(
                        canvas,
                        box_left,
                        y,
                        box_width,
                        BOX_HEIGHT,
                        if focused { theme.accent } else { theme.field_edge },
                    );
                    let unit = match &self.fields[index] {
                        Field::Number { unit, .. } => *unit,
                        _ => "",
                    };
                    let shown = with_unit(&value, unit);
                    let typed =
                        engine.simple_line(&shown, box_left + 6.0, y + 17.0, 9.0, theme.text);
                    let measured = typed.width - (box_left + 6.0);
                    renderer.draw_onto(canvas, &typed, 0.0, 0.0);
                    if focused {
                        let caret = box_left + 6.0 + measured - unit_width(unit, measured, &shown);
                        canvas.fill_rect(caret as i32, (y + 5.0) as i32, 1, 14, theme.text);
                    }
                    self.placed.push((Hit::Field(index), box_left, y, box_width, BOX_HEIGHT));
                    y += ROW + 4.0;
                    continue;
                }
                Field::Choice { label, items, current } => {
                    let line =
                        engine.simple_line(&label, left + PADDING, y + 17.0, 9.0, theme.text);
                    renderer.draw_onto(canvas, &line, 0.0, 0.0);
                    canvas.fill_rect(
                        box_left as i32,
                        y as i32,
                        box_width as i32,
                        BOX_HEIGHT as i32,
                        theme.field,
                    );
                    outline(
                        canvas,
                        box_left,
                        y,
                        box_width,
                        BOX_HEIGHT,
                        if focused { theme.accent } else { theme.field_edge },
                    );
                    let chosen = items.get(current).cloned().unwrap_or_default();
                    let line =
                        engine.simple_line(&chosen, box_left + 6.0, y + 17.0, 9.0, theme.text);
                    renderer.draw_onto(canvas, &line, 0.0, 0.0);
                    chevron(canvas, box_left + box_width - 16.0, y + BOX_HEIGHT / 2.0, theme.text);
                    self.placed.push((Hit::Field(index), box_left, y, box_width, BOX_HEIGHT));
                    y += ROW + 4.0;
                    continue;
                }
            }
        }
        y
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_buttons(
        &mut self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        left: f32,
        bottom: f32,
        width: f32,
        theme: &Theme,
    ) {
        let height = 26.0f32;
        let y = bottom - FOOTER_HEIGHT + (FOOTER_HEIGHT - height) / 2.0;
        let mut right = left + width - PADDING;

        // Right to left, so that the first button in the list ends up nearest
        // the right-hand edge — which is where OK goes.
        for index in (0..self.buttons.len()).rev() {
            let button = self.buttons[index].clone();
            let measured =
                engine.simple_line(&button.label, 0.0, 0.0, 9.0, theme.text).width + 32.0;
            let button_width = measured.max(80.0);
            let button_left = right - button_width;

            let focused = self.focus == self.fields.len() + index;
            let background = if button.default || focused { theme.accent } else { theme.field };
            canvas.fill_rect(
                button_left as i32,
                y as i32,
                button_width as i32,
                height as i32,
                background,
            );
            if focused {
                outline(
                    canvas,
                    button_left - 2.0,
                    y - 2.0,
                    button_width + 4.0,
                    height + 4.0,
                    theme.accent,
                );
            } else if !button.default {
                outline(canvas, button_left, y, button_width, height, theme.field_edge);
            }

            let ink = if button.default || focused { theme.on_accent() } else { theme.text };
            let line = engine.simple_line(&button.label, 0.0, 0.0, 9.0, ink);
            let text_width = line.width;
            let line = engine.simple_line(
                &button.label,
                button_left + (button_width - text_width) / 2.0,
                y + 17.0,
                9.0,
                ink,
            );
            renderer.draw_onto(canvas, &line, 0.0, 0.0);

            self.placed.push((Hit::Button(index), button_left, y, button_width, height));
            right = button_left - 8.0;
        }
    }

    fn draw_open_list(
        &mut self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        index: usize,
        theme: &Theme,
    ) {
        let Some((left, top, width, _)) = self.rect_of(Hit::Field(index)) else { return };
        let Some(Field::Choice { items, current, .. }) = self.fields.get(index).cloned() else {
            return;
        };

        let height = items.len() as f32 * ROW;
        let list_top = top + BOX_HEIGHT;
        canvas.fill_rect(left as i32, list_top as i32, width as i32, height as i32, theme.field);
        outline(canvas, left, list_top, width, height, theme.pane_edge);

        for (row, item) in items.iter().enumerate() {
            let y = list_top + row as f32 * ROW;
            if row == current {
                canvas.fill_rect(left as i32, y as i32, width as i32, ROW as i32, theme.hover);
            }
            let line = engine.simple_line(item, left + 8.0, y + 20.0, 9.0, theme.text);
            renderer.draw_onto(canvas, &line, 0.0, 0.0);
        }
    }
}

/// A number with what it is measured in written after it.
///
/// Word writes an inch as `1"`, with the mark against the digit, and a
/// centimetre as `2.54 cm`, with a space. So the space belongs to the word
/// rather than to the number, and a unit that is a mark gets none.
fn with_unit(value: &str, unit: &str) -> String {
    if unit.is_empty() {
        return value.to_owned();
    }
    if unit.chars().all(|character| !character.is_alphabetic()) {
        return format!("{value}{unit}");
    }
    format!("{value} {unit}")
}

/// How much of a measured width is the unit rather than the number.
///
/// The caret goes after the number, not after the unit: a person typing inches
/// is typing the number.
fn unit_width(unit: &str, measured: f32, shown: &str) -> f32 {
    let all = shown.chars().count();
    // Whatever `with_unit` put on the end: the unit, and the space before it
    // when it took one. Asking the same function is what keeps the two from
    // disagreeing.
    let tail = with_unit("", unit).chars().count();
    if tail == 0 || tail >= all {
        return 0.0;
    }
    // Proportional to the characters, which is near enough for a caret.
    measured * tail as f32 / all as f32
}

fn outline(canvas: &mut Canvas, x: f32, y: f32, width: f32, height: f32, colour: Color) {
    let (x, y, width, height) = (x as i32, y as i32, width as i32, height as i32);
    canvas.fill_rect(x, y, width, 1, colour);
    canvas.fill_rect(x, y + height - 1, width, 1, colour);
    canvas.fill_rect(x, y, 1, height, colour);
    canvas.fill_rect(x + width - 1, y, 1, height, colour);
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
    use super::{Answer, Dialog, Field, Reaction};
    use wp_shell::Key;

    fn dialog() -> Dialog {
        Dialog::new(
            "Test",
            vec![
                Field::Heading("A heading".to_owned()),
                Field::Text { label: "Name".to_owned(), value: String::new() },
                Field::Check { label: "Ticked".to_owned(), on: false },
                Field::Choice {
                    label: "Which".to_owned(),
                    items: vec!["First".to_owned(), "Second".to_owned()],
                    current: 0,
                },
            ],
        )
    }

    #[test]
    fn the_keyboard_starts_on_the_first_field_it_can_land_on() {
        // Not the heading, which is not something to fill in.
        let dialog = dialog();
        assert_eq!(dialog.focus, 1);
    }

    #[test]
    fn tab_walks_the_fields_and_then_the_buttons_and_comes_round() {
        let mut dialog = dialog();
        let mut seen = vec![dialog.focus];
        for _ in 0..5 {
            dialog.key(Key::Tab, false);
            seen.push(dialog.focus);
        }
        // Three fields, two buttons, and then back to the first field.
        assert_eq!(seen, vec![1, 2, 3, 4, 5, 1]);
    }

    #[test]
    fn shift_and_tab_walk_the_other_way() {
        let mut dialog = dialog();
        dialog.key(Key::Tab, true);
        assert_eq!(dialog.focus, 5, "the last button");
        dialog.key(Key::Tab, true);
        assert_eq!(dialog.focus, 4);
    }

    #[test]
    fn typing_goes_into_the_box_the_keyboard_is_on() {
        let mut dialog = dialog();
        for character in "Hello".chars() {
            dialog.character(character);
        }
        assert_eq!(dialog.said(1), "Hello");
        dialog.key(Key::Backspace, false);
        assert_eq!(dialog.said(1), "Hell");
    }

    #[test]
    fn a_number_box_takes_a_number_and_nothing_else() {
        let mut dialog = Dialog::new(
            "Test",
            vec![Field::Number { label: "Width".to_owned(), value: String::new(), unit: "\"" }],
        );
        for character in "1a2.5.x-".chars() {
            dialog.character(character);
        }
        assert_eq!(dialog.said(0), "12.5");
    }

    #[test]
    fn space_ticks_the_box_the_keyboard_is_on() {
        let mut dialog = dialog();
        dialog.key(Key::Tab, false);
        assert_eq!(dialog.focus, 2);
        assert!(!dialog.ticked(2));
        dialog.character(' ');
        assert!(dialog.ticked(2));
        dialog.character(' ');
        assert!(!dialog.ticked(2));
    }

    #[test]
    fn the_arrows_walk_a_list_without_dropping_it_open() {
        let mut dialog = dialog();
        dialog.key(Key::Tab, false);
        dialog.key(Key::Tab, false);
        assert_eq!(dialog.focus, 3);
        assert_eq!(dialog.chose(3), 0);
        dialog.key(Key::Down, false);
        assert_eq!(dialog.chose(3), 1);
        // And stops at the end rather than coming round, as a list does.
        dialog.key(Key::Down, false);
        assert_eq!(dialog.chose(3), 1);
        dialog.key(Key::Up, false);
        assert_eq!(dialog.chose(3), 0);
    }

    #[test]
    fn enter_presses_the_button_in_bold_and_escape_cancels() {
        let mut dialog = dialog();
        assert_eq!(dialog.key(Key::Enter, false), Reaction::Closed(Answer::Accept));
        assert_eq!(dialog.key(Key::Escape, false), Reaction::Closed(Answer::Cancel));
    }

    #[test]
    fn enter_on_a_button_presses_that_one_rather_than_the_default() {
        let mut dialog = dialog();
        // Walk to Cancel, which is the second button.
        for _ in 0..4 {
            dialog.key(Key::Tab, false);
        }
        assert_eq!(dialog.focus, 5);
        assert_eq!(dialog.key(Key::Enter, false), Reaction::Closed(Answer::Cancel));
    }

    #[test]
    fn a_dialog_that_is_telling_rather_than_asking_has_one_button() {
        let dialog = Dialog::message(
            "Word Count",
            vec![Field::Said { label: "Words".to_owned(), value: "417".to_owned() }],
        );
        assert_eq!(dialog.buttons.len(), 1);
        assert!(dialog.buttons[0].default);
    }

    #[test]
    fn a_mark_goes_against_the_number_and_a_word_after_a_space() {
        // Word writes an inch as 1" and a centimetre as 2.54 cm.
        assert_eq!(super::with_unit("1", "\""), "1\"");
        assert_eq!(super::with_unit("2.54", "cm"), "2.54 cm");
        assert_eq!(super::with_unit("3", ""), "3");
    }

    #[test]
    fn the_caret_sits_after_the_number_rather_than_after_the_unit() {
        // Half the width for half the characters: `1"` is two characters, one
        // of which is the mark.
        let width = super::unit_width("\"", 20.0, "1\"");
        assert!((width - 10.0).abs() < 0.001, "{width}");
        // Nothing to skip when the box carries no unit.
        assert_eq!(super::unit_width("", 20.0, "12"), 0.0);
        // Nor when the unit is all there is.
        assert_eq!(super::unit_width("cm", 20.0, " cm"), 0.0);
    }

    #[test]
    fn the_label_column_is_the_one_the_widest_label_needs() {
        // The column is measured from the labels, so a long one cannot run into
        // what stands beside it. Only the fields with a label down the left are
        // counted: a heading spans the panel and a tick box labels itself.
        let dialog = Dialog::message(
            "Word Count",
            vec![
                Field::Heading("Counts".to_owned()),
                Field::Said {
                    label: "Characters (no spaces)".to_owned(),
                    value: "1908".to_owned(),
                },
                Field::Check { label: "Include footnotes".to_owned(), on: false },
            ],
        );
        let labels: Vec<&str> = dialog.fields.iter().filter_map(Field::label).collect();
        assert_eq!(labels, vec!["Characters (no spaces)"]);
    }
}
