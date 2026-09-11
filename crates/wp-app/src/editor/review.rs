//! Comments and tracked changes, from the Review tab.

use wp_docx::languages::LANGUAGES;
use wp_docx::revisions::{Decision, Reviser};
use wp_docx::TextPosition;
use wp_shell::Response;

use crate::chrome::findbar::FindBar;
use crate::chrome::{Choice, Command, Popup};

use super::Editor;

/// One of the kinds of mark Word's Show Markup menu switches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MarkupKind {
    Comments,
    Changes,
    Formatting,
}

/// The three, by Word's names and in Word's order.
///
/// Word's menu has two more — Ink, which needs a pen, and Balloons, which is a
/// place to put the marks rather than a kind of mark. Neither is drawn, because
/// neither is here.
const MARKUP_KINDS: &[(&str, MarkupKind)] = &[
    ("Comments", MarkupKind::Comments),
    ("Insertions and Deletions", MarkupKind::Changes),
    ("Formatting", MarkupKind::Formatting),
];

impl Editor {
    /// Opens the strip that takes the text of a new comment.
    pub(super) fn start_comment(&mut self) -> Response {
        self.find_bar = Some(FindBar::for_comment());
        self.clamp_scroll();
        self.needs_redraw = true;
        self.report("Type the comment, then press Enter")
    }

    /// Attaches whatever was typed into that strip to the selection.
    pub(super) fn finish_comment(&mut self) -> Response {
        let Some(bar) = &self.find_bar else { return Response::Ignored };
        let text = bar.needle.trim().to_owned();
        if text.is_empty() {
            return self.close_find();
        }

        let author = self.reviser().author;
        let date = super::files::timestamp();
        match self.document.add_comment(&text, &author, &date) {
            Ok(_) => {
                self.find_bar = None;
                self.relayout();
                self.clamp_scroll();
                self.report("Comment added")
            }
            Err(error) => {
                wp_shell::dialog::show_error(&format!("Cannot add the comment: {error}"));
                Response::Ignored
            }
        }
    }

    /// Removes the comment the caret is inside, or the nearest one after it.
    pub(super) fn delete_comment_here(&mut self) -> Response {
        let Some(comment) = self.comment_at_caret().or_else(|| self.comment_after_caret()) else {
            return self.report("There is no comment here");
        };
        let changed = self.document.delete_comment(comment.id);
        self.edited(changed, "Comment deleted")
    }

    /// Moves the caret to the comment before or after it.
    pub(super) fn step_comment(&mut self, forwards: bool) -> Response {
        let comments = self.document.comments();
        if comments.is_empty() {
            return self.report("This document has no comments");
        }

        let caret = self.caret();
        let wanted = if forwards {
            comments
                .iter()
                .find(|comment| starts_after(comment, caret))
                .or_else(|| comments.first())
        } else {
            comments
                .iter()
                .rev()
                .find(|comment| !starts_after(comment, caret) && !starts_at(comment, caret))
                .or_else(|| comments.last())
        };

        let Some(comment) = wanted else { return Response::Ignored };
        let Some((start, end)) = comment.range else {
            return self.report(&format!("{}: {}", comment.author, comment.text));
        };

        self.document.move_caret(start, false);
        if end != start {
            self.document.move_caret(end, true);
        }
        self.reveal_caret();
        self.needs_redraw = true;
        self.report(&format!("{}: {}", comment.author, comment.text))
    }

    /// The comment whose range the caret is inside.
    pub(super) fn comment_at_caret(&self) -> Option<wp_docx::comments::Comment> {
        let caret = self.caret();
        self.document.comments().into_iter().find(|comment| {
            comment.range.is_some_and(|(start, end)| {
                (start.paragraph, start.offset) <= (caret.paragraph, caret.offset)
                    && (caret.paragraph, caret.offset) <= (end.paragraph, end.offset)
            })
        })
    }

    fn comment_after_caret(&self) -> Option<wp_docx::comments::Comment> {
        let caret = self.caret();
        self.document.comments().into_iter().find(|comment| starts_after(comment, caret))
    }

    /// Turns the recording of changes on or off.
    pub(super) fn toggle_track_changes(&mut self) -> Response {
        let wanted = !self.document.tracking_changes();
        let reviser = self.reviser();
        self.document.set_reviser(reviser);

        if !self.document.set_tracking_changes(wanted) {
            return Response::Ignored;
        }
        self.update_title();
        self.needs_redraw = true;
        self.report(if wanted { "Changes are being tracked" } else { "Changes are not tracked" })
    }

    /// Drops open Word's Show Markup menu: which kinds of mark are shown.
    ///
    /// Three kinds, and Word switches them apart because they answer different
    /// questions. "What did they change?" is insertions and deletions. "What
    /// did they say about it?" is the comments. "Did they touch the
    /// formatting?" is a question a person asks only when something looks
    /// wrong, and a document full of reformatted text is unreadable while every
    /// word of it is coloured.
    pub(super) fn open_markup_menu(&mut self) -> Response {
        if self.close_popup_if(Choice::Markup) {
            return Response::Redraw;
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::ShowMarkup) else {
            return Response::Ignored;
        };

        // Ticked the way Word ticks them: the ones being shown.
        let items = MARKUP_KINDS
            .iter()
            .map(|(label, kind)| {
                let on = self.showing_markup(*kind);
                format!("{} {label}", if on { '\u{2713}' } else { ' ' })
            })
            .collect();
        self.popup = Some(Popup::new(Choice::Markup, items, None, left, top, 240.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Switches one of the three on or off.
    pub(super) fn choose_markup(&mut self, index: usize) -> Response {
        self.popup = None;
        let Some((label, kind)) = MARKUP_KINDS.get(index).copied() else {
            return Response::Ignored;
        };

        let wanted = !self.showing_markup(kind);
        match kind {
            MarkupKind::Comments => {
                // Word's tick and its Show Comments button are one setting, so
                // this is the pane and nothing else. Keeping a flag of its own
                // beside the pane would be keeping the same fact twice, and the
                // two would part company the first time the pane was closed
                // from anywhere else.
                if wanted {
                    return self.show_comment_pane();
                }
                self.navigation.section = crate::chrome::navigation::Section::Headings;
            }
            MarkupKind::Changes => self.show_markup = wanted,
            MarkupKind::Formatting => self.show_formatting_markup = wanted,
        }
        self.relayout();
        self.report(&format!("{label} {}", if wanted { "shown" } else { "hidden" }))
    }

    /// Whether one of the three kinds is being shown.
    fn showing_markup(&self, kind: MarkupKind) -> bool {
        match kind {
            MarkupKind::Comments => {
                self.show_navigation
                    && self.navigation.section == crate::chrome::navigation::Section::Comments
            }
            MarkupKind::Changes => self.show_markup,
            MarkupKind::Formatting => self.show_formatting_markup,
        }
    }

    /// Accepts or rejects the changes in the paragraph at the caret.
    pub(super) fn resolve_here(&mut self, decision: Decision) -> Response {
        let resolved = self.document.resolve_revisions_here(decision);
        if resolved == 0 {
            return self.report("There are no tracked changes here");
        }
        self.relayout();
        self.reveal_caret();
        self.report(&name_for(decision, resolved))
    }

    /// Moves the caret to the tracked change before or after it.
    ///
    /// Word's Accept and Reject buttons do this as soon as they have dealt with
    /// one, so that a document can be gone through without touching the mouse.
    pub(super) fn step_change(&mut self, forwards: bool) -> Response {
        let paragraphs = self.document.paragraphs_with_revisions();
        if paragraphs.is_empty() {
            return Response::Ignored;
        }

        let here = self.caret().paragraph;
        let wanted = if forwards {
            paragraphs.iter().find(|index| **index > here).or_else(|| paragraphs.first())
        } else {
            paragraphs.iter().rev().find(|index| **index < here).or_else(|| paragraphs.last())
        };
        let Some(paragraph) = wanted.copied() else { return Response::Ignored };

        self.document.set_caret(TextPosition::new(paragraph, 0));
        self.reveal_caret();
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Accepts or rejects every change in the document.
    pub(super) fn resolve_all(&mut self, decision: Decision) -> Response {
        let resolved = self.document.resolve_all_revisions(decision);
        if resolved == 0 {
            return self.report("This document has no tracked changes");
        }
        self.relayout();
        self.reveal_caret();
        self.report(&name_for(decision, resolved))
    }

    /// Who this program records changes and comments as.
    ///
    /// The machine's user name, which is the only name it has to go on without
    /// asking. It is what Word does when it has not been told otherwise.
    pub(super) fn reviser(&self) -> Reviser {
        Reviser { author: super::files::user_name(), date: super::files::timestamp() }
    }
}

fn starts_after(comment: &wp_docx::comments::Comment, caret: TextPosition) -> bool {
    comment
        .range
        .is_some_and(|(start, _)| (start.paragraph, start.offset) > (caret.paragraph, caret.offset))
}

fn starts_at(comment: &wp_docx::comments::Comment, caret: TextPosition) -> bool {
    comment.range.is_some_and(|(start, _)| {
        (start.paragraph, start.offset) == (caret.paragraph, caret.offset)
    })
}

fn name_for(decision: Decision, count: usize) -> String {
    let verb = match decision {
        Decision::Accept => "Accepted",
        Decision::Reject => "Rejected",
    };
    if count == 1 {
        format!("{verb} one change")
    } else {
        format!("{verb} {count} changes")
    }
}

impl Editor {
    /// Shows the comments in the navigation pane.
    pub(super) fn show_comment_pane(&mut self) -> Response {
        self.show_navigation = true;
        self.navigation.section = crate::chrome::navigation::Section::Comments;
        self.relayout();
        let count = self.document.comments().len();
        self.report(
            match count {
                0 => "This document has no comments".to_owned(),
                1 => "One comment".to_owned(),
                many => format!("{many} comments"),
            }
            .as_str(),
        )
    }
}

impl Editor {
    /// Writes a table of contents, or brings the one there up to date.
    ///
    /// The page numbers come from the layout, which is the only thing that
    /// knows them — so the document is laid out, the numbers read off, and then
    /// the table written with them in.
    pub(super) fn write_contents(&mut self) -> Response {
        let pages = self.paragraph_pages();
        let count = self.document.insert_contents(3, &pages);
        if count == 0 && !self.document.has_contents() {
            return self.report("This document has no headings to gather");
        }
        self.relayout();
        self.reveal_caret();

        // Once the table is in, the pages have moved; a second pass puts the
        // right numbers in. Word does the same, and calls it updating.
        let pages = self.paragraph_pages();
        self.document.insert_contents(3, &pages);
        self.relayout();
        self.reveal_caret();

        self.report(
            match count {
                1 => "Table of contents: one heading".to_owned(),
                many => format!("Table of contents: {many} headings"),
            }
            .as_str(),
        )
    }

    /// Which page each paragraph starts on, counting from one.
    pub(super) fn paragraph_pages(&self) -> Vec<usize> {
        let mut pages = vec![0usize; self.document.paragraph_count()];
        for (number, page) in self.pages.iter().enumerate() {
            for line in &page.lines {
                if let Some(slot) = pages.get_mut(line.paragraph) {
                    if *slot == 0 {
                        *slot = number + 1;
                    }
                }
            }
        }
        pages
    }
}

impl Editor {
    /// Drops open the languages a stretch of text can be marked as.
    pub(super) fn open_languages(&mut self) -> Response {
        if self.popup.as_ref().is_some_and(|popup| popup.choice == Choice::Language) {
            self.popup = None;
            self.needs_redraw = true;
            return Response::Redraw;
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::Language) else {
            return Response::Ignored;
        };

        let here = self.document.language_here();
        let current = LANGUAGES.iter().position(|entry| entry.tag.eq_ignore_ascii_case(&here));
        let items = LANGUAGES.iter().map(|entry| entry.name.to_owned()).collect();
        self.popup = Some(Popup::new(Choice::Language, items, current, left, top, 280.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Marks the selection as being in whichever language was chosen.
    pub(super) fn choose_language(&mut self, index: usize) -> Response {
        self.popup = None;
        let Some(entry) = LANGUAGES.get(index) else { return Response::Ignored };
        let changed = self.document.set_language(entry.tag);
        self.finish_character_change(changed, &format!("Language: {}", entry.name))
    }
}

impl Editor {
    /// Compares this document with another and marks the difference.
    pub(super) fn compare_documents(&mut self) -> Response {
        let filters = [wp_shell::dialog::FileFilter { label: "Word documents", pattern: "*.docx" }];
        let Some(path) = wp_shell::dialog::open_file("Compare with", &filters) else {
            return Response::Ignored;
        };

        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) => return self.report(&format!("It could not be read: {error}")),
        };
        let revised = match wp_docx::Document::open(&bytes) {
            Ok(document) => document,
            Err(error) => return self.report(&format!("It could not be opened: {error}")),
        };

        // Named after what it is rather than after a person: nobody wrote these
        // changes, they were worked out.
        let marked = self.document.compare_with(&revised, "Compare");
        self.relayout();
        self.reveal_caret();

        self.edited(
            marked > 0,
            &match marked {
                0 => "The two documents say the same thing".to_owned(),
                1 => "One difference, marked as a change".to_owned(),
                many => format!("{many} differences, marked as changes"),
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::Document;
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event};

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    fn editor() -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("one two three")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor
    }

    /// Where each of the three sits on the menu.
    fn row_of(kind: MarkupKind) -> usize {
        MARKUP_KINDS.iter().position(|(_, found)| *found == kind).expect("a kind")
    }

    #[test]
    fn the_menu_offers_words_three_kinds() {
        assert_eq!(MARKUP_KINDS.len(), 3);
    }

    #[test]
    fn each_kind_is_switched_on_its_own() {
        let mut editor = editor();
        assert!(editor.showing_markup(MarkupKind::Changes));
        assert!(editor.showing_markup(MarkupKind::Formatting));

        editor.choose_markup(row_of(MarkupKind::Changes));
        assert!(!editor.showing_markup(MarkupKind::Changes));
        assert!(
            editor.showing_markup(MarkupKind::Formatting),
            "hiding the insertions hid the formatting too"
        );

        editor.choose_markup(row_of(MarkupKind::Formatting));
        assert!(!editor.showing_markup(MarkupKind::Formatting));
        assert!(!editor.showing_markup(MarkupKind::Changes));
    }

    #[test]
    fn the_comments_tick_is_the_pane_and_nothing_else() {
        let mut editor = editor();
        editor.navigation.section = crate::chrome::navigation::Section::Headings;
        assert!(!editor.showing_markup(MarkupKind::Comments));

        editor.choose_markup(row_of(MarkupKind::Comments));
        assert!(editor.showing_markup(MarkupKind::Comments), "the pane did not open");

        editor.choose_markup(row_of(MarkupKind::Comments));
        assert!(!editor.showing_markup(MarkupKind::Comments), "the pane did not shut");
    }

    #[test]
    fn the_menu_ticks_what_is_being_shown() {
        let mut editor = editor();
        editor.choose_markup(row_of(MarkupKind::Formatting));
        // The button is on the Review tab, and a menu hangs under a button that
        // has been drawn.
        editor.ribbon.tab = crate::chrome::ribbon::Tab::Review;
        editor.draw(1400, 900);
        editor.open_markup_menu();

        let popup = editor.popup.as_ref().expect("the menu");
        let row = |kind| popup.item(row_of(kind)).expect("a row");
        assert!(row(MarkupKind::Changes).starts_with('\u{2713}'));
        assert!(!row(MarkupKind::Formatting).starts_with('\u{2713}'));
    }

    #[test]
    fn reformatted_text_is_marked_in_the_authors_colour() {
        let mut editor = editor();
        editor.document.set_tracking_changes(true);
        editor.document.set_caret(wp_docx::TextPosition::new(0, 0));
        editor.document.move_caret(wp_docx::TextPosition::new(0, 3), true);
        editor.document.set_format(wp_docx::CharacterFormat::Bold, true);
        editor.relayout();

        // How many colours the page's text is drawn in. One means nothing is
        // marked; more than one means something is.
        let marked = |editor: &Editor| {
            let page = editor.pages.first().expect("a page");
            let mut colours: Vec<(u8, u8, u8, u8)> = page
                .glyphs
                .iter()
                .map(|glyph| {
                    (glyph.color.red, glyph.color.green, glyph.color.blue, glyph.color.alpha)
                })
                .collect();
            colours.sort_unstable();
            colours.dedup();
            colours.len()
        };
        assert!(marked(&editor) > 1, "the reformatted words are the same colour as the rest");

        editor.choose_markup(row_of(MarkupKind::Formatting));
        assert_eq!(marked(&editor), 1, "the mark is still there with the switch off");
    }
}
