//! Shapes and text boxes.
//!
//! # What a shape is in a document
//!
//! A drawing, like a picture — the same `w:drawing` wrapper, the same box
//! saying how big it is — but where a picture's graphic data holds an image,
//! a shape's holds a *`wps:wsp`*: a named geometry, a fill, a line, and, if it
//! is a text box, a whole body of paragraphs inside it.
//!
//! That is why a text box is not a special thing here. It is a rectangle whose
//! text happens to be worth reading and whose fill and line happen to be none.
//!
//! # Why the preset is kept as it was written
//!
//! There are about 180 preset geometries and this program draws a dozen. A
//! shape whose preset is not one of the dozen is drawn as a rectangle — but the
//! name is kept exactly as it was found, so saving the document back does not
//! turn somebody's cloud callout into a rectangle for good.

use wp_xml::tree::Element;

use crate::model::{Body, Paragraph};
use crate::{edit, read, Document};

/// English Metric Units to the point, which is what a drawing is measured in.
pub const EMU_PER_POINT: i64 = 12_700;

/// The namespace a word-processing shape is written in.
pub const WPS: &str = "http://schemas.microsoft.com/office/word/2010/wordprocessingShape";
/// The drawing namespace the geometry and the colours are in.
pub const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
/// And the one the box round a drawing is in.
pub const WP: &str = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";

/// A shape in a document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shape {
    /// The preset geometry's name, exactly as the document wrote it.
    pub preset: String,
    pub width_emu: i64,
    pub height_emu: i64,
    /// The colour inside, as six hex digits. `None` means nothing is drawn
    /// there and whatever is behind shows through.
    pub fill: Option<String>,
    /// The colour of the line round it, and how thick that line is.
    pub outline: Option<String>,
    pub outline_emu: i64,
    /// The paragraphs inside, which is what makes a shape a text box.
    pub text: Vec<Paragraph>,
    /// What the shape is called, which is what the selection pane would list.
    pub name: String,
    /// Where it floats, or `None` when it sits in the line of text.
    pub anchor: Option<crate::anchor::Anchor>,
    /// What it shows, said in words.
    ///
    /// Read out in place of the drawing by anything that cannot show it, which
    /// is the only thing a reader who cannot see it has.
    pub description: String,
}

impl Default for Shape {
    fn default() -> Self {
        Self {
            preset: "rect".to_owned(),
            width_emu: 0,
            height_emu: 0,
            fill: None,
            outline: None,
            outline_emu: 0,
            text: Vec::new(),
            name: "Shape".to_owned(),
            anchor: None,
            description: String::new(),
        }
    }
}

impl Shape {
    /// How wide it is in points.
    #[must_use]
    pub fn width_points(&self) -> f64 {
        self.width_emu as f64 / EMU_PER_POINT as f64
    }

    /// And how tall.
    #[must_use]
    pub fn height_points(&self) -> f64 {
        self.height_emu as f64 / EMU_PER_POINT as f64
    }

    /// How thick its line is, in points.
    #[must_use]
    pub fn outline_points(&self) -> f32 {
        self.outline_emu as f32 / EMU_PER_POINT as f32
    }

    /// Whether anything is written inside it.
    #[must_use]
    pub fn has_text(&self) -> bool {
        self.text.iter().any(|paragraph| !paragraph.plain_text().is_empty())
    }

    /// The text inside, as a body the layout can measure.
    #[must_use]
    pub fn body(&self) -> Body {
        Body { blocks: self.text.iter().cloned().map(crate::model::Block::Paragraph).collect() }
    }

    /// A shape of a preset and a size in points, with Word's own colours.
    #[must_use]
    pub fn preset(preset: &str, width_points: f64, height_points: f64) -> Self {
        Self {
            preset: preset.to_owned(),
            width_emu: (width_points * EMU_PER_POINT as f64) as i64,
            height_emu: (height_points * EMU_PER_POINT as f64) as i64,
            // The blue Word fills a new shape with, and the darker blue it
            // draws round one.
            fill: Some("4472C4".to_owned()),
            outline: Some("2F528F".to_owned()),
            outline_emu: EMU_PER_POINT,
            text: Vec::new(),
            name: "Shape".to_owned(),
            anchor: None,
            description: String::new(),
        }
    }

    /// The same shape, floating on the page rather than sitting in the line.
    #[must_use]
    pub fn floating(mut self, anchor: crate::anchor::Anchor) -> Self {
        self.anchor = Some(anchor);
        self
    }

    /// A text box: a rectangle with a line round it and nothing inside it.
    #[must_use]
    pub fn text_box(width_points: f64, height_points: f64, text: &str) -> Self {
        Self {
            preset: "rect".to_owned(),
            width_emu: (width_points * EMU_PER_POINT as f64) as i64,
            height_emu: (height_points * EMU_PER_POINT as f64) as i64,
            fill: None,
            outline: Some("000000".to_owned()),
            outline_emu: EMU_PER_POINT,
            text: text.split('\n').map(Paragraph::text).collect(),
            name: "Text Box".to_owned(),
            anchor: None,
            description: String::new(),
        }
    }
}

/// Reads a shape out of a `w:drawing`, if that is what it holds.
#[must_use]
pub fn read_shape(drawing: &Element) -> Option<Shape> {
    let wsp = find(drawing, "wsp")?;
    let mut shape = Shape { anchor: crate::anchor::read_anchor(drawing), ..Shape::default() };

    if let Some(properties) = find(wsp, "cNvPr") {
        if let Some(name) = properties.attribute_by_name("name") {
            shape.name = name.to_owned();
        }
    }
    // The description is on the drawing's own properties rather than the
    // shape's, because it describes the drawing as a whole.
    if let Some(properties) = find(drawing, "docPr") {
        if let Some(description) = properties.attribute_by_name("descr") {
            shape.description = description.to_owned();
        }
    }

    // The size is on the drawing's box, and again on the shape's own transform.
    // The box is what the text flows round, so it is the one to believe.
    if let Some(extent) = find(drawing, "extent") {
        shape.width_emu = number(extent, "cx");
        shape.height_emu = number(extent, "cy");
    }
    if shape.width_emu == 0 || shape.height_emu == 0 {
        if let Some(extent) = find(wsp, "ext") {
            shape.width_emu = number(extent, "cx");
            shape.height_emu = number(extent, "cy");
        }
    }

    let properties = find(wsp, "spPr");
    if let Some(geometry) = properties.and_then(|properties| child(properties, "prstGeom")) {
        if let Some(name) = geometry.attribute_by_name("prst") {
            shape.preset = name.to_owned();
        }
    }
    shape.fill = properties.and_then(solid_color);

    if let Some(line) = properties.and_then(|properties| child(properties, "ln")) {
        shape.outline_emu = line.attribute_by_name("w").and_then(|w| w.parse().ok()).unwrap_or(0);
        shape.outline = solid_color(line);
        // A line saying nothing about its colour is still a line: Word draws it
        // in the theme's, and black is nearer that than nothing at all.
        if shape.outline.is_none() && child(line, "noFill").is_none() {
            shape.outline = Some("000000".to_owned());
        }
    }

    if let Some(content) = find(wsp, "txbxContent") {
        let body = read::read_part(content);
        shape.text = body
            .blocks
            .into_iter()
            .filter_map(|block| match block {
                crate::model::Block::Paragraph(paragraph) => Some(paragraph),
                crate::model::Block::Table(_) => None,
            })
            .collect();
    }

    Some(shape)
}

/// Writes a shape as a `w:drawing` sitting in the line of text.
#[must_use]
pub fn shape_element(shape: &Shape, prefix: Option<&str>) -> Element {
    let mut drawing = Element::new(&edit::name_with(prefix, "drawing"), Some(read::W));

    // A drawing is one element or the other: in the line, or anchored to a
    // place with the text flowing round it. Everything below the wrapper is
    // the same either way.
    let floating = shape.anchor.is_some();
    let mut inline = Element::new(if floating { "wp:anchor" } else { "wp:inline" }, Some(WP));
    inline.declarations.push((Some("wp".to_owned()), WP.to_owned()));
    match &shape.anchor {
        Some(anchor) => crate::anchor::write_anchor(anchor, &mut inline, WP),
        None => {
            for side in ["distT", "distB", "distL", "distR"] {
                inline.set_attribute(side, "0");
            }
        }
    }

    let mut extent = Element::new("wp:extent", Some(WP));
    extent.set_attribute("cx", &shape.width_emu.to_string());
    extent.set_attribute("cy", &shape.height_emu.to_string());
    inline.push_element(extent);

    // The wrap comes after the extent and before the name, which is where the
    // schema puts it.
    if let Some(anchor) = &shape.anchor {
        inline.push_element(crate::anchor::wrap_element(anchor, WP));
    }

    let mut visible = Element::new("wp:docPr", Some(WP));
    visible.set_attribute("id", "1");
    visible.set_attribute("name", &shape.name);
    if !shape.description.is_empty() {
        visible.set_attribute("descr", &shape.description);
    }
    inline.push_element(visible);

    let mut graphic = Element::new("a:graphic", Some(A));
    graphic.declarations.push((Some("a".to_owned()), A.to_owned()));
    let mut data = Element::new("a:graphicData", Some(A));
    data.set_attribute("uri", WPS);
    data.push_element(word_shape(shape, prefix));
    graphic.push_element(data);
    inline.push_element(graphic);

    drawing.push_element(inline);
    drawing
}

/// The `wps:wsp` itself.
fn word_shape(shape: &Shape, prefix: Option<&str>) -> Element {
    let mut wsp = Element::new("wps:wsp", Some(WPS));
    wsp.declarations.push((Some("wps".to_owned()), WPS.to_owned()));

    let mut visible = Element::new("wps:cNvPr", Some(WPS));
    visible.set_attribute("id", "1");
    visible.set_attribute("name", &shape.name);
    wsp.push_element(visible);
    wsp.push_element(Element::new("wps:cNvSpPr", Some(WPS)));

    let mut properties = Element::new("wps:spPr", Some(WPS));

    let mut transform = Element::new("a:xfrm", Some(A));
    let mut offset = Element::new("a:off", Some(A));
    offset.set_attribute("x", "0");
    offset.set_attribute("y", "0");
    transform.push_element(offset);
    let mut extent = Element::new("a:ext", Some(A));
    extent.set_attribute("cx", &shape.width_emu.to_string());
    extent.set_attribute("cy", &shape.height_emu.to_string());
    transform.push_element(extent);
    properties.push_element(transform);

    let mut geometry = Element::new("a:prstGeom", Some(A));
    geometry.set_attribute("prst", &shape.preset);
    geometry.push_element(Element::new("a:avLst", Some(A)));
    properties.push_element(geometry);

    match &shape.fill {
        Some(colour) => properties.push_element(solid(colour)),
        // Said rather than left out: a shape with no fill element at all takes
        // the theme's, which is not the same as having none.
        None => properties.push_element(Element::new("a:noFill", Some(A))),
    }

    let mut line = Element::new("a:ln", Some(A));
    if shape.outline_emu > 0 {
        line.set_attribute("w", &shape.outline_emu.to_string());
    }
    match &shape.outline {
        Some(colour) => line.push_element(solid(colour)),
        None => line.push_element(Element::new("a:noFill", Some(A))),
    }
    properties.push_element(line);
    wsp.push_element(properties);

    // The shape takes the theme's second effect style, which is what the
    // Design tab's Effects gallery changes. Without this a shape is drawn flat
    // however the theme is set — see [`crate::theme::Effect`].
    let mut style = Element::new("wps:style", Some(WPS));
    let mut effect = Element::new("a:effectRef", Some(A));
    effect.set_attribute("idx", "2");
    let mut colour = Element::new("a:schemeClr", Some(A));
    colour.set_attribute("val", "accent1");
    effect.push_element(colour);
    style.push_element(effect);
    wsp.push_element(style);

    if !shape.text.is_empty() {
        let mut box_element = Element::new("wps:txbx", Some(WPS));
        let mut content = Element::new(&edit::name_with(prefix, "txbxContent"), Some(read::W));
        for paragraph in &shape.text {
            content.push_element(edit::paragraph_element(paragraph, prefix));
        }
        box_element.push_element(content);
        wsp.push_element(box_element);
    }

    // How the text sits in the shape. Word writes this even for a shape with no
    // text, and a shape without it is one Word repairs.
    let mut body = Element::new("wps:bodyPr", Some(WPS));
    body.set_attribute("rot", "0");
    body.set_attribute("anchor", "ctr");
    wsp.push_element(body);

    wsp
}

/// A solid fill of one colour.
fn solid(colour: &str) -> Element {
    let mut fill = Element::new("a:solidFill", Some(A));
    let mut value = Element::new("a:srgbClr", Some(A));
    value.set_attribute("val", colour);
    fill.push_element(value);
    fill
}

/// The colour of an element's solid fill, if it has one.
fn solid_color(parent: &Element) -> Option<String> {
    let fill = child(parent, "solidFill")?;
    let colour = child(fill, "srgbClr")?;
    colour.attribute_by_name("val").map(|value| value.to_uppercase())
}

/// An attribute read as a number, or nothing.
fn number(element: &Element, name: &str) -> i64 {
    element.attribute_by_name(name).and_then(|value| value.parse().ok()).unwrap_or(0)
}

/// A child by local name, whatever prefix it was written with.
fn child<'a>(parent: &'a Element, local: &str) -> Option<&'a Element> {
    parent.child_elements().find(|child| child.local_name() == local)
}

/// The named element anywhere under a root.
///
/// A drawing is six or seven elements deep and the depth differs between a
/// shape Word wrote and one this program did, so nothing here counts levels.
fn find<'a>(root: &'a Element, local: &str) -> Option<&'a Element> {
    if root.local_name() == local {
        return Some(root);
    }
    root.child_elements().find_map(|child| find(child, local))
}

impl Document {
    /// Puts a shape at the caret, in the line of text.
    ///
    /// One character wide in the caret's reckoning, exactly as a picture is, so
    /// Backspace can reach it and the caret can stand either side.
    pub fn insert_shape(&mut self, shape: &crate::shapes::Shape) -> bool {
        let caret = self.caret();
        self.record(crate::history::EditKind::Structural, caret, false);

        let prefix = self.prefix();
        let element = crate::shapes::shape_element(shape, prefix.as_deref());
        if !crate::position::insert_element_at(
            &mut self.tree_mut().root,
            caret,
            element,
            prefix.as_deref(),
        ) {
            return false;
        }

        self.set_caret(crate::TextPosition::new(caret.paragraph, caret.offset + 1));
        self.mark_modified();
        true
    }

    /// Every shape in the document, in reading order.
    #[must_use]
    pub fn shapes(&self) -> Vec<crate::shapes::Shape> {
        let mut out = Vec::new();
        for block in &self.body().blocks {
            gather_shapes(block, &mut out);
        }
        out
    }
}

/// Finds every shape in a block, whatever it is nested in.
fn gather_shapes(block: &crate::model::Block, out: &mut Vec<crate::shapes::Shape>) {
    match block {
        crate::model::Block::Paragraph(paragraph) => {
            for run in &paragraph.runs {
                for piece in &run.content {
                    if let crate::model::RunContent::Shape(shape) = piece {
                        out.push(shape.clone());
                    }
                }
            }
        }
        crate::model::Block::Table(table) => {
            for row in &table.rows {
                for cell in &row.cells {
                    for block in &cell.blocks {
                        gather_shapes(block, out);
                    }
                }
            }
        }
    }
}

impl Document {
    /// The shape the caret is beside, if it is beside one.
    ///
    /// Beside means the character before the caret or the character after it,
    /// because a drawing takes one character and a person putting the caret
    /// "on" it means either side.
    #[must_use]
    pub fn shape_here(&self) -> Option<crate::shapes::Shape> {
        let caret = self.caret();
        let paragraph = self.paragraph_element(caret.paragraph)?;
        let mut found = None;
        let mut offset = 0usize;
        walk_shapes(paragraph, &mut offset, caret.offset, &mut found);
        found
    }

    /// Replaces the shape the caret is beside.
    pub fn replace_shape_here(&mut self, shape: &crate::shapes::Shape) -> bool {
        let caret = self.caret();
        if self.shape_here().is_none() {
            return false;
        }
        self.record(crate::history::EditKind::Structural, caret, false);

        let prefix = self.prefix();
        let Some(path) = crate::position::paragraph_path(&self.tree().root, caret.paragraph) else {
            return false;
        };
        let Some(paragraph) = edit::element_at_path_mut(&mut self.tree_mut().root, &path) else {
            return false;
        };

        let replacement = shape_element(shape, prefix.as_deref());
        let mut offset = 0usize;
        let replaced = replace_shape(paragraph, &mut offset, caret.offset, replacement);
        if replaced {
            self.mark_modified();
        }
        replaced
    }
}

/// Finds the shape beside an offset, if there is one.
fn walk_shapes(
    element: &Element,
    offset: &mut usize,
    wanted: usize,
    found: &mut Option<crate::shapes::Shape>,
) {
    for node in &element.children {
        let Some(child) = node.as_element() else { continue };
        if child.namespace.as_deref() == Some(read::W)
            && matches!(child.local_name(), "drawing" | "pict" | "object")
        {
            // The drawing covers one character, so the caret is beside it when
            // it is at either end of that character.
            if *offset == wanted || *offset + 1 == wanted {
                if let Some(shape) = read_shape(child) {
                    *found = Some(shape);
                }
            }
            *offset += 1;
            continue;
        }
        if child.namespace.as_deref() == Some(read::W) && child.local_name() == "t" {
            *offset += child.text_content().len();
            continue;
        }
        walk_shapes(child, offset, wanted, found);
    }
}

/// Puts a new drawing in the place of the one beside an offset.
fn replace_shape(
    element: &mut Element,
    offset: &mut usize,
    wanted: usize,
    replacement: Element,
) -> bool {
    let mut at = None;
    for (index, node) in element.children.iter().enumerate() {
        let Some(child) = node.as_element() else { continue };
        if child.namespace.as_deref() == Some(read::W)
            && matches!(child.local_name(), "drawing" | "pict" | "object")
        {
            if (*offset == wanted || *offset + 1 == wanted) && read_shape(child).is_some() {
                at = Some(index);
                break;
            }
            *offset += 1;
            continue;
        }
        if child.namespace.as_deref() == Some(read::W) && child.local_name() == "t" {
            *offset += child.text_content().len();
        }
    }

    if let Some(index) = at {
        element.children[index] = wp_xml::tree::Node::Element(replacement);
        return true;
    }

    // Not at this level: the drawing is inside a run, and a run is inside the
    // paragraph.
    for node in &mut element.children {
        let Some(child) = node.as_element_mut() else { continue };
        if replace_shape(child, offset, wanted, replacement.clone()) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shape_survives_being_written_and_read_back() {
        let shape = Shape::preset("ellipse", 120.0, 60.0);
        let read = read_shape(&shape_element(&shape, Some("w"))).expect("a shape");
        assert_eq!(read.preset, "ellipse");
        assert_eq!(read.width_emu, shape.width_emu);
        assert_eq!(read.height_emu, shape.height_emu);
        assert_eq!(read.fill, shape.fill);
        assert_eq!(read.outline, shape.outline);
    }

    #[test]
    fn a_text_box_carries_its_text() {
        let shape = Shape::text_box(200.0, 80.0, "First line\nSecond line");
        let read = read_shape(&shape_element(&shape, Some("w"))).expect("a shape");
        assert!(read.has_text());
        assert_eq!(read.text.len(), 2);
        assert_eq!(read.text[0].plain_text(), "First line");
        assert_eq!(read.text[1].plain_text(), "Second line");
    }

    #[test]
    fn a_text_box_has_no_fill_and_a_line() {
        let shape = Shape::text_box(200.0, 80.0, "words");
        let read = read_shape(&shape_element(&shape, Some("w"))).expect("a shape");
        assert_eq!(read.fill, None, "a text box lets the page show through");
        assert_eq!(read.outline.as_deref(), Some("000000"));
    }

    #[test]
    fn a_preset_this_program_does_not_draw_is_kept_as_it_was_written() {
        let shape = Shape::preset("cloudCallout", 100.0, 100.0);
        let read = read_shape(&shape_element(&shape, Some("w"))).expect("a shape");
        assert_eq!(read.preset, "cloudCallout", "the name should survive a round trip");
    }

    #[test]
    fn a_size_in_points_reads_back_in_points() {
        let shape = Shape::preset("rect", 144.0, 72.0);
        assert!((shape.width_points() - 144.0).abs() < 0.01);
        assert!((shape.height_points() - 72.0).abs() < 0.01);
    }

    #[test]
    fn a_shape_with_nothing_written_in_it_has_no_text() {
        assert!(!Shape::preset("rect", 100.0, 100.0).has_text());
        assert!(!Shape::text_box(100.0, 100.0, "").has_text());
    }

    #[test]
    fn something_that_is_not_a_shape_is_not_read_as_one() {
        let mut drawing = Element::new("w:drawing", Some(read::W));
        drawing.push_element(Element::new("wp:inline", Some(WP)));
        assert!(read_shape(&drawing).is_none());
    }

    #[test]
    fn the_text_inside_is_a_body_the_layout_can_measure() {
        let shape = Shape::text_box(200.0, 80.0, "one\ntwo");
        assert_eq!(shape.body().blocks.len(), 2);
        assert_eq!(shape.body().plain_text(), "one\ntwo");
    }
}
