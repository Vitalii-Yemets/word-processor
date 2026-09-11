//! Several drawings written as one: Word's Group, Ungroup, and a group read
//! from a document somebody else wrote.

use wp_docx::anchor::{Anchor, Placement, Wrap};
use wp_docx::group::{read_group, Inside, Rect};
use wp_docx::model::{Block, Body, Paragraph, RunContent};
use wp_docx::shapes::Shape;
use wp_docx::{Document, TextPosition};

/// A document of one paragraph with two floating shapes in it.
///
/// Both are one inch square; the second is an inch to the right of and an inch
/// below the first, so the group of them is two inches square.
fn two_shapes() -> Document {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("Words")));
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    let mut document = Document::open(&bytes).expect("reopening");

    for (name, offset) in [("One", 0i64), ("Two", 914_400)] {
        let shape = Shape {
            name: name.to_owned(),
            width_emu: 914_400,
            height_emu: 914_400,
            fill: Some("4472C4".to_owned()),
            anchor: Some(Anchor {
                wrap: Wrap::None,
                horizontal: Placement::Offset(offset),
                vertical: Placement::Offset(offset),
                ..Anchor::default()
            }),
            ..Shape::default()
        };
        document.set_caret(TextPosition::new(0, 0));
        assert!(document.insert_shape(&shape), "the shape went nowhere");
    }
    document
}

/// Where the two shapes of [`two_shapes`] are, and the places they sit at.
fn both() -> Vec<(TextPosition, Rect)> {
    // Two drawings at the front of the paragraph, so they take the first two
    // characters of it.
    vec![
        (TextPosition::new(0, 0), Rect { x: 914_400, y: 914_400, width: 914_400, height: 914_400 }),
        (TextPosition::new(0, 1), Rect { x: 0, y: 0, width: 914_400, height: 914_400 }),
    ]
}

#[test]
fn two_drawings_become_one() {
    let mut document = two_shapes();
    assert_eq!(document.shapes().len(), 2);

    let at = document.group_drawings(&both()).expect("nothing was grouped");
    assert_eq!(at, TextPosition::new(0, 0));

    let groups = document.groups();
    assert_eq!(groups.len(), 1, "there should be one group");
    assert_eq!(groups[0].members.len(), 2, "both drawings should be in it");
    assert!(document.shapes().is_empty(), "the shapes are in the group now, not beside it");
}

#[test]
fn the_group_covers_every_drawing_in_it() {
    let mut document = two_shapes();
    document.group_drawings(&both()).expect("nothing was grouped");

    let group = document.groups().into_iter().next().expect("a group");
    // Two one-inch squares an inch apart cover two inches each way.
    assert_eq!(group.width_emu, 1_828_800);
    assert_eq!(group.height_emu, 1_828_800);
}

#[test]
fn the_members_keep_the_places_they_had() {
    let mut document = two_shapes();
    document.group_drawings(&both()).expect("nothing was grouped");
    let group = document.groups().into_iter().next().expect("a group");

    // The first member in the list is the first in the text, which is the one
    // at the far corner: that is the order the drawings were in.
    let places: Vec<(f32, f32)> = group
        .members
        .iter()
        .map(|member| {
            let (x, y, _, _) = group.fractions(member);
            (x, y)
        })
        .collect();
    assert!(places.contains(&(0.0, 0.0)), "nothing is at the corner: {places:?}");
    assert!(places.contains(&(0.5, 0.5)), "nothing is in the middle: {places:?}");
}

#[test]
fn a_group_stays_a_group_when_the_document_is_saved_and_opened_again() {
    let mut document = two_shapes();
    document.group_drawings(&both()).expect("nothing was grouped");

    let bytes = document.save().expect("saving");
    let reopened = Document::open(&bytes).expect("reopening");
    let groups = reopened.groups();
    assert_eq!(groups.len(), 1, "the group did not survive");
    assert_eq!(groups[0].members.len(), 2);
    assert_eq!(groups[0].width_emu, 1_828_800);
    assert!(reopened.shapes().is_empty(), "the members came loose");
}

#[test]
fn a_group_is_one_character_of_the_text() {
    // Two drawings took two places; the group of them takes one.
    let mut document = two_shapes();
    let before = document.paragraph_text(0).expect("the text").len();
    document.group_drawings(&both()).expect("nothing was grouped");
    let after = document.paragraph_text(0).expect("the text").len();
    assert_eq!(after, before - 1, "a group should take one place, not two");
}

#[test]
fn ungrouping_gives_the_drawings_back() {
    let mut document = two_shapes();
    let at = document.group_drawings(&both()).expect("nothing was grouped");

    let loose = document.ungroup_at(at);
    assert_eq!(loose.len(), 2, "both should have come out");
    assert!(document.groups().is_empty(), "the group is still there");
    assert_eq!(document.shapes().len(), 2, "the shapes did not come back");
}

#[test]
fn grouping_and_ungrouping_leaves_the_drawings_where_they_were() {
    let mut document = two_shapes();
    let at = document.group_drawings(&both()).expect("nothing was grouped");
    document.ungroup_at(at);

    // The two are an inch apart again, whichever way round they came out.
    let mut corners: Vec<(i64, i64)> = Vec::new();
    for offset in 0..2 {
        let anchor =
            document.anchor_at(TextPosition::new(0, offset)).expect("a drawing that floats");
        let Placement::Offset(across) = anchor.horizontal else { panic!("not an offset") };
        let Placement::Offset(down) = anchor.vertical else { panic!("not an offset") };
        corners.push((across, down));
    }
    corners.sort_unstable();
    assert_eq!(corners, vec![(0, 0), (914_400, 914_400)], "they did not come back where they were");
}

#[test]
fn one_undo_takes_back_a_whole_group() {
    let mut document = two_shapes();
    document.group_drawings(&both()).expect("nothing was grouped");
    assert!(document.undo(), "there was nothing to undo");

    assert!(document.groups().is_empty(), "the group is still there");
    assert_eq!(document.shapes().len(), 2, "the drawings did not come back");
}

#[test]
fn one_drawing_is_not_a_group() {
    let mut document = two_shapes();
    let one = both().into_iter().take(1).collect::<Vec<_>>();
    assert!(document.group_drawings(&one).is_none(), "one drawing was made a group of itself");
    assert!(document.groups().is_empty());
}

#[test]
fn a_group_can_hold_a_group() {
    let mut document = two_shapes();
    let inner = document.group_drawings(&both()).expect("nothing was grouped");

    // A third drawing beside the group, and the two of them grouped again.
    let shape = Shape {
        name: "Three".to_owned(),
        width_emu: 914_400,
        height_emu: 914_400,
        fill: Some("ED7D31".to_owned()),
        anchor: Some(Anchor {
            wrap: Wrap::None,
            horizontal: Placement::Offset(1_828_800),
            vertical: Placement::Offset(0),
            ..Anchor::default()
        }),
        ..Shape::default()
    };
    document.set_caret(TextPosition::new(0, inner.offset + 1));
    assert!(document.insert_shape(&shape));

    let outer = document
        .group_drawings(&[
            (inner, Rect { x: 0, y: 0, width: 1_828_800, height: 1_828_800 }),
            (
                TextPosition::new(0, inner.offset + 1),
                Rect { x: 1_828_800, y: 0, width: 914_400, height: 914_400 },
            ),
        ])
        .expect("nothing was grouped");

    let group = document.group_at(outer).expect("a group");
    assert_eq!(group.members.len(), 2, "the outer group holds two things");
    assert!(
        group.members.iter().any(|member| matches!(member.what, Inside::Group(_))),
        "the group inside it was flattened"
    );
    // One group, one shape, and the two shapes in the group inside it.
    assert_eq!(group.depth_first_count(), 4);

    // And one Ungroup gives back the group and the shape, not four shapes.
    let loose = document.ungroup_at(outer);
    assert_eq!(loose.len(), 2);
    assert_eq!(document.groups().len(), 1, "the inner group should still be a group");
}

/// A group as Word writes one, built by hand so that reading it is what is
/// being tested rather than writing it.
const WORD_GROUP: &str = r#"<w:drawing><wp:inline distT="0" distB="0" distL="0" distR="0">
<wp:extent cx="1828800" cy="914400"/>
<wp:docPr id="1" name="Group 7" descr="two boxes"/>
<a:graphic><a:graphicData uri="http://schemas.microsoft.com/office/word/2010/wordprocessingGroup">
<wpg:wgp><wpg:cNvGrpSpPr/>
<wpg:grpSpPr><a:xfrm>
<a:off x="0" y="0"/><a:ext cx="1828800" cy="914400"/>
<a:chOff x="1000" y="2000"/><a:chExt cx="3657600" cy="1828800"/>
</a:xfrm></wpg:grpSpPr>
<wps:wsp><wps:cNvPr id="2" name="Left"/><wps:spPr>
<a:xfrm><a:off x="1000" y="2000"/><a:ext cx="1828800" cy="1828800"/></a:xfrm>
<a:prstGeom prst="ellipse"><a:avLst/></a:prstGeom>
<a:solidFill><a:srgbClr val="4472C4"/></a:solidFill>
</wps:spPr></wps:wsp>
<wps:wsp><wps:cNvPr id="3" name="Right"/><wps:spPr>
<a:xfrm rot="5400000"><a:off x="1828600" y="2000"/><a:ext cx="1828800" cy="1828800"/></a:xfrm>
<a:prstGeom prst="rect"><a:avLst/></a:prstGeom>
</wps:spPr></wps:wsp>
</wpg:wgp>
</a:graphicData></a:graphic>
</wp:inline></w:drawing>"#;

/// The document that drawing sits in, with every namespace it uses declared.
fn word_document(drawing: &str) -> Vec<u8> {
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
 xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"
 xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"
 xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"
 xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture"
 xmlns:wps="http://schemas.microsoft.com/office/word/2010/wordprocessingShape"
 xmlns:wpg="http://schemas.microsoft.com/office/word/2010/wordprocessingGroup">
<w:body><w:p><w:r><w:t>before</w:t>{drawing}<w:t>after</w:t></w:r></w:p></w:body>
</w:document>"#
    );

    let mut package = wp_opc::Package::empty();
    package.add_part("word/document.xml", wp_opc::MAIN_DOCUMENT_CONTENT_TYPE, xml.into_bytes());
    let mut root = wp_opc::Relationships::new("");
    root.add(
        wp_opc::OFFICE_DOCUMENT_RELATIONSHIP,
        "word/document.xml",
        wp_opc::TargetMode::Internal,
    );
    package.set_relationships(&root).expect("the root relationships");
    package.save().expect("a saved package")
}

#[test]
fn a_group_word_wrote_is_read_as_a_group() {
    let bytes = word_document(WORD_GROUP);
    let document = Document::open(&bytes).expect("a readable document");

    let groups = document.groups();
    assert_eq!(groups.len(), 1, "the group was read as something else");
    let group = &groups[0];
    assert_eq!(group.name, "Group 7");
    assert_eq!(group.description, "two boxes");
    assert_eq!((group.width_emu, group.height_emu), (1_828_800, 914_400));
    assert_eq!(group.members.len(), 2);
}

#[test]
fn a_group_word_wrote_is_not_read_as_the_first_shape_in_it() {
    // The reason a group is asked for first: every other reader would find the
    // shapes inside it and answer for one of them.
    let bytes = word_document(WORD_GROUP);
    let document = Document::open(&bytes).expect("a readable document");
    assert!(document.shapes().is_empty(), "a group was read as a shape");

    let Block::Paragraph(paragraph) = &document.body().blocks[0] else { panic!("a paragraph") };
    let kinds: Vec<&RunContent> = paragraph.runs.iter().flat_map(|run| &run.content).collect();
    assert!(kinds.iter().any(|piece| matches!(piece, RunContent::Group(_))));
    assert!(!kinds.iter().any(|piece| matches!(piece, RunContent::Shape(_))));
}

#[test]
fn the_members_are_measured_in_the_rectangle_the_group_states() {
    // The inner rectangle is twice the outer one, so everything in it is drawn
    // at half size — and the inner offset is not zero, so a member at the very
    // corner is at the corner of the group and not somewhere outside it.
    let bytes = word_document(WORD_GROUP);
    let document = Document::open(&bytes).expect("a readable document");
    let group = document.groups().into_iter().next().expect("a group");

    let first = &group.members[0];
    assert_eq!(group.fractions(first), (0.0, 0.0, 0.5, 1.0));
    let second = &group.members[1];
    let (x, _, width, _) = group.fractions(second);
    assert!((x - 0.5).abs() < 0.001, "the second is at {x} across");
    assert!((width - 0.5).abs() < 0.001);
}

#[test]
fn what_is_inside_a_group_is_read_as_what_it_is() {
    let bytes = word_document(WORD_GROUP);
    let document = Document::open(&bytes).expect("a readable document");
    let group = document.groups().into_iter().next().expect("a group");

    let Inside::Shape(first) = &group.members[0].what else { panic!("not a shape") };
    assert_eq!(first.preset, "ellipse", "the geometry of a member was lost");
    assert_eq!(first.fill.as_deref(), Some("4472C4"));
    assert_eq!(first.name, "Left");

    let Inside::Shape(second) = &group.members[1].what else { panic!("not a shape") };
    assert_eq!(second.rotation, 5_400_000, "a member's own turn was lost");
}

#[test]
fn a_group_from_a_document_is_a_drawing_like_any_other() {
    // Which is what lets Wrap Text, Position, the pile menus, Align and Rotate
    // act on a group without knowing that a group is what they are acting on.
    let bytes = word_document(WORD_GROUP);
    let mut document = Document::open(&bytes).expect("a readable document");
    let at = TextPosition::new(0, 6);

    assert!(document.drawing_at(at), "a group is not a drawing");
    assert_eq!(document.drawing_size_at(at), Some((1_828_800, 914_400)));
    assert!(document.set_drawing_turn_at(
        at,
        wp_docx::floating::Turned { rotation: 5_400_000, ..wp_docx::floating::Turned::default() }
    ));
    assert_eq!(document.drawing_turn_at(at).rotation, 5_400_000);

    // And the turn is the group's own, not the first member's.
    let saved = document.save().expect("saving");
    let reopened = Document::open(&saved).expect("reopening");
    let group = reopened.groups().into_iter().next().expect("a group");
    assert_eq!(group.turned.rotation, 5_400_000);
    let Inside::Shape(first) = &group.members[0].what else { panic!("not a shape") };
    assert_eq!(first.rotation, 0, "the member was turned instead of the group");
}

#[test]
fn a_group_element_read_straight_is_the_same_group() {
    // The reader on its own, with no document round it: what the layout uses
    // when it is handed a drawing rather than a place in the text.
    let bytes = word_document(WORD_GROUP);
    let document = Document::open(&bytes).expect("a readable document");
    let from_model = document.groups().into_iter().next().expect("a group");
    let from_element = document.group_at(TextPosition::new(0, 6)).expect("a group");
    assert_eq!(from_model, from_element);

    // And something that is not a group is not read as one.
    let mut nothing = wp_xml::tree::Element::new("w:drawing", None);
    nothing.push_element(wp_xml::tree::Element::new("wp:inline", None));
    assert!(read_group(&nothing).is_none());
}

#[test]
fn resizing_a_group_scales_what_is_in_it() {
    // Word resizes a group by changing the rectangle it is drawn in and
    // leaving the one its members are measured in, so everything inside comes
    // out at the new proportion without a single member being touched.
    let mut document = two_shapes();
    let at = document.group_drawings(&both()).expect("nothing was grouped");
    let before = document.group_at(at).expect("a group");
    let places: Vec<(f32, f32, f32, f32)> =
        before.members.iter().map(|member| before.fractions(member)).collect();

    assert!(document.set_drawing_size_at(at, 914_400, 914_400), "it was not resized");

    let after = document.group_at(at).expect("a group");
    assert_eq!((after.width_emu, after.height_emu), (914_400, 914_400));
    assert_eq!(
        after.child_width, before.child_width,
        "the rectangle the members are measured in should not have moved"
    );
    let now: Vec<(f32, f32, f32, f32)> =
        after.members.iter().map(|member| after.fractions(member)).collect();
    assert_eq!(now, places, "the members are no longer where they were in the group");
}

#[test]
fn resizing_a_group_leaves_the_members_own_sizes_alone() {
    let mut document = two_shapes();
    let at = document.group_drawings(&both()).expect("nothing was grouped");
    let before: Vec<i64> =
        document.group_at(at).expect("a group").members.iter().map(|one| one.width_emu).collect();

    document.set_drawing_size_at(at, 457_200, 457_200);

    let after: Vec<i64> =
        document.group_at(at).expect("a group").members.iter().map(|one| one.width_emu).collect();
    assert_eq!(after, before, "a member was given the size of the whole group");
}
