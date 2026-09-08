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
use crate::{edit, position, Document, TextPosition};

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

        for (offset, block) in blocks.iter().enumerate() {
            parent.insert_element(position + offset, edit::block_element(block, prefix.as_deref()));
        }

        // The caret goes to the start of what used to be the first paragraph,
        // which is where the document proper now begins.
        self.set_caret(TextPosition::new(blocks.len(), 0));
        self.mark_modified();
        true
    }
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
                bottom: Some(Border {
                    style: "single".to_owned(),
                    size: 12,
                    color: Some("2E74B5".to_owned()),
                }),
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
