//! Groups: several drawings written as one.
//!
//! # Why a group has two rectangles
//!
//! Because a group is a drawing with a world inside it. The outer rectangle —
//! `a:off` and `a:ext` on the group's own transform, and the `wp:extent` above
//! it — says where the group is and how big it is drawn. The inner one —
//! `a:chOff` and `a:chExt` — says what coordinates the drawings inside it are
//! measured in.
//!
//! The two need not agree, and when they do not the group is a magnifying
//! glass: a group drawn two inches wide whose children are measured in a
//! four-inch space draws every child at half size. Word resizes a group by
//! changing the outer rectangle and leaving the inner one alone, which is
//! exactly why the two exist — the children keep the numbers they were written
//! with however often the group is dragged.
//!
//! So a member is placed by a proportion and never by a distance: where it sits
//! in the inner rectangle is where it sits in the outer one, as a fraction of
//! each. See [`Group::fractions`].
//!
//! # Why the element is carried through and not rebuilt
//!
//! The same reason a picture's is, and more so: a group holds pictures, and
//! rebuilding one from what is modelled here would throw away every crop,
//! every effect and every recolouring of every picture in it. So the model is
//! read-only — it is what the layout reads to draw the group — and the
//! commands that make and break groups move the elements themselves rather
//! than writing new ones from the model. See [`crate::Document::group_drawings`].

use wp_xml::tree::Element;

use crate::anchor::Anchor;
use crate::floating::Turned;
use crate::model::Picture;
use crate::shapes::Shape;
use crate::TextPosition;

/// The namespace a word-processing group is written in.
pub const WPG: &str = "http://schemas.microsoft.com/office/word/2010/wordprocessingGroup";

/// Several drawings written as one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Group {
    /// What the group is called, which is what a list of drawings shows.
    pub name: String,
    /// What it shows, said in words, for anyone who cannot see it.
    pub description: String,
    /// How big the whole group is drawn, in English Metric Units.
    pub width_emu: i64,
    pub height_emu: i64,
    /// Where it floats, or `None` when it sits in the line of text.
    pub anchor: Option<Anchor>,
    /// The rectangle the members are measured in: `a:chOff` and `a:chExt`.
    ///
    /// Not the rectangle the group is drawn in. See the note at the top of
    /// this file.
    pub child_x: i64,
    pub child_y: i64,
    pub child_width: i64,
    pub child_height: i64,
    /// How far round the whole group is turned, and whether it is mirrored.
    pub turned: Turned,
    /// What is in it, in the order it is drawn: first is furthest back.
    pub members: Vec<Member>,
}

/// One drawing inside a group, and where it sits in the group's own space.
///
/// The place and the size are on the member rather than inside it because that
/// is where the format puts them: a picture in a group has no `wp:extent` of
/// its own, only the `a:xfrm` that says where in the group it goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    pub x_emu: i64,
    pub y_emu: i64,
    pub width_emu: i64,
    pub height_emu: i64,
    pub what: Inside,
}

/// What a group holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Inside {
    Shape(Shape),
    Picture(Picture),
    /// A group of its own. Word lets a group hold a group, and a diagram
    /// pasted as drawings arrives as several levels of them.
    Group(Box<Group>),
}

impl Group {
    /// The width in points, which is the unit the layout works in.
    #[must_use]
    pub fn width_points(&self) -> f64 {
        self.width_emu as f64 / crate::shapes::EMU_PER_POINT as f64
    }

    #[must_use]
    pub fn height_points(&self) -> f64 {
        self.height_emu as f64 / crate::shapes::EMU_PER_POINT as f64
    }

    /// Where a member sits in the group's drawn rectangle, as fractions of it.
    ///
    /// Left, top, width and height, each a fraction of the group's own — which
    /// is all the layout needs, and keeps the two rectangles' arithmetic here
    /// rather than in the middle of placing a page.
    ///
    /// A group that states no inner rectangle is its own: the members are then
    /// measured in the same units as the group, which is the reading that makes
    /// such a group draw rather than collapse.
    #[must_use]
    pub fn fractions(&self, member: &Member) -> (f32, f32, f32, f32) {
        let across = self.inner_width() as f32;
        let down = self.inner_height() as f32;
        (
            (member.x_emu - self.child_x) as f32 / across,
            (member.y_emu - self.child_y) as f32 / down,
            member.width_emu as f32 / across,
            member.height_emu as f32 / down,
        )
    }

    /// The inner rectangle's width, never zero.
    fn inner_width(&self) -> i64 {
        match (self.child_width, self.width_emu) {
            (0, 0) => 1,
            (0, width) => width,
            (width, _) => width,
        }
    }

    fn inner_height(&self) -> i64 {
        match (self.child_height, self.height_emu) {
            (0, 0) => 1,
            (0, height) => height,
            (height, _) => height,
        }
    }

    /// Every drawing in the group and in the groups inside it, counted.
    ///
    /// What "Ungroup" would have to take apart, and what a count of the
    /// document's drawings has to agree with.
    #[must_use]
    pub fn depth_first_count(&self) -> usize {
        self.members
            .iter()
            .map(|member| match &member.what {
                Inside::Group(group) => 1 + group.depth_first_count(),
                _ => 1,
            })
            .sum()
    }
}

/// Reads a `w:drawing` that holds a group, or nothing when it holds something
/// else.
///
/// Tried before a shape and before a picture, and it has to be: a group holds
/// shapes and pictures, so a reader looking for the first `wps:wsp` under a
/// drawing would read a whole group as its first shape.
#[must_use]
pub fn read_group(drawing: &Element) -> Option<Group> {
    let wgp = find(drawing, "wgp")?;
    let mut group = Group { anchor: crate::anchor::read_anchor(drawing), ..Group::default() };

    if let Some(properties) = find(drawing, "docPr") {
        if let Some(name) = properties.attribute_by_name("name") {
            group.name = name.to_owned();
        }
        if let Some(description) = properties.attribute_by_name("descr") {
            group.description = description.to_owned();
        }
    }
    if group.name.is_empty() {
        group.name = "Group".to_owned();
    }

    // The size is on the drawing's box, which is what the text flows round.
    if let Some(extent) = find(drawing, "extent") {
        group.width_emu = number(extent, "cx");
        group.height_emu = number(extent, "cy");
    }

    read_into(wgp, &mut group);
    Some(group)
}

/// Reads a `wpg:grpSp`, which is a group inside a group.
///
/// The same thing without the `w:drawing` wrapper: a nested group's size comes
/// from its own transform, because there is no box above it to state one.
fn read_nested(grp: &Element) -> Group {
    let mut group = Group { name: "Group".to_owned(), ..Group::default() };
    if let Some(properties) = find(grp, "cNvPr") {
        if let Some(name) = properties.attribute_by_name("name") {
            group.name = name.to_owned();
        }
    }
    read_into(grp, &mut group);
    group
}

/// Everything a group and a nested group read the same way: the transform and
/// the members.
fn read_into(wgp: &Element, group: &mut Group) {
    if let Some(transform) = properties_of(wgp).and_then(|properties| child(properties, "xfrm")) {
        group.turned = Turned {
            rotation: attribute(transform, "rot"),
            flipped_across: matches!(transform.attribute_by_name("flipH"), Some("1" | "true")),
            flipped_down: matches!(transform.attribute_by_name("flipV"), Some("1" | "true")),
        };
        if let Some(extent) = child(transform, "ext") {
            if group.width_emu == 0 || group.height_emu == 0 {
                group.width_emu = number(extent, "cx");
                group.height_emu = number(extent, "cy");
            }
        }
        if let Some(offset) = child(transform, "chOff") {
            group.child_x = number(offset, "x");
            group.child_y = number(offset, "y");
        }
        if let Some(extent) = child(transform, "chExt") {
            group.child_width = number(extent, "cx");
            group.child_height = number(extent, "cy");
        }
    }

    for child_element in wgp.child_elements() {
        let Some(what) = inside(child_element) else { continue };
        let (x, y, width, height) = member_box(child_element);
        group.members.push(Member {
            x_emu: x,
            y_emu: y,
            width_emu: width,
            height_emu: height,
            what,
        });
    }
}

/// What one child of a group is, when it is a drawing at all.
///
/// A group's own properties and its non-visual properties are children too,
/// and are not drawings.
fn inside(element: &Element) -> Option<Inside> {
    match element.local_name() {
        "wsp" => crate::shapes::read_shape(element).map(Inside::Shape),
        "pic" => crate::read::read_picture(element).map(Inside::Picture),
        "grpSp" => Some(Inside::Group(Box::new(read_nested(element)))),
        _ => None,
    }
}

/// Where a member sits in its group, from its own transform.
fn member_box(element: &Element) -> (i64, i64, i64, i64) {
    let Some(transform) = properties_of(element).and_then(|properties| child(properties, "xfrm"))
    else {
        return (0, 0, 0, 0);
    };
    let (mut x, mut y) = (0, 0);
    if let Some(offset) = child(transform, "off") {
        x = number(offset, "x");
        y = number(offset, "y");
    }
    let (mut width, mut height) = (0, 0);
    if let Some(extent) = child(transform, "ext") {
        width = number(extent, "cx");
        height = number(extent, "cy");
    }
    (x, y, width, height)
}

/// The properties element a transform lives in, whichever kind of drawing it
/// belongs to: `spPr` for a shape or a picture, `grpSpPr` for a group.
fn properties_of(element: &Element) -> Option<&Element> {
    element.child_elements().find(|child| matches!(child.local_name(), "spPr" | "grpSpPr"))
}

/// A child by local name, whatever prefix it was written with.
fn child<'a>(parent: &'a Element, local: &str) -> Option<&'a Element> {
    parent.child_elements().find(|child| child.local_name() == local)
}

/// The named element anywhere under a root, the root itself included.
fn find<'a>(root: &'a Element, local: &str) -> Option<&'a Element> {
    if root.local_name() == local {
        return Some(root);
    }
    root.child_elements().find_map(|child| find(child, local))
}

fn number(element: &Element, name: &str) -> i64 {
    element.attribute_by_name(name).and_then(|value| value.parse().ok()).unwrap_or(0)
}

fn attribute(element: &Element, name: &str) -> i32 {
    element.attribute_by_name(name).and_then(|value| value.parse().ok()).unwrap_or(0)
}

impl crate::Document {
    /// Every group in the document, in reading order.
    #[must_use]
    pub fn groups(&self) -> Vec<Group> {
        let mut out = Vec::new();
        for block in &self.body().blocks {
            gather(block, &mut out);
        }
        out
    }

    /// The group at one place in the text, if a group is what is there.
    ///
    /// Read from the element rather than looked up in the model, so that it
    /// answers for a group in a header or a footnote as readily as one in the
    /// body: every `_at` in [`crate::floating`] works the same way.
    #[must_use]
    pub fn group_at(&self, at: crate::TextPosition) -> Option<Group> {
        read_group(self.drawing_element_at(at)?)
    }
}

/// Finds every group in a block, whatever it is nested in.
fn gather(block: &crate::model::Block, out: &mut Vec<Group>) {
    match block {
        crate::model::Block::Paragraph(paragraph) => {
            for run in &paragraph.runs {
                for piece in &run.content {
                    if let crate::model::RunContent::Group(group) = piece {
                        out.push(group.clone());
                    }
                }
            }
        }
        crate::model::Block::Table(table) => {
            for row in &table.rows {
                for cell in &row.cells {
                    for block in &cell.blocks {
                        gather(block, out);
                    }
                }
            }
        }
    }
}

/// The namespace a picture's own graphic is written in.
const PICTURE: &str = "http://schemas.openxmlformats.org/drawingml/2006/picture";

/// A rectangle in English Metric Units.
///
/// Measured from wherever the caller measures from — only the differences
/// between them matter, because a group is made where its members already are
/// and the whole point is that nothing appears to move. What knows where a
/// drawing is on the page is the layout, so the places are given here rather
/// than worked out here: the same reasoning Align follows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: i64,
    pub y: i64,
    pub width: i64,
    pub height: i64,
}

impl Rect {
    /// The smallest rectangle holding both.
    #[must_use]
    pub fn with(self, other: Self) -> Self {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        let right = (self.x + self.width).max(other.x + other.width);
        let bottom = (self.y + self.height).max(other.y + other.height);
        Self { x, y, width: right - x, height: bottom - y }
    }
}

impl crate::Document {
    /// Makes one drawing of several: Word's Group.
    ///
    /// Each member is given as the rectangle it covers on the page, in English
    /// Metric Units measured from any one origin. The group covers all of them
    /// and hangs where the first of them hung, so nothing appears to move.
    ///
    /// The members' elements are *moved* into the group and never rebuilt: a
    /// group holds pictures, and a picture rebuilt from what is modelled here
    /// would lose its crop, its effects and its recolouring. See the note at
    /// the top of this file.
    ///
    /// Returns where the new group is, or nothing when there was not more than
    /// one drawing to make it of.
    pub fn group_drawings(&mut self, members: &[(TextPosition, Rect)]) -> Option<TextPosition> {
        if members.len() < 2 {
            return None;
        }

        // Everything about every member, gathered before anything is taken
        // out: once the first is gone the places of the rest would be wrong.
        let mut taken: Vec<(TextPosition, Rect, Element)> = Vec::new();
        for (at, rect) in members {
            let element = self.drawing_element_at(*at)?.clone();
            taken.push((*at, *rect, element));
        }

        // In the order they are drawn, which is the order they stand in the
        // text: the first is furthest back, and a group keeps that order.
        taken.sort_by_key(|(at, _, _)| (at.paragraph, at.offset));
        let first = taken.first()?;
        let where_it_goes = first.0;
        let union = taken.iter().skip(1).fold(first.1, |so_far, (_, rect, _)| so_far.with(*rect));

        // The group hangs where the first member hung, moved by however far
        // that member stands from the corner of the group.
        let depth = self.next_drawing_depth();
        let anchor = grouped_anchor(
            self.anchor_at(where_it_goes),
            union.x - first.1.x,
            union.y - first.1.y,
            depth,
        );

        let name = format!("Group {}", self.groups().len() + 1);
        let graphics: Vec<(Rect, Element)> = taken
            .iter()
            .filter_map(|(_, rect, element)| {
                let inner = graphic_of(element)?.clone();
                let placed = Rect {
                    x: rect.x - union.x,
                    y: rect.y - union.y,
                    width: rect.width,
                    height: rect.height,
                };
                Some((placed, inner))
            })
            .collect();
        // A drawing this program cannot find the graphic of is one it must not
        // silently drop: better no group than a group missing a member.
        if graphics.len() != taken.len() {
            return None;
        }

        let caret = self.caret();
        self.record(crate::history::EditKind::Structural, caret, false);

        // Taken out from the last backwards, so that removing one does not
        // move the places of the others.
        let mut places: Vec<TextPosition> = taken.iter().map(|(at, _, _)| *at).collect();
        places.sort_by_key(|at| (at.paragraph, at.offset));
        for at in places.iter().rev() {
            crate::position::delete_range(
                &mut self.tree_mut().root,
                at.paragraph,
                at.offset,
                at.offset + 1,
            );
        }

        let prefix = self.prefix();
        let element = group_element(union, &graphics, &anchor, &name, prefix.as_deref());
        if !crate::position::insert_element_at(
            &mut self.tree_mut().root,
            where_it_goes,
            element,
            prefix.as_deref(),
        ) {
            return None;
        }

        self.set_caret(TextPosition::new(where_it_goes.paragraph, where_it_goes.offset + 1));
        self.mark_modified();
        Some(where_it_goes)
    }

    /// Takes a group apart: Word's Ungroup.
    ///
    /// Every member becomes a drawing of its own, floating where it was drawn.
    /// A group inside the group becomes a group in its own right rather than
    /// being taken apart too, because one Ungroup undoes one Group.
    ///
    /// Returns where the drawings that came out of it now are, in the order
    /// they are drawn — which is what Regroup needs to put them back.
    pub fn ungroup_at(&mut self, at: TextPosition) -> Vec<TextPosition> {
        let Some(group) = self.group_at(at) else { return Vec::new() };
        let Some(drawing) = self.drawing_element_at(at).cloned() else { return Vec::new() };
        let Some(wgp) = graphic_of(&drawing).cloned() else { return Vec::new() };
        if group.members.is_empty() {
            return Vec::new();
        }

        // The elements of the members, beside the model of each: the model says
        // where each one sits, and the element is what is kept.
        let inners: Vec<Element> = wgp
            .child_elements()
            .filter(|child| matches!(child.local_name(), "wsp" | "pic" | "grpSp"))
            .cloned()
            .collect();
        // The model and the elements have to agree, or a member would be put
        // back in the wrong place.
        if inners.len() != group.members.len() {
            return Vec::new();
        }

        // Where the group itself hangs, which every member now hangs from with
        // its own place inside the group added on.
        let anchor = self.anchor_at(at);
        let depth = self.next_drawing_depth();

        let caret = self.caret();
        self.record(crate::history::EditKind::Structural, caret, false);
        crate::position::delete_range(
            &mut self.tree_mut().root,
            at.paragraph,
            at.offset,
            at.offset + 1,
        );

        let prefix = self.prefix();
        let mut out = Vec::new();
        for (index, (member, inner)) in group.members.iter().zip(inners).enumerate() {
            let (fraction_x, fraction_y, fraction_width, fraction_height) = group.fractions(member);
            let scaled = |fraction: f32, whole: i64| (f64::from(fraction) * whole as f64) as i64;
            let rect = Rect {
                x: scaled(fraction_x, group.width_emu),
                y: scaled(fraction_y, group.height_emu),
                width: scaled(fraction_width, group.width_emu),
                height: scaled(fraction_height, group.height_emu),
            };
            let hangs = grouped_anchor(anchor.clone(), rect.x, rect.y, depth + index as u32);
            let element = loose_element(rect, &inner, &hangs, prefix.as_deref());
            let place = TextPosition::new(at.paragraph, at.offset + index);
            if crate::position::insert_element_at(
                &mut self.tree_mut().root,
                place,
                element,
                prefix.as_deref(),
            ) {
                out.push(place);
            }
        }

        self.mark_modified();
        out
    }
}

/// The anchor a drawing takes when it goes into a group or comes out of one.
///
/// The same arithmetic Align and a drag use: a distance is the same distance
/// whatever frame it is measured in, so a drawing is moved by a difference
/// rather than given a position worked out in a frame nobody here knows. A
/// drawing that was in the line of text starts floating, because a drawing in
/// the line has no place of its own to be moved from.
fn grouped_anchor(anchor: Option<Anchor>, across: i64, down: i64, depth: u32) -> Anchor {
    use crate::anchor::{Placement, Wrap};
    let mut anchor = anchor.unwrap_or(Anchor { wrap: Wrap::Square, depth, ..Anchor::default() });
    let was_across = match anchor.horizontal {
        Placement::Offset(distance) => distance,
        // A drawing lined up with an edge and then moved is no longer lined up
        // with it, so the alignment gives way to a distance.
        Placement::Aligned(_) => 0,
    };
    let was_down = match anchor.vertical {
        Placement::Offset(distance) => distance,
        Placement::Aligned(_) => 0,
    };
    anchor.horizontal = Placement::Offset(was_across + across);
    anchor.vertical = Placement::Offset(was_down + down);
    anchor.depth = depth;
    anchor
}

/// The graphic under a `w:drawing`: the `wps:wsp`, `pic:pic` or `wpg:wgp` that
/// says what the drawing is a drawing of.
fn graphic_of(drawing: &Element) -> Option<&Element> {
    let data = find(drawing, "graphicData")?;
    data.child_elements().find(|child| matches!(child.local_name(), "wsp" | "pic" | "wgp"))
}

/// Builds the `w:drawing` that holds a group.
fn group_element(
    union: Rect,
    members: &[(Rect, Element)],
    anchor: &Anchor,
    name: &str,
    prefix: Option<&str>,
) -> Element {
    use crate::shapes::{A, WP};

    let mut drawing =
        Element::new(&crate::edit::name_with(prefix, "drawing"), Some(crate::read::W));
    let mut floating = Element::new("wp:anchor", Some(WP));
    floating.declarations.push((Some("wp".to_owned()), WP.to_owned()));
    crate::anchor::write_anchor(anchor, &mut floating, WP);

    let mut extent = Element::new("wp:extent", Some(WP));
    extent.set_attribute("cx", &union.width.to_string());
    extent.set_attribute("cy", &union.height.to_string());
    floating.push_element(extent);
    floating.push_element(crate::anchor::wrap_element(anchor, WP));

    let mut visible = Element::new("wp:docPr", Some(WP));
    visible.set_attribute("id", "1");
    visible.set_attribute("name", name);
    floating.push_element(visible);

    let mut graphic = Element::new("a:graphic", Some(A));
    graphic.declarations.push((Some("a".to_owned()), A.to_owned()));
    let mut data = Element::new("a:graphicData", Some(A));
    data.set_attribute("uri", WPG);

    let mut wgp = Element::new("wpg:wgp", Some(WPG));
    wgp.declarations.push((Some("wpg".to_owned()), WPG.to_owned()));
    wgp.push_element(Element::new("wpg:cNvGrpSpPr", Some(WPG)));

    let mut properties = Element::new("wpg:grpSpPr", Some(WPG));
    // The two rectangles start out the same: a group just made is drawn at the
    // size its members already were, so nothing in it is magnified.
    properties.push_element(transform(
        Rect { x: 0, y: 0, width: union.width, height: union.height },
        true,
    ));
    wgp.push_element(properties);

    for (rect, member) in members {
        let mut member = member.clone();
        // A group put inside a group stops being a drawing in its own right
        // and becomes a member, which the format spells differently.
        if member.local_name() == "wgp" {
            member.name = "wpg:grpSp".to_owned();
        }
        set_member_box(&mut member, *rect);
        wgp.push_element(member);
    }

    data.push_element(wgp);
    graphic.push_element(data);
    floating.push_element(graphic);
    drawing.push_element(floating);
    drawing
}

/// Builds the `w:drawing` round one drawing taken out of a group.
fn loose_element(rect: Rect, member: &Element, anchor: &Anchor, prefix: Option<&str>) -> Element {
    use crate::shapes::{A, WP, WPS};

    let mut drawing =
        Element::new(&crate::edit::name_with(prefix, "drawing"), Some(crate::read::W));
    let mut floating = Element::new("wp:anchor", Some(WP));
    floating.declarations.push((Some("wp".to_owned()), WP.to_owned()));
    crate::anchor::write_anchor(anchor, &mut floating, WP);

    let mut extent = Element::new("wp:extent", Some(WP));
    extent.set_attribute("cx", &rect.width.to_string());
    extent.set_attribute("cy", &rect.height.to_string());
    floating.push_element(extent);
    floating.push_element(crate::anchor::wrap_element(anchor, WP));

    let mut visible = Element::new("wp:docPr", Some(WP));
    visible.set_attribute("id", "1");
    visible.set_attribute("name", name_of(member));
    floating.push_element(visible);

    let mut member = member.clone();
    // And a member that was a group becomes a drawing in its own right again.
    if member.local_name() == "grpSp" {
        member.name = "wpg:wgp".to_owned();
    }
    // It stands at its own corner now rather than somewhere inside a group.
    set_member_box(&mut member, Rect { x: 0, y: 0, width: rect.width, height: rect.height });

    let uri = match member.local_name() {
        "wsp" => WPS,
        "wgp" => WPG,
        _ => PICTURE,
    };

    let mut graphic = Element::new("a:graphic", Some(A));
    graphic.declarations.push((Some("a".to_owned()), A.to_owned()));
    let mut data = Element::new("a:graphicData", Some(A));
    data.set_attribute("uri", uri);
    data.push_element(member);
    graphic.push_element(data);
    floating.push_element(graphic);
    drawing.push_element(floating);
    drawing
}

/// What a drawing calls itself, for the `wp:docPr` of a drawing built round it.
fn name_of(member: &Element) -> &str {
    find(member, "cNvPr")
        .and_then(|properties| properties.attribute_by_name("name"))
        .unwrap_or("Drawing")
}

/// An `a:xfrm` stating a rectangle, and the same rectangle again for the
/// children when it belongs to a group.
fn transform(rect: Rect, with_children: bool) -> Element {
    use crate::shapes::A;
    let mut xfrm = Element::new("a:xfrm", Some(A));
    let mut offset = Element::new("a:off", Some(A));
    offset.set_attribute("x", &rect.x.to_string());
    offset.set_attribute("y", &rect.y.to_string());
    xfrm.push_element(offset);
    let mut extent = Element::new("a:ext", Some(A));
    extent.set_attribute("cx", &rect.width.to_string());
    extent.set_attribute("cy", &rect.height.to_string());
    xfrm.push_element(extent);
    if with_children {
        let mut child_offset = Element::new("a:chOff", Some(A));
        child_offset.set_attribute("x", &rect.x.to_string());
        child_offset.set_attribute("y", &rect.y.to_string());
        xfrm.push_element(child_offset);
        let mut child_extent = Element::new("a:chExt", Some(A));
        child_extent.set_attribute("cx", &rect.width.to_string());
        child_extent.set_attribute("cy", &rect.height.to_string());
        xfrm.push_element(child_extent);
    }
    xfrm
}

/// Says where a member sits in its group, keeping whatever else its transform
/// said: how far round it is turned, and which way it is mirrored.
///
/// The children's rectangle is never touched. On a group that is the space the
/// drawings inside are measured in, and changing it would move every one of
/// them.
fn set_member_box(member: &mut Element, rect: Rect) {
    use crate::shapes::A;
    let group = matches!(member.local_name(), "grpSp" | "wgp");
    let wanted = if group { "grpSpPr" } else { "spPr" };

    let prefix = member.name.split_once(':').map(|(prefix, _)| prefix.to_owned());
    if member.child_elements().all(|child| child.local_name() != wanted) {
        let name = prefix.map_or_else(|| wanted.to_owned(), |prefix| format!("{prefix}:{wanted}"));
        // Last, where the schema has it.
        member.push_element(Element::new(&name, member.namespace.clone().as_deref()));
    }
    let Some(properties) = member.child_elements_mut().find(|child| child.local_name() == wanted)
    else {
        return;
    };

    if properties.child_elements().all(|child| child.local_name() != "xfrm") {
        properties.insert_element(0, Element::new("a:xfrm", Some(A)));
    }
    let Some(xfrm) = properties.child_elements_mut().find(|child| child.local_name() == "xfrm")
    else {
        return;
    };

    xfrm.children.retain(|node| {
        node.as_element().is_none_or(|child| !matches!(child.local_name(), "off" | "ext"))
    });

    let mut offset = Element::new("a:off", Some(A));
    offset.set_attribute("x", &rect.x.to_string());
    offset.set_attribute("y", &rect.y.to_string());
    let mut extent = Element::new("a:ext", Some(A));
    extent.set_attribute("cx", &rect.width.to_string());
    extent.set_attribute("cy", &rect.height.to_string());
    xfrm.insert_element(0, extent);
    xfrm.insert_element(0, offset);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(child_width: i64, child_height: i64) -> Group {
        Group {
            width_emu: 1_000_000,
            height_emu: 500_000,
            child_width,
            child_height,
            ..Group::default()
        }
    }

    fn member(x: i64, y: i64, width: i64, height: i64) -> Member {
        Member {
            x_emu: x,
            y_emu: y,
            width_emu: width,
            height_emu: height,
            what: Inside::Shape(Shape::default()),
        }
    }

    #[test]
    fn a_member_filling_the_inner_rectangle_fills_the_group() {
        let group = group(2_000_000, 1_000_000);
        let whole = member(0, 0, 2_000_000, 1_000_000);
        assert_eq!(group.fractions(&whole), (0.0, 0.0, 1.0, 1.0));
    }

    #[test]
    fn the_inner_rectangle_is_a_magnifying_glass() {
        // Members measured in twice the space the group is drawn in come out
        // at half the size, which is how Word resizes a group.
        let group = group(2_000_000, 1_000_000);
        let half = member(1_000_000, 500_000, 1_000_000, 500_000);
        assert_eq!(group.fractions(&half), (0.5, 0.5, 0.5, 0.5));
    }

    #[test]
    fn the_inner_rectangle_need_not_start_at_nothing() {
        // Word writes the coordinates the drawings had where they stood, so
        // the inner offset is rarely zero.
        let mut group = group(2_000_000, 1_000_000);
        group.child_x = 1_000_000;
        group.child_y = 500_000;
        let at_the_corner = member(1_000_000, 500_000, 1_000_000, 500_000);
        assert_eq!(group.fractions(&at_the_corner), (0.0, 0.0, 0.5, 0.5));
    }

    #[test]
    fn a_group_that_states_no_inner_rectangle_is_its_own() {
        let group = group(0, 0);
        let half = member(500_000, 250_000, 500_000, 250_000);
        assert_eq!(group.fractions(&half), (0.5, 0.5, 0.5, 0.5));
    }

    #[test]
    fn an_empty_group_divides_by_something() {
        // Nothing to place, but the arithmetic still must not divide by zero.
        let mut group = group(0, 0);
        group.width_emu = 0;
        group.height_emu = 0;
        assert_eq!(group.fractions(&member(0, 0, 0, 0)), (0.0, 0.0, 0.0, 0.0));
    }

    #[test]
    fn the_drawings_inside_a_group_are_counted_through_the_groups_inside_it() {
        let mut inner = group(0, 0);
        inner.members.push(member(0, 0, 10, 10));
        inner.members.push(member(0, 0, 10, 10));

        let mut outer = group(0, 0);
        outer.members.push(member(0, 0, 10, 10));
        outer.members.push(Member {
            x_emu: 0,
            y_emu: 0,
            width_emu: 10,
            height_emu: 10,
            what: Inside::Group(Box::new(inner)),
        });

        // One shape, one group, and the two shapes inside that group.
        assert_eq!(outer.depth_first_count(), 4);
    }
}
