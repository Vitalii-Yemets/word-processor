//! The cover page.
//!
//! # What Word's cover pages are
//!
//! Building blocks: whole pages of prepared content kept in a template, dropped
//! in at the front, with the title and the author bound to the document's own
//! properties so that changing one changes the other. There are a dozen designs
//! and they differ only in arrangement and colour.
//!
//! This offers a few arrangements rather than a dozen, and writes the title and
//! the author as text taken from the properties at the moment it is inserted
//! rather than bound to them. A binding would be better and is a larger piece
//! of work — the content control it needs is a part of the format nothing here
//! reads yet — so what is written is what a reader sees, and changing the title
//! afterwards means changing it in both places.

use crate::history::EditKind;
use crate::model::{Alignment, Block, Paragraph, ParagraphProperties, Run, RunProperties};
use wp_xml::tree::Element;

use crate::{edit, position, read, Document, TextPosition};

/// How a cover page is arranged.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Layout {
    /// Everything in the middle of the page, which is the plainest and the one
    /// that suits a document of any kind.
    #[default]
    Centred,
    /// The title against the left margin with a rule under it, and the author
    /// at the foot — the arrangement a report usually takes.
    Banded,
    /// The title low on the page, which leaves the top empty for a mark or a
    /// picture to be put in afterwards.
    Footed,
}

impl Layout {
    /// What a person is shown when picking one.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Centred => "Centred",
            Self::Banded => "Banded",
            Self::Footed => "Title at the foot",
        }
    }

    /// Every one that can be picked.
    pub const ALL: &'static [Self] = &[Self::Centred, Self::Banded, Self::Footed];
}

/// What goes on a cover page.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cover {
    pub title: String,
    pub subtitle: String,
    pub author: String,
    /// Written as it should appear; nothing here works out what today is.
    pub date: String,
}

impl Cover {
    /// Whether there is anything at all to put on the page.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        [&self.title, &self.subtitle, &self.author, &self.date]
            .iter()
            .all(|piece| piece.trim().is_empty())
    }
}

impl Document {
    /// What a cover page would say, taken from the document's own properties.
    #[must_use]
    pub fn cover_from_properties(&self) -> Cover {
        let properties = self.properties();
        Cover {
            title: properties.title,
            subtitle: properties.subject,
            author: properties.author,
            date: properties.created.split('T').next().unwrap_or_default().to_owned(),
        }
    }

    /// Puts a cover page at the very front of the document.
    ///
    /// Always at the front, whatever the caret is doing: a cover page anywhere
    /// else is not a cover page. Returns whether anything was written.
    pub fn insert_cover_page(&mut self, layout: Layout, cover: &Cover) -> bool {
        if cover.is_empty() {
            return false;
        }
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);

        let blocks = cover_blocks(layout, cover);
        let prefix = self.prefix();
        let Some(path) = position::paragraph_path(&self.tree().root, 0) else { return false };
        let Some((position, parent_path)) = path.split_last() else { return false };
        let position = *position;
        let parent_path = parent_path.to_vec();
        let Some(parent) = edit::element_at_path_mut(&mut self.tree_mut().root, &parent_path)
        else {
            return false;
        };

        // Wrapped in a mark that says what these paragraphs are, so that
        // taking the cover page off again means taking off exactly what was
        // put in — see [`Document::remove_cover_page`].
        parent.insert_element(position, wrapped(&blocks, prefix.as_deref()));

        // The caret goes to the start of what used to be the first paragraph,
        // which is where the document proper now begins.
        self.set_caret(TextPosition::new(blocks.len(), 0));
        self.mark_modified();
        true
    }

    /// Where the cover page is among the body's children, if there is one.
    ///
    /// By the mark it carries and not by looking at the words: a document can
    /// begin with a title and a page break without that being a cover page,
    /// and a program that guessed would take away somebody's first page.
    #[must_use]
    fn cover_page_index(&self) -> Option<usize> {
        let body = read::find_body(&self.tree().root)?;
        body.children.iter().position(|node| match node {
            wp_xml::tree::Node::Element(element) => is_a_cover_page(element),
            _ => false,
        })
    }

    /// Whether the document has one.
    #[must_use]
    pub fn has_cover_page(&self) -> bool {
        self.cover_page_index().is_some()
    }

    /// Word's Remove Current Cover Page.
    ///
    /// Takes away the whole of what was put in — the paragraphs, the room
    /// above them and the page break that made the document proper start on a
    /// sheet of its own — and nothing else. That is what the mark is for: a
    /// cover page is a stretch of ordinary paragraphs, and without something
    /// saying where it begins and ends there is no way to tell it from the
    /// first page of the document.
    pub fn remove_cover_page(&mut self) -> bool {
        let Some(index) = self.cover_page_index() else { return false };
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);

        let Some(body) = read::find_body_mut(&mut self.tree_mut().root) else { return false };
        body.children.remove(index);

        self.set_caret(TextPosition::default());
        self.mark_modified();
        true
    }
}

/// What the format calls the gallery a cover page comes from.
///
/// Word writes this on the mark round one, and reads it back to know what it
/// is looking at. The same words it uses, because a document whose cover page
/// was put in here should be one Word can take off again.
const COVER_GALLERY: &str = "Cover Pages";

/// Whether an element is the mark round a cover page.
fn is_a_cover_page(element: &Element) -> bool {
    if !element.is(Some(read::W), "sdt") {
        return false;
    }
    element
        .child(Some(read::W), "sdtPr")
        .and_then(|properties| properties.child(Some(read::W), "docPartObj"))
        .and_then(|part| part.child(Some(read::W), "docPartGallery"))
        .and_then(|gallery| gallery.attribute(Some(read::W), "val"))
        .is_some_and(|gallery| gallery == COVER_GALLERY)
}

/// The blocks of a cover page, inside the mark that says what they are.
fn wrapped(blocks: &[Block], prefix: Option<&str>) -> Element {
    let name = |local: &str| edit::name_with(prefix, local);

    let mut gallery = Element::new(&name("docPartGallery"), Some(read::W));
    gallery.set_namespaced_attribute(&name("val"), read::W, COVER_GALLERY);

    let mut part = Element::new(&name("docPartObj"), Some(read::W));
    part.push_element(gallery);
    // Word writes this beside the gallery to say the mark is one of a kind
    // rather than one of a repeating set.
    part.push_element(Element::new(&name("docPartUnique"), Some(read::W)));

    let mut properties = Element::new(&name("sdtPr"), Some(read::W));
    properties.push_element(part);

    let mut content = Element::new(&name("sdtContent"), Some(read::W));
    for block in blocks {
        content.push_element(edit::block_element(block, prefix));
    }

    let mut mark = Element::new(&name("sdt"), Some(read::W));
    mark.push_element(properties);
    mark.push_element(content);
    mark
}

/// The paragraphs a cover page is made of.
///
/// The last one carries the page break, so the document proper starts on a
/// sheet of its own.
fn cover_blocks(layout: Layout, cover: &Cover) -> Vec<Block> {
    let alignment = match layout {
        Layout::Centred => Alignment::Center,
        Layout::Banded | Layout::Footed => Alignment::Start,
    };

    let mut blocks = Vec::new();

    // Room above the title, which is what puts it where the eye expects rather
    // than at the very top of the sheet.
    let space_above = match layout {
        Layout::Centred => 6,
        Layout::Banded => 3,
        Layout::Footed => 14,
    };
    for _ in 0..space_above {
        blocks.push(Block::Paragraph(Paragraph::text("")));
    }

    if !cover.title.trim().is_empty() {
        blocks.push(line(&cover.title, alignment, 56, true, None));
        if layout == Layout::Banded {
            // The rule under the title, which is what makes this arrangement
            // the banded one.
            blocks.push(rule());
        }
    }
    if !cover.subtitle.trim().is_empty() {
        blocks.push(line(&cover.subtitle, alignment, 28, false, Some("595959")));
    }

    // The author and the date sit apart from the title, at the foot of the page
    // in every arrangement but the last.
    let space_below = match layout {
        Layout::Centred => 4,
        Layout::Banded => 10,
        Layout::Footed => 1,
    };
    if !cover.author.trim().is_empty() || !cover.date.trim().is_empty() {
        for _ in 0..space_below {
            blocks.push(Block::Paragraph(Paragraph::text("")));
        }
    }
    if !cover.author.trim().is_empty() {
        blocks.push(line(&cover.author, alignment, 24, true, None));
    }
    if !cover.date.trim().is_empty() {
        blocks.push(line(&cover.date, alignment, 22, false, Some("595959")));
    }

    blocks.push(break_paragraph());
    blocks
}

/// One line of the cover page.
fn line(
    text: &str,
    alignment: Alignment,
    half_points: u32,
    bold: bool,
    color: Option<&str>,
) -> Block {
    Block::Paragraph(Paragraph {
        properties: ParagraphProperties {
            alignment: Some(alignment),
            space_after: Some(120),
            ..ParagraphProperties::default()
        },
        runs: vec![Run {
            properties: RunProperties {
                bold: bold.then_some(true),
                size_half_points: Some(half_points),
                color: color.map(str::to_owned),
                ..RunProperties::default()
            },
            content: vec![crate::model::RunContent::Text(text.to_owned())],
            field: None,
            revision: None,
            format_change: None,
        }],
    })
}

/// The rule drawn under a banded title.
///
/// A paragraph with a border along its bottom and nothing in it, which is how
/// a horizontal rule is written in this format — there is no rule element.
fn rule() -> Block {
    use crate::model::{Border, ParagraphBorders};
    Block::Paragraph(Paragraph {
        properties: ParagraphProperties {
            borders: ParagraphBorders {
                bottom: Some(Border::line("single", 12, Some("2E74B5"))),
                ..ParagraphBorders::default()
            },
            space_after: Some(240),
            ..ParagraphProperties::default()
        },
        runs: Vec::new(),
    })
}

/// The paragraph holding the break that ends the cover page.
fn break_paragraph() -> Block {
    Block::Paragraph(Paragraph {
        properties: ParagraphProperties::default(),
        runs: vec![Run {
            properties: RunProperties::default(),
            content: vec![crate::model::RunContent::Break(crate::model::BreakKind::Page)],
            field: None,
            revision: None,
            format_change: None,
        }],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cover() -> Cover {
        Cover {
            title: "A Report".to_owned(),
            subtitle: "Quarterly figures".to_owned(),
            author: "Ann Roe".to_owned(),
            date: "2026-09-08".to_owned(),
        }
    }

    #[test]
    fn a_cover_with_nothing_on_it_is_empty() {
        assert!(Cover::default().is_empty());
        assert!(Cover { title: "   ".to_owned(), ..Cover::default() }.is_empty());
        assert!(!cover().is_empty());
    }

    #[test]
    fn every_arrangement_ends_with_a_break() {
        for layout in Layout::ALL {
            let blocks = cover_blocks(*layout, &cover());
            let last = blocks.last().expect("a last paragraph");
            let Block::Paragraph(paragraph) = last else { panic!("a paragraph") };
            assert!(
                paragraph.runs.iter().any(|run| run
                    .content
                    .contains(&crate::model::RunContent::Break(crate::model::BreakKind::Page))),
                "{} does not end the page",
                layout.label()
            );
        }
    }

    #[test]
    fn every_arrangement_says_everything_it_was_given() {
        for layout in Layout::ALL {
            let text: String = cover_blocks(*layout, &cover())
                .iter()
                .map(Block::plain_text)
                .collect::<Vec<_>>()
                .join("\n");
            for piece in ["A Report", "Quarterly figures", "Ann Roe", "2026-09-08"] {
                assert!(text.contains(piece), "{} leaves out {piece}", layout.label());
            }
        }
    }

    #[test]
    fn what_was_left_out_is_not_written_as_an_empty_line() {
        let blocks = cover_blocks(
            Layout::Centred,
            &Cover { title: "A Report".to_owned(), ..Cover::default() },
        );
        let lines: Vec<String> =
            blocks.iter().map(Block::plain_text).filter(|line| !line.trim().is_empty()).collect();
        assert_eq!(lines, vec!["A Report".to_owned()]);
    }

    #[test]
    fn only_the_banded_arrangement_draws_a_rule() {
        let has_rule = |layout: Layout| {
            cover_blocks(layout, &cover()).iter().any(|block| match block {
                Block::Paragraph(paragraph) => !paragraph.properties.borders.is_empty(),
                Block::Table(_) => false,
            })
        };
        assert!(has_rule(Layout::Banded));
        assert!(!has_rule(Layout::Centred));
        assert!(!has_rule(Layout::Footed));
    }

    #[test]
    fn the_title_is_the_largest_thing_on_the_page() {
        let blocks = cover_blocks(Layout::Centred, &cover());
        let sizes: Vec<u32> = blocks
            .iter()
            .filter_map(|block| match block {
                Block::Paragraph(paragraph) => {
                    paragraph.runs.first().and_then(|run| run.properties.size_half_points)
                }
                Block::Table(_) => None,
            })
            .collect();
        let largest = sizes.iter().copied().max().expect("something is sized");
        assert_eq!(sizes.first().copied(), Some(largest), "the title is not the largest");
    }
}

#[cfg(test)]
mod taking_one_off {
    use super::*;
    use crate::model::{Block, Body, Paragraph};

    fn document(lines: &[&str]) -> Document {
        let mut body = Body::default();
        for line in lines {
            body.blocks.push(Block::Paragraph(Paragraph::text(line)));
        }
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        Document::open(&bytes).expect("reopening")
    }

    fn cover() -> Cover {
        Cover {
            title: String::from("The Title"),
            subtitle: String::new(),
            author: String::from("Ada Lovelace"),
            date: String::from("2027-01-01"),
        }
    }

    #[test]
    fn a_document_with_no_cover_page_says_so() {
        let document = document(&["The body of it"]);
        assert!(!document.has_cover_page());
        // And there is nothing to take off, so nothing happens.
        let mut document = document;
        assert!(!document.remove_cover_page());
        assert_eq!(document.plain_text().trim(), "The body of it");
    }

    #[test]
    fn one_put_in_here_is_marked_as_one() {
        let mut document = document(&["The body of it"]);
        assert!(document.insert_cover_page(Layout::Centred, &cover()));
        assert!(document.has_cover_page(), "it went in without its mark");
        assert!(document.plain_text().contains("The Title"));
    }

    #[test]
    fn the_mark_survives_being_written_out_and_read_back() {
        // A cover page put in today has to be one that can be taken off next
        // week, which means the mark lives in the file.
        let mut document = document(&["The body of it"]);
        document.insert_cover_page(Layout::Banded, &cover());
        let bytes = document.save().expect("saving");
        let reopened = Document::open(&bytes).expect("reopening");
        assert!(reopened.has_cover_page(), "the mark did not survive a round trip");
    }

    #[test]
    fn taking_it_off_leaves_the_document_as_it_was() {
        // The whole of what was put in — the words, the room above them and
        // the page break that made the document start on a sheet of its own —
        // and nothing else.
        let before = document(&["The body of it", "And the rest"]);
        let was = before.plain_text();
        let paragraphs = before.paragraph_count();

        let mut document = before;
        document.insert_cover_page(Layout::Footed, &cover());
        assert!(document.paragraph_count() > paragraphs, "nothing was put in");

        assert!(document.remove_cover_page(), "it would not come off");
        assert_eq!(document.plain_text(), was, "the document is not as it was");
        assert_eq!(document.paragraph_count(), paragraphs, "something was left behind");
        assert!(!document.has_cover_page());
    }

    #[test]
    fn taking_it_off_can_be_taken_back() {
        // One act to a person is one thing to undo.
        let mut document = document(&["The body of it"]);
        document.insert_cover_page(Layout::Centred, &cover());
        document.remove_cover_page();
        assert!(!document.has_cover_page());

        assert!(document.undo());
        assert!(document.has_cover_page(), "undoing did not put it back");
    }

    #[test]
    fn a_document_that_merely_begins_with_a_title_is_not_a_cover_page() {
        // The mark and not the words: a program that guessed would take away
        // somebody's first page.
        let document = document(&["The Title", "The body of it"]);
        assert!(!document.has_cover_page());
    }
}
