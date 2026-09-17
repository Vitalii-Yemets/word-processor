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

use wp_docx::blocks::{self, BuildingBlock, AUTO_TEXT, QUICK_PARTS};
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
const SORT_BY: usize = 3;
/// And in the part of it that refiles one.
const BLOCK_NAME: usize = 5;
const BLOCK_GALLERY: usize = 6;
const BLOCK_CATEGORY: usize = 7;
const BLOCK_DESCRIPTION: usize = 8;

/// The button that writes the boxes back on to the block that is chosen.
pub(super) const MODIFY: &str = "Modify";

/// What the organiser can be sorted by, which are the columns Word's has.
const COLUMNS: &[&str] = &["Name", "Gallery", "Category"];

/// Every gallery a block can be filed under: what the file calls it, and what
/// a person reads.
///
/// Word's own galleries are in here as well as the two a person fills, because
/// a block can get into one of them — saved from the page-number gallery, or
/// read out of a document Word wrote — and an organiser that could not name it
/// would show the block under a gallery it is not in, and refile it there the
/// moment anybody pressed Modify.
pub(crate) const GALLERIES: &[(&str, &str)] = &[
    (QUICK_PARTS, "Quick Parts"),
    (AUTO_TEXT, "AutoText"),
    (blocks::COVER_PAGES, "Cover Pages"),
    (blocks::PAGE_NUMBERS, "Page Numbers"),
    (blocks::PAGE_NUMBERS_TOP, "Page Numbers at the Top"),
    (blocks::PAGE_NUMBERS_BOTTOM, "Page Numbers at the Foot"),
    (blocks::PAGE_NUMBERS_MARGINS, "Page Numbers in the Margin"),
    (blocks::WATERMARKS, "Watermarks"),
    (blocks::HEADERS, "Headers"),
    (blocks::FOOTERS, "Footers"),
    (blocks::TABLES, "Tables"),
    (blocks::EQUATIONS, "Equations"),
    (blocks::TEXT_BOXES, "Text Boxes"),
    (blocks::CONTENTS, "Tables of Contents"),
    (blocks::BIBLIOGRAPHIES, "Bibliographies"),
];

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

    /// The blocks a person has saved to the AutoText gallery.
    ///
    /// Word's second gallery, which is the same machinery under another name
    /// and older than the first: what a person puts in it is the thing they
    /// type every day — a sign-off, an address, a paragraph of terms.
    pub(super) fn auto_text_blocks(&self) -> Vec<BuildingBlock> {
        self.own_template().map(|template| template.blocks_in(AUTO_TEXT)).unwrap_or_default()
    }

    /// Its menu: what is in it, and the way to put something in it.
    pub(super) fn auto_text_menu(&self) -> Vec<String> {
        let mut items: Vec<String> =
            self.auto_text_blocks().into_iter().map(|block| block.name).collect();
        items.push(crate::messages::t("Save Selection to AutoText Gallery").to_owned());
        items
    }

    /// Drops that menu open, under the button whose menu it came from.
    ///
    /// Word's AutoText is a submenu of Quick Parts, and this is that: the
    /// line on the first menu opens the second in the same place. It places
    /// itself because the button it hangs under already carries a menu, and
    /// one button cannot be looked up for two.
    pub(super) fn open_auto_text(&mut self) -> Response {
        if self.close_popup_if(crate::chrome::Choice::AutoText) {
            return Response::Redraw;
        }
        let Some((left, top, _)) = self.ribbon.command_rect(crate::chrome::Command::QuickParts)
        else {
            return Response::Ignored;
        };
        let items = self.auto_text_menu();
        let saved = self.auto_text_blocks().len();
        let rows: Vec<crate::chrome::popup::Row> = items
            .iter()
            .enumerate()
            .map(|(at, _)| {
                let icon = if at < saved {
                    crate::chrome::icons::Icon::QuickParts
                } else {
                    crate::chrome::icons::Icon::Save
                };
                crate::chrome::popup::Row::new(crate::chrome::popup::Kind::Choice, icon)
            })
            .collect();
        self.popup = Some(
            crate::chrome::Popup::new(
                crate::chrome::Choice::AutoText,
                items,
                None,
                left,
                top,
                300.0,
            )
            .with_rows(rows),
        );
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Puts in whichever was chosen, or saves the selection into the gallery.
    pub(super) fn choose_auto_text(&mut self, index: usize) -> Response {
        self.popup = None;
        let saved = self.auto_text_blocks();
        if let Some(block) = saved.get(index) {
            let name = block.name.clone();
            return self.insert_own_block(&name);
        }
        if index != saved.len() {
            return Response::Ignored;
        }
        self.save_selection_to(AUTO_TEXT)
    }

    /// Word's Save Selection to Quick Part Gallery: asks what to call it.
    pub(super) fn save_selection_as_block(&mut self) -> Response {
        self.save_selection_to(QUICK_PARTS)
    }

    /// The same question from whichever gallery asked it.
    ///
    /// Which gallery is part of the question rather than a detail of it: a
    /// person saving a design from the page-number gallery means it to be in
    /// the page-number gallery, and the dialog says so before they name it.
    pub(super) fn save_selection_to(&mut self, gallery: &'static str) -> Response {
        if self.document.selection().is_none() {
            return self.report("Select what to save first");
        }
        self.saving_to = gallery;
        let dialog = Dialog::new(
            "Create New Building Block",
            vec![
                Field::note(&crate::messages::with(
                    "The selection is saved to the {0} gallery.",
                    &[&gallery_name(gallery)],
                )),
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
        // Which gallery it goes to is whichever menu asked, so that Save
        // Selection to AutoText Gallery does not quietly put it under Quick
        // Parts.
        let block = BuildingBlock::named(name).in_gallery(self.saving_to);
        if !template.add_building_block(&block, &body) {
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
        self.organizer_dialog(0, 0)
    }

    /// Every block a person has saved, in whichever order was asked for.
    ///
    /// Word's organiser is a table whose headings sort it; this is a list
    /// with a box that says what to sort by, which is the same answer in the
    /// shape this program's dialogs have.
    fn sorted_blocks(&self, by: usize) -> Vec<BuildingBlock> {
        // Every gallery, not only the one the Quick Parts menu shows: the
        // organiser is where a person finds the piece they filed under the
        // wrong gallery, and one that hid those would be no use for that.
        let mut blocks =
            self.own_template().map(|template| template.building_blocks()).unwrap_or_default();
        blocks.sort_by_key(|block| {
            let key = match by {
                1 => &block.gallery,
                2 => &block.category,
                _ => &block.name,
            };
            key.to_lowercase()
        });
        blocks
    }

    /// The organiser, with one of the blocks chosen and one column sorted on.
    fn organizer_dialog(&mut self, by: usize, current: usize) -> Response {
        let blocks = self.sorted_blocks(by);
        if blocks.is_empty() {
            return self.report("Nothing has been saved as a building block yet");
        }
        let current = current.min(blocks.len() - 1);
        let chosen = blocks[current].clone();
        let rows = blocks
            .iter()
            .map(|block| {
                TreeRow::plain(&format!(
                    "{}  ({}, {})",
                    block.name,
                    gallery_name(&block.gallery),
                    block.category
                ))
            })
            .collect();

        let dialog = Dialog::with_buttons(
            "Building Blocks Organizer",
            vec![
                Field::note("Choose one to put it in, to refile it, or to take it away."),
                Field::Tree { label: "Building blocks".to_owned(), rows, current, scroll: 0 },
                Field::Heading("This one".to_owned()),
                Field::Choice {
                    label: "Sort by".to_owned(),
                    items: COLUMNS.iter().map(|name| crate::messages::t(name).to_owned()).collect(),
                    current: by,
                },
                Field::Text { label: "Name".to_owned(), value: chosen.name.clone() },
                Field::Choice {
                    label: "Gallery".to_owned(),
                    items: GALLERIES
                        .iter()
                        .map(|(_, shown)| crate::messages::t(shown).to_owned())
                        .collect(),
                    current: GALLERIES
                        .iter()
                        .position(|(name, _)| *name == chosen.gallery)
                        .unwrap_or_default(),
                },
                Field::Text { label: "Category".to_owned(), value: chosen.category.clone() },
                Field::Text { label: "Description".to_owned(), value: chosen.description.clone() },
            ],
            vec![
                Button { label: "Insert".to_owned(), answer: Answer::Accept, default: true },
                Button { label: MODIFY.to_owned(), answer: Answer::Named(MODIFY), default: false },
                Button { label: DELETE.to_owned(), answer: Answer::Named(DELETE), default: false },
                Button { label: "Cancel".to_owned(), answer: Answer::Cancel, default: false },
            ],
        )
        .wide(560.0);
        self.ask(Asking::Organizer, dialog)
    }

    /// Puts in whichever was chosen, or takes it away.
    pub(super) fn apply_organizer(&mut self, dialog: &Dialog, deleting: bool) -> Response {
        let Some(Field::Tree { current, .. }) = dialog.fields.get(BLOCK_ROWS) else {
            return Response::Ignored;
        };
        let blocks = self.sorted_blocks(dialog.chose(SORT_BY));
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

    /// The organiser's Modify and its Sort by, each of which changes the list
    /// and leaves the dialog standing.
    pub(super) fn organizer_button(&mut self, dialog: &Dialog, button: &str) -> Response {
        let by = dialog.chose(SORT_BY);
        let current = dialog.chose_row(BLOCK_ROWS);
        if button != MODIFY {
            return Response::Ignored;
        }

        let blocks = self.sorted_blocks(by);
        let Some(block) = blocks.get(current).cloned() else { return Response::Ignored };
        let wanted = BuildingBlock {
            name: dialog.said(BLOCK_NAME).trim().to_owned(),
            gallery: GALLERIES
                .get(dialog.chose(BLOCK_GALLERY))
                .map_or_else(|| QUICK_PARTS.to_owned(), |(name, _)| (*name).to_owned()),
            category: {
                let said = dialog.said(BLOCK_CATEGORY);
                let said = said.trim();
                if said.is_empty() {
                    wp_docx::blocks::GENERAL.to_owned()
                } else {
                    said.to_owned()
                }
            },
            description: dialog.said(BLOCK_DESCRIPTION).trim().to_owned(),
        };

        if wanted.name.is_empty() {
            self.status = crate::messages::t("A block has to have a name").to_owned();
            return self.organizer_dialog(by, current);
        }
        let named = block.name.clone();
        let done =
            self.change_own_template(move |template| template.edit_building_block(&named, &wanted));
        self.status = if done {
            crate::messages::with("{0} refiled", &[&block.name])
        } else {
            crate::messages::t("Another block is called that already").to_owned()
        };
        self.organizer_dialog(by, current)
    }
}

/// What a gallery is called where a person reads it.
///
/// The file says `quickParts` and `autoText`, which are names for a program;
/// Word shows Quick Parts and AutoText, and so does this.
fn gallery_name(gallery: &str) -> String {
    GALLERIES
        .iter()
        .find(|(name, _)| *name == gallery)
        .map_or_else(|| gallery.to_owned(), |(_, shown)| crate::messages::t(shown).to_owned())
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
        // Word's order: what is saved, the other gallery, the two commands,
        // then the document's own properties.
        assert_eq!(items[saved], "AutoText");
        assert_eq!(items[saved + 1], "Save Selection to Quick Part Gallery");
        assert_eq!(items[saved + 2], "Building Blocks Organizer");
        assert_eq!(items[saved + 3], "Author", "the fields come after the commands");
        assert_eq!(items.len(), saved + 3 + 7, "seven document properties");
    }

    #[test]
    fn the_autotext_menu_is_what_is_in_that_gallery_and_the_way_to_add_to_it() {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("one")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let library: &'static wp_layout::FontLibrary =
            Box::leak(Box::new(wp_layout::FontLibrary::scan_system()));
        let editor = crate::editor::Editor::new(library, document, None);

        let items = editor.auto_text_menu();
        let saved = editor.auto_text_blocks().len();
        assert_eq!(items.len(), saved + 1, "{items:?}");
        assert_eq!(items[saved], "Save Selection to AutoText Gallery");
    }

    #[test]
    fn a_gallery_is_shown_by_the_name_a_person_reads() {
        // The file says `quickParts`; Word says Quick Parts, and so does this.
        assert_eq!(gallery_name(QUICK_PARTS), "Quick Parts");
        assert_eq!(gallery_name(AUTO_TEXT), "AutoText");
        assert_eq!(gallery_name("somethingElse"), "somethingElse", "one Word does not name");
    }

    #[test]
    fn every_gallery_word_has_can_be_named_and_no_two_are_named_the_same() {
        // A block can reach any of them — saved from the page-number gallery,
        // or read out of a document Word wrote — and the organiser has to be
        // able to say which one it is in without refiling it.
        for (name, shown) in GALLERIES {
            assert_eq!(&gallery_name(name), &crate::messages::t(shown).to_owned());
        }
        for (at, (name, shown)) in GALLERIES.iter().enumerate() {
            for (other, other_shown) in &GALLERIES[at + 1..] {
                assert_ne!(name, other, "two galleries are filed under {name}");
                assert_ne!(shown, other_shown, "two galleries are shown as {shown}");
            }
        }
    }

    #[test]
    fn a_gallery_nobody_named_is_shown_as_whatever_the_file_called_it() {
        // Word has custom galleries as well, and a block in one of them is
        // better shown under a name nobody translated than hidden.
        assert_eq!(gallery_name("custom3"), "custom3");
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
