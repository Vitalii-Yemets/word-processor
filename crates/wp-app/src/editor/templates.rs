//! A document's template, and the styles it takes from it.
//!
//! # What Word does
//!
//! A document made from a template remembers it, and Developer ▸ Document
//! Template shows which in Word's Templates and Add-ins dialog: the path,
//! Attach… to point the document at another, and "Automatically update
//! document styles". Ticked, the document's styles are the template's: each
//! time the document is opened, and at once when the box is ticked and the
//! dialog shut — the template's every style over the document's own of the
//! same name, the rest of the document's left as they are.
//!
//! # What this does
//!
//! The same, with [`wp_docx::Document::update_styles_from`] doing the taking
//! over and `w:linkStyles` in the settings saying whether to.

use std::path::Path;

use wp_docx::Document;
use wp_shell::Response;

use crate::chrome::dialog::{Answer, Button, Dialog, Field};
use crate::messages::t;

use super::dialogs::Asking;
use super::Editor;

/// Word's button beside the template's path.
pub(super) const ATTACH: &str = "Attach...";

/// The rows of the dialog.
const TEMPLATE: usize = 1;
const UPDATE: usize = 2;

/// The types the Attach dialog offers: Word's templates.
const TEMPLATE_FILTERS: &[wp_shell::dialog::FileFilter] = &[
    wp_shell::dialog::FileFilter {
        label: "Word Templates (*.dotx;*.dotm;*.dot)",
        pattern: "*.dotx;*.dotm;*.dot",
    },
    wp_shell::dialog::FileFilter { label: "All files (*.*)", pattern: "*.*" },
];

/// A document with its template's styles taken over, if it says to take
/// them and its template can be read. Not counted as a change: the document
/// is as it would be opened any time.
pub(super) fn with_template_styles(mut document: Document) -> Document {
    if !document.links_styles() {
        return document;
    }
    let template = document
        .attached_template()
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| Document::open(&bytes).ok());
    if let Some(template) = template {
        let modified = document.is_modified();
        if document.update_styles_from(&template) > 0 && !modified {
            let _ = document.mark_saved();
        }
    }
    document
}

impl Editor {
    /// Word's Templates and Add-ins, off the Developer tab.
    pub(super) fn open_templates_dialog(&mut self) -> Response {
        let path = self.document.attached_template().unwrap_or_default();
        let dialog = self.templates_dialog(&path, self.document.links_styles());
        self.ask(Asking::Templates, dialog)
    }

    fn templates_dialog(&self, path: &str, update: bool) -> Dialog {
        let fields = vec![
            Field::Group("Document template".to_owned()),
            Field::Text { label: "Template".to_owned(), value: path.to_owned() },
            Field::Check { label: "Automatically update document styles".to_owned(), on: update },
        ];
        crate::chrome::dialog::check_rows(
            "Templates and Add-ins",
            &fields,
            &[(0, "a group"), (TEMPLATE, "a box"), (UPDATE, "a tick box")],
        );
        Dialog::with_buttons(
            "Templates and Add-ins",
            fields,
            vec![
                Button { label: "OK".to_owned(), answer: Answer::Accept, default: true },
                Button { label: ATTACH.to_owned(), answer: Answer::Named(ATTACH), default: false },
                Button { label: "Cancel".to_owned(), answer: Answer::Cancel, default: false },
            ],
        )
        .wide(520.0)
    }

    /// Attach…: a template chosen, and put in the box; the dialog stands.
    pub(super) fn attach_template_pressed(&mut self) -> Response {
        let dialog = self.dialog.clone();
        let update = dialog.as_ref().is_some_and(|dialog| dialog.ticked(UPDATE));
        let chosen = wp_shell::dialog::open_file(
            t("Attach Template"),
            &super::files::readable(TEMPLATE_FILTERS),
        );
        if let Some(path) = chosen {
            self.dialog = Some(self.templates_dialog(&path.display().to_string(), update));
        }
        self.needs_redraw = true;
        Response::Redraw
    }

    /// OK: the document points at the template in the box, says whether it
    /// takes its styles, and takes them now if it does.
    pub(super) fn apply_templates(&mut self, dialog: &Dialog) -> Response {
        let path = dialog.said(TEMPLATE).trim().to_owned();
        if !path.is_empty() && self.document.attached_template().as_deref() != Some(path.as_str()) {
            self.document.attach_template(&path);
        }
        let update = dialog.ticked(UPDATE);
        if update != self.document.links_styles() {
            self.document.set_links_styles(update);
        }
        if update && !path.is_empty() {
            match std::fs::read(Path::new(&path)).ok().and_then(|bytes| Document::open(&bytes).ok())
            {
                Some(template) => {
                    let changed = self.document.update_styles_from(&template);
                    self.status = crate::messages::with(
                        "{0} styles taken from the template",
                        &[&changed.to_string()],
                    );
                }
                None => {
                    self.status = crate::messages::with("Cannot read {0}", &[&path]);
                }
            }
        }
        self.relayout();
        self.needs_redraw = true;
        Response::Redraw
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_docx::kinds::Kind;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::styles::StyleDefinition;
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event};

    fn blank() -> Document {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("Hello")));
        Document::create(&body).expect("a document")
    }

    fn editor() -> Editor {
        let library: &'static FontLibrary = Box::leak(Box::new(FontLibrary::scan_system()));
        let mut editor = Editor::new(library, blank(), None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor
    }

    /// A template whose Heading 1 is red, saved where a document can point.
    fn red_template(name: &str) -> std::path::PathBuf {
        let folder = std::env::temp_dir().join(format!("wp-templates-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&folder);
        let mut template = blank();
        let mut heading =
            StyleDefinition::of(template.styles().get("Heading1").expect("Heading 1"));
        heading.run.color = Some("C00000".to_owned());
        assert!(template.set_style(&heading));
        template.set_kind(Kind::Template);
        let path = folder.join(name);
        std::fs::write(&path, template.save().expect("saved")).unwrap();
        path
    }

    fn red(document: &Document) -> bool {
        document.styles().get("Heading1").and_then(|s| s.run.color.as_deref()) == Some("C00000")
    }

    #[test]
    fn a_document_that_links_its_styles_takes_them_from_its_template_as_it_opens() {
        let template = red_template("red.dotx");
        let mut document = blank();
        document.attach_template(&template.display().to_string());
        document.set_links_styles(true);
        let path = template.with_file_name("linked.docx");
        std::fs::write(&path, document.save().expect("saved")).unwrap();
        // And one that does not.
        document.set_links_styles(false);
        let unlinked = template.with_file_name("unlinked.docx");
        std::fs::write(&unlinked, document.save().expect("saved")).unwrap();

        let mut editor = editor();
        editor.open_path(&path);
        assert!(red(&editor.document), "the template's styles were not taken");
        assert!(!editor.document.is_modified(), "opening is not a change");
        editor.open_path(&unlinked);
        assert!(!red(&editor.document), "a document that does not link took them");
        for file in [template, path, unlinked] {
            let _ = std::fs::remove_file(file);
        }
    }

    #[test]
    fn the_templates_dialog_attaches_and_takes_the_styles_at_once() {
        let template = red_template("dialog.dotx");
        let mut editor = editor();
        editor.run(crate::chrome::Command::DocumentTemplate);
        assert_eq!(editor.asking, Some(Asking::Templates));
        let dialog = editor.templates_dialog(&template.display().to_string(), true);
        editor.asking = None;
        editor.dialog = None;
        editor.apply_templates(&dialog);
        assert_eq!(
            editor.document.attached_template().as_deref(),
            Some(template.to_str().unwrap())
        );
        assert!(editor.document.links_styles());
        assert!(red(&editor.document));
        let _ = std::fs::remove_file(template);
    }
}
