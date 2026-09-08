//! Quick parts, WordArt and the selection pane.
//!
//! Three small things from the Insert and Layout tabs that only became possible
//! once fields and shapes were there: a quick part is a field, WordArt is a
//! shape with no fill and large text in it, and the selection pane is a list of
//! the shapes on the page.

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

    /// Drops open the fields that can be dropped into the text.
    pub(super) fn open_quick_parts(&mut self) -> Response {
        if self.close_popup_if(Choice::QuickPart) {
            return Response::Redraw;
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::QuickParts) else {
            return Response::Ignored;
        };
        let items = PARTS.iter().map(|(label, _)| (*label).to_owned()).collect();
        self.popup = Some(Popup::new(Choice::QuickPart, items, None, left, top, 240.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Puts the field that was chosen at the caret.
    pub(super) fn choose_quick_part(&mut self, index: usize) -> Response {
        self.popup = None;
        let Some((label, instruction)) = PARTS.get(index).copied() else {
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
        shape.fill = None;
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

    /// Goes to the drawing that was chosen and selects it.
    pub(super) fn choose_drawing(&mut self, index: usize) -> Response {
        self.popup = None;
        let drawings = self.drawings();
        let Some((at, name)) = drawings.get(index).cloned() else { return Response::Ignored };

        self.document.set_caret(TextPosition::new(at.paragraph, at.offset + 1));
        self.reveal_caret();
        self.needs_redraw = true;
        self.report(&format!("Selected {name}"))
    }

    /// Every drawing in the document, with where it is and what to call it.
    fn drawings(&self) -> Vec<(TextPosition, String)> {
        let mut out = Vec::new();
        for page in &self.pages {
            for shape in &page.shapes {
                let Some(at) = shape.at else { continue };
                out.push((at, shape.name.clone()));
            }
        }
        out
    }
}

/// The day out of a timestamp.
fn day_of(stamp: &str) -> String {
    stamp.split('T').next().unwrap_or_default().to_owned()
}
