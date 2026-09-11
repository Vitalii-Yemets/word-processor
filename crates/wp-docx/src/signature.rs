//! A line for somebody to sign on.
//!
//! # What this is and what it is not
//!
//! Word's Signature Line does two things at once. It draws a line to sign
//! above, with a cross, a name and a title under it — and it puts a signature
//! control in the document that, when double-clicked, asks for a *digital*
//! signature and seals the file with a certificate.
//!
//! The first is here. The second is not, and would be dishonest to fake: it is
//! cryptography, it needs a certificate store, and a document claiming to be
//! signed when nothing signed it is worse than one that makes no claim. What
//! this writes is the line as it prints — which is what the great majority of
//! signature lines are ever used for, because somebody prints the page and
//! signs it with a pen.
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

/// Who is to sign, and what they are called.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Signer {
    pub name: String,
    /// Their title under the line, which Word leaves out when it is empty.
    pub title: String,
}

impl Signer {
    /// Reads a signer from one line typed as `name; title`.
    #[must_use]
    pub fn parse(typed: &str) -> Self {
        let mut parts = typed.split(';');
        let name = parts.next().unwrap_or_default().trim().to_owned();
        let title = parts.next().unwrap_or_default().trim().to_owned();
        Self { name, title }
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

        // Into the paragraph after the line, which is where writing carries on.
        self.set_caret(TextPosition::new(caret.paragraph + blocks.len(), 0));
        self.mark_modified();
        true
    }
}

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
                    bottom: Some(Border {
                        style: "single".to_owned(),
                        size: 6,
                        color: Some("000000".to_owned()),
                    }),
                    ..ParagraphBorders::default()
                },
                space_after: Some(0),
                ..default_properties()
            },
            runs: vec![Run::text("X")],
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
