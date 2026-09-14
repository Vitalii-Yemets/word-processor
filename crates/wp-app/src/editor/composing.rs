//! Text an input method is composing, shown in the document before it is
//! committed.
//!
//! Chinese, Japanese and Korean are not typed a character at a time: a
//! person types sounds, the input method offers what they might mean, and
//! only what is chosen goes into the document. Until then the sounds — and
//! then the conversion being weighed — are shown where the text will go,
//! underlined so that they can be told from text that is there, with the
//! caret inside them. Word shows the composition inline in exactly this
//! way, and puts the input method's list of candidates beside the caret.
//! Every change of the composition replaces the last; committing puts the
//! chosen text in as if it had been typed; ending without committing takes
//! it out again. All of it is one undo step, since none of it was the
//! person's text until it was committed.

use wp_docx::TextPosition;
use wp_shell::{CompositionAttribute, Response};

use super::Editor;

/// What is being composed, and where it is in the document.
#[derive(Clone, Debug)]
pub(super) struct Composition {
    /// Where the composed text starts.
    pub(super) start: TextPosition,
    /// The composed text as it stands in the paragraph.
    pub(super) text: String,
    /// How each character of it stands, one per character.
    pub(super) attributes: Vec<CompositionAttribute>,
}

impl Editor {
    /// Whether an input method has text in the document that is not yet
    /// committed.
    pub(super) fn is_composing(&self) -> bool {
        self.composition.is_some()
    }

    /// The composition changed: what is shown is replaced by what it is
    /// now, and the caret goes where the input method says.
    pub(super) fn compose(
        &mut self,
        text: String,
        caret: usize,
        attributes: Vec<CompositionAttribute>,
    ) -> Response {
        if self.is_locked() {
            return self.refuse_locked();
        }
        if self.composition.is_none() {
            // The composition takes the place of whatever was selected, the
            // way the first typed character would.
            self.hide_mini_bar();
            self.forget_paste();
            self.drop_chosen_drawing();
            self.document.begin_gesture();
            self.document.delete_selection();
            self.composition = Some(Composition {
                start: self.document.caret(),
                text: String::new(),
                attributes: Vec::new(),
            });
        }
        self.remove_composed_text();
        let start = self.composition.as_ref().map_or_else(|| self.document.caret(), |c| c.start);
        self.document.set_caret(start);
        self.document.clear_selection();
        if !text.is_empty() {
            self.document.type_text(&text);
        }
        let offset: usize = text.chars().take(caret).map(char::len_utf8).sum();
        self.document.set_caret(TextPosition::new(start.paragraph, start.offset + offset));
        self.document.clear_selection();
        if let Some(composition) = self.composition.as_mut() {
            composition.text = text;
            composition.attributes = attributes;
        }
        self.relayout();
        self.reveal_caret();
        self.needs_redraw = true;
        Response::Redraw
    }

    /// The input method finished a piece of text: it goes in as typed, in
    /// place of whatever was being composed.
    pub(super) fn commit_composition(&mut self, text: String) -> Response {
        if self.is_locked() {
            return self.refuse_locked();
        }
        let was_composing = self.composition.is_some();
        if was_composing {
            self.remove_composed_text();
            let start =
                self.composition.as_ref().map_or_else(|| self.document.caret(), |c| c.start);
            self.document.set_caret(start);
            self.document.clear_selection();
        } else {
            self.hide_mini_bar();
            self.forget_paste();
            self.drop_chosen_drawing();
            self.document.begin_gesture();
            self.document.delete_selection();
        }
        // Typed one character at a time, so that a macro being recorded
        // sees it and the text takes the formatting typing takes.
        for character in text.chars() {
            self.record_typing(character);
        }
        let changed = self.document.type_text(&text);
        self.composition = None;
        self.document.end_gesture();
        self.recheck_proofing();
        self.edited(changed, "")
    }

    /// The composition is over. Anything still shown was not committed, and
    /// comes out.
    pub(super) fn end_composition(&mut self) -> Response {
        if self.composition.is_none() {
            return Response::Ignored;
        }
        self.remove_composed_text();
        let start = self.composition.take().map_or_else(|| self.document.caret(), |c| c.start);
        self.document.set_caret(start);
        self.document.clear_selection();
        self.document.end_gesture();
        self.relayout();
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Takes the composed text out of the paragraph, leaving the caret at
    /// its start.
    fn remove_composed_text(&mut self) {
        let Some(composition) = self.composition.as_ref() else { return };
        if composition.text.is_empty() {
            return;
        }
        let start = composition.start;
        let end = TextPosition::new(start.paragraph, start.offset + composition.text.len());
        self.document.set_caret(start);
        self.document.extend_selection_to(end);
        self.document.delete_selection();
        self.document.set_caret(start);
        self.document.clear_selection();
    }

    /// Draws the lines under the composition: dotted under what is still
    /// being typed, thin under what the input method has converted, thick
    /// under the clause whose conversion is being chosen, and in the
    /// colour of a mistake under what it could not convert.
    pub(super) fn draw_composition_marks(&mut self) {
        let Some(composition) = self.composition.clone() else { return };
        if composition.text.is_empty() {
            return;
        }
        // The runs of one attribute, as byte ranges of the paragraph.
        let mut runs: Vec<(usize, usize, CompositionAttribute)> = Vec::new();
        let mut offset = composition.start.offset;
        for (index, character) in composition.text.chars().enumerate() {
            let attribute =
                composition.attributes.get(index).copied().unwrap_or(CompositionAttribute::Input);
            let end = offset + character.len_utf8();
            match runs.last_mut() {
                Some((_, last_end, last)) if *last == attribute && *last_end == offset => {
                    *last_end = end;
                }
                _ => runs.push((offset, end, attribute)),
            }
            offset = end;
        }
        let mut marks: Vec<(f32, f32, f32, CompositionAttribute)> = Vec::new();
        for index in 0..self.pages.len() {
            let (origin_x, origin_y) = self.page_origin(index);
            let top = self.content_top() + origin_y - self.scroll_down();
            if top > self.view_height as f32 || top + self.pages[index].height < 0.0 {
                continue;
            }
            for &(start, end, attribute) in &runs {
                let from = TextPosition::new(composition.start.paragraph, start);
                let to = TextPosition::new(composition.start.paragraph, end);
                for (x, y, width, height) in self.pages[index].selection_rects(from, to) {
                    marks.push((origin_x + x, top + y + height - 1.0, width, attribute));
                }
            }
        }
        for (x, y, width, attribute) in marks {
            if width <= 0.0 {
                continue;
            }
            let (x, y, width) = (x as i32, y as i32, width.ceil() as i32);
            match attribute {
                CompositionAttribute::Input => {
                    let mut at = x;
                    while at < x + width {
                        self.canvas.fill_rect(at, y, 1, 1, self.theme.page_text);
                        at += 2;
                    }
                }
                CompositionAttribute::Converted => {
                    self.canvas.fill_rect(x, y, width, 1, self.theme.page_text);
                }
                CompositionAttribute::Target => {
                    self.canvas.fill_rect(x, y - 1, width, 2, self.theme.page_text);
                }
                CompositionAttribute::Error => {
                    self.canvas.fill_rect(x, y, width, 1, self.theme.danger);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::Document;
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event};

    use super::*;

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    fn editor(text: &str) -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::default()));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor.document.type_text(text);
        editor
    }

    fn compose(editor: &mut Editor, text: &str) -> Response {
        let attributes = vec![CompositionAttribute::Input; text.chars().count()];
        editor.handle(Event::Compose {
            text: text.to_owned(),
            caret: text.chars().count(),
            attributes,
        })
    }

    #[test]
    fn a_composition_is_shown_and_replaced_and_committed_as_one_edit() {
        let mut editor = editor("Say ");
        assert!(!editor.is_composing());
        compose(&mut editor, "");
        assert!(editor.is_composing());
        compose(&mut editor, "に");
        assert_eq!(editor.document.plain_text().trim_end_matches('\n'), "Say に");
        compose(&mut editor, "にほ");
        assert_eq!(editor.document.plain_text().trim_end_matches('\n'), "Say にほ");
        compose(&mut editor, "日本");
        assert_eq!(editor.document.plain_text().trim_end_matches('\n'), "Say 日本");
        assert_eq!(editor.document.caret(), TextPosition::new(0, "Say 日本".len()));
        editor.handle(Event::Commit("日本語".to_owned()));
        editor.handle(Event::ComposeEnd);
        assert!(!editor.is_composing());
        assert_eq!(editor.document.plain_text().trim_end_matches('\n'), "Say 日本語");
        // One undo takes the whole composition out, as one thing typed.
        assert!(editor.document.undo());
        assert_eq!(editor.document.plain_text().trim_end_matches('\n'), "Say ");
    }

    #[test]
    fn a_composition_given_up_leaves_nothing_behind() {
        let mut editor = editor("Say ");
        compose(&mut editor, "");
        compose(&mut editor, "kan");
        assert_eq!(editor.document.plain_text().trim_end_matches('\n'), "Say kan");
        editor.handle(Event::ComposeEnd);
        assert_eq!(editor.document.plain_text().trim_end_matches('\n'), "Say ");
        assert!(!editor.is_composing());
    }

    #[test]
    fn the_caret_sits_where_the_input_method_puts_it() {
        let mut editor = editor("");
        compose(&mut editor, "");
        let attributes = vec![CompositionAttribute::Converted; 3];
        editor.handle(Event::Compose { text: "한국어".to_owned(), caret: 1, attributes });
        assert_eq!(editor.document.caret(), TextPosition::new(0, "한".len()));
    }

    #[test]
    fn a_composition_takes_the_place_of_the_selection() {
        let mut editor = editor("old text");
        editor.document.set_caret(TextPosition::new(0, 0));
        editor.document.extend_selection_to(TextPosition::new(0, 3));
        compose(&mut editor, "");
        compose(&mut editor, "新");
        editor.handle(Event::Commit("新".to_owned()));
        editor.handle(Event::ComposeEnd);
        assert_eq!(editor.document.plain_text().trim_end_matches('\n'), "新 text");
    }

    #[test]
    fn a_commit_with_nothing_composed_is_typing() {
        let mut editor = editor("a");
        editor.handle(Event::Commit("bc".to_owned()));
        assert_eq!(editor.document.plain_text().trim_end_matches('\n'), "abc");
    }
}
