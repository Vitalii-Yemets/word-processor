//! Word's Design Mode, and the placeholder of a content control.
//!
//! # What a placeholder is
//!
//! A control put into a document holds the words that ask for an answer —
//! "Enter text", "Choose an item" — until somebody answers. Word draws
//! those grey, selects the whole of them when the control is clicked so
//! that typing replaces them, and marks the control as showing them
//! (`w:showingPlcHdr`) so that a program reading the file can tell a
//! question from an answer. All three are here: the grey is read into the
//! document ([`wp_docx::read`]), the click is [`Editor::used_a_control`],
//! and the mark comes off the moment the words change.
//!
//! # What Design Mode is
//!
//! The button on the Developer tab that turns the document into the form's
//! drawing board. Every control shows its tags — the start tag with the
//! control's title on it, the end tag after it — rather than the faint
//! brackets; and typing inside a control that is showing its placeholder
//! changes the placeholder itself, which stays a placeholder: that is how
//! the words that ask are written in the first place. Out of Design Mode
//! the same typing is an answer, and the mark comes off.
//!
//! The tags are drawn where the brackets are — see
//! [`Editor::draw_control_edges`] — and a block-level control's are drawn
//! at the start of its first paragraph and the end of its last. Word's tags
//! sit in the line and push the words along; these hang just above the
//! line and let the line above show through, because the layout does not
//! know about them and a tag drawn over the words would hide what it is
//! tagging.

use wp_docx::TextPosition;
use wp_raster::Color;
use wp_shell::Response;

use crate::messages::t;

use super::Editor;

/// How tall a tag is drawn, and the size of the words on it.
const TAG_HEIGHT: f32 = 11.0;
const TAG_TEXT: f32 = 6.0;

/// One tag, placed: where, how wide, and what it says.
struct Tag {
    x: f32,
    y: f32,
    width: f32,
    label: String,
    /// Whether it is the start tag, which carries the words, or the end.
    start: bool,
}

impl Editor {
    /// Word's Design Mode button: on, or off again.
    pub(super) fn toggle_design_mode(&mut self) -> Response {
        self.design_mode = !self.design_mode;
        self.needs_redraw = true;
        self.report(if self.design_mode {
            t("Design Mode is on: the tags show each control's title, and typing in a placeholder changes the placeholder")
        } else {
            t("Design Mode is off")
        })
    }

    /// What a control's start tag says: its title, or what kind it is.
    fn tag_label(alias: &str, kind: &str) -> String {
        if alias.trim().is_empty() {
            kind.to_owned()
        } else {
            alias.to_owned()
        }
    }

    /// Draws the tags of every control, in place of the brackets.
    pub(super) fn draw_design_tags(&mut self) {
        let controls = self.document.controls();
        let blocks = self.document.block_controls();
        if controls.is_empty() && blocks.is_empty() {
            return;
        }

        // Where each end is on the page, and what the start says.
        let mut ends: Vec<(TextPosition, bool, String)> = Vec::new();
        for control in &controls {
            let label = Self::tag_label(&control.alias, short_kind(control.kind.label()));
            ends.push((control.start, true, label));
            ends.push((control.end, false, String::new()));
        }
        for block in &blocks {
            let kind = match &block.kind {
                wp_docx::blockcontrols::BlockKind::RepeatingSection => "Repeating Section",
                wp_docx::blockcontrols::BlockKind::RepeatingItem => "Item",
                wp_docx::blockcontrols::BlockKind::Gallery { .. } => "Building Block Gallery",
                wp_docx::blockcontrols::BlockKind::Other => "Rich Text",
            };
            let label = Self::tag_label(&block.alias, kind);
            let last_end = self.document.paragraph_text(block.last).map_or(0, |text| text.len());
            ends.push((TextPosition::new(block.first, 0), true, label));
            ends.push((TextPosition::new(block.last, last_end), false, String::new()));
        }

        let mut tags: Vec<Tag> = Vec::new();
        for index in 0..self.pages.len() {
            let (origin_x, origin_y) = self.page_origin(index);
            let top = self.content_top() + origin_y - self.scroll_down();
            for (at, start, label) in &ends {
                // The start tag hangs over the first character; the end tag
                // over the end of the last, which is asked for by that
                // character rather than by the place after it.
                let (from, to) = if *start || at.offset == 0 {
                    (at.offset, at.offset + 1)
                } else {
                    (at.offset - 1, at.offset)
                };
                let rects = self.pages[index].selection_rects(
                    TextPosition::new(at.paragraph, from),
                    TextPosition::new(at.paragraph, to),
                );
                let (x, y, width, _) =
                    match if *start { rects.first().copied() } else { rects.last().copied() } {
                        Some(rect) => rect,
                        None => continue,
                    };
                let edge = if *start || at.offset == 0 { x } else { x + width };
                let measured = if *start {
                    self.chrome_engine.simple_line(label, 0.0, 0.0, TAG_TEXT, self.theme.text).width
                        + 8.0
                } else {
                    7.0
                };
                let tag_x = if *start { origin_x + edge } else { origin_x + edge - measured };
                tags.push(Tag {
                    x: tag_x,
                    y: top + y - TAG_HEIGHT + 1.0,
                    width: measured,
                    label: label.clone(),
                    start: *start,
                });
            }
        }

        // Translucent, so that the line above shows through where the tag
        // hangs over its descenders.
        let accent = self.theme.accent;
        let fill = Color::rgba(accent.red, accent.green, accent.blue, 70);
        let edge = Color::rgba(accent.red, accent.green, accent.blue, 200);
        let ink = self.theme.text;
        for tag in tags {
            let (x, y, width) = (tag.x as i32, tag.y as i32, tag.width as i32);
            self.canvas.fill_rect(x, y, width, TAG_HEIGHT as i32, fill);
            self.canvas.fill_rect(x, y, width, 1, edge);
            self.canvas.fill_rect(x, y + TAG_HEIGHT as i32 - 1, width, 1, edge);
            self.canvas.fill_rect(x, y, 1, TAG_HEIGHT as i32, edge);
            self.canvas.fill_rect(x + width - 1, y, 1, TAG_HEIGHT as i32, edge);
            if tag.start {
                let line = self.chrome_engine.simple_line(
                    &tag.label,
                    tag.x + 4.0,
                    tag.y + TAG_HEIGHT - 2.5,
                    TAG_TEXT,
                    ink,
                );
                self.renderer.draw_onto(&mut self.canvas, &line, 0.0, 0.0);
            } else {
                // The end tag carries a small mark rather than words.
                self.canvas.fill_rect(x + 3, y + TAG_HEIGHT as i32 / 2, width - 6, 1, ink);
            }
        }
    }

    /// A click in a control showing its placeholder selects the whole of
    /// it, so that typing replaces it — unless Design Mode is on, when the
    /// placeholder is what is being written.
    pub(super) fn select_placeholder(&mut self, at: TextPosition) -> Option<Response> {
        if self.design_mode {
            return None;
        }
        let control = self.document.control_at(at)?;
        if !control.placeholder || control.start == control.end {
            return None;
        }
        self.document.set_selections(&[(control.start, control.end)]);
        self.needs_redraw = true;
        Some(Response::Redraw)
    }

    /// After an event that may have edited: the control the caret is in
    /// stops showing its placeholder once its words have changed, unless
    /// Design Mode is on, when the words are the placeholder.
    pub(super) fn keep_placeholders(&mut self) {
        let caret = self.document.caret();
        let now = self.document.control_at(caret);
        let text = now.as_ref().map(|control| (control.start, self.document.control_text(control)));
        if let (Some(control), Some((start, held)), Some((was_start, was))) =
            (&now, &text, &self.last_control_text)
        {
            if control.placeholder && !self.design_mode && start == was_start && held != was {
                self.document.clear_placeholder(control.start);
                self.needs_redraw = true;
            }
        }
        self.last_control_text = text;
    }
}

/// The kind of a control as its tag says it: the label without the words
/// every kind shares.
fn short_kind(label: &str) -> &str {
    label.strip_suffix(" Content Control").unwrap_or(label)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_docx::controls::ControlKind;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::Document;
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event};

    use crate::chrome::Command;

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    fn editor() -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("Name: ")));
        let document = Document::create(&body).expect("a document");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor
    }

    /// A plain text control at the end of the paragraph, showing its
    /// placeholder.
    fn with_control(editor: &mut Editor) -> wp_docx::controls::Control {
        editor.document.set_caret(TextPosition::new(0, 6));
        assert!(editor.document.insert_control(ControlKind::PlainText, "Surname", &[]));
        let control = editor.document.controls().pop().expect("the control");
        assert!(control.placeholder, "a new control shows its placeholder");
        control
    }

    #[test]
    fn typing_over_the_placeholder_is_an_answer_and_the_control_stops_asking() {
        let mut editor = editor();
        let control = with_control(&mut editor);
        assert_eq!(editor.document.plain_text(), "Name: Enter surname");

        // The click selects the whole placeholder, and typing replaces it.
        let at = TextPosition::new(0, control.start.offset + 2);
        assert!(editor.used_a_control(at).is_some(), "the click was not the control's");
        assert_eq!(editor.document.selection(), Some((control.start, control.end)));
        for character in "Habgood".chars() {
            editor.handle(Event::Char(character));
        }
        assert_eq!(editor.document.plain_text(), "Name: Habgood");
        let control = editor.document.controls().pop().expect("the control");
        assert!(!control.placeholder, "it is still asking");

        // And a click in an answered control is the caret's, not the
        // control's.
        assert!(editor.used_a_control(at).is_none());
    }

    #[test]
    fn in_design_mode_typing_changes_the_placeholder_and_it_stays_one() {
        let mut editor = editor();
        let control = with_control(&mut editor);
        editor.run(Command::DesignMode);
        assert!(editor.design_mode);

        // No selecting of the placeholder: the caret goes where it was put.
        let at = TextPosition::new(0, control.end.offset);
        assert!(editor.used_a_control(at).is_none());
        editor.document.set_caret(at);
        editor.handle(Event::Char('!'));
        assert_eq!(editor.document.plain_text(), "Name: Enter surname!");
        let control = editor.document.controls().pop().expect("the control");
        assert!(control.placeholder, "the placeholder stopped being one");

        // The file says so, and the placeholder comes back grey.
        let again = Document::open(&editor.document.save().expect("saving")).expect("reopening");
        assert!(again.controls()[0].placeholder);
        let body = again.body();
        let Block::Paragraph(paragraph) = &body.blocks[0] else { panic!("not a paragraph") };
        let grey = paragraph
            .runs
            .iter()
            .find(|run| run.plain_text().contains("Enter"))
            .expect("the placeholder's run");
        assert_eq!(grey.properties.color.as_deref(), Some("808080"));

        editor.run(Command::DesignMode);
        assert!(!editor.design_mode);
    }

    #[test]
    fn the_tags_show_the_title_in_design_mode() {
        let mut editor = editor();
        with_control(&mut editor);
        editor.run(Command::DesignMode);
        editor.draw(1400, 900);
        assert_eq!(Editor::tag_label("Surname", "Plain Text"), "Surname");
        assert_eq!(Editor::tag_label("", "Plain Text"), "Plain Text");
        assert_eq!(short_kind(ControlKind::Date.label()), "Date Picker");
    }
}
