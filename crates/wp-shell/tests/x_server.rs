//! The X11 shell against a real X server.
//!
//! Xvfb is an X server with no screen: it draws into a file, which is all a
//! test needs to open a window, paint it, read the picture back off the
//! server's own frame buffer, and close it. Where the machine has no Xvfb
//! the tests pass without proving anything, and say so on the way out.
#![cfg(target_os = "linux")]

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::rc::Rc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use wp_raster::{Canvas, Color};
use wp_shell::{App, Event, Response, WindowCommand, WindowOptions};

/// Only one test talks to a display at a time: the display is named by an
/// environment variable, which is shared by the whole process.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

/// A virtual X server for the length of a test.
struct Server {
    child: Child,
    /// Where the server keeps its frame buffer, as an `xwd` file.
    frame_buffer: PathBuf,
}

impl Server {
    fn start(number: u32, width: u32, height: u32) -> Option<Self> {
        let display = format!(":{number}");
        let socket = format!("/tmp/.X11-unix/X{number}");
        let _ = std::fs::remove_file(format!("/tmp/.X{number}-lock"));
        let _ = std::fs::remove_file(&socket);
        let directory = std::env::temp_dir().join(format!("wp-xvfb-{number}"));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).ok()?;
        let child = Command::new("Xvfb")
            .args([
                &display,
                "-screen",
                "0",
                &format!("{width}x{height}x24"),
                "-nolisten",
                "tcp",
                "-fbdir",
            ])
            .arg(&directory)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let mut server = Self { child, frame_buffer: directory.join("Xvfb_screen0") };
        let started = Instant::now();
        while !Path::new(&socket).exists() {
            if started.elapsed() > Duration::from_secs(10) {
                server.stop();
                return None;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        // An X server is not a Wayland compositor: whatever a test before
        // this one left in the environment is not what this one uses.
        std::env::remove_var("WAYLAND_DISPLAY");
        std::env::set_var("DISPLAY", display);
        Some(server)
    }

    fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(directory) = self.frame_buffer.parent() {
            let _ = std::fs::remove_dir_all(directory);
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop();
    }
}

/// The screen as the server holds it, read off its own frame buffer file
/// — the server's side of the story, not this program's: red, green, blue
/// per pixel.
fn photograph(frame_buffer: &Path) -> Option<(usize, usize, Vec<u8>)> {
    let bytes = std::fs::read(frame_buffer).ok()?;
    if bytes.len() < 100 {
        return None;
    }
    let word =
        |at: usize| u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    let header_size = word(0) as usize;
    let width = word(16) as usize;
    let height = word(20) as usize;
    let little_endian = word(28) == 0;
    let bits_per_pixel = word(44) as usize;
    let bytes_per_line = word(48) as usize;
    let (red_mask, green_mask, blue_mask) = (word(56), word(60), word(64));
    let colours = word(76) as usize;
    let pixels_at = header_size + colours * 12;
    let take = |mask: u32, value: u32| -> u8 {
        if mask == 0 {
            return 0;
        }
        let shift = mask.trailing_zeros();
        let width = (mask >> shift).count_ones();
        let raw = (value & mask) >> shift;
        (raw << (8 - width.min(8))) as u8
    };
    let mut out = Vec::with_capacity(width * height * 3);
    for y in 0..height {
        for x in 0..width {
            let at = pixels_at + y * bytes_per_line + x * (bits_per_pixel / 8);
            let value = match (bits_per_pixel, little_endian) {
                (32, true) => {
                    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
                }
                (32, false) => {
                    u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
                }
                (24, true) => u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], 0]),
                (24, false) => u32::from_be_bytes([0, bytes[at], bytes[at + 1], bytes[at + 2]]),
                (16, true) => u32::from(u16::from_le_bytes([bytes[at], bytes[at + 1]])),
                (16, false) => u32::from(u16::from_be_bytes([bytes[at], bytes[at + 1]])),
                _ => 0,
            };
            out.push(take(red_mask, value));
            out.push(take(green_mask, value));
            out.push(take(blue_mask, value));
        }
    }
    Some((width, height, out))
}

/// Writes a photograph where the proofs go, when somewhere was named.
fn keep_proof(name: &str, width: usize, height: usize, rgb: &[u8]) {
    let Ok(directory) = std::env::var("WP_PROOFS") else { return };
    let mut canvas = Canvas::new(width, height);
    let rgba: Vec<u8> =
        rgb.chunks_exact(3).flat_map(|pixel| [pixel[0], pixel[1], pixel[2], 255]).collect();
    canvas.draw_pixels(&rgba, width, height, 0, 0, width, height);
    let _ = std::fs::create_dir_all(&directory);
    let _ = std::fs::write(Path::new(&directory).join(name), wp_raster::encode_png(&canvas));
}

/// The colour most of a photograph is.
fn dominant(rgb: &[u8]) -> (u8, u8, u8) {
    let mut counts = std::collections::HashMap::new();
    for pixel in rgb.chunks_exact(3) {
        *counts.entry((pixel[0], pixel[1], pixel[2])).or_insert(0usize) += 1;
    }
    counts.into_iter().max_by_key(|(_, count)| *count).map_or((0, 0, 0), |(colour, _)| colour)
}

/// What a program saw while its window was up, kept where the test can read
/// it after the shell has dropped the program.
#[derive(Default)]
struct Seen {
    ticks: u32,
    sizes: Vec<(u32, u32)>,
    scale: f32,
    photographed: Option<(usize, usize, Vec<u8>)>,
    own_shot: Option<wp_shell::screen::Shot>,
    windows_seen: Vec<String>,
    pasted: Option<String>,
    asked_to_close: u32,
}

/// A program that paints its window one colour with a white square, takes
/// a photograph part way, and closes itself after enough ticks.
struct Painter {
    canvas: Canvas,
    seen: Rc<RefCell<Seen>>,
    frame_buffer: PathBuf,
}

const CLOSE_AFTER: u32 = 30;

impl App for Painter {
    fn handle(&mut self, event: Event) -> Response {
        let mut seen = self.seen.borrow_mut();
        match event {
            Event::Resized { width, height } => {
                seen.sizes.push((width, height));
                Response::Redraw
            }
            Event::ScaleChanged { scale } => {
                seen.scale = scale;
                Response::Ignored
            }
            Event::Tick => {
                seen.ticks += 1;
                if seen.ticks == 10 {
                    // By now the window is mapped and painted: photograph it
                    // from outside, and through the shell's own screenshot.
                    seen.photographed = photograph(&self.frame_buffer);
                    seen.own_shot = wp_shell::screen::capture_screen();
                    seen.windows_seen = wp_shell::screen::windows()
                        .into_iter()
                        .map(|window| window.title)
                        .collect();
                    // And carry text through the clipboard while the window
                    // is up to answer for it.
                    assert!(wp_shell::clipboard::set_text("carried by the X selection"));
                    seen.pasted = wp_shell::clipboard::text();
                }
                if seen.ticks == CLOSE_AFTER {
                    return Response::Close;
                }
                Response::Ignored
            }
            Event::Closing => {
                seen.asked_to_close += 1;
                Response::Close
            }
            _ => Response::Ignored,
        }
    }

    fn draw(&mut self, width: usize, height: usize) -> &Canvas {
        self.canvas = Canvas::new(width, height);
        self.canvas.clear(Color::rgb(200, 40, 40));
        self.canvas.fill_rect(40, 40, 120, 80, Color::WHITE);
        &self.canvas
    }
}

#[test]
fn a_window_opens_paints_and_closes_on_a_real_server() {
    let _serial = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(server) = Server::start(97, 640, 480) else {
        eprintln!("skipped: Xvfb is not on this machine");
        return;
    };
    let seen = Rc::new(RefCell::new(Seen::default()));
    let painter = Painter {
        canvas: Canvas::new(1, 1),
        seen: Rc::clone(&seen),
        frame_buffer: server.frame_buffer.clone(),
    };
    let options = WindowOptions { title: "Painter".to_owned(), width: 400, height: 300 };
    wp_shell::run(options, Box::new(painter)).expect("the window opens on the virtual server");

    let seen = Rc::try_unwrap(seen).ok().expect("the shell let go of the program").into_inner();
    assert_eq!(seen.ticks, CLOSE_AFTER, "the loop ticks until the program closes");
    assert!(seen.sizes.contains(&(400, 300)), "the window is the size asked for: {:?}", seen.sizes);
    assert!(
        (seen.scale - 1.0).abs() < 0.01,
        "a server with no Xft.dpi is at scale 1: {}",
        seen.scale
    );
    assert_eq!(seen.asked_to_close, 0, "closing itself is not being asked to close");

    let (width, height, rgb) = seen.photographed.expect("the server's frame buffer was read");
    assert_eq!((width, height), (640, 480));
    keep_proof("linux-window.png", width, height, &rgb);
    // The window has no frame on a bare server, so it sits at the origin:
    // the top left is the painted red, and inside it the white square.
    let at = |x: usize, y: usize| {
        let base = (y * width + x) * 3;
        (rgb[base], rgb[base + 1], rgb[base + 2])
    };
    assert_eq!(at(10, 10), (200, 40, 40), "the window's own paint reached the server");
    assert_eq!(at(100, 80), (255, 255, 255), "and so did the square drawn on it");
    assert_eq!(
        dominant(&rgb[(width * 3) * 350..]),
        (0, 0, 0),
        "below the window the screen is still bare"
    );

    let own = seen.own_shot.expect("the shell's own screenshot");
    assert_eq!((own.width, own.height), (640, 480));
    let own_at = |x: usize, y: usize| {
        let base = (y * own.width + x) * 4;
        (own.pixels[base], own.pixels[base + 1], own.pixels[base + 2])
    };
    assert_eq!(
        own_at(10, 10),
        (200, 40, 40),
        "the shell's screenshot agrees with the frame buffer"
    );
    assert_eq!(own_at(100, 80), (255, 255, 255));

    assert_eq!(
        seen.windows_seen,
        vec!["Painter".to_owned()],
        "the screenshot list names the open window"
    );
    assert_eq!(
        seen.pasted.as_deref(),
        Some("carried by the X selection"),
        "text put on the clipboard comes back"
    );
}

/// Closes on the second request only; the first is refused the way an
/// editor refuses while it asks about unsaved changes.
struct Stubborn {
    canvas: Canvas,
    seen: Rc<RefCell<Seen>>,
}

impl App for Stubborn {
    fn handle(&mut self, event: Event) -> Response {
        let mut seen = self.seen.borrow_mut();
        match event {
            Event::Tick => {
                seen.ticks += 1;
                if seen.ticks == 5 || seen.ticks == 15 {
                    // Asks its own window to close through the window
                    // manager's channel, as the close button does.
                    wp_shell::window_command(WindowCommand::Close);
                }
                if seen.ticks > 60 {
                    return Response::Close;
                }
                Response::Ignored
            }
            Event::Closing => {
                seen.asked_to_close += 1;
                if seen.asked_to_close == 1 {
                    Response::Refuse
                } else {
                    Response::Close
                }
            }
            _ => Response::Ignored,
        }
    }

    fn draw(&mut self, width: usize, height: usize) -> &Canvas {
        self.canvas = Canvas::new(width, height);
        self.canvas.clear(Color::rgb(40, 40, 200));
        &self.canvas
    }
}

#[test]
fn a_program_that_refuses_to_close_stays_up_until_it_agrees() {
    let _serial = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(_server) = Server::start(98, 320, 240) else {
        eprintln!("skipped: Xvfb is not on this machine");
        return;
    };
    let seen = Rc::new(RefCell::new(Seen::default()));
    let stubborn = Stubborn { canvas: Canvas::new(1, 1), seen: Rc::clone(&seen) };
    let options = WindowOptions { title: "Stubborn".to_owned(), width: 200, height: 150 };
    wp_shell::run(options, Box::new(stubborn)).expect("the window opens");
    let seen = Rc::try_unwrap(seen).ok().expect("the shell let go of the program").into_inner();
    assert_eq!(seen.asked_to_close, 2, "asked to close twice: once refused, once agreed");
    assert!(
        (15..=30).contains(&seen.ticks),
        "closed on the second request, not the timeout: {} ticks",
        seen.ticks
    );
}
