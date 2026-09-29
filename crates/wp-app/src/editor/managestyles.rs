//! Word's Manage Styles: the defaults a new document starts from, and the
//! Organizer that carries a style from one file to another.
//!
//! # Two pages, and why they are one dialog
//!
//! Word's Manage Styles has four tabs and a button. Two of them are elsewhere
//! in this program already — Restrict is the style list inside Restrict
//! Editing (**J9**), and Edit is the Modify Style dialog — so what is left is
//! Set Defaults, and the Organizer its button opens. They are two tabs of one
//! dialog here rather than a dialog and a dialog it opens, because a dialog
//! that opens a dialog is a thing to shut twice.
//!
//! # What Set Defaults is for
//!
//! It is the third place Word offers Set as Default, and the one that says
//! plainly what the other two only imply: the font and the spacing a document
//! starts from, and a choice between this document and every document made
//! from the template. See [`super::defaults`], where that second half is.
//!
//! # What the Organizer is for
//!
//! A style lives in the document it was made in. The Organizer is how it gets
//! out: into `Normal.dotm`, and so into everything written afterwards, or out
//! of the template into a document that wants it. Word's dialog works out
//! which way round from which list was last clicked; this asks, because a
//! copy that goes the wrong way is a style overwritten in the wrong file.

use wp_docx::model::{ParagraphProperties, RunProperties};
use wp_docx::Document;
use wp_shell::Response;

use crate::chrome::dialog::{Answer, Button, Dialog, Field, TreeRow};

use super::dialogs::Asking;
use super::Editor;

/// Where each answer sits. The first tab.
const FONT: usize = 2;
const SIZE: usize = 3;
const SPACING: usize = 4;
const WHERE: usize = 5;
/// And the second.
const MINE: usize = 8;
const TEMPLATE: usize = 9;
const DIRECTION: usize = 10;
const NEW_NAME: usize = 11;

/// The buttons under the lists.
pub(super) const COPY: &str = "Copy";
pub(super) const DELETE: &str = "Delete";
pub(super) const RENAME: &str = "Rename";
/// Word's Manage Styles has an Edit tab with a Modify button on it; this is
/// that button, on the list that is here.
pub(super) const MODIFY: &str = "Modify";

impl Editor {
    /// Word's Manage Styles.
    pub(super) fn open_manage_styles(&mut self) -> Response {
        let resolved = self.document.styles().resolve_run(None, &RunProperties::default());
        let spacing = self.document.styles().document_paragraph_defaults().space_after.unwrap_or(0);
        let template = self.own_template();

        let dialog = Dialog::with_buttons(
            "Manage Styles",
            vec![
                Field::Tab("Set Defaults".to_owned()),
                Field::note("What a paragraph looks like before anything is applied to it."),
                Field::Text {
                    label: "Font".to_owned(),
                    value: resolved.font.clone().unwrap_or_default(),
                },
                Field::Number {
                    label: "Size".to_owned(),
                    value: crate::chrome::format_size(resolved.size_half_points as f32 / 2.0),
                    unit: "pt",
                },
                Field::Number {
                    label: "Space after".to_owned(),
                    value: crate::chrome::format_size(spacing as f32 / 20.0),
                    unit: "pt",
                },
                Field::Choice {
                    label: "Apply to".to_owned(),
                    items: vec![
                        crate::messages::t("This document only").to_owned(),
                        crate::messages::t("New documents based on this template").to_owned(),
                    ],
                    current: 0,
                },
                Field::Tab("Organizer".to_owned()),
                Field::Columns(2),
                Field::Tree {
                    label: "In this document".to_owned(),
                    rows: style_rows(&self.document),
                    current: 0,
                    scroll: 0,
                },
                Field::Tree {
                    label: "In Normal.dotm".to_owned(),
                    rows: template.as_ref().map(style_rows).unwrap_or_default(),
                    current: 0,
                    scroll: 0,
                },
                Field::Choice {
                    label: "Copy".to_owned(),
                    items: vec![
                        crate::messages::t("From this document to the template").to_owned(),
                        crate::messages::t("From the template to this document").to_owned(),
                    ],
                    current: 0,
                },
                Field::Text { label: "New name".to_owned(), value: String::new() },
            ],
            vec![
                Button { label: "OK".to_owned(), answer: Answer::Accept, default: true },
                Button { label: COPY.to_owned(), answer: Answer::Named(COPY), default: false },
                Button { label: DELETE.to_owned(), answer: Answer::Named(DELETE), default: false },
                Button { label: RENAME.to_owned(), answer: Answer::Named(RENAME), default: false },
                Button { label: MODIFY.to_owned(), answer: Answer::Named(MODIFY), default: false },
                Button { label: "Cancel".to_owned(), answer: Answer::Cancel, default: false },
            ],
        )
        .wide(640.0);
        self.ask(Asking::ManageStyles, dialog)
    }

    /// Applies the Set Defaults page.
    pub(super) fn apply_manage_styles(&mut self, dialog: &Dialog) -> Response {
        let font = dialog.said(FONT);
        let size = dialog.said(SIZE).parse::<f32>().ok();
        let spacing = dialog.said(SPACING).parse::<f32>().ok();
        let everywhere = dialog.chose(WHERE) == 1;

        let mut run = RunProperties::default();
        if !font.trim().is_empty() {
            run.font = Some(font.trim().to_owned());
        }
        if let Some(size) = size.filter(|points| *points > 0.0) {
            run.size_half_points = Some((size * 2.0).round() as u32);
        }
        let mut paragraph = ParagraphProperties::default();
        if let Some(points) = spacing.filter(|points| *points >= 0.0) {
            paragraph.space_after = Some((points * 20.0).round() as i32);
        }

        let mut changed = self.document.set_default_character_format(&run);
        changed |= self.document.set_default_paragraph_format(&paragraph);
        let kept =
            everywhere && (self.keep_default_font(&run) | self.keep_default_paragraph(&paragraph));

        self.relayout();
        let said = self.said_of_a_default("The defaults", kept);
        self.edited(changed, &said)
    }

    /// Copy, Delete or Rename under the Organizer's lists.
    pub(super) fn manage_styles_button(&mut self, dialog: &Dialog, button: &str) -> Response {
        // Which list is being worked on is what the direction says: copying
        // from this document acts on what is chosen in this document's list.
        let from_mine = dialog.chose(DIRECTION) == 0;
        let row = if from_mine { MINE } else { TEMPLATE };
        let Some(id) =
            dialog.tree_rows(row).get(dialog.chose_row(row)).map(|row| identifier(&row.text))
        else {
            return self.report("Choose a style first");
        };

        let done = match button {
            COPY => self.copy_style_across(&id, from_mine),
            DELETE => self.delete_style_from(&id, from_mine),
            RENAME => self.rename_style_in(&id, &dialog.said(NEW_NAME), from_mine),
            // Modify hands over to the dialog that changes a style, which is
            // where the work of changing one already lives.
            MODIFY => return self.open_modify_style_for(&id),
            _ => return Response::Ignored,
        };
        let said = match (button, done) {
            (COPY, true) => crate::messages::with("{0} copied", &[&id]),
            (DELETE, true) => crate::messages::with("{0} deleted", &[&id]),
            (RENAME, true) => crate::messages::with("{0} renamed", &[&id]),
            (RENAME, false) => crate::messages::t("Type the new name first").to_owned(),
            (_, false) => crate::messages::t("That style could not be changed").to_owned(),
            _ => String::new(),
        };
        self.status = said;
        self.relayout();
        // The lists have changed under the dialog, so it is asked again with
        // what is there now.
        self.open_manage_styles()
    }

    /// Copies one style to the other side.
    fn copy_style_across(&mut self, id: &str, from_mine: bool) -> bool {
        if from_mine {
            let mine = self.document.clone();
            let id = id.to_owned();
            return self.change_own_template(move |template| template.copy_style_from(&mine, &id));
        }
        let Some(template) = self.own_template() else { return false };
        self.document.copy_style_from(&template, id)
    }

    /// And takes one away from whichever side it is on.
    fn delete_style_from(&mut self, id: &str, from_mine: bool) -> bool {
        if from_mine {
            return self.document.delete_style(id);
        }
        let id = id.to_owned();
        self.change_own_template(move |template| template.delete_style(&id))
    }

    /// And renames one there.
    fn rename_style_in(&mut self, id: &str, name: &str, from_mine: bool) -> bool {
        if name.trim().is_empty() {
            return false;
        }
        if from_mine {
            return self.document.rename_style(id, name);
        }
        let (id, name) = (id.to_owned(), name.to_owned());
        self.change_own_template(move |template| template.rename_style(&id, &name))
    }
}

/// Every style of a document, as rows to pick from.
///
/// Shown by name with the identifier after it, because two styles may be
/// called the same thing and the identifier is what a copy goes by.
fn style_rows(document: &Document) -> Vec<TreeRow> {
    document
        .styles()
        .all()
        .iter()
        .map(|style| {
            let name = crate::names::shown(style.name.as_deref().unwrap_or(&style.id));
            TreeRow::plain(&format!("{name} ({})", style.id))
        })
        .collect()
}

/// The identifier out of a row written by [`style_rows`].
fn identifier(row: &str) -> String {
    row.rsplit_once('(')
        .map(|(_, rest)| rest.trim_end_matches(')').to_owned())
        .unwrap_or_else(|| row.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chrome::dialog::Answer;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event};

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    fn editor() -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("One")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor
    }

    #[test]
    fn the_dialog_shows_what_a_document_starts_from() {
        let mut editor = editor();
        editor.open_manage_styles();
        let dialog = editor.dialog.as_ref().expect("the dialog");
        assert_eq!(dialog.title, "Manage Styles");

        // The font the document actually resolves to, not an empty box.
        assert!(!dialog.said(FONT).trim().is_empty(), "the default font is not shown");
        assert!(dialog.said(SIZE).parse::<f32>().is_ok(), "{}", dialog.said(SIZE));
        assert_eq!(dialog.chose(WHERE), 0, "it offers to change every document by default");
    }

    #[test]
    fn set_defaults_writes_into_the_document() {
        let mut editor = editor();
        editor.open_manage_styles();
        let dialog = editor.dialog.as_mut().expect("the dialog");
        if let Some(Field::Text { value, .. }) = dialog.fields.get_mut(FONT) {
            *value = "Georgia".to_owned();
        }
        if let Some(Field::Number { value, .. }) = dialog.fields.get_mut(SIZE) {
            *value = "14".to_owned();
        }
        editor.finish_dialog(Answer::Accept);

        let resolved =
            editor.document.styles().resolve_run(None, &wp_docx::model::RunProperties::default());
        assert_eq!(resolved.font.as_deref(), Some("Georgia"));
        assert_eq!(resolved.size_half_points, 28, "the size did not arrive");
        assert!(editor.status.contains("this document"), "{}", editor.status);
    }

    #[test]
    fn the_organizer_lists_the_styles_of_both_files() {
        let mut editor = editor();
        editor.open_manage_styles();
        let dialog = editor.dialog.as_ref().expect("the dialog");

        let mine = dialog.tree_rows(MINE);
        assert!(!mine.is_empty(), "this document's styles are not listed");
        assert!(
            mine.iter().any(|row| row.text.contains("Heading1")),
            "{:?}",
            mine.iter().map(|row| row.text.clone()).collect::<Vec<_>>()
        );
        // The identifier is what a copy goes by, and it is what is read back
        // out of a row.
        assert_eq!(identifier("heading 1 (Heading1)"), "Heading1");
        assert_eq!(identifier("Normal"), "Normal", "a row with no identifier in it");
    }

    #[test]
    fn a_style_is_taken_out_of_the_document_it_is_in() {
        let mut editor = editor();
        assert!(editor.document.styles().get("Heading1").is_some());
        assert!(editor.delete_style_from("Heading1", true));
        assert!(
            editor.document.styles().get("Heading1").is_none(),
            "the style is still in the document"
        );
    }

    #[test]
    fn a_style_can_be_given_another_name_and_keeps_its_identifier() {
        let mut editor = editor();
        assert!(editor.rename_style_in("Heading1", "Chapter", true));

        let style = editor.document.styles().get("Heading1").expect("still there by identifier");
        assert_eq!(style.name.as_deref(), Some("Chapter"));
        // Everything that uses a style refers to it by identifier, so that is
        // what must not change.
        assert_eq!(style.id, "Heading1");

        assert!(!editor.rename_style_in("Heading1", "   ", true), "an empty name was taken");
    }

    #[test]
    fn a_style_that_is_not_there_is_not_copied_or_deleted() {
        let mut editor = editor();
        assert!(!editor.delete_style_from("NoSuchStyle", true));
        assert!(!editor.rename_style_in("NoSuchStyle", "Something", true));
    }
}
