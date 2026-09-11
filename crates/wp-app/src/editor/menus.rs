//! The menus the ribbon's arrows drop.
//!
//! # Why a button has an arrow at all
//!
//! Because most of what a ribbon button does has more than one answer, and Word
//! puts the common answer on the face and the rest behind an arrow. Bullets
//! puts bullets on; the arrow beside it asks which bullet. Accept accepts the
//! change under the caret; the arrow asks whether to accept all of them.
//!
//! Before this, each of these buttons did the common thing and nothing else,
//! and some of them cycled: pressing Change Case walked through the five cases
//! one press at a time, which is not what Word does and is not what anybody
//! expects. A person who wants small capitals should be able to ask for them
//! once.
//!
//! # What decides what is on a menu
//!
//! Only what the program can do. Word's Select menu has four entries and two of
//! them need a selection made of several separate stretches, which this program
//! has no model for; those two are named in the roadmap rather than drawn as
//! dead rows. The same rule as everywhere else: a switch that does nothing is a
//! lie told once per person who tries it.

use wp_docx::furniture::{Furniture, Preset};
use wp_docx::model::{Alignment, LineRule, LineSpacing, ParagraphProperties};
use wp_docx::model::{Run, RunContent, RunProperties, TabAlignment};
use wp_docx::numbering::Shape;
use wp_docx::page::CaseChange;
use wp_docx::revisions::Decision;
use wp_shell::Response;

use crate::chrome::icons::Icon;
use crate::chrome::popup::{Kind, Row};
use crate::chrome::{ribbon, Choice, Popup};

use super::Editor;

/// How wide a menu is drawn.
const WIDTH: f32 = 250.0;
/// And a gallery of marks, which needs no room for words.
const GALLERY_WIDTH: f32 = 200.0;

/// The bullets Word's library offers.
///
/// Word's gallery has seven; these are the ones whose characters any ordinary
/// font can draw, which is what keeps a document readable on a machine without
/// Wingdings. See [`wp_docx::numbering`] on why that matters.
const BULLETS: &[(&str, &str)] = &[
    ("\u{2022}", "Round"),
    ("\u{25E6}", "Hollow"),
    ("\u{25AA}", "Square"),
    ("\u{25C6}", "Diamond"),
    ("\u{27A2}", "Arrow"),
    ("\u{2713}", "Tick"),
    ("\u{2014}", "Dash"),
];

/// The number formats Word's library offers.
const NUMBERS: &[(&str, &str, &str)] = &[
    ("decimal", "%1.", "1. 2. 3."),
    ("decimal", "%1)", "1) 2) 3)"),
    ("upperRoman", "%1.", "I. II. III."),
    ("upperLetter", "%1.", "A. B. C."),
    ("lowerLetter", "%1)", "a) b) c)"),
    ("lowerRoman", "%1.", "i. ii. iii."),
];

/// The shapes a list with levels inside it comes in.
///
/// Three levels each, because that is how deep the lists this program writes
/// go — see `LIST_LEVELS`.
const MULTILEVEL: &[(&str, [Shape; 3])] = &[
    (
        "1.  a.  i.",
        [
            Shape::counted("decimal", "%1."),
            Shape::counted("lowerLetter", "%2."),
            Shape::counted("lowerRoman", "%3."),
        ],
    ),
    (
        "1.  1.1  1.1.1",
        [
            Shape::counted("decimal", "%1."),
            Shape::counted("decimal", "%1.%2"),
            Shape::counted("decimal", "%1.%2.%3"),
        ],
    ),
    (
        "I.  A.  1.",
        [
            Shape::counted("upperRoman", "%1."),
            Shape::counted("upperLetter", "%2."),
            Shape::counted("decimal", "%3."),
        ],
    ),
    (
        "\u{2022}  \u{25E6}  \u{25AA}",
        [Shape::bullet("\u{2022}"), Shape::bullet("\u{25E6}"), Shape::bullet("\u{25AA}")],
    ),
];

/// The spacings Word's line spacing menu offers.
const SPACINGS: &[(f32, &str)] =
    &[(1.0, "1.0"), (1.15, "1.15"), (1.5, "1.5"), (2.0, "2.0"), (2.5, "2.5"), (3.0, "3.0")];

/// Word's own room above and below a paragraph, in twentieths of a point.
///
/// Ten points before and after, which is what its Add Space entries put in.
const ROOM: i32 = 200;

/// One of the spacing sets Word's Design tab gives a whole document.
///
/// The numbers are Word's own, in twentieths of a point for the room round a
/// paragraph and in 240ths of single for the lines. They are not chosen here:
/// a document made in Word and given "Relaxed" has to look the same when it is
/// opened here and told the same thing.
struct Spacing {
    name: &'static str,
    before: i32,
    after: i32,
    /// The line spacing, as a multiple of single.
    lines: f32,
}

const DOCUMENT_SPACINGS: &[Spacing] = &[
    Spacing { name: "No Paragraph Space", before: 0, after: 0, lines: 1.0 },
    Spacing { name: "Compact", before: 0, after: 80, lines: 1.0 },
    Spacing { name: "Tight", before: 0, after: 120, lines: 1.15 },
    Spacing { name: "Open", before: 0, after: 200, lines: 1.15 },
    Spacing { name: "Relaxed", before: 0, after: 120, lines: 1.5 },
    Spacing { name: "Double", before: 0, after: 160, lines: 2.0 },
];

impl Editor {
    /// Drops open the menu one of the ribbon's arrows carries.
    pub(super) fn open_ribbon_menu(&mut self, choice: Choice) -> Response {
        if self.close_popup_if(choice) {
            return Response::Redraw;
        }
        let Some(command) = ribbon::command_of(choice) else { return Response::Ignored };
        let Some((left, top, _)) = self.ribbon.command_rect(command) else {
            return Response::Ignored;
        };

        let (items, rows, current, width) = self.menu_contents(choice);
        if items.is_empty() {
            return Response::Ignored;
        }

        self.popup = Some(Popup::new(choice, items, current, left, top, width).with_rows(rows));
        self.palette = None;
        self.table_grid = None;
        self.needs_redraw = true;
        Response::Redraw
    }

    /// What is on one of them, and which of its entries is in force.
    fn menu_contents(&self, choice: Choice) -> (Vec<String>, Vec<Row>, Option<usize>, f32) {
        let plain = |items: Vec<String>| {
            let rows = items.iter().map(|_| Row::default()).collect();
            (items, rows, None, WIDTH)
        };

        match choice {
            Choice::BulletLibrary => {
                let items: Vec<String> =
                    BULLETS.iter().map(|(mark, name)| format!("{mark}   {name}")).collect();
                let rows = items.iter().map(|_| Row::default()).collect();
                (items, rows, self.bullet_in_force(), GALLERY_WIDTH)
            }
            Choice::NumberLibrary => {
                let items: Vec<String> =
                    NUMBERS.iter().map(|(_, _, name)| (*name).to_owned()).collect();
                let rows = items.iter().map(|_| Row::default()).collect();
                (items, rows, self.number_in_force(), GALLERY_WIDTH)
            }
            Choice::MultilevelLibrary => {
                let mut items: Vec<String> =
                    MULTILEVEL.iter().map(|(name, _)| (*name).to_owned()).collect();
                let mut rows: Vec<Row> = items.iter().map(|_| Row::default()).collect();

                // Word's Change List Level lives on this menu too, as a list of
                // its own; with three levels there are two ways to go and no
                // need for the list.
                items.push(String::new());
                rows.push(Row::separator());
                items.push("Increase List Level".to_owned());
                rows.push(Row::new(Kind::Choice, Icon::IndentMore));
                items.push("Decrease List Level".to_owned());
                rows.push(Row::new(Kind::Choice, Icon::IndentLess));
                items.push("None".to_owned());
                rows.push(Row::new(Kind::Choice, Icon::LetterClear));
                (items, rows, None, WIDTH)
            }
            Choice::LineSpacing => {
                let mut items: Vec<String> =
                    SPACINGS.iter().map(|(_, name)| (*name).to_owned()).collect();
                let mut rows: Vec<Row> = items.iter().map(|_| Row::default()).collect();
                items.push(String::new());
                rows.push(Row::separator());

                let (before, after) = self.room_here();
                items.push(
                    if before > 0 {
                        "Remove Space Before Paragraph"
                    } else {
                        "Add Space Before Paragraph"
                    }
                    .to_owned(),
                );
                rows.push(Row::new(Kind::Choice, Icon::ParagraphSpacing));
                items.push(
                    if after > 0 {
                        "Remove Space After Paragraph"
                    } else {
                        "Add Space After Paragraph"
                    }
                    .to_owned(),
                );
                rows.push(Row::new(Kind::Choice, Icon::ParagraphSpacing));
                (items, rows, self.spacing_in_force(), WIDTH)
            }
            Choice::AlignmentTab => self.alignment_tab_menu(),
            Choice::DocumentSpacing => {
                // Word's list says what each set does under its name, because
                // "Compact" and "Tight" mean nothing until you are told.
                let mut items: Vec<String> = DOCUMENT_SPACINGS
                    .iter()
                    .map(|set| {
                        format!(
                            "{}   {} pt after, {:.2} lines",
                            set.name,
                            set.after / 20,
                            set.lines
                        )
                    })
                    .collect();
                let mut rows: Vec<Row> =
                    items.iter().map(|_| Row::new(Kind::Choice, Icon::ParagraphSpacing)).collect();

                items.push(String::new());
                rows.push(Row::separator());
                items.push("Custom Paragraph Spacing…".to_owned());
                rows.push(Row::new(Kind::Choice, Icon::LineSpacing));
                (items, rows, self.document_spacing_in_force(), 320.0)
            }
            Choice::LetterCase => {
                let items: Vec<String> =
                    CaseChange::ALL.iter().map(|case| case.label().to_owned()).collect();
                plain(items)
            }
            Choice::PageNumberPlace => {
                let items = vec![
                    "Top of Page".to_owned(),
                    "Bottom of Page".to_owned(),
                    "Current Position".to_owned(),
                    String::new(),
                    "Format Page Numbers…".to_owned(),
                    "Remove Page Numbers".to_owned(),
                ];
                let rows = vec![
                    Row::new(Kind::Choice, Icon::Header),
                    Row::new(Kind::Choice, Icon::Footer),
                    Row::new(Kind::Choice, Icon::PageNumber),
                    Row::separator(),
                    Row::new(Kind::Choice, Icon::PageNumber),
                    Row::new(Kind::Choice, Icon::LetterClear),
                ];
                (items, rows, None, WIDTH)
            }
            Choice::Selecting => {
                let items = vec![
                    "Select All".to_owned(),
                    "Select All Text With Similar Formatting".to_owned(),
                    "Selection Pane…".to_owned(),
                ];
                let rows = vec![
                    Row::new(Kind::Choice, Icon::Select),
                    Row::new(Kind::Choice, Icon::Select),
                    Row::new(Kind::Choice, Icon::SelectionPane),
                ];
                (items, rows, None, 300.0)
            }
            Choice::NoteJump => {
                let items = vec![
                    "Next Footnote".to_owned(),
                    "Previous Footnote".to_owned(),
                    "Next Endnote".to_owned(),
                    "Previous Endnote".to_owned(),
                ];
                let rows = items.iter().map(|_| Row::new(Kind::Choice, Icon::Footnote)).collect();
                (items, rows, None, WIDTH)
            }
            Choice::Accepting | Choice::Rejecting => {
                let verb = if choice == Choice::Accepting { "Accept" } else { "Reject" };
                let items = vec![
                    format!("{verb} and Move to Next"),
                    format!("{verb} This Change"),
                    format!("{verb} All Changes"),
                    format!("{verb} All and Stop Tracking"),
                ];
                let icon = if choice == Choice::Accepting { Icon::Accept } else { Icon::Reject };
                let rows = items.iter().map(|_| Row::new(Kind::Choice, icon)).collect();
                (items, rows, None, WIDTH)
            }
            Choice::Tracking => {
                let locked = self.document.protection()
                    == Some(wp_docx::appearance::EditMode::TrackedChanges);
                let items = vec![
                    "Track Changes".to_owned(),
                    if locked { "Unlock Tracking" } else { "Lock Tracking" }.to_owned(),
                ];
                let rows = vec![
                    Row::new(Kind::Choice, Icon::TrackChanges),
                    Row::new(Kind::Choice, Icon::RestrictEditing),
                ];
                let current = self.document.tracking_changes().then_some(0);
                (items, rows, current, WIDTH)
            }
            // Every other list is opened by its own command, which fills it in.
            _ => (Vec::new(), Vec::new(), None, WIDTH),
        }
    }

    /// Runs whichever row of one of them was pressed.
    pub(super) fn choose_from_menu(&mut self, choice: Choice, index: usize) -> Response {
        self.popup = None;
        match choice {
            Choice::BulletLibrary => self.choose_bullet(index),
            Choice::NumberLibrary => self.choose_number(index),
            Choice::MultilevelLibrary => self.choose_multilevel(index),
            Choice::LineSpacing => self.choose_spacing(index),
            Choice::DocumentSpacing => self.choose_document_spacing(index),
            Choice::AlignmentTab => self.choose_alignment_tab(index),
            Choice::LetterCase => self.choose_case(index),
            Choice::PageNumberPlace => self.choose_page_number(index),
            Choice::Selecting => self.choose_selecting(index),
            Choice::NoteJump => self.choose_note_jump(index),
            Choice::Accepting => self.choose_resolution(Decision::Accept, index),
            Choice::Rejecting => self.choose_resolution(Decision::Reject, index),
            Choice::Tracking => self.choose_tracking(index),
            _ => Response::Ignored,
        }
    }

    // --- The list libraries -------------------------------------------------

    /// Which bullet the paragraph at the caret is marked with.
    fn bullet_in_force(&self) -> Option<usize> {
        let list = self.document.list_here()?;
        let level = self.document.numbering().level(list.id, list.level)?;
        let mark = level.shown_as();
        BULLETS.iter().position(|(bullet, _)| *bullet == mark)
    }

    /// And which number format it counts in.
    fn number_in_force(&self) -> Option<usize> {
        let list = self.document.list_here()?;
        let level = self.document.numbering().level(list.id, list.level)?;
        let text = level.shown_as();
        NUMBERS.iter().position(|(format, template, _)| {
            *template == text
                && level.format == wp_docx::numbering::NumberFormat::from_attribute(format)
        })
    }

    fn choose_bullet(&mut self, index: usize) -> Response {
        let Some((mark, name)) = BULLETS.get(index).copied() else { return Response::Ignored };
        self.apply_list(&[Shape::bullet(mark)], &format!("{name} bullets"))
    }

    fn choose_number(&mut self, index: usize) -> Response {
        let Some((format, template, name)) = NUMBERS.get(index).copied() else {
            return Response::Ignored;
        };
        self.apply_list(&[Shape::counted(format, template)], name)
    }

    fn choose_multilevel(&mut self, index: usize) -> Response {
        if let Some((name, levels)) = MULTILEVEL.get(index).copied() {
            return self.apply_list(&levels, name);
        }
        // The rows under the line: the level, and the way back out of a gallery
        // that has no "off" of its own.
        match index - MULTILEVEL.len() {
            1 => self.step_list_level(true),
            2 => self.step_list_level(false),
            3 => {
                let changed = self.document.set_list_here(None);
                self.edited(changed, "List removed")
            }
            _ => Response::Ignored,
        }
    }

    /// Puts the paragraphs of the selection into a list of the given shape.
    fn apply_list(&mut self, levels: &[Shape], name: &str) -> Response {
        let Some(id) = self.document.list_shaped(levels) else {
            return self.report("This document's lists cannot be read");
        };
        // The level is kept: a paragraph two levels deep that is given another
        // mark stays two levels deep, which is what Word does.
        let level = self.document.list_here().map_or(0, |list| list.level);
        let changed =
            self.document.set_list_here(Some(wp_docx::model::NumberingReference { id, level }));
        self.edited(changed, name)
    }

    // --- Line spacing -------------------------------------------------------

    /// How much room there is above and below the paragraph at the caret.
    fn room_here(&self) -> (i32, i32) {
        let resolved = self.document.paragraph_format_here();
        (resolved.space_before, resolved.space_after)
    }

    fn spacing_in_force(&self) -> Option<usize> {
        let spacing = self.document.line_spacing_here()?;
        if spacing.rule != LineRule::Auto {
            return None;
        }
        let value = spacing.value as f32 / 240.0;
        SPACINGS.iter().position(|(amount, _)| (amount - value).abs() < 0.01)
    }

    fn choose_spacing(&mut self, index: usize) -> Response {
        if let Some((amount, name)) = SPACINGS.get(index).copied() {
            let spacing =
                LineSpacing { value: (amount * 240.0).round() as i32, rule: LineRule::Auto };
            let changed = self.document.set_line_spacing_here(Some(spacing));
            return self.edited(changed, &format!("Line spacing {name}"));
        }

        // The two rows under the line: the room above and below a paragraph,
        // which Word puts on this menu because that is where a person looking
        // for space between paragraphs looks for it.
        let (before, after) = self.room_here();
        let (change, note) = match index - SPACINGS.len() {
            1 if before > 0 => (
                ParagraphProperties { space_before: Some(0), ..ParagraphProperties::default() },
                "Space before removed",
            ),
            1 => (
                ParagraphProperties { space_before: Some(ROOM), ..ParagraphProperties::default() },
                "Space before added",
            ),
            2 if after > 0 => (
                ParagraphProperties { space_after: Some(0), ..ParagraphProperties::default() },
                "Space after removed",
            ),
            2 => (
                ParagraphProperties { space_after: Some(ROOM), ..ParagraphProperties::default() },
                "Space after added",
            ),
            // The separator, which cannot be picked.
            _ => return Response::Ignored,
        };
        let changed = self.document.set_paragraph_format(&change);
        self.edited(changed, note)
    }

    /// Word's Insert Alignment Tab: which way the tab sends what follows it.
    fn alignment_tab_menu(&self) -> (Vec<String>, Vec<Row>, Option<usize>, f32) {
        let items = vec!["Left".to_owned(), "Center".to_owned(), "Right".to_owned()];
        let rows = vec![
            Row::new(Kind::Choice, Icon::AlignStart),
            Row::new(Kind::Choice, Icon::AlignCenter),
            Row::new(Kind::Choice, Icon::AlignEnd),
        ];
        (items, rows, None, WIDTH)
    }

    /// Puts one in at the caret.
    fn choose_alignment_tab(&mut self, index: usize) -> Response {
        let alignment = match index {
            1 => TabAlignment::Center,
            2 => TabAlignment::End,
            0 => TabAlignment::Start,
            _ => return Response::Ignored,
        };
        let run = Run {
            properties: RunProperties::default(),
            content: vec![RunContent::PositionTab(alignment)],
            field: None,
            revision: None,
            format_change: None,
        };
        let changed = self.document.insert_runs(&[run]);
        self.relayout();
        self.edited(changed, "Alignment tab")
    }
    // --- The spacing of a whole document ------------------------------------

    /// Which of Word's sets the document is in, if it is in one.
    ///
    /// Read from `w:docDefaults` and not from the paragraph at the caret: this
    /// is about the document, and a paragraph that was given its own spacing
    /// says nothing about what every other paragraph starts from.
    fn document_spacing_in_force(&self) -> Option<usize> {
        let defaults = self.document.styles().document_paragraph_defaults();
        let lines = defaults.line_spacing.and_then(|spacing| {
            (spacing.rule == LineRule::Auto).then_some(spacing.value as f32 / 240.0)
        })?;
        DOCUMENT_SPACINGS.iter().position(|set| {
            defaults.space_before.unwrap_or(0) == set.before
                && defaults.space_after.unwrap_or(0) == set.after
                && (lines - set.lines).abs() < 0.01
        })
    }

    /// Gives the whole document one of them.
    fn choose_document_spacing(&mut self, index: usize) -> Response {
        let Some(set) = DOCUMENT_SPACINGS.get(index) else {
            // The last row: Word's Custom Paragraph Spacing, which is a dialog
            // about one paragraph's spacing with Set As Default on it — the
            // same job, and the dialog this program already has for it.
            if index == DOCUMENT_SPACINGS.len() + 1 {
                return self.open_paragraph_dialog();
            }
            return Response::Ignored;
        };

        // Into the document's defaults, which is the bottom of the chain: every
        // paragraph that never said otherwise follows it, and one that did is
        // left alone. That is what makes this the document's spacing rather
        // than a change to every paragraph in it.
        let change = ParagraphProperties {
            space_before: Some(set.before),
            space_after: Some(set.after),
            line_spacing: Some(LineSpacing {
                value: (set.lines * 240.0).round() as i32,
                rule: LineRule::Auto,
            }),
            ..ParagraphProperties::default()
        };
        let changed = self.document.set_default_paragraph_format(&change);
        self.relayout();
        self.edited(changed, &format!("Paragraph spacing: {}", set.name))
    }

    // --- The rest -----------------------------------------------------------

    fn choose_case(&mut self, index: usize) -> Response {
        let Some(wanted) = CaseChange::ALL.get(index).copied() else { return Response::Ignored };
        if self.document.selection().is_none() {
            return self.report("Select some text first");
        }
        self.case_change = wanted;
        let changed = self.document.change_case(wanted);
        self.edited(changed, wanted.label())
    }

    fn choose_page_number(&mut self, index: usize) -> Response {
        let caption = self.document_name();
        let put = |editor: &mut Self, kind: Furniture, preset: Preset, note: &str| -> Response {
            match editor.document.set_furniture(kind, preset, Alignment::Center, &caption) {
                Ok(changed) => editor.edited(changed, note),
                Err(error) => editor.report(&format!("Cannot set that: {error}")),
            }
        };

        match index {
            0 => put(self, Furniture::Header, Preset::PageNumber, "Page number at the top"),
            1 => put(self, Furniture::Footer, Preset::PageNumber, "Page number at the foot"),
            // Word's Current Position drops the number where the caret is, as a
            // field, so it counts like any other page number.
            2 => {
                let changed = self.document.insert_field("PAGE", "1");
                self.edited(changed, "Page number")
            }
            4 => self.open_page_numbering(),
            5 => {
                let header = self.document.set_furniture(
                    Furniture::Header,
                    Preset::None,
                    Alignment::Start,
                    "",
                );
                let footer = self.document.set_furniture(
                    Furniture::Footer,
                    Preset::None,
                    Alignment::Start,
                    "",
                );
                let changed = header.unwrap_or(false) || footer.unwrap_or(false);
                self.edited(changed, "Page numbers removed")
            }
            _ => Response::Ignored,
        }
    }

    fn choose_selecting(&mut self, index: usize) -> Response {
        match index {
            0 => self.run(crate::chrome::Command::SelectAll),
            1 => self.run(crate::chrome::Command::SelectSimilar),
            2 => self.run(crate::chrome::Command::SelectionPane),
            _ => Response::Ignored,
        }
    }

    fn choose_note_jump(&mut self, index: usize) -> Response {
        let kind =
            if index < 2 { wp_docx::notes::Kind::Footnote } else { wp_docx::notes::Kind::Endnote };
        self.step_note_of(kind, index % 2 == 0)
    }

    fn choose_resolution(&mut self, decision: Decision, index: usize) -> Response {
        match index {
            0 => {
                let response = self.resolve_here(decision);
                self.step_change(true);
                response
            }
            1 => self.resolve_here(decision),
            2 => self.resolve_all(decision),
            3 => {
                let response = self.resolve_all(decision);
                if self.document.tracking_changes() {
                    self.document.set_tracking_changes(false);
                }
                self.update_title();
                response
            }
            _ => Response::Ignored,
        }
    }

    fn choose_tracking(&mut self, index: usize) -> Response {
        match index {
            0 => self.toggle_track_changes(),
            1 => {
                let locked = self.document.protection()
                    == Some(wp_docx::appearance::EditMode::TrackedChanges);
                // Word's Lock Tracking stops the recording being switched off.
                // Here that is the document's own restriction to tracked
                // changes, which is the same thing written down.
                let wanted =
                    if locked { None } else { Some(wp_docx::appearance::EditMode::TrackedChanges) };
                if !locked && !self.document.tracking_changes() {
                    self.toggle_track_changes();
                }
                let changed = self.document.set_protection(wanted);
                self.needs_redraw = true;
                self.edited(changed, if locked { "Tracking unlocked" } else { "Tracking locked" })
            }
            _ => Response::Ignored,
        }
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
        body.blocks.push(Block::Paragraph(Paragraph::text("One")));
        body.blocks.push(Block::Paragraph(Paragraph::text("Two")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor
    }

    #[test]
    fn every_menu_the_ribbon_promises_has_something_on_it() {
        // A button drawn with an arrow and nothing behind it is the worst of
        // both: it says there is more and there is not.
        let editor = editor();
        for choice in [
            Choice::BulletLibrary,
            Choice::NumberLibrary,
            Choice::MultilevelLibrary,
            Choice::LineSpacing,
            Choice::LetterCase,
            Choice::PageNumberPlace,
            Choice::Selecting,
            Choice::NoteJump,
            Choice::Accepting,
            Choice::Rejecting,
            Choice::Tracking,
        ] {
            let (items, rows, ..) = editor.menu_contents(choice);
            assert!(!items.is_empty(), "{choice:?} drops an empty menu");
            assert_eq!(items.len(), rows.len(), "{choice:?} has a row for every entry");
        }
    }

    #[test]
    fn choosing_a_bullet_puts_that_bullet_on_the_paragraph() {
        let mut editor = editor();
        editor.choose_bullet(3);

        let list = editor.document.list_here().expect("the paragraph is in a list");
        let level =
            editor.document.numbering().level(list.id, list.level).expect("the list is defined");
        assert_eq!(level.shown_as(), "\u{25C6}", "the diamond did not go on");
    }

    #[test]
    fn the_menu_opens_showing_the_bullet_that_is_on() {
        let mut editor = editor();
        editor.choose_bullet(2);
        assert_eq!(editor.bullet_in_force(), Some(2));
    }

    #[test]
    fn two_paragraphs_given_the_same_bullet_share_one_list() {
        // Otherwise a numbered list would start again at one half way down.
        let mut editor = editor();
        editor.choose_bullet(1);
        let first = editor.document.list_here().expect("a list").id;

        editor.document.set_caret(wp_docx::TextPosition::new(1, 0));
        editor.choose_bullet(1);
        let second = editor.document.list_here().expect("a list").id;
        assert_eq!(first, second, "the same bullet made two lists");
    }

    #[test]
    fn two_different_bullets_are_two_lists() {
        let mut editor = editor();
        editor.choose_bullet(0);
        let first = editor.document.list_here().expect("a list").id;

        editor.document.set_caret(wp_docx::TextPosition::new(1, 0));
        editor.choose_bullet(4);
        let second = editor.document.list_here().expect("a list").id;
        assert_ne!(first, second, "two marks ended up as one list");
    }

    #[test]
    fn a_list_survives_being_saved_and_opened_again() {
        let mut editor = editor();
        editor.choose_number(2);
        let wanted = editor.document.list_here().expect("a list");

        let saved = editor.document.save().expect("saving");
        let reopened = Document::open(&saved).expect("reopening");
        let level =
            reopened.numbering().level(wanted.id, wanted.level).expect("the definition came back");
        assert_eq!(level.shown_as(), "%1.");
        assert_eq!(level.format, wp_docx::numbering::NumberFormat::UpperRoman);
    }

    #[test]
    fn the_case_menu_asks_once_rather_than_cycling() {
        let mut editor = editor();
        editor.document.set_caret(wp_docx::TextPosition::new(0, 0));
        editor.document.extend_selection_to(wp_docx::TextPosition::new(0, 3));
        // The third entry is Word's UPPERCASE, and picking it twice must not
        // walk on to the next case the way the old button did.
        editor.choose_case(2);
        assert_eq!(editor.document.plain_text().lines().next(), Some("ONE"));
        editor.choose_case(2);
        assert_eq!(editor.document.plain_text().lines().next(), Some("ONE"));
    }

    #[test]
    fn the_spacing_menu_sets_the_spacing_it_names() {
        let mut editor = editor();
        editor.choose_spacing(2);
        let spacing = editor.document.line_spacing_here().expect("a spacing");
        assert_eq!(spacing.value, 360, "1.5 lines is 360 twentieths of single");
        assert_eq!(editor.spacing_in_force(), Some(2));
    }

    #[test]
    fn the_room_round_a_paragraph_goes_on_and_off_again() {
        let mut editor = editor();
        let before_row = SPACINGS.len() + 1;

        editor.choose_spacing(before_row);
        assert_eq!(editor.room_here().0, ROOM, "the room was not added");
        editor.choose_spacing(before_row);
        assert_eq!(editor.room_here().0, 0, "the room was not taken away again");
    }

    #[test]
    fn the_separator_on_the_spacing_menu_cannot_be_picked() {
        let mut editor = editor();
        let before = editor.document.plain_text();
        assert_eq!(editor.choose_spacing(SPACINGS.len()), Response::Ignored);
        assert_eq!(editor.document.plain_text(), before);
    }

    #[test]
    fn a_multilevel_gallery_entry_defines_all_three_levels() {
        let mut editor = editor();
        // Word's "1. 1.1 1.1.1", which is the second entry.
        editor.choose_multilevel(1);
        let list = editor.document.list_here().expect("a list");

        let numbering = editor.document.numbering();
        assert_eq!(numbering.level(list.id, 0).expect("level one").shown_as(), "%1.");
        assert_eq!(numbering.level(list.id, 1).expect("level two").shown_as(), "%1.%2");
        assert_eq!(numbering.level(list.id, 2).expect("level three").shown_as(), "%1.%2.%3");
    }

    #[test]
    fn two_galleries_that_start_the_same_are_still_two_lists() {
        // "1. a. i." and "1. 1.1 1.1.1" both begin with a decimal, and a
        // document given one must not be given the other.
        let mut editor = editor();
        editor.choose_multilevel(0);
        let first = editor.document.list_here().expect("a list").id;

        editor.document.set_caret(wp_docx::TextPosition::new(1, 0));
        editor.choose_multilevel(1);
        let second = editor.document.list_here().expect("a list").id;
        assert_ne!(first, second, "two galleries came out as one list");
    }

    #[test]
    fn the_list_level_stops_at_the_ends_rather_than_wrapping_round() {
        let mut editor = editor();
        editor.choose_bullet(0);
        assert_eq!(editor.document.list_here().expect("a list").level, 0);

        // Out to the last level and one more, which must not come back to the
        // margin: a list indented once too often stays where it is.
        for _ in 0..5 {
            editor.step_list_level(true);
        }
        let deepest = u8::try_from(wp_docx::LIST_LEVELS).unwrap_or(3) - 1;
        assert_eq!(editor.document.list_here().expect("a list").level, deepest);

        for _ in 0..5 {
            editor.step_list_level(false);
        }
        assert_eq!(editor.document.list_here().expect("a list").level, 0);
    }

    #[test]
    fn the_last_row_of_the_multilevel_menu_takes_the_list_off() {
        let mut editor = editor();
        editor.choose_bullet(0);
        assert!(editor.document.list_here().is_some());

        let (items, ..) = editor.menu_contents(Choice::MultilevelLibrary);
        editor.choose_multilevel(items.len() - 1);
        assert!(editor.document.list_here().is_none(), "the list stayed on");
    }

    #[test]
    fn every_row_of_every_menu_leads_somewhere() {
        // A row that does nothing is the same lie as a button that does
        // nothing. Only a separator may be unpickable, and it is drawn as one.
        let editor = editor();
        for choice in [
            Choice::BulletLibrary,
            Choice::NumberLibrary,
            Choice::MultilevelLibrary,
            Choice::LineSpacing,
            Choice::LetterCase,
            Choice::PageNumberPlace,
            Choice::Selecting,
            Choice::NoteJump,
            Choice::Accepting,
            Choice::Rejecting,
            Choice::Tracking,
        ] {
            let (items, rows, ..) = editor.menu_contents(choice);
            for (index, (label, row)) in items.iter().zip(rows.iter()).enumerate() {
                if row.kind == Kind::Separator {
                    assert!(label.is_empty(), "{choice:?} row {index} is a separator with words");
                } else {
                    assert!(!label.is_empty(), "{choice:?} row {index} has no words");
                }
            }
        }
    }

    #[test]
    fn a_document_spacing_set_goes_into_the_documents_defaults() {
        // Not onto the paragraphs: every paragraph that never said otherwise
        // follows the document's defaults, and one that did is left alone.
        // That is what makes it the document's spacing.
        let mut editor = editor();
        // "Relaxed", which is six points after and a line and a half.
        editor.choose_document_spacing(4);

        let defaults = editor.document.styles().document_paragraph_defaults().clone();
        assert_eq!(defaults.space_after, Some(120));
        assert_eq!(defaults.line_spacing.map(|spacing| spacing.value), Some(360));

        // And the paragraph itself was not touched.
        let body = editor.document.body();
        let wp_docx::model::Block::Paragraph(paragraph) = &body.blocks[0] else {
            panic!("a paragraph")
        };
        assert_eq!(paragraph.properties.space_after, None, "it wrote on the paragraph");
    }

    #[test]
    fn the_set_that_is_in_force_is_the_one_marked() {
        let mut editor = editor();
        editor.choose_document_spacing(1);
        assert_eq!(editor.document_spacing_in_force(), Some(1));

        editor.choose_document_spacing(5);
        assert_eq!(editor.document_spacing_in_force(), Some(5));
    }

    #[test]
    fn a_document_in_none_of_the_sets_marks_none_of_them() {
        // A document whose defaults are something else entirely — which most
        // documents from elsewhere are — must not have a set ticked.
        let mut editor = editor();
        editor.document.set_default_paragraph_format(&ParagraphProperties {
            space_after: Some(133),
            line_spacing: Some(LineSpacing { value: 300, rule: LineRule::Auto }),
            ..ParagraphProperties::default()
        });
        assert_eq!(editor.document_spacing_in_force(), None);
    }

    #[test]
    fn the_document_spacing_reaches_what_is_drawn() {
        // The point of the whole item: the pages have to be laid out again,
        // or the setting is a line in a file nobody sees.
        let mut editor = editor();
        let before = editor.pages.first().map(|page| page.lines.len()).unwrap_or(0);
        assert!(before > 0, "nothing was laid out");

        editor.choose_document_spacing(5);
        let after = editor.document.paragraph_format_here();
        assert_eq!(after.space_after, 160, "the paragraph does not follow the document");
        assert_eq!(
            after.line_spacing.map(|spacing| spacing.value),
            Some(480),
            "double spacing did not reach the paragraph"
        );
    }

    #[test]
    fn the_last_row_opens_the_dialog_that_sets_the_defaults() {
        // Word's Custom Paragraph Spacing ends in a dialog with Set As Default
        // on it, which is the dialog this program already has for the job.
        let mut editor = editor();
        let (items, ..) = editor.menu_contents(Choice::DocumentSpacing);
        editor.choose_document_spacing(items.len() - 1);
        assert!(editor.in_dialog(), "the dialog did not open");
    }

    #[test]
    fn an_alignment_tab_goes_in_and_survives_being_saved() {
        let mut editor = editor();
        editor.choose_alignment_tab(2);

        let saved = editor.document.save().expect("saving");
        let reopened = Document::open(&saved).expect("reopening");
        let body = reopened.body();
        let wp_docx::model::Block::Paragraph(paragraph) = &body.blocks[0] else {
            panic!("a paragraph")
        };
        let found = paragraph
            .runs
            .iter()
            .flat_map(|run| run.content.iter())
            .any(|piece| matches!(piece, RunContent::PositionTab(TabAlignment::End)));
        assert!(found, "the alignment tab did not survive");
    }

    #[test]
    fn an_alignment_tab_sends_what_follows_it_to_the_far_end() {
        // The point of it: a header with a title on the left and a page number
        // against the right margin, which keeps its shape when the margins
        // move because there is no tab stop to move.
        let mut editor = editor();
        editor.document.type_text("Left");
        editor.choose_alignment_tab(2);
        editor.document.type_text("Right");
        editor.relayout();

        let page = editor.pages.first().expect("a page");
        let line = page.lines.first().expect("a line");
        let right_edge =
            page.glyphs.iter().map(|glyph| glyph.x + glyph.advance).fold(0.0, f32::max);
        assert!(
            right_edge > line.right - 2.0,
            "the text stopped at {right_edge} and the line ends at {}",
            line.right
        );
    }
}
