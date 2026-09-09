//! Captions, bookmarks and cross-references, from the References tab.

use wp_docx::authorities::Category;
use wp_docx::captions::{Label, Reference};
use wp_docx::TextPosition;
use wp_shell::Response;

use crate::chrome::findbar::{FindBar, Purpose};
use crate::chrome::{Choice, Popup};

use super::Editor;

impl Editor {
    /// Opens the strip that takes the words of a caption.
    pub(super) fn start_caption(&mut self) -> Response {
        self.find_bar = Some(FindBar::for_purpose(Purpose::Caption));
        self.clamp_scroll();
        self.needs_redraw = true;
        self.report("Type what the caption should say, then press Enter")
    }

    /// Puts the caption under the paragraph the caret is in.
    pub(super) fn finish_caption(&mut self) -> Response {
        let Some(bar) = &self.find_bar else { return Response::Ignored };
        let text = bar.needle.trim().to_owned();
        self.find_bar = None;

        // A caption with no words is still a caption: "Figure 3" on its own is
        // what somebody labelling a run of diagrams wants.
        let changed = self.document.add_caption(Label::Figure, &text);
        self.relayout();
        self.reveal_caret();
        self.edited(changed, "Caption added")
    }

    /// Drops open the list of places a cross-reference could point at.
    pub(super) fn open_references(&mut self, kind: Reference) -> Response {
        let targets = self.reference_targets();
        if targets.is_empty() {
            return self
                .report("Nothing to point at yet — add a bookmark, a caption or a heading first");
        }
        if self.popup.as_ref().is_some_and(|popup| popup.choice == Choice::Reference) {
            self.popup = None;
            self.needs_redraw = true;
            return Response::Redraw;
        }

        let button = match kind {
            Reference::Text => crate::chrome::Command::CrossReference,
            Reference::Page => crate::chrome::Command::PageReference,
        };
        let Some((left, top, _)) = self.ribbon.command_rect(button) else {
            return Response::Ignored;
        };

        self.reference_kind = kind;
        let items = targets.into_iter().map(|(_, label)| label).collect();
        self.popup = Some(Popup::new(Choice::Reference, items, None, left, top, 240.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Puts a reference to whichever place was chosen at the caret.
    pub(super) fn choose_reference(&mut self, index: usize) -> Response {
        self.popup = None;
        let targets = self.reference_targets();
        let Some((name, label)) = targets.get(index).cloned() else { return Response::Ignored };

        // A heading offered as a target has no bookmark of its own yet; naming
        // it now is what gives the reference something to find.
        self.name_heading_targets();

        let kind = self.reference_kind;
        let changed = self.document.add_cross_reference(&name, kind);
        self.relayout();
        self.reveal_caret();
        self.edited(changed, &format!("Reference to {label}"))
    }

    /// Everything a cross-reference could point at, with what to call it.
    ///
    /// Headings, captions and hand-made bookmarks — which is Word's list, minus
    /// the numbered items and the footnotes it also offers.
    pub(super) fn reference_targets(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();

        for caption in self.document.captions() {
            if let Some(name) = caption.bookmark {
                out.push((name, caption.text.clone()));
            }
        }
        for mark in self.document.bookmarks() {
            if mark.is_hidden() {
                continue;
            }
            let text = self
                .document
                .bookmark_text(&mark.name)
                .filter(|text| !text.is_empty())
                .unwrap_or_else(|| mark.name.clone());
            out.push((mark.name, text));
        }

        // Headings are worth offering even when nobody has named them, because
        // "see the chapter on X" is the commonest reference there is.
        for heading in self.headings() {
            let name = wp_docx::bookmarks::sanitise_name(&heading.text);
            if out.iter().any(|(existing, _)| *existing == name) {
                continue;
            }
            out.push((name, heading.text));
        }
        out
    }

    /// Names any heading a reference is about to point at, so it can be found.
    pub(super) fn name_heading_targets(&mut self) {
        let caret = self.caret();
        for heading in self.headings() {
            let name = wp_docx::bookmarks::sanitise_name(&heading.text);
            if self.document.bookmark(&name).is_some() {
                continue;
            }
            let length = self.document.paragraph_text(heading.paragraph).map_or(0, |t| t.len());
            self.document.set_caret(TextPosition::new(heading.paragraph, 0));
            self.document.extend_selection_to(TextPosition::new(heading.paragraph, length));
            self.document.add_bookmark(&name);
            self.document.clear_selection();
        }
        self.document.set_caret(caret);
    }
}

impl Editor {
    /// Writes a table of figures, or brings the one there up to date.
    pub(super) fn write_figures(&mut self) -> Response {
        let pages = self.paragraph_pages();
        let count = self.document.insert_figures(Label::Figure, &pages);
        self.relayout();

        // The table has moved the pages, so the numbers are worked out again —
        // the same two-pass rule a table of contents follows.
        let pages = self.paragraph_pages();
        self.document.insert_figures(Label::Figure, &pages);
        self.relayout();
        self.reveal_caret();

        self.report(
            match count {
                0 => "No captions to gather — add one with Caption first".to_owned(),
                1 => "Table of figures: one caption".to_owned(),
                many => format!("Table of figures: {many} captions"),
            }
            .as_str(),
        )
    }

    /// Opens the strip that takes the words to list in the index.
    pub(super) fn start_index_entry(&mut self) -> Response {
        let mut bar = FindBar::for_purpose(Purpose::IndexEntry);
        // Whatever is selected is what a person means to index, which saves
        // typing it out again.
        let selected = self.document.selected_text();
        if !selected.is_empty() && !selected.contains('\n') {
            bar.needle = selected;
        }
        self.find_bar = Some(bar);
        self.clamp_scroll();
        self.needs_redraw = true;
        self.report("Type the words to list in the index, then press Enter")
    }

    /// Marks the caret's place with whatever was typed.
    pub(super) fn finish_index_entry(&mut self) -> Response {
        let Some(bar) = &self.find_bar else { return Response::Ignored };
        let text = bar.needle.trim().to_owned();
        if text.is_empty() {
            return self.close_find();
        }
        self.find_bar = None;

        let changed = self.document.mark_index_entry(&text);
        self.needs_redraw = true;
        self.edited(changed, &format!("Marked: {text}"))
    }

    /// Writes the index, or brings the one there up to date.
    pub(super) fn write_index(&mut self) -> Response {
        let pages = self.paragraph_pages();
        let count = self.document.insert_index(&pages);
        self.relayout();

        let pages = self.paragraph_pages();
        self.document.insert_index(&pages);
        self.relayout();
        self.reveal_caret();

        self.report(
            match count {
                0 => "Nothing is marked for the index — use Mark Entry first".to_owned(),
                1 => "Index: one entry".to_owned(),
                many => format!("Index: {many} entries"),
            }
            .as_str(),
        )
    }
}

impl Editor {
    /// Opens the strip that takes a citation to mark.
    pub(super) fn start_authority(&mut self) -> Response {
        let mut bar = FindBar::for_purpose(Purpose::Authority);
        // What is selected is the citation, which saves typing it out again.
        let selected = self.document.selected_text();
        if !selected.is_empty() && !selected.contains('\n') {
            bar.needle = selected;
        }
        self.find_bar = Some(bar);
        self.clamp_scroll();
        self.needs_redraw = true;
        self.report("Type the citation, then a semicolon and its short form, then Enter")
    }

    /// Marks the citation that was typed.
    pub(super) fn finish_authority(&mut self) -> Response {
        let Some(bar) = &self.find_bar else { return Response::Ignored };
        let typed = bar.needle.trim().to_owned();
        if typed.is_empty() {
            return self.close_find();
        }
        self.find_bar = None;
        self.needs_redraw = true;

        // "Smith v Jones, 1 F.2d 1; Smith" — the long form, then the short one.
        let (long, short) = match typed.split_once(';') {
            Some((long, short)) => (long.trim().to_owned(), short.trim().to_owned()),
            None => (typed.clone(), String::new()),
        };
        let changed = self.document.mark_authority(&long, &short, Category::Cases);
        self.edited(changed, &format!("Marked: {long}"))
    }

    /// Drops open the categories a table of authorities can be made for.
    pub(super) fn open_authorities(&mut self) -> Response {
        if self.popup.as_ref().is_some_and(|popup| popup.choice == Choice::Authority) {
            self.popup = None;
            self.needs_redraw = true;
            return Response::Redraw;
        }
        let Some((left, top, _)) =
            self.ribbon.command_rect(crate::chrome::Command::TableOfAuthorities)
        else {
            return Response::Ignored;
        };

        let items = Category::ALL.iter().map(|category| category.label().to_owned()).collect();
        self.popup = Some(Popup::new(Choice::Authority, items, None, left, top, 260.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Writes the table for whichever category was chosen.
    pub(super) fn choose_authorities(&mut self, index: usize) -> Response {
        self.popup = None;
        let Some(category) = Category::ALL.get(index).copied() else { return Response::Ignored };

        let pages = self.paragraph_pages();
        let count = self.document.insert_authorities(category, &pages);
        self.relayout();

        // The table has moved the pages, so the numbers are worked out again.
        let pages = self.paragraph_pages();
        self.document.insert_authorities(category, &pages);
        self.relayout();
        self.reveal_caret();

        self.report(
            match count {
                0 => format!("Nothing is marked as {}", category.label().to_lowercase()),
                1 => format!("{}: one citation", category.label()),
                many => format!("{}: {many} citations", category.label()),
            }
            .as_str(),
        )
    }
}
