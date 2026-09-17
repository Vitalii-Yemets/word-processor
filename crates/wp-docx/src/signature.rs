//! A line for somebody to sign on.
//!
//! # What this is
//!
//! Word's Signature Line does two things at once. It draws a line to sign
//! above, with a cross, a name and a title under it — and it is something a
//! digital signature can be *about*: double-clicking one signs the document
//! and the signature records which line it was for.
//!
//! Both are here now. The drawing is paragraphs, below; the second half needs
//! only one thing of them, which is that a line can be named. So each is
//! wrapped in a bookmark whose name carries an identifier nothing else uses,
//! and a signature written for that line carries the same identifier — which
//! is what `SetupID` is in a signature's `SignatureInfoV1`, and what Word
//! keeps in the signature line's own picture. See [`crate::signing`] for the
//! signature that names one.
//!
//! # Why a bookmark and not Word's picture
//!
//! Word marks its signature line with a `v:shape` carrying an
//! `o:signatureline` element. A reader that does not know that element sees a
//! picture and nothing else, and cannot select the name or search for it. A
//! bookmark round paragraphs is text that prints the same, is searchable, and
//! survives a round trip through Word untouched — and it carries the
//! identifier, which is all the signing half asks of it.
//!
//! # Why it is paragraphs and not a picture
//!
//! Word draws its signature line as a picture with an extension attached. A
//! reader that does not know the extension sees the picture and nothing else,
//! and cannot select the name or search for it. Written as paragraphs with a
//! border, the same thing prints, and the name is text.

use crate::history::EditKind;
use crate::model::{
    Block, Border, Paragraph, ParagraphBorders, ParagraphProperties, Run, RunContent, RunProperties,
};
use crate::{edit, position, Document, TextPosition};

/// What a bookmark round a signature line is called, before its identifier.
///
/// Underscored, so that it does not show in a list of bookmarks a person
/// keeps: it is the program's own mark and not one anybody typed. See
/// [`Bookmark::is_hidden`](crate::bookmarks::Bookmark::is_hidden).
pub const LINE_MARK: &str = "_sigline_";

/// Who is to sign, and what they are called.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Signer {
    pub name: String,
    /// Their title under the line, which Word leaves out when it is empty.
    pub title: String,
    /// What this line is called, so that a signature can say it signed this
    /// one and not the other.
    ///
    /// Empty writes a line nothing can be about, which is what a line put in
    /// before this existed is. Where it comes from is the caller's business:
    /// this crate has no source of random bytes and one invented from the
    /// clock would collide the moment two lines went in together.
    pub id: String,
}

/// A signature line as it stands in a document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub id: String,
    pub name: String,
    pub title: String,
    /// Which paragraph it begins at, so that a person can be taken to it.
    pub at: TextPosition,
}

impl Signer {
    /// Reads a signer from one line typed as `name; title`.
    #[must_use]
    pub fn parse(typed: &str) -> Self {
        let mut parts = typed.split(';');
        let name = parts.next().unwrap_or_default().trim().to_owned();
        let title = parts.next().unwrap_or_default().trim().to_owned();
        Self { name, title, id: String::new() }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.name.trim().is_empty()
    }
}

impl Document {
    /// Puts a line to sign on where the caret is.
    ///
    /// Returns whether anything was put in. A line with nobody to sign it is
    /// not one, so an empty name inserts nothing.
    pub fn insert_signature_line(&mut self, signer: &Signer) -> bool {
        if signer.is_empty() {
            return false;
        }
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);

        let blocks = signature_blocks(signer);
        let prefix = self.prefix();
        let Some(path) = position::paragraph_path(&self.tree().root, caret.paragraph) else {
            return false;
        };
        let Some((position, parent_path)) = path.split_last() else { return false };
        // After the paragraph the caret is in, so a line asked for at the end
        // of a sentence does not cut the sentence in half.
        let position = *position + 1;
        let parent_path = parent_path.to_vec();
        let Some(parent) = edit::element_at_path_mut(&mut self.tree_mut().root, &parent_path)
        else {
            return false;
        };

        for (offset, block) in blocks.iter().enumerate() {
            parent.insert_element(position + offset, edit::block_element(block, prefix.as_deref()));
        }

        // Named, so that a signature can be about this line rather than about
        // the document at large. The mark goes on after the paragraphs are in,
        // because what it names is where they landed.
        let first = caret.paragraph + 1;
        let last = caret.paragraph + blocks.len();
        if !signer.id.trim().is_empty() {
            self.set_caret(TextPosition::new(first, 0));
            self.move_caret(TextPosition::new(last, 0), true);
            self.add_bookmark_quietly(&format!("{LINE_MARK}{}", signer.id));
        }

        // Into the paragraph after the line, which is where writing carries on.
        self.set_caret(TextPosition::new(last, 0));
        self.mark_modified();
        true
    }

    /// Every signature line the document carries, in the order they appear.
    ///
    /// Read back out of the document rather than kept beside it, for the
    /// reason everything else in this program is: a list kept alongside goes
    /// stale the moment somebody deletes a paragraph, and the document is the
    /// only thing that knows what is in it.
    ///
    /// A line somebody has taken apart — the bookmark still there, the
    /// paragraphs gone — comes back with whatever is left of its name, because
    /// what it is for is pairing a signature with a place, and a signature
    /// that names a line nobody can find is worth saying so about.
    #[must_use]
    pub fn signature_lines(&self) -> Vec<Line> {
        let body = self.body();
        let mut out = Vec::new();
        for mark in self.bookmarks() {
            let Some(id) = mark.name.strip_prefix(LINE_MARK) else { continue };
            // The paragraphs it covers: the cross, the name, and the title
            // where there is one. The cross is a line to sign above and not
            // anybody's name, so it is stepped over.
            let mut words = Vec::new();
            for index in mark.range.0.paragraph..=mark.range.1.paragraph {
                let Some(crate::model::Block::Paragraph(paragraph)) = body.blocks.get(index) else {
                    continue;
                };
                let text = paragraph.plain_text();
                let text = text.trim();
                if text.is_empty() || text == CROSS {
                    continue;
                }
                words.push(text.to_owned());
            }
            let mut words = words.into_iter();
            out.push(Line {
                id: id.to_owned(),
                name: words.next().unwrap_or_default(),
                title: words.next().unwrap_or_default(),
                at: mark.range.0,
            });
        }
        out
    }

    /// The line of a given name, if the document still has it.
    #[must_use]
    pub fn signature_line(&self, id: &str) -> Option<Line> {
        self.signature_lines().into_iter().find(|line| line.id == id)
    }
}

/// The mark above the line, which is where a pen goes and not a name.
const CROSS: &str = "X";

/// The paragraphs a signature line is made of.
#[must_use]
fn signature_blocks(signer: &Signer) -> Vec<Block> {
    let mut blocks = vec![
        // Room above, so the line does not sit against the text before it.
        Block::Paragraph(Paragraph {
            properties: ParagraphProperties { space_after: Some(240), ..default_properties() },
            runs: Vec::new(),
        }),
        // The cross, with the line under the whole paragraph.
        Block::Paragraph(Paragraph {
            properties: ParagraphProperties {
                borders: ParagraphBorders {
                    bottom: Some(Border::line("single", 6, Some("000000"))),
                    ..ParagraphBorders::default()
                },
                space_after: Some(0),
                ..default_properties()
            },
            runs: vec![Run::text(CROSS)],
        }),
        Block::Paragraph(name_paragraph(&signer.name)),
    ];

    if !signer.title.trim().is_empty() {
        blocks.push(Block::Paragraph(title_paragraph(&signer.title)));
    }
    blocks
}

fn default_properties() -> ParagraphProperties {
    ParagraphProperties::default()
}

/// The signer's name, under the line.
fn name_paragraph(name: &str) -> Paragraph {
    Paragraph {
        properties: ParagraphProperties { space_after: Some(0), ..default_properties() },
        runs: vec![Run {
            properties: RunProperties { size_half_points: Some(18), ..RunProperties::default() },
            content: vec![RunContent::Text(name.to_owned())],
            field: None,
            revision: None,
            format_change: None,
        }],
    }
}

/// And their title, smaller still.
fn title_paragraph(title: &str) -> Paragraph {
    Paragraph {
        properties: ParagraphProperties { space_after: Some(240), ..default_properties() },
        runs: vec![Run {
            properties: RunProperties {
                size_half_points: Some(16),
                italic: Some(true),
                color: Some("595959".to_owned()),
                ..RunProperties::default()
            },
            content: vec![RunContent::Text(title.to_owned())],
            field: None,
            revision: None,
            format_change: None,
        }],
    }
}

impl Document {
    /// Puts another document's text in at the caret.
    ///
    /// Word's Insert ▸ Object ▸ Text from File. The paragraphs, their
    /// formatting and their tables come across.
    ///
    /// # What does not come across
    ///
    /// Anything that points at a part of the other package: its pictures, its
    /// footnotes, its comments. A picture is not something a paragraph holds,
    /// it is something a paragraph refers to — see
    /// [`Document::insert_picture`] — and a reference copied without the thing
    /// it refers to is a broken document. Rather than write one, they are left
    /// behind, and the text arrives whole.
    pub fn insert_text_from(&mut self, other: &Document) -> usize {
        let blocks = other.body().blocks;
        if blocks.is_empty() {
            return 0;
        }

        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);

        let prefix = self.prefix();
        let Some(path) = position::paragraph_path(&self.tree().root, caret.paragraph) else {
            return 0;
        };
        let Some((position, parent_path)) = path.split_last() else { return 0 };
        let position = *position + 1;
        let parent_path = parent_path.to_vec();
        let Some(parent) = edit::element_at_path_mut(&mut self.tree_mut().root, &parent_path)
        else {
            return 0;
        };

        for (offset, block) in blocks.iter().enumerate() {
            parent.insert_element(position + offset, edit::block_element(block, prefix.as_deref()));
        }

        // At the start of what was brought in, which is what a person wants to
        // look at once it has arrived.
        self.set_caret(TextPosition::new(caret.paragraph + 1, 0));
        self.mark_modified();
        blocks.len()
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_signer_is_read_from_a_name_and_a_title() {
        let signer = Signer::parse("Ann Roe; Head of Sales");
        assert_eq!(signer.name, "Ann Roe");
        assert_eq!(signer.title, "Head of Sales");
    }

    #[test]
    fn a_name_on_its_own_is_a_signer_with_no_title() {
        let signer = Signer::parse("Ann Roe");
        assert_eq!(signer.name, "Ann Roe");
        assert!(signer.title.is_empty());
    }

    #[test]
    fn nothing_typed_is_nobody() {
        assert!(Signer::parse("   ").is_empty());
        assert!(Signer::parse("").is_empty());
    }

    #[test]
    fn a_signature_line_has_a_line_to_sign_on() {
        let blocks = signature_blocks(&Signer::parse("Ann Roe"));
        let ruled = blocks.iter().any(|block| match block {
            Block::Paragraph(paragraph) => {
                paragraph.properties.borders.bottom.as_ref().is_some_and(Border::is_visible)
            }
            _ => false,
        });
        assert!(ruled, "nothing to sign above");
    }

    #[test]
    fn a_signature_line_carries_the_name_and_the_title() {
        let blocks = signature_blocks(&Signer::parse("Ann Roe; Head of Sales"));
        let text: Vec<String> = blocks.iter().map(Block::plain_text).collect();
        assert!(text.iter().any(|line| line == "Ann Roe"), "{text:?}");
        assert!(text.iter().any(|line| line == "Head of Sales"), "{text:?}");
        assert!(text.iter().any(|line| line == "X"), "{text:?}");
    }

    #[test]
    fn no_title_means_no_paragraph_for_one() {
        let with = signature_blocks(&Signer::parse("Ann Roe; Head of Sales"));
        let without = signature_blocks(&Signer::parse("Ann Roe"));
        assert_eq!(without.len() + 1, with.len());
    }
}

#[cfg(test)]
mod lines {
    use super::*;
    use crate::model::{Block, Body, Paragraph};

    fn document() -> Document {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("Yours faithfully,")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        Document::open(&bytes).expect("reopening")
    }

    #[test]
    fn a_line_comes_back_by_the_name_it_was_given() {
        let mut document = document();
        document.set_caret(TextPosition::new(0, 0));
        assert!(document.insert_signature_line(&Signer {
            name: String::from("Ada Lovelace"),
            title: String::from("Director"),
            id: String::from("9a3f"),
        }));

        let lines = document.signature_lines();
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert_eq!(lines[0].id, "9a3f");
        assert_eq!(lines[0].name, "Ada Lovelace");
        assert_eq!(lines[0].title, "Director");
        assert_eq!(
            document.signature_line("9a3f").map(|line| line.name),
            Some("Ada Lovelace".to_owned())
        );
        assert_eq!(document.signature_line("nothing"), None);
    }

    #[test]
    fn the_name_survives_being_written_out_and_read_back() {
        // The whole point of the mark is that a signature made today can be
        // paired with a line somebody put in last week, which means it has to
        // live in the file and not in this program's memory.
        let mut document = document();
        document.set_caret(TextPosition::new(0, 0));
        document.insert_signature_line(&Signer {
            name: String::from("Grace Hopper"),
            title: String::new(),
            id: String::from("c17b"),
        });

        let bytes = document.save().expect("saving");
        let reopened = Document::open(&bytes).expect("reopening");
        let lines = reopened.signature_lines();
        assert_eq!(lines.len(), 1, "the mark did not survive a round trip");
        assert_eq!(lines[0].id, "c17b");
        assert_eq!(lines[0].name, "Grace Hopper");
        assert_eq!(lines[0].title, "", "a line with no title invented one");
    }

    #[test]
    fn a_line_with_no_name_is_still_a_line_and_nothing_can_be_about_it() {
        // Which is what a line put in before any of this existed is. It draws
        // and prints; it simply cannot be signed for.
        let mut document = document();
        document.set_caret(TextPosition::new(0, 0));
        assert!(document.insert_signature_line(&Signer {
            name: String::from("Somebody"),
            title: String::new(),
            id: String::new(),
        }));
        assert!(document.signature_lines().is_empty());
        assert!(document.plain_text().contains("Somebody"), "the line itself was not put in");
    }

    #[test]
    fn the_mark_is_not_shown_among_a_persons_own_bookmarks() {
        // It is the program's own and nobody typed it; a person looking at
        // their bookmarks should not have to wonder what it is.
        let mut document = document();
        document.set_caret(TextPosition::new(0, 0));
        document.insert_signature_line(&Signer {
            name: String::from("Ada Lovelace"),
            title: String::new(),
            id: String::from("9a3f"),
        });
        let marks = document.bookmarks();
        assert_eq!(marks.len(), 1);
        assert!(marks[0].is_hidden(), "{}", marks[0].name);
    }
}
