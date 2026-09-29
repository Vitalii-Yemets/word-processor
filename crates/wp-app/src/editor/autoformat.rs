//! Word's AutoFormat: a whole document formatted at once.
//!
//! # What it is for
//!
//! The rules that work as one types — quotes curled, `1st` raised, a line of
//! `- ` made a list — are no help to a document that arrived already typed:
//! a text file, a paste from an e-mail, something written with the rules
//! switched off. AutoFormat goes over such a document with the same rules and
//! does at once what typing it here would have done, and a little more that
//! only makes sense of a whole document: a short line with a blank line under
//! it is a heading.
//!
//! # Its switches are its own
//!
//! Word's AutoCorrect dialog has an AutoFormat tab beside the AutoFormat As
//! You Type one, with the same rules on both and each ticked apart. So the
//! rules here are [`crate::autocorrect::AutoCorrect::for_reformatting`]: the
//! typing rules with the tab's switches in place of their own, and none of
//! the ones that only make sense of a word as it is typed — the replacement
//! list, the capitals — which Word's AutoFormat does not apply either.
//!
//! # Reviewing it
//!
//! "AutoFormat and review each change" makes the same changes as tracked
//! changes, so that each can be seen and each taken back with the Review
//! tab's own Accept and Reject — the words as insertions and deletions, a
//! heading or a list as the paragraph's formatting changed. Then it asks: all
//! of it, none of it, or one at a time. None of it is the whole AutoFormat
//! undone. All of it is the same, and then AutoFormat run again untracked,
//! which is the same result with no marks on it — and, unlike accepting every
//! tracked change in the document, leaves somebody else's changes waiting for
//! their own review.

use wp_docx::model::NumberingReference;
use wp_docx::TextPosition;
use wp_shell::Response;

use crate::autocorrect::{AutoCorrect, Fix, Reformat};
use crate::chrome::dialog::{Answer, Button, Dialog, Field};
use crate::messages::{self, t};

use super::dialogs::Asking;
use super::Editor;

/// The two ways of running it, as the dialog offers them.
const NOW: &str = "AutoFormat now";
const REVIEWED: &str = "AutoFormat and review each change";

/// The dialog's button that hands over to the AutoFormat tab.
pub(super) const OPTIONS: &str = "Options...";

/// The three answers of the dialog that follows a review.
const ACCEPT_ALL: &str = "Accept All";
const REJECT_ALL: &str = "Reject All";
const REVIEW: &str = "Review Changes";

// The AutoFormat dialog.
const ABOUT: usize = 0;
const HOW: usize = 1;

/// Which tab of the AutoCorrect dialog is AutoFormat's.
const AUTOFORMAT_TAB: usize = 3;

impl Editor {
    /// Opens Word's AutoFormat dialog: now, or with a review.
    pub(super) fn open_autoformat(&mut self) -> Response {
        let fields = vec![
            Field::Heading(messages::with(
                "\u{201C}{0}\u{201D} will be formatted automatically.",
                &[&self.document_name()],
            )),
            Field::Choice {
                label: "How".to_owned(),
                items: vec![NOW.to_owned(), REVIEWED.to_owned()],
                current: 0,
            },
        ];
        crate::chrome::dialog::check_rows(
            "AutoFormat",
            &fields,
            &[(ABOUT, "a heading"), (HOW, "a list")],
        );
        let dialog = Dialog::with_buttons(
            "AutoFormat",
            fields,
            vec![
                Button { label: "OK".to_owned(), answer: Answer::Accept, default: true },
                Button {
                    label: OPTIONS.to_owned(),
                    answer: Answer::Named(OPTIONS),
                    default: false,
                },
                Button { label: "Cancel".to_owned(), answer: Answer::Cancel, default: false },
            ],
        )
        .wide(480.0);
        self.ask(Asking::AutoFormat, dialog)
    }

    /// What OK does.
    pub(super) fn apply_autoformat_dialog(&mut self, dialog: &Dialog) -> Response {
        if dialog.chose(HOW) == 1 {
            self.autoformat_for_review()
        } else {
            self.autoformat_now()
        }
    }

    /// Options…: the AutoCorrect dialog, open at the AutoFormat tab, which is
    /// what the button is for.
    pub(super) fn autoformat_options(&mut self) -> Response {
        self.dialog = None;
        self.asking = None;
        let response = self.open_autocorrect();
        if let Some(dialog) = &mut self.dialog {
            dialog.show_tab(AUTOFORMAT_TAB);
        }
        response
    }

    /// AutoFormat Now, and the dialog's first choice: every change made, as
    /// one thing to undo.
    pub(super) fn autoformat_now(&mut self) -> Response {
        let made = self.reformat_document();
        self.needs_redraw = true;
        if made == 0 {
            return self.report(t("AutoFormat found nothing to change"));
        }
        self.report(&messages::with("AutoFormat made {0} changes", &[&made.to_string()]))
    }

    /// The dialog's second choice: the changes made as tracked changes, and
    /// then the question of what to do with them.
    fn autoformat_for_review(&mut self) -> Response {
        let depth = self.document.undo_depth();
        self.document.begin_gesture();
        let was_tracking = self.document.tracking_changes();
        self.document.set_tracking_changes(true);
        let made = self.reformat_document();
        self.document.set_tracking_changes(was_tracking);
        self.document.end_gesture();
        self.relayout();

        if made == 0 {
            // Nothing to review, and the switch flipped on and off again is
            // nothing to undo either.
            if self.document.undo_depth() > depth {
                self.undo_step();
            }
            return self.report(t("AutoFormat found nothing to change"));
        }
        self.autoformat_review(made)
    }

    /// Word's question once the changes are marked.
    fn autoformat_review(&mut self, made: usize) -> Response {
        let fields = vec![
            Field::Heading(t("Formatting completed. You can now:").to_owned()),
            Field::Said { label: "Changes".to_owned(), value: made.to_string() },
            Field::Said {
                label: "Accept All".to_owned(),
                value: t("keep every change").to_owned(),
            },
            Field::Said {
                label: "Reject All".to_owned(),
                value: t("take them all back").to_owned(),
            },
            Field::Said {
                label: "Review Changes".to_owned(),
                value: t("go through them on the Review tab").to_owned(),
            },
        ];
        let dialog = Dialog::with_buttons(
            "AutoFormat",
            fields,
            vec![
                Button {
                    label: ACCEPT_ALL.to_owned(),
                    answer: Answer::Named(ACCEPT_ALL),
                    default: true,
                },
                Button {
                    label: REJECT_ALL.to_owned(),
                    answer: Answer::Named(REJECT_ALL),
                    default: false,
                },
                Button { label: REVIEW.to_owned(), answer: Answer::Named(REVIEW), default: false },
            ],
        )
        .wide(480.0);
        self.ask(Asking::AutoFormatReview, dialog)
    }

    /// One of the three answers. Shutting the dialog any other way leaves
    /// the changes marked, which loses nothing: they are there to review.
    pub(super) fn answer_autoformat_review(&mut self, answer: Answer) -> Response {
        match answer {
            Answer::Named(REJECT_ALL) => {
                let changed = self.undo_step();
                self.edited(changed, "")
            }
            Answer::Named(ACCEPT_ALL) => {
                self.undo_step();
                self.reformat_document();
                self.edited(true, "")
            }
            _ => {
                self.ribbon.tab = crate::chrome::ribbon::Tab::Review;
                self.needs_redraw = true;
                Response::Redraw
            }
        }
    }

    /// Goes over the whole document with the AutoFormat tab's rules, as one
    /// thing to undo. Returns how many changes it made.
    ///
    /// The words first, every paragraph: the quotes, the dashes, the raised
    /// ordinals, the fractions, the `*bold*`, the addresses. Then what each
    /// paragraph is, now its words are settled: a list where it begins with
    /// a list's marker, and a heading where it is a short line with a blank
    /// line under it.
    pub(super) fn reformat_document(&mut self) -> usize {
        let rules = self.autocorrect.for_reformatting();
        let tab = self.autocorrect.reformat.clone();
        let caret = self.document.caret();
        let count = self.document.paragraph_count();
        let mut made = 0usize;

        self.document.begin_gesture();
        for paragraph in 0..count {
            let Some(text) = self.document.paragraph_text(paragraph) else { continue };
            // From the end backwards, so that each change leaves the places
            // of the ones still to come where they were found.
            for fix in rules.fixes_in(&text).into_iter().rev() {
                if self.make_fix(paragraph, &fix) {
                    made += 1;
                }
            }
        }

        let mut counting: Option<(usize, i32, NumberingReference)> = None;
        for paragraph in 0..count {
            if self.make_list_of(paragraph, &rules, &tab, &mut counting) {
                made += 1;
            }
        }

        // Whether a line is one line long is the layout's to say, and the
        // layout has to have seen the words as they now are.
        self.relayout();
        for paragraph in 0..count {
            if self.make_heading_of(paragraph, &rules, &tab) {
                made += 1;
            }
        }

        let last = self.document.paragraph_count().saturating_sub(1);
        let length = self.document.paragraph_text(caret.paragraph.min(last)).map_or(0, |t| t.len());
        self.document
            .set_caret(TextPosition::new(caret.paragraph.min(last), caret.offset.min(length)));
        self.document.end_gesture();
        self.relayout();
        made
    }

    /// Makes one change to a paragraph's words.
    fn make_fix(&mut self, paragraph: usize, fix: &Fix) -> bool {
        let at = |offset| TextPosition::new(paragraph, offset);
        match fix {
            Fix::Text { start, end, putting } => {
                self.document.set_caret(at(*start));
                self.document.extend_selection_to(at(*end));
                self.document.delete_selection();
                self.document.type_text(putting)
            }
            Fix::Emphasis(emphasis) => {
                // As when typed: a document that limits formatting to its
                // styles keeps the marks as marks.
                if !self.autoformat_may_override() {
                    return false;
                }
                self.document.set_caret(at(emphasis.close));
                self.document.extend_selection_to(at(emphasis.close + 1));
                self.document.delete_selection();
                self.document.set_caret(at(emphasis.open));
                self.document.extend_selection_to(at(emphasis.open + 1));
                self.document.delete_selection();
                self.document.set_caret(at(emphasis.open));
                self.document.extend_selection_to(at(emphasis.close - 1));
                let change = if emphasis.bold {
                    wp_docx::model::RunProperties { bold: Some(true), ..Default::default() }
                } else {
                    wp_docx::model::RunProperties { italic: Some(true), ..Default::default() }
                };
                self.document.apply_run_formatting(&change);
                self.document.clear_selection();
                true
            }
            Fix::Link { start, end } => {
                let Some(text) = self.document.paragraph_text(paragraph) else { return false };
                let Some(address) = text.get(*start..*end).map(str::to_owned) else {
                    return false;
                };
                self.document.set_caret(at(*start));
                self.document.extend_selection_to(at(*end));
                let changed = self.document.add_hyperlink(&address, &address);
                self.document.clear_selection();
                changed
            }
        }
    }

    /// Makes a list of a paragraph that begins with a list's marker: `- `,
    /// `* ` or `• ` for bullets, `1. ` or `1) ` for numbers, the marker taken
    /// away because the list draws its own.
    ///
    /// Numbers that follow on — `2. ` under `1. ` — continue the list above
    /// rather than begin one of their own at two.
    fn make_list_of(
        &mut self,
        paragraph: usize,
        rules: &AutoCorrect,
        tab: &Reformat,
        counting: &mut Option<(usize, i32, NumberingReference)>,
    ) -> bool {
        let Some(text) = self.document.paragraph_text(paragraph) else { return false };
        if !self.plain_paragraph(paragraph, tab) {
            return false;
        }
        let Some(space) = text.find([' ', '\t']) else { return false };
        let (marker, rest) = (&text[..space], &text[space + 1..]);
        if rest.trim().is_empty() {
            return false;
        }

        let list = if matches!(marker, "-" | "*" | "\u{2022}") && tab.bulleted_lists {
            *counting = None;
            NumberingReference { id: wp_docx::BULLET_LIST, level: 0 }
        } else if let Some(number) = rules.list_start(marker).filter(|_| tab.numbered_lists) {
            let follows = counting
                .filter(|(above, was, _)| *above + 1 == paragraph && *was + 1 == number)
                .map(|(_, _, list)| list);
            let list = match follows {
                Some(list) => list,
                None if number == 1 => NumberingReference { id: wp_docx::NUMBERED_LIST, level: 0 },
                None => match self.document.numbered_list_starting_at(number) {
                    Some(id) => NumberingReference { id, level: 0 },
                    None => return false,
                },
            };
            *counting = Some((paragraph, number, list));
            list
        } else {
            return false;
        };

        self.document.set_caret(TextPosition::new(paragraph, 0));
        self.document.extend_selection_to(TextPosition::new(paragraph, space + 1));
        self.document.delete_selection();
        self.document.set_list_here(Some(list))
    }

    /// Makes a heading of a short line with a blank line under it: one line
    /// long, begun with a capital, not ended with punctuation — the rule for
    /// a line entered twice as it is typed, the blank line under it standing
    /// for the second Enter.
    fn make_heading_of(&mut self, paragraph: usize, rules: &AutoCorrect, tab: &Reformat) -> bool {
        if !tab.headings || !self.plain_paragraph(paragraph, tab) {
            return false;
        }
        let blank_under = self
            .document
            .paragraph_text(paragraph + 1)
            .is_some_and(|under| under.trim().is_empty());
        if !blank_under || self.document.paragraph_in_table(paragraph + 1) {
            return false;
        }
        let Some(text) = self.document.paragraph_text(paragraph) else { return false };
        let Some((level, tabs)) = rules.heading_level(&text) else { return false };
        let style = format!("Heading{level}");
        if !self.document.style_is_available(&style) {
            return false;
        }
        let (Some(first), Some(last)) = (
            self.caret_rect_at(TextPosition::new(paragraph, 0)),
            self.caret_rect_at(TextPosition::new(paragraph, text.len())),
        ) else {
            return false;
        };
        if (first.1 - last.1).abs() > 0.5 {
            return false;
        }

        self.document.set_caret(TextPosition::new(paragraph, 0));
        if tabs > 0 {
            self.document.extend_selection_to(TextPosition::new(paragraph, tabs));
            self.document.delete_selection();
        }
        self.document.set_paragraph_style_here(Some(&style))
    }

    /// Whether a paragraph is one AutoFormat may make a list or a heading of:
    /// not in a table, not in a list already, and — where the tab says to
    /// keep them — not given a style of its own.
    fn plain_paragraph(&mut self, paragraph: usize, tab: &Reformat) -> bool {
        if self.document.paragraph_in_table(paragraph) {
            return false;
        }
        if tab.keep_styles
            && self.document.style_of(paragraph).is_some_and(|style| style != "Normal")
        {
            return false;
        }
        self.document.set_caret(TextPosition::new(paragraph, 0));
        self.document.list_here().is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::revisions::ChangeKind;
    use wp_docx::Document;
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event, Key, Modifiers};

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    /// An editor on a document of these paragraphs, typed elsewhere with no
    /// rules at all — which is what AutoFormat is for.
    fn editor_of(paragraphs: &[&str]) -> Editor {
        let mut body = Body::default();
        for text in paragraphs {
            body.blocks.push(Block::Paragraph(Paragraph::text(text)));
        }
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor
    }

    fn text_of(editor: &Editor, paragraph: usize) -> String {
        editor.document.paragraph_text(paragraph).unwrap_or_default()
    }

    const PLAIN: &[&str] = &[
        "Introduction",
        "",
        "He said \"it's the 21st\" - and *meant* it; see www.example.com.",
        "- milk",
        "- bread",
        "1. first",
        "2. second",
    ];

    #[test]
    fn a_document_typed_elsewhere_is_formatted_as_if_typed_here() {
        let mut editor = editor_of(PLAIN);
        let made = editor.reformat_document();
        assert!(made >= 8, "{made}");

        // The heading, the words, the lists.
        assert_eq!(editor.document.style_of(0).as_deref(), Some("Heading1"));
        assert_eq!(
            text_of(&editor, 2),
            "He said \u{201C}it\u{2019}s the 21\u{02E2}\u{1D57}\u{201D} \u{2013} and meant it; \
             see www.example.com."
        );
        for (paragraph, text) in [(3, "milk"), (4, "bread"), (5, "first"), (6, "second")] {
            assert_eq!(text_of(&editor, paragraph), text);
            editor.document.set_caret(TextPosition::new(paragraph, 0));
            let list = editor.document.list_here().expect("a list");
            let wanted = if paragraph < 5 { wp_docx::BULLET_LIST } else { wp_docx::NUMBERED_LIST };
            assert_eq!(list.id, wanted, "paragraph {paragraph}");
        }
        // The emphasis became formatting, and the address a link.
        let Block::Paragraph(line) = &editor.document.body().blocks[2] else { panic!() };
        assert!(line.runs.iter().any(|run| run.properties.bold == Some(true)));
        assert!(editor.document.plain_text().contains("www.example.com"));

        // One undo takes the whole of it back.
        editor.handle(Event::KeyDown {
            key: Key::Letter('z'),
            modifiers: Modifiers { control: true, ..Modifiers::default() },
        });
        for (paragraph, text) in PLAIN.iter().enumerate() {
            assert_eq!(text_of(&editor, paragraph), *text);
        }
        assert_ne!(editor.document.style_of(0).as_deref(), Some("Heading1"));
    }

    #[test]
    fn the_tab_says_what_is_done_and_the_typing_rules_have_no_say() {
        let mut editor = editor_of(PLAIN);
        editor.autocorrect.reformat.curly_quotes = false;
        editor.autocorrect.reformat.headings = false;
        editor.autocorrect.reformat.bulleted_lists = false;
        // Switched off for typing, and still done here: the tab is its own.
        editor.autocorrect.ordinals = false;
        editor.reformat_document();
        assert!(text_of(&editor, 2).contains("\"it's"), "the quotes were curled");
        assert!(text_of(&editor, 2).contains("21\u{02E2}\u{1D57}"));
        assert_ne!(editor.document.style_of(0).as_deref(), Some("Heading1"));
        assert_eq!(text_of(&editor, 3), "- milk");
        assert_eq!(text_of(&editor, 5), "first", "the numbers were still for doing");
    }

    #[test]
    fn a_line_with_no_blank_line_under_it_or_a_sentence_is_no_heading() {
        let mut editor = editor_of(&["Introduction", "The text begins at once.", "", "Done."]);
        editor.reformat_document();
        assert_ne!(editor.document.style_of(0).as_deref(), Some("Heading1"));
        assert_ne!(editor.document.style_of(1).as_deref(), Some("Heading1"));
    }

    #[test]
    fn a_paragraph_with_a_style_of_its_own_keeps_it() {
        let mut editor = editor_of(&["- Quoted", "", "Plain line", ""]);
        editor.document.set_caret(TextPosition::new(0, 0));
        editor.document.set_paragraph_style_here(Some("Quote"));
        editor.document.set_caret(TextPosition::new(2, 0));
        editor.document.set_paragraph_style_here(Some("Quote"));
        editor.reformat_document();
        assert_eq!(text_of(&editor, 0), "- Quoted", "made a list of a styled paragraph");
        assert_eq!(editor.document.style_of(2).as_deref(), Some("Quote"));

        // Unless the tab says not to keep them.
        let mut editor = editor_of(&["Plain line", ""]);
        editor.document.set_caret(TextPosition::new(0, 0));
        editor.document.set_paragraph_style_here(Some("Quote"));
        editor.autocorrect.reformat.keep_styles = false;
        editor.reformat_document();
        assert_eq!(editor.document.style_of(0).as_deref(), Some("Heading1"));
    }

    #[test]
    fn reviewing_marks_every_change_and_rejecting_them_all_puts_it_back() {
        let mut editor = editor_of(PLAIN);
        editor.open_autoformat();
        if let Some(dialog) = &mut editor.dialog {
            if let Some(Field::Choice { current, .. }) = dialog.fields.get_mut(HOW) {
                *current = 1;
            }
        }
        editor.finish_dialog(Answer::Accept);
        assert_eq!(editor.asking, Some(Asking::AutoFormatReview), "no question afterwards");
        assert!(!editor.document.tracking_changes(), "the switch was left on");

        let changes = editor.document.changes();
        assert!(changes.iter().any(|change| change.kind == ChangeKind::Insertion));
        assert!(changes.iter().any(|change| change.kind == ChangeKind::Deletion));
        assert!(
            changes.iter().any(|change| change.kind == ChangeKind::Formatting),
            "the heading and the lists are not marked: {changes:?}"
        );

        editor.finish_dialog(Answer::Named(REJECT_ALL));
        assert_eq!(editor.document.revision_count(), 0);
        for (paragraph, text) in PLAIN.iter().enumerate() {
            assert_eq!(text_of(&editor, paragraph), *text);
        }
    }

    #[test]
    fn accepting_them_all_is_the_formatting_with_no_marks_left() {
        let mut editor = editor_of(PLAIN);
        editor.autoformat_for_review();
        editor.finish_dialog(Answer::Named(ACCEPT_ALL));
        assert_eq!(editor.document.revision_count(), 0);
        assert_eq!(editor.document.style_of(0).as_deref(), Some("Heading1"));
        assert_eq!(text_of(&editor, 3), "milk");
        assert!(text_of(&editor, 2).starts_with("He said \u{201C}"));
    }

    #[test]
    fn accepting_them_all_leaves_somebody_elses_changes_for_their_own_review() {
        let mut editor = editor_of(&["Introduction", "", "one - two"]);
        editor.document.set_tracking_changes(true);
        editor.document.set_caret(TextPosition::new(2, 0));
        editor.document.type_text("Already ");
        editor.document.set_tracking_changes(false);
        let theirs = editor.document.revision_count();
        assert_eq!(theirs, 1);

        editor.autoformat_for_review();
        assert!(editor.document.revision_count() > theirs);
        editor.finish_dialog(Answer::Named(ACCEPT_ALL));
        assert_eq!(editor.document.revision_count(), theirs, "their change was decided for them");
    }

    #[test]
    fn reviewing_one_at_a_time_leaves_the_marks_and_goes_to_the_review_tab() {
        let mut editor = editor_of(PLAIN);
        editor.autoformat_for_review();
        editor.finish_dialog(Answer::Named(REVIEW));
        assert!(editor.document.revision_count() > 0);
        assert_eq!(editor.ribbon.tab, crate::chrome::ribbon::Tab::Review);
    }

    #[test]
    fn a_document_with_nothing_to_change_is_told_so_and_left_alone() {
        let mut editor = editor_of(&["Nothing here to change."]);
        let depth = editor.document.undo_depth();
        editor.autoformat_for_review();
        assert_eq!(editor.asking, None);
        assert_eq!(editor.document.undo_depth(), depth);
        assert_eq!(editor.status, "AutoFormat found nothing to change");
    }

    #[test]
    fn options_opens_the_autoformat_tab() {
        let mut editor = editor_of(PLAIN);
        editor.open_autoformat();
        editor.finish_dialog(Answer::Named(OPTIONS));
        assert_eq!(editor.asking, Some(Asking::AutoCorrect));
        assert_eq!(editor.dialog.as_ref().map(Dialog::showing_tab), Some(AUTOFORMAT_TAB));
    }

    #[test]
    fn control_alt_k_formats_the_document_now() {
        let mut editor = editor_of(PLAIN);
        editor.handle(Event::KeyDown {
            key: Key::Letter('k'),
            modifiers: Modifiers { control: true, alt: true, ..Modifiers::default() },
        });
        assert_eq!(editor.document.style_of(0).as_deref(), Some("Heading1"));
    }
}
