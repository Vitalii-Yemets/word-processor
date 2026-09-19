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

use wp_xml::tree::{Element, Node};

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

    /// This turn seen from inside another: the one turn that draws a drawing
    /// turned by `self` sitting in a group turned by `outer`.
    ///
    /// # Why the angles are not simply added
    ///
    /// Because a mirror reverses the turn it is applied to. Turning a shape
    /// right and then holding the whole group up to a mirror is the same as
    /// mirroring the shape and turning it *left*: the two do not commute, and
    /// adding the angles would draw the members of a mirrored group leaning the
    /// wrong way.
    ///
    /// Mirroring both ways at once is a half turn rather than a mirror, and a
    /// half turn commutes with everything — so that case adds after all. The
    /// mirrors themselves always simply combine: doing one twice undoes it.
    #[must_use]
    pub fn inside(self, outer: Self) -> Self {
        let reversed = outer.flipped_across != outer.flipped_down;
        let rotation =
            if reversed { outer.rotation - self.rotation } else { outer.rotation + self.rotation };
        Self {
            rotation: rotation.rem_euclid(Self::WHOLE),
            flipped_across: self.flipped_across != outer.flipped_across,
            flipped_down: self.flipped_down != outer.flipped_down,
        }
    }

    /// Where a point of a group lands once the group itself is turned.
    ///
    /// Given how far the point is from the group's middle, across and down,
    /// this is how far it is from that middle afterwards. What turns a member's
    /// place; [`Self::inside`] turns the member itself.
    #[must_use]
    pub fn moves(self, across: f32, down: f32) -> (f32, f32) {
        let across = if self.flipped_across { -across } else { across };
        let down = if self.flipped_down { -down } else { down };
        let (sin, cos) = self.radians().sin_cos();
        (across * cos - down * sin, across * sin + down * cos)
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

    /// The `w:drawing` element at one place, whatever kind of drawing it holds.
    ///
    /// The element and not the model: this is what every command that changes a
    /// drawing where it stands works on, and the reason a picture keeps
    /// everything about it this program does not understand.
    pub(crate) fn drawing_element_at(&self, at: TextPosition) -> Option<&Element> {
        let paragraph = self.paragraph_element(at.paragraph)?;
        let mut offset = 0usize;
        let mut found = None;
        walk_drawings(paragraph, &mut offset, at.offset, &mut found);
        found
    }

    /// The same, named for the one caller that wants a picture's.
    pub(crate) fn picture_drawing_at(&self, at: TextPosition) -> Option<&Element> {
        self.drawing_element_at(at)
    }

    /// Which part of the package the picture at one place is embedded from.
    ///
    /// The relationship rather than the bytes, because the caller is asking so
    /// that it can decode the picture and find the size it was drawn at —
    /// which is what Word's Scale boxes are a percentage of. A shape or a chart
    /// answers nothing: neither has an original size to be a percentage of.
    #[must_use]
    pub fn picture_relationship_at(&self, at: TextPosition) -> Option<String> {
        let drawing = self.drawing_element_at(at)?;
        find_blip(drawing)
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

    /// Moves the drawing at one place into another paragraph, and says where it
    /// has landed.
    ///
    /// # Why a drawing moves between paragraphs at all
    ///
    /// Because a floating drawing hangs from a paragraph, and Word re-hangs it
    /// on the paragraph it is dropped nearest. A picture dragged three pages
    /// down and still tied to where it came from would follow that paragraph
    /// about: add a line above it and the picture moves, which is not what
    /// anybody who dropped it three pages down meant. The drawing's own place
    /// on the page is the caller's business — it has just worked out where the
    /// drawing looks like it is. See [`crate::editor`]'s handles.
    ///
    /// Answers `None` and changes nothing when the two paragraphs do not live
    /// in the same parent: a drawing dropped over a table cell would have to
    /// become a drawing inside that cell, which is a different thing from
    /// moving it and is not done here.
    pub fn move_drawing_to(&mut self, at: TextPosition, paragraph: usize) -> Option<TextPosition> {
        if at.paragraph == paragraph {
            return None;
        }
        let root = &self.tree().root;
        let here = position::paragraph_path(root, at.paragraph)?;
        let there = position::paragraph_path(root, paragraph)?;
        let (_, from_parent) = here.split_last()?;
        let (_, to_parent) = there.split_last()?;
        if from_parent != to_parent {
            return None;
        }

        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);
        let prefix = self.prefix();

        // Out of the paragraph it was in, taking the run with it when the run
        // held nothing else: an empty run left behind is an empty run saved.
        let source = edit::element_at_path_mut(&mut self.tree_mut().root, &here)?;
        let taken = take_drawing(source, at.offset)?;

        // And into the other one, as a run of its own at the end.
        let there = position::paragraph_path(&self.tree().root, paragraph)?;
        let target = edit::element_at_path_mut(&mut self.tree_mut().root, &there)?;
        let mut run = Element::new(&edit::name_with(prefix.as_deref(), "r"), Some(read::W));
        run.push_element(taken);
        target.push_element(run);

        self.mark_modified();
        // Appended at the end, so it stands after everything the paragraph
        // holds: the offset is the length of what is there.
        let offset = self.paragraph_text(paragraph).map_or(0, |text| text.len());
        Some(TextPosition::new(paragraph, offset.saturating_sub(1)))
    }

    /// Sets where the drawing at one place floats, or puts it back in the line.
    pub fn set_anchor_at(&mut self, at: TextPosition, anchor: Option<&Anchor>) -> bool {
        // A size or a place stated as a percentage is written with the 2010
        // extension, which has to be declared and marked ignorable before
        // anything under it is written. See [`edit::declare_extension`].
        if anchor.is_some_and(anchor::needs_extension) {
            edit::declare_extension(&mut self.tree_mut().root, anchor::WP14_PREFIX, anchor::WP14);
        }

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
pub(crate) fn set_anchor_on(
    drawing: &mut Element,
    anchor: Option<&Anchor>,
    prefix: Option<&str>,
) -> bool {
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
            // The percentages the extension states are written again below, so
            // the ones that were there go first: a drawing that is no longer a
            // percentage of anything must not keep saying it is.
            wrapper.children.retain(|node| {
                node.as_element()
                    .is_none_or(|child| !matches!(child.local_name(), "sizeRelH" | "sizeRelV"))
            });
            anchor::write_anchor(anchor, wrapper, &namespace);

            // The wrap goes after the extent and before the name, which is
            // where the schema puts it. `write_anchor` has just put the
            // positions on the end, so the wrap goes after them.
            wrapper.push_element(anchor::wrap_element(anchor, &namespace));
            // And the 2010 extension goes at the very end, after the graphic,
            // which is where Word writes it. `reorder` leaves what it does not
            // name where it found it, so pushing it last is enough.
            for element in anchor::relative_size_elements(anchor) {
                wrapper.push_element(element);
            }
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
        if is_drawing(child) {
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

/// Whether an element is a drawing that stands for one character of the
/// text: `w:drawing`, or one of the older wrappers holding a picture — the
/// same rule [`edit::atomic_text`] counts by.
fn is_drawing(element: &Element) -> bool {
    element.namespace.as_deref() == Some(read::W)
        && (element.local_name() == "drawing"
            || (matches!(element.local_name(), "pict" | "object") && edit::holds_picture(element)))
}

/// The same, to change it.
pub(crate) fn walk_drawings_mut(
    element: &mut Element,
    offset: &mut usize,
    wanted: usize,
    act: &mut impl FnMut(&mut Element),
) {
    for node in &mut element.children {
        let Some(child) = node.as_element_mut() else { continue };
        if is_drawing(child) {
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
            // The first transform and no other. A shape and a picture have
            // only one, so this never mattered until a group arrived: a group
            // holds a transform of its own and one for every drawing in it,
            // and turning the lot would turn each member as well as the whole.
            if *done {
                return;
            }
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
            if *done {
                return;
            }
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
            if *done {
                return;
            }
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
        // Everything below a group but the drawings in it. A group states the
        // rectangle it is drawn in and, separately, the one its members are
        // measured in; resizing the first and leaving the second is exactly
        // how a group scales what is inside it. Writing the new size onto
        // every member as well would make them all the size of the group.
        // See [`crate::group`].
        let members_below = element.local_name() == "wgp";
        for child in element.child_elements_mut() {
            if members_below && matches!(child.local_name(), "wsp" | "pic" | "grpSp") {
                continue;
            }
            write(child, width_emu, height_emu, done);
        }
    }
    let mut done = false;
    write(drawing, width_emu, height_emu, &mut done);
    done
}

/// The relationship a `a:blip` under an element points at, if there is one.
///
/// Walked rather than reached directly, because how deep a blip sits depends on
/// what kind of drawing holds it: a picture's is three elements down, and one
/// used as a shape's fill is deeper still.
fn find_blip(element: &Element) -> Option<String> {
    if element.local_name() == "blip" {
        // By namespace and not by written name: the attribute is `r:embed`, and
        // what its prefix is is the document's business.
        if let Some(id) = element.attribute(Some(read::RELATIONSHIPS), "embed") {
            return Some(id.to_owned());
        }
    }
    element.child_elements().find_map(find_blip)
}

/// Takes the drawing at an offset out of a paragraph, and the run with it if
/// the run held nothing else.
fn take_drawing(paragraph: &mut Element, wanted: usize) -> Option<Element> {
    let mut offset = 0usize;
    let mut taken = None;
    lift_drawing(paragraph, &mut offset, wanted, &mut taken);
    taken
}

/// Walks a paragraph counting what the editor counts, and lifts one drawing
/// out of it.
fn lift_drawing(
    element: &mut Element,
    offset: &mut usize,
    wanted: usize,
    taken: &mut Option<Element>,
) {
    let mut at = 0usize;
    while at < element.children.len() {
        let Some(child) = element.children[at].as_element_mut() else {
            at += 1;
            continue;
        };
        let is = |local: &str| {
            child.namespace.as_deref() == Some(read::W) && child.local_name() == local
        };
        if is("drawing") {
            if *offset == wanted && taken.is_none() {
                let Node::Element(found) = element.children.remove(at) else { return };
                *taken = Some(found);
                // The run that held it is worth nothing without it, unless it
                // holds something else as well.
                if element.namespace.as_deref() == Some(read::W)
                    && element.local_name() == "r"
                    && element.child_elements().all(|left| left.local_name() == "rPr")
                {
                    element.children.clear();
                }
                return;
            }
            *offset += 1;
            at += 1;
            continue;
        }
        if is("t") {
            *offset += child.text_content().len();
            at += 1;
            continue;
        }
        lift_drawing(child, offset, wanted, taken);
        if taken.is_some() {
            // An empty run is dropped on the way back out.
            if child.namespace.as_deref() == Some(read::W)
                && child.local_name() == "r"
                && child.children.is_empty()
            {
                element.children.remove(at);
            }
            return;
        }
        at += 1;
    }
}
