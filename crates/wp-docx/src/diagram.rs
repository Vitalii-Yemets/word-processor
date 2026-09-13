//! Diagrams: SmartArt, and the five parts a document keeps one in.
//!
//! # What a diagram is made of
//!
//! A frame and five parts. The frame sits in the text and says almost nothing:
//! `dgm:relIds` names four relationships and stops. Behind it are the data
//! model — the words and how they are related — the layout, which is the rules
//! for turning those words into a picture, the quick style and the colours.
//! And hanging off the data model is a fifth part nobody names in the schema:
//! the drawing Word made the last time it followed those rules.
//!
//! That drawing is the part everyone who is not Word depends on. It is shapes
//! with places, colours and words in them, and it is what this program draws:
//! a diagram Word laid out is drawn here exactly as Word laid it out, because
//! it *is* what Word laid out. The alternative is running the layout language
//! — a small typesetting engine of conditions, constraints and rules — and
//! drawing something that nearly agrees with Word instead.
//!
//! # What is written
//!
//! All five, so the diagram Word opens is a diagram and not a heap of boxes:
//! the data model with a point for every item and the connections between
//! them, a layout definition, a quick style, a colour list, and the drawing,
//! so anything that does not run the layout language still shows the picture.
//!
//! The layout definition written here is this program's own and is simpler
//! than the gallery's: the arrangements below say what they arrange, and the
//! same arrangement in Word's gallery has proportions and rules this one does
//! not. Word re-lays a diagram out whenever its text changes, and what it
//! draws then is what the definition in the file says — which is why one is
//! written at all, rather than the drawing alone.
//!
//! # Why the sizes are worked out from the room available
//!
//! A diagram wider than the text is a diagram half off the page. The caller
//! passes how much room there is — it is the one that knows the margins — and
//! the boxes are divided out of it.

use wp_xml::tree::Element;

use crate::group::{Group, Inside, Member};
use crate::model::{Alignment, Paragraph, ParagraphProperties, Run, RunContent, RunProperties};
use crate::shapes::Shape;
use crate::theme::{Slot, Theme};
use crate::{Document, Error, EMU_PER_INCH};

/// The namespace the four standard parts are written in.
pub const DIAGRAM: &str = "http://schemas.openxmlformats.org/drawingml/2006/diagram";
/// What a drawing says it holds when what it holds is a diagram.
pub const DIAGRAM_URI: &str = DIAGRAM;
/// The drawing itself, which is Microsoft's own extension to the format.
pub const DIAGRAM_DRAWING: &str = "http://schemas.microsoft.com/office/drawing/2008/diagram";

/// The relationships that reach the four parts the frame names.
pub const DATA_RELATIONSHIP: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/diagramData";
pub const LAYOUT_RELATIONSHIP: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/diagramLayout";
pub const STYLE_RELATIONSHIP: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/diagramQuickStyle";
pub const COLORS_RELATIONSHIP: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/diagramColors";
/// And the fifth, which the data model points at rather than the frame.
pub const DRAWING_RELATIONSHIP: &str =
    "http://schemas.microsoft.com/office/2007/relationships/diagramDrawing";

pub const DATA_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.drawingml.diagramData+xml";
pub const LAYOUT_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.drawingml.diagramLayout+xml";
pub const STYLE_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.drawingml.diagramStyle+xml";
pub const COLORS_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.drawingml.diagramColors+xml";
pub const DRAWING_CONTENT_TYPE: &str = "application/vnd.ms-office.drawingml.diagramDrawing+xml";

/// How the boxes are arranged.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Arrangement {
    /// One box after another across the page, with arrows between them.
    #[default]
    Process,
    /// One box under another, the whole width of the text.
    List,
    /// The first box over the rest, which fan out beneath it.
    Hierarchy,
}

impl Arrangement {
    pub const ALL: &'static [Self] = &[Self::Process, Self::List, Self::Hierarchy];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Process => "Process",
            Self::List => "Vertical list",
            Self::Hierarchy => "Hierarchy",
        }
    }

    /// The name the gallery gives this arrangement.
    ///
    /// Word's own, and deliberately: a diagram that calls itself by a name
    /// Word knows is a diagram Word offers to restyle and to change the layout
    /// of, in the tabs that appear when it is selected.
    #[must_use]
    pub fn layout_id(self) -> &'static str {
        match self {
            Self::Process => "urn:microsoft.com/office/officeart/2005/8/layout/process1",
            Self::List => "urn:microsoft.com/office/officeart/2005/8/layout/vList2",
            Self::Hierarchy => "urn:microsoft.com/office/officeart/2005/8/layout/hierarchy1",
        }
    }

    /// Which shelf of the gallery it sits on.
    #[must_use]
    pub fn category(self) -> &'static str {
        match self {
            Self::Process => "process",
            Self::List => "list",
            Self::Hierarchy => "hierarchy",
        }
    }

    /// Which arrangement a layout part names, when it names one of these.
    ///
    /// A diagram laid out by any of the hundred-odd other layouts in the
    /// gallery is not one of these, and says so by answering nothing: it is
    /// drawn from its own drawing, which is what it was laid out into.
    #[must_use]
    pub fn from_layout_id(id: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|arrangement| arrangement.layout_id() == id)
    }
}

/// How tall a box is.
const BOX_HEIGHT: i64 = EMU_PER_INCH * 7 / 10;
/// And a box in a stacked list, which holds a line and no more.
const LIST_HEIGHT: i64 = EMU_PER_INCH / 2;
/// The room between one box and the next.
const GAP: i64 = EMU_PER_INCH / 4;
/// How wide the arrow between two boxes of a process is.
const ARROW: i64 = EMU_PER_INCH / 5;
/// How thick the line joining a box to the one above it is drawn.
const JOIN: i64 = EMU_PER_INCH / 72;

/// The words of a diagram, as a tree.
///
/// A tree because the data model is one: a box may stand under another, which
/// is what makes a hierarchy a hierarchy rather than a row of boxes. The
/// arrangements that do not use the depth read a tree one level deep.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Node {
    pub text: String,
    pub children: Vec<Node>,
}

impl Node {
    #[must_use]
    pub fn new(text: &str) -> Self {
        Self { text: text.to_owned(), children: Vec::new() }
    }

    /// This node and everything under it, in the order the text pane lists
    /// them: a node before its children, a child before its own.
    fn walk<'a>(&'a self, out: &mut Vec<&'a Node>) {
        out.push(self);
        for child in &self.children {
            child.walk(out);
        }
    }
}

/// A diagram as the document keeps it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Diagram {
    /// Which arrangement the layout part names, when it names one this
    /// program knows.
    pub arrangement: Option<Arrangement>,
    /// The words, as a tree.
    pub nodes: Vec<Node>,
    /// What was drawn the last time the diagram was laid out, if the file
    /// carries it. This is what is drawn.
    pub drawing: Option<Group>,
}

impl Diagram {
    /// Every node of the tree, top to bottom.
    #[must_use]
    pub fn items(&self) -> Vec<&Node> {
        let mut out = Vec::new();
        for node in &self.nodes {
            node.walk(&mut out);
        }
        out
    }

    /// The words, one line per node, which is what the text pane shows.
    #[must_use]
    pub fn text(&self) -> Vec<String> {
        self.items().into_iter().map(|node| node.text.clone()).collect()
    }
}

/// What one drawn piece of a diagram is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Piece {
    /// A box with words in it.
    Box,
    /// The arrow from one box of a process to the next.
    Arrow,
    /// The line from a box to one standing under it.
    Join,
}

/// One piece of a diagram and where it goes, in the diagram's own space.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Place {
    /// Which point of the data model it draws.
    model: i32,
    preset: &'static str,
    kind: Piece,
    text: String,
    x: i64,
    y: i64,
    width: i64,
    height: i64,
}

/// The point of the data model that stands for the item at `index`.
///
/// Three apart, because each item owns three points: itself, the transition in
/// from whatever it hangs under, and the transition on to the next. Word gives
/// every one of them a name of its own and so does this.
const fn node_id(index: usize) -> i32 {
    100 + index as i32 * 3
}

/// The transition into an item: the line drawn from its parent to it.
const fn parent_id(index: usize) -> i32 {
    node_id(index) + 1
}

/// And the transition out of it: the arrow drawn from it to the next.
const fn sibling_id(index: usize) -> i32 {
    node_id(index) + 2
}

impl Document {
    /// Puts a diagram of `items` at the caret, inside `width_emu` of room.
    ///
    /// Five parts and a frame. Returns whether one was drawn, which is false
    /// when there is nothing to draw one of.
    pub fn insert_diagram(
        &mut self,
        arrangement: Arrangement,
        items: &[String],
        width_emu: i64,
    ) -> Result<bool, Error> {
        let items: Vec<&str> =
            items.iter().map(|item| item.trim()).filter(|item| !item.is_empty()).collect();
        if items.is_empty() || width_emu <= 0 {
            return Ok(false);
        }

        let places = places(arrangement, &items, width_emu);
        let height = places.iter().map(|place| place.y + place.height).max().unwrap_or(0);
        if height <= 0 {
            return Ok(false);
        }

        let theme = self.theme();
        let fill = theme.color(Slot::Accent1);
        let paper = theme.color(Slot::Light1);

        // One number for all five, because Word numbers them together and a
        // diagram whose parts do not share a number is one nobody can follow
        // by eye.
        let index = self.free_diagram_index();
        let data = format!("word/diagrams/data{index}.xml");
        let layout = format!("word/diagrams/layout{index}.xml");
        let quick_style = format!("word/diagrams/quickStyle{index}.xml");
        let colours = format!("word/diagrams/colors{index}.xml");
        let drawing = format!("word/diagrams/drawing{index}.xml");

        let caret = self.caret();
        self.record(crate::history::EditKind::Structural, caret, false);

        // The drawing first, because the data model has to name it, and it is
        // the data model's own relationship that reaches it rather than the
        // document's.
        self.add_package_part(&drawing, DRAWING_CONTENT_TYPE, drawing_xml(&places, &fill, &paper));
        // Beside the data model rather than under the document: the drawing is
        // the data model's own, and a relationship written from anywhere else
        // would point at it from the wrong part.
        let drawn_id =
            self.point_part_at(&data, &format!("drawing{index}.xml"), DRAWING_RELATIONSHIP)?;

        self.add_package_part(
            &data,
            DATA_CONTENT_TYPE,
            data_model_xml(arrangement, &items, &drawn_id),
        );
        self.add_package_part(&layout, LAYOUT_CONTENT_TYPE, layout_xml(arrangement));
        self.add_package_part(&quick_style, STYLE_CONTENT_TYPE, quick_style_xml());
        self.add_package_part(&colours, COLORS_CONTENT_TYPE, colours_xml());

        let ids = Ids {
            data: self.point_at_part(&data, DATA_RELATIONSHIP)?,
            layout: self.point_at_part(&layout, LAYOUT_RELATIONSHIP)?,
            style: self.point_at_part(&quick_style, STYLE_RELATIONSHIP)?,
            colours: self.point_at_part(&colours, COLORS_RELATIONSHIP)?,
        };

        let prefix = self.prefix();
        let frame = frame_element(&ids, width_emu, height, prefix.as_deref());
        let inserted = crate::position::insert_element_at(
            &mut self.tree_mut().root,
            caret,
            frame,
            prefix.as_deref(),
        );

        if inserted {
            self.set_caret(crate::TextPosition::new(caret.paragraph, caret.offset + 1));
            self.mark_modified();
        }
        Ok(inserted)
    }

    /// The diagram a frame points at, if the package holds one.
    ///
    /// The reference rather than the relationship alone, because how big the
    /// frame is is what the drawing inside it is measured against.
    #[must_use]
    pub fn diagram(&self, reference: &crate::model::DiagramReference) -> Option<Diagram> {
        let target = self.relationship_target(&reference.relationship)?;
        let text = self.package().xml_part(&target).and_then(Result::ok)?;
        let tree = wp_xml::tree::XmlTree::parse(&text).ok()?;

        let (nodes, arrangement, drawn) = read_data_model(&tree.root);
        let theme = self.theme();
        let drawing = drawn
            .and_then(|id| self.diagram_drawing(&target, &id, &theme))
            .or_else(|| {
                // No drawing in the file: the words, arranged the way the
                // layout part names — or listed, when it names one of the
                // hundred-odd layouts this program cannot lay out. A diagram
                // drawn as nothing says nothing, and what it says is the one
                // thing the data model does carry.
                (!nodes.is_empty()).then(|| {
                    drawn_from_words(
                        arrangement.unwrap_or(Arrangement::List),
                        &nodes,
                        reference.width_emu.max(EMU_PER_INCH),
                        &theme.color(Slot::Accent1),
                        &theme.color(Slot::Light1),
                    )
                })
            })
            .map(|mut group| {
                // The frame is what the text made room for, and what the
                // drawing is measured in is its own rectangle: the two need
                // not agree, and a diagram resized in Word is exactly the case
                // where they do not.
                group.width_emu = reference.width_emu.max(1);
                group.height_emu = reference.height_emu.max(1);
                group
            });

        Some(Diagram { arrangement, nodes, drawing })
    }

    /// Reads the drawing the data model points at.
    fn diagram_drawing(&self, data_part: &str, relationship: &str, theme: &Theme) -> Option<Group> {
        let relationships = self.package().relationships(data_part).ok()?;
        let target = relationships.by_id(relationship)?.resolved_target(data_part)?.ok()?;
        let text = self.package().xml_part(&target).and_then(Result::ok)?;
        let tree = wp_xml::tree::XmlTree::parse(&text).ok()?;
        read_drawing(&tree.root, theme)
    }

    /// Every diagram in the body, in the order they are written.
    #[must_use]
    pub fn diagrams(&self) -> Vec<crate::model::DiagramReference> {
        let mut out = Vec::new();
        for block in &self.body().blocks {
            gather_diagrams(block, &mut out);
        }
        out
    }

    /// A number no diagram in the package is using.
    fn free_diagram_index(&self) -> usize {
        let mut index = 1usize;
        while self.package().part(&format!("word/diagrams/data{index}.xml")).is_some() {
            index += 1;
        }
        index
    }
}

/// Every diagram a block holds, including the ones inside its cells.
fn gather_diagrams(block: &crate::model::Block, out: &mut Vec<crate::model::DiagramReference>) {
    match block {
        crate::model::Block::Paragraph(paragraph) => {
            for run in &paragraph.runs {
                for piece in &run.content {
                    if let RunContent::Diagram(reference) = piece {
                        out.push(reference.clone());
                    }
                }
            }
        }
        crate::model::Block::Table(table) => {
            for row in &table.rows {
                for cell in &row.cells {
                    for block in &cell.blocks {
                        gather_diagrams(block, out);
                    }
                }
            }
        }
    }
}

/// Which relationships the frame names.
struct Ids {
    data: String,
    layout: String,
    style: String,
    colours: String,
}

/// A node's words and its children's, top to bottom.
fn once_and_children(node: &Node) -> Vec<String> {
    let mut out = vec![node.text.clone()];
    for child in &node.children {
        out.extend(once_and_children(child));
    }
    out
}

/// Works out where every piece of a diagram goes.
fn places(arrangement: Arrangement, items: &[&str], width: i64) -> Vec<Place> {
    match arrangement {
        Arrangement::Process => process(items, width),
        Arrangement::List => list(items, width),
        Arrangement::Hierarchy => hierarchy(items, width),
    }
}

/// Boxes across the page with arrows between them.
fn process(items: &[&str], width: i64) -> Vec<Place> {
    let count = items.len() as i64;
    // The arrows take room out of the row before the boxes are divided up.
    let arrows = (count - 1).max(0) * (ARROW + GAP);
    let box_width = ((width - arrows) / count).max(EMU_PER_INCH / 2);

    let mut places = Vec::new();
    let mut x = 0i64;
    for (index, item) in items.iter().enumerate() {
        places.push(boxed(index, item, x, 0, box_width, BOX_HEIGHT));
        x += box_width;
        if index + 1 < items.len() {
            // The arrow sits in the middle of the gap, half the height of a
            // box so it points along the row rather than filling it.
            let arrow_top = (BOX_HEIGHT - BOX_HEIGHT / 3) / 2;
            places.push(Place {
                model: sibling_id(index),
                preset: "rightArrow",
                kind: Piece::Arrow,
                text: String::new(),
                x: x + GAP / 2,
                y: arrow_top,
                width: ARROW,
                height: BOX_HEIGHT / 3,
            });
            x += ARROW + GAP;
        }
    }
    places
}

/// Boxes one under another.
fn list(items: &[&str], width: i64) -> Vec<Place> {
    let mut places = Vec::new();
    let mut y = 0i64;
    for (index, item) in items.iter().enumerate() {
        places.push(boxed(index, item, 0, y, width, LIST_HEIGHT));
        y += LIST_HEIGHT + GAP / 2;
    }
    places
}

/// One box over the rest, with a line down to each of them.
fn hierarchy(items: &[&str], width: i64) -> Vec<Place> {
    let Some((first, rest)) = items.split_first() else { return Vec::new() };

    // The top box is a third of the width, in the middle.
    let top_width = (width / 3).max(EMU_PER_INCH);
    let mut places = vec![boxed(0, first, (width - top_width) / 2, 0, top_width, BOX_HEIGHT)];
    if rest.is_empty() {
        return places;
    }

    let count = rest.len() as i64;
    let gaps = (count - 1).max(0) * GAP;
    let box_width = ((width - gaps) / count).max(EMU_PER_INCH / 2);
    let row_top = BOX_HEIGHT + GAP;
    let from_x = width / 2;
    // Halfway down the gap, where the run across every child goes.
    let middle = BOX_HEIGHT + GAP / 2;

    // The stem out of the box above, drawn once however many hang under it.
    places.push(bar(parent_id(0), from_x - JOIN / 2, BOX_HEIGHT, JOIN, GAP / 2));

    let mut x = 0i64;
    for (index, item) in rest.iter().enumerate() {
        let index = index + 1;
        places.push(boxed(index, item, x, row_top, box_width, BOX_HEIGHT));

        // Down out of the box above, across, and down into this one — which is
        // how a tree is drawn and is not what any of the bent connectors draw:
        // every one of those leaves sideways, because what they join is one
        // shape's side to another's.
        let to_x = x + box_width / 2;
        let (left, right) = (from_x.min(to_x), from_x.max(to_x));
        places.push(bar(parent_id(index), left, middle - JOIN / 2, (right - left).max(JOIN), JOIN));
        places.push(bar(parent_id(index), to_x - JOIN / 2, middle, JOIN, row_top - middle));
        x += box_width + GAP;
    }
    places
}

/// One piece of the line joining a box to the one above it.
///
/// A bar and not a connector, and as many bars as the line has straight parts.
/// The pieces that draw one transition of the model all carry its name: the
/// drawing is a picture of the model and not a second copy of it.
fn bar(model: i32, x: i64, y: i64, width: i64, height: i64) -> Place {
    Place {
        model,
        preset: "rect",
        kind: Piece::Join,
        text: String::new(),
        x,
        y,
        width: width.max(JOIN),
        height: height.max(JOIN),
    }
}

/// A box with words in it.
fn boxed(index: usize, text: &str, x: i64, y: i64, width: i64, height: i64) -> Place {
    Place {
        model: node_id(index),
        preset: "roundRect",
        kind: Piece::Box,
        text: text.to_owned(),
        x,
        y,
        width,
        height,
    }
}

/// The five characters XML will not take as themselves.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            other => out.push(other),
        }
    }
    out
}

/// The words of one point or one shape, as DrawingML writes text.
fn text_xml(text: &str, size: Option<u32>, colour: Option<&str>) -> String {
    if text.is_empty() {
        return "<a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang=\"en-US\"/></a:p>".to_owned();
    }

    let mut properties = String::from("<a:rPr lang=\"en-US\"");
    if let Some(size) = size {
        properties.push_str(&format!(" sz=\"{size}\" b=\"1\""));
    }
    properties.push('>');
    if let Some(colour) = colour {
        properties.push_str(&format!("<a:solidFill><a:srgbClr val=\"{colour}\"/></a:solidFill>"));
    }
    properties.push_str("</a:rPr>");

    format!(
        "<a:bodyPr/><a:lstStyle/><a:p><a:pPr algn=\"ctr\"/><a:r>{properties}<a:t>{}</a:t></a:r></a:p>",
        escape(text)
    )
}

/// The data model: a point for every item and the connections between them.
fn data_model_xml(arrangement: Arrangement, items: &[&str], drawing: &str) -> String {
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
         <dgm:dataModel xmlns:dgm=\"{DIAGRAM}\" xmlns:a=\"{main}\" xmlns:r=\"{rel}\"><dgm:ptLst>",
        main = crate::edit::DRAWING_MAIN,
        rel = crate::edit::RELATIONSHIPS,
    );

    // The document point, which is the root everything else hangs under and
    // the one place the file says which layout, style and colours it wants.
    out.push_str(&format!(
        "<dgm:pt modelId=\"1\" type=\"doc\"><dgm:prSet loTypeId=\"{layout}\" loCatId=\"{category}\" \
         qsTypeId=\"{style}\" qsCatId=\"simple\" csTypeId=\"{colours}\" csCatId=\"accent1\" \
         phldr=\"0\"/><dgm:spPr/><dgm:t>{text}</dgm:t></dgm:pt>",
        layout = arrangement.layout_id(),
        category = arrangement.category(),
        style = QUICK_STYLE_ID,
        colours = COLOURS_ID,
        text = text_xml("", None, None),
    ));

    for (index, item) in items.iter().enumerate() {
        out.push_str(&format!(
            "<dgm:pt modelId=\"{}\"><dgm:prSet phldrT=\"[Text]\"/><dgm:spPr/><dgm:t>{}</dgm:t></dgm:pt>",
            node_id(index),
            text_xml(item, None, None),
        ));
        // The two transitions every item owns: the line in from whatever it
        // hangs under, and the arrow on to the next. They carry no words, and
        // they are here because the layout language reaches for them by name.
        for (id, kind) in [(parent_id(index), "parTrans"), (sibling_id(index), "sibTrans")] {
            out.push_str(&format!(
                "<dgm:pt modelId=\"{id}\" type=\"{kind}\" cxnId=\"{}\"><dgm:prSet/><dgm:spPr/>\
                 <dgm:t>{}</dgm:t></dgm:pt>",
                connection_id(index),
                text_xml("", None, None),
            ));
        }
    }

    out.push_str("</dgm:ptLst><dgm:cxnLst>");
    for (index, _) in items.iter().enumerate() {
        // A hierarchy hangs everything after the first under the first; the
        // other two hang everything under the document itself, which is what
        // makes them a row and not a tree.
        let (parent, order) = match arrangement {
            Arrangement::Hierarchy if index > 0 => (node_id(0), index - 1),
            _ => (1, index),
        };
        out.push_str(&format!(
            "<dgm:cxn modelId=\"{}\" srcId=\"{parent}\" destId=\"{}\" srcOrd=\"{order}\" \
             destOrd=\"0\" parTransId=\"{}\" sibTransId=\"{}\"/>",
            connection_id(index),
            node_id(index),
            parent_id(index),
            sibling_id(index),
        ));
    }
    out.push_str("</dgm:cxnLst><dgm:bg/><dgm:whole/>");

    // Where the drawing is. The schema has no room for it, so it goes in the
    // extension list, which is where every part of the format that came later
    // than the schema goes.
    out.push_str(&format!(
        "<dgm:extLst><a:ext uri=\"{{B4F0B5AB-D6F5-4A4B-A4B5-6FB2DA4A1A9F}}\">\
         <dsp:dataModelExt xmlns:dsp=\"{DIAGRAM_DRAWING}\" relId=\"{drawing}\" minVer=\"12.0\"/>\
         </a:ext></dgm:extLst>"
    ));
    out.push_str("</dgm:dataModel>");
    out
}

/// The connection that hangs the item at `index` under whatever holds it.
const fn connection_id(index: usize) -> i32 {
    1000 + index as i32
}

/// The drawing: the picture as it stands, for everything that does not run the
/// layout language.
fn drawing_xml(places: &[Place], fill: &str, paper: &str) -> String {
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
         <dsp:drawing xmlns:dsp=\"{DIAGRAM_DRAWING}\" xmlns:a=\"{main}\"><dsp:spTree>\
         <dsp:nvGrpSpPr><dsp:cNvPr id=\"0\" name=\"\"/><dsp:cNvGrpSpPr/></dsp:nvGrpSpPr>\
         <dsp:grpSpPr/>",
        main = crate::edit::DRAWING_MAIN,
    );

    for place in places {
        out.push_str(&format!(
            "<dsp:sp modelId=\"{}\"><dsp:nvSpPr><dsp:cNvPr id=\"0\" name=\"\"/><dsp:cNvSpPr/>\
             </dsp:nvSpPr><dsp:spPr><a:xfrm><a:off x=\"{}\" y=\"{}\"/>\
             <a:ext cx=\"{}\" cy=\"{}\"/></a:xfrm><a:prstGeom prst=\"{}\"><a:avLst/></a:prstGeom>\
             <a:solidFill><a:srgbClr val=\"{fill}\"/></a:solidFill><a:ln><a:noFill/></a:ln>",
            place.model, place.x, place.y, place.width, place.height, place.preset,
        ));
        out.push_str("</dsp:spPr>");
        out.push_str(&format!(
            "<dsp:txBody>{}</dsp:txBody>",
            text_xml(&place.text, Some(1200), Some(paper))
        ));
        out.push_str(&format!(
            "<dsp:txXfrm><a:off x=\"{}\" y=\"{}\"/><a:ext cx=\"{}\" cy=\"{}\"/></dsp:txXfrm>",
            place.x, place.y, place.width, place.height,
        ));
        out.push_str("</dsp:sp>");
    }

    out.push_str("</dsp:spTree></dsp:drawing>");
    out
}

/// What the quick style written here is called.
const QUICK_STYLE_ID: &str = "urn:microsoft.com/office/officeart/2005/8/quickstyle/simple1";
/// And the colour list.
const COLOURS_ID: &str = "urn:microsoft.com/office/officeart/2005/8/colors/accent1_2";
/// The style labels a diagram's pieces are drawn by. Every one of them is
/// written into both the style and the colour list, because a piece whose
/// label is missing from either is a piece drawn by whatever Word falls back
/// to rather than by what this file asks for.
const LABELS: &[&str] = &["node0", "node1", "alignNode1", "lnNode1", "sibTrans2D1", "fgAcc1"];

/// The layout: the rules Word follows when the words change.
fn layout_xml(arrangement: Arrangement) -> String {
    let (direction, sizes) = match arrangement {
        // Across the page, each box as wide as the row divided by how many
        // there are, with an arrow in every gap.
        Arrangement::Process => ("fromL", "<dgm:constr type=\"w\" for=\"ch\" ptType=\"node\" op=\"equ\"/>\
             <dgm:constr type=\"h\" for=\"ch\" ptType=\"node\" op=\"equ\"/>\
             <dgm:constr type=\"w\" for=\"ch\" ptType=\"sibTrans\" refType=\"w\" refPtType=\"node\" fact=\"0.3\"/>"),
        // Down the page, each box the whole width.
        Arrangement::List | Arrangement::Hierarchy => ("fromT", "<dgm:constr type=\"w\" for=\"ch\" ptType=\"node\" op=\"equ\"/>\
             <dgm:constr type=\"h\" for=\"ch\" ptType=\"node\" op=\"equ\"/>\
             <dgm:constr type=\"sp\" refType=\"h\" refPtType=\"node\" fact=\"0.3\"/>"),
    };

    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
         <dgm:layoutDef xmlns:dgm=\"{DIAGRAM}\" xmlns:a=\"{main}\" xmlns:r=\"{rel}\" \
         uniqueId=\"{id}\"><dgm:title val=\"\"/><dgm:desc val=\"\"/>\
         <dgm:catLst><dgm:cat type=\"{category}\" pri=\"1000\"/></dgm:catLst>\
         <dgm:layoutNode name=\"diagram\">\
         <dgm:varLst><dgm:dir val=\"norm\"/><dgm:resizeHandles val=\"exact\"/></dgm:varLst>\
         <dgm:alg type=\"lin\"><dgm:param type=\"linDir\" val=\"{direction}\"/></dgm:alg>\
         <dgm:shape type=\"none\" r:blip=\"\"><dgm:adjLst/></dgm:shape>\
         <dgm:presOf/><dgm:constrLst>{sizes}</dgm:constrLst><dgm:ruleLst/>\
         <dgm:forEach name=\"items\" axis=\"ch\" ptType=\"node\">\
         <dgm:layoutNode name=\"item\">\
         <dgm:alg type=\"tx\"/>\
         <dgm:shape type=\"roundRect\" r:blip=\"\"><dgm:adjLst/></dgm:shape>\
         <dgm:presOf axis=\"desOrSelf\" ptType=\"node\"/>\
         <dgm:constrLst><dgm:constr type=\"primFontSz\" val=\"1200\"/>\
         <dgm:constr type=\"lMarg\" refType=\"primFontSz\" fact=\"0.3\"/>\
         <dgm:constr type=\"rMarg\" refType=\"primFontSz\" fact=\"0.3\"/></dgm:constrLst>\
         <dgm:ruleLst><dgm:rule type=\"primFontSz\" val=\"5\" fact=\"NaN\" max=\"NaN\"/></dgm:ruleLst>\
         </dgm:layoutNode>\
         <dgm:forEach name=\"between\" axis=\"followSib\" ptType=\"sibTrans\" cnt=\"1\">\
         <dgm:layoutNode name=\"arrow\">\
         <dgm:alg type=\"sp\"/>\
         <dgm:shape type=\"rightArrow\" r:blip=\"\"><dgm:adjLst/></dgm:shape>\
         <dgm:presOf axis=\"self\"/><dgm:constrLst/><dgm:ruleLst/>\
         </dgm:layoutNode></dgm:forEach>\
         </dgm:forEach></dgm:layoutNode></dgm:layoutDef>",
        main = crate::edit::DRAWING_MAIN,
        rel = crate::edit::RELATIONSHIPS,
        id = arrangement.layout_id(),
        category = arrangement.category(),
    )
}

/// The quick style: what the pieces are made of, said as references into the
/// theme rather than as colours.
fn quick_style_xml() -> String {
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
         <dgm:styleDef xmlns:dgm=\"{DIAGRAM}\" xmlns:a=\"{main}\" uniqueId=\"{QUICK_STYLE_ID}\">\
         <dgm:title val=\"\"/><dgm:desc val=\"\"/>\
         <dgm:catLst><dgm:cat type=\"simple\" pri=\"10100\"/></dgm:catLst>\
         <dgm:scene3d><a:camera prst=\"orthographicFront\"/>\
         <a:lightRig rig=\"threePt\" dir=\"t\"/></dgm:scene3d>",
        main = crate::edit::DRAWING_MAIN,
    );
    for label in LABELS {
        out.push_str(&format!(
            "<dgm:styleLbl name=\"{label}\"><dgm:scene3d><a:camera prst=\"orthographicFront\"/>\
             <a:lightRig rig=\"threePt\" dir=\"t\"/></dgm:scene3d><dgm:sp3d/><dgm:txPr/>\
             <dgm:style><a:lnRef idx=\"0\"><a:scrgbClr r=\"0\" g=\"0\" b=\"0\"/></a:lnRef>\
             <a:fillRef idx=\"1\"><a:scrgbClr r=\"0\" g=\"0\" b=\"0\"/></a:fillRef>\
             <a:effectRef idx=\"0\"><a:scrgbClr r=\"0\" g=\"0\" b=\"0\"/></a:effectRef>\
             <a:fontRef idx=\"minor\"><a:schemeClr val=\"lt1\"/></a:fontRef></dgm:style>\
             </dgm:styleLbl>"
        ));
    }
    out.push_str("</dgm:styleDef>");
    out
}

/// The colour list: which of the theme's colours each piece is drawn in.
fn colours_xml() -> String {
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
         <dgm:colorsDef xmlns:dgm=\"{DIAGRAM}\" xmlns:a=\"{main}\" uniqueId=\"{COLOURS_ID}\">\
         <dgm:title val=\"\"/><dgm:desc val=\"\"/>\
         <dgm:catLst><dgm:cat type=\"accent1\" pri=\"11002\"/></dgm:catLst>",
        main = crate::edit::DRAWING_MAIN,
    );
    for label in LABELS {
        out.push_str(&format!(
            "<dgm:styleLbl name=\"{label}\">\
             <dgm:fillClrLst meth=\"repeat\"><a:schemeClr val=\"accent1\"/></dgm:fillClrLst>\
             <dgm:linClrLst meth=\"repeat\"><a:schemeClr val=\"accent1\"/></dgm:linClrLst>\
             <dgm:effectClrLst/><dgm:txLinClrLst/>\
             <dgm:txFillClrLst meth=\"repeat\"><a:schemeClr val=\"lt1\"/></dgm:txFillClrLst>\
             <dgm:txEffectClrLst/></dgm:styleLbl>"
        ));
    }
    out.push_str("</dgm:colorsDef>");
    out
}

/// The frame that stands in the text: four relationships and a size.
#[must_use]
fn frame_element(ids: &Ids, width_emu: i64, height_emu: i64, prefix: Option<&str>) -> Element {
    let mut drawing =
        Element::new(&crate::edit::name_with(prefix, "drawing"), Some(crate::read::W));

    let mut inline = Element::new("wp:inline", Some(crate::edit::DRAWING_WORDPROCESSING));
    inline
        .declarations
        .push((Some("wp".to_owned()), crate::edit::DRAWING_WORDPROCESSING.to_owned()));
    for side in ["distT", "distB", "distL", "distR"] {
        inline.set_attribute(side, "0");
    }

    let mut extent = Element::new("wp:extent", Some(crate::edit::DRAWING_WORDPROCESSING));
    extent.set_attribute("cx", &width_emu.to_string());
    extent.set_attribute("cy", &height_emu.to_string());
    inline.push_element(extent);

    let mut properties = Element::new("wp:docPr", Some(crate::edit::DRAWING_WORDPROCESSING));
    properties.set_attribute("id", "1");
    properties.set_attribute("name", "Diagram 1");
    inline.push_element(properties);
    inline.push_element(Element::new(
        "wp:cNvGraphicFramePr",
        Some(crate::edit::DRAWING_WORDPROCESSING),
    ));

    let mut graphic = Element::new("a:graphic", Some(crate::edit::DRAWING_MAIN));
    graphic.declarations.push((Some("a".to_owned()), crate::edit::DRAWING_MAIN.to_owned()));
    let mut data = Element::new("a:graphicData", Some(crate::edit::DRAWING_MAIN));
    data.set_attribute("uri", DIAGRAM_URI);

    let mut relations = Element::new("dgm:relIds", Some(DIAGRAM));
    relations.declarations.push((Some("dgm".to_owned()), DIAGRAM.to_owned()));
    relations.declarations.push((Some("r".to_owned()), crate::edit::RELATIONSHIPS.to_owned()));
    relations.set_namespaced_attribute("r:dm", crate::edit::RELATIONSHIPS, &ids.data);
    relations.set_namespaced_attribute("r:lo", crate::edit::RELATIONSHIPS, &ids.layout);
    relations.set_namespaced_attribute("r:qs", crate::edit::RELATIONSHIPS, &ids.style);
    relations.set_namespaced_attribute("r:cs", crate::edit::RELATIONSHIPS, &ids.colours);
    data.push_element(relations);

    graphic.push_element(data);
    inline.push_element(graphic);
    drawing.push_element(inline);
    drawing
}

/// Reads the data model: the words, which layout it asks for, and where the
/// drawing is.
fn read_data_model(root: &Element) -> (Vec<Node>, Option<Arrangement>, Option<String>) {
    let mut arrangement = None;
    let mut root_id = None;
    let mut words: Vec<(String, String)> = Vec::new();

    if let Some(points) = root.child(Some(DIAGRAM), "ptLst") {
        for point in points.children_named(Some(DIAGRAM), "pt") {
            let Some(id) = point.attribute(None, "modelId") else { continue };
            match point.attribute(None, "type") {
                // The document point holds no words; what it holds is which
                // layout the diagram asked for.
                Some("doc") => {
                    root_id = Some(id.to_owned());
                    arrangement = point
                        .child(Some(DIAGRAM), "prSet")
                        .and_then(|set| set.attribute(None, "loTypeId"))
                        .and_then(Arrangement::from_layout_id);
                }
                // The transitions and whatever the layout engine left behind
                // are not words anybody typed.
                Some("parTrans" | "sibTrans" | "pres") => {}
                _ => words.push((id.to_owned(), point_text(point))),
            }
        }
    }

    // Who hangs under whom, in the order the file gives.
    let mut links: Vec<(String, String, i32)> = Vec::new();
    if let Some(connections) = root.child(Some(DIAGRAM), "cxnLst") {
        for connection in connections.children_named(Some(DIAGRAM), "cxn") {
            if !matches!(connection.attribute(None, "type"), None | Some("parOf")) {
                continue;
            }
            let (Some(source), Some(destination)) =
                (connection.attribute(None, "srcId"), connection.attribute(None, "destId"))
            else {
                continue;
            };
            let order = connection
                .attribute(None, "srcOrd")
                .and_then(|value| value.parse::<i32>().ok())
                .unwrap_or(0);
            links.push((source.to_owned(), destination.to_owned(), order));
        }
    }

    let nodes = match root_id {
        Some(root_id) => children_of(&root_id, &words, &links, 0),
        // A model with no document point still has words, and a row of them is
        // the closest true reading of a file that says nothing about depth.
        None => words.iter().map(|(_, text)| Node::new(text)).collect(),
    };

    let drawing = find_local(root, "dataModelExt")
        .and_then(|extension| extension.attribute(None, "relId"))
        .map(str::to_owned);
    (nodes, arrangement, drawing)
}

/// Everything hanging under one point, in the order the connections give.
///
/// The depth is counted so that a model that points at itself — which a file
/// may say and a reader must survive — stops rather than going round for ever.
fn children_of(
    parent: &str,
    words: &[(String, String)],
    links: &[(String, String, i32)],
    depth: usize,
) -> Vec<Node> {
    if depth > 16 {
        return Vec::new();
    }
    let mut under: Vec<&(String, String, i32)> =
        links.iter().filter(|(source, _, _)| source == parent).collect();
    under.sort_by_key(|(_, _, order)| *order);

    under
        .into_iter()
        .filter_map(|(_, destination, _)| {
            let text = words.iter().find(|(id, _)| id == destination)?;
            Some(Node {
                text: text.1.clone(),
                children: children_of(destination, words, links, depth + 1),
            })
        })
        .collect()
}

/// The words of one point, with a line for each paragraph in it.
fn point_text(point: &Element) -> String {
    let Some(body) = point.child(Some(DIAGRAM), "t") else { return String::new() };
    paragraph_lines(body).join("\n")
}

/// Every paragraph of a DrawingML text body, as plain words.
fn paragraph_lines(body: &Element) -> Vec<String> {
    body.child_elements()
        .filter(|child| child.local_name() == "p")
        .map(|paragraph| {
            let mut line = String::new();
            gather_text(paragraph, &mut line);
            line
        })
        .collect()
}

/// The text of every `a:t` under an element, in order.
fn gather_text(element: &Element, out: &mut String) {
    for child in element.child_elements() {
        if child.local_name() == "t" {
            out.push_str(&child.text_content());
        } else {
            gather_text(child, out);
        }
    }
}

/// The first element anywhere under this one with a given local name.
fn find_local<'a>(element: &'a Element, local: &str) -> Option<&'a Element> {
    for child in element.child_elements() {
        if child.local_name() == local {
            return Some(child);
        }
        if let Some(found) = find_local(child, local) {
            return Some(found);
        }
    }
    None
}

/// Reads the drawing: the shapes the diagram was last laid out into.
fn read_drawing(root: &Element, theme: &Theme) -> Option<Group> {
    let tree = find_local(root, "spTree")?;

    let mut members = Vec::new();
    for shape in tree.child_elements().filter(|child| child.local_name() == "sp") {
        let Some(member) = read_drawn_shape(shape, theme) else { continue };
        members.push(member);
    }
    if members.is_empty() {
        return None;
    }

    // What the shapes are measured in: the rectangle they cover. The drawing
    // states no rectangle of its own, and taking the shapes' own is the only
    // reading under which a diagram fills the frame it was given.
    let width = members.iter().map(|member| member.x_emu + member.width_emu).max().unwrap_or(1);
    let height = members.iter().map(|member| member.y_emu + member.height_emu).max().unwrap_or(1);

    Some(Group {
        name: "Diagram".to_owned(),
        width_emu: width,
        height_emu: height,
        child_width: width.max(1),
        child_height: height.max(1),
        members,
        ..Group::default()
    })
}

/// One shape of a drawing, as a member of the group it becomes.
fn read_drawn_shape(element: &Element, theme: &Theme) -> Option<Member> {
    let properties = element.child_elements().find(|child| child.local_name() == "spPr")?;
    let transform = properties.child_elements().find(|child| child.local_name() == "xfrm")?;
    let offset = transform.child_elements().find(|child| child.local_name() == "off")?;
    let extent = transform.child_elements().find(|child| child.local_name() == "ext")?;

    let number = |element: &Element, name: &str| -> i64 {
        element.attribute(None, name).and_then(|value| value.trim().parse().ok()).unwrap_or(0)
    };
    let (x, y) = (number(offset, "x"), number(offset, "y"));
    let (width, height) = (number(extent, "cx"), number(extent, "cy"));
    if width <= 0 || height <= 0 {
        return None;
    }

    let mut shape = Shape {
        width_emu: width,
        height_emu: height,
        flipped_across: matches!(transform.attribute(None, "flipH"), Some("1" | "true")),
        flipped_down: matches!(transform.attribute(None, "flipV"), Some("1" | "true")),
        rotation: transform
            .attribute(None, "rot")
            .and_then(|value| value.parse().ok())
            .unwrap_or(0),
        outline: None,
        outline_emu: 0,
        fill: crate::fills::Fill::None,
        effects: crate::shapeeffects::read_effects(properties),
        ..Shape::default()
    };

    if let Some(geometry) =
        properties.child_elements().find(|child| child.local_name() == "prstGeom")
    {
        if let Some(name) = geometry.attribute(None, "prst") {
            shape.preset = name.to_owned();
        }
    }

    // The colours a diagram is drawn in are the theme's, named rather than
    // stated: a drawing read as though it said nothing about its colours is a
    // diagram drawn as a row of outlines.
    if let Some(solid) = properties.child_elements().find(|child| child.local_name() == "solidFill")
    {
        if let Some(colour) = colour_of(solid, theme) {
            shape.fill = crate::fills::Fill::Solid(colour);
        }
    }
    if let Some(line) = properties.child_elements().find(|child| child.local_name() == "ln") {
        let has_colour = line.child_elements().any(|child| child.local_name() == "solidFill");
        if has_colour {
            shape.outline = colour_of(line, theme);
            shape.outline_emu = line
                .attribute(None, "w")
                .and_then(|value| value.parse().ok())
                .unwrap_or(crate::shapes::EMU_PER_POINT);
        }
    }

    if let Some(body) = element.child_elements().find(|child| child.local_name() == "txBody") {
        shape.text = read_drawn_text(body, theme);
        shape.description = shape
            .text
            .iter()
            .map(crate::model::Paragraph::plain_text)
            .collect::<Vec<_>>()
            .join(" ")
            .trim()
            .to_owned();
        shape.name = shape.description.clone();
    }

    Some(Member {
        x_emu: x,
        y_emu: y,
        width_emu: width,
        height_emu: height,
        what: Inside::Shape(Box::new(shape)),
    })
}

/// The words in a drawn shape, as paragraphs this program can lay out.
fn read_drawn_text(body: &Element, theme: &Theme) -> Vec<Paragraph> {
    let mut out = Vec::new();
    for paragraph in body.child_elements().filter(|child| child.local_name() == "p") {
        let alignment = paragraph
            .child_elements()
            .find(|child| child.local_name() == "pPr")
            .and_then(|properties| properties.attribute(None, "algn"))
            .map(|value| match value {
                "ctr" => Alignment::Center,
                "r" => Alignment::End,
                "just" => Alignment::Both,
                _ => Alignment::Start,
            });

        let mut runs = Vec::new();
        for run in paragraph.child_elements().filter(|child| child.local_name() == "r") {
            let mut text = String::new();
            gather_text(run, &mut text);
            if text.is_empty() {
                continue;
            }
            let properties = run.child_elements().find(|child| child.local_name() == "rPr");
            // DrawingML states a size in hundredths of a point and this
            // program keeps half-points, which is the unit the text side of
            // the format uses.
            let size = properties
                .and_then(|properties| properties.attribute(None, "sz"))
                .and_then(|value| value.trim().parse::<u32>().ok())
                .map(|hundredths| (hundredths / 50).max(1));
            let colour = properties.and_then(|properties| {
                properties
                    .child_elements()
                    .find(|child| child.local_name() == "solidFill")
                    .and_then(|fill| colour_of(fill, theme))
            });
            runs.push(Run {
                properties: RunProperties {
                    bold: properties
                        .and_then(|properties| properties.attribute(None, "b"))
                        .map(|value| matches!(value, "1" | "true")),
                    italic: properties
                        .and_then(|properties| properties.attribute(None, "i"))
                        .map(|value| matches!(value, "1" | "true")),
                    size_half_points: size,
                    color: colour,
                    ..RunProperties::default()
                },
                content: vec![RunContent::Text(text)],
                field: None,
                revision: None,
                format_change: None,
            });
        }
        if runs.is_empty() {
            continue;
        }
        out.push(Paragraph {
            properties: ParagraphProperties {
                alignment,
                space_after: Some(0),
                ..ParagraphProperties::default()
            },
            runs,
        });
    }
    out
}

/// The colour an element names, whether it states one or names the theme's.
///
/// Both ways carry the same shifts — lighter, darker, a different shade of the
/// same hue — and they are applied here rather than dropped, because the shift
/// is how one diagram is drawn in six colours from one accent.
fn colour_of(parent: &Element, theme: &Theme) -> Option<String> {
    fn search(element: &Element) -> Option<&Element> {
        if matches!(element.local_name(), "srgbClr" | "schemeClr") {
            return Some(element);
        }
        element.child_elements().find_map(search)
    }

    let named = search(parent)?;
    let base = if named.local_name() == "srgbClr" {
        named.attribute(None, "val")?.to_uppercase()
    } else {
        theme.color(drawing_slot(named.attribute(None, "val")?)?)
    };
    Some(shifted(&base, named))
}

/// The slot one of DrawingML's own colour names stands for.
///
/// Two vocabularies again: the drawing side says dk1 and lt1 where the text
/// side says text1 and background1, and bg1 and tx1 where a theme means the
/// same two the other way about.
fn drawing_slot(name: &str) -> Option<Slot> {
    Some(match name {
        "dk1" | "tx1" => Slot::Dark1,
        "lt1" | "bg1" => Slot::Light1,
        "dk2" | "tx2" => Slot::Dark2,
        "lt2" | "bg2" => Slot::Light2,
        "accent1" => Slot::Accent1,
        "accent2" => Slot::Accent2,
        "accent3" => Slot::Accent3,
        "accent4" => Slot::Accent4,
        "accent5" => Slot::Accent5,
        "accent6" => Slot::Accent6,
        "hlink" => Slot::Hyperlink,
        "folHlink" => Slot::FollowedHyperlink,
        _ => return None,
    })
}

/// A colour with the shifts written under it applied.
///
/// The shifts are stated in thousandths of a percent, and the hue in
/// sixty-thousandths of a degree. Shade and tint are worked on the components
/// as they are written rather than in light as it is measured, which is what
/// the format asks for and is a shade off what a photometer would say.
fn shifted(base: &str, named: &Element) -> String {
    let digits =
        |at: usize| u8::from_str_radix(base.get(at..at + 2).unwrap_or("00"), 16).unwrap_or(0);
    let (mut red, mut green, mut blue) = (digits(0), digits(2), digits(4));

    let value = |element: &Element| -> f32 {
        element
            .attribute(None, "val")
            .and_then(|value| value.trim().parse::<f32>().ok())
            .unwrap_or(0.0)
            / 100_000.0
    };

    for shift in named.child_elements() {
        match shift.local_name() {
            "shade" => {
                let by = value(shift).clamp(0.0, 1.0);
                let darker = |component: u8| (f32::from(component) * by).round() as u8;
                (red, green, blue) = (darker(red), darker(green), darker(blue));
            }
            "tint" => {
                let by = value(shift).clamp(0.0, 1.0);
                let lighter =
                    |component: u8| (f32::from(component) * by + 255.0 * (1.0 - by)).round() as u8;
                (red, green, blue) = (lighter(red), lighter(green), lighter(blue));
            }
            // The rest are said in hue, saturation and lightness, so the
            // colour goes round into those and back again.
            "lumMod" | "lumOff" | "satMod" | "satOff" | "hueOff" | "hueMod" => {
                let (mut hue, mut saturation, mut lightness) = to_hsl(red, green, blue);
                match shift.local_name() {
                    "lumMod" => lightness *= value(shift),
                    "lumOff" => lightness += value(shift),
                    "satMod" => saturation *= value(shift),
                    "satOff" => saturation += value(shift),
                    // Sixty thousandths of a degree, and the hundred thousand
                    // above has already divided it by a hundred thousand.
                    "hueOff" => hue += value(shift) * 100_000.0 / 60_000.0,
                    _ => hue *= value(shift),
                }
                let (r, g, b) = from_hsl(
                    hue.rem_euclid(360.0),
                    saturation.clamp(0.0, 1.0),
                    lightness.clamp(0.0, 1.0),
                );
                (red, green, blue) = (r, g, b);
            }
            _ => {}
        }
    }
    format!("{red:02X}{green:02X}{blue:02X}")
}

/// Hue in degrees, saturation and lightness as fractions.
fn to_hsl(red: u8, green: u8, blue: u8) -> (f32, f32, f32) {
    let (red, green, blue) =
        (f32::from(red) / 255.0, f32::from(green) / 255.0, f32::from(blue) / 255.0);
    let largest = red.max(green).max(blue);
    let smallest = red.min(green).min(blue);
    let lightness = (largest + smallest) / 2.0;
    let span = largest - smallest;
    if span <= f32::EPSILON {
        return (0.0, 0.0, lightness);
    }

    let saturation = span / (1.0 - (2.0 * lightness - 1.0).abs());
    let hue = if (largest - red).abs() < f32::EPSILON {
        60.0 * (((green - blue) / span) % 6.0)
    } else if (largest - green).abs() < f32::EPSILON {
        60.0 * ((blue - red) / span + 2.0)
    } else {
        60.0 * ((red - green) / span + 4.0)
    };
    (hue.rem_euclid(360.0), saturation, lightness)
}

/// And back.
fn from_hsl(hue: f32, saturation: f32, lightness: f32) -> (u8, u8, u8) {
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let second = chroma * (1.0 - ((hue / 60.0) % 2.0 - 1.0).abs());
    let (red, green, blue) = match hue {
        hue if hue < 60.0 => (chroma, second, 0.0),
        hue if hue < 120.0 => (second, chroma, 0.0),
        hue if hue < 180.0 => (0.0, chroma, second),
        hue if hue < 240.0 => (0.0, second, chroma),
        hue if hue < 300.0 => (second, 0.0, chroma),
        _ => (chroma, 0.0, second),
    };
    let lift = lightness - chroma / 2.0;
    let byte = |value: f32| ((value + lift) * 255.0).round().clamp(0.0, 255.0) as u8;
    (byte(red), byte(green), byte(blue))
}

/// The picture a diagram makes when the file carries no drawing of it.
fn drawn_from_words(
    arrangement: Arrangement,
    nodes: &[Node],
    width: i64,
    fill: &str,
    paper: &str,
) -> Group {
    let words: Vec<String> = nodes.iter().flat_map(once_and_children).collect();
    let borrowed: Vec<&str> = words.iter().map(String::as_str).collect();
    group_of(&places(arrangement, &borrowed, width), fill, paper)
}

/// Several pieces as one drawing.
fn group_of(places: &[Place], fill: &str, paper: &str) -> Group {
    let mut members = Vec::new();
    for place in places {
        let mut shape = Shape {
            preset: place.preset.to_owned(),
            width_emu: place.width,
            height_emu: place.height,
            fill: crate::fills::Fill::Solid(fill.to_owned()),
            outline: None,
            outline_emu: 0,
            name: place.text.clone(),
            description: place.text.clone(),
            ..Shape::default()
        };
        if !place.text.is_empty() {
            shape.text = vec![caption(&place.text, paper)];
        }
        members.push(Member {
            x_emu: place.x,
            y_emu: place.y,
            width_emu: place.width,
            height_emu: place.height,
            what: Inside::Shape(Box::new(shape)),
        });
    }

    let width = members.iter().map(|member| member.x_emu + member.width_emu).max().unwrap_or(1);
    let height = members.iter().map(|member| member.y_emu + member.height_emu).max().unwrap_or(1);
    Group {
        name: "Diagram".to_owned(),
        width_emu: width,
        height_emu: height,
        child_width: width.max(1),
        child_height: height.max(1),
        members,
        ..Group::default()
    }
}

/// The words inside a box: centred, on the paper's colour, and small enough to
/// fit.
fn caption(text: &str, colour: &str) -> Paragraph {
    Paragraph {
        properties: ParagraphProperties {
            alignment: Some(Alignment::Center),
            space_after: Some(0),
            ..ParagraphProperties::default()
        },
        runs: vec![Run {
            properties: RunProperties {
                color: Some(colour.to_owned()),
                size_half_points: Some(24),
                bold: Some(true),
                ..RunProperties::default()
            },
            content: vec![RunContent::Text(text.to_owned())],
            field: None,
            revision: None,
            format_change: None,
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOM: i64 = EMU_PER_INCH * 6;

    fn items(count: usize) -> Vec<String> {
        (0..count).map(|index| format!("Step {index}")).collect()
    }

    fn borrowed(items: &[String]) -> Vec<&str> {
        items.iter().map(String::as_str).collect()
    }

    fn parsed(xml: &str) -> Element {
        wp_xml::tree::XmlTree::parse(xml).expect("the part should parse").root
    }

    #[test]
    fn every_arrangement_says_what_it_is_called_and_what_the_gallery_calls_it() {
        for arrangement in Arrangement::ALL {
            assert!(!arrangement.label().is_empty());
            assert_eq!(Arrangement::from_layout_id(arrangement.layout_id()), Some(*arrangement));
        }
        assert_eq!(Arrangement::from_layout_id("urn:something/else"), None);
    }

    #[test]
    fn a_process_has_a_box_for_every_step_and_an_arrow_between_each_pair() {
        let items = items(4);
        let places = process(&borrowed(&items), ROOM);
        assert_eq!(places.iter().filter(|place| place.kind == Piece::Box).count(), 4);
        assert_eq!(places.iter().filter(|place| place.kind == Piece::Arrow).count(), 3);
    }

    #[test]
    fn one_step_needs_no_arrow() {
        let items = items(1);
        assert_eq!(process(&borrowed(&items), ROOM).len(), 1);
    }

    #[test]
    fn a_process_stays_inside_the_room_it_was_given() {
        let items = items(4);
        let places = process(&borrowed(&items), ROOM);
        let right = places.iter().map(|place| place.x + place.width).max().expect("some pieces");
        assert!(right <= ROOM, "the diagram is {right} wide in {ROOM} of room");
    }

    #[test]
    fn a_list_puts_each_box_under_the_last() {
        let items = items(3);
        let places = list(&borrowed(&items), ROOM);
        assert_eq!(places.len(), 3);
        let tops: Vec<i64> = places.iter().map(|place| place.y).collect();
        assert!(tops[0] < tops[1] && tops[1] < tops[2], "{tops:?}");
        assert!(places.iter().all(|place| place.width == ROOM));
    }

    #[test]
    fn a_hierarchy_puts_the_first_box_above_the_others_and_draws_a_line_to_each() {
        let items = items(4);
        let places = hierarchy(&borrowed(&items), ROOM);
        let boxes: Vec<&Place> = places.iter().filter(|place| place.kind == Piece::Box).collect();
        assert_eq!(boxes.len(), 4);
        assert!(boxes[1..].iter().all(|place| place.y > boxes[0].y), "the row is not below");

        // The line down to each box: one stem out of the box above, and a run
        // across and a drop into each of the three under it.
        let joins: Vec<&Place> = places.iter().filter(|place| place.kind == Piece::Join).collect();
        assert_eq!(joins.len(), 1 + 3 * 2);
        // Every piece of it is inside the gap between the two rows, which is
        // where a line that joins them can be.
        let row_top = boxes[1].y;
        assert!(
            joins.iter().all(|join| join.y >= BOX_HEIGHT && join.y + join.height <= row_top),
            "a line is drawn outside the gap it crosses"
        );
        // And it reaches from the middle of the box above to the middle of the
        // top of each box below.
        let middle = boxes[0].x + boxes[0].width / 2;
        assert!(joins.iter().any(|join| join.x <= middle && middle <= join.x + join.width));
        for boxed in &boxes[1..] {
            let under = boxed.x + boxed.width / 2;
            assert!(
                joins.iter().any(|join| {
                    join.x <= under
                        && under <= join.x + join.width
                        && join.y + join.height == row_top
                }),
                "nothing comes down into the box at {}",
                boxed.x
            );
        }
    }

    #[test]
    fn a_hierarchy_of_one_is_the_one_box() {
        let items = items(1);
        assert_eq!(hierarchy(&borrowed(&items), ROOM).len(), 1);
    }

    #[test]
    fn the_data_model_says_one_point_for_every_item_and_hangs_them_where_it_should() {
        let items = items(3);
        let xml = data_model_xml(Arrangement::Process, &borrowed(&items), "rId1");
        let (nodes, arrangement, drawing) = read_data_model(&parsed(&xml));

        assert_eq!(arrangement, Some(Arrangement::Process));
        assert_eq!(drawing.as_deref(), Some("rId1"));
        assert_eq!(nodes.len(), 3, "a process is a row and not a tree");
        assert_eq!(nodes[0].text, "Step 0");
        assert_eq!(nodes[2].text, "Step 2");
        assert!(nodes.iter().all(|node| node.children.is_empty()));
    }

    #[test]
    fn a_hierarchy_hangs_everything_under_the_first_box() {
        let items = items(3);
        let xml = data_model_xml(Arrangement::Hierarchy, &borrowed(&items), "rId1");
        let (nodes, _, _) = read_data_model(&parsed(&xml));

        assert_eq!(nodes.len(), 1, "a hierarchy has one point at its top");
        assert_eq!(nodes[0].text, "Step 0");
        assert_eq!(nodes[0].children.len(), 2);
        assert_eq!(nodes[0].children[1].text, "Step 2");
    }

    #[test]
    fn the_words_of_a_box_come_back_whatever_they_are_made_of() {
        let typed = vec!["Fish & chips".to_owned(), "<angles>".to_owned()];
        let xml = data_model_xml(Arrangement::List, &borrowed(&typed), "rId1");
        let (nodes, _, _) = read_data_model(&parsed(&xml));
        assert_eq!(nodes[0].text, "Fish & chips");
        assert_eq!(nodes[1].text, "<angles>");
    }

    #[test]
    fn the_drawing_holds_a_shape_for_every_piece_with_its_words_in_it() {
        let items = items(3);
        let places = process(&borrowed(&items), ROOM);
        let xml = drawing_xml(&places, "4472C4", "FFFFFF");
        let group = read_drawing(&parsed(&xml), &Theme::default()).expect("a drawing");

        assert_eq!(group.members.len(), places.len());
        let boxes: Vec<&Member> = group
            .members
            .iter()
            .filter(|member| match &member.what {
                Inside::Shape(shape) => shape.preset == "roundRect",
                _ => false,
            })
            .collect();
        assert_eq!(boxes.len(), 3);
        let Inside::Shape(first) = &boxes[0].what else { panic!("a shape") };
        assert_eq!(first.text.len(), 1);
        assert_eq!(first.text[0].plain_text(), "Step 0");
        assert_eq!(first.fill, crate::fills::Fill::Solid("4472C4".to_owned()));
    }

    #[test]
    fn a_drawing_that_names_the_themes_colours_is_drawn_in_them() {
        // What Word writes: the drawing states no colours of its own, it names
        // the theme's. A reader that only understands the ones written in hex
        // draws the diagram as a row of empty outlines.
        let xml = format!(
            "<dsp:drawing xmlns:dsp=\"{DIAGRAM_DRAWING}\" xmlns:a=\"{main}\"><dsp:spTree>\
             <dsp:sp modelId=\"1\"><dsp:spPr>\
             <a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"100\" cy=\"50\"/></a:xfrm>\
             <a:prstGeom prst=\"roundRect\"/>\
             <a:solidFill><a:schemeClr val=\"accent2\"/></a:solidFill>\
             </dsp:spPr></dsp:sp></dsp:spTree></dsp:drawing>",
            main = crate::edit::DRAWING_MAIN,
        );
        let theme = Theme::default();
        let group = read_drawing(&parsed(&xml), &theme).expect("a drawing");
        let Inside::Shape(shape) = &group.members[0].what else { panic!("a shape") };
        assert_eq!(shape.fill, crate::fills::Fill::Solid(theme.color(Slot::Accent2)));
    }

    #[test]
    fn a_colour_the_file_shifts_is_shifted() {
        // A lighter accent is how one colour list draws six boxes in six
        // colours, and it is stated as a shift and not as a colour.
        let lighter = shifted("4472C4", &parsed("<a:schemeClr xmlns:a=\"x\"><a:lumMod val=\"60000\"/><a:lumOff val=\"40000\"/></a:schemeClr>"));
        assert_ne!(lighter, "4472C4");

        let same = shifted("4472C4", &parsed("<a:schemeClr xmlns:a=\"x\"/>"));
        assert_eq!(same, "4472C4", "a colour with nothing said under it is itself");

        let black =
            shifted("FFFFFF", &parsed("<a:srgbClr xmlns:a=\"x\"><a:shade val=\"0\"/></a:srgbClr>"));
        assert_eq!(black, "000000");
    }

    #[test]
    fn a_colour_goes_round_into_hue_and_lightness_and_comes_back_itself() {
        for colour in [(0x44, 0x72, 0xC4), (255, 0, 0), (0, 0, 0), (255, 255, 255), (12, 200, 90)] {
            let (hue, saturation, lightness) = to_hsl(colour.0, colour.1, colour.2);
            let back = from_hsl(hue, saturation, lightness);
            assert_eq!(back, colour, "{colour:?} came back as {back:?}");
        }
    }

    #[test]
    fn every_part_a_diagram_needs_is_well_formed() {
        let items = items(3);
        for arrangement in Arrangement::ALL {
            let places = places(*arrangement, &borrowed(&items), ROOM);
            for part in [
                data_model_xml(*arrangement, &borrowed(&items), "rId1"),
                layout_xml(*arrangement),
                quick_style_xml(),
                colours_xml(),
                drawing_xml(&places, "4472C4", "FFFFFF"),
            ] {
                wp_xml::tree::XmlTree::parse(&part)
                    .unwrap_or_else(|error| panic!("{}: {error}", arrangement.label()));
            }
        }
    }

    #[test]
    fn the_style_and_the_colours_name_the_same_pieces() {
        // A piece whose label one of them is missing is a piece drawn by
        // whatever Word falls back to rather than by what this file asks for.
        let style = quick_style_xml();
        let colours = colours_xml();
        for label in LABELS {
            assert!(style.contains(&format!("name=\"{label}\"")), "the style is missing {label}");
            assert!(
                colours.contains(&format!("name=\"{label}\"")),
                "the colours are missing {label}"
            );
        }
    }
    #[test]
    fn a_data_model_written_the_way_word_writes_one_is_read_the_same() {
        // Names that are numbers written by hand are the easy case. This is
        // the other one: the ids are the long marks Word writes, the points
        // the layout engine left behind are in among the ones somebody typed,
        // and the connections that say which shape drew what are in among the
        // ones that say which box hangs under which.
        let xml = format!(
            "<dgm:dataModel xmlns:dgm=\"{DIAGRAM}\" xmlns:a=\"{main}\"><dgm:ptLst>\
             <dgm:pt modelId=\"{{31CFA9C6-0000-0000-0000-000000000001}}\" type=\"doc\">\
             <dgm:prSet loTypeId=\"urn:microsoft.com/office/officeart/2005/8/layout/hierarchy1\" \
             loCatId=\"hierarchy\"/><dgm:t><a:bodyPr/><a:p><a:endParaRPr lang=\"en-GB\"/></a:p></dgm:t></dgm:pt>\
             <dgm:pt modelId=\"{{31CFA9C6-0000-0000-0000-000000000002}}\">\
             <dgm:t><a:bodyPr/><a:p><a:r><a:t>Head office</a:t></a:r></a:p></dgm:t></dgm:pt>\
             <dgm:pt modelId=\"{{31CFA9C6-0000-0000-0000-000000000003}}\" type=\"parTrans\" \
             cxnId=\"{{31CFA9C6-0000-0000-0000-0000000000A1}}\"><dgm:t><a:bodyPr/><a:p/></dgm:t></dgm:pt>\
             <dgm:pt modelId=\"{{31CFA9C6-0000-0000-0000-000000000004}}\">\
             <dgm:t><a:bodyPr/><a:p><a:r><a:t>North</a:t></a:r><a:r><a:t> &amp; South</a:t></a:r></a:p></dgm:t></dgm:pt>\
             <dgm:pt modelId=\"{{31CFA9C6-0000-0000-0000-000000000005}}\" type=\"pres\">\
             <dgm:prSet presAssocID=\"{{31CFA9C6-0000-0000-0000-000000000002}}\" presName=\"hierRoot\"/>\
             <dgm:t><a:bodyPr/><a:p/></dgm:t></dgm:pt>\
             </dgm:ptLst><dgm:cxnLst>\
             <dgm:cxn modelId=\"{{31CFA9C6-0000-0000-0000-0000000000A0}}\" \
             srcId=\"{{31CFA9C6-0000-0000-0000-000000000001}}\" \
             destId=\"{{31CFA9C6-0000-0000-0000-000000000002}}\" srcOrd=\"0\" destOrd=\"0\"/>\
             <dgm:cxn modelId=\"{{31CFA9C6-0000-0000-0000-0000000000A1}}\" type=\"parOf\" \
             srcId=\"{{31CFA9C6-0000-0000-0000-000000000002}}\" \
             destId=\"{{31CFA9C6-0000-0000-0000-000000000004}}\" srcOrd=\"0\" destOrd=\"0\"/>\
             <dgm:cxn modelId=\"{{31CFA9C6-0000-0000-0000-0000000000A2}}\" type=\"presOf\" \
             srcId=\"{{31CFA9C6-0000-0000-0000-000000000002}}\" \
             destId=\"{{31CFA9C6-0000-0000-0000-000000000005}}\" srcOrd=\"0\" destOrd=\"0\"/>\
             </dgm:cxnLst><dgm:extLst><a:ext uri=\"{{B4F0B5AB}}\">\
             <dsp:dataModelExt xmlns:dsp=\"{DIAGRAM_DRAWING}\" relId=\"rId7\" minVer=\"12.0\"/>\
             </a:ext></dgm:extLst></dgm:dataModel>",
            main = crate::edit::DRAWING_MAIN,
        );

        let (nodes, arrangement, drawing) = read_data_model(&parsed(&xml));
        assert_eq!(arrangement, Some(Arrangement::Hierarchy));
        assert_eq!(drawing.as_deref(), Some("rId7"));
        assert_eq!(nodes.len(), 1, "the point the layout engine left behind is not a box");
        assert_eq!(nodes[0].text, "Head office");
        assert_eq!(nodes[0].children.len(), 1);
        // Two runs of one paragraph are one line, which is what the box says.
        assert_eq!(nodes[0].children[0].text, "North & South");
    }

    #[test]
    fn a_diagram_laid_out_by_a_layout_this_program_does_not_know_still_says_what_it_says() {
        let xml = format!(
            "<dgm:dataModel xmlns:dgm=\"{DIAGRAM}\" xmlns:a=\"{main}\"><dgm:ptLst>\
             <dgm:pt modelId=\"1\" type=\"doc\"><dgm:prSet loTypeId=\"urn:microsoft.com/office/\
             officeart/2005/8/layout/gear1\"/><dgm:t><a:bodyPr/><a:p/></dgm:t></dgm:pt>\
             <dgm:pt modelId=\"2\"><dgm:t><a:bodyPr/><a:p><a:r><a:t>Turn</a:t></a:r></a:p></dgm:t></dgm:pt>\
             </dgm:ptLst><dgm:cxnLst>\
             <dgm:cxn modelId=\"9\" srcId=\"1\" destId=\"2\" srcOrd=\"0\" destOrd=\"0\"/>\
             </dgm:cxnLst></dgm:dataModel>",
            main = crate::edit::DRAWING_MAIN,
        );
        let (nodes, arrangement, drawing) = read_data_model(&parsed(&xml));
        assert_eq!(arrangement, None, "no arrangement here draws gears");
        assert_eq!(drawing, None);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].text, "Turn");
    }

    #[test]
    fn a_model_that_points_at_itself_is_read_and_not_followed_for_ever() {
        let xml = format!(
            "<dgm:dataModel xmlns:dgm=\"{DIAGRAM}\" xmlns:a=\"{main}\"><dgm:ptLst>\
             <dgm:pt modelId=\"1\" type=\"doc\"><dgm:t><a:bodyPr/><a:p/></dgm:t></dgm:pt>\
             <dgm:pt modelId=\"2\"><dgm:t><a:bodyPr/><a:p><a:r><a:t>Round</a:t></a:r></a:p></dgm:t></dgm:pt>\
             </dgm:ptLst><dgm:cxnLst>\
             <dgm:cxn modelId=\"8\" srcId=\"1\" destId=\"2\" srcOrd=\"0\"/>\
             <dgm:cxn modelId=\"9\" srcId=\"2\" destId=\"2\" srcOrd=\"0\"/>\
             </dgm:cxnLst></dgm:dataModel>",
            main = crate::edit::DRAWING_MAIN,
        );
        let (nodes, _, _) = read_data_model(&parsed(&xml));
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].text, "Round");
    }

    #[test]
    fn a_diagram_with_no_drawing_in_the_file_is_drawn_from_what_it_says() {
        // Which is the whole of what a data model carries, and is better than
        // an empty frame where a diagram should be.
        let nodes = vec![Node {
            text: "Head office".to_owned(),
            children: vec![Node::new("North"), Node::new("South")],
        }];
        let drawn = drawn_from_words(Arrangement::Hierarchy, &nodes, ROOM, "4472C4", "FFFFFF");

        let words: Vec<String> = drawn
            .members
            .iter()
            .filter_map(|member| match &member.what {
                Inside::Shape(shape) => Some(shape),
                _ => None,
            })
            .filter(|shape| !shape.text.is_empty())
            .map(|shape| shape.text[0].plain_text())
            .collect();
        assert_eq!(words, vec!["Head office", "North", "South"]);
        assert!(drawn.child_width > 0 && drawn.child_height > 0);
    }
}
