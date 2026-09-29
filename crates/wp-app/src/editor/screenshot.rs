//! Putting a picture of the screen into the document.
//!
//! # What this does and what Word's button does
//!
//! Word offers a thumbnail of each open window, and below them Screen Clipping,
//! which hides Word and lets a rectangle be dragged out of whatever is behind
//! it. The list of windows is here, by name rather than by thumbnail, together
//! with the whole screen. Screen Clipping is offered where the desktop does
//! the dragging — on Wayland, through its portal, which is also the only way
//! a program there may photograph the screen at all, and which lists no
//! windows. Elsewhere it would need a window of this program's own drawn
//! over the desktop, which there is not.
//!
//! The picture goes in as a PNG. It could go in as a bitmap and save the
//! compressing, but a document full of uncompressed screenshots is a document
//! nobody can send anywhere.

use wp_raster::{Canvas, Color};
use wp_shell::Response;

use crate::chrome::{Choice, Command, Popup};
use crate::messages::t;

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
        let mut items = vec![t("The whole screen").to_owned()];
        items.extend(self.screen_windows.iter().map(|window| window.title.clone()));
        // Last, under the windows, where Word puts it.
        if wp_shell::screen::can_clip() {
            items.push(t("Screen Clipping").to_owned());
        }

        self.popup = Some(Popup::new(Choice::Screenshot, items, None, left, top, 380.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Takes the list away, and the picture that was chosen on the next
    /// tick.
    ///
    /// Nothing of this program should be in the picture, so the list it was
    /// chosen from is off the screen before the shutter goes — which means
    /// drawn again without it first. Taken at once, the picture was of the
    /// window as it last was: with the list still open over it.
    pub(super) fn choose_screenshot(&mut self, index: usize) -> Response {
        self.popup = None;
        self.screenshot_due = Some(index);
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Takes the picture chosen, if one is due, and puts it in the document.
    pub(super) fn take_screenshot_due(&mut self) -> Option<Response> {
        let index = self.screenshot_due.take()?;
        Some(self.take_screenshot(index))
    }

    fn take_screenshot(&mut self, index: usize) -> Response {
        let clipping = index == self.screen_windows.len() + 1;
        let shot = match index.checked_sub(1) {
            None => wp_shell::screen::capture_screen(),
            // The rectangle is the person's to drag out, and theirs to
            // cancel: a cancelled one is no picture, and nothing is wrong.
            Some(_) if clipping => match wp_shell::screen::clip() {
                Some(shot) => Some(shot),
                None => return self.report("No picture was taken"),
            },
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
                self.choose_drawing_here();
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

#[cfg(test)]
mod tests {
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::Document;
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event};

    use super::*;

    fn editor() -> Editor {
        let library: &'static FontLibrary = Box::leak(Box::new(FontLibrary::scan_system()));
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("Hello")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let mut editor = Editor::new(library, Document::open(&bytes).expect("reopening"), None);
        editor.handle(Event::Resized { width: 1200, height: 800 });
        editor.draw(1200, 800);
        editor
    }

    /// The list is taken off the screen, and the window drawn again without
    /// it, before the picture is taken: on the next tick, not at the press.
    #[test]
    fn the_picture_is_taken_once_the_list_is_off_the_screen() {
        let mut editor = editor();
        editor.choose_tab(crate::chrome::ribbon::Tab::Insert);
        editor.draw(1200, 800);
        editor.run(Command::Screenshot);
        assert!(editor.popup.as_ref().is_some_and(|popup| popup.choice == Choice::Screenshot));
        assert_eq!(editor.popup.as_ref().and_then(|popup| popup.item(0)), Some("The whole screen"));

        assert_eq!(editor.choose_screenshot(0), Response::Redraw);
        assert!(editor.popup.is_none(), "the list is gone");
        assert_eq!(editor.screenshot_due, Some(0), "and the picture waits for the next tick");
        assert!(editor.status.is_empty(), "nothing has been tried yet");

        editor.draw(1200, 800);
        editor.handle(Event::Tick);
        assert_eq!(editor.screenshot_due, None);
        // With no screen to photograph here, the attempt says so.
        assert_eq!(editor.status, "The screen could not be photographed");
    }
}
