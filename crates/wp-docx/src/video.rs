//! A video from the web, as a document keeps one.
//!
//! # What Word writes
//!
//! A picture and an address. The frame Word could fetch goes in as an ordinary
//! picture; the address goes on the drawing's own properties as a link, the
//! same link any picture may carry; and beside that link an extension says
//! that the picture stands for a video rather than being one. Word draws a
//! play sign over such a picture, and pressing it plays the video.
//!
//! So a document with a video in it holds no video: it holds a still of one,
//! the address it plays from, and the markup that would embed a player.
//!
//! # What this does
//!
//! Reads all three and draws the frame with the sign over it, so a document
//! from Word looks here as it does there, and a press follows the address to
//! whatever the person watches videos with.
//!
//! Writing one takes a frame, and a frame is a still of a video: this program
//! does not talk to the network and cannot fetch one. So the caller passes the
//! frame — and the one place in the program that offers to put a video in has
//! none to pass, which is why it writes a link instead. See the roadmap.

use wp_xml::tree::Element;

use crate::{Document, Error, TextPosition};

/// The namespace the video extension is written in.
pub const WEB_VIDEO: &str = "http://schemas.microsoft.com/office/word/2012/wordprocessingDrawing";
/// What an extension list calls a video. The format has one of these for every
/// extension; this is the number Word gives to a video from the web.
pub const WEB_VIDEO_URI: &str = "{C809E66F-F1BF-436E-b5F7-EEA9579F0CBA}";

impl Document {
    /// Puts a video from the web at the caret: its frame, and the address it
    /// plays from.
    ///
    /// The frame is a picture in whatever format it arrives in, the same as
    /// any other, because that is what it is.
    pub fn insert_web_video(
        &mut self,
        frame: &[u8],
        extension: &str,
        address: &str,
        width_emu: i64,
        height_emu: i64,
    ) -> Result<bool, Error> {
        if address.trim().is_empty() {
            return Ok(false);
        }

        // One gesture: the picture, the address and the mark that ties them
        // together are one thing to put in and one thing to take back.
        self.begin_gesture();
        let inserted = self.insert_picture(frame, extension, width_emu, height_emu)?;
        if !inserted {
            self.end_gesture();
            return Ok(false);
        }

        let link = self.link_relationship(address);
        let caret = self.caret();
        let at = TextPosition::new(caret.paragraph, caret.offset.saturating_sub(1));
        let marked = self.mark_as_video(at, link.as_deref());
        self.end_gesture();
        Ok(marked)
    }

    /// Says of the drawing at one place that it is a video, and where it plays
    /// from.
    fn mark_as_video(&mut self, at: TextPosition, link: Option<&str>) -> bool {
        let Some(path) = crate::position::paragraph_path(&self.tree().root, at.paragraph) else {
            return false;
        };
        let Some(paragraph) =
            crate::edit::element_at_path_mut(&mut self.tree_to_edit().root, &path)
        else {
            return false;
        };

        let mut done = false;
        let mut offset = 0usize;
        crate::floating::walk_drawings_mut(paragraph, &mut offset, at.offset, &mut |drawing| {
            let Some(properties) = drawing_properties(drawing) else { return };
            if let Some(link) = link {
                let mut click = Element::new("a:hlinkClick", Some(crate::edit::DRAWING_MAIN));
                click
                    .declarations
                    .push((Some("a".to_owned()), crate::edit::DRAWING_MAIN.to_owned()));
                // And the one the address is named in: an element that uses a
                // prefix it does not declare is not XML, and the document
                // written with it does not open again.
                click
                    .declarations
                    .push((Some("r".to_owned()), crate::edit::RELATIONSHIPS.to_owned()));
                click.set_namespaced_attribute("r:id", crate::edit::RELATIONSHIPS, link);
                properties.push_element(click);
            }

            let mut list = Element::new("a:extLst", Some(crate::edit::DRAWING_MAIN));
            list.declarations.push((Some("a".to_owned()), crate::edit::DRAWING_MAIN.to_owned()));
            let mut extension = Element::new("a:ext", Some(crate::edit::DRAWING_MAIN));
            extension.set_attribute("uri", WEB_VIDEO_URI);
            let mut video = Element::new("wp15:webVideoPr", Some(WEB_VIDEO));
            video.declarations.push((Some("wp15".to_owned()), WEB_VIDEO.to_owned()));
            // What Word writes to embed a player. Nothing here embeds one, and
            // an empty string is the truthful answer rather than markup this
            // program cannot make good on.
            video.set_attribute("embeddedHtml", "");
            extension.push_element(video);
            list.push_element(extension);
            properties.push_element(list);
            done = true;
        });

        if done {
            self.note_change();
        }
        done
    }
}

/// The drawing's own properties, which is where a link and an extension go.
fn drawing_properties(drawing: &mut Element) -> Option<&mut Element> {
    fn search(element: &mut Element) -> Option<&mut Element> {
        if element.local_name() == "docPr" {
            return Some(element);
        }
        for child in element.child_elements_mut() {
            if let Some(found) = search(child) {
                return Some(found);
            }
        }
        None
    }
    search(drawing)
}
