//! Making a drawing float, and putting it back in the line.
//!
//! # Why this is surgery rather than a rewrite
//!
//! A shape is read into the model and written back out of it, so changing one
//! is a matter of changing the model and building the element again. A picture
//! is not: the model holds where it is and how big, and the element holds a
//! great deal more — the crop, the effects, the colour it was recoloured to,
//! the frame Word drew round it. Rebuilding a picture's element from what is
//! modelled would throw all of that away, and this program's first promise is
//! that a document opened here keeps everything it does not understand.
//!
//! So the anchor is changed where it stands. The wrapper is one element or the
//! other — `wp:inline` in the line, `wp:anchor` floating — and everything under
//! it is the same either way. Renaming the wrapper and changing its own
//! attributes and its own children is the whole of the difference, and the
//! graphic below it is never touched.
//!
//! # Why both kinds go through one door
//!
//! Because Word's Arrange group does not care which it is. Wrap Text, Position,
//! Bring Forward and the rest are about a drawing, and a picture is a drawing.
//! [`Document::anchor_here`] and [`Document::set_anchor_here`] answer for both,
//! so the commands above them stopped having to ask.

use wp_xml::tree::Element;

use crate::anchor::{self, Anchor};
use crate::history::EditKind;
use crate::{edit, position, read, Document};

/// The namespace a drawing's placement lives in.
const WP: &str = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";

/// The attributes an inline drawing carries and a floating one does not.
const INLINE_ONLY: &[&str] = &["distT", "distB", "distL", "distR"];

/// The attributes a floating drawing carries and an inline one does not.
const FLOATING_ONLY: &[&str] = &[
    "distT",
    "distB",
    "distL",
    "distR",
    "simplePos",
    "relativeHeight",
    "behindDoc",
    "locked",
    "layoutInCell",
    "allowOverlap",
];

/// The children a floating drawing carries and an inline one does not.
const FLOATING_CHILDREN: &[&str] = &[
    "simplePos",
    "positionH",
    "positionV",
    "wrapNone",
    "wrapSquare",
    "wrapTight",
    "wrapThrough",
    "wrapTopAndBottom",
];

impl Document {
    /// Where the drawing beside the caret floats, if it is a drawing and it
    /// floats.
    ///
    /// A shape and a picture both answer. Nothing else is a drawing.
    #[must_use]
    pub fn anchor_here(&self) -> Option<Anchor> {
        if let Some(shape) = self.shape_here() {
            return shape.anchor;
        }
        let caret = self.caret();
        let paragraph = self.paragraph_element(caret.paragraph)?;
        let mut offset = 0usize;
        let mut found = None;
        walk_drawings(paragraph, &mut offset, caret.offset, &mut found);
        anchor::read_anchor(found?)
    }

    /// Whether the caret is beside a drawing of any kind.
    #[must_use]
    pub fn drawing_here(&self) -> bool {
        if self.shape_here().is_some() {
            return true;
        }
        let caret = self.caret();
        let Some(paragraph) = self.paragraph_element(caret.paragraph) else { return false };
        let mut offset = 0usize;
        let mut found = None;
        walk_drawings(paragraph, &mut offset, caret.offset, &mut found);
        found.is_some()
    }

    /// The number every floating drawing in the document carries.
    ///
    /// Shapes and pictures together, because the pile they are ordered in is
    /// one pile: a picture laid over a shape is over it or under it, and a
    /// count that saw only one kind would put a new drawing among the others
    /// rather than on top of them. See [`crate::anchor::Anchor::depth`].
    #[must_use]
    pub fn drawing_depths(&self) -> Vec<u32> {
        let mut out: Vec<u32> = self
            .shapes()
            .iter()
            .filter_map(|shape| shape.anchor.as_ref())
            .map(|a| a.depth)
            .collect();
        for block in &self.body().blocks {
            gather_picture_depths(block, &mut out);
        }
        out
    }

    /// Sets where the drawing beside the caret floats, or puts it back in the
    /// line.
    ///
    /// Returns whether anything changed.
    pub fn set_anchor_here(&mut self, anchor: Option<&Anchor>) -> bool {
        // A shape is rebuilt from its model, which is what every other command
        // that changes one does.
        if let Some(mut shape) = self.shape_here() {
            shape.anchor = anchor.cloned();
            return self.replace_shape_here(&shape);
        }

        let caret = self.caret();
        if !self.drawing_here() {
            return false;
        }
        self.record(EditKind::Structural, caret, false);

        let prefix = self.prefix();
        let Some(path) = position::paragraph_path(&self.tree().root, caret.paragraph) else {
            return false;
        };
        let Some(paragraph) = edit::element_at_path_mut(&mut self.tree_mut().root, &path) else {
            return false;
        };

        let mut offset = 0usize;
        let mut done = false;
        walk_drawings_mut(paragraph, &mut offset, caret.offset, &mut |drawing| {
            done = set_anchor_on(drawing, anchor, prefix.as_deref());
        });
        if done {
            self.mark_modified();
        }
        done
    }
}

/// Changes one `w:drawing` between floating and in the line.
fn set_anchor_on(drawing: &mut Element, anchor: Option<&Anchor>, prefix: Option<&str>) -> bool {
    // Whichever wrapper is there: everything under it stays exactly as it is.
    let Some(wrapper) = drawing
        .child_elements_mut()
        .find(|child| matches!(child.local_name(), "inline" | "anchor"))
    else {
        return false;
    };
    let namespace = wrapper.namespace.clone().unwrap_or_else(|| WP.to_owned());
    let written = wrapper.prefix().map(str::to_owned);
    let name = |local: &str| match &written {
        Some(prefix) => format!("{prefix}:{local}"),
        None => local.to_owned(),
    };

    match anchor {
        Some(anchor) => {
            wrapper.name = name("anchor");
            for attribute in FLOATING_ONLY {
                wrapper.attributes.retain(|held| !held.name.ends_with(attribute));
            }
            wrapper.children.retain(|node| {
                node.as_element()
                    .is_none_or(|child| !FLOATING_CHILDREN.contains(&child.local_name()))
            });
            anchor::write_anchor(anchor, wrapper, &namespace);

            // The wrap goes after the extent and before the name, which is
            // where the schema puts it. `write_anchor` has just put the
            // positions on the end, so the wrap goes after them.
            wrapper.push_element(anchor::wrap_element(anchor, &namespace));
            reorder(wrapper);
        }
        None => {
            wrapper.name = name("inline");
            for attribute in FLOATING_ONLY {
                wrapper.attributes.retain(|held| !held.name.ends_with(attribute));
            }
            wrapper.children.retain(|node| {
                node.as_element()
                    .is_none_or(|child| !FLOATING_CHILDREN.contains(&child.local_name()))
            });
            for side in INLINE_ONLY {
                wrapper.set_attribute(side, "0");
            }
        }
    }
    let _ = prefix;
    true
}

/// Puts a floating wrapper's children in the order the schema wants.
///
/// `wp:simplePos`, then the two positions, then the extent, then the wrap, then
/// the name and the graphic. Word rejects a drawing whose children are out of
/// sequence, and the ones just added went on the end.
fn reorder(wrapper: &mut Element) {
    const ORDER: &[&str] = &[
        "simplePos",
        "positionH",
        "positionV",
        "extent",
        "effectExtent",
        "wrapNone",
        "wrapSquare",
        "wrapTight",
        "wrapThrough",
        "wrapTopAndBottom",
        "docPr",
        "cNvGraphicFramePr",
        "graphic",
    ];
    let rank = |node: &wp_xml::tree::Node| {
        node.as_element()
            .and_then(|child| ORDER.iter().position(|name| *name == child.local_name()))
            .unwrap_or(ORDER.len())
    };
    // A stable sort, so anything the order does not name keeps the place it had
    // relative to its neighbours.
    let mut children = std::mem::take(&mut wrapper.children);
    children.sort_by_key(rank);
    wrapper.children = children;
}

/// Finds the drawing beside an offset.
fn walk_drawings<'a>(
    element: &'a Element,
    offset: &mut usize,
    wanted: usize,
    found: &mut Option<&'a Element>,
) {
    for node in &element.children {
        let Some(child) = node.as_element() else { continue };
        if child.namespace.as_deref() == Some(read::W) && child.local_name() == "drawing" {
            // The drawing covers one character, so the caret is beside it when
            // it is at either end of that character.
            if *offset == wanted || *offset + 1 == wanted {
                *found = Some(child);
            }
            *offset += 1;
            continue;
        }
        if child.namespace.as_deref() == Some(read::W) && child.local_name() == "t" {
            *offset += child.text_content().len();
            continue;
        }
        walk_drawings(child, offset, wanted, found);
    }
}

/// The same, to change it.
fn walk_drawings_mut(
    element: &mut Element,
    offset: &mut usize,
    wanted: usize,
    act: &mut impl FnMut(&mut Element),
) {
    for node in &mut element.children {
        let Some(child) = node.as_element_mut() else { continue };
        if child.namespace.as_deref() == Some(read::W) && child.local_name() == "drawing" {
            if *offset == wanted || *offset + 1 == wanted {
                act(child);
            }
            *offset += 1;
            continue;
        }
        if child.namespace.as_deref() == Some(read::W) && child.local_name() == "t" {
            *offset += child.text_content().len();
            continue;
        }
        walk_drawings_mut(child, offset, wanted, act);
    }
}

/// Every floating picture's depth in a block, whatever it is nested in.
fn gather_picture_depths(block: &crate::model::Block, out: &mut Vec<u32>) {
    match block {
        crate::model::Block::Paragraph(paragraph) => {
            for run in &paragraph.runs {
                for piece in &run.content {
                    if let crate::model::RunContent::Picture(picture) = piece {
                        if let Some(anchor) = &picture.anchor {
                            out.push(anchor.depth);
                        }
                    }
                }
            }
        }
        crate::model::Block::Table(table) => {
            for row in &table.rows {
                for cell in &row.cells {
                    for block in &cell.blocks {
                        gather_picture_depths(block, out);
                    }
                }
            }
        }
    }
}
