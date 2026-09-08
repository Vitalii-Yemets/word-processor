//! Translating with a glossary, from the Review tab.
//!
//! The glossary is a file the person supplies, the same way the spelling check
//! takes a word list. Word's button sends the document to a translation service
//! instead; this program talks to nobody, and [`wp_docx::translate`] says why.
//!
//! Pressing the button with no glossary loaded asks for one. Pressing it again
//! translates: the selection if there is one, the whole document if not — which
//! is the choice Word's own menu offers.

use wp_docx::translate::Glossary;
use wp_shell::dialog::FileFilter;
use wp_shell::Response;

use super::Editor;

impl Editor {
    /// Translates with the glossary, asking for one the first time.
    pub(super) fn translate(&mut self) -> Response {
        if self.glossary.is_empty() {
            return self.load_glossary();
        }

        let selection = self.document.selection().is_some();
        let glossary = core::mem::take(&mut self.glossary);
        let replaced = self.document.translate(&glossary, selection);
        self.glossary = glossary;

        if replaced == 0 {
            return self.report("No term of the glossary was found");
        }
        self.relayout();
        self.reveal_caret();
        let where_ = if selection { "the selection" } else { "the document" };
        self.edited(true, &format!("{replaced} terms translated in {where_}"))
    }

    /// Asks for the glossary to translate with.
    pub(super) fn load_glossary(&mut self) -> Response {
        let filters = [
            FileFilter { label: "Glossaries", pattern: "*.txt" },
            FileFilter { label: "All files", pattern: "*.*" },
        ];
        let Some(path) = wp_shell::dialog::open_file("Glossary", &filters) else {
            return Response::Ignored;
        };

        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) => return self.report(&format!("The glossary could not be read: {error}")),
        };

        let glossary = Glossary::parse(&bytes);
        if glossary.is_empty() {
            return self.report("That file holds no terms — one per line, as source = target");
        }
        let count = glossary.len();
        self.glossary = glossary;
        self.report(&format!("{count} terms — press Translate again to use them"))
    }
}
