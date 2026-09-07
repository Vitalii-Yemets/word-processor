//! The windowed application.
//!
//! It opens a `.docx`, lays it out, shows the pages, and lets them be edited.
//! Everything on screen is drawn by this project: the archive is unpacked, the
//! XML parsed, the fonts read, the outlines rasterized and the window filled,
//! without a line of third-party code.
//!
//! Editing goes through the document model, which changes only the nodes it
//! must. A file opened here keeps everything this program does not model — a
//! chart, a content control, a colleague's tracked change — even after it has
//! been typed in and saved.

// A release build has no console, which is what a windowed program should look
// like. A debug build keeps one, since that is where diagnostics go.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

use wp_docx::model::{Alignment, Block, Body, Paragraph, Run};
use wp_docx::{Document, TextPosition};
use wp_layout::{FontLibrary, LayoutEngine, Page, Renderer};
use wp_raster::{Canvas, Color};
use wp_shell::{App, Event, Key, Modifiers, Response, WindowOptions};

/// Space between one page and the next, in pixels.
const PAGE_GAP: f32 = 24.0;
/// How far the wheel moves the view per notch.
const SCROLL_PER_NOTCH: f32 = 90.0;
/// Height of the strip along the bottom that shows what is going on.
const STATUS_HEIGHT: f32 = 26.0;
/// How much of the window to keep clear around the caret when scrolling to it.
const CARET_MARGIN: f32 = 40.0;

const BACKGROUND: Color = Color::rgb(0x3A, 0x3D, 0x41);
const PAPER: Color = Color::WHITE;
const PAGE_EDGE: Color = Color::rgb(0x20, 0x22, 0x24);
const CARET: Color = Color::rgb(0x10, 0x50, 0xC0);
const STATUS_BACKGROUND: Color = Color::rgb(0x24, 0x26, 0x29);

fn main() -> std::process::ExitCode {
    let argument = std::env::args().nth(1);

    match start(argument.as_deref()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn start(path: Option<&str>) -> Result<(), String> {
    if !wp_shell::is_supported() {
        return Err(
            "this build has no window support; only Windows is implemented so far".to_owned()
        );
    }

    // The library is needed for as long as the window is open, and both the
    // layout engine and the renderer borrow from it. Giving it the lifetime of
    // the process is simpler and more honest than threading a lifetime through
    // every type, and nothing is freed early because nothing is freed at all.
    let library: &'static FontLibrary = Box::leak(Box::new(FontLibrary::scan_system()));
    if library.is_empty() {
        return Err("no usable fonts were found on this machine".to_owned());
    }

    let (document, file, title) = match path {
        Some(path) => {
            let bytes =
                std::fs::read(path).map_err(|error| format!("cannot read {path}: {error}"))?;
            let document =
                Document::open(&bytes).map_err(|error| format!("cannot open {path}: {error}"))?;
            (document, Some(PathBuf::from(path)), file_name(path))
        }
        None => {
            // Started with no file, so there is something to look at rather than
            // an empty window.
            let document = Document::create(&welcome_document())
                .map_err(|error| format!("cannot build the sample document: {error}"))?;
            (document, None, "Sample document".to_owned())
        }
    };

    let editor = Editor::new(library, document, file);
    let options = WindowOptions {
        title: format!("{title} — Word Processor"),
        ..WindowOptions::default()
    };

    wp_shell::run(options, Box::new(editor)).map_err(|error| error.to_string())
}

fn file_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
        .to_owned()
}

/// Shows a document, and edits it.
struct Editor {
    document: Document,
    engine: LayoutEngine<'static>,
    renderer: Renderer<'static>,
    pages: Vec<Page>,
    /// The window-sized image, kept so a repaint does not allocate one.
    canvas: Canvas,
    scroll: f32,
    view_width: usize,
    view_height: usize,
    caret: TextPosition,
    file: Option<PathBuf>,
    /// What the strip along the bottom says.
    status: String,
    needs_redraw: bool,
}

impl Editor {
    fn new(
        library: &'static FontLibrary,
        document: Document,
        file: Option<PathBuf>,
    ) -> Self {
        let mut engine = LayoutEngine::new(library).with_dpi(96.0);
        let pages = engine.layout_document(&document);

        Self {
            document,
            engine,
            renderer: Renderer::new(library),
            pages,
            canvas: Canvas::new(1, 1),
            scroll: 0.0,
            view_width: 0,
            view_height: 0,
            caret: TextPosition::new(0, 0),
            file,
            status: String::from("Ready"),
            needs_redraw: true,
        }
    }

    // --- Geometry ----------------------------------------------------------

    /// Where a page's top-left corner sits, before scrolling is applied.
    fn page_origin(&self, index: usize) -> (f32, f32) {
        let mut y = PAGE_GAP;
        for page in self.pages.iter().take(index) {
            y += page.height + PAGE_GAP;
        }
        let width = self.pages.get(index).map_or(0.0, |page| page.width);
        let x = ((self.view_width as f32 - width) / 2.0).max(0.0);
        (x, y)
    }

    fn total_height(&self) -> f32 {
        self.pages.iter().map(|page| page.height + PAGE_GAP).sum::<f32>() + PAGE_GAP
    }

    /// The height available for pages, above the status strip.
    fn viewport_height(&self) -> f32 {
        (self.view_height as f32 - STATUS_HEIGHT).max(1.0)
    }

    fn clamp_scroll(&mut self) {
        let limit = (self.total_height() - self.viewport_height()).max(0.0);
        self.scroll = self.scroll.clamp(0.0, limit);
    }

    fn scroll_by(&mut self, amount: f32) -> Response {
        let before = self.scroll;
        self.scroll += amount;
        self.clamp_scroll();
        if (self.scroll - before).abs() < 0.5 {
            Response::Ignored
        } else {
            self.needs_redraw = true;
            Response::Redraw
        }
    }

    // --- The caret ---------------------------------------------------------

    /// Every line in the document, as page and line indices in reading order.
    fn lines(&self) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        for (page_index, page) in self.pages.iter().enumerate() {
            for line_index in 0..page.lines.len() {
                out.push((page_index, line_index));
            }
        }
        out
    }

    /// Which line the caret is on.
    fn caret_line(&self) -> Option<(usize, usize)> {
        self.lines().into_iter().find(|(page, line)| {
            let line = &self.pages[*page].lines[*line];
            line.paragraph == self.caret.paragraph
                && self.caret.offset >= line.start_offset
                && self.caret.offset <= line.end_offset
        })
    }

    /// Where the caret should be drawn, in window coordinates.
    fn caret_rect(&self) -> Option<(f32, f32, f32)> {
        let (page_index, _) = self.caret_line()?;
        let page = self.pages.get(page_index)?;
        let (origin_x, origin_y) = self.page_origin(page_index);
        let (x, y, height) = page.caret_at(self.caret)?;
        Some((origin_x + x, origin_y + y - self.scroll, height))
    }

    /// Scrolls so the caret is on screen, if it is not already.
    fn reveal_caret(&mut self) {
        let Some((_, y, height)) = self.caret_rect() else { return };
        let viewport = self.viewport_height();

        if y < CARET_MARGIN {
            self.scroll -= CARET_MARGIN - y;
        } else if y + height > viewport - CARET_MARGIN {
            self.scroll += y + height - (viewport - CARET_MARGIN);
        }
        self.clamp_scroll();
    }

    fn paragraph_text(&self, index: usize) -> String {
        self.document.paragraph_text(index).unwrap_or_default()
    }

    /// The byte offset of the character boundary before an offset.
    fn previous_boundary(text: &str, offset: usize) -> usize {
        text[..offset.min(text.len())]
            .char_indices()
            .next_back()
            .map_or(0, |(index, _)| index)
    }

    /// The byte offset of the character boundary after an offset.
    fn next_boundary(text: &str, offset: usize) -> usize {
        match text.get(offset..).and_then(|rest| rest.chars().next()) {
            Some(character) => offset + character.len_utf8(),
            None => text.len(),
        }
    }

    fn move_left(&mut self) {
        let text = self.paragraph_text(self.caret.paragraph);
        if self.caret.offset > 0 {
            self.caret.offset = Self::previous_boundary(&text, self.caret.offset);
        } else if self.caret.paragraph > 0 {
            // Off the front of a paragraph is the end of the one before it.
            self.caret.paragraph -= 1;
            self.caret.offset = self.paragraph_text(self.caret.paragraph).len();
        }
    }

    fn move_right(&mut self) {
        let text = self.paragraph_text(self.caret.paragraph);
        if self.caret.offset < text.len() {
            self.caret.offset = Self::next_boundary(&text, self.caret.offset);
        } else if self.caret.paragraph + 1 < self.document.paragraph_count() {
            self.caret.paragraph += 1;
            self.caret.offset = 0;
        }
    }

    /// Moves the caret a line up or down, keeping roughly the same column.
    fn move_vertically(&mut self, downwards: bool) {
        let lines = self.lines();
        let Some(current) = self.caret_line() else { return };
        let Some(index) = lines.iter().position(|entry| *entry == current) else {
            return;
        };

        let target = if downwards { index + 1 } else { index.wrapping_sub(1) };
        let Some(&(page_index, line_index)) = lines.get(target) else {
            return;
        };

        // The column is kept by remembering where the caret is on screen and
        // asking the target line what sits at the same place.
        let wanted_x = self
            .pages
            .get(current.0)
            .and_then(|page| page.caret_at(self.caret))
            .map_or(0.0, |(x, _, _)| x);

        let page = &self.pages[page_index];
        let line = &page.lines[line_index];
        if let Some(position) = page.position_at(wanted_x, line.baseline) {
            self.caret = position;
        }
    }

    fn move_to_line_edge(&mut self, end: bool) {
        let Some((page_index, line_index)) = self.caret_line() else { return };
        let line = &self.pages[page_index].lines[line_index];
        self.caret = TextPosition::new(
            line.paragraph,
            if end { line.end_offset } else { line.start_offset },
        );
    }

    // --- Editing -----------------------------------------------------------

    /// Lays the document out again after a change.
    fn relayout(&mut self) {
        self.pages = self.engine.layout_document(&self.document);

        // An edit can shorten the document under the caret.
        let count = self.document.paragraph_count();
        if count == 0 {
            self.caret = TextPosition::default();
        } else {
            self.caret.paragraph = self.caret.paragraph.min(count - 1);
            let length = self.paragraph_text(self.caret.paragraph).len();
            self.caret.offset = self.caret.offset.min(length);
        }

        self.clamp_scroll();
        self.needs_redraw = true;
    }

    fn insert(&mut self, text: &str) -> Response {
        if !self.document.insert_text(self.caret, text) {
            return Response::Ignored;
        }
        self.caret.offset += text.len();
        self.relayout();
        self.reveal_caret();
        self.status = String::from("Edited");
        Response::Redraw
    }

    fn backspace(&mut self) -> Response {
        if self.caret.offset > 0 {
            let text = self.paragraph_text(self.caret.paragraph);
            let start = Self::previous_boundary(&text, self.caret.offset);
            if self.document.delete_range(self.caret.paragraph, start, self.caret.offset) {
                self.caret.offset = start;
            }
        } else if self.caret.paragraph > 0 {
            // At the very start, Backspace joins this paragraph onto the last.
            let previous = self.caret.paragraph - 1;
            let join_at = self.paragraph_text(previous).len();
            if !self.document.merge_with_previous(self.caret.paragraph) {
                return Response::Ignored;
            }
            self.caret = TextPosition::new(previous, join_at);
        } else {
            return Response::Ignored;
        }

        self.relayout();
        self.reveal_caret();
        self.status = String::from("Edited");
        Response::Redraw
    }

    fn delete_forward(&mut self) -> Response {
        let text = self.paragraph_text(self.caret.paragraph);
        if self.caret.offset < text.len() {
            let end = Self::next_boundary(&text, self.caret.offset);
            if !self.document.delete_range(self.caret.paragraph, self.caret.offset, end) {
                return Response::Ignored;
            }
        } else if self.caret.paragraph + 1 < self.document.paragraph_count() {
            if !self.document.merge_with_previous(self.caret.paragraph + 1) {
                return Response::Ignored;
            }
        } else {
            return Response::Ignored;
        }

        self.relayout();
        self.reveal_caret();
        self.status = String::from("Edited");
        Response::Redraw
    }

    fn split(&mut self) -> Response {
        if !self.document.split_paragraph(self.caret) {
            return Response::Ignored;
        }
        self.caret = TextPosition::new(self.caret.paragraph + 1, 0);
        self.relayout();
        self.reveal_caret();
        self.status = String::from("Edited");
        Response::Redraw
    }

    fn save(&mut self) -> Response {
        let path = match &self.file {
            Some(path) => path.clone(),
            // A document that came from nowhere still has to go somewhere.
            None => PathBuf::from("untitled.docx"),
        };

        self.status = match self.document.save() {
            Ok(bytes) => match std::fs::write(&path, &bytes) {
                Ok(()) => {
                    // Only once the bytes are really on disk does the document
                    // count as saved.
                    let _ = self.document.mark_saved();
                    self.file = Some(path.clone());
                    format!("Saved {} ({} bytes)", path.display(), bytes.len())
                }
                Err(error) => format!("Cannot write {}: {error}", path.display()),
            },
            Err(error) => format!("Cannot save: {error}"),
        };

        self.needs_redraw = true;
        Response::Redraw
    }

    // --- Drawing -----------------------------------------------------------

    fn draw_status(&mut self) {
        let top = (self.view_height as f32 - STATUS_HEIGHT) as i32;
        self.canvas.fill_rect(
            0,
            top,
            self.view_width as i32,
            STATUS_HEIGHT.ceil() as i32,
            STATUS_BACKGROUND,
        );

        let text = format!(
            "{}   paragraph {} of {}, offset {}{}",
            self.status,
            self.caret.paragraph + 1,
            self.document.paragraph_count(),
            self.caret.offset,
            if self.document.is_modified() { "   •  unsaved changes" } else { "" }
        );

        // The status strip is drawn with the same text engine as the document,
        // so there is only one way to put text on screen in this program.
        let baseline = self.view_height as f32 - STATUS_HEIGHT + 18.0;
        let line = self.engine.simple_line(&text, 10.0, baseline, 9.0, Color::rgb(0xC8, 0xCC, 0xD0));
        self.renderer.draw_onto(&mut self.canvas, &line, 0.0, 0.0);
    }
}

impl App for Editor {
    fn handle(&mut self, event: Event) -> Response {
        match event {
            Event::Resized { width, height } => {
                self.view_width = width as usize;
                self.view_height = height as usize;
                self.clamp_scroll();
                self.needs_redraw = true;
                Response::Redraw
            }

            Event::Scroll { lines } => self.scroll_by(-lines * SCROLL_PER_NOTCH),

            Event::MouseDown { x, y, .. } => {
                // Which page was clicked, and where on it.
                for index in 0..self.pages.len() {
                    let (origin_x, origin_y) = self.page_origin(index);
                    let page_x = x as f32 - origin_x;
                    let page_y = y as f32 - origin_y + self.scroll;
                    let page = &self.pages[index];

                    if page_y >= 0.0 && page_y <= page.height {
                        if let Some(position) = page.position_at(page_x, page_y) {
                            self.caret = position;
                            self.status = String::from("Ready");
                            self.needs_redraw = true;
                            return Response::Redraw;
                        }
                    }
                }
                Response::Ignored
            }

            Event::Char(character) => self.insert(&character.to_string()),

            Event::KeyDown { key, modifiers } => self.key(key, modifiers),

            Event::Closing => Response::Ignored,
        }
    }

    fn draw(&mut self, width: usize, height: usize) -> &Canvas {
        if self.canvas.width() != width || self.canvas.height() != height {
            self.canvas = Canvas::new(width, height);
            self.view_width = width;
            self.view_height = height;
            self.needs_redraw = true;
        }
        if !self.needs_redraw {
            return &self.canvas;
        }

        self.canvas.clear(BACKGROUND);

        for index in 0..self.pages.len() {
            let (origin_x, origin_y) = self.page_origin(index);
            let y = origin_y - self.scroll;
            let page_height = self.pages[index].height;

            // Anything scrolled off the screen costs nothing but this test.
            if y + page_height < 0.0 || y > height as f32 {
                continue;
            }

            let page_width = self.pages[index].width;
            self.canvas.fill_rect(
                origin_x as i32 - 1,
                y as i32 - 1,
                page_width as i32 + 2,
                page_height as i32 + 2,
                PAGE_EDGE,
            );
            self.canvas.fill_rect(
                origin_x as i32,
                y as i32,
                page_width as i32,
                page_height as i32,
                PAPER,
            );

            let page = &self.pages[index];
            self.renderer.draw_onto(&mut self.canvas, page, origin_x, y);
        }

        if let Some((x, y, caret_height)) = self.caret_rect() {
            self.canvas.fill_rect(x as i32, y as i32, 2, caret_height.ceil() as i32, CARET);
        }

        self.draw_status();

        self.needs_redraw = false;
        &self.canvas
    }
}

impl Editor {
    /// Reacts to a key that is not ordinary typing.
    fn key(&mut self, key: Key, modifiers: Modifiers) -> Response {
        if modifiers.control {
            return match key {
                Key::Letter('s') => self.save(),
                _ => Response::Ignored,
            };
        }

        match key {
            Key::Left => {
                self.move_left();
                self.reveal_caret();
                self.needs_redraw = true;
                Response::Redraw
            }
            Key::Right => {
                self.move_right();
                self.reveal_caret();
                self.needs_redraw = true;
                Response::Redraw
            }
            Key::Up | Key::Down => {
                self.move_vertically(key == Key::Down);
                self.reveal_caret();
                self.needs_redraw = true;
                Response::Redraw
            }
            Key::Home | Key::End => {
                self.move_to_line_edge(key == Key::End);
                self.reveal_caret();
                self.needs_redraw = true;
                Response::Redraw
            }
            Key::PageDown => self.scroll_by(self.viewport_height() * 0.9),
            Key::PageUp => self.scroll_by(-(self.viewport_height() * 0.9)),
            Key::Enter => self.split(),
            Key::Backspace => self.backspace(),
            Key::Delete => self.delete_forward(),
            Key::Tab => self.insert("\t"),
            Key::Escape => Response::Close,
            Key::Letter(_) => Response::Ignored,
        }
    }
}

/// The document shown when the program is started without a file.
fn welcome_document() -> Body {
    let mut body = Body::default();

    body.blocks.push(Block::Paragraph(
        Paragraph::text("Word Processor").with_style("Title").with_alignment(Alignment::Center),
    ));
    body.blocks.push(Block::Paragraph(
        Paragraph::from_runs(vec![Run::text(
            "Every pixel on this page was drawn by this program",
        )
        .italic()
        .colored("595959")])
        .with_alignment(Alignment::Center),
    ));

    body.blocks.push(Block::Paragraph(
        Paragraph::text("Try typing").with_style("Heading1"),
    ));
    body.blocks.push(Block::Paragraph(Paragraph::text(
        "Click anywhere in the text to put the caret there, then type. Enter splits a \
         paragraph, Backspace joins one onto the last, and Ctrl+S saves.",
    )));
    body.blocks.push(Block::Paragraph(Paragraph::text(
        "An edit goes through the document model, which changes only the nodes it must. A file \
         opened here keeps everything this program does not model - a chart, a content control, \
         somebody else's tracked change - even after it has been typed in and saved.",
    )));

    body.blocks.push(Block::Paragraph(
        Paragraph::text("What is drawn here").with_style("Heading1"),
    ));
    body.blocks.push(Block::Paragraph(Paragraph::text(
        "The archive was unpacked, the XML parsed, the styles resolved, the fonts read from this \
         machine, the glyph outlines rasterized and the window filled - all by code in this \
         project, with no third-party libraries of any kind.",
    )));

    body.blocks.push(Block::Paragraph(
        Paragraph::text("What is not here yet").with_style("Heading1"),
    ));
    body.blocks.push(Block::Paragraph(Paragraph::text(
        "There is no selection, no undo, and no formatting from the keyboard. Arabic and the \
         Indic scripts draw their isolated letter forms, because the shaping engine that joins \
         them is a later stage. Tables are laid out as their paragraphs, without cells or \
         borders.",
    )));

    body
}
