//! Every drawing known by a number of its own.
//!
//! A drawing's `wp:docPr/@id` is unique in the whole document — the standard
//! says so, and Word keeps one count for the body, the headers, the footers,
//! the notes and the comments together. Every drawing here was written with
//! `id="1"`, so a second one could not be told from the first. These put in
//! one of each kind, open a document with numbers of its own, reach into a
//! header, paste, group and ungroup, and look at the saved file.

use wp_docx::anchor::{Anchor, Placement, Wrap};
use wp_docx::chart::{Chart, Kind};
use wp_docx::diagram::Arrangement;
use wp_docx::furniture::{Furniture, Preset};
use wp_docx::group::Rect;
use wp_docx::ink::{Ink, Stroke};
use wp_docx::model::{Alignment, Block, Body, Paragraph, Run, RunContent};
use wp_docx::shapes::Shape;
use wp_docx::{Document, TextPosition, EMU_PER_INCH};
use wp_xml::tree::{Element, XmlTree};

const WP: &str = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
const W14: &str = "http://schemas.microsoft.com/office/word/2010/wordml";

const PICTURE: &[u8] = b"\x89PNG\r\n\x1a\n a picture";
const OTHER_PICTURE: &[u8] = b"\x89PNG\r\n\x1a\n another picture";

fn document(text: &str) -> Document {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text(text)));
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

fn round_trip(document: &Document) -> Document {
    Document::open(&document.save().expect("saving")).expect("reopening")
}

/// A tick, in two strokes.
fn ink() -> Ink {
    let pen = |points: Vec<(i64, i64)>| Stroke {
        colour: "0070C0".to_owned(),
        width_emu: 18_000,
        transparency: 0,
        flat: false,
        points,
        pressure: Vec::new(),
    };
    Ink {
        strokes: vec![
            pen(vec![(0, 180_000), (90_000, 270_000)]),
            pen(vec![(90_000, 270_000), (270_000, 0)]),
        ],
    }
}

fn floating(across: i64) -> Anchor {
    Anchor {
        wrap: Wrap::None,
        horizontal: Placement::Offset(across),
        vertical: Placement::Offset(0),
        ..Anchor::default()
    }
}

/// The number each drawing is known by, in every part of the saved file but
/// the glossary, which counts for itself: each wrapper's `wp:docPr`, and the
/// `w14:cNvPr` of ink in the line, which has no wrapper.
fn numbers(document: &Document) -> Vec<(String, u32)> {
    fn walk(part: &str, element: &Element, in_wrapper: bool, out: &mut Vec<(String, u32)>) {
        let namespace = element.namespace.as_deref();
        let local = element.local_name();
        let id = || element.attribute_by_name("id").and_then(|id| id.parse().ok());
        if namespace == Some(WP) && local == "docPr" {
            out.push((part.to_owned(), id().expect("a number")));
            return;
        }
        if namespace == Some(W14) && local == "cNvPr" && !in_wrapper {
            out.push((part.to_owned(), id().expect("a number")));
            return;
        }
        let wrapper = in_wrapper || (namespace == Some(WP) && matches!(local, "inline" | "anchor"));
        for child in element.child_elements() {
            walk(part, child, wrapper, out);
        }
    }

    let bytes = document.save().expect("saving");
    let package = wp_opc::Package::open(&bytes).expect("a package");
    let mut out = Vec::new();
    for entry in package.content_parts() {
        if !entry.name.ends_with(".xml") || entry.name.contains("glossary") {
            continue;
        }
        let Some(Ok(text)) = package.xml_part(&entry.name) else { continue };
        let Ok(tree) = XmlTree::parse(&text) else { continue };
        walk(&entry.name, &tree.root, false, &mut out);
    }
    out
}

/// Says the numbers are all different, and gives them back in order.
fn all_different(document: &Document) -> Vec<u32> {
    let found = numbers(document);
    let mut ids: Vec<u32> = found.iter().map(|(_, id)| *id).collect();
    ids.sort_unstable();
    let before = ids.len();
    ids.dedup();
    assert_eq!(ids.len(), before, "a number used twice: {found:?}");
    ids
}

#[test]
fn every_drawing_put_in_is_known_by_a_number_of_its_own() {
    let mut document = document("Before after");
    document.set_caret(TextPosition::new(0, 7));
    let square = Shape::preset("rect", 72.0, 72.0);
    assert!(document.insert_shape(&square));
    assert!(document.insert_shape(&square.clone().floating(floating(914_400))));
    assert!(document.insert_picture(PICTURE, "png", 914_400, 914_400).expect("a picture"));
    assert!(document.insert_picture(OTHER_PICTURE, "png", 914_400, 914_400).expect("another"));
    let chart = Chart::parse(Kind::Column, "Sales", "North=10; South=20");
    assert!(document.insert_chart(&chart, EMU_PER_INCH * 4, EMU_PER_INCH * 3).expect("a chart"));
    assert!(document.insert_ink(&ink()).expect("ink in the line"));
    assert!(document.insert_ink_floating(&ink(), &floating(0)).expect("ink on the page"));
    let items: Vec<String> = ["Plan", "Draw"].iter().map(|item| (*item).to_owned()).collect();
    assert!(document.insert_diagram(Arrangement::Process, &items, EMU_PER_INCH * 6).expect("one"));

    // Each shape takes two: its drawing's, and its own after it.
    assert_eq!(all_different(&document), [1, 3, 5, 6, 7, 8, 9, 10]);
    // And the same once the file has been read back, which is where a number
    // would have to be told apart from another.
    assert_eq!(all_different(&round_trip(&document)).len(), 8);
}

/// Sets every drawing's number in one part of a saved document to `to`.
fn numbered_in(bytes: &[u8], part: &str, to: u32) -> Vec<u8> {
    let mut package = wp_opc::Package::open(bytes).expect("a package");
    let text = package.xml_part(part).expect("the part").expect("readable");
    let mut out = String::new();
    let mut rest = text.as_str();
    while let Some(at) = rest.find("<wp:docPr id=\"") {
        let (before, after) = rest.split_at(at + "<wp:docPr id=\"".len());
        out.push_str(before);
        out.push_str(&to.to_string());
        rest = &after[after.find('"').expect("the end of the number")..];
    }
    out.push_str(rest);
    package.set_part(part, out.into_bytes());
    package.save().expect("saving the package")
}

/// A document with a picture in its body and one in its header.
fn pictures_in_body_and_header() -> Document {
    let mut document = document("Body text");
    document.set_caret(TextPosition::new(0, 4));
    assert!(document.insert_picture(PICTURE, "png", 914_400, 914_400).expect("a picture"));
    assert!(document
        .set_furniture(Furniture::Header, Preset::Text, Alignment::Start, "Header")
        .expect("a header"));
    let header = document.furniture_part(Furniture::Header).expect("the header's part");
    assert!(document.enter_part(&header));
    document.set_caret(TextPosition::new(0, 0));
    assert!(document.insert_picture(OTHER_PICTURE, "png", 914_400, 914_400).expect("another"));
    assert!(document.leave_part());
    document
}

#[test]
fn a_drawing_in_a_header_counts() {
    // Put in while the body's is in the package and not being edited, and
    // the other way round.
    let mut document = pictures_in_body_and_header();
    assert_eq!(all_different(&document), [1, 2]);
    let found = numbers(&document);
    assert!(found.iter().any(|(part, id)| part == "word/document.xml" && *id == 1), "{found:?}");
    assert!(found.iter().any(|(part, id)| part.contains("header") && *id == 2), "{found:?}");

    document.set_caret(TextPosition::new(0, 0));
    assert!(document.insert_picture(PICTURE, "png", 914_400, 914_400).expect("a third"));
    assert_eq!(all_different(&document), [1, 2, 3]);
}

#[test]
fn a_new_drawing_is_numbered_above_every_number_the_document_came_with() {
    let bytes = pictures_in_body_and_header().save().expect("saving");
    let header = Document::open(&bytes)
        .expect("reopening")
        .furniture_part(Furniture::Header)
        .expect("the header's part");
    let bytes = numbered_in(&bytes, "word/document.xml", 41);
    let bytes = numbered_in(&bytes, &header, 57);
    let mut document = Document::open(&bytes).expect("reopening");
    assert_eq!(all_different(&document), [41, 57]);

    document.set_caret(TextPosition::new(0, 0));
    assert!(document.insert_picture(PICTURE, "png", 914_400, 914_400).expect("a picture"));
    assert_eq!(all_different(&document), [41, 57, 58]);
}

#[test]
fn a_pasted_copy_is_known_by_a_number_of_its_own() {
    let mut document = document("Before after");
    document.set_caret(TextPosition::new(0, 7));
    assert!(document.insert_picture(PICTURE, "png", 914_400, 914_400).expect("a picture"));
    document.set_caret(TextPosition::new(0, 7));
    document.extend_selection_to(TextPosition::new(0, 8));
    let copied = document.copy_selection();
    document.clear_selection();
    document.set_caret(TextPosition::new(0, 0));
    assert!(document.paste_blocks(&copied));
    assert!(document.paste_blocks(&copied));

    assert_eq!(all_different(&document), [1, 2, 3]);
}

#[test]
fn a_group_and_the_drawings_that_come_out_of_it_are_numbered_apart() {
    let mut document = document("Words");
    for across in [0, 914_400] {
        document.set_caret(TextPosition::new(0, 0));
        let shape = Shape::preset("ellipse", 72.0, 72.0).floating(floating(across));
        assert!(document.insert_shape(&shape));
    }
    let members = [
        (TextPosition::new(0, 0), Rect { x: 914_400, y: 0, width: 914_400, height: 914_400 }),
        (TextPosition::new(0, 1), Rect { x: 0, y: 0, width: 914_400, height: 914_400 }),
    ];
    let at = document.group_drawings(&members).expect("a group");
    let grouped = all_different(&document);
    assert_eq!(grouped.len(), 1, "one drawing, the group");
    // Its shapes keep the numbers a connector in it would name them by, and
    // the group's own comes from the same count, above them.
    let members = member_numbers(&document);
    assert_eq!(members.len(), 2);
    assert!(members.iter().all(|id| *id < grouped[0]), "{members:?} and the group {grouped:?}");

    let out = document.ungroup_at(at);
    assert_eq!(out.len(), 2);
    assert_eq!(all_different(&document).len(), 2);
}

/// The numbers of the shapes inside every group of the saved body.
fn member_numbers(document: &Document) -> Vec<u32> {
    fn walk(element: &Element, in_group: bool, out: &mut Vec<u32>) {
        if in_group && element.local_name() == "cNvPr" {
            out.extend(element.attribute_by_name("id").and_then(|id| id.parse::<u32>().ok()));
        }
        let in_group = in_group || element.local_name() == "wgp";
        for child in element.child_elements() {
            walk(child, in_group, out);
        }
    }
    let saved = round_trip(document);
    let mut out = Vec::new();
    walk(&saved.tree().root, false, &mut out);
    out
}

#[test]
fn a_document_made_from_a_model_numbers_its_drawings_from_one() {
    let shape = |name: &str| {
        let mut run = Run::text("");
        run.content = vec![RunContent::Shape(Box::new(Shape {
            name: name.to_owned(),
            ..Shape::preset("rect", 36.0, 36.0)
        }))];
        run
    };
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::from_runs(vec![shape("One"), shape("Two")])));
    body.blocks.push(Block::Paragraph(Paragraph::from_runs(vec![shape("Three")])));
    let document = Document::create(&body).expect("a document");

    // Each drawing's number, and after it the shape's own.
    assert_eq!(all_different(&document), [1, 3, 5]);
    let own: Vec<u32> = document.shapes().iter().map(|shape| shape.id).collect();
    assert_eq!(own, [2, 4, 6]);
}

#[test]
fn a_shape_changed_where_it_stands_keeps_its_number() {
    let mut document = document("Words");
    document.set_caret(TextPosition::new(0, 0));
    assert!(document.insert_shape(&Shape::preset("rect", 72.0, 72.0)));
    assert!(document.insert_shape(&Shape::preset("rect", 72.0, 72.0)));
    let before = numbers(&document);
    let own = |document: &Document| -> Vec<u32> {
        document.shapes().iter().map(|shape| shape.id).collect()
    };
    let own_before = own(&document);

    let mut changed = Shape::preset("ellipse", 144.0, 72.0);
    changed.name = "Changed".to_owned();
    assert!(document.replace_shape_at(TextPosition::new(0, 1), &changed));
    assert_eq!(numbers(&document), before, "the same drawing, known by the same number");
    assert_eq!(own(&document), own_before, "and the same shape, by its own");
}

#[test]
fn two_new_shapes_grouped_are_members_a_connector_can_tell_apart() {
    // A document that came with numbers of its own, up to 57.
    let bytes = pictures_in_body_and_header().save().expect("saving");
    let header = Document::open(&bytes)
        .expect("reopening")
        .furniture_part(Furniture::Header)
        .expect("the header's part");
    let bytes = numbered_in(&bytes, "word/document.xml", 41);
    let bytes = numbered_in(&bytes, &header, 57);
    let mut document = Document::open(&bytes).expect("reopening");

    for across in [0, 914_400] {
        document.set_caret(TextPosition::new(0, 0));
        let shape = Shape::preset("rect", 72.0, 72.0).floating(floating(across));
        assert!(document.insert_shape(&shape));
    }
    let members = [
        (TextPosition::new(0, 0), Rect { x: 914_400, y: 0, width: 914_400, height: 914_400 }),
        (TextPosition::new(0, 1), Rect { x: 0, y: 0, width: 914_400, height: 914_400 }),
    ];
    document.group_drawings(&members).expect("a group");

    let members = member_numbers(&document);
    assert_eq!(members.len(), 2);
    assert_ne!(members[0], members[1], "two members a connector cannot tell apart");
    assert!(members.iter().all(|id| *id > 57), "{members:?}, and the document had up to 57");
    let drawings = all_different(&document);
    assert!(members.iter().all(|id| !drawings.contains(id)), "{members:?} and {drawings:?}");
}

/// Every drawing's number and name in the saved file, as `numbers` finds them.
fn names(document: &Document) -> Vec<(u32, String)> {
    fn walk(element: &Element, in_wrapper: bool, out: &mut Vec<(u32, String)>) {
        let namespace = element.namespace.as_deref();
        let local = element.local_name();
        if (namespace == Some(WP) && local == "docPr")
            || (namespace == Some(W14) && local == "cNvPr" && !in_wrapper)
        {
            let id = element.attribute_by_name("id").and_then(|id| id.parse().ok());
            let name = element.attribute_by_name("name").unwrap_or_default().to_owned();
            out.push((id.expect("a number"), name));
            return;
        }
        let wrapper = in_wrapper || (namespace == Some(WP) && matches!(local, "inline" | "anchor"));
        for child in element.child_elements() {
            walk(child, wrapper, out);
        }
    }
    let saved = round_trip(document);
    let mut out = Vec::new();
    walk(&saved.tree().root, false, &mut out);
    out.sort();
    out
}

#[test]
fn a_new_drawing_is_named_after_its_number() {
    let mut document = document("Before after");
    for across in [0, 914_400] {
        document.set_caret(TextPosition::new(0, 0));
        let shape = Shape::preset("rect", 72.0, 72.0).floating(floating(across));
        assert!(document.insert_shape(&shape));
    }
    let members = [
        (TextPosition::new(0, 0), Rect { x: 914_400, y: 0, width: 914_400, height: 914_400 }),
        (TextPosition::new(0, 1), Rect { x: 0, y: 0, width: 914_400, height: 914_400 }),
    ];
    document.group_drawings(&members).expect("a group");
    document.set_caret(TextPosition::new(0, 8));
    assert!(document.insert_picture(PICTURE, "png", 914_400, 914_400).expect("a picture"));
    let chart = Chart::parse(Kind::Column, "Sales", "North=10; South=20");
    assert!(document.insert_chart(&chart, EMU_PER_INCH * 4, EMU_PER_INCH * 3).expect("a chart"));
    let items: Vec<String> = ["Plan", "Draw"].iter().map(|item| (*item).to_owned()).collect();
    assert!(document.insert_diagram(Arrangement::Process, &items, EMU_PER_INCH * 6).expect("one"));
    assert!(document.insert_shape(&Shape::preset("ellipse", 72.0, 72.0)));
    assert!(document.insert_ink(&ink()).expect("ink in the line"));
    assert!(document.insert_ink_floating(&ink(), &floating(0)).expect("ink on the page"));
    // And one somebody named, which keeps the name.
    let mut named = Shape::preset("rect", 36.0, 36.0);
    named.name = "Logo".to_owned();
    assert!(document.insert_shape(&named));

    let found = names(&document);
    let mut words: Vec<&str> = Vec::new();
    for (id, name) in &found {
        if name == "Logo" {
            continue;
        }
        let word = name
            .strip_suffix(&format!(" {id}"))
            .unwrap_or_else(|| panic!("{name:?} is not named after its number {id}: {found:?}"));
        words.push(word);
    }
    words.sort_unstable();
    assert_eq!(words, ["Chart", "Diagram", "Group", "Ink", "Ink", "Picture", "Shape"]);
    assert!(found.iter().any(|(_, name)| name == "Logo"), "{found:?}");
}

#[test]
fn a_pasted_copy_keeps_its_name_unless_the_document_has_it() {
    let mut source = document("Three: ");
    source.set_caret(TextPosition::new(0, 7));
    for _ in 0..3 {
        assert!(source.insert_picture(PICTURE, "png", 914_400, 914_400).expect("a picture"));
    }
    source.set_caret(TextPosition::new(0, 9));
    source.extend_selection_to(TextPosition::new(0, 10));
    let copied = source.copy_selection();
    source.clear_selection();

    // Into a document with no drawing of that name: it keeps the name it came
    // with, under a number of its own.
    let mut other = document("One: ");
    other.set_caret(TextPosition::new(0, 5));
    assert!(other.insert_picture(OTHER_PICTURE, "png", 914_400, 914_400).expect("a picture"));
    other.set_caret(TextPosition::new(0, 0));
    assert!(other.paste_blocks(&copied));
    let named = |id: u32, name: &str| (id, name.to_owned());
    assert_eq!(names(&other), [named(1, "Picture 1"), named(2, "Picture 3")]);

    // Beside the one it was copied from: that name is taken, and it takes the
    // one its number gives it.
    source.set_caret(TextPosition::new(0, 0));
    assert!(source.paste_blocks(&copied));
    assert_eq!(
        names(&source),
        [
            named(1, "Picture 1"),
            named(2, "Picture 2"),
            named(3, "Picture 3"),
            named(4, "Picture 4")
        ]
    );
}

#[test]
fn a_connected_pair_pasted_beside_itself_is_a_pair_of_its_own() {
    use wp_docx::joins::{Join, Joins};

    // Two boxes, and a connector from the first to the second put in between
    // them: the box, the connector, the box.
    let mut document = document("Words");
    document.set_caret(TextPosition::new(0, 0));
    assert!(document.insert_shape(&Shape::preset("rect", 72.0, 72.0).floating(floating(0))));
    assert!(document
        .insert_shape(&Shape::preset("rect", 72.0, 72.0).floating(floating(EMU_PER_INCH * 2))));
    let own = |document: &Document| -> Vec<u32> {
        document.shapes().iter().map(|shape| shape.id).collect()
    };
    let boxes = own(&document);
    let mut connector =
        Shape::preset("straightConnector1", 72.0, 1.0).floating(floating(EMU_PER_INCH));
    connector.joins = Joins {
        start: Some(Join { shape: boxes[0], site: 3 }),
        end: Some(Join { shape: boxes[1], site: 1 }),
    };
    document.set_caret(TextPosition::new(0, 1));
    assert!(document.insert_shape(&connector));
    let joined = |shape: &Shape| {
        (shape.joins.start.map(|end| end.shape), shape.joins.end.map(|end| end.shape))
    };

    // All three copied and pasted beside the originals.
    document.set_caret(TextPosition::new(0, 0));
    document.extend_selection_to(TextPosition::new(0, 3));
    let copied = document.copy_selection();
    document.clear_selection();
    document.set_caret(TextPosition::new(0, 8));
    assert!(document.paste_blocks(&copied));
    let document = round_trip(&document);
    let shapes = document.shapes();
    assert_eq!(shapes.len(), 6);
    let ids = own(&document);
    let mut distinct = ids.clone();
    distinct.sort_unstable();
    distinct.dedup();
    assert_eq!(distinct.len(), 6, "{ids:?}");
    // The originals: box, connector, box; and the copies after the words.
    let (first, second, copied_first, copied_second) = (ids[0], ids[2], ids[3], ids[5]);
    assert_eq!(
        joined(&shapes[1]),
        (Some(first), Some(second)),
        "the original still joins its pair"
    );
    assert_eq!(
        joined(&shapes[4]),
        (Some(copied_first), Some(copied_second)),
        "the pasted connector joins the pasted pair"
    );

    // A box and the connector without the other box: the pasted connector
    // joins the pasted box, and the box outside the copy as it did.
    let mut document = document;
    document.set_caret(TextPosition::new(0, 0));
    document.extend_selection_to(TextPosition::new(0, 2));
    let copied = document.copy_selection();
    document.clear_selection();
    document.set_caret(TextPosition::new(0, 0));
    assert!(document.paste_blocks(&copied));
    let shapes = document.shapes();
    let (pasted_box, pasted_connector) = (&shapes[0], &shapes[1]);
    assert!(!ids.contains(&pasted_box.id), "a number of its own: {}", pasted_box.id);
    assert_eq!(joined(pasted_connector), (Some(pasted_box.id), Some(second)));
}
