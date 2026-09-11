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

mod autocorrect;
mod chrome;
mod editor;
mod measure;
mod sample;
mod settings;

use std::path::{Path, PathBuf};

use editor::Editor;
use wp_docx::Document;
use wp_layout::FontLibrary;
use wp_shell::{App, Event, WindowOptions};

fn main() -> std::process::ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();

    // A picture of the window, written to a file instead of shown on a screen.
    // The whole interface is drawn by this program onto a canvas, so it can be
    // drawn without a window at all — which is how it gets checked on a machine
    // that has no display.
    let outcome = match arguments.first().map(String::as_str) {
        Some("--picture") => picture(&arguments[1..]),
        _ => start(arguments.first().map(String::as_str)),
    };

    match outcome {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            std::process::ExitCode::FAILURE
        }
    }
}

/// Draws the window into a PNG rather than onto a screen.
fn picture(arguments: &[String]) -> Result<(), String> {
    let [document_path, image_path, rest @ ..] = arguments else {
        return Err("usage: --picture <document.docx|-> <image.png> [width height] [option...]
\n             options: light, tab=<file|home|insert|layout|view>,
             file=<info|new|open|saveas|export>, nonav, marks"
            .to_owned());
    };
    let width: usize = rest
        .first()
        .map_or(Ok(1400), |value| value.parse())
        .map_err(|error: std::num::ParseIntError| format!("width is not a number: {error}"))?;
    let height: usize = rest
        .get(1)
        .map_or(Ok(900), |value| value.parse())
        .map_err(|error: std::num::ParseIntError| format!("height is not a number: {error}"))?;

    let library = font_library()?;
    let document = if document_path == "-" {
        Document::create(&sample::welcome_document())
            .map_err(|error| format!("cannot build the sample document: {error}"))?
    } else {
        let bytes = std::fs::read(document_path)
            .map_err(|error| format!("cannot read {document_path}: {error}"))?;
        Document::open(&bytes).map_err(|error| format!("cannot open {document_path}: {error}"))?
    };

    let mut editor = Editor::new(library, document, None);
    // The size comes first: an option that puts something on the screen has to
    // know how big the screen is before it can decide where.
    editor.handle(Event::Resized { width: width as u32, height: height as u32 });
    // Drawn once before the options are applied, because some of them ask the
    // ribbon where one of its buttons ended up, and a ribbon that has never
    // been drawn does not know.
    editor.draw(width, height);

    // The options exist because this is the only way the interface can be
    // looked at on a machine with no display, and a picture of one theme and
    // one tab would leave most of it unseen.
    for option in rest.iter().skip(2) {
        // Drawn again between one option and the next, because an option may
        // depend on where the last one put something: `menu=accept` hangs a
        // list under a button that `tab=review` has only just brought out.
        editor.draw(width, height);
        editor.set_view_option(option)?;
    }

    let canvas = editor.draw(width, height);

    std::fs::write(image_path, wp_raster::encode_png(canvas))
        .map_err(|error| format!("cannot write {image_path}: {error}"))
}

/// The fonts on this machine.
///
/// Given the lifetime of the process: the layout engine and the renderer both
/// borrow from it for as long as the window is open, and threading a lifetime
/// through every type would buy nothing, because nothing is freed anyway.
fn font_library() -> Result<&'static FontLibrary, String> {
    let library: &'static FontLibrary = Box::leak(Box::new(FontLibrary::scan_system()));
    if library.is_empty() {
        return Err("no usable fonts were found on this machine".to_owned());
    }
    Ok(library)
}

fn start(path: Option<&str>) -> Result<(), String> {
    if !wp_shell::is_supported() {
        return Err(
            "this build has no window support; only Windows is implemented so far".to_owned()
        );
    }

    let library = font_library()?;
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
            let document = Document::create(&sample::welcome_document())
                .map_err(|error| format!("cannot build the sample document: {error}"))?;
            (document, None, "Document".to_owned())
        }
    };

    let mut editor = Editor::new(library, document, file);
    // The window comes up the way it was left rather than the way it starts.
    editor.apply_settings(settings::Settings::load());
    let options =
        WindowOptions { title: format!("{title} — Word Processor"), width: 1400, height: 900 };

    wp_shell::run(options, Box::new(editor)).map_err(|error| error.to_string())
}

fn file_name(path: &str) -> String {
    Path::new(path).file_name().and_then(|name| name.to_str()).unwrap_or(path).to_owned()
}
