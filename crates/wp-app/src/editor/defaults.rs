//! A person's own defaults, kept where a person's defaults go.
//!
//! # What Set as Default means
//!
//! Word's Font dialog, its Paragraph dialog and its Design tab all have a
//! button that says Set as Default, and every one of them means the same
//! thing: *this, from now on, in everything I write*. Not in this document —
//! in the next one too, and after a restart.
//!
//! A program that wrote such a thing only into the document open at the time
//! would be answering a different question from the one that was asked, and
//! the person would find out a week later when the next document came up in
//! Calibri again.
//!
//! # Where they go
//!
//! Into `Normal.dotm`, which is Word's answer and is already here: **J6**
//! made the template and made new documents from it. See
//! [`super::ownblocks`], where the file is found and read.
//!
//! So Set as Default writes twice — into the document, because the person is
//! looking at it and expects it to change, and into the template, because
//! that is what "from now on" means. The two are one thing done and are said
//! as one thing.
//!
//! # What Word does that this does not
//!
//! Word asks. Its dialogs put up "Do you want to set the default font to
//! Calibri 12? This will affect all new documents based on the NORMAL
//! template." — two buttons, and the person chooses between this document and
//! every document. Here the button itself says which it is: the one on the
//! dialog is Set as Default and means everything, and a person who wants only
//! this document presses OK instead.

use wp_docx::model::{ParagraphProperties, RunProperties};
use wp_docx::theme::Theme;
use wp_docx::Document;

use super::Editor;

impl Editor {
    /// Changes the person's own template, and says whether anything changed.
    ///
    /// The template is read afresh, changed and written back, which is the
    /// same care [`super::ownblocks`] takes with it: holding it open would
    /// mean two windows writing over each other's defaults.
    pub(super) fn change_own_template(
        &mut self,
        change: impl FnOnce(&mut Document) -> bool,
    ) -> bool {
        let Some(mut template) = self.own_template() else { return false };
        if !change(&mut template) {
            return false;
        }
        self.save_own_template(&template)
    }

    /// Word's Set as Default on the Font dialog, as far as the template.
    pub(super) fn keep_default_font(&mut self, change: &RunProperties) -> bool {
        let change = change.clone();
        self.change_own_template(move |template| template.set_default_character_format(&change))
    }

    /// And on the Paragraph dialog.
    pub(super) fn keep_default_paragraph(&mut self, change: &ParagraphProperties) -> bool {
        let change = change.clone();
        self.change_own_template(move |template| template.set_default_paragraph_format(&change))
    }

    /// And the Design tab's, which is a whole theme rather than a property.
    pub(super) fn keep_default_theme(&mut self, theme: &Theme) -> bool {
        let theme = theme.clone();
        self.change_own_template(move |template| template.set_theme(&theme).unwrap_or(false))
    }

    /// What to say afterwards: the same words whichever dialog asked, because
    /// it is the same thing.
    pub(super) fn said_of_a_default(&self, what: &str, reached_the_template: bool) -> String {
        if reached_the_template {
            crate::messages::with("{0} set for this and every new document", &[what])
        } else {
            crate::messages::with("{0} set for this document only", &[what])
        }
    }
}
