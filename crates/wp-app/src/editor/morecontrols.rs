//! The three content controls that hold more than words: a picture, a
//! repeating section, and a gallery of building blocks.
//!
//! # A picture control is clicked to be given its picture
//!
//! It is put in holding a placeholder the program draws — a grey card with
//! a hill and a sun on it, the shape everybody reads as "a picture goes
//! here" — because a control holding nothing has no width and nothing to
//! click. Clicking it asks for a file, as Insert Picture does, and the
//! picture chosen takes the placeholder's place at its own size, brought
//! down to the width of the text where it is wider.
//!
//! # A repeating section repeats by being copied
//!
//! Word draws a plus at the corner of each item of a repeating section, and
//! pressing it puts a copy of the item after it, words and all. So does
//! this: the plus is drawn at the end of the item's last line, and its menu
//! — the right button — offers a copy before or after, and taking the item
//! away, which Word refuses for the last one and so does this.
//!
//! # A gallery control offers what is filed under its gallery
//!
//! Clicking one drops the list of the blocks in its gallery — the person's
//! own, from the template they live in ([`super::ownblocks`]) — and choosing
//! one puts the block's paragraphs in place of what the control held. A
//! gallery with nothing filed in it says so.

use wp_docx::blockcontrols::BlockKind;
use wp_docx::controls::ControlKind;
use wp_docx::{TextPosition, EMU_PER_INCH};
use wp_raster::{Canvas, Color};
use wp_shell::Response;

use crate::chrome::{Choice, Popup};
use crate::messages::t;

use super::{Editor, DPI};

/// The gallery a new gallery control offers, which is Word's default.
const DEFAULT_GALLERY: &str = "Quick Parts";
const DEFAULT_CATEGORY: &str = "General";

/// How big the placeholder picture is, in pixels at the screen's usual
/// ninety-six to the inch: an inch and a half square.
const PLACEHOLDER: usize = 144;

/// How big the plus at the corner of a repeating item is drawn.
const PLUS: f32 = 14.0;

/// The placeholder a picture control holds until it is given a picture: a
/// grey card with a hill and a sun.
#[must_use]
pub fn placeholder_picture() -> Vec<u8> {
    let mut canvas = Canvas::new(PLACEHOLDER, PLACEHOLDER);
    let size = PLACEHOLDER as i32;
    canvas.fill_rect(0, 0, size, size, Color::rgb(0xE8, 0xE8, 0xE8));
    let edge = Color::rgb(0xB0, 0xB0, 0xB0);
    canvas.fill_rect(0, 0, size, 2, edge);
    canvas.fill_rect(0, size - 2, size, 2, edge);
    canvas.fill_rect(0, 0, 2, size, edge);
    canvas.fill_rect(size - 2, 0, 2, size, edge);
    // The sun, as rows of a circle.
    let sun = Color::rgb(0xA0, 0xA0, 0xA0);
    let (centre_x, centre_y, radius) = (size * 3 / 4, size / 4, size / 12);
    for row in -radius..=radius {
        let half = ((radius * radius - row * row) as f32).sqrt() as i32;
        canvas.fill_rect(centre_x - half, centre_y + row, half * 2, 1, sun);
    }
    // And the hill, as rows of a triangle, with a smaller one behind it.
    let hill = Color::rgb(0x90, 0x90, 0x90);
    let base = size * 3 / 4;
    for row in 0..(size / 2) {
        let width = row * 2;
        canvas.fill_rect(size / 2 - width / 2 - size / 8, base - size / 2 + row, width, 1, hill);
    }
    for row in 0..(size / 3) {
        let width = row * 2;
        canvas.fill_rect(size * 5 / 8 - width / 2, base - size / 3 + row, width, 1, hill);
    }
    wp_raster::encode_png(&canvas)
}

impl Editor {
    /// Word's Picture Content Control: put in at the caret, holding the
    /// placeholder.
    pub(super) fn insert_picture_control(&mut self) -> Response {
        let per_pixel = EMU_PER_INCH / DPI as i64;
        let side = PLACEHOLDER as i64 * per_pixel;
        match self.document.insert_picture_control(&placeholder_picture(), "png", side, side) {
            Ok(changed) => {
                self.relayout();
                self.reveal_caret();
                self.edited(changed, ControlKind::Picture.label())
            }
            Err(error) => self.report(&crate::messages::with(
                "Cannot insert the picture: {0}",
                &[&error.to_string()],
            )),
        }
    }

    /// A click on a picture control: asks for the picture it is for.
    pub(super) fn choose_control_picture(&mut self, at: TextPosition) -> Response {
        let Some(path) = wp_shell::dialog::open_file(
            t("Insert Picture"),
            &super::files::readable(super::insert::PICTURE_FILTERS),
        ) else {
            return Response::Ignored;
        };
        self.put_picture_into_control(at, &path)
    }

    /// The same from a file already chosen.
    fn put_picture_into_control(&mut self, at: TextPosition, path: &std::path::Path) -> Response {
        let Ok(bytes) = std::fs::read(path) else {
            return self
                .report(&crate::messages::with("Cannot read {0}", &[&path.display().to_string()]));
        };
        let extension =
            path.extension().and_then(|value| value.to_str()).unwrap_or("png").to_owned();
        self.picture_into_control(at, &bytes, &extension)
    }

    /// And from the bytes of one, at its own size brought down to the
    /// width of the text.
    pub(super) fn picture_into_control(
        &mut self,
        at: TextPosition,
        bytes: &[u8],
        extension: &str,
    ) -> Response {
        let Ok(image) = wp_image::decode(bytes) else {
            return self.report(t(
                "That is not a picture this program can read. PNG, JPEG, BMP, GIF, TIFF and the metafiles are.",
            ));
        };
        let per_pixel = EMU_PER_INCH / DPI as i64;
        let mut width = image.width.max(1) as i64 * per_pixel;
        let mut height = image.height.max(1) as i64 * per_pixel;
        let room = self.text_width_emu();
        if room > 0 && width > room {
            height = height * room / width;
            width = room;
        }
        match self.document.set_control_picture(at, bytes, extension, width, height) {
            Ok(changed) => {
                self.relayout();
                self.edited(changed, t("Picture"))
            }
            Err(error) => self.report(&crate::messages::with(
                "Cannot insert the picture: {0}",
                &[&error.to_string()],
            )),
        }
    }

    /// Word's Repeating Section Content Control: round the selection, or
    /// the paragraph the caret is in.
    pub(super) fn insert_repeating_section(&mut self) -> Response {
        let changed = self.document.insert_repeating_section();
        if !changed {
            return self.report(t(
                "A repeating section goes round whole paragraphs that are side by side",
            ));
        }
        self.relayout();
        self.edited(true, t("Repeating Section Content Control"))
    }

    /// A copy of the item the caret is in, before it or after it.
    pub(super) fn repeat_item(&mut self, after: bool) -> Response {
        let paragraph = self.document.caret().paragraph;
        let changed = self.document.add_repeating_item(paragraph, after);
        if !changed {
            return self.report(t("The caret is not in a repeating section"));
        }
        self.relayout();
        self.reveal_caret();
        self.edited(true, t("Item added"))
    }

    /// The item the caret is in, taken out — unless it is the last.
    pub(super) fn delete_repeat_item(&mut self) -> Response {
        let paragraph = self.document.caret().paragraph;
        if self.document.repeating_item_at(paragraph).is_none() {
            return self.report(t("The caret is not in a repeating section"));
        }
        let changed = self.document.remove_repeating_item(paragraph);
        if !changed {
            return self.report(t("The last item of a repeating section stays"));
        }
        self.relayout();
        self.reveal_caret();
        self.edited(true, t("Item deleted"))
    }

    /// Whether the caret is in a repeating section, for the menu.
    #[must_use]
    pub(super) fn in_repeating_item(&self) -> bool {
        self.document.repeating_item_at(self.document.caret().paragraph).is_some()
    }

    /// Word's Building Block Gallery Content Control, at the caret.
    pub(super) fn insert_gallery_control(&mut self) -> Response {
        let changed = self.document.insert_gallery_control(DEFAULT_GALLERY, DEFAULT_CATEGORY);
        self.relayout();
        self.reveal_caret();
        self.edited(changed, t("Building Block Gallery Content Control"))
    }

    /// A click in a gallery control: the caret goes there, and the gallery
    /// drops open under it.
    pub(super) fn offer_gallery(&mut self, at: TextPosition) -> Option<Response> {
        let control = self.document.block_control_at(at.paragraph)?;
        let BlockKind::Gallery { gallery, .. } = control.kind else { return None };
        self.document.set_caret(at);
        let names: Vec<String> = self
            .own_template()
            .map(|template| template.blocks_in(&gallery))
            .unwrap_or_default()
            .into_iter()
            .map(|block| block.name)
            .collect();
        if names.is_empty() {
            return Some(self.report(&crate::messages::with(
                "There is nothing in the {0} gallery to choose from",
                &[&gallery],
            )));
        }
        let (left, top) = match self.caret_rect() {
            Some((x, y, _, height)) => (x, y + height),
            None => (100.0, 200.0),
        };
        self.gallery_names = names.clone();
        self.popup = Some(Popup::new(Choice::GalleryBlock, names, None, left, top, 220.0));
        self.needs_redraw = true;
        Some(Response::Redraw)
    }

    /// Whichever block was chosen goes into the control.
    pub(super) fn choose_gallery_block(&mut self, index: usize) -> Response {
        self.popup = None;
        let Some(name) = self.gallery_names.get(index).cloned() else { return Response::Ignored };
        let Some(template) = self.own_template() else { return Response::Ignored };
        self.choose_block_from(&template, &name)
    }

    /// The block of that name in a template, put into the control the
    /// caret is in.
    pub(super) fn choose_block_from(
        &mut self,
        template: &wp_docx::Document,
        name: &str,
    ) -> Response {
        let Some(body) = template.building_block_body(name) else {
            return self
                .report(&crate::messages::with("{0} is no longer in the template", &[name]));
        };
        let paragraph = self.document.caret().paragraph;
        let changed = self.document.fill_gallery_control(paragraph, &body.blocks);
        self.relayout();
        self.reveal_caret();
        self.edited(changed, &crate::messages::with("{0} put in", &[name]))
    }

    /// Draws the plus at the corner of every repeating item, and remembers
    /// where each is for the press that follows.
    pub(super) fn draw_repeat_buttons(&mut self) {
        self.repeat_buttons.clear();
        let items: Vec<(usize, usize)> = self
            .document
            .block_controls()
            .into_iter()
            .filter(|control| control.kind == BlockKind::RepeatingItem)
            .map(|control| (control.first, control.last))
            .collect();
        if items.is_empty() {
            return;
        }
        let mut places: Vec<(f32, f32, usize)> = Vec::new();
        for index in 0..self.pages.len() {
            let (origin_x, origin_y) = self.page_origin(index);
            let top = self.content_top() + origin_y - self.scroll_down();
            for (_, last) in &items {
                // The last character of the item's last paragraph, or the
                // paragraph's own mark where it has none: the plus goes
                // after it.
                let end = self.document.paragraph_text(*last).map_or(0, |text| text.len());
                let (from, to) = if end > 0 { (end - 1, end) } else { (0, 1) };
                let rects = self.pages[index]
                    .selection_rects(TextPosition::new(*last, from), TextPosition::new(*last, to));
                let Some((x, y, width, height)) = rects.last().copied() else { continue };
                places.push((origin_x + x + width + 6.0, top + y + height - PLUS, *last));
            }
        }
        let colour = self.theme.control_edge;
        let ink = self.theme.text;
        for (x, y, paragraph) in places {
            self.canvas.fill_rect(x as i32, y as i32, PLUS as i32, PLUS as i32, self.theme.field);
            for (dx, dy, w, h) in [
                (0.0, 0.0, PLUS, 1.0),
                (0.0, PLUS - 1.0, PLUS, 1.0),
                (0.0, 0.0, 1.0, PLUS),
                (PLUS - 1.0, 0.0, 1.0, PLUS),
            ] {
                self.canvas.fill_rect((x + dx) as i32, (y + dy) as i32, w as i32, h as i32, colour);
            }
            let middle = PLUS / 2.0;
            self.canvas.fill_rect(
                (x + 3.0) as i32,
                (y + middle) as i32,
                (PLUS - 6.0) as i32,
                1,
                ink,
            );
            self.canvas.fill_rect(
                (x + middle) as i32,
                (y + 3.0) as i32,
                1,
                (PLUS - 6.0) as i32,
                ink,
            );
            self.repeat_buttons.push((x, y, PLUS, PLUS, paragraph));
        }
    }

    /// A press on one of those pluses, if it was on one.
    pub(super) fn press_repeat_button(&mut self, x: i32, y: i32) -> bool {
        let (x, y) = (crate::chrome::mirror::flip(x) as f32, y as f32);
        let hit = self
            .repeat_buttons
            .iter()
            .find(|(left, top, width, height, _)| {
                x >= *left && x < left + width && y >= *top && y < top + height
            })
            .map(|(.., paragraph)| *paragraph);
        let Some(paragraph) = hit else { return false };
        if self.document.add_repeating_item(paragraph, true) {
            self.relayout();
            self.reveal_caret();
            self.status = t("Item added").to_owned();
            self.update_title();
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::Document;
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event};

    use crate::chrome::Command;

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    fn editor(lines: &[&str]) -> Editor {
        let mut body = Body::default();
        for line in lines {
            body.blocks.push(Block::Paragraph(Paragraph::text(line)));
        }
        let document = Document::create(&body).expect("a document");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor
    }

    #[test]
    fn the_placeholder_is_a_picture_this_program_can_read_back() {
        let image = wp_image::decode(&placeholder_picture()).expect("a picture");
        assert_eq!((image.width, image.height), (PLACEHOLDER, PLACEHOLDER));
    }

    #[test]
    fn a_picture_control_goes_in_from_the_ribbon_and_takes_a_picture_when_clicked() {
        let mut editor = editor(&["Before"]);
        editor.set_view_option("tab=developer").expect("the Developer tab");
        editor.document.set_caret(TextPosition::new(0, 6));
        editor.run(Command::Control(6));
        let control = editor.document.controls().pop().expect("the control");
        assert_eq!(control.kind, ControlKind::Picture);
        assert_eq!(
            (control.start.offset, control.end.offset),
            (6, 7),
            "a picture is one character"
        );

        // A picture chosen for it.
        let mut canvas = Canvas::new(20, 10);
        canvas.fill_rect(0, 0, 20, 10, Color::rgb(0, 0, 0xFF));
        editor.picture_into_control(control.start, &wp_raster::encode_png(&canvas), "png");

        assert!(editor.status.contains("Picture"), "{}", editor.status);
        let control = editor.document.controls().pop().expect("the control again");
        assert_eq!(control.kind, ControlKind::Picture);
        // Two pictures in the package now: the placeholder and the chosen.
        let pictures = editor
            .document
            .package()
            .entries()
            .iter()
            .filter(|entry| entry.name.starts_with("word/media/"))
            .count();
        assert_eq!(pictures, 2);
        let again = Document::open(&editor.document.save().expect("saving")).expect("reopening");
        assert_eq!(again.controls()[0].kind, ControlKind::Picture);
    }

    #[test]
    fn a_repeating_section_repeats_from_its_plus_and_its_menu() {
        let mut editor = editor(&["Name", "Street", "After"]);
        editor.document.set_selections(&[(TextPosition::new(0, 0), TextPosition::new(1, 6))]);
        editor.run(Command::RepeatingSection);
        assert!(editor.in_repeating_item());
        assert_eq!(editor.document.block_controls().len(), 2);

        // The plus is drawn at the corner of the item, and pressing it
        // copies the item.
        editor.draw(1400, 900);
        assert_eq!(editor.repeat_buttons.len(), 1, "no plus was drawn");
        let (x, y, width, height, _) = editor.repeat_buttons[0];
        assert!(editor.press_repeat_button((x + width / 2.0) as i32, (y + height / 2.0) as i32));
        assert_eq!(editor.document.plain_text(), "Name\nStreet\nName\nStreet\nAfter");

        // The menu's three: before, after, and away.
        editor.document.set_caret(TextPosition::new(2, 0));
        editor.run(Command::RepeatItemBefore);
        assert_eq!(editor.document.plain_text(), "Name\nStreet\nName\nStreet\nName\nStreet\nAfter");
        editor.run(Command::DeleteRepeatItem);
        editor.run(Command::DeleteRepeatItem);
        assert_eq!(editor.document.plain_text(), "Name\nStreet\nAfter");
        editor.run(Command::DeleteRepeatItem);
        assert!(editor.status.contains("stays"), "{}", editor.status);
        assert_eq!(editor.document.plain_text(), "Name\nStreet\nAfter");
    }

    #[test]
    fn a_gallery_control_offers_the_blocks_of_its_gallery_and_takes_the_one_chosen() {
        let mut editor = editor(&["Before", ""]);
        editor.document.set_caret(TextPosition::new(1, 0));
        editor.run(Command::GalleryControl);
        assert_eq!(editor.document.plain_text(), "Before\nChoose a building block.");

        // A click in it is the control's, and with nothing filed under the
        // gallery on this machine it says so; a click elsewhere is not.
        assert!(editor.offer_gallery(TextPosition::new(0, 3)).is_none());
        let offered = editor.offer_gallery(TextPosition::new(1, 3)).expect("the control's click");
        assert_eq!(offered, Response::Redraw);
        assert!(editor.status.contains("Quick Parts"), "{}", editor.status);
        assert_eq!(editor.document.caret(), TextPosition::new(1, 3));

        // A template with a block in it — in memory, since the one on disk
        // is the person's own — and the block chosen goes into the control.
        let mut template = Document::create(&Body::default()).expect("a template");
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("Yours faithfully,")));
        body.blocks.push(Block::Paragraph(Paragraph::text("A. Habgood")));
        assert!(
            template.add_building_block(&wp_docx::blocks::BuildingBlock::named("Sign-off"), &body)
        );
        editor.choose_block_from(&template, "Sign-off");
        assert_eq!(editor.document.plain_text(), "Before\nYours faithfully,\nA. Habgood");
        assert!(matches!(
            editor.document.block_control_at(2).map(|control| control.kind),
            Some(BlockKind::Gallery { .. })
        ));
    }
}
