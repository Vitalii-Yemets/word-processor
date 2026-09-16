//! A person's own building blocks, and the template they live in.
//!
//! # Why there is a file at all
//!
//! Because the point of saving a piece of a document under a name is to have
//! it in the *next* document. A block kept in the document it was saved from
//! would be no use whatever. So it goes into a template — the one Word calls
//! `Normal.dotm` and writes everything personal into — and every document
//! this program opens can reach it.
//!
//! The format of it all is [`wp_docx::blocks`]. What is here is where the
//! file is, when it is read, and what a person does with what is in it.
//!
//! # What is read when
//!
//! The template is read from disk each time it is wanted rather than held
//! open. It is small, it is asked about only when a menu drops, and holding
//! it open would mean two programs writing over each other's blocks — which
//! is what happens to `Normal.dotm` in Word and is nobody's favourite thing
//! about it.

use std::path::PathBuf;

use wp_docx::blocks::{BuildingBlock, QUICK_PARTS};
use wp_docx::Document;
use wp_shell::Response;

use crate::chrome::dialog::{Answer, Button, Dialog, Field, TreeRow};

use super::dialogs::Asking;
use super::Editor;

/// What the file a person's own blocks live in is called.
///
/// Word's name for it, and the same name on every platform: a person who
/// knows what `Normal.dotm` is should find it where they look.
pub(super) const NORMAL: &str = "Normal.dotm";

/// Where each answer sits in the organiser.
const BLOCK_ROWS: usize = 1;

/// The button that takes a block away rather than putting one in.
pub(super) const DELETE: &str = "Delete";

impl Editor {
    /// Where the person's own template is, whether or not it exists yet.
    pub(super) fn own_template_path(&self) -> Option<PathBuf> {
        super::backstage::personal_templates_folder().map(|folder| folder.join(NORMAL))
    }

    /// The person's own template, read afresh.
    ///
    /// An empty one where there is no file yet, so that the first block
    /// saved has somewhere to go.
    pub(super) fn own_template(&self) -> Option<Document> {
        let path = self.own_template_path()?;
        match std::fs::read(&path) {
            Ok(bytes) => Document::from_template(&bytes, path.to_str())
                .or_else(|_| Document::open(&bytes))
                .ok(),
            Err(_) => wp_docx::blocks::empty_template().ok(),
        }
    }

    /// Writes it back, making the folder if it is not there.
    pub(super) fn save_own_template(&mut self, template: &Document) -> bool {
        let Some(path) = self.own_template_path() else { return false };
        if let Some(folder) = path.parent() {
            let _ = std::fs::create_dir_all(folder);
        }
        match template.save() {
            Ok(bytes) => std::fs::write(&path, bytes).is_ok(),
            Err(_) => false,
        }
    }

    /// The blocks a person has saved, by name.
    pub(super) fn own_blocks(&self) -> Vec<BuildingBlock> {
        self.own_template().map(|template| template.blocks_in(QUICK_PARTS)).unwrap_or_default()
    }

    /// Word's Save Selection to Quick Part Gallery: asks what to call it.
    pub(super) fn save_selection_as_block(&mut self) -> Response {
        if self.document.selection().is_none() {
            return self.report("Select what to save first");
        }
        let dialog = Dialog::new(
            "Create New Building Block",
            vec![
                Field::note("The selection is saved under this name."),
                Field::Text { label: "Name".to_owned(), value: String::new() },
            ],
        );
        self.ask(Asking::NewBlock, dialog)
    }

    /// Saves it.
    pub(super) fn apply_new_block(&mut self, dialog: &Dialog) -> Response {
        let name = dialog.said(1);
        let name = name.trim();
        if name.is_empty() {
            return self.report("A building block needs a name");
        }
        let blocks = self.document.copy_selection();
        if blocks.is_empty() {
            return self.report("There is nothing selected to save");
        }

        let Some(mut template) = self.own_template() else {
            return self.report("There is nowhere to keep a building block on this machine");
        };
        let body = wp_docx::model::Body { blocks };
        if !template.add_building_block(&BuildingBlock::named(name), &body) {
            return self.report("It could not be saved");
        }
        if !self.save_own_template(&template) {
            return self.report("The template could not be written");
        }
        self.report(&format!("Saved as {name}"))
    }

    /// Puts a saved block in at the caret.
    pub(super) fn insert_own_block(&mut self, name: &str) -> Response {
        let Some(template) = self.own_template() else {
            return self.report("There is no template on this machine");
        };
        let put_in = self.document.insert_building_block(&template, name);
        self.relayout();
        self.reveal_caret();
        self.edited(put_in, &format!("{name} put in"))
    }

    /// Word's Building Blocks Organizer: everything saved, and a way to take
    /// one away.
    pub(super) fn open_organizer(&mut self) -> Response {
        let blocks = self.own_blocks();
        if blocks.is_empty() {
            return self.report("Nothing has been saved as a building block yet");
        }
        let rows = blocks
            .iter()
            .map(|block| TreeRow::plain(&format!("{}  ({})", block.name, block.category)))
            .collect();
        let dialog = Dialog::with_buttons(
            "Building Blocks Organizer",
            vec![
                Field::note("Choose one to put it in, or to take it away."),
                Field::Tree { label: "Building blocks".to_owned(), rows, current: 0, scroll: 0 },
            ],
            vec![
                Button { label: "Insert".to_owned(), answer: Answer::Accept, default: true },
                Button { label: DELETE.to_owned(), answer: Answer::Named(DELETE), default: false },
                Button { label: "Cancel".to_owned(), answer: Answer::Cancel, default: false },
            ],
        );
        self.ask(Asking::Organizer, dialog)
    }

    /// Puts in whichever was chosen, or takes it away.
    pub(super) fn apply_organizer(&mut self, dialog: &Dialog, deleting: bool) -> Response {
        let Some(Field::Tree { current, .. }) = dialog.fields.get(BLOCK_ROWS) else {
            return Response::Ignored;
        };
        let blocks = self.own_blocks();
        let Some(block) = blocks.get(*current) else { return Response::Ignored };
        let name = block.name.clone();

        if !deleting {
            return self.insert_own_block(&name);
        }
        let Some(mut template) = self.own_template() else { return Response::Ignored };
        if !template.remove_building_block(&name) {
            return self.report("It was already gone");
        }
        if !self.save_own_template(&template) {
            return self.report("The template could not be written");
        }
        self.report(&format!("{name} taken away"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_docx::model::{Block, Body, Paragraph};

    #[test]
    fn the_menu_is_the_saved_pieces_then_the_two_commands_then_the_fields() {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("one")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let library: &'static wp_layout::FontLibrary =
            Box::leak(Box::new(wp_layout::FontLibrary::scan_system()));
        let editor = crate::editor::Editor::new(library, document, None);

        let items = editor.quick_part_menu();
        let saved = editor.own_blocks().len();
        assert_eq!(items[saved], "Save Selection to Quick Part Gallery");
        assert_eq!(items[saved + 1], "Building Blocks Organizer");
        assert_eq!(items[saved + 2], "Author", "the fields come after the commands");
        assert_eq!(items.len(), saved + 2 + 7, "seven document properties");
    }

    #[test]
    fn the_template_is_called_what_word_calls_it() {
        assert_eq!(NORMAL, "Normal.dotm");
    }

    /// The template machinery without a machine: what a block saved into a
    /// template and read back out of it comes to.
    #[test]
    fn a_block_saved_into_a_template_comes_back_out_of_it() {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("nothing")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let mut template = Document::open(&bytes).expect("reopening");

        let mut saved = Body::default();
        saved.blocks.push(Block::Paragraph(Paragraph::text("Yours faithfully,")));
        assert!(template.add_building_block(&BuildingBlock::named("Signature"), &saved));

        let written = template.save().expect("saving the template");
        let reopened = Document::open(&written).expect("reopening the template");
        let names: Vec<String> =
            reopened.blocks_in(QUICK_PARTS).into_iter().map(|block| block.name).collect();
        assert_eq!(names, vec!["Signature".to_owned()]);
    }
}
