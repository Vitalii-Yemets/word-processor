//! The page-number gallery: this program's own designs, drawn here.
//!
//! # Why there was nothing in it
//!
//! Word's Page Number button asks two questions. Where does it go — the head
//! of the page, the foot, the margin, where the caret is — and then, what
//! does it look like: a gallery of twenty-odd arrangements with names like
//! "Accent Bar 2" and "Vertical Outline 1". Those arrangements are Word's own
//! content, shipped inside Word, and **J6** would not put something else
//! behind their names, because a gallery that draws one thing under the name
//! of another is lying about what a person is picking.
//!
//! So the second question went unasked, and the menu offered three positions
//! and nothing else. That is the gap this fills: the same two questions, with
//! a gallery of designs that are this program's own and named for what they
//! are. "On a grey band" says what a person is about to get. "Accent Bar 2"
//! says only that somebody numbered it.
//!
//! # What a design is
//!
//! Two decisions and nothing more: how the number reads — bare, "Page 1",
//! "Page 1 of 4", in brackets, between dashes — and what is drawn round it.
//! Everything else follows from where it was asked for, which is why the rule
//! on a header is drawn underneath and the rule on a footer above: both face
//! the text.
//!
//! # And the rest of the gallery
//!
//! Underneath the designs is whatever the person has saved into that gallery
//! themselves, and the line that puts another one there. Word's galleries are
//! all of them half its own content and half the person's; this is the second
//! half, and it is the half that is the same machinery as [`wp_docx::blocks`].

use wp_docx::blocks::{PAGE_NUMBERS, PAGE_NUMBERS_BOTTOM, PAGE_NUMBERS_TOP};
use wp_docx::furniture::{Furniture, Which};
use wp_docx::model::{Alignment, Block, Body, Border, Paragraph, ParagraphProperties, Run};
use wp_shell::Response;

use crate::chrome::icons::Icon;
use crate::chrome::popup::{Kind, Row};
use crate::chrome::{Choice, Command, Popup};

use super::Editor;

/// The colour of a rule, and of a band.
///
/// Grey rather than the theme's accent: what a design decides is an
/// arrangement, and what colour a document is printed in is the document's
/// own business. A page number that changed colour when somebody changed the
/// theme would be deciding something it was not asked about.
const RULE: &str = "808080";
const BAND: &str = "EEEEEE";

/// How thick that rule is, in eighths of a point.
const THICKNESS: u32 = 6;

/// How wide the gallery is drawn.
const WIDTH: f32 = 300.0;

/// Where the number is going, which Word asks before it asks what it looks
/// like.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Place {
    /// The running head, on every page.
    Top,
    /// The running foot, which is where a printed page usually has one.
    #[default]
    Bottom,
    /// Where the caret is, as a field in the text: Word's Current Position,
    /// and the one that is part of a sentence rather than part of the page.
    Here,
    /// In the margin beside the text, which is Word's Page Margins.
    ///
    /// Not a running head at all: a number in the margin is a text box
    /// standing in the empty band at the side of the paper. It goes into the
    /// header anyway, because that is what makes something appear on every
    /// page — but what the header holds is the box and not the number.
    Margins,
}

impl Place {
    /// Which header or footer this place is, when it is one at all.
    fn furniture(self) -> Option<Furniture> {
        match self {
            // The margin's box is carried by the header, since the header is
            // what every page has a copy of.
            Self::Top | Self::Margins => Some(Furniture::Header),
            Self::Bottom => Some(Furniture::Footer),
            Self::Here => None,
        }
    }

    /// The gallery a block saved from here goes into.
    ///
    /// Word keeps one per place rather than one for all of them, because a
    /// design that belongs at the foot of a page does not belong in the
    /// middle of a sentence.
    pub(crate) fn gallery(self) -> &'static str {
        match self {
            Self::Top => PAGE_NUMBERS_TOP,
            Self::Bottom => PAGE_NUMBERS_BOTTOM,
            Self::Margins => wp_docx::blocks::PAGE_NUMBERS_MARGINS,
            Self::Here => PAGE_NUMBERS,
        }
    }

    /// All three, for the catalogue of everything the program can say.
    pub(crate) const ALL: &'static [Self] = &[Self::Top, Self::Bottom, Self::Margins, Self::Here];

    /// What the menu calls it.
    ///
    /// Word's own words. Here rather than in the menu because the catalogue
    /// of everything the program can say is gathered by walking tables like
    /// this one, and a label written where the menu is built is a label no
    /// translator ever sees.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Top => "Top of Page",
            Self::Bottom => "Bottom of Page",
            Self::Margins => "Page Margins",
            Self::Here => "Current Position",
        }
    }

    /// Which drawing goes beside it.
    pub(crate) fn icon(self) -> crate::chrome::icons::Icon {
        use crate::chrome::icons::Icon;
        match self {
            Self::Top => Icon::Header,
            Self::Bottom => Icon::Footer,
            Self::Margins => Icon::Margins,
            Self::Here => Icon::PageNumber,
        }
    }

    /// What to say when one has gone in.
    pub(crate) fn said(self) -> &'static str {
        match self {
            Self::Top => "Page number at the top: {0}",
            Self::Bottom => "Page number at the foot: {0}",
            Self::Margins => "Page number in the margin: {0}",
            Self::Here => "Page number: {0}",
        }
    }
}

/// How the number reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reads {
    /// The number by itself.
    Bare,
    /// "Page 1", which says what the number is a number of.
    Page,
    /// "Page 1 of 4", which says how much is left as well as where one is.
    OfTotal,
    /// "[1]", which is how a number is set beside text rather than under it.
    Bracketed,
    /// "— 1 —", which is how a book sets one.
    Dashed,
}

/// And what is drawn round it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Trim {
    /// Nothing at all, which is most of them.
    Nothing,
    /// A grey band the width of the text.
    Band,
    /// A rule between the number and the text, on whichever side the text is.
    Rule,
}

/// One design.
pub(crate) struct Design {
    /// What it is called where a person reads it, which says what it draws.
    pub(crate) name: &'static str,
    alignment: Alignment,
    reads: Reads,
    trim: Trim,
}

/// The gallery.
///
/// Nine, which is fewer than Word's twenty-odd and more than the one it had.
/// Each is here because somebody wants it: the three plain ones because a
/// number goes where the eye already is, "Page 1 of 4" because a person
/// reading a printout wants to know whether they have the whole of it, the
/// band and the rule because a running foot wants separating from the text
/// above it.
pub(crate) const DESIGNS: &[Design] = &[
    Design {
        name: "Plain, to the left",
        alignment: Alignment::Start,
        reads: Reads::Bare,
        trim: Trim::Nothing,
    },
    Design {
        name: "Plain, in the middle",
        alignment: Alignment::Center,
        reads: Reads::Bare,
        trim: Trim::Nothing,
    },
    Design {
        name: "Plain, to the right",
        alignment: Alignment::End,
        reads: Reads::Bare,
        trim: Trim::Nothing,
    },
    Design {
        name: "Page 1",
        alignment: Alignment::Center,
        reads: Reads::Page,
        trim: Trim::Nothing,
    },
    Design {
        name: "Page 1 of 4",
        alignment: Alignment::Center,
        reads: Reads::OfTotal,
        trim: Trim::Nothing,
    },
    Design {
        name: "In brackets",
        alignment: Alignment::Center,
        reads: Reads::Bracketed,
        trim: Trim::Nothing,
    },
    Design {
        name: "Between dashes",
        alignment: Alignment::Center,
        reads: Reads::Dashed,
        trim: Trim::Nothing,
    },
    Design {
        name: "On a grey band",
        alignment: Alignment::Center,
        reads: Reads::Bare,
        trim: Trim::Band,
    },
    Design {
        name: "Against a rule, to the right",
        alignment: Alignment::End,
        reads: Reads::Bare,
        trim: Trim::Rule,
    },
];

/// How wide a box in the margin is, in points.
///
/// Narrow: what stands in it is a number of one or two digits, and a box wider
/// than the margin would hang over the text it is meant to stand beside.
const MARGIN_BOX_WIDTH: f64 = 36.0;

/// And how tall.
const MARGIN_BOX_HEIGHT: f64 = 18.0;

/// The designs that stand in a margin.
///
/// Two, because a margin has two sides and there is nothing else to decide:
/// the number is the number. Word offers the same two with its own decoration
/// round them, and the decoration is Word's content rather than an
/// arrangement — the same reason **J22** drew its own.
pub(crate) const IN_THE_MARGIN: &[Design] = &[
    Design {
        name: "In the left margin",
        alignment: Alignment::Center,
        reads: Reads::Bare,
        trim: Trim::Nothing,
    },
    Design {
        name: "In the right margin",
        alignment: Alignment::Center,
        reads: Reads::Bare,
        trim: Trim::Nothing,
    },
];

/// A header holding nothing but a box standing in one of the margins.
///
/// # Why the header
///
/// Because a header is what every page has a copy of. The number itself is
/// not in the header — it is inside a box the header carries, and the box is
/// anchored to the margin rather than to the header's own line, so it stands
/// beside the text rather than above it.
fn margin_body(right: bool) -> Body {
    use wp_docx::anchor::{Anchor, Placement, Relative, Wrap};

    let mut box_of = wp_docx::shapes::Shape::text_box(MARGIN_BOX_WIDTH, MARGIN_BOX_HEIGHT, "");
    // No line round it and nothing behind it: what is wanted is a number in
    // the margin, not a number in a box in the margin.
    box_of.outline = None;
    box_of.text = vec![Paragraph {
        properties: ParagraphProperties {
            alignment: Some(Alignment::Center),
            ..ParagraphProperties::default()
        },
        runs: vec![Run::field("PAGE", "1")],
    }];
    box_of.anchor = Some(Anchor {
        // In the band itself rather than against the edge of the text, which
        // is the whole difference between a margin number and a plain one.
        horizontal_from: if right { Relative::RightMargin } else { Relative::LeftMargin },
        horizontal: Placement::Aligned("center".to_owned()),
        // Halfway down the paper, which is where Word puts one. Measured
        // from the page and not from the text area: the head this box is
        // carried by is laid out on a page of its own, where the text area
        // has no foot to measure from.
        vertical_from: Relative::Page,
        vertical: Placement::Aligned("center".to_owned()),
        // Nothing wraps round it: it is in the margin, where there is no text
        // to push out of the way.
        wrap: Wrap::None,
        ..Anchor::default()
    });

    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph {
        properties: ParagraphProperties::default(),
        runs: vec![Run {
            properties: wp_docx::model::RunProperties::default(),
            content: vec![wp_docx::model::RunContent::Shape(Box::new(box_of))],
            field: None,
            revision: None,
            format_change: None,
        }],
    }));
    body
}

/// Which of them a place can offer.
///
/// All of them where there is a whole paragraph to arrange. Where the number
/// goes into a sentence there is no paragraph of its own to align or to draw
/// a band behind, so what is left is the readings — and only one of each,
/// because three designs that differ by an alignment nothing will apply would
/// be three lines doing the same thing.
pub(super) fn designs_for(place: Place) -> Vec<usize> {
    if place == Place::Margins {
        return (0..IN_THE_MARGIN.len()).collect();
    }
    if place != Place::Here {
        return (0..DESIGNS.len()).collect();
    }
    let mut readings: Vec<Reads> = Vec::new();
    let mut chosen = Vec::new();
    for (at, design) in DESIGNS.iter().enumerate() {
        if design.trim == Trim::Nothing && !readings.contains(&design.reads) {
            readings.push(design.reads);
            chosen.push(at);
        }
    }
    chosen
}

/// Which set of designs a place draws from.
///
/// The margin's are their own: what stands in a margin is a box, and what
/// stands in a running head is a paragraph, so there is no arrangement the
/// two could share.
pub(crate) fn designs_of(place: Place) -> &'static [Design] {
    if place == Place::Margins {
        IN_THE_MARGIN
    } else {
        DESIGNS
    }
}

/// The runs a design is made of.
///
/// The number is a field — the instruction `PAGE` with the last answer
/// somebody worked out cached beside it — because that is the only kind of
/// number that is still right on the second page.
fn runs(reads: Reads) -> Vec<Run> {
    let number = || Run::field("PAGE", "1");
    match reads {
        Reads::Bare => vec![number()],
        Reads::Page => vec![Run::text("Page "), number()],
        Reads::OfTotal => {
            vec![Run::text("Page "), number(), Run::text(" of "), Run::field("NUMPAGES", "1")]
        }
        Reads::Bracketed => vec![Run::text("["), number(), Run::text("]")],
        Reads::Dashed => vec![Run::text("\u{2014} "), number(), Run::text(" \u{2014}")],
    }
}

/// And the whole header or footer it makes.
fn body(design: &Design, kind: Furniture) -> Body {
    let mut properties =
        ParagraphProperties { alignment: Some(design.alignment), ..ParagraphProperties::default() };
    match design.trim {
        Trim::Nothing => {}
        Trim::Band => properties.shading = Some(BAND.to_owned()),
        // On whichever side the text is: a rule under a running head and over
        // a running foot both separate the number from what it numbers, and a
        // rule on the far side separates it from the edge of the paper, which
        // needs no separating.
        Trim::Rule => {
            let line = Border::line("single", THICKNESS, Some(RULE));
            match kind {
                Furniture::Header => properties.borders.bottom = Some(line),
                Furniture::Footer => properties.borders.top = Some(line),
            }
        }
    }

    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph { properties, runs: runs(design.reads) }));
    body
}

impl Editor {
    /// Drops the gallery open for one of the places.
    ///
    /// It places itself under the Page Number button rather than being looked
    /// up by a button of its own, the way [`Editor::open_auto_text`] does: it
    /// is the second half of that button's menu, and one button cannot be
    /// looked up for two.
    pub(super) fn open_page_number_designs(&mut self, place: Place) -> Response {
        if self.close_popup_if(Choice::PageNumberDesign) && self.page_number_place == place {
            return Response::Redraw;
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::PageNumber) else {
            return Response::Ignored;
        };
        self.page_number_place = place;

        let (items, rows) = self.page_number_gallery(place);
        self.popup = Some(
            Popup::new(Choice::PageNumberDesign, items, None, left, top, WIDTH).with_rows(rows),
        );
        self.needs_redraw = true;
        Response::Redraw
    }

    /// What is in it: the designs, what the person has saved into it, and the
    /// line that saves another.
    pub(super) fn page_number_gallery(&self, place: Place) -> (Vec<String>, Vec<Row>) {
        let mut items: Vec<String> = designs_for(place)
            .into_iter()
            .map(|at| crate::messages::t(designs_of(place)[at].name).to_owned())
            .collect();
        let mut rows: Vec<Row> =
            items.iter().map(|_| Row::new(Kind::Choice, Icon::PageNumber)).collect();

        for block in self.blocks_in_gallery(place.gallery()) {
            items.push(block);
            rows.push(Row::new(Kind::Choice, Icon::QuickParts));
        }

        items.push(String::new());
        rows.push(Row::separator());
        items.push(crate::messages::t("Save Selection to Page Number Gallery").to_owned());
        rows.push(Row::new(Kind::Choice, Icon::Save));
        (items, rows)
    }

    /// The names of whatever is in one of the galleries a person fills.
    pub(super) fn blocks_in_gallery(&self, gallery: &str) -> Vec<String> {
        self.own_template()
            .map(|template| {
                template.blocks_in(gallery).into_iter().map(|block| block.name).collect()
            })
            .unwrap_or_default()
    }

    /// Puts in whichever line was pressed.
    pub(super) fn choose_page_number_design(&mut self, index: usize) -> Response {
        self.popup = None;
        let place = self.page_number_place;
        let offered = designs_for(place);

        if let Some(design) = offered.get(index).map(|at| &designs_of(place)[*at]) {
            return self.put_page_number(place, design);
        }

        let saved = self.blocks_in_gallery(place.gallery());
        let past = index - offered.len();
        if let Some(name) = saved.get(past).cloned() {
            return self.insert_own_block(&name);
        }
        // The separator, which cannot be pressed, then the line that saves.
        if past == saved.len() + 1 {
            return self.save_selection_to(place.gallery());
        }
        Response::Ignored
    }

    /// One design, where it was asked for.
    fn put_page_number(&mut self, place: Place, design: &Design) -> Response {
        let note = crate::messages::with(place.said(), &[crate::messages::t(design.name)]);
        if place == Place::Margins {
            // The second of the two is the right-hand margin, and the design
            // is the whole of what the header holds.
            let right = design.name.contains("right");
            let body = margin_body(right);
            let done = self
                .document
                .set_furniture_body(Furniture::Header, Which::Default, &body)
                .unwrap_or(false);
            self.relayout();
            return self.edited(done, &note);
        }

        let Some(kind) = place.furniture() else {
            // Current Position: the runs go into the text at the caret, and
            // nothing about the page is changed. A number in a sentence is
            // part of the sentence.
            let changed = self.document.insert_runs(&runs(design.reads));
            self.relayout();
            self.reveal_caret();
            return self.edited(changed, &note);
        };

        match self.document.set_furniture_body(kind, Which::Default, &body(design, kind)) {
            Ok(changed) => {
                self.relayout();
                self.edited(changed, &note)
            }
            Err(error) => self.report(&format!("Cannot set that: {error}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_docx::model::Paragraph as ModelParagraph;
    use wp_docx::Document;

    /// An editor over two paragraphs, which is enough to have a page.
    fn editor() -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(ModelParagraph::text("One")));
        body.blocks.push(Block::Paragraph(ModelParagraph::text("Two")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let library: &'static wp_layout::FontLibrary =
            Box::leak(Box::new(wp_layout::FontLibrary::scan_system()));
        Editor::new(library, document, None)
    }

    /// What the words of a body come to, fields and all.
    fn words(body: &Body) -> String {
        body.blocks
            .iter()
            .filter_map(|block| match block {
                Block::Paragraph(paragraph) => Some(paragraph),
                _ => None,
            })
            .flat_map(|paragraph| paragraph.runs.iter())
            .filter_map(|run| match run.content.first() {
                Some(wp_docx::model::RunContent::Text(text)) => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn every_design_numbers_the_page() {
        // A design in the page-number gallery that did not number the page
        // would be a line nobody could use.
        for design in DESIGNS {
            assert!(!design.name.trim().is_empty());
            let made = body(design, Furniture::Footer);
            assert_eq!(made.blocks.len(), 1, "{}", design.name);
            let Block::Paragraph(paragraph) = &made.blocks[0] else { panic!("{}", design.name) };
            assert!(
                paragraph.runs.iter().any(|run| run.field.as_deref() == Some("PAGE")),
                "{} has no page field in it",
                design.name
            );
        }
    }

    #[test]
    fn no_two_designs_draw_the_same_thing() {
        // Two lines of a gallery that do the same are one line and a lie.
        for (at, design) in DESIGNS.iter().enumerate() {
            for other in &DESIGNS[at + 1..] {
                assert_ne!(
                    (design.alignment, design.reads, design.trim),
                    (other.alignment, other.reads, other.trim),
                    "{} and {} are the same design",
                    design.name,
                    other.name
                );
            }
        }
    }

    #[test]
    fn a_rule_faces_the_text_whichever_end_of_the_page_it_is_at() {
        let ruled = DESIGNS.iter().find(|design| design.trim == Trim::Rule).expect("a ruled one");

        let head = body(ruled, Furniture::Header);
        let Block::Paragraph(paragraph) = &head.blocks[0] else { panic!("a paragraph") };
        assert!(paragraph.properties.borders.bottom.is_some(), "a head's rule is underneath");
        assert!(paragraph.properties.borders.top.is_none());

        let foot = body(ruled, Furniture::Footer);
        let Block::Paragraph(paragraph) = &foot.blocks[0] else { panic!("a paragraph") };
        assert!(paragraph.properties.borders.top.is_some(), "a foot's rule is above");
        assert!(paragraph.properties.borders.bottom.is_none());
    }

    #[test]
    fn the_banded_one_is_banded() {
        let banded = DESIGNS.iter().find(|design| design.trim == Trim::Band).expect("a banded one");
        let made = body(banded, Furniture::Footer);
        let Block::Paragraph(paragraph) = &made.blocks[0] else { panic!("a paragraph") };
        assert_eq!(paragraph.properties.shading.as_deref(), Some(BAND));
    }

    #[test]
    fn a_number_in_a_sentence_is_offered_only_the_designs_that_mean_anything_there() {
        // No alignment, no band, no rule: a paragraph's worth of decoration
        // applied to a word in the middle of one would decorate the sentence.
        let offered = designs_for(Place::Here);
        assert!(offered.len() < DESIGNS.len(), "everything was offered");
        let mut readings = Vec::new();
        for at in offered {
            let design = &DESIGNS[at];
            assert_eq!(design.trim, Trim::Nothing, "{} is decorated", design.name);
            assert!(!readings.contains(&design.reads), "{} reads like another", design.name);
            readings.push(design.reads);
        }
        // And every reading there is, so nothing is lost by going inline.
        assert_eq!(readings.len(), 5);
    }

    #[test]
    fn the_whole_gallery_is_offered_at_either_end_of_the_page() {
        assert_eq!(designs_for(Place::Top).len(), DESIGNS.len());
        assert_eq!(designs_for(Place::Bottom).len(), DESIGNS.len());
    }

    #[test]
    fn each_place_saves_into_a_gallery_of_its_own() {
        // Word's are four; a design that belongs at the foot of a page does
        // not belong in the middle of a sentence, and one list for both would
        // put it there.
        assert_eq!(Place::Top.gallery(), "pgNumT");
        assert_eq!(Place::Bottom.gallery(), "pgNumB");
        assert_eq!(Place::Here.gallery(), "pgNum");
    }

    #[test]
    fn the_gallery_is_the_designs_then_what_was_saved_then_the_way_to_save_more() {
        // Word's galleries are all of them two halves, and a gallery a person
        // cannot add to is one half of one.
        let editor = editor();
        for place in Place::ALL.iter().copied() {
            let (items, rows) = editor.page_number_gallery(place);
            let designs = designs_for(place).len();
            assert_eq!(items.len(), rows.len(), "{place:?} has a row for every line");
            assert_eq!(items.len(), designs + 2, "{place:?}: {items:?}");
            assert!(items[designs].is_empty(), "the separator carries no words");
            assert_eq!(rows[designs].kind, Kind::Separator);
            assert_eq!(items[designs + 1], "Save Selection to Page Number Gallery");
        }
    }

    #[test]
    fn every_line_of_the_gallery_leads_somewhere() {
        // The same rule the ribbon's menus are held to: a row that does
        // nothing is the same lie as a button that does nothing.
        let editor = editor();
        for place in Place::ALL.iter().copied() {
            let (items, rows) = editor.page_number_gallery(place);
            for (at, (label, row)) in items.iter().zip(rows.iter()).enumerate() {
                if row.kind == Kind::Separator {
                    assert!(label.is_empty(), "{place:?} row {at} is a separator with words");
                } else {
                    assert!(!label.is_empty(), "{place:?} row {at} has no words");
                }
            }
        }
    }

    #[test]
    fn a_design_chosen_at_the_foot_writes_a_footer_that_numbers_the_page() {
        let mut editor = editor();
        editor.page_number_place = Place::Bottom;
        // "Page 1 of 4", which is the fifth line.
        editor.choose_page_number_design(4);

        let footer = editor.document.furniture(Furniture::Footer).expect("a footer was written");
        assert_eq!(words(&footer), "Page 1 of 1", "{footer:?}");
        assert!(editor.document.furniture(Furniture::Header).is_none(), "the head was touched");
    }

    #[test]
    fn and_at_the_head_it_writes_a_header_instead() {
        let mut editor = editor();
        editor.page_number_place = Place::Top;
        editor.choose_page_number_design(0);

        let header = editor.document.furniture(Furniture::Header).expect("a header was written");
        assert_eq!(words(&header), "1");
        assert!(editor.document.furniture(Furniture::Footer).is_none(), "the foot was touched");
    }

    #[test]
    fn a_number_at_the_caret_goes_into_the_text_and_leaves_the_page_alone() {
        let mut editor = editor();
        editor.page_number_place = Place::Here;
        // The second reading, which is "Page 1".
        editor.choose_page_number_design(1);

        assert!(editor.document.furniture(Furniture::Header).is_none());
        assert!(editor.document.furniture(Furniture::Footer).is_none());
        let body = editor.document.body();
        assert!(words(&body).contains("Page 1"), "{:?}", words(&body));
    }

    #[test]
    fn the_last_line_asks_what_to_call_what_is_being_saved() {
        let mut editor = editor();
        editor.page_number_place = Place::Bottom;
        // Nothing is selected, so it says so rather than opening a dialog to
        // name nothing.
        let (items, _) = editor.page_number_gallery(Place::Bottom);
        editor.choose_page_number_design(items.len() - 1);
        assert!(editor.dialog.is_none(), "it asked for a name with nothing to save");

        editor.document.select_all();
        let (items, _) = editor.page_number_gallery(Place::Bottom);
        editor.choose_page_number_design(items.len() - 1);
        assert!(editor.dialog.is_some(), "it did not ask what to call it");
        assert_eq!(editor.saving_to, "pgNumB", "it would have gone to the wrong gallery");
    }

    #[test]
    fn the_separator_in_the_middle_of_it_does_nothing_at_all() {
        let mut editor = editor();
        editor.page_number_place = Place::Bottom;
        let separator = designs_for(Place::Bottom).len();
        assert!(matches!(editor.choose_page_number_design(separator), Response::Ignored));
        assert!(editor.document.furniture(Furniture::Footer).is_none());
    }

    #[test]
    fn the_margin_is_a_place_of_its_own_with_designs_of_its_own() {
        // What stands in a margin is a box and what stands in a running head
        // is a paragraph, so there is no arrangement the two could share.
        assert_eq!(designs_of(Place::Margins).len(), IN_THE_MARGIN.len());
        assert_eq!(designs_of(Place::Top).len(), DESIGNS.len());
        assert_eq!(designs_for(Place::Margins).len(), 2, "a margin has two sides");
        assert_eq!(Place::Margins.gallery(), "pgNumMargins");
    }

    #[test]
    fn a_number_in_the_margin_is_a_box_anchored_to_that_margin() {
        use wp_docx::anchor::Relative;
        use wp_docx::model::RunContent;

        for (right, wanted) in [(false, Relative::LeftMargin), (true, Relative::RightMargin)] {
            let body = margin_body(right);
            let Block::Paragraph(paragraph) = &body.blocks[0] else { panic!("a paragraph") };
            let Some(RunContent::Shape(shape)) = paragraph.runs[0].content.first() else {
                panic!("the margin's number is not a box");
            };
            let anchor = shape.anchor.as_ref().expect("a floating box");
            assert_eq!(anchor.horizontal_from, wanted);
            // In the band itself, not against the edge of the text, which is
            // the whole difference between a margin number and a plain one.
            assert!(matches!(
                anchor.horizontal,
                wp_docx::anchor::Placement::Aligned(ref edge) if edge == "center"
            ));
            // And it numbers the page, or it is a box with nothing in it.
            assert!(
                shape.text[0].runs.iter().any(|run| run.field.as_deref() == Some("PAGE")),
                "no page field in the box"
            );
        }
    }

    #[test]
    fn choosing_one_writes_a_header_carrying_the_box() {
        // The header, because a header is what every page has a copy of. The
        // number is not in the header: the box is, and the box stands in the
        // margin.
        let mut editor = editor();
        editor.page_number_place = Place::Margins;
        editor.choose_page_number_design(1);

        let header = editor.document.furniture(Furniture::Header).expect("a header");
        let Block::Paragraph(paragraph) = &header.blocks[0] else { panic!("a paragraph") };
        assert!(
            matches!(paragraph.runs[0].content.first(), Some(wp_docx::model::RunContent::Shape(_))),
            "the header holds no box"
        );
        assert!(editor.document.furniture(Furniture::Footer).is_none(), "the foot was touched");
    }

    #[test]
    fn the_place_menu_offers_all_four_and_each_says_what_it_is() {
        // Word's four, and each opens a gallery rather than acting: a row
        // that looks like it will act and instead opens a list has misled
        // whoever pressed it.
        assert_eq!(Place::ALL.len(), 4);
        for place in Place::ALL.iter().copied() {
            assert!(!place.label().trim().is_empty(), "{place:?} has no name");
            assert!(!place.gallery().trim().is_empty(), "{place:?} saves nowhere");
        }
    }

    #[test]
    fn the_readings_read_the_way_they_are_named() {
        let words = |reads: Reads| -> String {
            runs(reads)
                .iter()
                .map(|run| match run.content.first() {
                    Some(wp_docx::model::RunContent::Text(text)) => text.clone(),
                    _ => String::new(),
                })
                .collect()
        };
        assert_eq!(words(Reads::Bare), "1");
        assert_eq!(words(Reads::Page), "Page 1");
        assert_eq!(words(Reads::OfTotal), "Page 1 of 1");
        assert_eq!(words(Reads::Bracketed), "[1]");
        assert_eq!(words(Reads::Dashed), "\u{2014} 1 \u{2014}");
    }
}
