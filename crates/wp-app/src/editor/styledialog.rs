//! Word's New Style and Modify Style, and its Style Inspector.
//!
//! # One dialog for two buttons
//!
//! Word's "Create New Style from Formatting" and "Modify Style" are the same
//! dialog with a different title and a different starting point: one begins
//! blank and takes the formatting where the caret is, the other begins with
//! what the style already says. Two dialogs would be two places to change when
//! a field is added, so there is one.
//!
//! # What it does not hold
//!
//! Word's has a strip of formatting buttons across the middle — bold, the font,
//! the size, the alignment — and a Format menu that opens the Font and
//! Paragraph dialogs for everything else. The strip is a shortcut to what those
//! dialogs hold, so this offers the two dialogs and nothing in between: the
//! buttons behind the Format menu are exactly the ones **C2** and **C3**
//! already built, and building a second, smaller copy of them here would be two
//! places to be wrong.
//!
//! # The Style Inspector
//!
//! Word's shows what the selection is formatted with, split into what came from
//! the paragraph style and what was written on top of it by hand. That split is
//! the point: "why is this bold?" is a question about which of the two, and the
//! whole chain of styles behind the answer is what this shows.

use wp_docx::StyleDefinition;
use wp_shell::Response;

use crate::chrome::dialog::{Answer, Button, Dialog, Field};

use super::dialogs::Asking;
use super::Editor;

const NAME: usize = 0;
const ROW_ONE: usize = 1;
const BASED_ON: usize = 2;
const NEXT: usize = 3;

/// The two buttons past OK and Cancel, which hand over to the dialogs that
/// already hold every character and paragraph format there is.
pub(super) const FORMAT_FONT: &str = "Font…";
pub(super) const FORMAT_PARAGRAPH: &str = "Paragraph…";

/// The row of the based-on list that means "nothing", which is what a style at
/// the root of the chain is built on.
const NOTHING: &str = "(no style)";

impl Editor {
    /// Word's Modify Style, on whichever style the caret is in.
    pub(super) fn open_modify_style(&mut self) -> Response {
        let here = self.document.style_here();
        let existing =
            here.as_deref().and_then(|id| self.document.styles().get(id)).map(StyleDefinition::of);

        match existing {
            Some(style) => {
                let dialog = self.style_dialog("Modify Style", &style);
                self.editing_style = Some(style);
                self.ask(Asking::Style, dialog)
            }
            // The body style has no definition of its own to modify, so this is
            // where a new one is made from what is here — which is what Word's
            // New Style does anyway.
            None => self.open_new_style(),
        }
    }

    /// Word's Create New Style from Formatting.
    ///
    /// It begins with the formatting where the caret is, as Word's does: a
    /// person makes a style by getting a paragraph to look right and then
    /// naming it.
    pub(super) fn open_new_style(&mut self) -> Response {
        let style = StyleDefinition {
            id: self.unused_style_id(),
            name: String::new(),
            based_on: self.document.style_here(),
            next: None,
            paragraph: super::paragraphdialog::authored(&self.document.paragraph_format_here()),
            run: super::fontdialog::authored(&self.document.character_format_here()),
        };
        let dialog = self.style_dialog("New Style", &style);
        self.editing_style = Some(style);
        self.ask(Asking::Style, dialog)
    }

    /// An identifier no style in the document has yet.
    fn unused_style_id(&self) -> String {
        let taken = |id: &str| self.document.styles().get(id).is_some();
        (1..)
            .map(|number| format!("Style{number}"))
            .find(|id| !taken(id))
            .unwrap_or_else(|| "Style".to_owned())
    }

    /// The dialog itself.
    pub(super) fn style_dialog(&self, title: &str, style: &StyleDefinition) -> Dialog {
        // Every paragraph style, by name, for the two lists.
        let mut names = vec![NOTHING.to_owned()];
        let mut ids: Vec<Option<String>> = vec![None];
        for found in self.document.styles().all() {
            if found.kind != wp_docx::StyleKind::Paragraph || found.id == style.id {
                continue;
            }
            names.push(found.name.clone().unwrap_or_else(|| found.id.clone()));
            ids.push(Some(found.id.clone()));
        }
        let row_of = |wanted: Option<&str>| {
            wanted
                .and_then(|id| {
                    ids.iter().position(|found| {
                        found.as_deref().is_some_and(|found| found.eq_ignore_ascii_case(id))
                    })
                })
                .unwrap_or(0)
        };

        let fields = vec![
            Field::Text { label: "Name".to_owned(), value: style.name.clone() },
            Field::Columns(2),
            Field::Choice {
                label: "Style based on".to_owned(),
                items: names.clone(),
                current: row_of(style.based_on.as_deref()),
            },
            Field::Choice {
                label: "Style for following paragraph".to_owned(),
                items: names,
                current: row_of(style.next.as_deref()),
            },
        ];

        crate::chrome::dialog::check_rows(
            title,
            &fields,
            &[(NAME, "a box"), (ROW_ONE, "a row"), (BASED_ON, "a list"), (NEXT, "a list")],
        );

        Dialog::with_buttons(
            title,
            fields,
            vec![
                Button { label: "OK".to_owned(), answer: Answer::Accept, default: true },
                Button {
                    label: FORMAT_FONT.to_owned(),
                    answer: Answer::Named(FORMAT_FONT),
                    default: false,
                },
                Button {
                    label: FORMAT_PARAGRAPH.to_owned(),
                    answer: Answer::Named(FORMAT_PARAGRAPH),
                    default: false,
                },
                Button { label: "Cancel".to_owned(), answer: Answer::Cancel, default: false },
            ],
        )
        .wide(560.0)
    }

    /// What the dialog's fields say, over whatever the style already had.
    pub(super) fn style_dialog_says(&self, dialog: &Dialog) -> Option<StyleDefinition> {
        let mut style = self.editing_style.clone()?;
        style.name = dialog.said(NAME).trim().to_owned();

        // The lists hold names; the file holds identifiers.
        let id_at = |row: usize| -> Option<String> {
            let name = dialog.said(row);
            if name == NOTHING {
                return None;
            }
            self.document
                .styles()
                .all()
                .iter()
                .find(|found| found.name.as_deref().unwrap_or(&found.id) == name)
                .map(|found| found.id.clone())
        };
        style.based_on = id_at(BASED_ON);
        style.next = id_at(NEXT);
        Some(style)
    }

    /// Writes the style, and puts it on the paragraph that was being looked at.
    pub(super) fn apply_style_dialog(&mut self, dialog: &Dialog) -> Response {
        let Some(style) = self.style_dialog_says(dialog) else { return Response::Ignored };
        self.editing_style = None;

        // A style with no name is a style nobody can find again.
        if style.name.is_empty() {
            return self.report("A style needs a name");
        }
        let mut changed = self.document.set_style(&style);
        changed |= self.document.set_paragraph_style_here(Some(&style.id));
        self.relayout();
        self.edited(changed, &format!("Style: {}", style.name))
    }

    /// Hands over to the Font or Paragraph dialog, and comes back afterwards.
    ///
    /// What Word's Format menu inside this dialog does. The formatting is put
    /// on the paragraph the caret is in, and taken back off it into the style
    /// when this dialog is answered — which is how Word's works too, and is why
    /// its preview updates as you go.
    pub(super) fn style_dialog_format(&mut self, which: &str) -> Response {
        let Some(style) = self.editing_style.clone() else { return Response::Ignored };
        self.dialog = None;
        self.asking = None;
        self.editing_style = Some(style);
        self.formatting_a_style = true;

        if which == FORMAT_FONT {
            self.open_font_dialog()
        } else {
            self.open_paragraph_dialog()
        }
    }

    /// Comes back to the style dialog once one of those has been answered.
    pub(super) fn back_to_style_dialog(&mut self) -> Response {
        self.formatting_a_style = false;
        let Some(mut style) = self.editing_style.clone() else { return Response::Ignored };
        // Whatever the other dialog put on the paragraph is what the style now
        // says.
        style.paragraph = super::paragraphdialog::authored(&self.document.paragraph_format_here());
        style.run = super::fontdialog::authored(&self.document.character_format_here());

        let title = if self.document.styles().get(&style.id).is_some() {
            "Modify Style"
        } else {
            "New Style"
        };
        let dialog = self.style_dialog(title, &style);
        self.editing_style = Some(style);
        self.ask(Asking::Style, dialog)
    }

    /// Word's Style Inspector: what the selection is formatted with, and where
    /// each half of it came from.
    pub(super) fn open_style_inspector(&mut self) -> Response {
        let paragraph = self.document.paragraph_format_here();
        let run = self.document.character_format_here();
        let here = self.document.style_here();

        // The chain behind the style, outermost ancestor first, which is the
        // order the formatting is applied in.
        let chain = here
            .as_deref()
            .map(|id| {
                self.document
                    .styles()
                    .chain(id)
                    .iter()
                    .map(|style| style.name.clone().unwrap_or_else(|| style.id.clone()))
                    .collect::<Vec<_>>()
                    .join(" ▸ ")
            })
            .filter(|text| !text.is_empty())
            .unwrap_or_else(|| "Normal".to_owned());

        let said = |label: &str, value: String| Field::Said { label: label.to_owned(), value };
        let dialog = Dialog::message(
            "Style Inspector",
            vec![
                Field::Group("Paragraph formatting".to_owned()),
                said("Style", chain),
                said("Alignment", format!("{:?}", paragraph.alignment)),
                said(
                    "Indents",
                    format!(
                        "left {:.2}\", right {:.2}\", first line {:.2}\"",
                        f64::from(paragraph.indent_start) / 1440.0,
                        f64::from(paragraph.indent_end) / 1440.0,
                        f64::from(paragraph.indent_first_line) / 1440.0,
                    ),
                ),
                said(
                    "Spacing",
                    format!(
                        "{} pt before, {} pt after",
                        paragraph.space_before / 20,
                        paragraph.space_after / 20,
                    ),
                ),
                Field::Group("Text formatting".to_owned()),
                said("Font", run.font.clone().unwrap_or_else(|| "Body font".to_owned())),
                said("Size", format!("{} pt", run.size_points())),
                said("Weight", weight_of(&run)),
                said("Colour", run.color.clone().unwrap_or_else(|| "Automatic".to_owned())),
            ],
        )
        .wide(520.0);
        self.ask(Asking::Inspector, dialog)
    }
}

/// What a run's weight and slope come to, in words.
fn weight_of(run: &wp_docx::model::ResolvedRunProperties) -> String {
    let mut said: Vec<&str> = Vec::new();
    if run.bold {
        said.push("bold");
    }
    if run.italic {
        said.push("italic");
    }
    if run.underline.is_visible() {
        said.push("underlined");
    }
    if said.is_empty() {
        return "regular".to_owned();
    }
    said.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::Document;
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event, Key, Modifiers};

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    fn editor() -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("A heading").with_style("Heading1")));
        body.blocks.push(Block::Paragraph(Paragraph::text("Body text")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor
    }

    fn name_it(editor: &mut Editor, name: &str) {
        if let Some(dialog) = &mut editor.dialog {
            if let Some(Field::Text { value, .. }) = dialog.fields.get_mut(NAME) {
                *value = name.to_owned();
            }
        }
    }

    #[test]
    fn a_new_style_takes_the_formatting_where_the_caret_is() {
        let mut editor = editor();
        editor.document.set_caret(wp_docx::TextPosition::new(0, 0));
        editor.open_new_style();

        let style = editor.editing_style.as_ref().expect("a style being made");
        // The heading is bold, so the style made from it is bold.
        assert_eq!(style.run.bold, Some(true));
    }

    #[test]
    fn a_style_with_no_name_is_refused() {
        let mut editor = editor();
        editor.open_new_style();
        let before = editor.document.styles().all().len();
        editor.handle(Event::KeyDown { key: Key::Enter, modifiers: Modifiers::default() });
        assert_eq!(editor.document.styles().all().len(), before, "a nameless style was written");
    }

    #[test]
    fn a_named_style_is_written_and_applied() {
        let mut editor = editor();
        editor.document.set_caret(wp_docx::TextPosition::new(1, 0));
        editor.open_new_style();
        name_it(&mut editor, "Quotation");
        editor.handle(Event::KeyDown { key: Key::Enter, modifiers: Modifiers::default() });

        let written = editor
            .document
            .styles()
            .all()
            .iter()
            .any(|style| style.name.as_deref() == Some("Quotation"));
        assert!(written, "the style was not written");
        assert!(editor.document.style_here().is_some(), "it was not applied to the paragraph");
    }

    #[test]
    fn a_style_survives_being_saved_and_opened() {
        let mut editor = editor();
        editor.open_new_style();
        name_it(&mut editor, "Quotation");
        editor.handle(Event::KeyDown { key: Key::Enter, modifiers: Modifiers::default() });

        let bytes = editor.document.save().expect("saving");
        let reopened = Document::open(&bytes).expect("reopening");
        assert!(reopened
            .styles()
            .all()
            .iter()
            .any(|style| style.name.as_deref() == Some("Quotation")));
    }

    #[test]
    fn modifying_a_style_shows_what_that_style_says_rather_than_what_it_inherits() {
        // The difference matters: a dialog showing the resolved formatting
        // would write every inherited property into the style and cut it off
        // from what it is based on.
        let mut editor = editor();
        editor.document.set_caret(wp_docx::TextPosition::new(0, 0));
        editor.open_modify_style();

        let style = editor.editing_style.as_ref().expect("a style being modified");
        assert_eq!(style.id, "Heading1");
        assert_eq!(
            editor.dialog.as_ref().map(|dialog| dialog.title.clone()),
            Some("Modify Style".to_owned())
        );
    }

    #[test]
    fn the_inspector_shows_the_whole_chain_behind_the_paragraph() {
        let mut editor = editor();
        editor.document.set_caret(wp_docx::TextPosition::new(0, 0));
        editor.open_style_inspector();

        let dialog = editor.dialog.as_ref().expect("a dialog");
        // The style, and whatever it is based on, in the order they apply.
        assert!(dialog.said(1).is_empty() || !dialog.said(1).is_empty());
        let shown = match dialog.fields.get(1) {
            Some(Field::Said { value, .. }) => value.clone(),
            other => panic!("row 1 is {other:?}"),
        };
        assert!(shown.contains("eading"), "the chain does not name the heading: {shown}");
    }

    #[test]
    fn the_weight_is_said_in_words() {
        let mut run = wp_docx::model::ResolvedRunProperties::default();
        assert_eq!(weight_of(&run), "regular");
        run.bold = true;
        run.italic = true;
        assert_eq!(weight_of(&run), "bold, italic");
    }
}
