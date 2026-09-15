//! The editor itself on a real X server.
//!
//! The shell has its own tests against Xvfb; this one puts the whole
//! editor — ribbon, rulers, page, fonts — through the same window, reads
//! the picture back, and closes it the way the frame's button would. Where
//! the machine has no Xvfb the test passes without proving anything, and
//! says so on the way out.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::rc::Rc;
use std::time::{Duration, Instant};

use wp_docx::Document;
use wp_layout::FontLibrary;
use wp_raster::Canvas;
use wp_shell::screen::Shot;
use wp_shell::{accessibility, App, Cursor, Event, Response, WindowCommand, WindowOptions};

use crate::editor::Editor;
use crate::sample;

/// A virtual X server for the length of a test.
struct Server {
    child: Child,
}

impl Server {
    fn start(number: u32, width: u32, height: u32) -> Option<Self> {
        let display = format!(":{number}");
        let socket = format!("/tmp/.X11-unix/X{number}");
        let _ = std::fs::remove_file(format!("/tmp/.X{number}-lock"));
        let _ = std::fs::remove_file(&socket);
        let child = Command::new("Xvfb")
            .args([&display, "-screen", "0", &format!("{width}x{height}x24"), "-nolisten", "tcp"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let mut server = Self { child };
        let started = Instant::now();
        while !Path::new(&socket).exists() {
            if started.elapsed() > Duration::from_secs(10) {
                server.stop();
                return None;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        std::env::set_var("DISPLAY", display);
        Some(server)
    }

    fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop();
    }
}

/// What the test saw while the editor was up.
#[derive(Default)]
struct Seen {
    ticks: u32,
    shot: Option<Shot>,
    closed_on_request: bool,
}

/// The editor with a clock on it: it photographs the screen once the
/// window has been up long enough to be painted, then asks its own window
/// to close — through the window manager's channel, as the close button
/// does — and everything else goes to the editor untouched.
struct Timed {
    editor: Editor,
    seen: Rc<RefCell<Seen>>,
}

const PHOTOGRAPH_AT: u32 = 25;
const GIVE_UP_AT: u32 = 150;

impl App for Timed {
    fn handle(&mut self, event: Event) -> Response {
        match event {
            Event::Tick => {
                let mut seen = self.seen.borrow_mut();
                seen.ticks += 1;
                if seen.ticks == PHOTOGRAPH_AT {
                    seen.shot = wp_shell::screen::capture_screen();
                    wp_shell::window_command(WindowCommand::Close);
                }
                if seen.ticks > GIVE_UP_AT {
                    return Response::Close;
                }
            }
            Event::Closing => {
                let response = self.editor.handle(event);
                self.seen.borrow_mut().closed_on_request = response != Response::Refuse;
                return response;
            }
            _ => {}
        }
        self.editor.handle(event)
    }

    fn cursor(&mut self, x: i32, y: i32) -> Cursor {
        self.editor.cursor(x, y)
    }

    fn is_caption(&mut self, x: i32, y: i32) -> bool {
        self.editor.is_caption(x, y)
    }

    fn switch_window(&mut self, index: usize) {
        self.editor.switch_window(index);
    }

    fn draw(&mut self, width: usize, height: usize) -> &Canvas {
        self.editor.draw(width, height)
    }

    fn accessible_elements(&mut self) -> Vec<accessibility::Element> {
        self.editor.accessible_elements()
    }

    fn accessible_invoke(&mut self, id: u64) -> Response {
        self.editor.accessible_invoke(id)
    }

    fn accessible_text(&mut self) -> Option<accessibility::TextState> {
        self.editor.accessible_text()
    }

    fn accessible_select(&mut self, start: usize, end: usize) -> Response {
        self.editor.accessible_select(start, end)
    }

    fn accessible_rects(&mut self, start: usize, end: usize) -> Vec<(i32, i32, i32, i32)> {
        self.editor.accessible_rects(start, end)
    }
}

/// How much of a picture is one colour.
fn share_of(shot: &Shot, colour: (u8, u8, u8)) -> f32 {
    let matching = shot
        .pixels
        .chunks_exact(4)
        .filter(|pixel| (pixel[0], pixel[1], pixel[2]) == colour)
        .count();
    matching as f32 / (shot.width * shot.height) as f32
}

/// The colours a band of rows holds, most common first.
fn colours_in_rows(shot: &Shot, rows: std::ops::Range<usize>) -> Vec<((u8, u8, u8), usize)> {
    let mut counts = std::collections::HashMap::new();
    for y in rows {
        for x in 0..shot.width {
            let at = (y * shot.width + x) * 4;
            let pixel = &shot.pixels[at..at + 4];
            *counts.entry((pixel[0], pixel[1], pixel[2])).or_insert(0usize) += 1;
        }
    }
    let mut counts: Vec<_> = counts.into_iter().collect();
    counts.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
    counts
}

#[test]
fn the_editor_comes_up_on_a_real_server_and_closes_from_its_frame() {
    let Some(_server) = Server::start(96, 1400, 900) else {
        eprintln!("skipped: Xvfb is not on this machine");
        return;
    };
    let library: &'static FontLibrary = Box::leak(Box::new(FontLibrary::scan_system()));
    let document = Document::create(&sample::welcome_document()).expect("the sample document");
    let editor = Editor::opened(library, document, None::<PathBuf>);
    let seen = Rc::new(RefCell::new(Seen::default()));
    let timed = Timed { editor, seen: Rc::clone(&seen) };
    let options =
        WindowOptions { title: "Document — Word Processor".to_owned(), width: 1400, height: 900 };
    wp_shell::run(options, Box::new(timed))
        .expect("the editor's window opens on the virtual server");

    let seen = Rc::try_unwrap(seen).ok().expect("the shell let go of the editor").into_inner();
    assert!(seen.closed_on_request, "an unchanged document closes when its frame asks");
    assert!(
        (PHOTOGRAPH_AT..=PHOTOGRAPH_AT + 10).contains(&seen.ticks),
        "closed on the request, not the timeout: {} ticks",
        seen.ticks
    );
    let shot = seen.shot.expect("the screen was photographed");
    assert_eq!((shot.width, shot.height), (1400, 900));
    if let Ok(directory) = std::env::var("WP_PROOFS") {
        let mut canvas = Canvas::new(shot.width, shot.height);
        canvas.draw_pixels(&shot.pixels, shot.width, shot.height, 0, 0, shot.width, shot.height);
        let _ = std::fs::create_dir_all(&directory);
        let _ = std::fs::write(
            Path::new(&directory).join("linux-editor.png"),
            wp_raster::encode_png(&canvas),
        );
    }
    // The page of the welcome document sits on the desk around it, in
    // whichever theme the settings say; nothing of the bare server's black
    // shows through.
    let theme = crate::chrome::theme::Theme::of(crate::chrome::theme::Mode::Dark);
    let page = (theme.page.red, theme.page.green, theme.page.blue);
    let light = crate::chrome::theme::Theme::of(crate::chrome::theme::Mode::Light);
    let light_page = (light.page.red, light.page.green, light.page.blue);
    let paper = share_of(&shot, page).max(share_of(&shot, light_page));
    assert!(paper > 0.25, "the page is on the screen: {paper} of it is paper");
    assert!(share_of(&shot, (0, 0, 0)) < 0.02, "the window covers the screen");
    // The caption bar along the top is drawn by the editor, not the server.
    let top = colours_in_rows(&shot, 0..8);
    assert_ne!(top[0].0, (0, 0, 0), "the caption bar is painted: {:?}", &top[..top.len().min(3)]);
}
