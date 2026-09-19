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
    /// The values behind the shape's yellow handles: `a:avLst`, in the order
    /// the document gave them and under the names it used.
    ///
    /// Each is a hundred-thousandth of whatever its own preset measures it in —
    /// mostly a fraction of the shape's shorter side, and for the shapes made
    /// of arcs an angle in sixtieths of a degree. A preset with no entry here
    /// is drawn at the proportion the format falls back on, which is what Word
    /// does with the same file. See [`crate::shapes::Shape::adjust`].
    pub adjusts: Vec<(String, i32)>,
    pub width_emu: i64,
    pub height_emu: i64,
    /// What is inside it: nothing, one colour, a gradient, a hatching, or one
    /// of the theme's fills by number. See [`crate::fills`].
    pub fill: crate::fills::Fill,
    /// The colour of the line round it — stated, or named from the theme —
    /// and how thick that line is. A thickness of nought with a line style
    /// named is the theme's: see [`Shape::outline_points_in`].
    pub outline: Option<crate::colour::Colour>,
    pub outline_emu: i64,
    /// Which of the theme's line styles the line takes its thickness from
    /// when it states none — `a:lnRef/@idx`, from one — and nought for a
    /// shape that names no style.
    pub line_style: u8,
    /// The colour the words inside are drawn in when they name none of their
    /// own: `a:fontRef`, which is what makes the words in a gallery shape
    /// white on the shape's colour. Nothing means the automatic colour.
    pub ink: Option<crate::colour::Colour>,
    /// What is drawn at the two ends of that line, which is what makes a
    /// connector an arrow. Nothing at either end for every shape that is not a
    /// line. See [`crate::lines`].
    pub head_end: crate::lines::LineEnd,
    pub tail_end: crate::lines::LineEnd,
    /// What the shape is drawn with besides its fill and its line: the shadow
    /// under it, the glow round it, the soft edge, the reflection. See
    /// [`crate::shapeeffects`].
    pub effects: crate::shapeeffects::Effects,
    /// What makes it solid rather than flat: the bevel round its edge and the
    /// depth behind it, and the scene it is seen and lit in. See
    /// [`crate::depth`].
    pub depth: crate::depth::Depth,
    pub scene: crate::depth::Scene,
    /// The paragraphs inside, which is what makes a shape a text box.
    pub text: Vec<Paragraph>,
    /// What the shape is called, which is what the selection pane would list.
    pub name: String,
    /// The number the file knows it by: `wps:cNvPr/@id`, which is what a
    /// connector names when it says which shape it is fastened to.
    pub id: u32,
    /// Which shapes the two ends of this connector are fastened to, for a
    /// shape that is a connector. See [`crate::joins`].
    pub joins: crate::joins::Joins,
    /// Where it floats, or `None` when it sits in the line of text.
    pub anchor: Option<crate::anchor::Anchor>,
    /// What it shows, said in words.
    ///
    /// Read out in place of the drawing by anything that cannot show it, which
    /// is the only thing a reader who cannot see it has.
    pub description: String,
    /// How far round it is turned, in sixtieths of a thousandth of a degree —
    /// `a:xfrm/@rot`, and the unit the whole of DrawingML measures angles in.
    /// Clockwise, and a whole turn is 21,600,000.
    pub rotation: i32,
    /// Whether it is drawn as its own mirror image, across or down.
    /// `@flipH` and `@flipV`, which are not a turn: a shape flipped once is
    /// not the same as one turned by anything.
    pub flipped_across: bool,
    pub flipped_down: bool,
}

impl Default for Shape {
    fn default() -> Self {
        Self {
            preset: "rect".to_owned(),
            adjusts: Vec::new(),
            width_emu: 0,
            height_emu: 0,
            fill: crate::fills::Fill::None,
            outline: None,
            outline_emu: 0,
            line_style: 0,
            ink: None,
            head_end: crate::lines::LineEnd::default(),
            tail_end: crate::lines::LineEnd::default(),
            effects: crate::shapeeffects::Effects::default(),
            depth: crate::depth::Depth::default(),
            scene: crate::depth::Scene::default(),
            text: Vec::new(),
            name: "Shape".to_owned(),
            id: 0,
            joins: crate::joins::Joins::default(),
            anchor: None,
            description: String::new(),
            rotation: 0,
            flipped_across: false,
            flipped_down: false,
        }
    }
}

impl Shape {
    /// One of the shape's adjustments, by the name the format calls it.
    ///
    /// `adj` for a shape with one and `adj1`, `adj2` and so on for a shape with
    /// several — and a shape with one is written both ways by different
    /// programs, so asking for either finds the other. Nothing when the
    /// document said nothing, which means the shape is drawn at the proportion
    /// the format falls back on.
    #[must_use]
    pub fn adjust(&self, name: &str) -> Option<i32> {
        let also = match name {
            "adj" => "adj1",
            "adj1" => "adj",
            _ => name,
        };
        self.adjusts
            .iter()
            .find(|(it, _)| it == name)
            .or_else(|| self.adjusts.iter().find(|(it, _)| it == also))
            .map(|(_, value)| *value)
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

    /// How thick its line is, in points, when it states a thickness.
    #[must_use]
    pub fn outline_points(&self) -> f32 {
        self.outline_emu as f32 / EMU_PER_POINT as f32
    }

    /// How thick its line is against a theme: its own thickness, or the
    /// thickness of the theme's line style it names, or three quarters of
    /// a point, which is what a line with no thickness anywhere is drawn at.
    #[must_use]
    pub fn outline_points_in(&self, theme: &crate::theme::Theme) -> f32 {
        let emu = if self.outline_emu > 0 {
            self.outline_emu
        } else {
            match theme.line_width_emu(self.line_style) {
                0 => 9525,
                width => width,
            }
        };
        emu as f32 / EMU_PER_POINT as f32
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
    ///
    /// Word's, and named the way Word names them: the theme's first fill
    /// style in the first accent, its second line style in that accent
    /// darkened by half, and white words. None of them is a colour, so a
    /// shape made here changes with the theme as one made in Word does.
    #[must_use]
    pub fn preset(preset: &str, width_points: f64, height_points: f64) -> Self {
        use crate::colour::Colour;
        use crate::theme::Slot;
        Self {
            preset: preset.to_owned(),
            width_emu: (width_points * EMU_PER_POINT as f64) as i64,
            height_emu: (height_points * EMU_PER_POINT as f64) as i64,
            fill: crate::fills::Fill::Styled { index: 1, colour: Colour::scheme(Slot::Accent1) },
            outline: Some(Colour::scheme(Slot::Accent1).shifted("shade", 50_000)),
            outline_emu: 0,
            line_style: 2,
            ink: Some(Colour::scheme(Slot::Light1)),
            text: Vec::new(),
            name: "Shape".to_owned(),
            anchor: None,
            description: String::new(),
            ..Self::default()
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
            fill: crate::fills::Fill::None,
            outline: Some(crate::colour::Colour::rgb("000000")),
            outline_emu: EMU_PER_POINT,
            text: text.split('\n').map(Paragraph::text).collect(),
            name: "Text Box".to_owned(),
            anchor: None,
            description: String::new(),
            ..Self::default()
        }
    }
}

/// Reads a shape out of a `w:drawing`, if that is what it holds.
#[must_use]
pub fn read_shape(drawing: &Element) -> Option<Shape> {
    // A group holds shapes, so a drawing that holds a group would answer here
    // with the first shape in it — and every command that replaces a shape
    // would then replace the whole group with that one member. A group is a
    // group. See [`crate::group`].
    if find(drawing, "wgp").is_some() {
        return None;
    }
    let wsp = find(drawing, "wsp")?;
    let mut shape = Shape { anchor: crate::anchor::read_anchor(drawing), ..Shape::default() };

    // What a connector is fastened to, which is on its own non-visual
    // properties: a shape with those is a connector and a shape without them
    // is not.
    if let Some(properties) = find(wsp, "cNvCnPr") {
        shape.joins = crate::joins::read_joins(properties);
    }
    if let Some(properties) = find(wsp, "cNvPr") {
        shape.id = properties.attribute_by_name("id").and_then(|id| id.parse().ok()).unwrap_or(0);
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
    // How far round it is turned, and whether it is drawn as its own mirror
    // image. Both live on the shape's own transform.
    if let Some(transform) = properties.and_then(|properties| child(properties, "xfrm")) {
        shape.rotation =
            transform.attribute_by_name("rot").and_then(|value| value.parse().ok()).unwrap_or(0);
        shape.flipped_across = matches!(transform.attribute_by_name("flipH"), Some("1" | "true"));
        shape.flipped_down = matches!(transform.attribute_by_name("flipV"), Some("1" | "true"));
    }
    if let Some(geometry) = properties.and_then(|properties| child(properties, "prstGeom")) {
        if let Some(name) = geometry.attribute_by_name("prst") {
            shape.preset = name.to_owned();
        }
        shape.adjusts = read_adjusts(geometry);
    }
    // What the shape takes from the theme by number: `wps:style`, which is
    // where a shape from Word's gallery keeps its fill, its line and the
    // colour of its words. What the properties state comes first; the style
    // answers for whatever they leave unsaid.
    let style = child(wsp, "style");
    let reference = |name: &str| -> (u8, Option<crate::colour::Colour>) {
        let Some(element) = style.and_then(|style| child(style, name)) else { return (0, None) };
        let index = element.attribute_by_name("idx").and_then(|idx| idx.parse().ok()).unwrap_or(0);
        (index, crate::colour::read_colour(element))
    };
    let (fill_style, fill_colour) = reference("fillRef");
    let (line_style, line_colour) = reference("lnRef");
    shape.fill = match properties.and_then(crate::fills::read_fill_said) {
        Some(fill) => fill,
        None => match (fill_style, fill_colour) {
            (index, Some(colour)) if index > 0 => crate::fills::Fill::Styled { index, colour },
            _ => crate::fills::Fill::None,
        },
    };
    shape.effects = properties.map(crate::shapeeffects::read_effects).unwrap_or_default();
    shape.depth = properties.map(crate::depth::read_depth).unwrap_or_default();
    shape.scene = properties.map(crate::depth::read_scene).unwrap_or_default();
    shape.line_style = line_style;
    shape.ink = reference("fontRef").1;

    match properties.and_then(|properties| child(properties, "ln")) {
        Some(line) => {
            shape.outline_emu =
                line.attribute_by_name("w").and_then(|w| w.parse().ok()).unwrap_or(0);
            shape.head_end = crate::lines::read_end(line, "headEnd");
            shape.tail_end = crate::lines::read_end(line, "tailEnd");
            // A line saying nothing about its colour is still a line, in the
            // colour the style names; and one saying nothing anywhere is
            // drawn in the theme's text colour, which is what Word does.
            shape.outline = if child(line, "noFill").is_some() {
                None
            } else {
                child(line, "solidFill")
                    .and_then(crate::colour::read_colour)
                    .or(line_colour)
                    .or_else(|| Some(crate::colour::Colour::scheme(crate::theme::Slot::Dark1)))
            };
        }
        // No line of its own: the style's, if it names one.
        None => shape.outline = if line_style > 0 { line_colour } else { None },
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
    // The 2010 extension goes last of all, after the graphic — pushed here and
    // moved to the end below, where the graphic is added.
    let relative =
        shape.anchor.as_ref().map(crate::anchor::relative_size_elements).unwrap_or_default();

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
    for element in relative {
        inline.push_element(element);
    }

    drawing.push_element(inline);
    drawing
}

/// The `wps:wsp` itself.
fn word_shape(shape: &Shape, prefix: Option<&str>) -> Element {
    let mut wsp = Element::new("wps:wsp", Some(WPS));
    wsp.declarations.push((Some("wps".to_owned()), WPS.to_owned()));

    let mut visible = Element::new("wps:cNvPr", Some(WPS));
    visible.set_attribute("id", &shape.id.max(1).to_string());
    visible.set_attribute("name", &shape.name);
    wsp.push_element(visible);
    // A connector says so here, and says what it is fastened to. Everything
    // else says it is an ordinary shape.
    if shape.joins.is_nothing() && !shape.preset.contains("onnector") {
        wsp.push_element(Element::new("wps:cNvSpPr", Some(WPS)));
    } else {
        let mut connector = Element::new("wps:cNvCnPr", Some(WPS));
        for element in crate::joins::join_elements(shape.joins) {
            connector.push_element(element);
        }
        wsp.push_element(connector);
    }

    let mut properties = Element::new("wps:spPr", Some(WPS));

    let mut transform = Element::new("a:xfrm", Some(A));
    // Written only when it says something: a shape that is not turned and not
    // mirrored says nothing, which is what every shape this program made until
    // now wrote.
    if shape.rotation != 0 {
        transform.set_attribute("rot", &shape.rotation.to_string());
    }
    if shape.flipped_across {
        transform.set_attribute("flipH", "1");
    }
    if shape.flipped_down {
        transform.set_attribute("flipV", "1");
    }
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
    geometry.push_element(adjust_element(&shape.adjusts, "a"));
    properties.push_element(geometry);

    // A fill the shape takes by number is said in its style, below, and not
    // here.
    if let Some(fill) = crate::fills::fill_element(&shape.fill) {
        properties.push_element(fill);
    }

    // The line is written when the shape says something of its own about it:
    // a thickness, no line at all, or an end. A line that is the style's
    // through and through is left to the style, which is how Word writes a
    // shape from its gallery.
    let has_ends = crate::lines::end_element("a:headEnd", shape.head_end).is_some()
        || crate::lines::end_element("a:tailEnd", shape.tail_end).is_some();
    let own_line =
        shape.outline.is_none() || shape.outline_emu > 0 || shape.line_style == 0 || has_ends;
    if own_line {
        let mut line = Element::new("a:ln", Some(A));
        if shape.outline_emu > 0 {
            line.set_attribute("w", &shape.outline_emu.to_string());
        }
        match &shape.outline {
            Some(colour) => line.push_element(crate::colour::solid_fill(colour)),
            None => line.push_element(Element::new("a:noFill", Some(A))),
        }
        // The ends come after the fill of the line, which is the order the
        // schema asks for: a document whose elements are in the wrong order
        // is a document Word will not open at all.
        for (name, end) in [("a:headEnd", shape.head_end), ("a:tailEnd", shape.tail_end)] {
            if let Some(element) = crate::lines::end_element(name, end) {
                line.push_element(element);
            }
        }
        properties.push_element(line);
    }
    // And what it is drawn with besides the two of them, which the schema
    // wants after the line and before anything three-dimensional.
    if let Some(list) = crate::shapeeffects::effects_element(&shape.effects) {
        properties.push_element(list);
    }
    // Then the scene it stands in and what makes it solid, in that order: the
    // schema asks for the room before the thing standing in it.
    if let Some(scene) = crate::depth::scene_element(&shape.scene) {
        properties.push_element(scene);
    }
    if let Some(solid) = crate::depth::depth_element(&shape.depth) {
        properties.push_element(solid);
    }
    wsp.push_element(properties);

    // What the shape takes from the theme by number, in the order the schema
    // wants: the line, the fill, the effect and the font. The effect is
    // always the theme's second effect style, which is what the Design tab's
    // Effects gallery changes; without it a shape is drawn flat however the
    // theme is set — see [`crate::theme::Effect`].
    let mut style = Element::new("wps:style", Some(WPS));
    let accent = crate::colour::Colour::scheme(crate::theme::Slot::Accent1);
    let reference = |name: &str, index: u8, colour: &crate::colour::Colour| {
        let mut element = Element::new(name, Some(A));
        element.set_attribute("idx", &index.to_string());
        element.push_element(crate::colour::colour_element(colour));
        element
    };
    style.push_element(reference(
        "a:lnRef",
        shape.line_style,
        shape.outline.as_ref().unwrap_or(&accent),
    ));
    let (fill_index, fill_colour) = match &shape.fill {
        crate::fills::Fill::Styled { index, colour } => (*index, colour),
        _ => (0, &accent),
    };
    style.push_element(reference("a:fillRef", fill_index, fill_colour));
    style.push_element(reference("a:effectRef", 2, &accent));
    let mut font = Element::new("a:fontRef", Some(A));
    font.set_attribute("idx", "minor");
    font.push_element(crate::colour::colour_element(
        shape.ink.as_ref().unwrap_or(&crate::colour::Colour::scheme(crate::theme::Slot::Dark1)),
    ));
    style.push_element(font);
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

/// The values behind a shape's yellow handles, as the document gave them.
///
/// A `gd` says its value as a formula, and the only formula an adjustment ever
/// uses is `val 25000`. One saying anything else is one this program cannot
/// work out, and it is kept out of the list rather than guessed at: a shape
/// drawn at the wrong proportion because a formula was misread is worse than
/// one drawn at the proportion the format falls back on.
fn read_adjusts(geometry: &Element) -> Vec<(String, i32)> {
    let Some(values) = child(geometry, "avLst") else {
        return Vec::new();
    };
    values
        .child_elements()
        .filter(|gd| gd.local_name() == "gd")
        .filter_map(|gd| {
            let name = gd.attribute_by_name("name")?;
            let formula = gd.attribute_by_name("fmla")?;
            let value = formula.strip_prefix("val ")?.trim().parse().ok()?;
            Some((name.to_owned(), value))
        })
        .collect()
}

/// And the same written back out.
///
/// An empty `avLst` where there were no adjustments, because that is what Word
/// writes there and a shape with no element at all is a shape Word rewrites the
/// first time it is opened.
fn adjust_element(adjusts: &[(String, i32)], prefix: &str) -> Element {
    let mut values = Element::new(&format!("{prefix}:avLst"), Some(A));
    for (name, value) in adjusts {
        let mut gd = Element::new(&format!("{prefix}:gd"), Some(A));
        gd.set_attribute("name", name);
        gd.set_attribute("fmla", &format!("val {value}"));
        values.push_element(gd);
    }
    values
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

        // A new floating drawing goes on top of the ones already there, which
        // is what Word does and what anybody putting one down expects. Two
        // drawings at the same depth would also be two that cannot be told
        // apart by the commands that move one past the other.
        let mut shape = shape.clone();
        if let Some(anchor) = &mut shape.anchor {
            anchor.depth = self.next_drawing_depth();
        }
        let shape = &shape;

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

    /// The number to give a floating drawing that is to go on top of the ones
    /// already in the document.
    ///
    /// One more than the highest there is, or Word's starting number when there
    /// are none. See [`crate::anchor::Anchor::depth`].
    #[must_use]
    pub fn next_drawing_depth(&self) -> u32 {
        self.drawing_depths()
            .into_iter()
            .max()
            .map_or(crate::anchor::USUAL_DEPTH, |highest| highest.saturating_add(1))
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

impl Document {
    /// Moves one of a shape's handles.
    ///
    /// The name is the format's own for that handle — `adj` for a shape with
    /// one and `adj1`, `adj2` and so on for a shape with several — and it comes
    /// from whatever described the handle, so that a value written here is
    /// found again by whatever drew it. A handle the shape already carries is
    /// changed where it stands, so the rest of the list keeps the order the
    /// document wrote it in.
    ///
    /// Returns whether anything changed: a handle dragged to where it already
    /// was is not an edit.
    pub fn set_adjust_at(&mut self, at: crate::TextPosition, name: &str, value: i32) -> bool {
        let Some(mut shape) = self.shape_at(at) else { return false };
        if shape.adjust(name) == Some(value) {
            return false;
        }
        // Under whichever of the two names it is already written, or under the
        // name it was asked for.
        let also = match name {
            "adj" => "adj1",
            "adj1" => "adj",
            other => other,
        };
        if let Some(entry) = shape.adjusts.iter_mut().find(|(it, _)| it == name || it == also) {
            entry.1 = value;
        } else {
            shape.adjusts.push((name.to_owned(), value));
        }
        self.replace_shape_at(at, &shape)
    }
}

impl Document {
    /// Every shape in the document with the place in the text it sits at.
    ///
    /// The place is what every command that changes a drawing is given, so a
    /// pass that walks the shapes in order to change one needs both halves.
    #[must_use]
    pub fn shape_places(&self) -> Vec<(crate::TextPosition, crate::shapes::Shape)> {
        let mut out = Vec::new();
        for index in 0..self.body().blocks.len() {
            let Some(paragraph) = self.paragraph_element(index) else { continue };
            let mut offset = 0usize;
            walk_shape_places(paragraph, index, &mut offset, &mut out);
        }
        out
    }

    /// Puts every connector fastened to a shape back where that shape is.
    ///
    /// # Why this is done at all, when the screen is already right
    ///
    /// Because the screen and the file are two different things. Where a joined
    /// connector is *drawn* follows from where the shapes it is fastened to
    /// went, and that is worked out afresh every time the document is laid out.
    /// The file still says the box it was saved with — so a document moved
    /// about here and then saved would open in Word with its connectors back
    /// where they used to be, which is the sort of thing that makes a program
    /// untrustworthy with somebody else's work.
    ///
    /// # What it leaves alone
    ///
    /// Connectors with an end fastened to nothing, and connectors whose shapes
    /// are not both placed by an offset from the same thing. Two drawings
    /// placed in different frames — one from the margin and one from the page —
    /// have no common measure in the model, and guessing one would move the
    /// connector somewhere neither shape is.
    pub fn rejoin_connectors(&mut self) -> bool {
        let places = self.shape_places();
        // Where each shape's box is, in the measure its anchor is stated in.
        let boxes: Vec<(
            u32,
            crate::anchor::Relative,
            crate::anchor::Relative,
            i64,
            i64,
            i64,
            i64,
        )> = places
            .iter()
            .filter(|(_, shape)| shape.id != 0)
            .filter_map(|(_, shape)| {
                let anchor = shape.anchor.as_ref()?;
                let (
                    crate::anchor::Placement::Offset(across),
                    crate::anchor::Placement::Offset(down),
                ) = (&anchor.horizontal, &anchor.vertical)
                else {
                    return None;
                };
                Some((
                    shape.id,
                    anchor.horizontal_from,
                    anchor.vertical_from,
                    *across,
                    *down,
                    shape.width_emu,
                    shape.height_emu,
                ))
            })
            .collect();

        let mut changed = false;
        for (at, shape) in &places {
            if shape.joins.is_nothing() {
                continue;
            }
            let Some(anchor) = shape.anchor.as_ref() else { continue };
            let Some(start) = shape.joins.start.and_then(|join| site_of(&boxes, join)) else {
                continue;
            };
            let Some(end) = shape.joins.end.and_then(|join| site_of(&boxes, join)) else {
                continue;
            };
            if start.0 != end.0 || start.1 != end.1 {
                // The two shapes are placed from different things.
                continue;
            }
            let (across, down) = (start.2.min(end.2), start.3.min(end.3));
            let (width, height) = ((end.2 - start.2).abs(), (end.3 - start.3).abs());

            let mut moved = anchor.clone();
            moved.horizontal_from = start.0;
            moved.vertical_from = start.1;
            moved.horizontal = crate::anchor::Placement::Offset(across);
            moved.vertical = crate::anchor::Placement::Offset(down);
            // Which way round it is drawn: an end fastened to a shape on the
            // right is the same connector mirrored.
            let flipped_across = end.2 < start.2;
            let flipped_down = end.3 < start.3;

            let same = anchor.horizontal == moved.horizontal
                && anchor.vertical == moved.vertical
                && anchor.horizontal_from == moved.horizontal_from
                && anchor.vertical_from == moved.vertical_from
                && shape.width_emu == width
                && shape.height_emu == height
                && shape.flipped_across == flipped_across
                && shape.flipped_down == flipped_down;
            if same {
                continue;
            }

            changed |= self.set_anchor_at(*at, Some(&moved));
            changed |= self.set_drawing_size_at(*at, width, height);
            changed |= self.set_drawing_turn_at(
                *at,
                crate::floating::Turned { rotation: shape.rotation, flipped_across, flipped_down },
            );
        }
        changed
    }
}

/// Where one end of a connector is fastened, in the measure its shape is placed
/// in: what it is placed from, and the point itself.
fn site_of(
    boxes: &[(u32, crate::anchor::Relative, crate::anchor::Relative, i64, i64, i64, i64)],
    join: crate::joins::Join,
) -> Option<(crate::anchor::Relative, crate::anchor::Relative, i64, i64)> {
    let (_, from_x, from_y, across, down, width, height) =
        *boxes.iter().find(|(id, ..)| *id == join.shape)?;
    // The same four points the layout uses, in the format's own order: the
    // top, the left, the bottom, the right. See
    // [`wp_layout::connectors::connection_site`].
    let (x, y) = match join.site {
        0 => (across + width / 2, down),
        1 => (across, down + height / 2),
        2 => (across + width / 2, down + height),
        3 => (across + width, down + height / 2),
        _ => (across + width / 2, down + height / 2),
    };
    Some((from_x, from_y, x, y))
}

/// Finds every shape in an element, counting the offsets as the caret does.
fn walk_shape_places(
    element: &Element,
    paragraph: usize,
    offset: &mut usize,
    out: &mut Vec<(crate::TextPosition, crate::shapes::Shape)>,
) {
    for node in &element.children {
        let Some(child) = node.as_element() else { continue };
        if child.namespace.as_deref() == Some(read::W)
            && matches!(child.local_name(), "drawing" | "pict" | "object")
        {
            if let Some(shape) = read_shape(child) {
                out.push((crate::TextPosition::new(paragraph, *offset), shape));
            }
            *offset += 1;
            continue;
        }
        if child.namespace.as_deref() == Some(read::W) && child.local_name() == "t" {
            *offset += child.text_content().len();
            continue;
        }
        walk_shape_places(child, paragraph, offset, out);
    }
}

/// Finds every shape in a block, whatever it is nested in.
fn gather_shapes(block: &crate::model::Block, out: &mut Vec<crate::shapes::Shape>) {
    match block {
        crate::model::Block::Paragraph(paragraph) => {
            for run in &paragraph.runs {
                for piece in &run.content {
                    if let crate::model::RunContent::Shape(shape) = piece {
                        out.push((**shape).clone());
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
    /// Which drawing that is, when there are two of them and the caret is
    /// between them, is [`Document::drawing_place_here`]'s answer rather than
    /// this one's: a shape does not come before a picture any more.
    #[must_use]
    pub fn shape_here(&self) -> Option<crate::shapes::Shape> {
        self.shape_at(self.drawing_place_here()?)
    }

    /// The shape at one place in the text, if a shape is what is there.
    ///
    /// The place is the drawing's own: the offset of the character it takes,
    /// not a caret beside it. Two drawings next to each other are two places.
    #[must_use]
    pub fn shape_at(&self, at: crate::TextPosition) -> Option<crate::shapes::Shape> {
        let paragraph = self.paragraph_element(at.paragraph)?;
        let mut found = None;
        let mut offset = 0usize;
        walk_shapes(paragraph, &mut offset, at.offset, &mut found);
        found
    }

    /// Replaces the shape the caret is beside.
    pub fn replace_shape_here(&mut self, shape: &crate::shapes::Shape) -> bool {
        let Some(at) = self.drawing_place_here() else { return false };
        self.replace_shape_at(at, shape)
    }

    /// Replaces the shape at one place in the text.
    pub fn replace_shape_at(
        &mut self,
        at: crate::TextPosition,
        shape: &crate::shapes::Shape,
    ) -> bool {
        let caret = self.caret();
        if self.shape_at(at).is_none() {
            return false;
        }
        self.record(crate::history::EditKind::Structural, caret, false);

        let prefix = self.prefix();
        let Some(path) = crate::position::paragraph_path(&self.tree().root, at.paragraph) else {
            return false;
        };
        let Some(paragraph) = edit::element_at_path_mut(&mut self.tree_mut().root, &path) else {
            return false;
        };

        let replacement = shape_element(shape, prefix.as_deref());
        let mut offset = 0usize;
        let replaced = replace_shape(paragraph, &mut offset, at.offset, replacement);
        if replaced {
            self.mark_modified();
        }
        replaced
    }
}

/// Finds the shape at an offset, if there is one.
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
            // The offset wanted is the drawing's own, not a caret beside it:
            // which of two neighbours is meant is settled before this.
            if *offset == wanted {
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

/// Puts a new drawing in the place of the one at an offset.
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
            if *offset == wanted && read_shape(child).is_some() {
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
        assert_eq!(read.fill, crate::fills::Fill::None, "a text box lets the page show through");
        assert_eq!(read.outline, Some(crate::colour::Colour::rgb("000000")));
    }

    #[test]
    fn a_gallery_shape_names_its_colours_from_the_theme_and_keeps_them_through_a_round_trip() {
        use crate::colour::{Base, Colour};
        use crate::theme::{Slot, Theme};

        let shape = Shape::preset("rect", 100.0, 50.0);
        let element = shape_element(&shape, Some("w"));
        // Written as Word writes it: nothing about the fill or the line in the
        // properties, and the style saying which of the theme's to take.
        let properties = find(&element, "spPr").expect("properties");
        assert!(child(properties, "solidFill").is_none());
        assert!(child(properties, "ln").is_none());
        let style = find(&element, "style").expect("a style");
        assert_eq!(child(style, "fillRef").and_then(|r| r.attribute_by_name("idx")), Some("1"));
        assert_eq!(child(style, "lnRef").and_then(|r| r.attribute_by_name("idx")), Some("2"));
        assert!(child(style, "fontRef").is_some());

        let read = read_shape(&element).expect("a shape");
        assert_eq!(read.fill, shape.fill);
        assert_eq!(read.outline, shape.outline);
        assert_eq!(read.line_style, 2);
        assert_eq!(read.ink, Some(Colour::scheme(Slot::Light1)));
        let Some(outline) = &read.outline else { panic!("no outline") };
        assert_eq!(outline.base, Base::Scheme(Slot::Accent1));

        // Against the Office theme that is the blue and the darker blue.
        let theme = Theme::default();
        assert_eq!(read.fill.solid_hex(&theme), Some("4472C4".to_owned()));
        assert_eq!(outline.resolve(&theme), "223962");
        assert!((read.outline_points_in(&theme) - 1.0).abs() < 0.01, "line style two is a point");
        // And against another theme, that theme's.
        let mut red = Theme::default();
        red.colors[4] = "C00000".to_owned();
        assert_eq!(read.fill.solid_hex(&red), Some("C00000".to_owned()));
    }

    #[test]
    fn a_line_of_the_shape_s_own_over_the_style_keeps_its_thickness_and_takes_the_style_s_colour() {
        use crate::colour::{Base, Colour};
        use crate::theme::Slot;

        let mut shape = Shape::preset("ellipse", 100.0, 50.0);
        shape.outline_emu = EMU_PER_POINT * 3;
        let element = shape_element(&shape, Some("w"));
        let properties = find(&element, "spPr").expect("properties");
        let line = child(properties, "ln").expect("the line is the shape's own now");
        assert_eq!(line.attribute_by_name("w"), Some("38100"));
        let read = read_shape(&element).expect("a shape");
        assert_eq!(read.outline_emu, EMU_PER_POINT * 3);
        assert_eq!(read.outline, shape.outline);

        // A line that says nothing about its colour takes the style's; one
        // that says nothing anywhere is drawn in the text colour.
        let text = "<w:drawing xmlns:w=\"w\" xmlns:wps=\"wps\" xmlns:a=\"a\"><wp:inline xmlns:wp=\"wp\">\
            <wp:extent cx=\"914400\" cy=\"914400\"/></wp:inline>\
            <wps:wsp><wps:spPr><a:prstGeom prst=\"rect\"/><a:ln w=\"12700\"/></wps:spPr>\
            <wps:style><a:lnRef idx=\"1\"><a:schemeClr val=\"accent4\"/></a:lnRef>\
            <a:fillRef idx=\"0\"><a:schemeClr val=\"accent1\"/></a:fillRef></wps:style></wps:wsp></w:drawing>";
        let read =
            read_shape(&wp_xml::tree::XmlTree::parse(text).expect("parses").root).expect("a shape");
        assert_eq!(read.outline, Some(Colour::scheme(Slot::Accent4)));
        assert_eq!(read.fill, crate::fills::Fill::None, "fill style nought is no fill");
        let bare = text
            .replace("<wps:style>", "<wps:style><a:lnRef idx=\"0\"/>")
            .replace("<a:lnRef idx=\"1\"><a:schemeClr val=\"accent4\"/></a:lnRef>", "");
        let read = read_shape(&wp_xml::tree::XmlTree::parse(&bare).expect("parses").root)
            .expect("a shape");
        assert_eq!(read.outline.map(|colour| colour.base), Some(Base::Scheme(Slot::Dark1)));
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
