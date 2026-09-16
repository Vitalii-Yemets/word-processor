//! Quick parts, WordArt and the selection pane.
//!
//! Three small things from the Insert and Layout tabs that only became possible
//! once fields and shapes were there: a quick part is a field, WordArt is a
//! shape with no fill and large text in it, and the selection pane is a list of
//! the drawings in the document, which picks one out by name.

use wp_docx::shapes::Shape;
use wp_docx::TextPosition;
use wp_shell::Response;

use crate::chrome::findbar::{FindBar, Purpose};
use crate::chrome::{Choice, Command, Popup};

use super::Editor;

/// The fields a quick part can be, and what each is called.
///
/// Every one of them is worked out from the document itself, so it stays right
/// when the document changes. A field this program could not work out would be
/// a field showing a stale answer for ever, and there is no use in that.
const PARTS: &[(&str, &str)] = &[
    ("Author", "AUTHOR"),
    ("Title", "TITLE"),
    ("Subject", "SUBJECT"),
    ("Keywords", "KEYWORDS"),
    ("Company", "COMPANY"),
    ("Date created", "CREATEDATE"),
    ("Date last saved", "SAVEDATE"),
];

/// The looks WordArt comes in: the colour of the letters, and their outline.
const WORD_ART: &[(&str, &str, Option<&str>)] = &[
    ("Blue fill", "4472C4", None),
    ("Blue fill, dark outline", "4472C4", Some("2F528F")),
    ("Grey fill", "A5A5A5", None),
    ("Black fill", "000000", None),
    ("Gold fill", "FFC000", Some("7F6000")),
    ("Red fill", "C00000", Some("7F0000")),
    ("Green fill", "70AD47", Some("507E32")),
    ("White fill, black outline", "FFFFFF", Some("000000")),
];

/// How big WordArt is by default, in points.
const ART_SIZE: f64 = 40.0;

impl Editor {
    // --- Quick parts ----------------------------------------------------------

    /// Drops open what Word's Quick Parts button drops open: the pieces a
    /// person has saved, the two commands that manage them, and the document
    /// properties that can be put in as fields.
    pub(super) fn open_quick_parts(&mut self) -> Response {
        self.open_ribbon_menu(Choice::QuickPart)
    }

    /// What is on that menu, in Word's order.
    ///
    /// The saved pieces first, because that is what a person opens the menu
    /// for once they have saved any.
    pub(super) fn quick_part_menu(&self) -> Vec<String> {
        use crate::messages::t;
        let mut items: Vec<String> =
            self.own_blocks().into_iter().map(|block| block.name).collect();
        items.push(t("Save Selection to Quick Part Gallery").to_owned());
        items.push(t("Building Blocks Organizer").to_owned());
        items.extend(PARTS.iter().map(|(label, _)| (*label).to_owned()));
        items
    }

    /// Does whichever line was pressed.
    pub(super) fn choose_quick_part(&mut self, index: usize) -> Response {
        self.popup = None;
        let saved = self.own_blocks();

        // The saved pieces, then the two commands, then the fields.
        if let Some(block) = saved.get(index) {
            let name = block.name.clone();
            return self.insert_own_block(&name);
        }
        let past = index - saved.len();
        if past == 0 {
            return self.save_selection_as_block();
        }
        if past == 1 {
            return self.open_organizer();
        }
        let Some((label, instruction)) = PARTS.get(past - 2).copied() else {
            return Response::Ignored;
        };

        // What the field says now, so the document reads correctly before
        // anything works it out again.
        let properties = self.document.properties();
        let shown = match instruction {
            "AUTHOR" => properties.author,
            "TITLE" => properties.title,
            "SUBJECT" => properties.subject,
            "KEYWORDS" => properties.keywords,
            "COMPANY" => properties.company,
            "CREATEDATE" => day_of(&properties.created),
            "SAVEDATE" => day_of(&properties.modified),
            _ => String::new(),
        };
        let shown = if shown.trim().is_empty() { format!("[{label}]") } else { shown };

        let changed = self.document.insert_field(instruction, &shown);
        self.relayout();
        self.reveal_caret();
        self.edited(changed, &format!("{label} field"))
    }

    // --- WordArt --------------------------------------------------------------

    /// Drops open the looks WordArt comes in.
    pub(super) fn open_word_art(&mut self) -> Response {
        if self.close_popup_if(Choice::WordArt) {
            return Response::Redraw;
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::WordArt) else {
            return Response::Ignored;
        };
        let items = WORD_ART.iter().map(|(label, _, _)| (*label).to_owned()).collect();
        self.popup = Some(Popup::new(Choice::WordArt, items, None, left, top, 260.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Remembers the look and asks for the words.
    pub(super) fn choose_word_art(&mut self, index: usize) -> Response {
        self.popup = None;
        if WORD_ART.get(index).is_none() {
            return Response::Ignored;
        }
        self.word_art_style = index;

        let mut bar = FindBar::for_purpose(Purpose::WordArt);
        let selected = self.document.selected_text();
        if !selected.is_empty() && !selected.contains('\n') {
            bar.needle = selected;
        }
        self.find_bar = Some(bar);
        self.clamp_scroll();
        self.needs_redraw = true;
        self.report("Type the words, then press Enter")
    }

    /// Puts the WordArt in.
    pub(super) fn finish_word_art(&mut self) -> Response {
        let Some(bar) = &self.find_bar else { return Response::Ignored };
        let text = bar.needle.trim().to_owned();
        self.find_bar = None;
        self.needs_redraw = true;
        if text.is_empty() {
            return self.report("Nothing was typed, so nothing was added");
        }

        let Some((label, fill, outline)) = WORD_ART.get(self.word_art_style).copied() else {
            return Response::Ignored;
        };

        // WordArt is a shape with no fill and no line of its own — what is seen
        // is the letters, and they carry the colour.
        let width = ART_SIZE * 0.62 * text.chars().count().max(4) as f64;
        let mut shape = Shape::text_box(width, ART_SIZE * 1.8, &text);
        shape.name = "WordArt".to_owned();
        shape.fill = wp_docx::fills::Fill::None;
        shape.outline = None;
        shape.outline_emu = 0;
        for paragraph in &mut shape.text {
            paragraph.properties.alignment = Some(wp_docx::model::Alignment::Center);
            for run in &mut paragraph.runs {
                run.properties.bold = Some(true);
                run.properties.size_half_points = Some((ART_SIZE * 2.0) as u32);
                run.properties.color = Some(fill.to_owned());
            }
        }
        // The outline of the letters is not drawn yet, so a look that has one
        // is written with the darker colour rather than a lie about an edge.
        if let Some(outline) = outline {
            for paragraph in &mut shape.text {
                for run in &mut paragraph.runs {
                    if fill == "FFFFFF" {
                        run.properties.color = Some(outline.to_owned());
                    }
                }
            }
        }

        let changed = self.document.insert_shape(&shape);
        self.choose_drawing_here();
        self.relayout();
        self.reveal_caret();
        self.edited(changed, &format!("WordArt: {label}"))
    }

    // --- The selection pane ---------------------------------------------------

    /// Drops open the list of drawings in the document.
    pub(super) fn open_selection_pane(&mut self) -> Response {
        if self.close_popup_if(Choice::Drawing) {
            return Response::Redraw;
        }
        let drawings = self.drawings();
        if drawings.is_empty() {
            return self.report("This document has no shapes or pictures");
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::SelectionPane) else {
            return Response::Ignored;
        };

        let items = drawings.iter().map(|(_, name)| name.clone()).collect();
        self.popup = Some(Popup::new(Choice::Drawing, items, None, left, top, 260.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Chooses the drawing that was picked out of the list.
    ///
    /// Chosen, not merely gone to: the handles come up round it and every
    /// command in Arrange is then about it, which is the whole point of picking
    /// one by name. See [`super::handles`].
    pub(super) fn choose_drawing(&mut self, index: usize) -> Response {
        self.popup = None;
        let drawings = self.drawings();
        let Some((at, name)) = drawings.get(index).cloned() else { return Response::Ignored };

        self.choose_drawing_at(at);
        self.reveal_caret();
        self.report(&format!("Selected {name}"))
    }

    /// Every drawing in the document, with where it is and what to call it.
    ///
    /// Pictures as well as shapes, because Word's pane lists both and a pane
    /// that showed half of them would be a pane that could not reach the other
    /// half. They are listed in the order they are drawn, nearest the reader
    /// last, which is the order the pane shows them in.
    fn drawings(&self) -> Vec<(TextPosition, String)> {
        let mut out = Vec::new();
        for page in &self.pages {
            for drawing in page.drawings_under().into_iter().chain(page.drawings_over()) {
                let (at, name) = match drawing {
                    wp_layout::Drawing::Shape(shape) => (shape.at, shape.name.clone()),
                    wp_layout::Drawing::Picture(picture) => (picture.at, picture.name.clone()),
                };
                let Some(at) = at else { continue };
                out.push((at, name));
            }
        }
        out
    }
}

/// The day out of a timestamp.
fn day_of(stamp: &str) -> String {
    stamp.split('T').next().unwrap_or_default().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::Document;
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event};

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    /// A document with one shape and one picture in it.
    fn editor() -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("Words")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });

        editor.document.set_caret(TextPosition::new(0, 0));
        let shape = Shape { name: "Rectangle".to_owned(), ..Shape::preset("rect", 72.0, 72.0) };
        assert!(editor.document.insert_shape(&shape), "the shape went nowhere");

        let canvas = wp_raster::Canvas::filled(20, 20, wp_raster::Color::BLACK);
        let png = wp_raster::encode_png(&canvas);
        let end = editor.document.paragraph_text(0).map_or(0, |text| text.len());
        editor.document.set_caret(TextPosition::new(0, end));
        editor.document.insert_picture(&png, "png", 914_400, 914_400).expect("a picture");
        editor.relayout();
        editor
    }

    #[test]
    fn the_pane_lists_pictures_as_well_as_shapes() {
        let editor = editor();
        let names: Vec<String> = editor.drawings().into_iter().map(|(_, name)| name).collect();
        assert!(names.iter().any(|name| name == "Rectangle"), "no shape: {names:?}");
        assert!(names.len() == 2, "the picture is missing: {names:?}");
    }

    #[test]
    fn picking_one_out_of_the_pane_chooses_it() {
        let mut editor = editor();
        let at = editor.drawings().first().map(|(at, _)| *at).expect("a drawing");
        editor.choose_drawing(0);
        assert_eq!(editor.chosen_drawings, vec![at], "the pane did not choose it");
    }
}
