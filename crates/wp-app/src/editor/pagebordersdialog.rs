//! Word's Borders and Shading, on its Page Border tab.
//!
//! # Why this and not the list of edges
//!
//! The Page Borders button used to drop the same short list a paragraph border
//! drops — top, bottom, all, none — and put a border round the *paragraph*.
//! That is the wrong thing twice over: the button says page, and a page border
//! is not made of the same decisions. A paragraph border has edges and a
//! colour; a page border has those and three more that only a page can have —
//! which pages of the section carry it, how far in from the paper it sits, and
//! whether that distance is measured from the paper or from the text.
//!
//! # What is here and what is not
//!
//! Word's tab has Setting (None, Box, Shadow, 3-D, Custom), a list of line
//! styles, a colour, a width, an Art gallery, Apply to, and an Options button
//! for the distance. All of it is here. Word's own Options is folded into the
//! dialog rather than hidden behind a second one: there are two fields in it.
//!
//! The Art gallery offers the part of Word's list that is a pattern rather than
//! a picture — a row of black squares, a checkerboard, a Greek wave — because
//! the pictures are artwork Word ships and this program draws nothing it did
//! not make. See [`wp_docx::art`]. A document that arrives carrying one of the
//! pictures keeps it: the list shows it, says it is kept, and drawing it is a
//! plain line of its width.
//!
//! # Why the width list changes under it
//!
//! Because `w:sz` means eighths of a point for a line and whole points for a
//! border of art — the same attribute, two units. A list that said "1 pt" and
//! wrote a border eight times that is a list that lies, so picking a pattern
//! swaps the widths for the ones art is measured in, and picking a line style
//! swaps them back. The width in front of a person's eyes stays the width they
//! chose: only the unit under it moves.
//!
//! Shadow and 3-D are not other kinds of box. They are the same box with
//! something else said about how its lines are drawn, which is how the format
//! has it too — `w:shadow` and `w:frame` on each edge — and why picking one of
//! them here ticks the same four edges Box does.

use wp_docx::art;
use wp_docx::model::Border;
use wp_docx::pageborders::{Display, PageBorders, FURTHEST, USUAL_DISTANCE};
use wp_shell::Response;

use crate::chrome::dialog::{Dialog, Field};

use super::dialogs::Asking;
use super::Editor;

const SETTING: usize = 0;
const EDGES: usize = 1;
const EDGES_ONE: usize = 2;
const TOP: usize = 3;
const BOTTOM: usize = 4;
const EDGES_TWO: usize = 5;
const LEFT: usize = 6;
const RIGHT: usize = 7;
const LINE: usize = 8;
const STYLE: usize = 9;
const COLOUR: usize = 10;
const WIDTH: usize = 11;
pub(super) const ART: usize = 12;
const WHERE: usize = 13;
const APPLIES: usize = 14;
const DISPLAY: usize = 15;
const MEASURED: usize = 16;
const DISTANCE: usize = 17;

/// The first row of the Art list, which is no art at all.
const ART_NONE: usize = 0;

/// Word's Setting column.
const SETTINGS: &[&str] = &["None", "Box", "Shadow", "3-D", "Custom"];
const SETTING_NONE: usize = 0;
const SETTING_BOX: usize = 1;
const SETTING_SHADOW: usize = 2;
const SETTING_3D: usize = 3;

/// The line styles Word lists, by the names the file uses for them.
///
/// Word's own list, in Word's own order, and all of it: its dialog shows each
/// as a picture of the line rather than by name, and the names here are what
/// those pictures are called elsewhere in Word's own interface.
const STYLES: &[(&str, &str)] = &[
    ("Solid", "single"),
    ("Dotted", "dotted"),
    ("Dashed", "dashed"),
    ("Dashed, small gap", "dashSmallGap"),
    ("Dash dot", "dotDash"),
    ("Dash dot dot", "dotDotDash"),
    ("Dash dot, stroked", "dashDotStroked"),
    ("Double", "double"),
    ("Triple", "triple"),
    ("Thin thick, small gap", "thinThickSmallGap"),
    ("Thick thin, small gap", "thickThinSmallGap"),
    ("Thin thick thin, small gap", "thinThickThinSmallGap"),
    ("Thin thick, medium gap", "thinThickMediumGap"),
    ("Thick thin, medium gap", "thickThinMediumGap"),
    ("Thin thick thin, medium gap", "thinThickThinMediumGap"),
    ("Thin thick, large gap", "thinThickLargeGap"),
    ("Thick thin, large gap", "thickThinLargeGap"),
    ("Thin thick thin, large gap", "thinThickThinLargeGap"),
    ("Wave", "wave"),
    ("Double wave", "doubleWave"),
    ("Emboss", "threeDEmboss"),
    ("Engrave", "threeDEngrave"),
    ("Outset", "outset"),
    ("Inset", "inset"),
    ("Thick", "thick"),
];

/// The widths Word offers, in eighths of a point, which is the unit `w:sz`
/// uses.
const WIDTHS: &[(&str, u32)] = &[
    ("¼ pt", 2),
    ("½ pt", 4),
    ("¾ pt", 6),
    ("1 pt", 8),
    ("1½ pt", 12),
    ("2¼ pt", 18),
    ("3 pt", 24),
    ("4½ pt", 36),
    ("6 pt", 48),
];

/// And the widths an art border comes in, in whole points — the unit `w:sz`
/// uses for one, and as far as it goes.
const ART_WIDTHS: &[(&str, u32)] = &[
    ("4 pt", 4),
    ("6 pt", 6),
    ("8 pt", 8),
    ("10 pt", 10),
    ("12 pt", 12),
    ("15 pt", 15),
    ("20 pt", art::USUAL_WIDTH),
    ("25 pt", 25),
    ("31 pt", art::WIDEST),
];

/// The colours, by the names Word's list uses.
const COLOURS: &[(&str, Option<&str>)] = &[
    ("Automatic", None),
    ("Black", Some("000000")),
    ("Grey", Some("808080")),
    ("Blue", Some("2B579A")),
    ("Red", Some("C00000")),
    ("Green", Some("548235")),
    ("Orange", Some("ED7D31")),
];

/// Word's Apply to, on the page tab.
const SCOPES: &[&str] = &["Whole document", "This section"];
const SCOPE_DOCUMENT: usize = 0;

/// And which pages of it, which Word puts on the same list.
const DISPLAYS: &[(&str, Display)] = &[
    ("All pages", Display::AllPages),
    ("First page only", Display::FirstPage),
    ("All except first page", Display::NotFirstPage),
];

const MEASURED_FROM: &[&str] = &["Edge of page", "Text"];

impl Editor {
    /// Opens it on the caret's section.
    pub(super) fn open_page_borders(&mut self) -> Response {
        let borders = self.document.page_borders();
        let dialog = self.page_borders_dialog(&borders);
        self.ask(Asking::PageBorders, dialog)
    }

    /// The dialog itself, filled in from what the section says now.
    pub(super) fn page_borders_dialog(&mut self, borders: &PageBorders) -> Dialog {
        let line = first_line(borders);
        let tick = |label: &str, on: bool| Field::Check { label: label.to_owned(), on };

        let setting = if borders.is_empty() {
            SETTING_NONE
        } else if borders.top.is_some()
            && borders.bottom.is_some()
            && borders.start.is_some()
            && borders.end.is_some()
        {
            // A box all the way round, and which of the three it is depends on
            // what its lines say about themselves.
            if line.shadow {
                SETTING_SHADOW
            } else if line.frame {
                SETTING_3D
            } else {
                SETTING_BOX
            }
        } else {
            SETTINGS.len() - 1
        };

        let fields = vec![
            Field::Choice {
                label: "Setting".to_owned(),
                items: SETTINGS.iter().map(|name| (*name).to_owned()).collect(),
                current: setting,
            },
            // The four edges, two by two, which is Word's preview turned into
            // something that can be reached from a keyboard.
            Field::Group("Edges".to_owned()),
            Field::Columns(2),
            tick("Top", borders.top.is_some()),
            tick("Bottom", borders.bottom.is_some()),
            Field::Columns(2),
            tick("Left", borders.start.is_some()),
            tick("Right", borders.end.is_some()),
            Field::Group("Line".to_owned()),
            Field::Choice {
                label: "Style".to_owned(),
                items: STYLES.iter().map(|(name, _)| (*name).to_owned()).collect(),
                current: STYLES.iter().position(|(_, kind)| *kind == line.style).unwrap_or(0),
            },
            Field::Choice {
                label: "Color".to_owned(),
                items: COLOURS.iter().map(|(name, _)| (*name).to_owned()).collect(),
                current: COLOURS
                    .iter()
                    .position(|(_, value)| value.map(str::to_owned) == line.color)
                    .unwrap_or(0),
            },
            Field::Choice {
                label: "Width".to_owned(),
                items: widths_for(line.is_art())
                    .iter()
                    .map(|(name, _)| (*name).to_owned())
                    .collect(),
                current: width_row(line.is_art(), line.size),
            },
            Field::Choice {
                label: "Art".to_owned(),
                items: art_items(&line),
                current: art_row(&line),
            },
            Field::Group("Where it goes".to_owned()),
            Field::Choice {
                label: "Apply to".to_owned(),
                items: SCOPES.iter().map(|name| (*name).to_owned()).collect(),
                current: SCOPE_DOCUMENT,
            },
            Field::Choice {
                label: "On".to_owned(),
                items: DISPLAYS.iter().map(|(name, _)| (*name).to_owned()).collect(),
                current: DISPLAYS
                    .iter()
                    .position(|(_, kind)| *kind == borders.display)
                    .unwrap_or(0),
            },
            Field::Choice {
                label: "Measure from".to_owned(),
                items: MEASURED_FROM.iter().map(|name| (*name).to_owned()).collect(),
                current: usize::from(borders.from_text),
            },
            Field::Number {
                label: "Distance".to_owned(),
                value: borders.distance.to_string(),
                unit: "pt",
            },
        ];

        crate::chrome::dialog::check_rows(
            "Borders and Shading",
            &fields,
            &[
                (SETTING, "a list"),
                (EDGES, "a group"),
                (EDGES_ONE, "a row"),
                (TOP, "a tick box"),
                (BOTTOM, "a tick box"),
                (EDGES_TWO, "a row"),
                (LEFT, "a tick box"),
                (RIGHT, "a tick box"),
                (LINE, "a group"),
                (STYLE, "a list"),
                (COLOUR, "a list"),
                (WIDTH, "a list"),
                (ART, "a list"),
                (WHERE, "a group"),
                (APPLIES, "a list"),
                (DISPLAY, "a list"),
                (MEASURED, "a list"),
                (DISTANCE, "a number"),
            ],
        );

        // What the two lists that both write `w:val` were showing when the
        // dialog was built, so that a change to one can be told from a change
        // to the other.
        self.page_border_lists =
            (STYLES.iter().position(|(_, kind)| *kind == line.style).unwrap_or(0), art_row(&line));

        Dialog::new("Borders and Shading", fields).wide(460.0)
    }

    /// Keeps the Style list, the Art list and the widths agreeing.
    ///
    /// They are three ways of saying two attributes, and Word keeps them in
    /// step as they are touched: picking a pattern is picking a border, so the
    /// line style stops being what is drawn, and picking a line style puts the
    /// art back to none. The width list follows, because its unit belongs to
    /// whichever kind is now chosen.
    pub(super) fn page_borders_changed(&mut self) {
        let (was_style, was_art) = self.page_border_lists;
        let Some(dialog) = self.dialog.as_mut() else { return };
        let (style_now, art_now) = (dialog.chose(STYLE), dialog.chose(ART));
        if style_now == was_style && art_now == was_art {
            // Something else on the dialog: an edge ticked, a distance typed.
            return;
        }

        // The one just touched is the one meant.
        let art_row = if art_now != was_art { art_now } else { ART_NONE };
        if art_row != art_now {
            if let Some(Field::Choice { current, .. }) = dialog.fields.get_mut(ART) {
                *current = art_row;
            }
        }

        // The width keeps the size it is showing and changes the unit under it,
        // so that picking a pattern does not quietly make the border eight
        // times what it said.
        let (was_art_kind, is_art_kind) = (was_art != ART_NONE, art_row != ART_NONE);
        if was_art_kind != is_art_kind {
            let chosen = widths_for(was_art_kind)
                .get(dialog.chose(WIDTH))
                .map_or(art::USUAL_WIDTH, |(_, size)| *size);
            let size = if is_art_kind { (chosen / 8).max(1) } else { chosen * 8 };
            if let Some(Field::Choice { items, current, .. }) = dialog.fields.get_mut(WIDTH) {
                *items =
                    widths_for(is_art_kind).iter().map(|(name, _)| (*name).to_owned()).collect();
                *current = width_row(is_art_kind, size);
            }
        }

        self.page_border_lists = (style_now, art_row);
    }

    /// Takes what the dialog says and puts it round the pages.
    pub(super) fn apply_page_borders(&mut self, dialog: &Dialog) -> Response {
        let setting = dialog.chose(SETTING);
        // A pattern picked out of the Art gallery is the border: it and the line
        // style are the same attribute, and the gallery is the one that has
        // something to say when it is not at "(none)".
        let held = first_line(&self.document.page_borders());
        let art = art_style(dialog.chose(ART), &held);
        let line = Border::line(
            art.as_deref()
                .unwrap_or_else(|| STYLES.get(dialog.chose(STYLE)).map_or("single", |(_, k)| k)),
            widths_for(art.is_some()).get(dialog.chose(WIDTH)).map_or(4, |(_, size)| *size),
            COLOURS.get(dialog.chose(COLOUR)).and_then(|(_, value)| *value),
        )
        // Shadow and 3-D are not other kinds of box: they are the same box with
        // something else said about how its lines are drawn, which is exactly
        // how the format has it — an attribute on each edge.
        .with_effect(setting == SETTING_SHADOW, setting == SETTING_3D);

        // Word's Setting column and its four edges say the same thing two ways,
        // and the one that was touched last is the one that means it. None
        // clears everything; Box, Shadow and 3-D tick everything; Custom leaves
        // the ticks as they are, which is what makes it custom.
        let (top, bottom, left, right) = match setting {
            SETTING_NONE => (false, false, false, false),
            SETTING_BOX | SETTING_SHADOW | SETTING_3D => (true, true, true, true),
            _ => (
                dialog.ticked(TOP),
                dialog.ticked(BOTTOM),
                dialog.ticked(LEFT),
                dialog.ticked(RIGHT),
            ),
        };

        let edge = |on: bool| on.then(|| line.clone());
        let wanted = PageBorders {
            top: edge(top),
            bottom: edge(bottom),
            start: edge(left),
            end: edge(right),
            display: DISPLAYS
                .get(dialog.chose(DISPLAY))
                .map_or(Display::AllPages, |(_, kind)| *kind),
            from_text: dialog.chose(MEASURED) == 1,
            distance: dialog
                .said(DISTANCE)
                .trim()
                .parse::<u32>()
                .unwrap_or(USUAL_DISTANCE)
                .min(FURTHEST),
        };

        let changed = if dialog.chose(APPLIES) == SCOPE_DOCUMENT {
            self.document.set_page_borders_everywhere(&wanted)
        } else {
            self.document.set_page_borders(&wanted)
        };

        self.relayout();
        self.edited(changed, if wanted.is_empty() { "Page border removed" } else { "Page border" })
    }
}

/// Which widths belong to a border of art, and which to a line.
fn widths_for(art: bool) -> &'static [(&'static str, u32)] {
    if art {
        ART_WIDTHS
    } else {
        WIDTHS
    }
}

/// Which row of that list a width is on: the first that is at least as wide.
fn width_row(art: bool, size: u32) -> usize {
    let widths = widths_for(art);
    widths.iter().position(|(_, offered)| *offered >= size).unwrap_or(widths.len() - 1)
}

/// The Art list: nothing, then the patterns, and then — only for a document
/// that already carries one — the picture it came with.
///
/// The last row is what keeps such a document safe. Without it the list would
/// say "(none)", and pressing OK would quietly turn somebody's border of apples
/// into a plain line.
fn art_items(line: &Border) -> Vec<String> {
    let mut items = vec!["(none)".to_owned()];
    items.extend(art::DRAWN.iter().map(|(label, _)| (*label).to_owned()));
    if line.is_art() && !art::is_drawn(&line.style) {
        items.push(format!("{} (kept, drawn as a line)", line.style));
    }
    items
}

/// Which row of it a border is on.
fn art_row(line: &Border) -> usize {
    if !line.is_art() {
        return ART_NONE;
    }
    art::DRAWN
        .iter()
        .position(|(_, name)| *name == line.style)
        .map_or(art::DRAWN.len() + 1, |at| at + 1)
}

/// And which art a row means, if it means one at all.
fn art_style(row: usize, held: &Border) -> Option<String> {
    let at = row.checked_sub(1)?;
    match art::DRAWN.get(at) {
        Some((_, name)) => Some((*name).to_owned()),
        // Past the end of the patterns is the row that says "kept", and what it
        // keeps is whatever the document already had.
        None => held.is_art().then(|| held.style.clone()),
    }
}

/// The line the dialog opens showing.
///
/// Whichever edge is drawn, because Word's dialog has one style, one colour and
/// one width for all four — a page bordered differently on each side is not
/// something its dialog can say, and this one does not pretend otherwise.
fn first_line(borders: &PageBorders) -> Border {
    [&borders.top, &borders.start, &borders.bottom, &borders.end]
        .into_iter()
        .flatten()
        .find(|border| border.is_visible())
        .cloned()
        .unwrap_or(Border::line("single", 4, None))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::Document;
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event};

    /// Where a style sits on the list, found by the name the file uses.
    ///
    /// By name rather than by number, so that a style added to the list does
    /// not quietly make a test about double borders a test about dashed ones.
    fn style_row(kind: &str) -> usize {
        STYLES.iter().position(|(_, found)| *found == kind).expect("a style")
    }

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    fn editor() -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("A page to put a border round")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor
    }

    /// Answers the dialog as pressing OK does.
    fn accept(editor: &mut Editor) {
        let dialog = editor.dialog.take().expect("a dialog");
        editor.asking = None;
        editor.apply_page_borders(&dialog);
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

    #[test]
    fn a_box_goes_round_the_pages_and_not_round_the_paragraph() {
        let mut editor = editor();
        editor.open_page_borders();
        choose(&mut editor, SETTING, SETTING_BOX);
        accept(&mut editor);

        let borders = editor.document.page_borders();
        assert!(!borders.is_empty(), "no page border was written");
        assert!(borders.top.is_some() && borders.end.is_some());

        // And the paragraph was left alone, which is the whole point.
        let body = editor.document.body();
        let Block::Paragraph(paragraph) = &body.blocks[0] else { panic!("a paragraph") };
        assert!(paragraph.properties.borders.is_empty(), "it bordered the paragraph");
    }

    #[test]
    fn the_setting_and_the_ticks_agree_when_it_is_reopened() {
        let mut editor = editor();
        editor.open_page_borders();
        choose(&mut editor, SETTING, SETTING_BOX);
        accept(&mut editor);

        editor.open_page_borders();
        let dialog = editor.dialog.as_ref().expect("a dialog");
        assert_eq!(dialog.chose(SETTING), SETTING_BOX);
        assert!(dialog.ticked(TOP) && dialog.ticked(BOTTOM));
    }

    #[test]
    fn custom_keeps_the_edges_that_were_ticked() {
        let mut editor = editor();
        editor.open_page_borders();
        choose(&mut editor, SETTING, SETTINGS.len() - 1);
        tick(&mut editor, TOP, true);
        tick(&mut editor, BOTTOM, true);
        tick(&mut editor, LEFT, false);
        tick(&mut editor, RIGHT, false);
        accept(&mut editor);

        let borders = editor.document.page_borders();
        assert!(borders.top.is_some() && borders.bottom.is_some());
        assert!(borders.start.is_none() && borders.end.is_none(), "the sides went on anyway");
    }

    #[test]
    fn none_takes_the_border_off_again() {
        let mut editor = editor();
        editor.open_page_borders();
        choose(&mut editor, SETTING, SETTING_BOX);
        accept(&mut editor);
        assert!(!editor.document.page_borders().is_empty());

        editor.open_page_borders();
        choose(&mut editor, SETTING, SETTING_NONE);
        accept(&mut editor);
        assert!(editor.document.page_borders().is_empty(), "the border stayed");
    }

    #[test]
    fn the_style_the_colour_and_the_width_reach_the_file() {
        let mut editor = editor();
        editor.open_page_borders();
        choose(&mut editor, SETTING, SETTING_BOX);
        choose(&mut editor, STYLE, style_row("double"));
        choose(&mut editor, COLOUR, 4);
        choose(&mut editor, WIDTH, 5);
        accept(&mut editor);

        let saved = editor.document.save().expect("saving");
        let reopened = Document::open(&saved).expect("reopening");
        let top = reopened.page_borders().top.expect("a top border");
        assert_eq!(top.style, "double");
        assert_eq!(top.color.as_deref(), Some("C00000"));
        assert_eq!(top.size, 18);
    }

    #[test]
    fn which_pages_and_how_far_in_survive_being_saved() {
        let mut editor = editor();
        editor.open_page_borders();
        choose(&mut editor, SETTING, SETTING_BOX);
        choose(&mut editor, DISPLAY, 1);
        choose(&mut editor, MEASURED, 1);
        if let Some(dialog) = &mut editor.dialog {
            if let Some(Field::Number { value, .. }) = dialog.fields.get_mut(DISTANCE) {
                *value = "10".to_owned();
            }
        }
        accept(&mut editor);

        let saved = editor.document.save().expect("saving");
        let borders = Document::open(&saved).expect("reopening").page_borders();
        assert_eq!(borders.display, Display::FirstPage);
        assert!(borders.from_text);
        assert_eq!(borders.distance, 10);
    }

    #[test]
    fn a_distance_further_than_the_file_allows_is_brought_back_in() {
        // `w:space` reaches thirty-one points and no further, and Word's own
        // dialog will not take more either.
        let mut editor = editor();
        editor.open_page_borders();
        choose(&mut editor, SETTING, SETTING_BOX);
        if let Some(dialog) = &mut editor.dialog {
            if let Some(Field::Number { value, .. }) = dialog.fields.get_mut(DISTANCE) {
                *value = "99".to_owned();
            }
        }
        accept(&mut editor);
        assert_eq!(editor.document.page_borders().distance, FURTHEST);
    }

    #[test]
    fn the_border_is_drawn_on_the_page() {
        // The point of the whole item: what is written has to appear on the
        // paper, and a page border is four lines nothing else would draw.
        let mut editor = editor();
        let before = editor.pages.first().map_or(0, |page| page.decorations.len());

        editor.open_page_borders();
        choose(&mut editor, SETTING, SETTING_BOX);
        accept(&mut editor);

        let after = editor.pages.first().map_or(0, |page| page.decorations.len());
        assert_eq!(after, before + 4, "four edges were not drawn");
    }

    #[test]
    fn a_style_is_drawn_as_that_style_and_not_as_a_line() {
        // A dotted border and a solid one of the same width are the same
        // amount of ink and a different thing to look at. If the style is only
        // written down and not drawn, the list offering five of them is four
        // rows of lie.
        let mut editor = editor();

        editor.open_page_borders();
        choose(&mut editor, SETTING, SETTING_BOX);
        accept(&mut editor);
        let solid = editor.pages.first().map_or(0, |page| page.decorations.len());

        editor.open_page_borders();
        choose(&mut editor, SETTING, SETTING_BOX);
        choose(&mut editor, STYLE, style_row("dotted"));
        accept(&mut editor);
        let dotted = editor.pages.first().map_or(0, |page| page.decorations.len());
        assert!(dotted > solid, "a dotted border came out as four solid lines");

        editor.open_page_borders();
        choose(&mut editor, SETTING, SETTING_BOX);
        choose(&mut editor, STYLE, style_row("double"));
        accept(&mut editor);
        let double = editor.pages.first().map_or(0, |page| page.decorations.len());
        assert_eq!(double, solid * 2, "a double border is two lines an edge");
    }

    #[test]
    fn every_style_word_lists_is_one_the_file_knows() {
        // A name the format does not have would be written into the document
        // and come back drawn as a plain line, which is a row of the list that
        // looks like a choice and is not one.
        for (label, kind) in STYLES {
            assert!(!kind.is_empty(), "{label} has no name in the file");
            assert!(
                kind.chars().all(char::is_alphanumeric),
                "{label} is written as {kind}, which is not one of the format's names"
            );
        }
    }

    #[test]
    fn a_shadow_and_a_frame_reach_the_file_and_come_back() {
        for (setting, shadow, frame) in
            [(SETTING_BOX, false, false), (SETTING_SHADOW, true, false), (SETTING_3D, false, true)]
        {
            let mut editor = editor();
            editor.open_page_borders();
            choose(&mut editor, SETTING, setting);
            accept(&mut editor);

            let saved = editor.document.save().expect("saving");
            let reopened = Document::open(&saved).expect("reopening");
            let top = reopened.page_borders().top.expect("a top border");
            assert_eq!(top.shadow, shadow, "setting {setting}");
            assert_eq!(top.frame, frame, "setting {setting}");
        }
    }

    #[test]
    fn the_dialog_opens_on_the_setting_the_border_is() {
        let mut editor = editor();
        editor.open_page_borders();
        choose(&mut editor, SETTING, SETTING_SHADOW);
        accept(&mut editor);

        editor.open_page_borders();
        let dialog = editor.dialog.as_ref().expect("a dialog");
        assert_eq!(dialog.chose(SETTING), SETTING_SHADOW, "a shadowed box opened as a plain one");
    }

    /// Which row of the Art list a pattern is on, found by the name the file
    /// uses — the same reason `style_row` looks its own up by name.
    fn art_list_row(name: &str) -> usize {
        art::DRAWN.iter().position(|(_, found)| *found == name).expect("a pattern") + 1
    }

    /// What the Width list is showing now.
    fn width_items(editor: &Editor) -> Vec<String> {
        match editor.dialog.as_ref().and_then(|dialog| dialog.fields.get(WIDTH)) {
            Some(Field::Choice { items, .. }) => items.clone(),
            _ => panic!("the width is not a list"),
        }
    }

    #[test]
    fn a_pattern_picked_out_of_the_gallery_goes_round_the_pages() {
        let mut editor = editor();
        editor.open_page_borders();
        choose(&mut editor, SETTING, SETTING_BOX);
        choose(&mut editor, ART, art_list_row("triangles"));
        editor.page_borders_changed();
        accept(&mut editor);

        let top = editor.document.page_borders().top.expect("a top border");
        assert_eq!(top.style, "triangles");
        assert!(top.is_art(), "it was not written as art");
        // And it is drawn as itself: a row of triangles is a great many
        // rectangles, where a line is one.
        let drawn = editor.pages.first().map_or(0, |page| page.decorations.len());
        assert!(drawn > 20, "the pattern came out as {drawn} rectangles");
    }

    #[test]
    fn an_art_border_is_measured_in_points_and_a_line_in_eighths() {
        let mut editor = editor();
        editor.open_page_borders();
        choose(&mut editor, SETTING, SETTING_BOX);
        choose(&mut editor, ART, art_list_row("checkered"));
        editor.page_borders_changed();

        // The list swapped to the widths art is measured in.
        assert_eq!(
            width_items(&editor),
            ART_WIDTHS.iter().map(|(name, _)| *name).collect::<Vec<_>>()
        );
        choose(&mut editor, WIDTH, ART_WIDTHS.len() - 1);
        accept(&mut editor);

        let top = editor.document.page_borders().top.expect("a top border");
        assert_eq!(top.size, art::WIDEST, "the width was written in the wrong unit");
        assert_eq!(top.width_points(), art::WIDEST as f32, "thirty-one points came out as inches");
    }

    #[test]
    fn picking_a_line_style_puts_the_gallery_back_to_none() {
        let mut editor = editor();
        editor.open_page_borders();
        choose(&mut editor, SETTING, SETTING_BOX);
        choose(&mut editor, ART, art_list_row("zigZag"));
        editor.page_borders_changed();

        choose(&mut editor, STYLE, style_row("double"));
        editor.page_borders_changed();
        assert_eq!(
            editor.dialog.as_ref().expect("a dialog").chose(ART),
            ART_NONE,
            "the gallery still says a pattern is in force"
        );
        assert_eq!(width_items(&editor), WIDTHS.iter().map(|(name, _)| *name).collect::<Vec<_>>());

        accept(&mut editor);
        assert_eq!(editor.document.page_borders().top.expect("a border").style, "double");
    }

    #[test]
    fn the_width_in_front_of_you_stays_the_width_you_chose() {
        let mut editor = editor();
        editor.open_page_borders();
        choose(&mut editor, SETTING, SETTING_BOX);
        // Six points as a line, which is 48 eighths.
        choose(&mut editor, WIDTH, WIDTHS.iter().position(|(_, size)| *size == 48).expect("6 pt"));
        choose(&mut editor, ART, art_list_row("triangles"));
        editor.page_borders_changed();
        accept(&mut editor);

        let top = editor.document.page_borders().top.expect("a border");
        assert_eq!(top.size, 6, "six points of line became {} points of art", top.size);
    }

    #[test]
    fn one_of_words_pictures_is_kept_rather_than_thrown_away() {
        // A document made in Word with a border of apples. This program cannot
        // draw apples and does not pretend to — but opening the dialog and
        // pressing OK must not turn them into a plain line.
        let mut editor = editor();
        let apples = Border::line("apples", 20, None);
        editor.document.set_page_borders_everywhere(&PageBorders::box_all(&apples));

        editor.open_page_borders();
        let dialog = editor.dialog.as_ref().expect("a dialog");
        assert_ne!(dialog.chose(ART), ART_NONE, "the gallery says there is no art");
        accept(&mut editor);

        let top = editor.document.page_borders().top.expect("a border");
        assert_eq!(top.style, "apples", "the apples were thrown away");
        assert_eq!(top.size, 20, "and its width changed under it");
    }

    #[test]
    fn a_picture_word_ships_is_drawn_as_a_line_of_its_width() {
        let mut editor = editor();
        let apples = Border::line("apples", 20, None);
        editor.document.set_page_borders_everywhere(&PageBorders::box_all(&apples));
        editor.relayout();

        // Four edges, four rectangles: a plain line each, and nothing invented.
        let drawn = editor.pages.first().map_or(0, |page| page.decorations.len());
        assert_eq!(drawn, 4, "a picture was drawn as {drawn} rectangles");
    }

    #[test]
    fn a_shadowed_box_draws_more_than_a_plain_one() {
        let mut editor = editor();
        editor.open_page_borders();
        choose(&mut editor, SETTING, SETTING_BOX);
        accept(&mut editor);
        let plain = editor.pages.first().map_or(0, |page| page.decorations.len());

        editor.open_page_borders();
        choose(&mut editor, SETTING, SETTING_SHADOW);
        accept(&mut editor);
        let shadowed = editor.pages.first().map_or(0, |page| page.decorations.len());
        assert!(shadowed > plain, "the shadow was written down and not drawn");
    }
}
