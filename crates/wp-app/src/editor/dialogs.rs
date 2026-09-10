//! The dialogs the program opens, and what it does with their answers.
//!
//! The machinery is [`crate::chrome::dialog`]; this is where the questions are
//! asked. A dialog is opened with the fields it needs, and when it is answered
//! the fields are read back and applied — which is why what each field means is
//! written down beside where it is read, and nowhere else.

use wp_shell::{Key, Response};

use crate::chrome::dialog::{Answer, Dialog, Field, Reaction};

use super::Editor;

/// Where the tick box sits in the Word Count dialog.
///
/// Named rather than written as a number twice: the row it is on is decided by
/// the list the dialog is built from, a few lines further down.
const EDGES: usize = 6;

/// Which question is being asked, so the answer can be acted on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Asking {
    /// How much there is of the document. Word's Word Count: a dialog that
    /// tells rather than asks.
    WordCount,
    /// The margins, the paper and the way round it goes.
    PageSetup,
    /// A name for this place in the document.
    Bookmark,
    /// Every character format there is: Word's Font dialog.
    Font,
}

impl Editor {
    /// Whether a dialog is up. While one is, it has the window.
    #[must_use]
    pub(super) fn in_dialog(&self) -> bool {
        self.dialog.is_some()
    }

    /// Opens a dialog, and remembers what it is asking.
    pub(super) fn ask(&mut self, asking: Asking, dialog: Dialog) -> Response {
        self.dialog = Some(dialog);
        self.asking = Some(asking);
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Shuts the dialog, acting on the answer if it was accepted.
    fn finish_dialog(&mut self, answer: Answer) -> Response {
        let Some(dialog) = self.dialog.take() else { return Response::Ignored };
        let asking = self.asking.take();
        self.needs_redraw = true;

        if answer == Answer::Cancel {
            return Response::Redraw;
        }
        match asking {
            // Nothing to take from a dialog that was telling rather than
            // asking.
            Some(Asking::PageSetup) => self.apply_page_setup(&dialog),
            Some(Asking::Bookmark) => self.apply_bookmark(&dialog),
            Some(Asking::Font) => self.apply_font_dialog(&dialog, answer == Answer::Other),
            Some(Asking::WordCount) | None => {
                let _ = dialog;
                Response::Redraw
            }
        }
    }

    /// A press while a dialog is up.
    pub(super) fn dialog_press(&mut self, x: i32, y: i32) -> Response {
        let Some(dialog) = &mut self.dialog else { return Response::Ignored };
        let reaction = dialog.press(x, y);
        self.reacted(reaction)
    }

    /// What to do about whatever the dialog made of the event.
    ///
    /// A dialog that shows what it is asking about — Word Count is the one —
    /// is built again when a field changes, because the numbers it is showing
    /// are the answer to that field.
    fn reacted(&mut self, reaction: Reaction) -> Response {
        match reaction {
            Reaction::Closed(answer) => self.finish_dialog(answer),
            Reaction::Changed => {
                match self.asking {
                    Some(Asking::WordCount) => {
                        self.count_the_edges =
                            self.dialog.as_ref().is_some_and(|dialog| dialog.ticked(EDGES));
                        self.dialog = Some(self.word_count_dialog(self.count_the_edges));
                    }
                    // The Font dialog's preview shows what its fields say, so
                    // the dialog is built again from what they now say — with
                    // the tab that was showing kept, or clicking a tab would
                    // put it straight back.
                    Some(Asking::Font) => {
                        if let Some(dialog) = self.dialog.take() {
                            let said = self.font_dialog_says(&dialog);
                            let mut built = self.font_dialog(&said);
                            built.carry_typing_from(&dialog);
                            self.dialog = Some(built);
                        }
                    }
                    _ => {}
                }
                self.needs_redraw = true;
                Response::Redraw
            }
            Reaction::Ignored => Response::Ignored,
        }
    }

    /// The pointer moving while a dialog is up.
    pub(super) fn dialog_hover(&mut self, x: i32, y: i32) -> bool {
        self.dialog.as_mut().is_some_and(|dialog| dialog.hover(x, y))
    }

    /// A key while a dialog is up.
    pub(super) fn dialog_key(&mut self, key: Key, shift: bool, control: bool) -> Response {
        let Some(dialog) = &mut self.dialog else { return Response::Ignored };
        let reaction = dialog.key(key, shift, control);
        self.reacted(reaction)
    }

    /// A character typed while a dialog is up.
    pub(super) fn dialog_character(&mut self, character: char) -> Response {
        let Some(dialog) = &mut self.dialog else { return Response::Ignored };
        let reaction = dialog.character(character);
        self.reacted(reaction)
    }

    /// Draws whatever dialog is up, over everything else.
    pub(super) fn draw_dialog(&mut self) {
        let Some(mut dialog) = self.dialog.take() else { return };
        let theme = self.theme;
        dialog.draw(&mut self.canvas, &mut self.chrome_engine, &mut self.renderer, &theme);
        self.dialog = Some(dialog);
    }

    /// Word's Word Count: how much of the document there is.
    ///
    /// The tick box at the bottom is Word's, and it does what Word's does: the
    /// counts are of the body alone, or of the body together with everything
    /// written round the edges of it.
    pub(super) fn open_word_count(&mut self) -> Response {
        let dialog = self.word_count_dialog(self.count_the_edges);
        self.ask(Asking::WordCount, dialog)
    }

    /// The text the counts are made of.
    ///
    /// Notes and text boxes are counted separately because Word counts them
    /// separately: they are not in the body, and a person asking how long the
    /// document is usually means the body.
    fn counted_text(&self, edges: bool) -> String {
        let mut text = self.document.plain_text();
        if !edges {
            return text;
        }
        for kind in [wp_docx::notes::Kind::Footnote, wp_docx::notes::Kind::Endnote] {
            for note in self.document.notes(kind) {
                text.push('\n');
                text.push_str(&note.text);
            }
        }
        for shape in self.document.shapes() {
            for paragraph in &shape.text {
                text.push('\n');
                text.push_str(&paragraph.plain_text());
            }
        }
        text
    }

    /// The dialog itself, built afresh whenever the tick box changes — which is
    /// how the numbers come to move when it is ticked.
    fn word_count_dialog(&self, edges: bool) -> Dialog {
        let text = self.counted_text(edges);
        let words = super::draw::count_words(&text);
        let characters = text.chars().count();
        let spaceless = text.chars().filter(|character| !character.is_whitespace()).count();
        let paragraphs = self.document.paragraph_count();
        let lines: usize = self.pages.iter().map(|page| page.lines.len()).sum();

        let said = |label: &str, value: String| Field::Said { label: label.to_owned(), value };
        Dialog::message(
            "Word Count",
            vec![
                said("Pages", self.pages.len().max(1).to_string()),
                said("Words", words.to_string()),
                said("Characters (no spaces)", spaceless.to_string()),
                said("Characters (with spaces)", characters.to_string()),
                said("Paragraphs", paragraphs.to_string()),
                said("Lines", lines.to_string()),
                Field::Check {
                    label: "Include textboxes, footnotes and endnotes".to_owned(),
                    on: edges,
                },
            ],
        )
    }

    /// Word's Page Setup: the margins typed rather than chosen, and the paper
    /// and the way round it goes.
    pub(super) fn open_page_setup(&mut self) -> Response {
        let (top, right, bottom, left) = self.document.page_margins();
        let inches = |twips: i32| {
            let value = f64::from(twips) / 1440.0;
            format!("{value:.2}")
        };

        let papers: Vec<String> =
            wp_docx::page::PAGE_SIZES.iter().map(|(name, ..)| (*name).to_owned()).collect();
        let paper = self
            .document
            .page_size_name()
            .and_then(|name| papers.iter().position(|found| found == name))
            .unwrap_or(0);

        let dialog = Dialog::new(
            "Page Setup",
            vec![
                Field::Heading("Margins".to_owned()),
                Field::Number { label: "Top".to_owned(), value: inches(top), unit: "\"" },
                Field::Number { label: "Bottom".to_owned(), value: inches(bottom), unit: "\"" },
                Field::Number { label: "Left".to_owned(), value: inches(left), unit: "\"" },
                Field::Number { label: "Right".to_owned(), value: inches(right), unit: "\"" },
                Field::Heading("Paper".to_owned()),
                Field::Choice { label: "Paper size".to_owned(), items: papers, current: paper },
                Field::Choice {
                    label: "Orientation".to_owned(),
                    items: vec!["Portrait".to_owned(), "Landscape".to_owned()],
                    current: usize::from(self.document.is_landscape()),
                },
            ],
        );
        self.ask(Asking::PageSetup, dialog)
    }

    /// Takes what the Page Setup dialog says and puts it on the document.
    fn apply_page_setup(&mut self, dialog: &Dialog) -> Response {
        let twips = |said: String| {
            let inches: f64 = said.trim().parse().unwrap_or(1.0);
            // Word's own limits: a margin cannot be negative, and cannot be so
            // large that there is no page left.
            (inches.clamp(0.0, 22.0) * 1440.0).round() as i32
        };
        let top = twips(dialog.said(1));
        let bottom = twips(dialog.said(2));
        let left = twips(dialog.said(3));
        let right = twips(dialog.said(4));

        let mut changed = self.document.set_page_margins(top, right, bottom, left);
        if let Some((_, width, height)) = wp_docx::page::PAGE_SIZES.get(dialog.chose(6)) {
            changed |= self.document.set_page_size(*width, *height);
        }
        changed |= self.document.set_landscape(dialog.chose(7) == 1);

        self.relayout();
        self.edited(changed, "Page setup")
    }
    /// Word's Bookmark dialog: a name for this place, and the names already in
    /// the document.
    pub(super) fn start_bookmark(&mut self) -> Response {
        let existing: Vec<String> =
            self.document.bookmarks().into_iter().map(|mark| mark.name).collect();
        let mut fields =
            vec![Field::Text { label: "Bookmark name".to_owned(), value: String::new() }];
        if !existing.is_empty() {
            fields.push(Field::Heading("Already in this document".to_owned()));
            for name in existing.iter().take(6) {
                fields.push(Field::Said { label: String::new(), value: name.clone() });
            }
        }
        self.ask(Asking::Bookmark, Dialog::new("Bookmark", fields))
    }

    /// Names the selection, or the caret.
    fn apply_bookmark(&mut self, dialog: &Dialog) -> Response {
        let wanted = dialog.said(0);
        let wanted = wanted.trim();
        if wanted.is_empty() {
            return Response::Redraw;
        }
        let name = wp_docx::bookmarks::sanitise_name(wanted);
        let changed = self.document.add_bookmark(&name);
        self.needs_redraw = true;
        self.edited(changed, &format!("Bookmark: {name}"))
    }
}

#[cfg(test)]
mod tests {
    use super::Asking;
    use crate::chrome::dialog::{Answer, Dialog, Field};
    use crate::editor::Editor;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::Document;
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event, Key, Modifiers};

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    fn editor() -> Editor {
        let mut body = Body::default();
        for index in 0..8 {
            body.blocks.push(Block::Paragraph(Paragraph::text(&format!("Paragraph {index}"))));
        }
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1200, height: 800 });
        editor
    }

    #[test]
    fn a_dialog_takes_the_keyboard_away_from_the_document() {
        let mut editor = editor();
        let before = editor.document.plain_text();
        editor.ask(
            Asking::Bookmark,
            Dialog::new(
                "Bookmark",
                vec![Field::Text { label: "Bookmark name".to_owned(), value: String::new() }],
            ),
        );

        // Typing goes into the dialog, not into the page behind it.
        for character in "Start".chars() {
            editor.handle(Event::Char(character));
        }
        assert_eq!(editor.document.plain_text(), before, "the document took the typing");
        assert_eq!(editor.dialog.as_ref().map(|dialog| dialog.said(0)), Some("Start".to_owned()));
    }

    #[test]
    fn a_click_on_the_page_behind_does_nothing_while_a_dialog_is_up() {
        let mut editor = editor();
        editor.open_word_count();
        let caret = editor.document.caret();

        // Well away from the panel, which is drawn in the middle.
        editor.handle(Event::MouseDown { x: 20, y: 700, modifiers: Modifiers::default() });
        assert!(editor.in_dialog(), "the dialog went away");
        assert_eq!(editor.document.caret(), caret, "the caret moved behind the dialog");
    }

    #[test]
    fn escape_puts_a_dialog_away_and_changes_nothing() {
        let mut editor = editor();
        let margins = editor.document.page_margins();
        editor.open_page_setup();
        assert!(editor.in_dialog());

        editor.handle(Event::KeyDown { key: Key::Escape, modifiers: Modifiers::default() });
        assert!(!editor.in_dialog(), "the dialog stayed up");
        assert_eq!(editor.document.page_margins(), margins, "cancelling changed the page");
    }

    #[test]
    fn page_setup_puts_what_was_typed_onto_the_document() {
        let mut editor = editor();
        editor.open_page_setup();

        // The top margin, typed afresh: Word takes inches, and stores twips.
        if let Some(dialog) = &mut editor.dialog {
            dialog.fields[1] =
                Field::Number { label: "Top".to_owned(), value: "2".to_owned(), unit: "\"" };
        }
        editor.handle(Event::KeyDown { key: Key::Enter, modifiers: Modifiers::default() });

        assert!(!editor.in_dialog());
        assert_eq!(editor.document.page_margins().0, 2880, "two inches is 2880 twips");
    }

    #[test]
    fn the_bookmark_dialog_names_the_place_the_caret_is_in() {
        let mut editor = editor();
        editor.start_bookmark();
        if let Some(dialog) = &mut editor.dialog {
            dialog.fields[0] =
                Field::Text { label: "Bookmark name".to_owned(), value: "Chapter One".to_owned() };
        }
        editor.handle(Event::KeyDown { key: Key::Enter, modifiers: Modifiers::default() });

        let names: Vec<String> =
            editor.document.bookmarks().into_iter().map(|mark| mark.name).collect();
        // Word puts an underscore where a space was, because a name with a
        // space in it is not one a field can refer to.
        assert_eq!(names, vec!["Chapter_One".to_owned()]);
    }

    #[test]
    fn word_count_tells_and_does_not_ask() {
        let mut editor = editor();
        editor.open_word_count();
        let dialog = editor.dialog.as_ref().expect("a dialog");
        assert_eq!(dialog.buttons.len(), 1, "a dialog that tells has one button");

        // Every count Word shows, in the order Word shows them.
        let labels: Vec<&str> = dialog.fields.iter().filter_map(Field::label).collect();
        assert_eq!(
            labels,
            vec![
                "Pages",
                "Words",
                "Characters (no spaces)",
                "Characters (with spaces)",
                "Paragraphs",
                "Lines",
            ]
        );
        assert_eq!(editor.finish_dialog(Answer::Accept), wp_shell::Response::Redraw);
        assert!(!editor.in_dialog());
    }
}
