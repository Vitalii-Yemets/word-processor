//! Word's Font dialog: every character format there is, on two tabs.
//!
//! # Why it is worth a file of its own
//!
//! It is the largest dialog in the program and the only one where every field
//! means something different: a list of the fonts on the machine, a size that
//! can be typed or chosen, two colours, seven tick boxes, four measurements in
//! four different units, and five things asked of the font itself. Putting the
//! rows in one place and reading them back in the next function is what keeps
//! the two halves from drifting — a field moved in one and not the other is a
//! dialog that quietly applies the wrong thing.
//!
//! # How the rows are numbered
//!
//! By constant, never by a number written twice. The rows are the order of the
//! list [`Editor::font_dialog`] builds, and every one that is read back is
//! named below. A field added in the middle moves the ones after it, and the
//! constants are the only place that has to know.

use wp_docx::model::{ResolvedRunProperties, RunProperties, Underline, VerticalAlignment};
use wp_docx::typography::{Ligatures, NumberForms, NumberSpacing, OpenType, NORMAL_SCALE};
use wp_shell::Response;

use crate::chrome::dialog::{Answer, Button, Dialog, Field, Sample};
use crate::chrome::palette::TEXT_COLORS;

use super::dialogs::Asking;
use super::Editor;

// The Font tab. The markers that arrange the rows — a tab, a row of columns,
// a group's caption — are numbered along with everything else, because they
// take a place in the one list of fields.
const TAB_FONT: usize = 0;
const ROW_TYPEFACE: usize = 1;
const FONT: usize = 2;
const STYLE: usize = 3;
const SIZE: usize = 4;
const ROW_COLOUR: usize = 5;
const COLOR: usize = 6;
const UNDERLINE: usize = 7;
const UNDERLINE_COLOR: usize = 8;
const EFFECTS: usize = 9;
const ROW_EFFECT_ONE: usize = 10;
const STRIKE: usize = 11;
const SMALL_CAPS: usize = 12;
const ROW_EFFECT_TWO: usize = 13;
const DOUBLE_STRIKE: usize = 14;
const ALL_CAPS: usize = 15;
const ROW_EFFECT_THREE: usize = 16;
const SUPERSCRIPT: usize = 17;
const HIDDEN: usize = 18;
const SUBSCRIPT: usize = 19;
const PREVIEW_GROUP_FONT: usize = 20;
const PREVIEW_FONT: usize = 21;

// The Advanced tab.
const TAB_ADVANCED: usize = 22;
const SPACING_GROUP: usize = 23;
const ROW_SPACING_ONE: usize = 24;
const SCALE: usize = 25;
const SPACING: usize = 26;
const ROW_SPACING_TWO: usize = 27;
const POSITION: usize = 28;
const KERNING: usize = 29;
const OPENTYPE_GROUP: usize = 30;
const ROW_FEATURE_ONE: usize = 31;
const LIGATURES: usize = 32;
const NUMBER_SPACING: usize = 33;
const ROW_FEATURE_TWO: usize = 34;
const NUMBER_FORMS: usize = 35;
const STYLISTIC_SET: usize = 36;
const CONTEXTUAL: usize = 37;
const PREVIEW_GROUP_ADVANCED: usize = 38;
const PREVIEW_ADVANCED: usize = 39;

/// The answer that means "make this the default for new documents".
///
/// A third button beside OK and Cancel, as Word has: it applies what the dialog
/// says and then writes it into the style everything else inherits from.
pub(super) const SET_AS_DEFAULT: &str = "Set As Default";

/// The four weights Word offers, which are bold and italic in the four
/// combinations rather than four separate things.
const STYLES: &[&str] = &["Regular", "Italic", "Bold", "Bold Italic"];

/// The underline styles, in Word's order, with the model's name for each.
const UNDERLINES: &[(&str, Underline)] = &[
    ("(none)", Underline::None),
    ("Single", Underline::Single),
    ("Double", Underline::Double),
    ("Thick", Underline::Thick),
    ("Dotted", Underline::Dotted),
    ("Dashed", Underline::Dashed),
    ("Wave", Underline::Wave),
];

/// What the preview shows when there is nothing selected to show.
///
/// Word shows the name of the chosen font, which says two things at once: what
/// it is called and what it looks like.
fn sample_text(selected: &str, font: &str) -> String {
    let trimmed = selected.trim();
    if trimmed.is_empty() || trimmed.chars().count() > 40 {
        return font.to_owned();
    }
    trimmed.to_owned()
}

/// Where a name sits in a list, or the first row when it is not there at all.
fn position_of(items: &[String], wanted: &str) -> usize {
    items.iter().position(|item| item.eq_ignore_ascii_case(wanted)).unwrap_or(0)
}

impl Editor {
    /// Opens Word's Font dialog on whatever the caret or the selection has.
    pub(super) fn open_font_dialog(&mut self) -> Response {
        let dialog = self.font_dialog(&self.document.character_format_here());
        self.ask(Asking::Font, dialog)
    }

    /// The dialog itself, built from a set of formatting.
    ///
    /// Built again whenever a field changes, because the preview at the bottom
    /// of each tab has to show what the fields now say — which is the whole
    /// point of a preview.
    pub(super) fn font_dialog(&self, now: &ResolvedRunProperties) -> Dialog {
        let mut fonts: Vec<String> = self.families.clone();
        let chosen_font = now
            .font
            .clone()
            .unwrap_or_else(|| fonts.first().cloned().unwrap_or_else(|| "Calibri".to_owned()));
        // A document may ask for a font this machine has never had, and Word
        // still shows its name. The document says what it says: opening the
        // dialog must not quietly change the font to whatever happens to be
        // first in the list, which is what falling back to row zero would do.
        if !fonts.iter().any(|name| name.eq_ignore_ascii_case(&chosen_font)) {
            fonts.insert(0, chosen_font.clone());
        }

        let sizes: Vec<String> =
            crate::chrome::SIZES.iter().map(|value| crate::chrome::format_size(*value)).collect();
        let colours: Vec<String> =
            TEXT_COLORS.iter().map(|(label, _)| (*label).to_owned()).collect();
        let colour_of = |value: Option<&str>| {
            TEXT_COLORS
                .iter()
                .position(|(_, hex)| match (hex, value) {
                    (None, None) => true,
                    (Some(hex), Some(value)) => hex.eq_ignore_ascii_case(value),
                    _ => false,
                })
                .unwrap_or(0)
        };

        let weight = usize::from(now.bold) * 2 + usize::from(now.italic);
        let sample = Sample {
            text: sample_text(&self.document.selected_text(), &chosen_font),
            properties: Box::new(now.clone()),
        };

        let choice = |label: &str, items: Vec<String>, current: usize| Field::Choice {
            label: label.to_owned(),
            items,
            current,
        };
        let check = |label: &str, on: bool| Field::Check { label: label.to_owned(), on };

        let fields = vec![
            // --- Font ------------------------------------------------------
            // Word's arrangement: three across the top, three under them, the
            // effects in two columns inside a box, and the preview in a box of
            // its own at the foot.
            Field::Tab("Font".to_owned()),
            Field::Columns(3),
            choice("Font", fonts.clone(), position_of(&fonts, &chosen_font)),
            choice("Font style", STYLES.iter().map(|s| (*s).to_owned()).collect(), weight),
            choice("Size", sizes.clone(), position_of(&sizes, &format_points(now))),
            Field::Columns(3),
            choice("Font color", colours.clone(), colour_of(now.color.as_deref())),
            choice(
                "Underline style",
                UNDERLINES.iter().map(|(label, _)| (*label).to_owned()).collect(),
                UNDERLINES.iter().position(|(_, kind)| *kind == now.underline).unwrap_or(0),
            ),
            choice("Underline color", colours, colour_of(now.underline_color.as_deref())),
            Field::Group("Effects".to_owned()),
            Field::Columns(2),
            check("Strikethrough", now.strike),
            check("Small caps", now.small_caps),
            Field::Columns(2),
            check("Double strikethrough", now.double_strike),
            check("All caps", now.caps),
            Field::Columns(2),
            check("Superscript", now.vertical_align == VerticalAlignment::Superscript),
            check("Hidden", now.hidden),
            check("Subscript", now.vertical_align == VerticalAlignment::Subscript),
            Field::Group("Preview".to_owned()),
            Field::Preview(Box::new(sample.clone())),
            // --- Advanced --------------------------------------------------
            Field::Tab("Advanced".to_owned()),
            Field::Group("Character Spacing".to_owned()),
            Field::Columns(2),
            Field::Number { label: "Scale".to_owned(), value: now.scale.to_string(), unit: "%" },
            Field::Number {
                label: "Spacing".to_owned(),
                // Twentieths of a point in the file, points on the screen:
                // nobody types a twentieth of a point.
                value: format!("{:.2}", f64::from(now.spacing_twentieths) / 20.0),
                unit: "pt",
            },
            Field::Columns(2),
            Field::Number {
                label: "Position".to_owned(),
                value: format!("{:.1}", f64::from(now.position_half_points) / 2.0),
                unit: "pt",
            },
            Field::Number {
                label: "Kerning from".to_owned(),
                // Zero is Word's own way of saying "never", and a document that
                // says nothing gets the kerning this program has always drawn.
                value: format!("{:.1}", f64::from(now.kerning_half_points.unwrap_or(0)) / 2.0),
                unit: "pt",
            },
            Field::Group("OpenType Features".to_owned()),
            Field::Columns(2),
            choice(
                "Ligatures",
                Ligatures::CHOICES.iter().map(|kind| kind.label().to_owned()).collect(),
                Ligatures::CHOICES
                    .iter()
                    .position(|kind| *kind == now.open_type.ligatures)
                    .unwrap_or(0),
            ),
            choice(
                "Number spacing",
                NumberSpacing::CHOICES.iter().map(|kind| kind.label().to_owned()).collect(),
                NumberSpacing::CHOICES
                    .iter()
                    .position(|kind| *kind == now.open_type.number_spacing)
                    .unwrap_or(0),
            ),
            Field::Columns(2),
            choice(
                "Number forms",
                NumberForms::CHOICES.iter().map(|kind| kind.label().to_owned()).collect(),
                NumberForms::CHOICES
                    .iter()
                    .position(|kind| *kind == now.open_type.number_forms)
                    .unwrap_or(0),
            ),
            choice(
                "Stylistic sets",
                core::iter::once("Default".to_owned())
                    .chain((1..=20).map(|set| set.to_string()))
                    .collect(),
                now.open_type.stylistic_sets.first().map_or(0, |set| usize::from(*set)),
            ),
            check("Use contextual alternates", now.open_type.contextual_alternates),
            Field::Group("Preview".to_owned()),
            Field::Preview(Box::new(sample)),
        ];

        check_rows(&fields);
        // Wider than a plain dialog: three fields across need the room, and
        // Word's own Font dialog is wider than its Bookmark for the same
        // reason.
        Dialog::with_buttons(
            "Font",
            fields,
            vec![
                Button { label: "OK".to_owned(), answer: Answer::Accept, default: true },
                Button {
                    label: SET_AS_DEFAULT.to_owned(),
                    answer: Answer::Named(SET_AS_DEFAULT),
                    default: false,
                },
                Button { label: "Cancel".to_owned(), answer: Answer::Cancel, default: false },
            ],
        )
        .wide(480.0)
    }

    /// What the dialog's fields say, as formatting.
    ///
    /// Resolved rather than authored, so that the preview can be drawn from it
    /// and the same reading can be turned into a change. Every row that is read
    /// is named at the top of this file.
    pub(super) fn font_dialog_says(&self, dialog: &Dialog) -> ResolvedRunProperties {
        let weight = dialog.chose(STYLE);
        let colour = |row: usize| {
            TEXT_COLORS.get(dialog.chose(row)).and_then(|(_, hex)| hex.map(str::to_owned))
        };

        ResolvedRunProperties {
            font: Some(dialog.said(FONT)),
            bold: weight >= 2,
            italic: weight % 2 == 1,
            size_half_points: half_points(&dialog.said(SIZE)),
            color: colour(COLOR),
            underline: UNDERLINES
                .get(dialog.chose(UNDERLINE))
                .map_or(Underline::None, |(_, kind)| kind.clone()),
            underline_color: colour(UNDERLINE_COLOR),
            strike: dialog.ticked(STRIKE),
            double_strike: dialog.ticked(DOUBLE_STRIKE),
            // Word's two are one property with three values, and ticking one
            // unticks the other. Superscript wins if somebody ticks both, which
            // is the order Word reads them in.
            vertical_align: if dialog.ticked(SUPERSCRIPT) {
                VerticalAlignment::Superscript
            } else if dialog.ticked(SUBSCRIPT) {
                VerticalAlignment::Subscript
            } else {
                VerticalAlignment::Baseline
            },
            small_caps: dialog.ticked(SMALL_CAPS),
            caps: dialog.ticked(ALL_CAPS),
            hidden: dialog.ticked(HIDDEN),
            scale: number(&dialog.said(SCALE)).unwrap_or(f64::from(NORMAL_SCALE)).clamp(1.0, 600.0)
                as u32,
            spacing_twentieths: (number(&dialog.said(SPACING)).unwrap_or(0.0) * 20.0).round()
                as i32,
            position_half_points: (number(&dialog.said(POSITION)).unwrap_or(0.0) * 2.0).round()
                as i32,
            kerning_half_points: Some(
                (number(&dialog.said(KERNING)).unwrap_or(0.0).max(0.0) * 2.0).round() as u32,
            ),
            open_type: OpenType {
                ligatures: *Ligatures::CHOICES
                    .get(dialog.chose(LIGATURES))
                    .unwrap_or(&Ligatures::None),
                number_spacing: *NumberSpacing::CHOICES
                    .get(dialog.chose(NUMBER_SPACING))
                    .unwrap_or(&NumberSpacing::Default),
                number_forms: *NumberForms::CHOICES
                    .get(dialog.chose(NUMBER_FORMS))
                    .unwrap_or(&NumberForms::Default),
                // The first row of the list is "Default", which asks for no set
                // at all; the rest are the twenty a font may carry.
                stylistic_sets: match dialog.chose(STYLISTIC_SET) {
                    0 => Vec::new(),
                    set => vec![set as u8],
                },
                contextual_alternates: dialog.ticked(CONTEXTUAL),
            },
            // Not on this dialog: kept as they were so that answering it does
            // not quietly take away a highlight or a glow.
            highlight: self.document.character_format_here().highlight,
            effect: self.document.character_format_here().effect,
            right_to_left: self.document.character_format_here().right_to_left,
            language: self.document.character_format_here().language,
        }
    }

    /// Puts what the dialog says onto the selection.
    pub(super) fn apply_font_dialog(&mut self, dialog: &Dialog, as_default: bool) -> Response {
        let wanted = self.font_dialog_says(dialog);
        let change = authored(&wanted);

        let mut changed = self.document.set_character_format(&change);
        if as_default {
            // Word's Set As Default writes the formatting into the style every
            // other style is built from, so it reaches new documents and every
            // paragraph that never said otherwise.
            changed |= self.document.set_default_character_format(&change);
        }
        self.relayout();
        self.edited(changed, if as_default { "Default font" } else { "Font" })
    }
}

/// That the rows are where the constants at the top of this file say they are.
///
/// The whole file rests on it: a row inserted in the middle of the list moves
/// every row after it, and a dialog that then reads the wrong ones applies the
/// wrong formatting without ever looking wrong. Checked when the dialog is
/// built — a handful of comparisons against a click — rather than left to a
/// test, because the cost is nothing and the failure is silent.
fn check_rows(fields: &[Field]) {
    let wanted: &[(usize, &str)] = &[
        (TAB_FONT, "a tab"),
        (ROW_TYPEFACE, "a row"),
        (FONT, "a list"),
        (STYLE, "a list"),
        (SIZE, "a list"),
        (ROW_COLOUR, "a row"),
        (COLOR, "a list"),
        (UNDERLINE, "a list"),
        (UNDERLINE_COLOR, "a list"),
        (EFFECTS, "a group"),
        (ROW_EFFECT_ONE, "a row"),
        (STRIKE, "a tick box"),
        (SMALL_CAPS, "a tick box"),
        (ROW_EFFECT_TWO, "a row"),
        (DOUBLE_STRIKE, "a tick box"),
        (ALL_CAPS, "a tick box"),
        (ROW_EFFECT_THREE, "a row"),
        (SUPERSCRIPT, "a tick box"),
        (HIDDEN, "a tick box"),
        (SUBSCRIPT, "a tick box"),
        (PREVIEW_GROUP_FONT, "a group"),
        (PREVIEW_FONT, "a preview"),
        (TAB_ADVANCED, "a tab"),
        (SPACING_GROUP, "a group"),
        (ROW_SPACING_ONE, "a row"),
        (SCALE, "a number"),
        (SPACING, "a number"),
        (ROW_SPACING_TWO, "a row"),
        (POSITION, "a number"),
        (KERNING, "a number"),
        (OPENTYPE_GROUP, "a group"),
        (ROW_FEATURE_ONE, "a row"),
        (LIGATURES, "a list"),
        (NUMBER_SPACING, "a list"),
        (ROW_FEATURE_TWO, "a row"),
        (NUMBER_FORMS, "a list"),
        (STYLISTIC_SET, "a list"),
        (CONTEXTUAL, "a tick box"),
        (PREVIEW_GROUP_ADVANCED, "a group"),
        (PREVIEW_ADVANCED, "a preview"),
    ];
    crate::chrome::dialog::check_rows("Font", fields, wanted);
}

/// The size on the dialog, in the points a person types.
fn format_points(now: &ResolvedRunProperties) -> String {
    crate::chrome::format_size(now.size_points() as f32)
}

/// A typed size as the half-points the format stores.
fn half_points(said: &str) -> u32 {
    let points = number(said).unwrap_or(11.0);
    // Word's own limits: from one point to sixteen hundred and thirty-eight.
    ((points.clamp(1.0, 1638.0)) * 2.0).round() as u32
}

/// A number typed into a box, however it was typed.
///
/// A comma for a decimal point is what half the world types, and a box that
/// refuses it is a box that looks broken.
fn number(said: &str) -> Option<f64> {
    said.trim().replace(',', ".").parse().ok()
}

/// Resolved formatting as the authored properties that would produce it.
///
/// Everything is written out rather than left unsaid: a dialog is answered all
/// at once, and a field left unsaid would let a style put back the very thing
/// that was just turned off.
pub(super) fn authored(wanted: &ResolvedRunProperties) -> RunProperties {
    RunProperties {
        font: wanted.font.clone(),
        bold: Some(wanted.bold),
        italic: Some(wanted.italic),
        size_half_points: Some(wanted.size_half_points),
        color: wanted.color.clone(),
        underline: Some(wanted.underline.clone()),
        underline_color: wanted.underline_color.clone(),
        strike: Some(wanted.strike),
        double_strike: Some(wanted.double_strike),
        vertical_align: Some(wanted.vertical_align),
        small_caps: Some(wanted.small_caps),
        caps: Some(wanted.caps),
        hidden: Some(wanted.hidden),
        scale: Some(wanted.scale),
        spacing_twentieths: Some(wanted.spacing_twentieths),
        position_half_points: Some(wanted.position_half_points),
        kerning_half_points: wanted.kerning_half_points,
        open_type: Some(wanted.open_type.clone()),
        // A colour written out overrides a colour named after the theme, so the
        // name has to go with it — otherwise the theme puts the old one back.
        color_theme: None,
        font_theme: None,
        // Left alone: this dialog does not ask about them.
        highlight: None,
        effect: None,
        right_to_left: None,
        language: None,
        style: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sample_is_the_font_name_when_nothing_is_selected() {
        assert_eq!(sample_text("", "Cambria"), "Cambria");
        assert_eq!(sample_text("   ", "Cambria"), "Cambria");
    }

    #[test]
    fn the_sample_is_the_selection_when_there_is_one() {
        assert_eq!(sample_text("Chapter One", "Cambria"), "Chapter One");
    }

    #[test]
    fn a_selection_too_long_to_fit_falls_back_to_the_name() {
        let long = "a".repeat(80);
        assert_eq!(sample_text(&long, "Cambria"), "Cambria");
    }

    #[test]
    fn a_size_typed_with_a_comma_is_still_a_size() {
        // Half the world types a comma for a decimal point.
        assert_eq!(half_points("11,5"), 23);
        assert_eq!(half_points("11.5"), 23);
    }

    #[test]
    fn a_size_outside_what_word_allows_is_brought_back_inside() {
        assert_eq!(half_points("0"), 2);
        assert_eq!(half_points("99999"), 3276);
        // And something that is not a number at all falls back to Word's own
        // default rather than to nothing.
        assert_eq!(half_points("nonsense"), 22);
    }

    #[test]
    fn the_underlines_offered_are_the_ones_the_model_knows() {
        for (_, kind) in UNDERLINES {
            // Round-tripping through the file is what the document does with
            // whichever of these is chosen.
            assert_eq!(Underline::from_attribute(kind.to_attribute()), *kind);
        }
    }
}

#[cfg(test)]
mod editor_tests {
    use super::*;
    use crate::chrome::dialog::Field;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::{Document, TextPosition};
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event, Key, Modifiers};

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    /// An editor with one paragraph, all of it selected.
    fn editor() -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("The quick brown fox")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");

        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1200, height: 800 });
        editor.document.set_caret(TextPosition::new(0, 0));
        editor.document.extend_selection_to(TextPosition::new(0, 19));
        editor
    }

    /// Sets a tick box by row, the way a click on it would.
    fn tick(editor: &mut Editor, row: usize, on: bool) {
        if let Some(dialog) = &mut editor.dialog {
            if let Some(Field::Check { on: state, .. }) = dialog.fields.get_mut(row) {
                *state = on;
            }
        }
    }

    /// Types a number into one of the Advanced tab's boxes.
    fn type_number(editor: &mut Editor, row: usize, text: &str) {
        if let Some(dialog) = &mut editor.dialog {
            if let Some(Field::Number { value, .. }) = dialog.fields.get_mut(row) {
                *value = text.to_owned();
            }
        }
    }

    fn accept(editor: &mut Editor) {
        editor.handle(Event::KeyDown { key: Key::Enter, modifiers: Modifiers::default() });
    }

    #[test]
    fn the_dialog_opens_showing_what_the_selection_already_has() {
        let mut editor = editor();
        editor.document.set_format(wp_docx::CharacterFormat::Bold, true);
        editor.open_font_dialog();

        let dialog = editor.dialog.as_ref().expect("a dialog");
        // "Bold" is the third of Word's four weights.
        assert_eq!(dialog.chose(STYLE), 2);
    }

    #[test]
    fn the_effects_reach_the_document() {
        let mut editor = editor();
        editor.open_font_dialog();
        tick(&mut editor, SMALL_CAPS, true);
        tick(&mut editor, DOUBLE_STRIKE, true);
        accept(&mut editor);

        assert!(!editor.in_dialog(), "the dialog stayed up");
        let now = editor.document.character_format_here();
        assert!(now.small_caps, "small capitals did not reach the document");
        assert!(now.double_strike, "the second line did not reach the document");
    }

    #[test]
    fn the_advanced_tab_reaches_the_document_in_the_units_the_file_uses() {
        let mut editor = editor();
        editor.open_font_dialog();
        type_number(&mut editor, SCALE, "150");
        // Word takes points and the file stores twentieths of one.
        type_number(&mut editor, SPACING, "1.5");
        // And half-points for these two.
        type_number(&mut editor, POSITION, "3");
        type_number(&mut editor, KERNING, "8");
        accept(&mut editor);

        let now = editor.document.character_format_here();
        assert_eq!(now.scale, 150);
        assert_eq!(now.spacing_twentieths, 30);
        assert_eq!(now.position_half_points, 6);
        assert_eq!(now.kerning_half_points, Some(16));
    }

    #[test]
    fn what_the_dialog_applies_survives_being_saved_and_opened() {
        // The one thing that separates formatting from a picture of it.
        let mut editor = editor();
        editor.open_font_dialog();
        tick(&mut editor, ALL_CAPS, true);
        type_number(&mut editor, SCALE, "80");
        accept(&mut editor);

        let bytes = editor.document.save().expect("saving");
        let reopened = Document::open(&bytes).expect("reopening");
        let run = reopened.body().blocks.iter().find_map(|block| match block {
            Block::Paragraph(paragraph) => paragraph.runs.first().cloned(),
            _ => None,
        });
        let properties = run.expect("a run").properties;
        assert_eq!(properties.caps, Some(true));
        assert_eq!(properties.scale, Some(80));
    }

    #[test]
    fn cancelling_changes_nothing() {
        let mut editor = editor();
        let before = editor.document.character_format_here();
        editor.open_font_dialog();
        tick(&mut editor, ALL_CAPS, true);
        editor.handle(Event::KeyDown { key: Key::Escape, modifiers: Modifiers::default() });

        assert!(!editor.in_dialog());
        assert_eq!(editor.document.character_format_here(), before);
    }

    #[test]
    fn the_preview_follows_the_fields() {
        let mut editor = editor();
        editor.open_font_dialog();
        tick(&mut editor, ALL_CAPS, true);
        // A press on the tick box is what rebuilds the dialog; here the box is
        // set directly, so the rebuild is asked for the same way the press asks.
        editor.dialog_press(-1, -1);

        let dialog = editor.dialog.as_ref().expect("a dialog");
        match dialog.fields.get(PREVIEW_FONT) {
            Some(Field::Preview(sample)) => {
                assert!(sample.properties.caps, "the preview is not showing capitals");
            }
            other => panic!("row {PREVIEW_FONT} is {other:?}, not a preview"),
        }
    }

    #[test]
    fn ctrl_and_tab_walk_the_two_tabs() {
        let mut editor = editor();
        editor.open_font_dialog();
        // The keyboard starts on the first field of the Font tab.
        editor.dialog_key(Key::Tab, false, true);

        // On the Advanced tab the first field the keyboard can land on is
        // Scale, so what is typed next goes into it — which proves both that
        // the tab changed and that the keyboard went with it.
        for character in "150".chars() {
            editor.dialog_character(character);
        }
        let dialog = editor.dialog.as_ref().expect("a dialog");
        assert_eq!(dialog.said(SCALE), "100150", "the keyboard did not follow the tab");
    }

    #[test]
    fn set_as_default_reaches_the_styles_rather_than_only_the_selection() {
        let mut editor = editor();
        editor.open_font_dialog();
        type_number(&mut editor, SCALE, "120");

        let dialog = editor.dialog.clone().expect("a dialog");
        editor.apply_font_dialog(&dialog, true);

        // A paragraph that was never touched now has it too, because it comes
        // from the defaults every style is built on.
        let bytes = editor.document.save().expect("saving");
        let reopened = Document::open(&bytes).expect("reopening");
        let styles = String::from_utf8_lossy(
            reopened.package().part("word/styles.xml").expect("a styles part"),
        )
        .into_owned();
        assert!(styles.contains("docDefaults"), "there are no document defaults");
        assert!(styles.contains("w:w"), "the scale did not reach the defaults");
    }
}
