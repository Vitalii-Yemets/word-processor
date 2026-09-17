//! What the document says about itself, and the cover page built from it.
//!
//! # Why a list and not a form
//!
//! Word shows the properties as a panel of boxes. This program draws every
//! control it has, and a panel of ten boxes is ten controls that exist nowhere
//! else in it. A list of what the document says, where picking a line opens the
//! strip that is already used for typing one line of anything, is the same job
//! done with what is here — and it shows the current values without being
//! opened, which the panel does not.

use wp_docx::cover::{Cover, Layout};
use wp_docx::properties::Properties;
use wp_shell::Response;

use crate::chrome::findbar::{FindBar, Purpose};
use crate::chrome::icons::Icon;
use crate::chrome::popup::{Kind, Row};
use crate::chrome::{Choice, Command, Popup};

use super::Editor;

/// Which property a line of the list is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Field {
    Title,
    Subject,
    Author,
    Keywords,
    Description,
    Category,
    Company,
}

impl Field {
    /// What the property is called, as Word calls it.
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Title => "Title",
            Self::Subject => "Subject",
            Self::Author => "Author",
            Self::Keywords => "Tags",
            Self::Description => "Comments",
            Self::Category => "Category",
            Self::Company => "Company",
        }
    }

    /// Reads it out of the properties.
    pub(super) fn read(self, properties: &Properties) -> String {
        match self {
            Self::Title => properties.title.clone(),
            Self::Subject => properties.subject.clone(),
            Self::Author => properties.author.clone(),
            Self::Keywords => properties.keywords.clone(),
            Self::Description => properties.description.clone(),
            Self::Category => properties.category.clone(),
            Self::Company => properties.company.clone(),
        }
    }

    /// Writes it into them.
    fn write(self, properties: &mut Properties, value: String) {
        match self {
            Self::Title => properties.title = value,
            Self::Subject => properties.subject = value,
            Self::Author => properties.author = value,
            Self::Keywords => properties.keywords = value,
            Self::Description => properties.description = value,
            Self::Category => properties.category = value,
            Self::Company => properties.company = value,
        }
    }

    /// The list, in the order Word's panel puts them.
    pub(super) const ALL: &'static [Self] = &[
        Self::Title,
        Self::Subject,
        Self::Author,
        Self::Keywords,
        Self::Description,
        Self::Category,
        Self::Company,
    ];
}

impl Editor {
    /// Shows what the document says about itself, on the page that is about
    /// the document.
    ///
    /// Word has one place for this and it is the Info page of the File tab.
    /// Anything else that asks for the properties — the Insert tab's cover
    /// page, a button that says Properties — opens that.
    pub(super) fn open_properties(&mut self) -> Response {
        self.open_backstage(crate::chrome::backstage::Place::Info)
    }

    /// Opens the strip that takes a new value for the property chosen.
    pub(super) fn choose_property(&mut self, index: usize) -> Response {
        self.popup = None;
        let Some(field) = Field::ALL.get(index).copied() else { return Response::Ignored };

        let mut bar = FindBar::for_purpose(Purpose::Property);
        bar.needle = field.read(&self.document.properties());
        self.find_bar = Some(bar);
        self.editing_property = Some(field);
        self.clamp_scroll();
        self.needs_redraw = true;
        self.report(&format!("Type the {}, then press Enter", field.label().to_lowercase()))
    }

    /// Writes what was typed into the property that was chosen.
    pub(super) fn finish_property(&mut self) -> Response {
        let Some(bar) = &self.find_bar else { return Response::Ignored };
        let value = bar.needle.trim().to_owned();
        let Some(field) = self.editing_property.take() else { return self.close_find() };
        self.find_bar = None;
        self.needs_redraw = true;

        let mut properties = self.document.properties();
        field.write(&mut properties, value.clone());
        match self.document.set_properties(&properties) {
            Ok(changed) => {
                self.update_title();
                let note = if value.is_empty() {
                    format!("{} cleared", field.label())
                } else {
                    format!("{}: {value}", field.label())
                };
                self.edited(changed, &note)
            }
            Err(error) => self.report(&format!("The properties could not be saved: {error}")),
        }
    }

    /// Drops open the cover page arrangements.
    pub(super) fn open_cover_pages(&mut self) -> Response {
        if self.popup.as_ref().is_some_and(|popup| popup.choice == Choice::Cover) {
            self.popup = None;
            self.needs_redraw = true;
            return Response::Redraw;
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::CoverPage) else {
            return Response::Ignored;
        };

        let (items, rows) = self.cover_gallery();
        self.popup = Some(Popup::new(Choice::Cover, items, None, left, top, 260.0).with_rows(rows));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// What is in that gallery.
    ///
    /// The three arrangements this program draws, then whatever the person has
    /// saved into the cover-page gallery, then the line that saves another.
    /// Word's is the same two halves — its own designs and the person's — and
    /// a gallery with only the first half is one a person can never add to.
    pub(super) fn cover_gallery(&self) -> (Vec<String>, Vec<Row>) {
        let mut items: Vec<String> =
            Layout::ALL.iter().map(|layout| layout.label().to_owned()).collect();
        let mut rows: Vec<Row> =
            items.iter().map(|_| Row::new(Kind::Choice, Icon::CoverPage)).collect();

        for block in self.blocks_in_gallery(wp_docx::blocks::COVER_PAGES) {
            items.push(block);
            rows.push(Row::new(Kind::Choice, Icon::QuickParts));
        }

        items.push(String::new());
        rows.push(Row::separator());
        // Word's line at the foot of the gallery, and it is there only when
        // there is one to take off: a line that does nothing is the same lie
        // as a button that does nothing.
        if self.document.has_cover_page() {
            items.push(crate::messages::t("Remove Current Cover Page").to_owned());
            rows.push(Row::new(Kind::Choice, Icon::LetterClear));
        }
        items.push(crate::messages::t("Save Selection to Cover Page Gallery").to_owned());
        rows.push(Row::new(Kind::Choice, Icon::Save));
        (items, rows)
    }

    /// Puts a cover page on the front of the document.
    pub(super) fn choose_cover_page(&mut self, index: usize) -> Response {
        self.popup = None;
        let Some(layout) = Layout::ALL.get(index).copied() else {
            let saved = self.blocks_in_gallery(wp_docx::blocks::COVER_PAGES);
            let past = index - Layout::ALL.len();
            if let Some(name) = saved.get(past).cloned() {
                // A cover page goes at the front, whatever the caret was on.
                self.document.move_caret(wp_docx::TextPosition::default(), false);
                return self.insert_own_block(&name);
            }
            // The separator, which cannot be pressed, then the line that takes
            // one off where there is one, then the line that saves.
            let after_separator = past.checked_sub(saved.len() + 1);
            let removing = self.document.has_cover_page();
            return match (after_separator, removing) {
                (Some(0), true) => {
                    let taken = self.document.remove_cover_page();
                    self.relayout();
                    self.scroll = 0.0;
                    self.reveal_caret();
                    self.edited(taken, crate::messages::t("Cover page taken off"))
                }
                (Some(0), false) | (Some(1), true) => {
                    self.save_selection_to(wp_docx::blocks::COVER_PAGES)
                }
                _ => Response::Ignored,
            };
        };

        // What goes on it comes from the document's own properties. With none
        // set there is nothing to write, and saying so is more use than putting
        // in an empty page.
        let cover = self.cover_to_insert();
        if cover.is_empty() {
            return self
                .report("Give the document a title first — File ▸ Properties, then Cover Page");
        }

        let changed = self.document.insert_cover_page(layout, &cover);
        self.relayout();
        self.scroll = 0.0;
        self.reveal_caret();
        self.edited(changed, &format!("Cover page: {}", layout.label()))
    }

    /// What the cover page should say.
    ///
    /// The properties, with the file's own name standing in for a title nobody
    /// has set — which is the name the person already knows the document by.
    fn cover_to_insert(&self) -> Cover {
        let mut cover = self.document.cover_from_properties();
        if cover.title.trim().is_empty() {
            cover.title = self
                .file
                .as_ref()
                .and_then(|path| path.file_stem())
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default();
        }
        if cover.author.trim().is_empty() {
            cover.author = super::files::user_name();
        }
        if cover.date.trim().is_empty() {
            cover.date = super::files::today();
        }
        cover
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::Document;

    fn editor() -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("One")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let library: &'static wp_layout::FontLibrary =
            Box::leak(Box::new(wp_layout::FontLibrary::scan_system()));
        Editor::new(library, document, None)
    }

    #[test]
    fn the_cover_gallery_is_the_arrangements_then_the_way_to_save_one_of_your_own() {
        let editor = editor();
        let (items, rows) = editor.cover_gallery();
        assert_eq!(items.len(), rows.len());
        let drawn = Layout::ALL.len();
        let saved = editor.blocks_in_gallery(wp_docx::blocks::COVER_PAGES).len();
        assert_eq!(items.len(), drawn + saved + 2, "{items:?}");
        assert!(items[drawn + saved].is_empty(), "the separator carries no words");
        assert_eq!(items[drawn + saved + 1], "Save Selection to Cover Page Gallery");
    }

    #[test]
    fn saving_one_files_it_under_cover_pages_and_not_under_quick_parts() {
        let mut editor = editor();
        editor.document.select_all();
        let (items, _) = editor.cover_gallery();
        editor.choose_cover_page(items.len() - 1);
        assert!(editor.dialog.is_some(), "it did not ask what to call it");
        assert_eq!(editor.saving_to, "coverPg");
    }

    #[test]
    fn the_separator_on_it_does_nothing() {
        let mut editor = editor();
        let saved = editor.blocks_in_gallery(wp_docx::blocks::COVER_PAGES).len();
        let separator = Layout::ALL.len() + saved;
        assert!(matches!(editor.choose_cover_page(separator), Response::Ignored));
        assert!(editor.dialog.is_none());
    }
}
