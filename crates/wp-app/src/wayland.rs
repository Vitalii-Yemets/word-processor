//! The editor itself on a Wayland compositor.
//!
//! The shell has its own tests against sway; this one puts the whole
//! editor — ribbon, rulers, page, fonts — on the same compositor, types
//! into the document, photographs the screen through another client, and
//! closes the window. Where the machine has no sway the test passes
//! without proving anything, and says so on the way out.

use std::cell::RefCell;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::rc::Rc;
use std::time::{Duration, Instant};

use wp_docx::Document;
use wp_layout::FontLibrary;
use wp_raster::Canvas;
use wp_shell::{accessibility, App, Cursor, Event, Response, WindowCommand, WindowOptions};

use crate::editor::Editor;

/// Who the compositor runs as: sway will not run as root, and everything
/// in the build container is root.
const COMPOSITOR_USER: &str = "compositor";

/// A compositor with no screen, for the length of a test.
struct Compositor {
    child: Child,
    runtime: PathBuf,
    display: String,
}

impl Compositor {
    fn start(width: u32, height: u32) -> Option<Self> {
        let runtime = std::env::temp_dir().join(format!("wp-wayland-app-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&runtime);
        std::fs::create_dir_all(&runtime).ok()?;
        let config = runtime.join("config");
        let mut file = std::fs::File::create(&config).ok()?;
        writeln!(
            file,
            "default_border none\ndefault_floating_border none\ngaps inner 0\noutput HEADLESS-1 resolution {width}x{height}\n"
        )
        .ok()?;
        drop(file);
        Command::new("chown").args(["-R", COMPOSITOR_USER]).arg(&runtime).status().ok()?;

        let started_as = format!(
            "XDG_RUNTIME_DIR={} WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 XDG_SESSION_TYPE=wayland exec sway -c {}",
            runtime.display(),
            config.display()
        );
        let child = Command::new("su")
            .args([COMPOSITOR_USER, "-s", "/bin/sh", "-c", &started_as])
            .env_remove("DISPLAY")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let mut compositor = Self { child, runtime: runtime.clone(), display: String::new() };
        let started = Instant::now();
        loop {
            if let Some(found) = socket_in(&runtime) {
                compositor.display = found;
                break;
            }
            if started.elapsed() > Duration::from_secs(20) {
                compositor.stop();
                return None;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        std::thread::sleep(Duration::from_millis(500));
        std::env::set_var("XDG_RUNTIME_DIR", &runtime);
        std::env::set_var("WAYLAND_DISPLAY", &compositor.display);
        Some(compositor)
    }

    fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.runtime);
        // The environment is the whole program's: a compositor that has
        // stopped must not be what the next test in this binary talks to.
        std::env::remove_var("WAYLAND_DISPLAY");
    }
}

impl Drop for Compositor {
    fn drop(&mut self) {
        self.stop();
    }
}

/// The name of the compositor's socket, which is `wayland-` and a number.
fn socket_in(runtime: &Path) -> Option<String> {
    let entries = std::fs::read_dir(runtime).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("wayland-") && !name.ends_with(".lock") {
            return Some(name);
        }
    }
    None
}

/// What the test saw while the editor was up.
#[derive(Default)]
struct Seen {
    ticks: u32,
    typed: String,
    shot: Option<(usize, usize, Vec<u8>)>,
    /// What the document held when the picture was taken.
    document_text: String,
    asked_to_close: u32,
    closed_on_request: bool,
}

/// The editor with a clock on it: it types into the document, photographs
/// the screen, and asks its own window to close.
struct Timed {
    editor: Editor,
    seen: Rc<RefCell<Seen>>,
    runtime: PathBuf,
    display: String,
}

const TYPE_AT: u32 = 10;
/// Long enough after the typing for every letter to have arrived: a key
/// every hundred and twenty milliseconds, and eighteen of them.
const PHOTOGRAPH_AT: u32 = 80;
const CLOSE_AT: u32 = 84;
/// The second request, after the document has been saved. The first is
/// refused: there is typing in it that is not on disk, and the question
/// about that cannot be asked where there is no dialog program — so the
/// window stays, which is the safe answer and the right one.
const CLOSE_AGAIN_AT: u32 = 90;
const GIVE_UP_AT: u32 = 150;

impl App for Timed {
    fn handle(&mut self, event: Event) -> Response {
        match event {
            Event::Tick => {
                let ticks = {
                    let mut seen = self.seen.borrow_mut();
                    seen.ticks += 1;
                    seen.ticks
                };
                if ticks == TYPE_AT {
                    // Two sacrificial spaces: a compositor with no devices
                    // has no keyboard until this typing makes one, and the
                    // first key is pressed in the same instant.
                    let _ = Command::new("wtype")
                        .args(["-s", "120"])
                        .arg("  Hello from Wayland")
                        .env("XDG_RUNTIME_DIR", &self.runtime)
                        .env("WAYLAND_DISPLAY", &self.display)
                        .env_remove("DISPLAY")
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn();
                }
                if ticks == PHOTOGRAPH_AT {
                    let shot = photograph(&self.runtime, &self.display);
                    let text = self.editor.document.paragraph_text(0).unwrap_or_default();
                    let mut seen = self.seen.borrow_mut();
                    seen.shot = shot;
                    seen.document_text = text;
                }
                if ticks == CLOSE_AT {
                    wp_shell::window_command(WindowCommand::Close);
                }
                if ticks == CLOSE_AGAIN_AT {
                    let _ = self.editor.document.mark_saved();
                    wp_shell::window_command(WindowCommand::Close);
                }
                if ticks > GIVE_UP_AT {
                    return Response::Close;
                }
            }
            Event::Closing => {
                let response = self.editor.handle(event);
                let mut seen = self.seen.borrow_mut();
                seen.asked_to_close += 1;
                seen.closed_on_request = response != Response::Refuse;
                return response;
            }
            Event::Char(character) => {
                self.seen.borrow_mut().typed.push(character);
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

/// What `grim` photographed, as red, green and blue.
fn photograph(runtime: &Path, display: &str) -> Option<(usize, usize, Vec<u8>)> {
    let output = Command::new("grim")
        .args(["-t", "ppm", "-"])
        .env("XDG_RUNTIME_DIR", runtime)
        .env("WAYLAND_DISPLAY", display)
        .env_remove("DISPLAY")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    read_ppm(&output.stdout)
}

fn read_ppm(bytes: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
    let mut fields = Vec::new();
    let mut at = 0usize;
    while fields.len() < 4 && at < bytes.len() {
        while at < bytes.len() && bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        let start = at;
        while at < bytes.len() && !bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        fields.push(String::from_utf8_lossy(&bytes[start..at]).into_owned());
    }
    if fields.first().map(String::as_str) != Some("P6") {
        return None;
    }
    let width: usize = fields.get(1)?.parse().ok()?;
    let height: usize = fields.get(2)?.parse().ok()?;
    at += 1;
    Some((width, height, bytes.get(at..at + width * height * 3)?.to_vec()))
}

/// How much of a picture is one colour.
fn share_of(rgb: &[u8], colour: (u8, u8, u8)) -> f32 {
    let matching =
        rgb.chunks_exact(3).filter(|pixel| (pixel[0], pixel[1], pixel[2]) == colour).count();
    matching as f32 / (rgb.len() / 3) as f32
}

#[test]
fn the_editor_comes_up_on_a_compositor_takes_typing_and_closes() {
    let _display = crate::xserver::one_display_at_a_time();
    let Some(compositor) = Compositor::start(1400, 900) else {
        eprintln!("skipped: no compositor could be started on this machine");
        return;
    };
    let library: &'static FontLibrary = Box::leak(Box::new(FontLibrary::scan_system()));
    // One empty paragraph, as a new document has: somewhere for the caret
    // to be, and so somewhere for the typing to go.
    let mut body = wp_docx::model::Body::default();
    body.blocks.push(wp_docx::model::Block::Paragraph(wp_docx::model::Paragraph::default()));
    let document = Document::create(&body).expect("a document to type into");
    let editor = Editor::opened(library, document, None::<PathBuf>);
    let seen = Rc::new(RefCell::new(Seen::default()));
    let timed = Timed {
        editor,
        seen: Rc::clone(&seen),
        runtime: compositor.runtime.clone(),
        display: compositor.display.clone(),
    };
    let options =
        WindowOptions { title: "Document — Word Processor".to_owned(), width: 1400, height: 900 };
    wp_shell::run(options, Box::new(timed)).expect("the editor's window opens on the compositor");

    let seen = Rc::try_unwrap(seen).ok().expect("the shell let go of the editor").into_inner();
    assert_eq!(seen.asked_to_close, 2, "asked twice: refused while unsaved, then agreed");
    assert!(seen.closed_on_request, "a saved document closes when it is asked to");
    assert!(
        (CLOSE_AGAIN_AT..CLOSE_AGAIN_AT + 10).contains(&seen.ticks),
        "closed on the second request, not the timeout: {} ticks",
        seen.ticks
    );

    assert_eq!(seen.typed.trim_start(), "Hello from Wayland", "what was typed reached the editor");
    let (width, height, rgb) = seen.shot.expect("the screen was photographed");
    assert!(
        seen.document_text.contains("Hello from Wayland"),
        "and went into the document: {:?}",
        seen.document_text
    );
    assert_eq!((width, height), (1400, 900));
    if let Ok(directory) = std::env::var("WP_PROOFS") {
        let mut canvas = Canvas::new(width, height);
        let rgba: Vec<u8> =
            rgb.chunks_exact(3).flat_map(|pixel| [pixel[0], pixel[1], pixel[2], 255]).collect();
        canvas.draw_pixels(&rgba, width, height, 0, 0, width, height);
        let _ = std::fs::create_dir_all(&directory);
        let _ = std::fs::write(
            Path::new(&directory).join("wayland-editor.png"),
            wp_raster::encode_png(&canvas),
        );
    }
    // The page of the document, on the desk the theme paints, and nothing
    // of the compositor's own background showing through.
    let theme = crate::chrome::theme::Theme::of(crate::chrome::theme::Mode::Dark);
    let page = (theme.page.red, theme.page.green, theme.page.blue);
    let light = crate::chrome::theme::Theme::of(crate::chrome::theme::Mode::Light);
    let light_page = (light.page.red, light.page.green, light.page.blue);
    let paper = share_of(&rgb, page).max(share_of(&rgb, light_page));
    assert!(paper > 0.2, "the page is on the screen: {paper} of it is paper");
}
