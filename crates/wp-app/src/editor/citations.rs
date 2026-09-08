//! Sources, citations and the bibliography, from the References tab.

use wp_docx::bibliography::{Source, SourceKind};
use wp_shell::Response;

use crate::chrome::findbar::{FindBar, Purpose};
use crate::chrome::{Choice, Popup};

use super::Editor;

impl Editor {
    /// Drops open the list of sources a citation could name.
    pub(super) fn open_citations(&mut self) -> Response {
        if self.popup.as_ref().is_some_and(|popup| popup.choice == Choice::Citation) {
            self.popup = None;
            self.needs_redraw = true;
            return Response::Redraw;
        }

        let sources = self.document.sources();
        if sources.is_empty() {
            // Nothing to cite yet, so the one thing that can be done is done
            // rather than shown as an empty list.
            return self.start_source();
        }

        let Some((left, top, _)) = self.ribbon.command_rect(crate::chrome::Command::InsertCitation)
        else {
            return Response::Ignored;
        };

        let mut items: Vec<String> = sources.iter().map(describe).collect();
        items.push("Add New Source…".to_owned());
        self.popup = Some(Popup::new(Choice::Citation, items, None, left, top, 280.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Puts a citation of whichever source was chosen at the caret.
    pub(super) fn choose_citation(&mut self, index: usize) -> Response {
        self.popup = None;
        let sources = self.document.sources();
        // One past the end is the "Add New Source…" line under the list.
        let Some(source) = sources.get(index) else { return self.start_source() };

        let tag = source.tag.clone();
        let shown = source.short();
        let changed = self.document.insert_citation(&tag);
        self.relayout();
        self.reveal_caret();
        self.edited(changed, &format!("Cited {shown}"))
    }

    /// Drops open the list of sources, for taking one away.
    pub(super) fn open_sources(&mut self) -> Response {
        if self.popup.as_ref().is_some_and(|popup| popup.choice == Choice::Source) {
            self.popup = None;
            self.needs_redraw = true;
            return Response::Redraw;
        }

        let sources = self.document.sources();
        if sources.is_empty() {
            return self.report("This document has no sources yet — use Add Source");
        }

        let Some((left, top, _)) = self.ribbon.command_rect(crate::chrome::Command::ManageSources)
        else {
            return Response::Ignored;
        };

        // Each line says what pressing it does, because pressing it cannot be
        // asked about first.
        let items: Vec<String> =
            sources.iter().map(|source| format!("Remove {}", describe(source))).collect();
        self.popup = Some(Popup::new(Choice::Source, items, None, left, top, 300.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Takes away whichever source was chosen.
    pub(super) fn choose_source(&mut self, index: usize) -> Response {
        self.popup = None;
        let sources = self.document.sources();
        let Some(source) = sources.get(index) else { return Response::Ignored };

        let shown = describe(source);
        let tag = source.tag.clone();
        let changed = self.document.remove_source(&tag);
        self.needs_redraw = true;
        self.edited(changed, &format!("Removed {shown}"))
    }

    /// Opens the strip that takes the details of a new source.
    pub(super) fn start_source(&mut self) -> Response {
        self.find_bar = Some(FindBar::for_purpose(Purpose::Source));
        self.clamp_scroll();
        self.needs_redraw = true;
        self.report("Author; Title; Year; Publisher; City — then press Enter")
    }

    /// Adds the source that was typed.
    pub(super) fn finish_source(&mut self) -> Response {
        let Some(bar) = &self.find_bar else { return Response::Ignored };
        let typed = bar.needle.trim().to_owned();
        if typed.is_empty() {
            return self.close_find();
        }
        self.find_bar = None;
        self.needs_redraw = true;

        let source = parse_source(&typed);
        match self.document.add_source(&source) {
            Ok(tag) => self.report(&format!("Source {tag} added — use Citation to cite it")),
            Err(error) => self.report(&format!("The source could not be saved: {error}")),
        }
    }

    /// Writes a bibliography, or brings the one there up to date.
    pub(super) fn write_bibliography(&mut self) -> Response {
        let count = self.document.insert_bibliography();
        self.relayout();
        self.reveal_caret();

        self.report(
            match count {
                0 => "Nothing in this document is cited yet".to_owned(),
                1 => "Bibliography: one source".to_owned(),
                many => format!("Bibliography: {many} sources"),
            }
            .as_str(),
        )
    }
}

/// What a source is called in a list.
fn describe(source: &Source) -> String {
    let title = if source.title.is_empty() { source.tag.clone() } else { source.title.clone() };
    if source.author.is_empty() {
        return title;
    }
    format!("{title} — {}", source.author)
}

/// Reads the semicolon-separated details a person typed.
///
/// The order is the one a reference is spoken in — who wrote it, what it is
/// called, when, and where it came from — so it can be typed without being
/// looked up. Everything after the author may be left out.
fn parse_source(typed: &str) -> Source {
    let mut pieces = typed.split(';').map(str::trim);
    let author = pieces.next().unwrap_or_default().to_owned();
    let title = pieces.next().unwrap_or_default().to_owned();
    let year = pieces.next().unwrap_or_default().to_owned();
    let publisher = pieces.next().unwrap_or_default().to_owned();
    let city = pieces.next().unwrap_or_default().to_owned();

    // A source with an address rather than a publisher is a web site, which is
    // worth knowing because it is written differently.
    let kind = if city.starts_with("http") || publisher.starts_with("http") {
        SourceKind::InternetSite
    } else {
        SourceKind::Book
    };

    Source { tag: String::new(), kind, author, title, year, publisher, city }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pieces_are_read_in_the_order_they_are_spoken() {
        let source = parse_source("John Doe; A Book; 2001; A Press; London");
        assert_eq!(source.author, "John Doe");
        assert_eq!(source.title, "A Book");
        assert_eq!(source.year, "2001");
        assert_eq!(source.publisher, "A Press");
        assert_eq!(source.city, "London");
    }

    #[test]
    fn everything_after_the_author_may_be_left_out() {
        let source = parse_source("John Doe");
        assert_eq!(source.author, "John Doe");
        assert!(source.title.is_empty());
    }

    #[test]
    fn an_address_makes_it_a_web_site() {
        assert_eq!(
            parse_source("A; B; 2020; ; https://example.org").kind,
            SourceKind::InternetSite
        );
        assert_eq!(parse_source("A; B; 2020; A Press; London").kind, SourceKind::Book);
    }

    #[test]
    fn a_source_is_listed_by_its_title_and_who_wrote_it() {
        let source = Source {
            title: "A Book".to_owned(),
            author: "John Doe".to_owned(),
            ..Source::default()
        };
        assert_eq!(describe(&source), "A Book — John Doe");
    }
}
