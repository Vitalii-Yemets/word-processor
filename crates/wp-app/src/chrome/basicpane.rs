//! Word's Visual Basic Editor: the project, the code and the debugger.
//!
//! # Why it is a page and not a window
//!
//! Word opens a second application window for this, with its own title bar
//! and its own taskbar button. This program draws one window and everything
//! in it, so the editor is a page that fills the window — the same answer the
//! Print page gives, and for the same reason: what is being done has nothing
//! to do with the document's own page, and half a window of each would serve
//! neither.
//!
//! # What is on it
//!
//! The project down the left, a module at a time in the middle, and the two
//! things a debugger is for along the bottom: the Immediate window, where a
//! line of Visual Basic is typed and answered, and the Watch, where whatever
//! the stopped procedure can see is listed. A dot in the margin is a
//! breakpoint; an arrow in the margin is where the macro has stopped.
//!
//! # What this file is and is not
//!
//! The view: where everything goes, what it looks like, and what was pressed.
//! Nothing here runs a macro or changes a document — see
//! [`crate::editor::basic`] for that, and [`crate::editor::debugger`] for
//! what stopping in the middle takes.

use wp_layout::{LayoutEngine, Renderer};
use wp_raster::{Canvas, Color};

use super::theme::Theme;
use crate::messages::t;

/// How wide the project tree down the left is.
pub const TREE_WIDTH: f32 = 220.0;

/// How wide the margin beside the code is, where a breakpoint is drawn.
pub const MARGIN: f32 = 26.0;

/// How tall the strip along the bottom is.
pub const BOTTOM: f32 = 150.0;

/// How tall one line of code is drawn, and what size it is drawn at.
pub const LINE: f32 = 16.0;
pub const TEXT: f32 = 9.0;

/// The room round everything.
const PADDING: f32 = 8.0;

/// What was pressed on the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pressed {
    /// The cross, or Escape.
    Close,
    /// One of the modules in the tree.
    Module(usize),
    /// A place in the code: which line, and how far along it.
    Code(usize, usize),
    /// The margin beside a line, which turns a breakpoint on or off.
    Margin(usize),
    /// The box along the bottom that a line is typed into.
    Immediate,
    /// Nothing that does anything.
    Nothing,
}

/// The Visual Basic editor, as it is on the screen.
#[derive(Clone, Debug, Default)]
pub struct BasicPane {
    /// What the project is called, and what is in it.
    pub project: String,
    pub modules: Vec<String>,
    /// Which module is being looked at.
    pub showing: usize,
    /// Its text, a line at a time, which is what is edited.
    pub lines: Vec<String>,
    /// Where the caret is: which line, and how many characters along.
    pub caret: (usize, usize),
    /// The first line showing.
    pub scroll: usize,
    /// Which lines have a breakpoint on them, counting from one.
    pub breakpoints: std::collections::BTreeSet<usize>,
    /// Which line the macro has stopped on, if it has.
    pub stopped: Option<usize>,
    /// What is being typed into the Immediate window, and what it has said.
    pub immediate: String,
    pub answers: Vec<String>,
    /// Whether the caret is in the Immediate window rather than in the code.
    pub in_immediate: bool,
    /// What the stopped procedure can see.
    pub watched: Vec<(String, String)>,
    /// How many lines fit, worked out when it was last drawn.
    pub room: usize,
    /// Where the page begins, under the window's title bar.
    pub top: f32,
}

impl BasicPane {
    /// The editor showing a project's modules.
    #[must_use]
    pub fn new(project: &str, modules: Vec<String>) -> Self {
        Self {
            project: project.to_owned(),
            modules,
            room: 20,
            lines: vec![String::new()],
            ..Self::default()
        }
    }

    /// The text of the module being edited, as one string.
    #[must_use]
    pub fn text(&self) -> String {
        // Written back with the line endings Visual Basic uses, whatever was
        // typed: a module with Unix endings is a module Word shows as one
        // long line.
        self.lines.join("\r\n")
    }

    /// Puts a module's text in, as lines.
    pub fn show(&mut self, source: &str) {
        self.lines = source.replace("\r\n", "\n").split('\n').map(str::to_owned).collect();
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        self.caret = (0, 0);
        self.scroll = 0;
    }

    /// Keeps the caret inside the text and the text inside the window.
    pub fn settle(&mut self) {
        self.caret.0 = self.caret.0.min(self.lines.len().saturating_sub(1));
        let length = self.lines[self.caret.0].chars().count();
        self.caret.1 = self.caret.1.min(length);
        if self.caret.0 < self.scroll {
            self.scroll = self.caret.0;
        } else if self.caret.0 >= self.scroll + self.room {
            self.scroll = self.caret.0 + 1 - self.room;
        }
    }

    /// Where the cross that shuts the page is.
    #[must_use]
    pub fn cross(&self, width: f32) -> (f32, f32, f32) {
        (width - PADDING - LINE, self.top + PADDING, LINE)
    }

    /// What is at a point, for a press.
    #[must_use]
    pub fn at(&self, x: f32, y: f32, width: f32, height: f32) -> Pressed {
        let (left, top, size) = self.cross(width);
        if x >= left && x <= left + size && y >= top && y <= top + size {
            return Pressed::Close;
        }
        let bottom = height - BOTTOM;
        if y > bottom {
            return Pressed::Immediate;
        }
        if x < TREE_WIDTH {
            let row = ((y - self.top - PADDING - LINE) / LINE).floor();
            if row < 0.0 {
                return Pressed::Nothing;
            }
            let index = row as usize;
            return if index < self.modules.len() {
                Pressed::Module(index)
            } else {
                Pressed::Nothing
            };
        }
        let _ = width;

        let row = ((y - self.top - PADDING) / LINE).floor().max(0.0) as usize + self.scroll;
        if row >= self.lines.len() {
            return Pressed::Nothing;
        }
        if x < TREE_WIDTH + MARGIN {
            return Pressed::Margin(row + 1);
        }
        // How far along the line the press is: measured rather than guessed,
        // because the letters are not all the same width.
        Pressed::Code(row, usize::MAX)
    }

    /// Draws the whole page.
    pub fn draw(
        &mut self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        theme: &Theme,
        top: f32,
        width: usize,
        height: usize,
    ) {
        #[allow(clippy::cast_precision_loss)]
        let (width, height) = (width as f32, height as f32);
        // Under the window's own title bar, which belongs to the window and
        // not to what is in it.
        self.top = top;
        canvas.fill_rect(0, top as i32, width as i32, (height - top) as i32, theme.pane);
        self.room = ((height - top - BOTTOM - PADDING * 2.0) / LINE).max(1.0) as usize;

        self.draw_tree(canvas, engine, renderer, theme, height);
        self.draw_code(canvas, engine, renderer, theme, width, height);
        self.draw_bottom(canvas, engine, renderer, theme, width, height);

        // The way out, which a page with only a key to leave it by would not
        // have: the same cross the panes down the side of the window use.
        let (left, top, size) = self.cross(width);
        for step in 0..(size as i32) {
            canvas.fill_rect(left as i32 + step, top as i32 + step, 1, 1, theme.text);
            canvas.fill_rect(left as i32 + step, top as i32 + size as i32 - step, 1, 1, theme.text);
        }
    }

    fn draw_tree(
        &self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        theme: &Theme,
        height: f32,
    ) {
        canvas.fill_rect(
            0,
            self.top as i32,
            TREE_WIDTH as i32,
            (height - BOTTOM - self.top) as i32,
            theme.field,
        );
        let heading = engine.simple_line(
            &format!("{} — {}", t("Project"), self.project),
            PADDING,
            self.top + PADDING + TEXT,
            TEXT,
            theme.text,
        );
        renderer.draw_onto(canvas, &heading, 0.0, 0.0);

        for (index, name) in self.modules.iter().enumerate() {
            #[allow(clippy::cast_precision_loss)]
            let y = self.top + PADDING + LINE * (index + 1) as f32;
            if index == self.showing {
                canvas.fill_rect(2, y as i32, TREE_WIDTH as i32 - 4, LINE as i32, theme.accent);
            }
            let colour = if index == self.showing { theme.on_accent() } else { theme.text };
            let line = engine.simple_line(name, PADDING + 12.0, y + TEXT + 2.0, TEXT, colour);
            renderer.draw_onto(canvas, &line, 0.0, 0.0);
        }
    }

    fn draw_code(
        &self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        theme: &Theme,
        width: f32,
        height: f32,
    ) {
        let left = TREE_WIDTH;
        let bottom = height - BOTTOM;
        canvas.fill_rect(
            left as i32,
            self.top as i32,
            (width - left) as i32,
            (bottom - self.top) as i32,
            theme.page,
        );
        canvas.fill_rect(
            left as i32,
            self.top as i32,
            MARGIN as i32,
            (bottom - self.top) as i32,
            theme.field,
        );

        for showing in 0..self.room {
            let Some(text) = self.lines.get(self.scroll + showing) else { break };
            let number = self.scroll + showing + 1;
            #[allow(clippy::cast_precision_loss)]
            let y = self.top + PADDING + LINE * showing as f32;
            if self.stopped == Some(number) {
                canvas.fill_rect(
                    left as i32,
                    y as i32,
                    (width - left) as i32,
                    LINE as i32,
                    theme.hover,
                );
            }
            if self.breakpoints.contains(&number) {
                // A round dot in the margin, which is what Word draws.
                let middle = (left + MARGIN / 2.0, y + LINE / 2.0);
                for row in -4i32..=4 {
                    for column in -4i32..=4 {
                        if row * row + column * column <= 16 {
                            canvas.fill_rect(
                                middle.0 as i32 + column,
                                middle.1 as i32 + row,
                                1,
                                1,
                                theme.danger,
                            );
                        }
                    }
                }
            }
            let line =
                engine.simple_line(text, left + MARGIN + 4.0, y + TEXT + 3.0, TEXT, theme.text);
            renderer.draw_onto(canvas, &line, 0.0, 0.0);
        }

        // The caret, where the letters before it end.
        if !self.in_immediate
            && self.caret.0 >= self.scroll
            && self.caret.0 < self.scroll + self.room
        {
            let text = &self.lines[self.caret.0];
            let before: String = text.chars().take(self.caret.1).collect();
            let measured = engine.simple_line(&before, 0.0, 0.0, TEXT, theme.text).width;
            #[allow(clippy::cast_precision_loss)]
            let y = self.top + PADDING + LINE * (self.caret.0 - self.scroll) as f32;
            canvas.fill_rect(
                (left + MARGIN + 4.0 + measured) as i32,
                y as i32 + 2,
                1,
                LINE as i32 - 4,
                theme.text,
            );
        }
    }

    fn draw_bottom(
        &self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        theme: &Theme,
        width: f32,
        height: f32,
    ) {
        let top = height - BOTTOM;
        canvas.fill_rect(0, top as i32, width as i32, BOTTOM as i32, theme.field);
        canvas.fill_rect(0, top as i32, width as i32, 1, theme.field_edge);

        let half = width / 2.0;
        let heading = engine.simple_line(t("Immediate"), PADDING, top + LINE, TEXT, theme.dim_text);
        renderer.draw_onto(canvas, &heading, 0.0, 0.0);
        let watch =
            engine.simple_line(t("Watch"), half + PADDING, top + LINE, TEXT, theme.dim_text);
        renderer.draw_onto(canvas, &watch, 0.0, 0.0);

        // What has been answered, newest last, and then what is being typed.
        let rows = ((BOTTOM - LINE * 2.0) / LINE) as usize;
        let from = self.answers.len().saturating_sub(rows);
        for (showing, answer) in self.answers[from..].iter().enumerate() {
            #[allow(clippy::cast_precision_loss)]
            let y = top + LINE * (showing + 2) as f32;
            let line = engine.simple_line(answer, PADDING, y, TEXT, theme.text);
            renderer.draw_within(canvas, &line, PADDING, y - LINE, half - PADDING * 2.0, LINE);
        }
        #[allow(clippy::cast_precision_loss)]
        let typing_y = top + LINE * (self.answers[from..].len() + 2) as f32;
        let typed = format!("> {}", self.immediate);
        let line = engine.simple_line(&typed, PADDING, typing_y, TEXT, theme.text);
        let measured = line.width;
        renderer.draw_within(canvas, &line, PADDING, typing_y - LINE, half - PADDING * 2.0, LINE);
        if self.in_immediate {
            canvas.fill_rect(
                (PADDING + measured + 1.0) as i32,
                (typing_y - TEXT) as i32,
                1,
                TEXT as i32 + 3,
                theme.text,
            );
        }

        for (showing, (name, value)) in self.watched.iter().take(rows).enumerate() {
            #[allow(clippy::cast_precision_loss)]
            let y = top + LINE * (showing + 2) as f32;
            let line = engine.simple_line(
                &format!("{name} = {value}"),
                half + PADDING,
                y,
                TEXT,
                theme.text,
            );
            renderer.draw_within(
                canvas,
                &line,
                half + PADDING,
                y - LINE,
                half - PADDING * 2.0,
                LINE,
            );
        }
    }

    /// How far along a line a point is, measured letter by letter.
    #[must_use]
    pub fn column_at(&self, engine: &mut LayoutEngine<'_>, line: usize, x: f32) -> usize {
        let Some(text) = self.lines.get(line) else { return 0 };
        let wanted = x - TREE_WIDTH - MARGIN - 4.0;
        let letters: Vec<char> = text.chars().collect();
        for at in 0..letters.len() {
            let before: String = letters[..=at].iter().collect();
            let measured = engine.simple_line(&before, 0.0, 0.0, TEXT, Color::BLACK).width;
            if measured > wanted {
                return at;
            }
        }
        letters.len()
    }
}
