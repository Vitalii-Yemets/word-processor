//! Word's Hyphenation menu on the Layout tab: None, Automatic, Manual, and
//! the options behind them.
//!
//! # What the three do
//!
//! **None** and **Automatic** are one setting of the document, and the
//! layout obeys it: with it on, a word that does not fit is broken where the
//! patterns of its language allow, with a hyphen drawn at the break. See
//! [`wp_dict::hyphenation`] for where the patterns come from, which is the
//! machine and not this program.
//!
//! **Manual** is Word's other way: nothing is broken until the person says
//! so. The document is walked, every place the automatic rule would break a
//! word is offered one at a time — the word shown with its possible breaks,
//! the one the rule would take chosen — and a Yes puts an optional hyphen
//! there, which is the mark the document keeps and the layout breaks at
//! whether or not the setting is on. A No leaves the word whole and moves on.
//!
//! **Hyphenation Options** is the dialog with all four settings: whether to
//! hyphenate at all, whether words in capitals may be, the zone, and how
//! many lines in a row may end with a hyphen.

use wp_docx::TextPosition;
use wp_layout::LayoutEngine;
use wp_shell::Response;

use crate::chrome::dialog::{Answer, Button, Dialog, Field};
use crate::chrome::{Choice, Command, Popup};
use crate::messages::t;

use super::dialogs::Asking;
use super::Editor;

/// The optional hyphen: the mark a Yes puts into the word.
const SOFT: char = '\u{00AD}';

/// The button on the options dialog that goes on to hyphenate by hand.
pub(super) const MANUAL: &str = "Manual...";

/// Where manual hyphenation has got to.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Manual {
    /// The places the person said No to, which are not offered again.
    declined: Vec<TextPosition>,
    /// The word being asked about: where it begins, and where in it each
    /// possible break falls, with the one the rule would take.
    asking: Option<(TextPosition, Vec<usize>, usize)>,
}

impl Editor {
    /// Drops the Hyphenation list.
    pub(super) fn open_hyphenation(&mut self) -> Response {
        if self.close_popup_if(Choice::Hyphenation) {
            return Response::Redraw;
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::Hyphenation) else {
            return Response::Ignored;
        };
        let current = usize::from(self.document.automatic_hyphenation());
        let items = vec![
            t("None").to_owned(),
            t("Automatic").to_owned(),
            t("Manual...").to_owned(),
            t("Hyphenation Options...").to_owned(),
        ];
        self.popup = Some(Popup::new(Choice::Hyphenation, items, Some(current), left, top, 220.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// One line of it chosen.
    pub(super) fn choose_hyphenation(&mut self, index: usize) -> Response {
        self.popup = None;
        match index {
            0 | 1 => {
                let on = index == 1;
                let changed = self.document.set_automatic_hyphenation(on);
                self.relayout();
                if on && !self.has_patterns() {
                    // The document says so, and Word will hyphenate it; here
                    // nothing is broken until the machine has the patterns.
                    return self.report(&format!(
                        "{} {}",
                        t("Hyphenation: automatic."),
                        t("No hyphenation patterns for this language are on this machine")
                    ));
                }
                self.edited(
                    changed,
                    if on { "Hyphenation: automatic" } else { "Hyphenation: none" },
                )
            }
            2 => self.start_manual_hyphenation(),
            3 => self.open_hyphenation_options(),
            _ => Response::Ignored,
        }
    }

    /// Whether the machine has patterns for the language at the caret.
    fn has_patterns(&self) -> bool {
        let language = self.document.language_here();
        wp_dict::hyphenation::path_for_language(&language).is_some()
    }

    /// Word's Hyphenation dialog: the four settings.
    pub(super) fn open_hyphenation_options(&mut self) -> Response {
        let zone = self.document.hyphenation_zone().unwrap_or(360);
        let limit = self.document.consecutive_hyphen_limit().unwrap_or(0).max(0);
        let dialog = Dialog::with_buttons(
            "Hyphenation",
            vec![
                Field::Check {
                    label: "Automatically hyphenate document".to_owned(),
                    on: self.document.automatic_hyphenation(),
                },
                Field::Check {
                    label: "Hyphenate words in CAPS".to_owned(),
                    on: self.document.hyphenate_capitals(),
                },
                Field::Number {
                    label: "Hyphenation zone".to_owned(),
                    value: format!("{}", zone as f32 / 20.0),
                    unit: "pt",
                },
                Field::Number {
                    label: "Limit consecutive hyphens to".to_owned(),
                    value: if limit == 0 { "0".to_owned() } else { limit.to_string() },
                    unit: "lines (0: no limit)",
                },
            ],
            vec![
                Button { label: MANUAL.to_owned(), answer: Answer::Named(MANUAL), default: false },
                Button { label: "OK".to_owned(), answer: Answer::Accept, default: true },
                Button { label: "Cancel".to_owned(), answer: Answer::Cancel, default: false },
            ],
        );
        self.ask(Asking::HyphenationOptions, dialog)
    }

    /// The dialog answered: every setting written, and hand hyphenation begun
    /// when that is the button that shut it.
    pub(super) fn apply_hyphenation_options(&mut self, dialog: &Dialog, manual: bool) -> Response {
        let mut changed = self.document.set_automatic_hyphenation(dialog.ticked(0));
        changed |= self.document.set_hyphenate_capitals(dialog.ticked(1));
        let zone =
            dialog.said(2).trim().parse::<f32>().ok().map(|points| (points * 20.0).round() as i32);
        changed |= self.document.set_hyphenation_zone(zone.filter(|twips| *twips >= 0));
        let limit = dialog.said(3).trim().parse::<i32>().ok().filter(|lines| *lines >= 0);
        changed |= self.document.set_consecutive_hyphen_limit(match limit {
            Some(0) | None => None,
            other => other,
        });
        self.relayout();
        if manual {
            self.edited(changed, "");
            return self.start_manual_hyphenation();
        }
        self.edited(changed, "Hyphenation options")
    }

    // --- by hand ------------------------------------------------------------------

    /// Begins walking the document, offering each word the rule would break.
    pub(super) fn start_manual_hyphenation(&mut self) -> Response {
        if !self.has_patterns() {
            return self.report(t("No hyphenation patterns for this language are on this machine"));
        }
        self.manual_hyphenation = Manual::default();
        self.ask_next_hyphenation()
    }

    /// Finds the next word the automatic rule would break, and asks about
    /// it; done when there is none.
    fn ask_next_hyphenation(&mut self) -> Response {
        let Some((at, breaks, chosen)) = self.next_hyphenation_candidate() else {
            self.manual_hyphenation = Manual::default();
            return self.report(t("Hyphenation is complete"));
        };
        let word = self.word_at(at);
        // Each way the word could be broken, the one the rule would take
        // chosen: what Word shows as the word with a cursor among its breaks.
        let items: Vec<String> =
            breaks.iter().map(|cut| format!("{}-{}", &word[..*cut], &word[*cut..])).collect();
        let current = breaks.iter().position(|cut| *cut == chosen).unwrap_or(0);
        self.manual_hyphenation.asking = Some((at, breaks, chosen));
        let dialog = Dialog::with_buttons(
            "Manual Hyphenation",
            vec![Field::Choice { label: "Hyphenate at".to_owned(), items, current }],
            vec![
                Button { label: "Yes".to_owned(), answer: Answer::Accept, default: true },
                Button { label: "No".to_owned(), answer: Answer::Named("No"), default: false },
                Button { label: "Cancel".to_owned(), answer: Answer::Cancel, default: false },
            ],
        );
        self.ask(Asking::ManualHyphenation, dialog)
    }

    /// The word beginning at a place in the text.
    fn word_at(&self, at: TextPosition) -> String {
        let text = self.document.paragraph_text(at.paragraph).unwrap_or_default();
        text[at.offset.min(text.len())..]
            .chars()
            .take_while(|character| character.is_alphabetic())
            .collect()
    }

    /// The next place the automatic rule would break a word, after the ones
    /// declined: where the word begins, where in it each break may fall,
    /// and the break the rule takes.
    ///
    /// Asked of the layout itself: the document is laid out again with
    /// hyphenation on, and every line that ends with a hyphen the patterns
    /// put there names a word. That is the same rule the setting obeys, zone
    /// and limit and all, so what is offered by hand is what Automatic would
    /// do on its own.
    fn next_hyphenation_candidate(&mut self) -> Option<(TextPosition, Vec<usize>, usize)> {
        let mut copy = self.document.clone();
        copy.set_automatic_hyphenation(true);
        let mut engine = LayoutEngine::new(self.library).with_dpi(self.pixels_per_inch());
        let metrics = self.view_metrics();
        let pages = engine.layout_document_with(&copy, metrics);

        let mut found: Vec<(TextPosition, usize)> = Vec::new();
        for page in &pages {
            for line in &page.lines {
                let Some(hyphen) = page.glyphs[line.glyphs.clone()]
                    .iter()
                    .rev()
                    .find(|glyph| !glyph.invisible)
                    .filter(|glyph| glyph.source_length == 0)
                else {
                    continue;
                };
                // The hyphen names where the break falls; the word is the
                // letters round it, and one the person already marked is
                // not asked about.
                let text = self.document.paragraph_text(line.paragraph).unwrap_or_default();
                let cut = hyphen.source.offset.min(text.len());
                if text[..cut].ends_with(SOFT) {
                    continue;
                }
                let start = text[..cut]
                    .char_indices()
                    .rev()
                    .take_while(|(_, character)| character.is_alphabetic())
                    .last()
                    .map_or(cut, |(index, _)| index);
                found.push((TextPosition::new(line.paragraph, start), cut - start));
            }
        }
        found.sort();
        found.dedup();
        for (at, chosen) in found {
            if self.manual_hyphenation.declined.contains(&at) {
                continue;
            }
            let word = self.word_at(at);
            let language = self.document.language_at(at);
            let Some(patterns) = wp_dict::hyphenation::for_language(&language) else { continue };
            let breaks = patterns.breaks(&word);
            if breaks.is_empty() {
                continue;
            }
            return Some((at, breaks, chosen));
        }
        None
    }

    /// The dialog answered: Yes puts the mark in at the break chosen, No
    /// leaves the word and goes on, and either asks about the next.
    pub(super) fn answer_manual_hyphenation(&mut self, dialog: &Dialog, yes: bool) -> Response {
        let Some((at, breaks, _)) = self.manual_hyphenation.asking.take() else {
            return Response::Ignored;
        };
        if yes {
            let picked = dialog.said(0);
            let cut =
                picked.find('-').and_then(|dash| breaks.iter().find(|cut| **cut == dash).copied());
            if let Some(cut) = cut {
                let place = TextPosition::new(at.paragraph, at.offset + cut);
                let caret = self.document.caret();
                let changed = self.document.insert_text(place, &SOFT.to_string());
                self.document.set_caret(caret);
                self.relayout();
                self.edited(changed, "");
            }
        }
        // Declined or done, the word is not offered again: with the mark in
        // it the layout breaks it there and the rule has nothing to add.
        self.manual_hyphenation.declined.push(at);
        self.ask_next_hyphenation()
    }

    /// Cancel: the words marked so far stay marked, and the walk stops.
    pub(super) fn cancel_manual_hyphenation(&mut self) {
        self.manual_hyphenation = Manual::default();
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

    /// Whether the machine has English patterns at all.
    fn patterns_present() -> bool {
        if wp_dict::hyphenation::for_language("en-US").is_none() {
            eprintln!("no English hyphenation patterns on this machine; skipping");
            return false;
        }
        true
    }

    /// A page of long words, which cannot all fit whole on their lines.
    fn editor() -> Editor {
        let mut body = Body::default();
        let words = "responsibility understanding information typography development knowledge extraordinary government international photograph telephone university beautiful algorithm ".repeat(4);
        body.blocks.push(Block::Paragraph(Paragraph::text(words.trim())));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor
    }

    /// How many lines on the pages end with a hyphen that is not text.
    fn hyphenated_lines(editor: &Editor) -> usize {
        editor
            .pages
            .iter()
            .flat_map(|page| {
                page.lines.iter().map(move |line| {
                    page.glyphs[line.glyphs.clone()]
                        .iter()
                        .rev()
                        .find(|glyph| !glyph.invisible)
                        .is_some_and(|glyph| glyph.source_length == 0)
                })
            })
            .filter(|hyphenated| *hyphenated)
            .count()
    }

    #[test]
    fn automatic_breaks_the_words_and_none_puts_them_back_whole() {
        if !patterns_present() {
            return;
        }
        let mut editor = editor();
        assert_eq!(hyphenated_lines(&editor), 0);
        editor.choose_hyphenation(1);
        assert!(editor.document.automatic_hyphenation());
        assert!(hyphenated_lines(&editor) >= 1, "nothing was broken");
        editor.choose_hyphenation(0);
        assert!(!editor.document.automatic_hyphenation());
        assert_eq!(hyphenated_lines(&editor), 0);
    }

    #[test]
    fn the_options_dialog_writes_all_four_settings() {
        let mut editor = editor();
        editor.open_hyphenation_options();
        assert_eq!(editor.asking, Some(Asking::HyphenationOptions));
        let mut dialog = editor.dialog.take().expect("the dialog");
        assert!(!dialog.ticked(0));
        assert_eq!(dialog.said(2), "18");
        if let Some(Field::Check { on, .. }) = dialog.fields.get_mut(0) {
            *on = true;
        }
        if let Some(Field::Check { on, .. }) = dialog.fields.get_mut(1) {
            *on = false;
        }
        if let Some(Field::Number { value, .. }) = dialog.fields.get_mut(2) {
            *value = "36".to_owned();
        }
        if let Some(Field::Number { value, .. }) = dialog.fields.get_mut(3) {
            *value = "2".to_owned();
        }
        editor.apply_hyphenation_options(&dialog, false);
        assert!(editor.document.automatic_hyphenation());
        assert!(!editor.document.hyphenate_capitals());
        assert_eq!(editor.document.hyphenation_zone(), Some(720));
        assert_eq!(editor.document.consecutive_hyphen_limit(), Some(2));
    }

    #[test]
    fn manual_hyphenation_offers_each_word_and_a_yes_puts_the_mark_in() {
        if !patterns_present() {
            return;
        }
        let mut editor = editor();
        editor.start_manual_hyphenation();
        assert_eq!(editor.asking, Some(Asking::ManualHyphenation));
        let dialog = editor.dialog.take().expect("the dialog");
        let offered = dialog.said(0);
        assert!(offered.contains('-'), "{offered}");
        let (at, breaks, chosen) = editor.manual_hyphenation.asking.clone().expect("a word");
        assert!(breaks.contains(&chosen));

        // Yes: the mark goes in at the break the rule chose, and the next
        // word is asked about.
        editor.answer_manual_hyphenation(&dialog, true);
        let text = editor.document.paragraph_text(at.paragraph).unwrap_or_default();
        assert!(text[at.offset + chosen..].starts_with(SOFT), "no mark at the break");
        assert_eq!(editor.asking, Some(Asking::ManualHyphenation));
        let second = editor.manual_hyphenation.asking.clone().expect("a second word");
        assert_ne!(second.0, at);

        // No: the word is left whole and not asked about again; Cancel stops.
        let dialog = editor.dialog.take().expect("the dialog");
        editor.answer_manual_hyphenation(&dialog, false);
        assert!(editor.manual_hyphenation.declined.contains(&second.0));
        editor.cancel_manual_hyphenation();
        assert!(editor.manual_hyphenation.asking.is_none());
        // With the mark in, the automatic setting off, the word still breaks
        // at the mark: that is what the mark is.
        assert!(!editor.document.automatic_hyphenation());
        assert!(hyphenated_lines(&editor) >= 1);
    }
}
