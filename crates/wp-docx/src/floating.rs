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
use crate::{edit, position, read, Document, TextPosition};

/// How a drawing is turned: how far round, and whether it is mirrored.
///
/// One word for both kinds, because Word's Rotate menu does not care which it
/// is turning and neither does anything above this.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Turned {
    /// Sixtieths of a thousandth of a degree, clockwise. A whole turn is
    /// 21,600,000, which is the unit the whole of DrawingML measures angles in.
    pub rotation: i32,
    pub flipped_across: bool,
    pub flipped_down: bool,
}

impl Turned {
    /// A whole turn, in the unit the format counts angles in.
    pub const WHOLE: i32 = 21_600_000;

    /// The same turn with a quarter added or taken away.
    #[must_use]
    pub fn turned_by(self, sixtieths: i32) -> Self {
        let rotation = (self.rotation + sixtieths).rem_euclid(Self::WHOLE);
        Self { rotation, ..self }
    }

    /// Whether it is turned or mirrored at all.
    #[must_use]
    pub fn is_turned(self) -> bool {
        self.rotation != 0 || self.flipped_across || self.flipped_down
    }

    /// The angle in radians, which is what drawing it asks for.
    #[must_use]
    pub fn radians(self) -> f32 {
        self.rotation as f32 / Self::WHOLE as f32 * core::f32::consts::TAU
    }

    /// How the drawing under an element is turned.
    ///
    /// The turn lives in the graphic's own `a:xfrm`, wherever that is below the
    /// wrapper: `pic:spPr/a:xfrm` for a picture, `wps:spPr/a:xfrm` for a shape.
    /// A drawing that was never turned carries no transform at all.
    #[must_use]
    pub fn under(element: &Element) -> Self {
        fn search(element: &Element) -> Option<&Element> {
            if element.local_name() == "xfrm" {
                return Some(element);
            }
            element.child_elements().find_map(search)
        }
        let Some(transform) = search(element) else { return Self::default() };
        Self {
            rotation: transform
                .attribute_by_name("rot")
                .and_then(|value| value.parse().ok())
                .unwrap_or(0),
            flipped_across: matches!(transform.attribute_by_name("flipH"), Some("1" | "true")),
            flipped_down: matches!(transform.attribute_by_name("flipV"), Some("1" | "true")),
        }
    }

    /// How a shape of the model is turned.
    #[must_use]
    pub fn of_shape(shape: &crate::shapes::Shape) -> Self {
        Self {
            rotation: shape.rotation,
            flipped_across: shape.flipped_across,
            flipped_down: shape.flipped_down,
        }
    }
}

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
    /// Which drawing the caret is beside, as the place that drawing is at.
    ///
    /// A drawing takes one character, so a caret "on" one is at either end of
    /// that character — and two drawings side by side are both beside a caret
    /// between them. The one after the caret is the answer, and the one before
    /// it where there is nothing after: that is the reading which makes a
    /// drawing just typed the drawing meant.
    ///
    /// The place returned is the drawing's own, and every `_at` below takes one
    /// of those rather than a caret. A drawing chosen with the mouse is named
    /// exactly, whatever else is beside it.
    #[must_use]
    pub fn drawing_place_here(&self) -> Option<TextPosition> {
        let caret = self.caret();
        if self.drawing_at(caret) {
            return Some(caret);
        }
        let before = TextPosition::new(caret.paragraph, caret.offset.checked_sub(1)?);
        self.drawing_at(before).then_some(before)
    }

    /// Whether a drawing of any kind is at one place in the text.
    #[must_use]
    pub fn drawing_at(&self, at: TextPosition) -> bool {
        self.shape_at(at).is_some() || self.picture_drawing_at(at).is_some()
    }

    /// Where the drawing beside the caret floats, if it is a drawing and it
    /// floats.
    ///
    /// A shape and a picture both answer. Nothing else is a drawing.
    #[must_use]
    pub fn anchor_here(&self) -> Option<Anchor> {
        self.anchor_at(self.drawing_place_here()?)
    }

    /// Where the drawing at one place floats, if it floats.
    #[must_use]
    pub fn anchor_at(&self, at: TextPosition) -> Option<Anchor> {
        if let Some(shape) = self.shape_at(at) {
            return shape.anchor;
        }
        anchor::read_anchor(self.picture_drawing_at(at)?)
    }

    /// Whether the caret is beside a drawing of any kind.
    #[must_use]
    pub fn drawing_here(&self) -> bool {
        self.drawing_place_here().is_some()
    }

    /// How big the drawing beside the caret is, in English Metric Units.
    #[must_use]
    pub fn drawing_size_here(&self) -> Option<(i64, i64)> {
        self.drawing_size_at(self.drawing_place_here()?)
    }

    /// How big the drawing at one place is.
    #[must_use]
    pub fn drawing_size_at(&self, at: TextPosition) -> Option<(i64, i64)> {
        if let Some(shape) = self.shape_at(at) {
            return Some((shape.width_emu, shape.height_emu));
        }
        let extent = find_extent(self.picture_drawing_at(at)?)?;
        Some((number(extent, "cx"), number(extent, "cy")))
    }

    /// The `w:drawing` element at one place, when a shape is not what is there.
    fn picture_drawing_at(&self, at: TextPosition) -> Option<&Element> {
        let paragraph = self.paragraph_element(at.paragraph)?;
        let mut offset = 0usize;
        let mut found = None;
        walk_drawings(paragraph, &mut offset, at.offset, &mut found);
        found
    }

    /// How far round the drawing at one place is turned, and whether it is
    /// drawn as its own mirror image.
    #[must_use]
    pub fn drawing_turn_at(&self, at: TextPosition) -> Turned {
        if let Some(shape) = self.shape_at(at) {
            return Turned::of_shape(&shape);
        }
        self.picture_drawing_at(at).map_or_else(Turned::default, Turned::under)
    }

    /// Turns it.
    ///
    /// A shape is rebuilt from its model, as every other change to one is; a
    /// picture's own transform is changed where it stands, because rebuilding a
    /// picture would throw away everything the model does not hold. See the note
    /// at the top of this file.
    pub fn set_drawing_turn_at(&mut self, at: TextPosition, turned: Turned) -> bool {
        if let Some(mut shape) = self.shape_at(at) {
            shape.rotation = turned.rotation;
            shape.flipped_across = turned.flipped_across;
            shape.flipped_down = turned.flipped_down;
            return self.replace_shape_at(at, &shape);
        }

        let caret = self.caret();
        if !self.drawing_at(at) {
            return false;
        }
        self.record(EditKind::Structural, caret, false);
        let Some(path) = position::paragraph_path(&self.tree().root, at.paragraph) else {
            return false;
        };
        let Some(paragraph) = edit::element_at_path_mut(&mut self.tree_mut().root, &path) else {
            return false;
        };

        let mut offset = 0usize;
        let mut done = false;
        walk_drawings_mut(paragraph, &mut offset, at.offset, &mut |drawing| {
            done = turn(drawing, turned);
        });
        if done {
            self.mark_modified();
        }
        done
    }

    /// Makes it that big.
    ///
    /// A drawing says its size twice — once on the box the text flows round and
    /// once on the graphic's own transform — and Word believes the box. Both are
    /// written, because a document that said two different sizes would be a
    /// document that looked different in two programs.
    pub fn set_drawing_size_here(&mut self, width_emu: i64, height_emu: i64) -> bool {
        let Some(at) = self.drawing_place_here() else { return false };
        self.set_drawing_size_at(at, width_emu, height_emu)
    }

    /// Makes the drawing at one place that big.
    pub fn set_drawing_size_at(
        &mut self,
        at: TextPosition,
        width_emu: i64,
        height_emu: i64,
    ) -> bool {
        let (width_emu, height_emu) = (width_emu.max(1), height_emu.max(1));
        if let Some(mut shape) = self.shape_at(at) {
            shape.width_emu = width_emu;
            shape.height_emu = height_emu;
            return self.replace_shape_at(at, &shape);
        }

        let caret = self.caret();
        if !self.drawing_at(at) {
            return false;
        }
        self.record(EditKind::Structural, caret, false);
        let Some(path) = position::paragraph_path(&self.tree().root, at.paragraph) else {
            return false;
        };
        let Some(paragraph) = edit::element_at_path_mut(&mut self.tree_mut().root, &path) else {
            return false;
        };

        let mut offset = 0usize;
        let mut done = false;
        walk_drawings_mut(paragraph, &mut offset, at.offset, &mut |drawing| {
            done = resize(drawing, width_emu, height_emu);
        });
        if done {
            self.mark_modified();
        }
        done
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
        let Some(at) = self.drawing_place_here() else { return false };
        self.set_anchor_at(at, anchor)
    }

    /// Sets where the drawing at one place floats, or puts it back in the line.
    pub fn set_anchor_at(&mut self, at: TextPosition, anchor: Option<&Anchor>) -> bool {
        // A shape is rebuilt from its model, which is what every other command
        // that changes one does.
        if let Some(mut shape) = self.shape_at(at) {
            shape.anchor = anchor.cloned();
            return self.replace_shape_at(at, &shape);
        }

        let caret = self.caret();
        if !self.drawing_at(at) {
            return false;
        }
        self.record(EditKind::Structural, caret, false);

        let prefix = self.prefix();
        let Some(path) = position::paragraph_path(&self.tree().root, at.paragraph) else {
            return false;
        };
        let Some(paragraph) = edit::element_at_path_mut(&mut self.tree_mut().root, &path) else {
            return false;
        };

        let mut offset = 0usize;
        let mut done = false;
        walk_drawings_mut(paragraph, &mut offset, at.offset, &mut |drawing| {
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

/// Finds the drawing at an offset.
fn walk_drawings<'a>(
    element: &'a Element,
    offset: &mut usize,
    wanted: usize,
    found: &mut Option<&'a Element>,
) {
    for node in &element.children {
        let Some(child) = node.as_element() else { continue };
        if child.namespace.as_deref() == Some(read::W) && child.local_name() == "drawing" {
            // The offset wanted is the drawing's own, not a caret beside it:
            // which of two neighbours is meant was settled before this.
            if *offset == wanted {
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
            if *offset == wanted {
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

/// Writes a turn onto a drawing's own transform, making one if it has none.
///
/// The angle lives on `a:xfrm`, which is inside the graphic rather than on the
/// box the text flows round: the box stays where it is and the picture inside
/// it turns, which is why a turned picture still keeps the words out of the
/// same rectangle. A drawing whose transform says nothing is given one.
fn turn(drawing: &mut Element, turned: Turned) -> bool {
    fn write(element: &mut Element, turned: Turned, done: &mut bool) {
        if element.local_name() == "xfrm" {
            if turned.rotation == 0 {
                element.attributes.retain(|held| !held.name.ends_with("rot"));
            } else {
                element.set_attribute("rot", &turned.rotation.to_string());
            }
            for (name, on) in [("flipH", turned.flipped_across), ("flipV", turned.flipped_down)] {
                if on {
                    element.set_attribute(name, "1");
                } else {
                    element.attributes.retain(|held| !held.name.ends_with(name));
                }
            }
            *done = true;
            return;
        }
        for child in element.child_elements_mut() {
            write(child, turned, done);
        }
    }

    let mut done = false;
    write(drawing, turned, &mut done);
    if done {
        return true;
    }

    // No transform at all: one is made inside the picture's own properties,
    // which is where the format puts it.
    fn made(turned: Turned) -> Element {
        let mut transform = Element::new("a:xfrm", Some(edit::DRAWING_MAIN));
        if turned.rotation != 0 {
            transform.set_attribute("rot", &turned.rotation.to_string());
        }
        if turned.flipped_across {
            transform.set_attribute("flipH", "1");
        }
        if turned.flipped_down {
            transform.set_attribute("flipV", "1");
        }
        transform
    }

    fn give_one(element: &mut Element, turned: Turned, done: &mut bool) {
        if element.local_name() == "spPr" {
            element.insert_element(0, made(turned));
            *done = true;
            return;
        }
        for child in element.child_elements_mut() {
            give_one(child, turned, done);
        }
    }
    give_one(drawing, turned, &mut done);
    if done {
        return true;
    }

    // Not even the properties a transform lives in. Word always writes them,
    // but a document from something terser need not, and a picture nobody can
    // turn because of what it does not say would be a button doing nothing.
    // They go last inside the graphic's own element, which is where the schema
    // has them, and take that element's prefix: a new one would need a
    // declaration to go with it.
    fn give_properties(element: &mut Element, turned: Turned, done: &mut bool) {
        if matches!(element.local_name(), "pic" | "wsp" | "sp") {
            let prefix = element.name.split_once(':').map(|(prefix, _)| prefix.to_owned());
            let name = prefix.map_or_else(|| "spPr".to_owned(), |prefix| format!("{prefix}:spPr"));
            let mut properties = Element::new(&name, element.namespace.as_deref());
            properties.push_element(made(turned));
            element.push_element(properties);
            *done = true;
            return;
        }
        for child in element.child_elements_mut() {
            give_properties(child, turned, done);
        }
    }
    give_properties(drawing, turned, &mut done);
    done
}

/// The box a drawing's text flows round.
fn find_extent(drawing: &Element) -> Option<&Element> {
    fn search<'a>(element: &'a Element, local: &str) -> Option<&'a Element> {
        if element.local_name() == local {
            return Some(element);
        }
        element.child_elements().find_map(|child| search(child, local))
    }
    search(drawing, "extent")
}

/// An attribute read as a number, or nothing.
fn number(element: &Element, name: &str) -> i64 {
    element.attribute_by_name(name).and_then(|text| text.parse().ok()).unwrap_or(0)
}

/// Writes a size onto every element of a drawing that states one.
///
/// `wp:extent` is the box, and `a:ext` inside the graphic's transform is the
/// graphic itself. A drawing that stated one and not the other would be drawn
/// at one size here and another in Word.
fn resize(drawing: &mut Element, width_emu: i64, height_emu: i64) -> bool {
    fn write(element: &mut Element, width_emu: i64, height_emu: i64, done: &mut bool) {
        if matches!(element.local_name(), "extent" | "ext") {
            element.set_attribute("cx", &width_emu.to_string());
            element.set_attribute("cy", &height_emu.to_string());
            *done = true;
        }
        for child in element.child_elements_mut() {
            write(child, width_emu, height_emu, done);
        }
    }
    let mut done = false;
    write(drawing, width_emu, height_emu, &mut done);
    done
}
