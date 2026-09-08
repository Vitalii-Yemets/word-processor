//! Putting a picture of the screen into the document.
//!
//! # What this does and what Word's button does
//!
//! Word offers a thumbnail of each open window, and below them Screen Clipping,
//! which hides Word and lets a rectangle be dragged out of whatever is behind
//! it. The list of windows is here, by name rather than by thumbnail, together
//! with the whole screen. The dragged clipping is not: it needs a window of its
//! own drawn over the desktop, and the two ways of getting a picture that this
//! list gives are the ones people use.
//!
//! The picture goes in as a PNG. It could go in as a bitmap and save the
//! compressing, but a document full of uncompressed screenshots is a document
//! nobody can send anywhere.

use wp_raster::{Canvas, Color};
use wp_shell::Response;

use crate::chrome::{Choice, Command, Popup};

use wp_docx::EMU_PER_INCH;

use super::{Editor, DPI};

impl Editor {
    /// Drops open the whole screen and every window that is open.
    pub(super) fn open_screenshot(&mut self) -> Response {
        if self.close_popup_if(Choice::Screenshot) {
            return Response::Redraw;
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::Screenshot) else {
            return Response::Ignored;
        };

        self.screen_windows = wp_shell::screen::windows();
        let mut items = vec!["The whole screen".to_owned()];
        items.extend(self.screen_windows.iter().map(|window| window.title.clone()));

        self.popup = Some(Popup::new(Choice::Screenshot, items, None, left, top, 380.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Takes the picture that was chosen and puts it in the document.
    pub(super) fn choose_screenshot(&mut self, index: usize) -> Response {
        self.popup = None;

        // Nothing of this program should be in the picture, so the list it was
        // chosen from is gone before the shutter goes.
        let shot = match index.checked_sub(1) {
            None => wp_shell::screen::capture_screen(),
            Some(at) => match self.screen_windows.get(at) {
                Some(window) => wp_shell::screen::capture_window(window.handle),
                None => return Response::Ignored,
            },
        };

        let Some(shot) = shot else {
            return self.report("The screen could not be photographed");
        };
        if shot.width == 0 || shot.height == 0 {
            return self.report("The picture came back empty");
        }

        let bytes = wp_raster::encode_png(&canvas_from(&shot));

        // A screenshot is measured in screen pixels, and a screen pixel is a
        // ninety-sixth of an inch — the same as a picture from a file. One
        // wider than the text is brought down to fit.
        let per_pixel = EMU_PER_INCH / DPI as i64;
        let mut width = shot.width as i64 * per_pixel;
        let mut height = shot.height as i64 * per_pixel;
        let room = self.text_width_emu();
        if room > 0 && width > room {
            height = height * room / width;
            width = room;
        }

        match self.document.insert_picture(&bytes, "png", width, height) {
            Ok(inserted) => {
                let (across, down) = (shot.width, shot.height);
                self.edited(inserted, &format!("Screenshot, {across} by {down}"))
            }
            Err(error) => self.report(&format!("The picture could not be inserted: {error}")),
        }
    }
}

/// Turns what the system handed back into something that can be encoded.
fn canvas_from(shot: &wp_shell::screen::Shot) -> Canvas {
    let mut canvas = Canvas::new(shot.width, shot.height);
    for y in 0..shot.height {
        for x in 0..shot.width {
            let at = (y * shot.width + x) * 4;
            let Some(pixel) = shot.pixels.get(at..at + 4) else { continue };
            // Straight over the top: the canvas starts transparent, and a
            // screen has nothing behind it to blend with.
            canvas.blend(x, y, Color::rgba(pixel[0], pixel[1], pixel[2], pixel[3]), 255);
        }
    }
    canvas
}
