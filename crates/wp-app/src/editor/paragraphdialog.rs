//! Word's Paragraph dialog: where the lines of a paragraph sit, and where it
//! is allowed to break.
//!
//! # The two tabs, and why they are two
//!
//! Indents and Spacing is about where the text goes on the page. Line and Page
//! Breaks is about where the page is allowed to come between the lines. They
//! are separate questions, asked of the same paragraph, and Word keeps them
//! apart — which is worth copying, because a person looking for "keep this
//! heading with the paragraph under it" is not looking among the indents.
//!
//! # The preview is a shape, not text
//!
//! Word draws grey bars rather than words. That is the right choice and is
//! copied here: what is being set is where the lines begin and end and how far
//! apart they are, and bars show that at a glance where a wall of text would
//! hide it. The paragraphs either side are drawn faintly, because spacing is
//! only visible against something and an indent only against a margin.
//!
//! # How the rows are numbered
//!
//! By constant, as in [`super::fontdialog`], and checked when the dialog is
//! built. The markers that arrange the rows take places in the list too.

use wp_docx::model::{
    Alignment, LineRule, LineSpacing, ParagraphProperties, ResolvedParagraphProperties,
};
use wp_shell::Response;

use crate::chrome::dialog::{Answer, Button, Dialog, Field, ParagraphSample};
use crate::measure;

use super::dialogs::Asking;
use super::Editor;

// Indents and Spacing.
const TAB_GENERAL: usize = 0;
const GENERAL: usize = 1;
const ROW_GENERAL: usize = 2;
const ALIGNMENT: usize = 3;
const OUTLINE_LEVEL: usize = 4;
const INDENTATION: usize = 5;
const ROW_INDENT: usize = 6;
const INDENT_LEFT: usize = 7;
const INDENT_RIGHT: usize = 8;
const ROW_SPECIAL: usize = 9;
const SPECIAL: usize = 10;
const SPECIAL_BY: usize = 11;
const MIRROR: usize = 12;
const SPACING: usize = 13;
const ROW_SPACE: usize = 14;
const SPACE_BEFORE: usize = 15;
const SPACE_AFTER: usize = 16;
const ROW_LINES: usize = 17;
const LINE_SPACING: usize = 18;
const LINE_SPACING_AT: usize = 19;
const CONTEXTUAL: usize = 20;
const PREVIEW_GROUP_GENERAL: usize = 21;
const PREVIEW_GENERAL: usize = 22;

// Line and Page Breaks.
const TAB_BREAKS: usize = 23;
const PAGINATION: usize = 24;
const ROW_PAGE_ONE: usize = 25;
const WIDOW_CONTROL: usize = 26;
const KEEP_NEXT: usize = 27;
const ROW_PAGE_TWO: usize = 28;
const KEEP_LINES: usize = 29;
const PAGE_BREAK_BEFORE: usize = 30;
const EXCEPTIONS: usize = 31;
const ROW_EXCEPTIONS: usize = 32;
const SUPPRESS_LINE_NUMBERS: usize = 33;
const NO_HYPHENATION: usize = 34;
const PREVIEW_GROUP_BREAKS: usize = 35;
const PREVIEW_BREAKS: usize = 36;

/// Word's third and fourth buttons on this dialog.
pub(super) const TABS: &str = "Tabs…";
pub(super) const SET_AS_DEFAULT: &str = "Set As Default";

/// The alignments Word offers, in Word's order and by Word's names.
const ALIGNMENTS: &[(&str, Alignment)] = &[
    ("Left", Alignment::Start),
    ("Centered", Alignment::Center),
    ("Right", Alignment::End),
    ("Justified", Alignment::Both),
];

/// What Word's "Special" offers: nothing, a first line pushed in, or a first
/// line pulled out with the rest pushed in.
const SPECIALS: &[&str] = &["(none)", "First line", "Hanging"];

/// The line spacings Word offers. The first three are multiples of single, and
/// the last three take a measurement of their own.
const LINE_SPACINGS: &[&str] =
    &["Single", "1.5 lines", "Double", "At least", "Exactly", "Multiple"];

/// Where a spacing sits in that list, and what "At" then means.
///
/// Word's list mixes two things: three named multiples, and three rules that
/// take a number. Which of the six a paragraph has is worked out from the rule
/// the file stores and, for the automatic rule, from the multiple itself.
fn spacing_row(spacing: Option<LineSpacing>) -> (usize, String) {
    let Some(spacing) = spacing else { return (0, "1".to_owned()) };
    match spacing.rule {
        // 240ths of single spacing.
        LineRule::Auto => {
            let multiple = f64::from(spacing.value) / 240.0;
            let row = if (multiple - 1.0).abs() < 0.01 {
                0
            } else if (multiple - 1.5).abs() < 0.01 {
                1
            } else if (multiple - 2.0).abs() < 0.01 {
                2
            } else {
                5
            };
            (row, format!("{multiple:.2}"))
        }
        // Twentieths of a point, shown as the points a person types.
        LineRule::AtLeast => (3, format!("{:.1}", f64::from(spacing.value) / 20.0)),
        LineRule::Exact => (4, format!("{:.1}", f64::from(spacing.value) / 20.0)),
    }
}

/// The other way round: a row of the list and what was typed beside it.
fn spacing_of(row: usize, at: &str) -> Option<LineSpacing> {
    let number = at.trim().replace(',', ".").parse::<f64>().ok();
    match row {
        0 => Some(LineSpacing { value: 240, rule: LineRule::Auto }),
        1 => Some(LineSpacing { value: 360, rule: LineRule::Auto }),
        2 => Some(LineSpacing { value: 480, rule: LineRule::Auto }),
        3 => Some(LineSpacing {
            value: (number.unwrap_or(12.0).clamp(0.0, 1584.0) * 20.0).round() as i32,
            rule: LineRule::AtLeast,
        }),
        4 => Some(LineSpacing {
            value: (number.unwrap_or(12.0).clamp(0.0, 1584.0) * 20.0).round() as i32,
            rule: LineRule::Exact,
        }),
        5 => Some(LineSpacing {
            value: (number.unwrap_or(1.0).clamp(0.06, 132.0) * 240.0).round() as i32,
            rule: LineRule::Auto,
        }),
        _ => None,
    }
}

/// Points, for the spacing boxes.
///
/// Word measures these in points whatever the unit setting says, and so does
/// this: somebody who asked for centimetres did not ask for the space above a
/// paragraph in centimetres.
fn points(twips: i32) -> String {
    format!("{:.0}", f64::from(twips) / 20.0)
}

impl Editor {
    /// Opens Word's Paragraph dialog on the paragraph the caret is in.
    pub(super) fn open_paragraph_dialog(&mut self) -> Response {
        let dialog = self.paragraph_dialog(&self.document.paragraph_format_here());
        self.ask(Asking::Paragraph, dialog)
    }

    /// The dialog itself, built from a set of formatting.
    pub(super) fn paragraph_dialog(&self, now: &ResolvedParagraphProperties) -> Dialog {
        let choice = |label: &str, items: &[&str], current: usize| Field::Choice {
            label: label.to_owned(),
            items: items.iter().map(|item| (*item).to_owned()).collect(),
            current,
        };
        let check = |label: &str, on: bool| Field::Check { label: label.to_owned(), on };
        let number = |label: &str, value: String, unit: &'static str| Field::Number {
            label: label.to_owned(),
            value,
            unit,
        };

        // Word's "Special" is two properties in one list: a first line pushed
        // in is a positive first-line indent, and a hanging indent a negative
        // one. Which of the three it is comes from the sign.
        let (special, special_by) = if now.indent_first_line > 0 {
            (1, measure::format(now.indent_first_line, self.unit))
        } else if now.indent_first_line < 0 {
            (2, measure::format(-now.indent_first_line, self.unit))
        } else {
            (0, "0.00".to_owned())
        };

        let (spacing_row, spacing_at) = spacing_row(now.line_spacing);
        let outline = now.outline_level.map_or(0, |level| usize::from(level) + 1);
        let levels: Vec<String> = core::iter::once("Body Text".to_owned())
            .chain((1..=9).map(|level| format!("Level {level}")))
            .collect();
        let sample = ParagraphSample { properties: Box::new(now.clone()) };

        let fields = vec![
            // --- Indents and Spacing ---------------------------------------
            Field::Tab("Indents and Spacing".to_owned()),
            Field::Group("General".to_owned()),
            Field::Columns(2),
            choice(
                "Alignment",
                &ALIGNMENTS.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
                ALIGNMENTS.iter().position(|(_, kind)| *kind == now.alignment).unwrap_or(0),
            ),
            Field::Choice { label: "Outline level".to_owned(), items: levels, current: outline },
            Field::Group("Indentation".to_owned()),
            Field::Columns(2),
            number("Left", measure::format(now.indent_start, self.unit), self.unit.mark()),
            number("Right", measure::format(now.indent_end, self.unit), self.unit.mark()),
            Field::Columns(2),
            choice("Special", SPECIALS, special),
            number("By", special_by, self.unit.mark()),
            check("Mirror indents", now.mirror_indents),
            Field::Group("Spacing".to_owned()),
            Field::Columns(2),
            number("Before", points(now.space_before), "pt"),
            number("After", points(now.space_after), "pt"),
            Field::Columns(2),
            choice("Line spacing", LINE_SPACINGS, spacing_row),
            number("At", spacing_at, ""),
            check("Don't add space between paragraphs of the same style", now.contextual_spacing),
            Field::Group("Preview".to_owned()),
            Field::Shape(Box::new(sample.clone())),
            // --- Line and Page Breaks --------------------------------------
            Field::Tab("Line and Page Breaks".to_owned()),
            Field::Group("Pagination".to_owned()),
            Field::Columns(2),
            check("Widow/Orphan control", now.widow_control),
            check("Keep with next", now.keep_next),
            Field::Columns(2),
            check("Keep lines together", now.keep_lines),
            check("Page break before", now.page_break_before),
            Field::Group("Formatting exceptions".to_owned()),
            Field::Columns(2),
            check("Suppress line numbers", now.suppress_line_numbers),
            check("Don't hyphenate", now.no_hyphenation),
            Field::Group("Preview".to_owned()),
            Field::Shape(Box::new(sample)),
        ];

        check_rows(&fields);
        Dialog::with_buttons(
            "Paragraph",
            fields,
            vec![
                Button { label: "OK".to_owned(), answer: Answer::Accept, default: true },
                Button { label: TABS.to_owned(), answer: Answer::Named(TABS), default: false },
                Button {
                    label: SET_AS_DEFAULT.to_owned(),
                    answer: Answer::Named(SET_AS_DEFAULT),
                    default: false,
                },
                Button { label: "Cancel".to_owned(), answer: Answer::Cancel, default: false },
            ],
        )
        .wide(520.0)
    }

    /// What the dialog's fields say, as formatting.
    pub(super) fn paragraph_dialog_says(&self, dialog: &Dialog) -> ResolvedParagraphProperties {
        let now = self.document.paragraph_format_here();

        // "Special" and "By" are one property between them: a first line pushed
        // in is positive, a hanging indent negative, and neither is zero.
        let by = measure::parse(&dialog.said(SPECIAL_BY), self.unit).unwrap_or(0).abs();
        let indent_first_line = match dialog.chose(SPECIAL) {
            1 => by,
            2 => -by,
            _ => 0,
        };

        ResolvedParagraphProperties {
            alignment: ALIGNMENTS
                .get(dialog.chose(ALIGNMENT))
                .map_or(Alignment::Start, |(_, kind)| *kind),
            // Word counts Body Text as no level at all, which is what the file
            // means by saying nothing.
            outline_level: match dialog.chose(OUTLINE_LEVEL) {
                0 => None,
                level => Some((level - 1) as u8),
            },
            indent_start: measure::parse(&dialog.said(INDENT_LEFT), self.unit).unwrap_or(0),
            indent_end: measure::parse(&dialog.said(INDENT_RIGHT), self.unit).unwrap_or(0),
            indent_first_line,
            mirror_indents: dialog.ticked(MIRROR),
            space_before: measure::parse(&dialog.said(SPACE_BEFORE), measure::Unit::Points)
                .unwrap_or(0),
            space_after: measure::parse(&dialog.said(SPACE_AFTER), measure::Unit::Points)
                .unwrap_or(0),
            line_spacing: spacing_of(dialog.chose(LINE_SPACING), &dialog.said(LINE_SPACING_AT)),
            contextual_spacing: dialog.ticked(CONTEXTUAL),
            widow_control: dialog.ticked(WIDOW_CONTROL),
            keep_next: dialog.ticked(KEEP_NEXT),
            keep_lines: dialog.ticked(KEEP_LINES),
            page_break_before: dialog.ticked(PAGE_BREAK_BEFORE),
            suppress_line_numbers: dialog.ticked(SUPPRESS_LINE_NUMBERS),
            no_hyphenation: dialog.ticked(NO_HYPHENATION),
            // Not on this dialog: kept as they were, so answering it does not
            // quietly take a border or a list away.
            right_to_left: now.right_to_left,
            numbering: now.numbering,
            borders: now.borders,
            shading: now.shading,
            tab_stops: now.tab_stops,
        }
    }

    /// Puts what the dialog says onto the paragraphs the selection covers.
    pub(super) fn apply_paragraph_dialog(&mut self, dialog: &Dialog, as_default: bool) -> Response {
        let wanted = self.paragraph_dialog_says(dialog);
        let change = authored(&wanted);

        let mut changed = self.document.set_paragraph_format(&change);
        if as_default {
            changed |= self.document.set_default_paragraph_format(&change);
        }
        self.relayout();
        self.edited(changed, if as_default { "Default paragraph" } else { "Paragraph" })
    }
}

/// That the rows are where the constants at the top of this file say they are.
fn check_rows(fields: &[Field]) {
    crate::chrome::dialog::check_rows(
        "Paragraph",
        fields,
        &[
            (TAB_GENERAL, "a tab"),
            (GENERAL, "a group"),
            (ROW_GENERAL, "a row"),
            (ALIGNMENT, "a list"),
            (OUTLINE_LEVEL, "a list"),
            (INDENTATION, "a group"),
            (ROW_INDENT, "a row"),
            (INDENT_LEFT, "a number"),
            (INDENT_RIGHT, "a number"),
            (ROW_SPECIAL, "a row"),
            (SPECIAL, "a list"),
            (SPECIAL_BY, "a number"),
            (MIRROR, "a tick box"),
            (SPACING, "a group"),
            (ROW_SPACE, "a row"),
            (SPACE_BEFORE, "a number"),
            (SPACE_AFTER, "a number"),
            (ROW_LINES, "a row"),
            (LINE_SPACING, "a list"),
            (LINE_SPACING_AT, "a number"),
            (CONTEXTUAL, "a tick box"),
            (PREVIEW_GROUP_GENERAL, "a group"),
            (PREVIEW_GENERAL, "a shape"),
            (TAB_BREAKS, "a tab"),
            (PAGINATION, "a group"),
            (ROW_PAGE_ONE, "a row"),
            (WIDOW_CONTROL, "a tick box"),
            (KEEP_NEXT, "a tick box"),
            (ROW_PAGE_TWO, "a row"),
            (KEEP_LINES, "a tick box"),
            (PAGE_BREAK_BEFORE, "a tick box"),
            (EXCEPTIONS, "a group"),
            (ROW_EXCEPTIONS, "a row"),
            (SUPPRESS_LINE_NUMBERS, "a tick box"),
            (NO_HYPHENATION, "a tick box"),
            (PREVIEW_GROUP_BREAKS, "a group"),
            (PREVIEW_BREAKS, "a shape"),
        ],
    );
}

/// Resolved formatting as the authored properties that would produce it.
///
/// Everything is written out rather than left unsaid: a dialog is answered all
/// at once, and a property left unsaid would let a style put back the very
/// thing that was just turned off.
pub(super) fn authored(wanted: &ResolvedParagraphProperties) -> ParagraphProperties {
    ParagraphProperties {
        alignment: Some(wanted.alignment),
        outline_level: wanted.outline_level,
        indent_start: Some(wanted.indent_start),
        indent_end: Some(wanted.indent_end),
        indent_first_line: Some(wanted.indent_first_line),
        mirror_indents: Some(wanted.mirror_indents),
        space_before: Some(wanted.space_before),
        space_after: Some(wanted.space_after),
        line_spacing: wanted.line_spacing,
        contextual_spacing: Some(wanted.contextual_spacing),
        widow_control: Some(wanted.widow_control),
        keep_next: Some(wanted.keep_next),
        keep_lines: Some(wanted.keep_lines),
        page_break_before: Some(wanted.page_break_before),
        suppress_line_numbers: Some(wanted.suppress_line_numbers),
        no_hyphenation: Some(wanted.no_hyphenation),
        // Left alone: this dialog does not ask about them, and a style or the
        // paragraph itself has already said.
        style: None,
        right_to_left: None,
        numbering: None,
        borders: wp_docx::model::ParagraphBorders::default(),
        shading: None,
        tab_stops: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_three_named_multiples_are_recognised_as_themselves() {
        // Word's list has Single, 1.5 lines and Double as named rows, and
        // anything else as "Multiple" with the number beside it.
        assert_eq!(spacing_row(Some(LineSpacing { value: 240, rule: LineRule::Auto })).0, 0);
        assert_eq!(spacing_row(Some(LineSpacing { value: 360, rule: LineRule::Auto })).0, 1);
        assert_eq!(spacing_row(Some(LineSpacing { value: 480, rule: LineRule::Auto })).0, 2);
        assert_eq!(spacing_row(Some(LineSpacing { value: 300, rule: LineRule::Auto })).0, 5);
    }

    #[test]
    fn the_two_rules_that_take_a_measurement_show_it_in_points() {
        let (row, at) = spacing_row(Some(LineSpacing { value: 280, rule: LineRule::Exact }));
        assert_eq!(row, 4);
        assert_eq!(at, "14.0", "twentieths of a point are shown as points");
    }

    #[test]
    fn what_the_list_says_comes_back_as_what_the_file_stores() {
        // 1.3 rather than 1.5, because a multiple of exactly one and a half is
        // "1.5 lines" — Word shows it that way when the dialog is opened again,
        // and so does this. Only a multiple with no name of its own stays on
        // the Multiple row.
        for row in 0..LINE_SPACINGS.len() {
            let spacing = spacing_of(row, "1.3").expect("every row is a spacing");
            assert_eq!(spacing_row(Some(spacing)).0, row, "row {row} did not survive");
        }
    }

    #[test]
    fn a_multiple_that_has_a_name_is_shown_by_its_name() {
        // Setting Multiple 2 and opening the dialog again shows Double, which
        // is what Word does: they are the same spacing written the same way.
        let spacing = spacing_of(5, "2").expect("a spacing");
        assert_eq!(spacing_row(Some(spacing)).0, 2);
    }
}

#[cfg(test)]
mod editor_tests {
    use super::*;
    use crate::chrome::dialog::Field;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::Document;
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event, Key, Modifiers};

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    fn editor() -> Editor {
        let mut body = Body::default();
        for index in 0..4 {
            body.blocks.push(Block::Paragraph(Paragraph::text(&format!("Paragraph {index}"))));
        }
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1200, height: 800 });
        editor
    }

    fn type_number(editor: &mut Editor, row: usize, text: &str) {
        if let Some(dialog) = &mut editor.dialog {
            if let Some(Field::Number { value, .. }) = dialog.fields.get_mut(row) {
                *value = text.to_owned();
            }
        }
    }

    fn choose(editor: &mut Editor, row: usize, index: usize) {
        if let Some(dialog) = &mut editor.dialog {
            if let Some(Field::Choice { current, .. }) = dialog.fields.get_mut(row) {
                *current = index;
            }
        }
    }

    fn tick(editor: &mut Editor, row: usize, on: bool) {
        if let Some(dialog) = &mut editor.dialog {
            if let Some(Field::Check { on: state, .. }) = dialog.fields.get_mut(row) {
                *state = on;
            }
        }
    }

    fn accept(editor: &mut Editor) {
        editor.handle(Event::KeyDown { key: Key::Enter, modifiers: Modifiers::default() });
    }

    #[test]
    fn the_dialog_opens_showing_what_the_paragraph_already_has() {
        let mut editor = editor();
        editor.document.set_alignment_here(Alignment::Center);
        editor.open_paragraph_dialog();

        let dialog = editor.dialog.as_ref().expect("a dialog");
        // Centered is the second of the four alignments Word offers.
        assert_eq!(dialog.chose(ALIGNMENT), 1);
    }

    #[test]
    fn the_indents_reach_the_document_in_the_units_the_file_uses() {
        let mut editor = editor();
        editor.open_paragraph_dialog();
        type_number(&mut editor, INDENT_LEFT, "1");
        type_number(&mut editor, INDENT_RIGHT, "0.5");
        accept(&mut editor);

        let now = editor.document.paragraph_format_here();
        assert_eq!(now.indent_start, 1440, "an inch is 1440 twips");
        assert_eq!(now.indent_end, 720);
    }

    #[test]
    fn a_hanging_indent_is_the_first_line_pulled_out() {
        // The Special list is two properties in one: which of the three it is
        // comes from the sign of the first-line indent.
        let mut editor = editor();
        editor.open_paragraph_dialog();
        choose(&mut editor, SPECIAL, 2);
        type_number(&mut editor, SPECIAL_BY, "0.5");
        accept(&mut editor);

        assert_eq!(editor.document.paragraph_format_here().indent_first_line, -720);

        // And opening it again shows the same thing rather than a positive one.
        editor.open_paragraph_dialog();
        let dialog = editor.dialog.as_ref().expect("a dialog");
        assert_eq!(dialog.chose(SPECIAL), 2);
        assert_eq!(dialog.said(SPECIAL_BY), "0.50");
    }

    #[test]
    fn the_spacing_boxes_are_points_and_the_file_holds_twentieths() {
        let mut editor = editor();
        editor.open_paragraph_dialog();
        type_number(&mut editor, SPACE_BEFORE, "18");
        type_number(&mut editor, SPACE_AFTER, "6");
        accept(&mut editor);

        let now = editor.document.paragraph_format_here();
        assert_eq!(now.space_before, 360);
        assert_eq!(now.space_after, 120);
    }

    #[test]
    fn the_breaks_tab_reaches_the_document() {
        let mut editor = editor();
        editor.open_paragraph_dialog();
        tick(&mut editor, KEEP_NEXT, true);
        tick(&mut editor, PAGE_BREAK_BEFORE, true);
        tick(&mut editor, NO_HYPHENATION, true);
        accept(&mut editor);

        let now = editor.document.paragraph_format_here();
        assert!(now.keep_next);
        assert!(now.page_break_before);
        assert!(now.no_hyphenation);
    }

    #[test]
    fn what_the_dialog_applies_survives_being_saved_and_opened() {
        let mut editor = editor();
        editor.open_paragraph_dialog();
        tick(&mut editor, CONTEXTUAL, true);
        choose(&mut editor, LINE_SPACING, 2);
        accept(&mut editor);

        let bytes = editor.document.save().expect("saving");
        let reopened = Document::open(&bytes).expect("reopening");
        let properties = reopened
            .body()
            .blocks
            .iter()
            .find_map(|block| match block {
                Block::Paragraph(paragraph) => Some(paragraph.properties.clone()),
                _ => None,
            })
            .expect("a paragraph");

        assert_eq!(properties.contextual_spacing, Some(true));
        // Double is twice single, and single is 240.
        assert_eq!(properties.line_spacing.map(|spacing| spacing.value), Some(480));
    }

    #[test]
    fn cancelling_changes_nothing() {
        let mut editor = editor();
        let before = editor.document.paragraph_format_here();
        editor.open_paragraph_dialog();
        type_number(&mut editor, INDENT_LEFT, "2");
        editor.handle(Event::KeyDown { key: Key::Escape, modifiers: Modifiers::default() });

        assert!(!editor.in_dialog());
        assert_eq!(editor.document.paragraph_format_here(), before);
    }

    #[test]
    fn the_preview_follows_the_fields() {
        let mut editor = editor();
        editor.open_paragraph_dialog();
        choose(&mut editor, ALIGNMENT, 2);
        editor.dialog_press(-1, -1);

        let dialog = editor.dialog.as_ref().expect("a dialog");
        match dialog.fields.get(PREVIEW_GENERAL) {
            Some(Field::Shape(sample)) => {
                assert_eq!(sample.properties.alignment, Alignment::End);
            }
            other => panic!("row {PREVIEW_GENERAL} is {other:?}, not a shape"),
        }
    }

    #[test]
    fn set_as_default_reaches_the_styles_rather_than_only_this_paragraph() {
        let mut editor = editor();
        editor.open_paragraph_dialog();
        type_number(&mut editor, SPACE_AFTER, "10");

        let dialog = editor.dialog.clone().expect("a dialog");
        editor.apply_paragraph_dialog(&dialog, true);

        let bytes = editor.document.save().expect("saving");
        let reopened = Document::open(&bytes).expect("reopening");
        let styles = String::from_utf8_lossy(
            reopened.package().part("word/styles.xml").expect("a styles part"),
        )
        .into_owned();
        assert!(styles.contains("pPrDefault"), "there are no paragraph defaults");
        assert!(styles.contains("200"), "the spacing did not reach the defaults");
    }
}
