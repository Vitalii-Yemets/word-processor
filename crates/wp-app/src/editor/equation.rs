//! Equations, from the Insert tab.
//!
//! One strip and one line: the linear format Word's own editor accepts, typed
//! in and turned into a built-up equation. `a/b` is a fraction, `x^2` a power,
//! `x_1` an index, `sqrt(x)` a root, and `\alpha` a Greek letter — made one as
//! soon as it is finished, by the Math AutoCorrect list, which is the strip's
//! and the settings' rather than the equation's: see `crate::autocorrect`.

use wp_docx::math;
use wp_shell::Response;

use crate::chrome::findbar::{FindBar, Purpose};

use super::Editor;

impl Editor {
    /// Asks for the equation.
    pub(super) fn start_equation(&mut self) -> Response {
        self.find_bar = Some(FindBar::for_purpose(Purpose::Equation));
        self.clamp_scroll();
        self.needs_redraw = true;
        self.report("Type the equation: a/b, x^2, x_1, sqrt(x), \\alpha")
    }

    /// Puts it in.
    pub(super) fn finish_equation(&mut self) -> Response {
        let Some(bar) = &self.find_bar else { return Response::Ignored };
        let typed = bar.needle.clone();
        self.find_bar = None;
        self.needs_redraw = true;

        let parsed = math::parse(&typed);
        if parsed.is_empty() {
            return self.report("Nothing was typed, so no equation was put in");
        }

        let changed = self.document.insert_equation(&parsed);
        self.relayout();
        self.reveal_caret();
        self.edited(changed, &format!("Equation: {}", parsed.plain_text()))
    }
}

#[cfg(test)]
mod tests {
    use crate::chrome::Command;
    use crate::editor::Editor;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::Document;
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event};

    fn editor() -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::default()));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let library: &'static FontLibrary = Box::leak(Box::new(FontLibrary::scan_system()));
        let mut editor = Editor::new(library, document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor
    }

    fn typed(editor: &Editor) -> String {
        editor.find_bar.as_ref().map(|bar| bar.needle.clone()).unwrap_or_default()
    }

    #[test]
    fn a_name_becomes_its_character_as_soon_as_it_is_finished() {
        let mut editor = editor();
        editor.run(Command::Equation);
        for character in "x = \\alpha+\\beta".chars() {
            editor.handle(Event::Char(character));
        }
        // `\alpha` was finished by the plus; `\beta` has not been finished yet
        // and could still become `\betaxyz`.
        assert_eq!(typed(&editor), "x = \u{3B1}+\\beta");
        editor.handle(Event::Char(' '));
        assert_eq!(typed(&editor), "x = \u{3B1}+\u{3B2} ");

        // A name that is on no list is left as it was typed, and a letter
        // after a name is part of the name.
        for character in "\\nosuch \\pix".chars() {
            editor.handle(Event::Char(character));
        }
        assert_eq!(typed(&editor), "x = \u{3B1}+\u{3B2} \\nosuch \\pix");

        editor.handle(Event::Char('\r'));
        assert!(editor.find_bar.is_none(), "the equation was not put in");
        assert!(editor.status.contains('\u{3B1}'), "{}", editor.status);
    }

    #[test]
    fn the_strip_leaves_names_alone_when_the_list_is_switched_off() {
        let mut editor = editor();
        editor.autocorrect.math_replace = false;
        editor.run(Command::Equation);
        for character in "\\alpha ".chars() {
            editor.handle(Event::Char(character));
        }
        assert_eq!(typed(&editor), "\\alpha ");
    }

    #[test]
    fn a_name_added_to_the_list_is_used_in_the_strip() {
        let mut editor = editor();
        editor.autocorrect.math.insert("\\ohm".to_owned(), "\u{2126}".to_owned());
        editor.run(Command::Equation);
        for character in "5\\ohm ".chars() {
            editor.handle(Event::Char(character));
        }
        assert_eq!(typed(&editor), "5\u{2126} ");
    }
}
