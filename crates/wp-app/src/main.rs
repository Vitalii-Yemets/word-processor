//! The windowed application.
//!
//! It opens a `.docx`, lays it out, and shows the pages. Everything on screen is
//! drawn by this project: the archive is unpacked, the XML parsed, the fonts
//! read, the outlines rasterized and the window filled, without a line of
//! third-party code.
//!
//! This is a viewer, not yet an editor. Editing exists in the layers below — the
//! document model can replace text and add paragraphs without disturbing the
//! rest of a file — but connecting a caret and a keyboard to it is the next
//! stage, and pretending otherwise would be worse than saying so.

// A release build has no console, which is what a windowed program should look
// like. A debug build keeps one, since that is where diagnostics go.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
#![forbid(unsafe_code)]

use std::path::Path;

use wp_docx::model::{Alignment, Block, Body, Paragraph, Run};
use wp_docx::Document;
use wp_layout::{FontLibrary, LayoutEngine, Page, Renderer};
use wp_raster::{Canvas, Color};
use wp_shell::{App, Event, Key, Response, WindowOptions};

/// Space between one page and the next, in pixels.
const PAGE_GAP: f32 = 24.0;
/// How far the wheel moves the view per notch.
const SCROLL_PER_NOTCH: f32 = 90.0;
/// How far an arrow key moves it.
const SCROLL_PER_KEY: f32 = 60.0;

/// The colour behind the pages.
const BACKGROUND: Color = Color::rgb(0x3A, 0x3D, 0x41);
/// The page itself.
const PAPER: Color = Color::WHITE;
/// A thin edge, so a white page on a grey ground still has a boundary.
const PAGE_EDGE: Color = Color::rgb(0x20, 0x22, 0x24);

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

    let (document, title) = match path {
        Some(path) => {
            let bytes = std::fs::read(path).map_err(|error| format!("cannot read {path}: {error}"))?;
            let document = Document::open(&bytes)
                .map_err(|error| format!("cannot open {path}: {error}"))?;
            (document, file_name(path))
        }
        None => {
            // Started with no file, so there is something to look at rather than
            // an empty window.
            let document = Document::create(&welcome_document())
                .map_err(|error| format!("cannot build the sample document: {error}"))?;
            (document, "Sample document".to_owned())
        }
    };

    let viewer = Viewer::new(library, &document);
    let options = WindowOptions {
        title: format!("{title} — Word Processor"),
        ..WindowOptions::default()
    };

    wp_shell::run(options, Box::new(viewer)).map_err(|error| error.to_string())
}

fn file_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
        .to_owned()
}

/// Shows the pages of a document, and scrolls.
struct Viewer {
    renderer: Renderer<'static>,
    pages: Vec<Page>,
    /// The window-sized image, kept so a repaint does not allocate one.
    canvas: Canvas,
    scroll: f32,
    view_width: usize,
    view_height: usize,
    /// Whether the canvas still matches what should be on screen.
    needs_redraw: bool,
}

impl Viewer {
    fn new(library: &'static FontLibrary, document: &Document) -> Self {
        let pages = LayoutEngine::new(library).with_dpi(96.0).layout_document(document);

        Self {
            renderer: Renderer::new(library),
            pages,
            canvas: Canvas::new(1, 1),
            scroll: 0.0,
            view_width: 0,
            view_height: 0,
            needs_redraw: true,
        }
    }

    /// The height of every page stacked with gaps between them.
    fn total_height(&self) -> f32 {
        self.pages.iter().map(|page| page.height + PAGE_GAP).sum::<f32>() + PAGE_GAP
    }

    /// Keeps the view inside the document.
    fn clamp_scroll(&mut self) {
        let limit = (self.total_height() - self.view_height as f32).max(0.0);
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
}

impl App for Viewer {
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
            Event::KeyDown(key) => match key {
                Key::Down => self.scroll_by(SCROLL_PER_KEY),
                Key::Up => self.scroll_by(-SCROLL_PER_KEY),
                Key::PageDown => self.scroll_by(self.view_height as f32 * 0.9),
                Key::PageUp => self.scroll_by(-(self.view_height as f32 * 0.9)),
                Key::Home => self.scroll_by(-self.total_height()),
                Key::End => self.scroll_by(self.total_height()),
                Key::Escape => Response::Close,
                _ => Response::Ignored,
            },
            Event::Char(_) | Event::MouseDown { .. } | Event::Closing => Response::Ignored,
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

        let mut y = PAGE_GAP - self.scroll;
        for page in &self.pages {
            let left = ((width as f32 - page.width) / 2.0).max(0.0);

            // Anything scrolled off the screen costs nothing but this test.
            if y + page.height >= 0.0 && y <= height as f32 {
                self.canvas.fill_rect(
                    left as i32 - 1,
                    y as i32 - 1,
                    page.width as i32 + 2,
                    page.height as i32 + 2,
                    PAGE_EDGE,
                );
                self.canvas.fill_rect(
                    left as i32,
                    y as i32,
                    page.width as i32,
                    page.height as i32,
                    PAPER,
                );
                self.renderer.draw_onto(&mut self.canvas, page, left, y);
            }

            y += page.height + PAGE_GAP;
        }

        self.needs_redraw = false;
        &self.canvas
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
        Paragraph::text("What you are looking at").with_style("Heading1"),
    ));
    body.blocks.push(Block::Paragraph(Paragraph::text(
        "The archive was unpacked, the XML parsed, the styles resolved, the fonts read from \
         this machine, the glyph outlines rasterized and the window filled - all by code in \
         this project, with no third-party libraries of any kind.",
    )));
    body.blocks.push(Block::Paragraph(Paragraph::text(
        "Open a document by passing it on the command line, or by dropping it on the program.",
    )));

    body.blocks.push(Block::Paragraph(
        Paragraph::text("Moving around").with_style("Heading1"),
    ));
    for line in [
        "Mouse wheel, or the up and down arrows, to scroll.",
        "Page Up and Page Down to move a screen at a time.",
        "Home and End for the start and the end.",
        "Escape to close.",
    ] {
        body.blocks.push(Block::Paragraph(Paragraph::text(line)));
    }

    body.blocks.push(Block::Paragraph(
        Paragraph::text("What is not here yet").with_style("Heading1"),
    ));
    body.blocks.push(Block::Paragraph(Paragraph::text(
        "This is a viewer. Editing works in the layers underneath - text can be replaced and \
         paragraphs added without disturbing anything else in a file - but there is no caret \
         and no keyboard editing on screen yet.",
    )));
    body.blocks.push(Block::Paragraph(Paragraph::text(
        "Arabic and the Indic scripts draw their isolated letter forms, because the shaping \
         engine that joins them is a later stage. Tables are laid out as their paragraphs, \
         without cells or borders.",
    )));

    body
}
