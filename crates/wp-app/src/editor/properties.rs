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

        let items = Layout::ALL.iter().map(|layout| layout.label().to_owned()).collect();
        self.popup = Some(Popup::new(Choice::Cover, items, None, left, top, 220.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Puts a cover page on the front of the document.
    pub(super) fn choose_cover_page(&mut self, index: usize) -> Response {
        self.popup = None;
        let Some(layout) = Layout::ALL.get(index).copied() else { return Response::Ignored };

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
