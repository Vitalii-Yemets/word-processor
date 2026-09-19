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
//! That drawing is what a diagram Word laid out is drawn from here, exactly
//! as Word laid it out, because it *is* what Word laid out. A diagram with no
//! drawing — a file another program wrote, or one whose words have changed
//! here since — is laid out by running the layout: see [`language`]. The
//! same run is what puts a box in when a box is added, and what re-lays the
//! picture out when the arrangement is changed, which is what makes SmartArt
//! SmartArt rather than a heap of shapes.
//!
//! # What is written
//!
//! All five, so the diagram Word opens is a diagram and not a heap of boxes:
//! the data model with a point for every item and the connections between
//! them, a layout definition, a quick style, a colour list, and the drawing,
//! so anything that does not run the layout language still shows the picture.
//! The drawing is written again whenever the words change, so it is never
//! stale.
//!
//! The layout definitions written here are this program's own, in the
//! gallery's language and simpler than the gallery's: the arrangements below
//! say what they arrange, and the same arrangement in Word's gallery has
//! proportions and rules these do not. The same definition is what this
//! program lays out by and what Word lays out by when the words change in
//! Word.
//!
//! # Why the sizes are worked out from the room available
//!
//! A diagram wider than the text is a diagram half off the page. The caller
//! passes how much room there is — it is the one that knows the margins — and
//! the boxes are divided out of it.

use wp_xml::tree::Element;

/// The layout language, which turns the words into the picture.
pub(crate) mod language;
/// The quick style and the colour list, which say what the pieces are drawn
/// in.
pub(crate) mod styles;

use crate::group::{Group, Inside, Member};
use crate::model::{Alignment, Paragraph, ParagraphProperties, Run, RunContent, RunProperties};
use crate::shapes::Shape;
use crate::theme::{Slot, Theme};
use crate::{Document, Error, EMU_PER_INCH};

use language::{Definition, Drawn, Model};
use styles::Styling;

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
    /// Boxes round a circle, with arrows going round.
    Cycle,
    /// Levels one on another, the first at the top and narrowest.
    Pyramid,
    /// Boxes in rows, each row as full as the room allows.
    BlockList,
}

impl Arrangement {
    pub const ALL: &'static [Self] =
        &[Self::Process, Self::List, Self::Hierarchy, Self::Cycle, Self::Pyramid, Self::BlockList];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Process => "Basic Process",
            Self::List => "Vertical Box List",
            Self::Hierarchy => "Hierarchy",
            Self::Cycle => "Basic Cycle",
            Self::Pyramid => "Basic Pyramid",
            Self::BlockList => "Basic Block List",
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
            Self::Cycle => "urn:microsoft.com/office/officeart/2005/8/layout/cycle2",
            Self::Pyramid => "urn:microsoft.com/office/officeart/2005/8/layout/pyramid1",
            Self::BlockList => "urn:microsoft.com/office/officeart/2005/8/layout/default",
        }
    }

    /// Which shelf of the gallery it sits on.
    #[must_use]
    pub fn category(self) -> &'static str {
        match self {
            Self::Process => "process",
            Self::List | Self::BlockList => "list",
            Self::Hierarchy => "hierarchy",
            Self::Cycle => "cycle",
            Self::Pyramid => "pyramid",
        }
    }

    /// Which arrangement a layout part names, when it names one of these.
    ///
    /// A diagram laid out by any of the hundred-odd other layouts in the
    /// gallery is not one of these, and says so by answering nothing: it is
    /// drawn from its own drawing, or by running its own layout.
    #[must_use]
    pub fn from_layout_id(id: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|arrangement| arrangement.layout_id() == id)
    }

    /// Whether the boxes hang under one another as a tree, or all under the
    /// document as a row.
    #[must_use]
    pub fn is_tree(self) -> bool {
        self == Self::Hierarchy
    }

    /// How tall a diagram of this arrangement is drawn when it is first put
    /// in, for so many boxes, so many levels deep, in so much room across.
    #[must_use]
    pub fn natural_height(self, count: usize, depth: usize, width: i64) -> i64 {
        let count = count.max(1) as i64;
        match self {
            Self::Process => BOX_HEIGHT,
            Self::List => count * (LIST_HEIGHT + GAP / 2) - GAP / 2,
            Self::Hierarchy => depth.max(1) as i64 * (BOX_HEIGHT + GAP) - GAP,
            Self::Cycle => width * 3 / 4,
            Self::Pyramid => width * 3 / 5,
            Self::BlockList => {
                let rows = (count + 2) / 3;
                rows * (BOX_HEIGHT + GAP / 2)
            }
        }
    }
}

/// Which of the theme's colours a diagram is drawn in: Word's Change Colors.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Colouring {
    /// Every box in the first accent.
    #[default]
    Accent1,
    Accent2,
    Accent3,
    Accent4,
    Accent5,
    Accent6,
    /// The six accents in turn, which is what Word calls Colorful.
    Colorful,
}

impl Colouring {
    pub const ALL: &'static [Self] = &[
        Self::Colorful,
        Self::Accent1,
        Self::Accent2,
        Self::Accent3,
        Self::Accent4,
        Self::Accent5,
        Self::Accent6,
    ];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Colorful => "Colorful - Accent Colors",
            Self::Accent1 => "Colored Fill - Accent 1",
            Self::Accent2 => "Colored Fill - Accent 2",
            Self::Accent3 => "Colored Fill - Accent 3",
            Self::Accent4 => "Colored Fill - Accent 4",
            Self::Accent5 => "Colored Fill - Accent 5",
            Self::Accent6 => "Colored Fill - Accent 6",
        }
    }

    /// The name the gallery gives the colour list.
    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            Self::Colorful => "urn:microsoft.com/office/officeart/2005/8/colors/colorful1",
            Self::Accent1 => "urn:microsoft.com/office/officeart/2005/8/colors/accent1_2",
            Self::Accent2 => "urn:microsoft.com/office/officeart/2005/8/colors/accent2_2",
            Self::Accent3 => "urn:microsoft.com/office/officeart/2005/8/colors/accent3_2",
            Self::Accent4 => "urn:microsoft.com/office/officeart/2005/8/colors/accent4_2",
            Self::Accent5 => "urn:microsoft.com/office/officeart/2005/8/colors/accent5_2",
            Self::Accent6 => "urn:microsoft.com/office/officeart/2005/8/colors/accent6_2",
        }
    }

    #[must_use]
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|colouring| colouring.id() == id)
    }

    /// Which shelf of the gallery it sits on.
    fn category(self) -> &'static str {
        match self {
            Self::Colorful => "colorful",
            Self::Accent1 => "accent1",
            Self::Accent2 => "accent2",
            Self::Accent3 => "accent3",
            Self::Accent4 => "accent4",
            Self::Accent5 => "accent5",
            Self::Accent6 => "accent6",
        }
    }

    /// The theme's names for the colours the boxes take, in turn.
    fn scheme_names(self) -> &'static [&'static str] {
        match self {
            Self::Colorful => &["accent1", "accent2", "accent3", "accent4", "accent5", "accent6"],
            Self::Accent1 => &["accent1"],
            Self::Accent2 => &["accent2"],
            Self::Accent3 => &["accent3"],
            Self::Accent4 => &["accent4"],
            Self::Accent5 => &["accent5"],
            Self::Accent6 => &["accent6"],
        }
    }
}

/// How tall a box is.
const BOX_HEIGHT: i64 = EMU_PER_INCH * 7 / 10;
/// And a box in a stacked list, which holds a line and no more.
const LIST_HEIGHT: i64 = EMU_PER_INCH / 2;
/// The room between one box and the next.
const GAP: i64 = EMU_PER_INCH / 4;

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

    /// How many levels deep the tree under this node goes, counting itself.
    #[must_use]
    pub fn depth(&self) -> usize {
        1 + self.children.iter().map(Node::depth).max().unwrap_or(0)
    }

    /// How many nodes there are, counting itself.
    #[must_use]
    pub fn count(&self) -> usize {
        1 + self.children.iter().map(Node::count).sum::<usize>()
    }
}

/// A diagram as the document keeps it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Diagram {
    /// Which arrangement the layout part names, when it names one this
    /// program knows.
    pub arrangement: Option<Arrangement>,
    /// Which colours the colour list asks for, when it is one of Word's.
    pub colouring: Option<Colouring>,
    /// Whether the diagram reads right to left.
    pub right_to_left: bool,
    /// The words, as a tree.
    pub nodes: Vec<Node>,
    /// What is drawn: the drawing the file carries, or the words laid out by
    /// the layout when it carries none.
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
        // A hierarchy hangs everything after the first under the first; the
        // rest hang everything under the document itself, which is what makes
        // them a row and not a tree.
        let nodes: Vec<Node> = if arrangement.is_tree() {
            vec![Node {
                text: items[0].to_owned(),
                children: items[1..].iter().map(|item| Node::new(item)).collect(),
            }]
        } else {
            items.iter().map(|item| Node::new(item)).collect()
        };
        self.insert_diagram_of(arrangement, &nodes, width_emu)
    }

    /// Puts a diagram of a tree of words at the caret.
    pub fn insert_diagram_of(
        &mut self,
        arrangement: Arrangement,
        nodes: &[Node],
        width_emu: i64,
    ) -> Result<bool, Error> {
        if nodes.is_empty() || width_emu <= 0 {
            return Ok(false);
        }
        let count: usize = nodes.iter().map(Node::count).sum();
        let depth = nodes.iter().map(Node::depth).max().unwrap_or(1);
        let height = arrangement.natural_height(count, depth, width_emu);

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
        let colouring = Colouring::default();
        let colours_tree = parse(&colours_xml(colouring));
        let style_tree = parse(&quick_style_xml());
        let styling = Styling::read(
            colours_tree.as_ref().map(|tree| &tree.root),
            style_tree.as_ref().map(|tree| &tree.root),
            &self.theme(),
        );
        let model_xml = data_model_xml(arrangement, colouring, nodes, "rId1", false);
        let mut drawn = lay_out_parts(&layout_xml(arrangement), &model_xml, width_emu, height);
        // A picture that came to less than the room it was given takes a
        // frame its own size: an arrangement that scales as a whole keeps
        // its shape, and leaves no empty room under itself.
        let used = drawn.iter().map(|piece| piece.y + piece.height).max().unwrap_or(height);
        let height = if used < height - EMU_PER_INCH / 20 {
            let trimmed = used.max(EMU_PER_INCH / 4);
            drawn = lay_out_parts(&layout_xml(arrangement), &model_xml, width_emu, trimmed);
            trimmed
        } else {
            height
        };
        self.add_package_part(&drawing, DRAWING_CONTENT_TYPE, drawing_xml(&drawn, &styling));
        // Beside the data model rather than under the document: the drawing is
        // the data model's own, and a relationship written from anywhere else
        // would point at it from the wrong part.
        let drawn_id =
            self.point_part_at(&data, &format!("drawing{index}.xml"), DRAWING_RELATIONSHIP)?;

        self.add_package_part(
            &data,
            DATA_CONTENT_TYPE,
            data_model_xml(arrangement, colouring, nodes, &drawn_id, false),
        );
        self.add_package_part(&layout, LAYOUT_CONTENT_TYPE, layout_xml(arrangement));
        self.add_package_part(&quick_style, STYLE_CONTENT_TYPE, quick_style_xml());
        self.add_package_part(&colours, COLORS_CONTENT_TYPE, colours_xml(colouring));

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

        let read = read_data_model(&tree.root);
        let theme = self.theme();
        let styling = self.diagram_styling(reference, &theme);
        let drawing = read
            .drawing
            .as_ref()
            .and_then(|id| self.diagram_drawing(&target, id, &theme, &styling))
            .or_else(|| {
                // No drawing in the file: the words, laid out by the layout
                // the file carries — which is what Word would draw.
                let width = reference.width_emu.max(EMU_PER_INCH);
                let height = reference.height_emu.max(EMU_PER_INCH / 2);
                let layout = self.diagram_part_text(&reference.layout)?;
                let definition = Definition::read(&parse(&layout)?.root)?;
                let model = Model::read(&tree.root);
                let drawn = language::lay_out(&definition, &model, width, height);
                (!drawn.is_empty()).then(|| group_of(&drawn, &styling))
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

        Some(Diagram {
            arrangement: read.arrangement,
            colouring: read.colouring,
            right_to_left: read.right_to_left,
            nodes: read.nodes,
            drawing,
        })
    }

    /// The colours and the quick style a diagram's pieces are drawn in.
    fn diagram_styling(
        &self,
        reference: &crate::model::DiagramReference,
        theme: &Theme,
    ) -> Styling {
        let colours = self.diagram_part_text(&reference.colours).and_then(|text| parse(&text));
        let style = self.diagram_part_text(&reference.style).and_then(|text| parse(&text));
        Styling::read(
            colours.as_ref().map(|tree| &tree.root),
            style.as_ref().map(|tree| &tree.root),
            theme,
        )
    }

    /// The text of a part the frame names by relationship.
    fn diagram_part_text(&self, relationship: &str) -> Option<String> {
        if relationship.is_empty() {
            return None;
        }
        let target = self.relationship_target(relationship)?;
        self.package().xml_part(&target).and_then(Result::ok)
    }

    /// Reads the drawing the data model points at.
    fn diagram_drawing(
        &self,
        data_part: &str,
        relationship: &str,
        theme: &Theme,
        styling: &Styling,
    ) -> Option<Group> {
        let relationships = self.package().relationships(data_part).ok()?;
        let target = relationships.by_id(relationship)?.resolved_target(data_part)?.ok()?;
        let text = self.package().xml_part(&target).and_then(Result::ok)?;
        let tree = wp_xml::tree::XmlTree::parse(&text).ok()?;
        read_drawing(&tree.root, theme, styling)
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

    /// The diagram at a position in the text, if a frame stands there.
    #[must_use]
    pub fn diagram_at(&self, at: crate::TextPosition) -> Option<crate::model::DiagramReference> {
        let body = self.body();
        let paragraphs = body.paragraphs();
        let paragraph = paragraphs.get(at.paragraph)?;
        let mut offset = 0usize;
        for run in &paragraph.runs {
            for piece in &run.content {
                let length = match piece {
                    RunContent::Text(text) => text.len(),
                    _ => 1,
                };
                if let RunContent::Diagram(reference) = piece {
                    if at.offset >= offset && at.offset < offset + length {
                        return Some(reference.clone());
                    }
                }
                offset += length;
            }
        }
        None
    }

    /// Gives a diagram new words, and lays it out again.
    ///
    /// The data model is written afresh from the tree, the drawing is laid
    /// out again by the diagram's own layout, and the frame keeps its size.
    /// Nothing typed is nothing to draw, and the diagram is left as it was.
    pub fn set_diagram_nodes(
        &mut self,
        reference: &crate::model::DiagramReference,
        nodes: &[Node],
    ) -> bool {
        if nodes.is_empty() {
            return false;
        }
        self.rewrite_diagram(reference, Some(nodes), None, None, None)
    }

    /// Changes a diagram's arrangement, keeping its words.
    pub fn set_diagram_arrangement(
        &mut self,
        reference: &crate::model::DiagramReference,
        arrangement: Arrangement,
    ) -> bool {
        self.rewrite_diagram(reference, None, Some(arrangement), None, None)
    }

    /// Changes which of the theme's colours a diagram is drawn in.
    pub fn set_diagram_colouring(
        &mut self,
        reference: &crate::model::DiagramReference,
        colouring: Colouring,
    ) -> bool {
        self.rewrite_diagram(reference, None, None, Some(colouring), None)
    }

    /// Turns a diagram round to read right to left, or back.
    pub fn set_diagram_direction(
        &mut self,
        reference: &crate::model::DiagramReference,
        right_to_left: bool,
    ) -> bool {
        self.rewrite_diagram(reference, None, None, None, Some(right_to_left))
    }

    /// Makes a diagram's frame larger or smaller by a factor, and lays the
    /// picture out again to fill it.
    pub fn resize_diagram(
        &mut self,
        reference: &crate::model::DiagramReference,
        factor: f64,
    ) -> bool {
        let Some(at) = self.diagram_position(reference) else { return false };
        let width = ((reference.width_emu as f64) * factor).round() as i64;
        let height = ((reference.height_emu as f64) * factor).round() as i64;
        if width < EMU_PER_INCH / 2 || height < EMU_PER_INCH / 4 {
            return false;
        }
        // The step keeps the frame and the parts as they are, and the
        // frame's size is changed the way any drawing's is — inside a
        // gesture, so that change is not a step of its own.
        let parts = self.diagram_parts(reference);
        let names: Vec<&str> = parts.iter().map(String::as_str).collect();
        self.record_with_parts(at, &names);
        self.begin_gesture();
        let resized = self.set_drawing_size_at(at, width, height);
        self.end_gesture();
        if !resized {
            return false;
        }
        let resized = crate::model::DiagramReference {
            width_emu: width,
            height_emu: height,
            ..reference.clone()
        };
        self.relay_diagram(&resized, None, None, None, None);
        self.mark_modified();
        true
    }

    /// Where a diagram's frame stands in the text.
    fn diagram_position(
        &self,
        reference: &crate::model::DiagramReference,
    ) -> Option<crate::TextPosition> {
        let body = self.body();
        for (index, paragraph) in body.paragraphs().iter().enumerate() {
            let mut offset = 0usize;
            for run in &paragraph.runs {
                for piece in &run.content {
                    if let RunContent::Diagram(found) = piece {
                        if found.relationship == reference.relationship {
                            return Some(crate::TextPosition::new(index, offset));
                        }
                    }
                    offset += match piece {
                        RunContent::Text(text) => text.len(),
                        _ => 1,
                    };
                }
            }
        }
        None
    }

    /// Rewrites what a change asks for, and records the step.
    fn rewrite_diagram(
        &mut self,
        reference: &crate::model::DiagramReference,
        nodes: Option<&[Node]>,
        arrangement: Option<Arrangement>,
        colouring: Option<Colouring>,
        right_to_left: Option<bool>,
    ) -> bool {
        let Some(at) = self.diagram_position(reference) else { return false };
        // The words and the picture live in parts of the package, so the
        // step keeps those parts as they are: undo puts them back.
        let parts = self.diagram_parts(reference);
        let names: Vec<&str> = parts.iter().map(String::as_str).collect();
        self.record_with_parts(at, &names);
        let done = self.relay_diagram(reference, nodes, arrangement, colouring, right_to_left);
        if done {
            self.mark_modified();
        }
        done
    }

    /// The parts a diagram is kept in: the data model, the layout, the
    /// colours, and the drawing hung off the data model.
    fn diagram_parts(&self, reference: &crate::model::DiagramReference) -> Vec<String> {
        let mut parts = Vec::new();
        for relationship in [&reference.relationship, &reference.layout, &reference.colours] {
            if let Some(part) = self.relationship_target(relationship) {
                parts.push(part);
            }
        }
        if let Some(data_part) = self.relationship_target(&reference.relationship) {
            if let Ok(relationships) = self.package().relationships(&data_part) {
                for found in relationships.by_type(DRAWING_RELATIONSHIP) {
                    if let Some(Ok(target)) = found.resolved_target(&data_part) {
                        parts.push(target);
                    }
                }
            }
        }
        parts
    }

    /// Writes a diagram's parts again — the data model, and whichever of the
    /// layout and the colours changed — and lays the drawing out again.
    fn relay_diagram(
        &mut self,
        reference: &crate::model::DiagramReference,
        nodes: Option<&[Node]>,
        arrangement: Option<Arrangement>,
        colouring: Option<Colouring>,
        right_to_left: Option<bool>,
    ) -> bool {
        let Some(data_part) = self.relationship_target(&reference.relationship) else {
            return false;
        };
        let Some(text) = self.package().xml_part(&data_part).and_then(Result::ok) else {
            return false;
        };
        let Some(tree) = parse(&text) else { return false };
        let was = read_data_model(&tree.root);
        let nodes: Vec<Node> = nodes.map(<[Node]>::to_vec).unwrap_or(was.nodes);
        let arrangement = arrangement.or(was.arrangement).unwrap_or_default();
        let colouring = colouring.or(was.colouring).unwrap_or_default();
        let right_to_left = right_to_left.unwrap_or(was.right_to_left);

        // The words, rehung the way the arrangement hangs them: a tree for a
        // hierarchy, a row for the rest — with the words in the same order.
        let nodes = rehang(nodes, arrangement);

        // The layout, written again when the arrangement changed or the one
        // in the file is not one this program wrote; the colours, when they
        // changed.
        let layout_changed = was.arrangement != Some(arrangement);
        if let (Some(layout_part), true) =
            (self.relationship_target(&reference.layout), layout_changed)
        {
            self.add_package_part(&layout_part, LAYOUT_CONTENT_TYPE, layout_xml(arrangement));
        }
        if let (Some(colours_part), true) =
            (self.relationship_target(&reference.colours), was.colouring != Some(colouring))
        {
            self.add_package_part(&colours_part, COLORS_CONTENT_TYPE, colours_xml(colouring));
        }

        // A file with no drawing gets one, hung off the data model where Word
        // hangs it.
        let drawing_id = match &was.drawing {
            Some(id) => id.clone(),
            None => {
                let index = self.free_diagram_index();
                let Ok(id) = self.point_part_at(
                    &data_part,
                    &format!("drawing{index}.xml"),
                    DRAWING_RELATIONSHIP,
                ) else {
                    return false;
                };
                id
            }
        };
        let Some(drawing_part) =
            self.package().relationships(&data_part).ok().and_then(|relationships| {
                relationships.by_id(&drawing_id)?.resolved_target(&data_part)?.ok()
            })
        else {
            return false;
        };

        // The drawing, laid out by the layout the file now holds.
        let model_xml = data_model_xml(arrangement, colouring, &nodes, &drawing_id, right_to_left);
        let Some(layout) = self.diagram_part_text(&reference.layout) else { return false };
        let width = reference.width_emu.max(EMU_PER_INCH);
        let height = reference.height_emu.max(EMU_PER_INCH / 2);
        let drawn = lay_out_parts(&layout, &model_xml, width, height);
        let theme = self.theme();
        let styling = self.diagram_styling(reference, &theme);
        self.add_package_part(&data_part, DATA_CONTENT_TYPE, model_xml);
        self.add_package_part(&drawing_part, DRAWING_CONTENT_TYPE, drawing_xml(&drawn, &styling));
        true
    }

    /// A number no diagram in the package is using.
    fn free_diagram_index(&self) -> usize {
        let mut index = 1usize;
        while self.package().part(&format!("word/diagrams/data{index}.xml")).is_some()
            || self.package().part(&format!("word/diagrams/drawing{index}.xml")).is_some()
        {
            index += 1;
        }
        index
    }
}

/// The words as an arrangement takes them: the tree they are, whatever the
/// arrangement — a process of a tree shows the deeper words as bullets in
/// their boxes, as Word's does — except that a hierarchy asked of a row
/// hangs the rest under the first, which is what a person changing a row
/// of boxes into a hierarchy means.
fn rehang(nodes: Vec<Node>, arrangement: Arrangement) -> Vec<Node> {
    let is_row = nodes.iter().all(|node| node.children.is_empty());
    if !arrangement.is_tree() || !is_row || nodes.len() < 2 {
        return nodes;
    }
    let mut nodes = nodes;
    let rest = nodes.split_off(1);
    nodes[0].children = rest;
    nodes
}

/// Lays a model out by a layout, both as the text of their parts.
fn lay_out_parts(layout: &str, model: &str, width: i64, height: i64) -> Vec<Drawn> {
    let (Some(layout), Some(model)) = (parse(layout), parse(model)) else { return Vec::new() };
    let Some(definition) = Definition::read(&layout.root) else { return Vec::new() };
    let model = Model::read(&model.root);
    language::lay_out(&definition, &model, width, height)
}

fn parse(text: &str) -> Option<wp_xml::tree::XmlTree> {
    wp_xml::tree::XmlTree::parse(text).ok()
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

/// A text body holding one paragraph of one run.
fn text_xml(text: &str) -> String {
    format!(
        "<a:bodyPr/><a:lstStyle/><a:p><a:pPr algn=\"ctr\"/><a:r><a:rPr lang=\"en-US\"/>\
         <a:t>{}</a:t></a:r></a:p>",
        escape(text)
    )
}

/// The point of the data model that stands for the item numbered `index` in
/// the walk of the tree.
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

/// The connection that hangs the item numbered `index` under whatever holds
/// it.
const fn connection_id(index: usize) -> i32 {
    1000 + index as i32
}

/// The data model: a point for every item and the connections between them.
fn data_model_xml(
    arrangement: Arrangement,
    colouring: Colouring,
    nodes: &[Node],
    drawing: &str,
    right_to_left: bool,
) -> String {
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
         <dgm:dataModel xmlns:dgm=\"{DIAGRAM}\" xmlns:a=\"{main}\" xmlns:r=\"{rel}\"><dgm:ptLst>",
        main = crate::edit::DRAWING_MAIN,
        rel = crate::edit::RELATIONSHIPS,
    );

    // The document point, which is the root everything else hangs under and
    // the one place the file says which layout, style and colours it wants,
    // and which way it reads.
    out.push_str(&format!(
        "<dgm:pt modelId=\"1\" type=\"doc\"><dgm:prSet loTypeId=\"{layout}\" loCatId=\"{category}\" \
         qsTypeId=\"{style}\" qsCatId=\"simple\" csTypeId=\"{colours}\" csCatId=\"{colour_category}\" \
         phldr=\"0\"><dgm:presLayoutVars><dgm:dir val=\"{direction}\"/><dgm:resizeHandles val=\"exact\"/>\
         </dgm:presLayoutVars></dgm:prSet><dgm:spPr/><dgm:t>{text}</dgm:t></dgm:pt>",
        layout = arrangement.layout_id(),
        category = arrangement.category(),
        style = QUICK_STYLE_ID,
        colours = colouring.id(),
        colour_category = colouring.category(),
        direction = if right_to_left { "rev" } else { "norm" },
        text = text_xml(""),
    ));

    // Every item in the order the tree walks them, each with its two
    // transitions: the line in from whatever it hangs under, and the arrow on
    // to the next. They carry no words, and they are here because the layout
    // language reaches for them by name.
    let mut items: Vec<(usize, &Node, Option<usize>, usize)> = Vec::new();
    fn walk<'a>(
        node: &'a Node,
        parent: Option<usize>,
        order: usize,
        out: &mut Vec<(usize, &'a Node, Option<usize>, usize)>,
    ) {
        let index = out.len();
        out.push((index, node, parent, order));
        for (order, child) in node.children.iter().enumerate() {
            walk(child, Some(index), order, out);
        }
    }
    for (order, node) in nodes.iter().enumerate() {
        walk(node, None, order, &mut items);
    }
    for (index, node, _, _) in &items {
        out.push_str(&format!(
            "<dgm:pt modelId=\"{}\"><dgm:prSet phldrT=\"[Text]\"/><dgm:spPr/><dgm:t>{}</dgm:t></dgm:pt>",
            node_id(*index),
            text_xml(&node.text),
        ));
        for (id, kind) in [(parent_id(*index), "parTrans"), (sibling_id(*index), "sibTrans")] {
            out.push_str(&format!(
                "<dgm:pt modelId=\"{id}\" type=\"{kind}\" cxnId=\"{}\"><dgm:prSet/><dgm:spPr/>\
                 <dgm:t>{}</dgm:t></dgm:pt>",
                connection_id(*index),
                text_xml(""),
            ));
        }
    }

    out.push_str("</dgm:ptLst><dgm:cxnLst>");
    for (index, _, parent, order) in &items {
        let source = parent.map_or(1, node_id);
        out.push_str(&format!(
            "<dgm:cxn modelId=\"{}\" srcId=\"{source}\" destId=\"{}\" srcOrd=\"{order}\" \
             destOrd=\"0\" parTransId=\"{}\" sibTransId=\"{}\"/>",
            connection_id(*index),
            node_id(*index),
            parent_id(*index),
            sibling_id(*index),
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

/// The drawing: the picture as it stands, for everything that does not run the
/// layout language.
fn drawing_xml(drawn: &[Drawn], styling: &Styling) -> String {
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
         <dsp:drawing xmlns:dsp=\"{DIAGRAM_DRAWING}\" xmlns:a=\"{main}\"><dsp:spTree>\
         <dsp:nvGrpSpPr><dsp:cNvPr id=\"0\" name=\"\"/><dsp:cNvGrpSpPr/></dsp:nvGrpSpPr>\
         <dsp:grpSpPr/>",
        main = crate::edit::DRAWING_MAIN,
    );

    for piece in drawn {
        let fill = styling.fill_of(&piece.style_label, piece.index, piece.count);
        let line = styling.line_of(&piece.style_label, piece.index, piece.count);
        let ink = styling.text_of(&piece.style_label, piece.index, piece.count);
        let rotation = (piece.rotation * 60000.0).round() as i64;
        out.push_str(&format!(
            "<dsp:sp modelId=\"{}\"><dsp:nvSpPr><dsp:cNvPr id=\"0\" name=\"\"/><dsp:cNvSpPr/>\
             </dsp:nvSpPr><dsp:spPr><a:xfrm{}><a:off x=\"{}\" y=\"{}\"/>\
             <a:ext cx=\"{}\" cy=\"{}\"/></a:xfrm><a:prstGeom prst=\"{}\"><a:avLst/></a:prstGeom>",
            escape(&piece.point_id),
            if rotation != 0 { format!(" rot=\"{rotation}\"") } else { String::new() },
            piece.x,
            piece.y,
            piece.width,
            piece.height,
            escape(&piece.preset),
        ));
        match &fill {
            Some(colour) => {
                out.push_str(&format!("<a:solidFill><a:srgbClr val=\"{colour}\"/></a:solidFill>"));
            }
            None => out.push_str("<a:noFill/>"),
        }
        match &line {
            Some(colour) => out.push_str(&format!(
                "<a:ln w=\"{}\"><a:solidFill><a:srgbClr val=\"{colour}\"/></a:solidFill></a:ln>",
                crate::shapes::EMU_PER_POINT
            )),
            None => out.push_str("<a:ln><a:noFill/></a:ln>"),
        }
        out.push_str("</dsp:spPr>");
        // The style, which is what Word reads the fill from when the shape
        // states none; here the shape states it, and the style agrees.
        out.push_str(
            "<dsp:style><a:lnRef idx=\"0\"><a:scrgbClr r=\"0\" g=\"0\" b=\"0\"/></a:lnRef>\
             <a:fillRef idx=\"1\"><a:scrgbClr r=\"0\" g=\"0\" b=\"0\"/></a:fillRef>\
             <a:effectRef idx=\"0\"><a:scrgbClr r=\"0\" g=\"0\" b=\"0\"/></a:effectRef>\
             <a:fontRef idx=\"minor\"><a:schemeClr val=\"lt1\"/></a:fontRef></dsp:style>",
        );
        if !piece.text.is_empty() {
            let size = (piece.font_size * 100.0).round() as u32;
            out.push_str(&format!(
                "<dsp:txBody><a:bodyPr lIns=\"{}\" rIns=\"{}\" anchor=\"ctr\"/><a:lstStyle/>",
                piece.margins.0.max(0),
                piece.margins.1.max(0),
            ));
            for (level, line) in &piece.text {
                let align = match piece.horizontal.as_str() {
                    "l" => "l",
                    "r" => "r",
                    _ => "ctr",
                };
                out.push_str(&format!("<a:p><a:pPr lvl=\"{level}\" algn=\"{align}\">"));
                if *level > 0 {
                    out.push_str("<a:buChar char=\"•\"/>");
                } else {
                    out.push_str("<a:buNone/>");
                }
                out.push_str(&format!(
                    "</a:pPr><a:r><a:rPr lang=\"en-US\" sz=\"{size}\"><a:solidFill><a:srgbClr val=\"{ink}\"/>\
                     </a:solidFill></a:rPr><a:t>{}</a:t></a:r></a:p>",
                    escape(line)
                ));
            }
            out.push_str("</dsp:txBody>");
            let (tx, ty, tw, th) =
                piece.text_rect.unwrap_or((piece.x, piece.y, piece.width, piece.height));
            out.push_str(&format!(
                "<dsp:txXfrm><a:off x=\"{tx}\" y=\"{ty}\"/><a:ext cx=\"{tw}\" cy=\"{th}\"/></dsp:txXfrm>"
            ));
        }
        out.push_str("</dsp:sp>");
    }

    out.push_str("</dsp:spTree></dsp:drawing>");
    out
}

/// What the quick style written here is called.
const QUICK_STYLE_ID: &str = "urn:microsoft.com/office/officeart/2005/8/quickstyle/simple1";
/// The style labels a diagram's pieces are drawn by. Every one of them is
/// written into both the style and the colour list, because a piece whose
/// label is missing from either is a piece drawn by whatever Word falls back
/// to rather than by what this file asks for.
const LABELS: &[&str] = &["node0", "node1", "alignNode1", "lnNode1", "sibTrans2D1", "fgAcc1"];
/// And the label of the line from a box to the one under it.
const LINE_LABEL: &str = "parChTrans1D2";

/// The words in a box, as a layout node: the text, with the font as large
/// as the box allows down to a floor. The box shows the words of its point
/// and of everything hanging under it, as bullets — which is how Word's
/// process and list layouts show a deeper level.
const TEXT_NODE: &str = "<dgm:alg type=\"tx\"/>\
    <dgm:presOf axis=\"desOrSelf\" ptType=\"node\"/>\
    <dgm:constrLst><dgm:constr type=\"primFontSz\" val=\"18\"/>\
    <dgm:constr type=\"lMarg\" refType=\"primFontSz\" fact=\"0.3\"/>\
    <dgm:constr type=\"rMarg\" refType=\"primFontSz\" fact=\"0.3\"/></dgm:constrLst>\
    <dgm:ruleLst><dgm:rule type=\"primFontSz\" val=\"5\" fact=\"NaN\" max=\"NaN\"/></dgm:ruleLst>";

/// The same, showing the point's own words alone: what a box of a hierarchy
/// shows, whose deeper level is boxes of its own.
const OWN_TEXT_NODE: &str = "<dgm:alg type=\"tx\"/>\
    <dgm:presOf axis=\"self\"/>\
    <dgm:constrLst><dgm:constr type=\"primFontSz\" val=\"18\"/>\
    <dgm:constr type=\"lMarg\" refType=\"primFontSz\" fact=\"0.3\"/>\
    <dgm:constr type=\"rMarg\" refType=\"primFontSz\" fact=\"0.3\"/></dgm:constrLst>\
    <dgm:ruleLst><dgm:rule type=\"primFontSz\" val=\"5\" fact=\"NaN\" max=\"NaN\"/></dgm:ruleLst>";

/// The layout: the rules Word follows when the words change, and the rules
/// this program follows to draw the same picture.
fn layout_xml(arrangement: Arrangement) -> String {
    let body = match arrangement {
        // Across the page, each box as wide as the row divided by how many
        // there are, with an arrow in every gap a third as wide as a box.
        Arrangement::Process => format!(
            "<dgm:alg type=\"lin\"><dgm:param type=\"linDir\" val=\"fromL\"/></dgm:alg>\
             <dgm:shape type=\"none\" r:blip=\"\"><dgm:adjLst/></dgm:shape>\
             <dgm:presOf/><dgm:constrLst>\
             <dgm:constr type=\"w\" for=\"ch\" ptType=\"node\" op=\"equ\"/>\
             <dgm:constr type=\"h\" for=\"ch\" ptType=\"node\" refType=\"h\"/>\
             <dgm:constr type=\"w\" for=\"ch\" ptType=\"sibTrans\" refType=\"w\" refFor=\"ch\" refPtType=\"node\" fact=\"0.3\"/>\
             <dgm:constr type=\"h\" for=\"ch\" ptType=\"sibTrans\" refType=\"h\" fact=\"0.33\"/>\
             </dgm:constrLst><dgm:ruleLst/>\
             <dgm:forEach name=\"items\" axis=\"ch\" ptType=\"node\">\
             <dgm:layoutNode name=\"item\" styleLbl=\"node1\">{TEXT_NODE}\
             <dgm:shape type=\"roundRect\" r:blip=\"\"><dgm:adjLst/></dgm:shape>\
             </dgm:layoutNode>\
             <dgm:forEach name=\"between\" axis=\"followSib\" ptType=\"sibTrans\" cnt=\"1\">\
             <dgm:layoutNode name=\"arrow\" styleLbl=\"sibTrans2D1\">\
             <dgm:alg type=\"sp\"/>\
             <dgm:shape type=\"rightArrow\" r:blip=\"\"><dgm:adjLst/></dgm:shape>\
             <dgm:presOf axis=\"self\"/><dgm:constrLst/><dgm:ruleLst/>\
             </dgm:layoutNode></dgm:forEach>\
             </dgm:forEach>"
        ),
        // Down the page, each box the whole width and all of them one height.
        Arrangement::List => format!(
            "<dgm:alg type=\"lin\"><dgm:param type=\"linDir\" val=\"fromT\"/></dgm:alg>\
             <dgm:shape type=\"none\" r:blip=\"\"><dgm:adjLst/></dgm:shape>\
             <dgm:presOf/><dgm:constrLst>\
             <dgm:constr type=\"h\" for=\"ch\" ptType=\"node\" op=\"equ\"/>\
             <dgm:constr type=\"w\" for=\"ch\" ptType=\"node\" refType=\"w\"/>\
             <dgm:constr type=\"sp\" refType=\"h\" fact=\"0.05\"/>\
             </dgm:constrLst><dgm:ruleLst/>\
             <dgm:forEach name=\"items\" axis=\"ch\" ptType=\"node\">\
             <dgm:layoutNode name=\"item\" styleLbl=\"node1\">{TEXT_NODE}\
             <dgm:shape type=\"roundRect\" r:blip=\"\"><dgm:adjLst/></dgm:shape>\
             </dgm:layoutNode></dgm:forEach>"
        ),
        // Each box over the boxes that hang under it, with a line down to
        // each, the whole tree scaled to fit.
        Arrangement::Hierarchy => format!(
            "<dgm:alg type=\"hierChild\"><dgm:param type=\"chAlign\" val=\"ctr\"/></dgm:alg>\
             <dgm:shape type=\"none\" r:blip=\"\"><dgm:adjLst/></dgm:shape>\
             <dgm:presOf/><dgm:constrLst>\
             <dgm:constr type=\"sibSp\" refType=\"w\" fact=\"0.04\"/>\
             </dgm:constrLst><dgm:ruleLst/>\
             <dgm:forEach name=\"roots\" axis=\"ch\" ptType=\"node\">\
             <dgm:layoutNode name=\"hierRoot\">\
             <dgm:alg type=\"hierRoot\"/>\
             <dgm:shape type=\"none\" r:blip=\"\"><dgm:adjLst/></dgm:shape>\
             <dgm:presOf/><dgm:constrLst>\
             <dgm:constr type=\"sp\" refType=\"h\" fact=\"0.12\"/>\
             <dgm:constr type=\"w\" for=\"ch\" forName=\"box\" refType=\"w\" fact=\"0.28\"/>\
             <dgm:constr type=\"h\" for=\"ch\" forName=\"box\" refType=\"w\" refFor=\"ch\" refForName=\"box\" fact=\"0.5\"/>\
             </dgm:constrLst><dgm:ruleLst/>\
             <dgm:layoutNode name=\"box\" styleLbl=\"node1\">{OWN_TEXT_NODE}\
             <dgm:shape type=\"roundRect\" r:blip=\"\"><dgm:adjLst/></dgm:shape>\
             </dgm:layoutNode>\
             <dgm:layoutNode name=\"children\">\
             <dgm:alg type=\"hierChild\"><dgm:param type=\"chAlign\" val=\"ctr\"/></dgm:alg>\
             <dgm:shape type=\"none\" r:blip=\"\"><dgm:adjLst/></dgm:shape>\
             <dgm:presOf/><dgm:constrLst>\
             <dgm:constr type=\"sibSp\" refType=\"w\" fact=\"0.04\"/>\
             </dgm:constrLst><dgm:ruleLst/>\
             <dgm:forEach name=\"lines\" axis=\"ch\" ptType=\"parTrans\">\
             <dgm:layoutNode name=\"line\" styleLbl=\"{LINE_LABEL}\">\
             <dgm:alg type=\"conn\"><dgm:param type=\"dim\" val=\"1D\"/>\
             <dgm:param type=\"begPts\" val=\"bCtr\"/><dgm:param type=\"endPts\" val=\"tCtr\"/></dgm:alg>\
             <dgm:shape type=\"conn\" r:blip=\"\"><dgm:adjLst/></dgm:shape>\
             <dgm:presOf axis=\"self\"/><dgm:constrLst/><dgm:ruleLst/>\
             </dgm:layoutNode></dgm:forEach>\
             <dgm:forEach name=\"under\" ref=\"roots\"/>\
             </dgm:layoutNode>\
             </dgm:layoutNode></dgm:forEach>"
        ),
        // Round a circle from the top, with an arrow between each box and
        // the next and one from the last back to the first.
        Arrangement::Cycle => format!(
            "<dgm:alg type=\"cycle\"><dgm:param type=\"stAng\" val=\"0\"/>\
             <dgm:param type=\"spanAng\" val=\"360\"/></dgm:alg>\
             <dgm:shape type=\"none\" r:blip=\"\"><dgm:adjLst/></dgm:shape>\
             <dgm:presOf/><dgm:constrLst>\
             <dgm:constr type=\"w\" for=\"ch\" ptType=\"node\" refType=\"w\" fact=\"0.24\"/>\
             <dgm:constr type=\"h\" for=\"ch\" ptType=\"node\" refType=\"w\" refFor=\"ch\" refPtType=\"node\" fact=\"0.6\"/>\
             <dgm:constr type=\"w\" for=\"ch\" ptType=\"sibTrans\" refType=\"w\" refFor=\"ch\" refPtType=\"node\" fact=\"0.4\"/>\
             <dgm:constr type=\"h\" for=\"ch\" ptType=\"sibTrans\" refType=\"w\" refFor=\"ch\" refPtType=\"node\" fact=\"0.25\"/>\
             </dgm:constrLst><dgm:ruleLst/>\
             <dgm:forEach name=\"items\" axis=\"ch\" ptType=\"node\">\
             <dgm:layoutNode name=\"item\" styleLbl=\"node1\">{TEXT_NODE}\
             <dgm:shape type=\"roundRect\" r:blip=\"\"><dgm:adjLst/></dgm:shape>\
             </dgm:layoutNode>\
             <dgm:forEach name=\"round\" axis=\"self\" ptType=\"sibTrans\" hideLastTrans=\"0\">\
             <dgm:layoutNode name=\"arrow\" styleLbl=\"sibTrans2D1\">\
             <dgm:alg type=\"conn\"/>\
             <dgm:shape type=\"rightArrow\" r:blip=\"\"><dgm:adjLst/></dgm:shape>\
             <dgm:presOf axis=\"self\"/><dgm:constrLst/><dgm:ruleLst/>\
             </dgm:layoutNode></dgm:forEach>\
             </dgm:forEach>"
        ),
        // Levels one on another, the first at the top and a triangle, the
        // rest wider trapezoids down to the foot.
        Arrangement::Pyramid => format!(
            "<dgm:alg type=\"pyra\"/>\
             <dgm:shape type=\"none\" r:blip=\"\"><dgm:adjLst/></dgm:shape>\
             <dgm:presOf/><dgm:constrLst/><dgm:ruleLst/>\
             <dgm:forEach name=\"levels\" axis=\"ch\" ptType=\"node\">\
             <dgm:layoutNode name=\"level\" styleLbl=\"node1\">{TEXT_NODE}\
             <dgm:shape type=\"trapezoid\" r:blip=\"\"><dgm:adjLst/></dgm:shape>\
             </dgm:layoutNode></dgm:forEach>"
        ),
        // Boxes in rows, as many to a row as fills the room best, each row
        // read from the left.
        Arrangement::BlockList => format!(
            "<dgm:alg type=\"snake\"><dgm:param type=\"grDir\" val=\"tL\"/>\
             <dgm:param type=\"flowDir\" val=\"row\"/><dgm:param type=\"contDir\" val=\"sameDir\"/>\
             <dgm:param type=\"bkpt\" val=\"endCnv\"/></dgm:alg>\
             <dgm:shape type=\"none\" r:blip=\"\"><dgm:adjLst/></dgm:shape>\
             <dgm:presOf/><dgm:constrLst>\
             <dgm:constr type=\"w\" for=\"ch\" ptType=\"node\" refType=\"w\" fact=\"0.3\"/>\
             <dgm:constr type=\"h\" for=\"ch\" ptType=\"node\" refType=\"w\" refFor=\"ch\" refPtType=\"node\" fact=\"0.6\"/>\
             <dgm:constr type=\"w\" for=\"ch\" ptType=\"sibTrans\" refType=\"w\" refFor=\"ch\" refPtType=\"node\" fact=\"0.12\"/>\
             <dgm:constr type=\"sp\" refType=\"h\" refFor=\"ch\" refPtType=\"node\" fact=\"0.2\"/>\
             </dgm:constrLst><dgm:ruleLst/>\
             <dgm:forEach name=\"items\" axis=\"ch\" ptType=\"node\">\
             <dgm:layoutNode name=\"item\" styleLbl=\"node1\">{TEXT_NODE}\
             <dgm:shape type=\"rect\" r:blip=\"\"><dgm:adjLst/></dgm:shape>\
             </dgm:layoutNode>\
             <dgm:forEach name=\"between\" axis=\"followSib\" ptType=\"sibTrans\" cnt=\"1\">\
             <dgm:layoutNode name=\"space\">\
             <dgm:alg type=\"sp\"/>\
             <dgm:shape type=\"none\" r:blip=\"\"><dgm:adjLst/></dgm:shape>\
             <dgm:presOf axis=\"self\"/><dgm:constrLst/><dgm:ruleLst/>\
             </dgm:layoutNode></dgm:forEach>\
             </dgm:forEach>"
        ),
    };

    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
         <dgm:layoutDef xmlns:dgm=\"{DIAGRAM}\" xmlns:a=\"{main}\" xmlns:r=\"{rel}\" \
         uniqueId=\"{id}\"><dgm:title val=\"\"/><dgm:desc val=\"\"/>\
         <dgm:catLst><dgm:cat type=\"{category}\" pri=\"1000\"/></dgm:catLst>\
         <dgm:layoutNode name=\"diagram\">\
         <dgm:varLst><dgm:dir val=\"norm\"/><dgm:resizeHandles val=\"exact\"/></dgm:varLst>\
         {body}</dgm:layoutNode></dgm:layoutDef>",
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
    for label in LABELS.iter().chain([LINE_LABEL].iter()) {
        // A line between boxes has a line and no fill; a box has a fill and
        // no line.
        let (line, fill) = if *label == LINE_LABEL { (2, 0) } else { (0, 1) };
        out.push_str(&format!(
            "<dgm:styleLbl name=\"{label}\"><dgm:scene3d><a:camera prst=\"orthographicFront\"/>\
             <a:lightRig rig=\"threePt\" dir=\"t\"/></dgm:scene3d><dgm:sp3d/><dgm:txPr/>\
             <dgm:style><a:lnRef idx=\"{line}\"><a:scrgbClr r=\"0\" g=\"0\" b=\"0\"/></a:lnRef>\
             <a:fillRef idx=\"{fill}\"><a:scrgbClr r=\"0\" g=\"0\" b=\"0\"/></a:fillRef>\
             <a:effectRef idx=\"0\"><a:scrgbClr r=\"0\" g=\"0\" b=\"0\"/></a:effectRef>\
             <a:fontRef idx=\"minor\"><a:schemeClr val=\"lt1\"/></a:fontRef></dgm:style>\
             </dgm:styleLbl>"
        ));
    }
    out.push_str("</dgm:styleDef>");
    out
}

/// The colour list: which of the theme's colours each piece is drawn in.
fn colours_xml(colouring: Colouring) -> String {
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
         <dgm:colorsDef xmlns:dgm=\"{DIAGRAM}\" xmlns:a=\"{main}\" uniqueId=\"{id}\">\
         <dgm:title val=\"\"/><dgm:desc val=\"\"/>\
         <dgm:catLst><dgm:cat type=\"{category}\" pri=\"11002\"/></dgm:catLst>",
        main = crate::edit::DRAWING_MAIN,
        id = colouring.id(),
        category = colouring.category(),
    );
    let fills: String = colouring
        .scheme_names()
        .iter()
        .map(|name| format!("<a:schemeClr val=\"{name}\"/>"))
        .collect();
    for label in LABELS {
        out.push_str(&format!(
            "<dgm:styleLbl name=\"{label}\">\
             <dgm:fillClrLst meth=\"repeat\">{fills}</dgm:fillClrLst>\
             <dgm:linClrLst meth=\"repeat\">{fills}</dgm:linClrLst>\
             <dgm:effectClrLst/><dgm:txLinClrLst/>\
             <dgm:txFillClrLst meth=\"repeat\"><a:schemeClr val=\"lt1\"/></dgm:txFillClrLst>\
             <dgm:txEffectClrLst/></dgm:styleLbl>"
        ));
    }
    // The line from a box to the one under it, in the first of the colours.
    let first = colouring.scheme_names()[0];
    out.push_str(&format!(
        "<dgm:styleLbl name=\"{LINE_LABEL}\">\
         <dgm:fillClrLst meth=\"repeat\"><a:schemeClr val=\"{first}\"/></dgm:fillClrLst>\
         <dgm:linClrLst meth=\"repeat\"><a:schemeClr val=\"{first}\"/></dgm:linClrLst>\
         <dgm:effectClrLst/><dgm:txLinClrLst/>\
         <dgm:txFillClrLst meth=\"repeat\"><a:schemeClr val=\"dk1\"/></dgm:txFillClrLst>\
         <dgm:txEffectClrLst/></dgm:styleLbl>"
    ));
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

/// What a data model says, read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct ReadModel {
    nodes: Vec<Node>,
    arrangement: Option<Arrangement>,
    colouring: Option<Colouring>,
    right_to_left: bool,
    /// The relationship the drawing hangs off, if the model names one.
    drawing: Option<String>,
}

/// Reads the data model: the words as a tree, which layout and colours the
/// diagram asked for, and where its drawing is.
fn read_data_model(root: &Element) -> ReadModel {
    let mut read = ReadModel::default();
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
                    let set = point.child(Some(DIAGRAM), "prSet");
                    read.arrangement = set
                        .and_then(|set| set.attribute(None, "loTypeId"))
                        .and_then(Arrangement::from_layout_id);
                    read.colouring = set
                        .and_then(|set| set.attribute(None, "csTypeId"))
                        .and_then(Colouring::from_id);
                    read.right_to_left = set
                        .and_then(|set| set.child(Some(DIAGRAM), "presLayoutVars"))
                        .and_then(|vars| vars.child(Some(DIAGRAM), "dir"))
                        .and_then(|dir| dir.attribute(None, "val"))
                        == Some("rev");
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

    read.nodes = match root_id {
        Some(root_id) => children_of(&root_id, &words, &links, 0),
        // A model with no document point still has words, and a row of them is
        // the closest true reading of a file that says nothing about depth.
        None => words.iter().map(|(_, text)| Node::new(text)).collect(),
    };

    read.drawing = find_local(root, "dataModelExt")
        .and_then(|extension| extension.attribute(None, "relId"))
        .map(str::to_owned);
    read
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
fn read_drawing(root: &Element, theme: &Theme, styling: &Styling) -> Option<Group> {
    let tree = find_local(root, "spTree")?;

    let mut members = Vec::new();
    for shape in tree.child_elements().filter(|child| child.local_name() == "sp") {
        members.extend(read_drawn_shape(shape, theme, styling));
    }
    if members.is_empty() {
        return None;
    }
    Some(group_around(members))
}

/// Several members as one drawing, measured by what they cover.
///
/// The drawing states no rectangle of its own, and taking the shapes' own is
/// the only reading under which a diagram fills the frame it was given. A
/// picture that does not reach the frame's edge is taken to be centred in
/// it: as much room after the last shape as before the first.
fn group_around(members: Vec<Member>) -> Group {
    let left = members.iter().map(|member| member.x_emu).min().unwrap_or(0).max(0);
    let top = members.iter().map(|member| member.y_emu).min().unwrap_or(0).max(0);
    let width =
        members.iter().map(|member| member.x_emu + member.width_emu).max().unwrap_or(1) + left;
    let height =
        members.iter().map(|member| member.y_emu + member.height_emu).max().unwrap_or(1) + top;
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

/// One shape of a drawing, as members of the group it becomes: the shape,
/// and its words as a member of their own, hung where the drawing hangs
/// them.
fn read_drawn_shape(element: &Element, theme: &Theme, styling: &Styling) -> Vec<Member> {
    let Some(properties) = element.child_elements().find(|child| child.local_name() == "spPr")
    else {
        return Vec::new();
    };
    let Some(transform) = properties.child_elements().find(|child| child.local_name() == "xfrm")
    else {
        return Vec::new();
    };
    let (Some(offset), Some(extent)) = (
        transform.child_elements().find(|child| child.local_name() == "off"),
        transform.child_elements().find(|child| child.local_name() == "ext"),
    ) else {
        return Vec::new();
    };

    let number = |element: &Element, name: &str| -> i64 {
        element.attribute(None, name).and_then(|value| value.trim().parse().ok()).unwrap_or(0)
    };
    let (x, y) = (number(offset, "x"), number(offset, "y"));
    let (width, height) = (number(extent, "cx"), number(extent, "cy"));
    if width <= 0 || height <= 0 {
        return Vec::new();
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

    // The style: what Word reads the fill and the line from when the shape
    // states none, by index into the theme's format scheme with the colour
    // beside it.
    let style = element.child_elements().find(|child| child.local_name() == "style");
    let styled = |local: &str| -> Option<(u32, Option<String>)> {
        let reference = style?.child_elements().find(|child| child.local_name() == local)?;
        let index = reference.attribute(None, "idx")?.parse().ok()?;
        Some((index, colour_of(reference, theme)))
    };

    // The colours a diagram is drawn in are the theme's, named rather than
    // stated: a drawing read as though it said nothing about its colours is a
    // diagram drawn as a row of outlines. A gradient is read as one, and a
    // shape that says nothing takes what its style says.
    let solid = properties
        .child_elements()
        .find(|child| child.local_name() == "solidFill")
        .and_then(|solid| colour_of(solid, theme));
    let says_no_fill = properties.child_elements().any(|child| child.local_name() == "noFill");
    shape.fill = match (solid, crate::fills::read_fill(properties)) {
        // Stated as one colour, perhaps by a name in the theme with shifts
        // under it, which is followed here.
        (Some(colour), _) => crate::fills::Fill::Solid(colour),
        (None, crate::fills::Fill::None) if !says_no_fill => match styled("fillRef") {
            Some((index, Some(colour))) if index > 0 => crate::fills::Fill::Solid(colour),
            Some((index, None)) if index > 0 => {
                crate::fills::Fill::Solid(styling.accent().to_owned())
            }
            _ => crate::fills::Fill::None,
        },
        (None, other) => other,
    };
    if let Some(line) = properties.child_elements().find(|child| child.local_name() == "ln") {
        let has_colour = line.child_elements().any(|child| child.local_name() == "solidFill");
        if has_colour {
            shape.outline = colour_of(line, theme);
            shape.outline_emu = line
                .attribute(None, "w")
                .and_then(|value| value.parse().ok())
                .unwrap_or(crate::shapes::EMU_PER_POINT);
        }
    } else if let Some((index, colour)) = styled("lnRef") {
        if index > 0 {
            shape.outline = colour.or_else(|| Some(styling.accent().to_owned()));
            shape.outline_emu = crate::shapes::EMU_PER_POINT;
        }
    }
    // The words' colour is beside the font the style names, whose index is
    // a word and not a number.
    let ink = style
        .and_then(|style| style.child_elements().find(|child| child.local_name() == "fontRef"))
        .and_then(|reference| colour_of(reference, theme));

    let mut members = Vec::new();
    if let Some(body) = element.child_elements().find(|child| child.local_name() == "txBody") {
        let text = read_drawn_text(body, theme, ink.as_deref());
        shape.description = text
            .iter()
            .map(crate::model::Paragraph::plain_text)
            .collect::<Vec<_>>()
            .join(" ")
            .trim()
            .to_owned();
        shape.name = shape.description.clone();

        // Where the words go: their own rectangle when the drawing gives
        // them one — the words of a shape whose middle is not where its room
        // is — and the shape's otherwise. Either way they are hung in the
        // middle of it when the body asks, which is where Word hangs them.
        let text_rect = element
            .child_elements()
            .find(|child| child.local_name() == "txXfrm")
            .and_then(|transform| {
                let offset = transform.child_elements().find(|c| c.local_name() == "off")?;
                let extent = transform.child_elements().find(|c| c.local_name() == "ext")?;
                Some((
                    number(offset, "x"),
                    number(offset, "y"),
                    number(extent, "cx"),
                    number(extent, "cy"),
                ))
            });
        let body_properties = body.child_elements().find(|child| child.local_name() == "bodyPr");
        let anchor = body_properties
            .and_then(|properties| properties.attribute(None, "anchor"))
            .unwrap_or("t");
        // The room between the edge and the words, which the body states in
        // units and which is a tenth of an inch when it says nothing.
        let inset = |name: &str| -> i64 {
            body_properties
                .and_then(|properties| properties.attribute(None, name))
                .and_then(|value| value.parse().ok())
                .unwrap_or(crate::shapes::EMU_PER_POINT * 72 / 10)
        };
        if !text.is_empty() {
            let (tx, ty, tw, th) = text_rect.unwrap_or((x, y, width, height));
            members.push(words_member(
                text,
                tx,
                ty,
                tw,
                th,
                anchor,
                Some((inset("lIns"), inset("rIns"))),
            ));
        }
    }

    members.insert(
        0,
        Member {
            x_emu: x,
            y_emu: y,
            width_emu: width,
            height_emu: height,
            what: Inside::Shape(Box::new(shape)),
        },
    );
    members
}

/// The words of a shape as a member of their own: a rectangle with nothing
/// drawn but the words, hung where the anchor says.
fn words_member(
    text: Vec<Paragraph>,
    x: i64,
    y: i64,
    width: i64,
    height: i64,
    anchor: &str,
    margins: Option<(i64, i64)>,
) -> Member {
    // The words are drawn inset from the member's edge by a tenth of an
    // inch either side, as a text box's are. A drawing from Word states its
    // words' rectangle with that inset already meant; words laid out here
    // were fitted to the room inside the layout's own margins, so the
    // member is widened by the difference.
    let inset = crate::shapes::EMU_PER_POINT * 72 / 10;
    let (x, width) = match margins {
        Some((left, right)) => (x + left - inset, width - left - right + 2 * inset),
        None => (x, width),
    };
    // How tall the words come to, by an estimate of the font, so they can
    // be hung in the middle or at the foot.
    let lines: Vec<String> = text.iter().map(Paragraph::plain_text).collect();
    let size = text
        .iter()
        .flat_map(|paragraph| paragraph.runs.iter())
        .find_map(|run| run.properties.size_half_points)
        .map_or(12.0, |half| f64::from(half) / 2.0);
    let rows = language::rows_of(&lines, size, (width - 2 * inset) as f64);
    let text_height = (rows * size * 1.25 * 12700.0).round() as i64 + 12700 * 4;
    let top = match anchor {
        "ctr" => y + ((height - text_height) / 2).max(0),
        "b" => y + (height - text_height).max(0),
        _ => y,
    };
    let tall = text_height.min(height).max(1);
    let shape = Shape {
        preset: "rect".to_owned(),
        width_emu: width,
        height_emu: tall,
        fill: crate::fills::Fill::None,
        outline: None,
        outline_emu: 0,
        text,
        ..Shape::default()
    };
    Member {
        x_emu: x,
        y_emu: top,
        width_emu: width,
        height_emu: tall,
        what: Inside::Shape(Box::new(shape)),
    }
}

/// The words in a drawn shape, as paragraphs this program can lay out.
fn read_drawn_text(body: &Element, theme: &Theme, ink: Option<&str>) -> Vec<Paragraph> {
    let mut out = Vec::new();
    for paragraph in body.child_elements().filter(|child| child.local_name() == "p") {
        let properties = paragraph.child_elements().find(|child| child.local_name() == "pPr");
        let alignment =
            properties.and_then(|properties| properties.attribute(None, "algn")).map(|value| {
                match value {
                    "ctr" => Alignment::Center,
                    "r" => Alignment::End,
                    "just" => Alignment::Both,
                    _ => Alignment::Start,
                }
            });
        let level: i32 = properties
            .and_then(|properties| properties.attribute(None, "lvl"))
            .and_then(|value| value.parse().ok())
            .unwrap_or(0);

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
            let colour = properties
                .and_then(|properties| {
                    properties
                        .child_elements()
                        .find(|child| child.local_name() == "solidFill")
                        .and_then(|fill| colour_of(fill, theme))
                })
                .or_else(|| ink.map(str::to_owned));
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
        // A deeper level is a bullet under the line above, indented by its
        // depth.
        if level > 0 {
            if let Some(RunContent::Text(text)) =
                runs.first_mut().and_then(|first| first.content.first_mut())
            {
                text.insert_str(0, "• ");
            }
        }
        out.push(Paragraph {
            properties: ParagraphProperties {
                alignment,
                space_after: Some(0),
                indent_start: (level > 0).then_some(level * 360),
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
pub(crate) fn colour_of(parent: &Element, theme: &Theme) -> Option<String> {
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
    let saturation = if lightness > 0.5 {
        span / (2.0 - largest - smallest)
    } else {
        span / (largest + smallest)
    };
    let hue = if largest == red {
        60.0 * ((green - blue) / span).rem_euclid(6.0)
    } else if largest == green {
        60.0 * ((blue - red) / span + 2.0)
    } else {
        60.0 * ((red - green) / span + 4.0)
    };
    (hue, saturation, lightness)
}

/// And back to the three components.
fn from_hsl(hue: f32, saturation: f32, lightness: f32) -> (u8, u8, u8) {
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let sector = hue / 60.0;
    let second = chroma * (1.0 - (sector.rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match sector as u32 {
        0 => (chroma, second, 0.0),
        1 => (second, chroma, 0.0),
        2 => (0.0, chroma, second),
        3 => (0.0, second, chroma),
        4 => (second, 0.0, chroma),
        _ => (chroma, 0.0, second),
    };
    let lift = lightness - chroma / 2.0;
    let component = |value: f32| ((value + lift) * 255.0).round().clamp(0.0, 255.0) as u8;
    (component(r), component(g), component(b))
}

/// What was laid out, as one drawing this program can draw: a shape for
/// every piece, with its words hung in the middle of it.
fn group_of(drawn: &[Drawn], styling: &Styling) -> Group {
    let mut members = Vec::new();
    for piece in drawn {
        let fill = styling.fill_of(&piece.style_label, piece.index, piece.count);
        let line = styling.line_of(&piece.style_label, piece.index, piece.count);
        let ink = styling.text_of(&piece.style_label, piece.index, piece.count);
        let words: Vec<String> = piece.text.iter().map(|(_, line)| line.clone()).collect();
        let shape = Shape {
            preset: piece.preset.clone(),
            width_emu: piece.width,
            height_emu: piece.height,
            rotation: (piece.rotation * 60000.0).round() as i32,
            fill: fill.map_or(crate::fills::Fill::None, crate::fills::Fill::Solid),
            outline: line,
            outline_emu: crate::shapes::EMU_PER_POINT,
            name: words.join(" "),
            description: words.join(" "),
            ..Shape::default()
        };
        members.push(Member {
            x_emu: piece.x,
            y_emu: piece.y,
            width_emu: piece.width,
            height_emu: piece.height,
            what: Inside::Shape(Box::new(shape)),
        });
        if !piece.text.is_empty() {
            let alignment = match piece.horizontal.as_str() {
                "l" => Alignment::Start,
                "r" => Alignment::End,
                _ => Alignment::Center,
            };
            let paragraphs: Vec<Paragraph> = piece
                .text
                .iter()
                .map(|(level, line)| caption(line, &ink, piece.font_size, *level, alignment))
                .collect();
            let (tx, ty, tw, th) =
                piece.text_rect.unwrap_or((piece.x, piece.y, piece.width, piece.height));
            members.push(words_member(paragraphs, tx, ty, tw, th, "ctr", Some(piece.margins)));
        }
    }
    group_around(members)
}

/// The words inside a box: in the colour the style gives them, at the size
/// the layout fitted them to.
fn caption(text: &str, colour: &str, size: f64, level: u8, alignment: Alignment) -> Paragraph {
    let text = if level > 0 { format!("• {text}") } else { text.to_owned() };
    Paragraph {
        properties: ParagraphProperties {
            alignment: Some(alignment),
            space_after: Some(0),
            indent_start: (level > 0).then_some(i32::from(level) * 360),
            ..ParagraphProperties::default()
        },
        runs: vec![Run {
            properties: RunProperties {
                color: Some(colour.to_owned()),
                size_half_points: Some((size * 2.0).round().max(2.0) as u32),
                ..RunProperties::default()
            },
            content: vec![RunContent::Text(text)],
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

    fn nodes(count: usize) -> Vec<Node> {
        items(count).iter().map(|item| Node::new(item)).collect()
    }

    fn parsed(xml: &str) -> Element {
        wp_xml::tree::XmlTree::parse(xml).expect("the part should parse").root
    }

    /// What an arrangement draws for some words, in a room six inches wide.
    fn drawn(arrangement: Arrangement, nodes: &[Node]) -> Vec<Drawn> {
        let count: usize = nodes.iter().map(Node::count).sum();
        let depth = nodes.iter().map(Node::depth).max().unwrap_or(1);
        let height = arrangement.natural_height(count, depth, ROOM);
        let model = data_model_xml(arrangement, Colouring::default(), nodes, "rId1", false);
        lay_out_parts(&layout_xml(arrangement), &model, ROOM, height)
    }

    fn boxes(drawn: &[Drawn]) -> Vec<&Drawn> {
        drawn.iter().filter(|piece| !piece.text.is_empty()).collect()
    }

    #[test]
    fn every_arrangement_says_what_it_is_called_and_what_the_gallery_calls_it() {
        for arrangement in Arrangement::ALL {
            assert!(!arrangement.label().is_empty());
            assert_eq!(Arrangement::from_layout_id(arrangement.layout_id()), Some(*arrangement));
        }
        assert_eq!(Arrangement::from_layout_id("urn:something/else"), None);
        for colouring in Colouring::ALL {
            assert_eq!(Colouring::from_id(colouring.id()), Some(*colouring));
        }
    }

    #[test]
    fn a_process_has_a_box_for_every_step_and_an_arrow_between_each_pair() {
        let drawn = drawn(Arrangement::Process, &nodes(4));
        assert_eq!(boxes(&drawn).len(), 4);
        assert_eq!(drawn.iter().filter(|piece| piece.preset == "rightArrow").count(), 3);
    }

    #[test]
    fn one_step_needs_no_arrow() {
        assert_eq!(drawn(Arrangement::Process, &nodes(1)).len(), 1);
    }

    #[test]
    fn every_arrangement_stays_inside_the_room_it_was_given() {
        for arrangement in Arrangement::ALL {
            let words = if arrangement.is_tree() {
                vec![Node { text: "Top".to_owned(), children: nodes(4) }]
            } else {
                nodes(5)
            };
            let count: usize = words.iter().map(Node::count).sum();
            let height = arrangement.natural_height(count, 2, ROOM);
            let drawn = drawn(*arrangement, &words);
            assert_eq!(boxes(&drawn).len(), count, "{}", arrangement.label());
            for piece in &drawn {
                assert!(
                    piece.x >= 0 && piece.x + piece.width <= ROOM + 1,
                    "{}: {piece:?}",
                    arrangement.label()
                );
                assert!(
                    piece.y >= 0 && piece.y + piece.height <= height + 1,
                    "{}: {piece:?}",
                    arrangement.label()
                );
            }
        }
    }

    #[test]
    fn a_list_puts_each_box_under_the_last() {
        let drawn = drawn(Arrangement::List, &nodes(3));
        let tops: Vec<i64> = boxes(&drawn).iter().map(|piece| piece.y).collect();
        assert!(tops[0] < tops[1] && tops[1] < tops[2], "{tops:?}");
        assert!(boxes(&drawn).iter().all(|piece| piece.width == ROOM));
    }

    #[test]
    fn a_hierarchy_puts_the_first_box_above_the_others_and_draws_a_line_to_each() {
        let tree = vec![Node { text: "Top".to_owned(), children: nodes(3) }];
        let drawn = drawn(Arrangement::Hierarchy, &tree);
        let boxes = boxes(&drawn);
        assert_eq!(boxes.len(), 4);
        assert!(boxes[1..].iter().all(|piece| piece.y > boxes[0].y), "the row is not below");

        // The line down to each box: a stem out of the box above, a run
        // across, and a drop into each of the three under it.
        let joins: Vec<&Drawn> = drawn.iter().filter(|piece| piece.preset == "rect").collect();
        assert_eq!(joins.len(), 3 * 3);
        let row_top = boxes[1].y;
        let head_bottom = boxes[0].y + boxes[0].height;
        for join in &joins {
            assert!(join.y + 1 >= head_bottom && join.y + join.height <= row_top + 1, "{join:?}");
        }
        for under in &boxes[1..] {
            let middle = under.x + under.width / 2;
            assert!(
                joins.iter().any(|join| join.x <= middle && middle <= join.x + join.width + 1),
                "nothing comes down into the box at {}",
                under.x
            );
        }
    }

    #[test]
    fn a_hierarchy_goes_down_as_many_levels_as_the_words_do() {
        let tree = vec![Node {
            text: "Top".to_owned(),
            children: vec![Node { text: "Middle".to_owned(), children: nodes(2) }],
        }];
        let drawn = drawn(Arrangement::Hierarchy, &tree);
        let boxes = boxes(&drawn);
        assert_eq!(boxes.len(), 4);
        let mut tops: Vec<i64> = boxes.iter().map(|piece| piece.y).collect();
        tops.dedup();
        assert_eq!(tops.len(), 3, "three rows: {tops:?}");
    }

    #[test]
    fn a_hierarchy_of_one_is_the_one_box() {
        assert_eq!(drawn(Arrangement::Hierarchy, &nodes(1)).len(), 1);
    }

    #[test]
    fn a_cycle_goes_round_with_an_arrow_from_the_last_back_to_the_first() {
        let drawn = drawn(Arrangement::Cycle, &nodes(4));
        assert_eq!(boxes(&drawn).len(), 4);
        let arrows: Vec<&Drawn> =
            drawn.iter().filter(|piece| piece.preset == "rightArrow").collect();
        assert_eq!(arrows.len(), 4, "one between each pair, and one back round");
        // The arrows are turned to follow the circle.
        assert!(arrows.iter().any(|arrow| arrow.rotation > 0.0));
        // The first box is at the top and the third at the bottom.
        let boxes = boxes(&drawn);
        assert!(boxes[0].y < boxes[2].y);
    }

    #[test]
    fn a_pyramid_widens_towards_its_foot() {
        let drawn = drawn(Arrangement::Pyramid, &nodes(3));
        let boxes = boxes(&drawn);
        assert_eq!(boxes[0].preset, "triangle");
        assert_eq!(boxes[1].preset, "trapezoid");
        assert!(boxes[0].width < boxes[1].width && boxes[1].width < boxes[2].width);
    }

    #[test]
    fn a_block_list_fills_rows() {
        let drawn = drawn(Arrangement::BlockList, &nodes(6));
        let boxes = boxes(&drawn);
        assert_eq!(boxes.len(), 6);
        let mut rows: Vec<i64> = boxes.iter().map(|piece| piece.y).collect();
        rows.dedup();
        assert!(rows.len() >= 2, "one row for six boxes");
    }

    #[test]
    fn the_words_are_hung_in_the_middle_of_their_box() {
        let drawn = drawn(Arrangement::Process, &nodes(2));
        let styling = Styling::read(None, None, &Theme::default());
        let group = group_of(&drawn, &styling);
        // Two boxes, an arrow, and the words of each box.
        assert_eq!(group.members.len(), 5);
        let (shape, words) = (&group.members[0], &group.members[1]);
        assert!(words.y_emu >= shape.y_emu);
        assert!(words.y_emu + words.height_emu <= shape.y_emu + shape.height_emu);
        let Inside::Shape(text) = &words.what else { panic!("a shape") };
        assert_eq!(text.text[0].plain_text(), "Step 0");
        assert_eq!(text.fill, crate::fills::Fill::None);
    }

    #[test]
    fn the_data_model_says_one_point_for_every_item_and_hangs_them_where_it_should() {
        let xml =
            data_model_xml(Arrangement::Process, Colouring::default(), &nodes(3), "rId1", false);
        let read = read_data_model(&parsed(&xml));
        assert_eq!(read.arrangement, Some(Arrangement::Process));
        assert_eq!(read.colouring, Some(Colouring::Accent1));
        assert_eq!(read.drawing.as_deref(), Some("rId1"));
        assert_eq!(read.nodes.len(), 3, "a process is a row and not a tree");
        assert_eq!(read.nodes[0].text, "Step 0");
        assert_eq!(read.nodes[2].text, "Step 2");
        assert!(read.nodes.iter().all(|node| node.children.is_empty()));
        assert!(!read.right_to_left);
    }

    #[test]
    fn a_tree_is_written_as_a_tree_and_read_back_as_one() {
        let tree = vec![Node {
            text: "Top".to_owned(),
            children: vec![
                Node { text: "Middle".to_owned(), children: nodes(2) },
                Node::new("Aside"),
            ],
        }];
        let xml = data_model_xml(Arrangement::Hierarchy, Colouring::Colorful, &tree, "rId1", true);
        let read = read_data_model(&parsed(&xml));
        assert_eq!(read.nodes, tree);
        assert_eq!(read.colouring, Some(Colouring::Colorful));
        assert!(read.right_to_left);
    }

    #[test]
    fn the_words_of_a_box_come_back_whatever_they_are_made_of() {
        let words = vec![Node::new("Fish & chips"), Node::new("<angles>")];
        let xml = data_model_xml(Arrangement::List, Colouring::default(), &words, "rId1", false);
        let read = read_data_model(&parsed(&xml));
        assert_eq!(read.nodes[0].text, "Fish & chips");
        assert_eq!(read.nodes[1].text, "<angles>");
    }

    #[test]
    fn the_drawing_holds_a_shape_for_every_piece_with_its_words_in_it() {
        let drawn = drawn(Arrangement::Process, &nodes(3));
        let styling = Styling::read(None, None, &Theme::default());
        let xml = drawing_xml(&drawn, &styling);
        let group = read_drawing(&parsed(&xml), &Theme::default(), &styling).expect("a drawing");
        let words: Vec<String> = group
            .members
            .iter()
            .filter_map(|member| match &member.what {
                Inside::Shape(shape) if !shape.text.is_empty() => Some(shape.text[0].plain_text()),
                _ => None,
            })
            .collect();
        assert_eq!(words, vec!["Step 0", "Step 1", "Step 2"]);
        let Inside::Shape(first) = &group.members[0].what else { panic!("a shape") };
        assert_eq!(first.fill, crate::fills::Fill::Solid(Theme::default().color(Slot::Accent1)));
    }

    #[test]
    fn a_drawing_that_names_the_themes_colours_is_drawn_in_them() {
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
        let styling = Styling::read(None, None, &theme);
        let group = read_drawing(&parsed(&xml), &theme, &styling).expect("a drawing");
        let Inside::Shape(shape) = &group.members[0].what else { panic!("a shape") };
        assert_eq!(shape.fill, crate::fills::Fill::Solid(theme.color(Slot::Accent2)));
    }

    #[test]
    fn a_shape_that_states_no_fill_takes_what_its_style_says() {
        // What Word writes for most shapes of a diagram: the fill is in the
        // style, by index, with the colour beside it.
        let xml = format!(
            "<dsp:drawing xmlns:dsp=\"{DIAGRAM_DRAWING}\" xmlns:a=\"{main}\"><dsp:spTree>\
             <dsp:sp modelId=\"1\"><dsp:spPr>\
             <a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"100\" cy=\"50\"/></a:xfrm>\
             <a:prstGeom prst=\"roundRect\"/></dsp:spPr>\
             <dsp:style><a:lnRef idx=\"2\"><a:schemeClr val=\"accent3\"/></a:lnRef>\
             <a:fillRef idx=\"1\"><a:schemeClr val=\"accent4\"/></a:fillRef>\
             <a:fontRef idx=\"minor\"><a:schemeClr val=\"lt1\"/></a:fontRef></dsp:style>\
             <dsp:txBody><a:bodyPr anchor=\"ctr\"/><a:p><a:r><a:t>Words</a:t></a:r></a:p></dsp:txBody>\
             <dsp:txXfrm><a:off x=\"10\" y=\"10\"/><a:ext cx=\"80\" cy=\"30\"/></dsp:txXfrm>\
             </dsp:sp></dsp:spTree></dsp:drawing>",
            main = crate::edit::DRAWING_MAIN,
        );
        let theme = Theme::default();
        let styling = Styling::read(None, None, &theme);
        let group = read_drawing(&parsed(&xml), &theme, &styling).expect("a drawing");
        let Inside::Shape(shape) = &group.members[0].what else { panic!("a shape") };
        assert_eq!(shape.fill, crate::fills::Fill::Solid(theme.color(Slot::Accent4)));
        assert_eq!(shape.outline, Some(theme.color(Slot::Accent3)));
        // The words are in their own rectangle, in the font's colour.
        let words = &group.members[1];
        assert_eq!(words.x_emu, 10);
        let Inside::Shape(text) = &words.what else { panic!("a shape") };
        assert_eq!(text.text[0].plain_text(), "Words");
        assert_eq!(text.text[0].runs[0].properties.color, Some(theme.color(Slot::Light1)));
    }

    #[test]
    fn a_gradient_fill_is_read_as_one() {
        let xml = format!(
            "<dsp:drawing xmlns:dsp=\"{DIAGRAM_DRAWING}\" xmlns:a=\"{main}\"><dsp:spTree>\
             <dsp:sp modelId=\"1\"><dsp:spPr>\
             <a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"100\" cy=\"50\"/></a:xfrm>\
             <a:prstGeom prst=\"rect\"/>\
             <a:gradFill><a:gsLst><a:gs pos=\"0\"><a:srgbClr val=\"FF0000\"/></a:gs>\
             <a:gs pos=\"100000\"><a:srgbClr val=\"0000FF\"/></a:gs></a:gsLst><a:lin ang=\"5400000\"/></a:gradFill>\
             </dsp:spPr></dsp:sp></dsp:spTree></dsp:drawing>",
            main = crate::edit::DRAWING_MAIN,
        );
        let theme = Theme::default();
        let styling = Styling::read(None, None, &theme);
        let group = read_drawing(&parsed(&xml), &theme, &styling).expect("a drawing");
        let Inside::Shape(shape) = &group.members[0].what else { panic!("a shape") };
        assert!(matches!(shape.fill, crate::fills::Fill::Gradient(_)), "{:?}", shape.fill);
    }

    #[test]
    fn a_colour_the_file_shifts_is_shifted() {
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
        for arrangement in Arrangement::ALL {
            let drawn = drawn(*arrangement, &nodes(3));
            let styling = Styling::read(None, None, &Theme::default());
            for part in [
                data_model_xml(*arrangement, Colouring::Colorful, &nodes(3), "rId1", false),
                layout_xml(*arrangement),
                quick_style_xml(),
                colours_xml(Colouring::Colorful),
                drawing_xml(&drawn, &styling),
            ] {
                wp_xml::tree::XmlTree::parse(&part)
                    .unwrap_or_else(|error| panic!("{}: {error}", arrangement.label()));
            }
        }
    }

    #[test]
    fn the_style_and_the_colours_name_the_same_pieces() {
        let style = quick_style_xml();
        let colours = colours_xml(Colouring::Accent2);
        for label in LABELS {
            assert!(style.contains(&format!("name=\"{label}\"")), "the style is missing {label}");
            assert!(
                colours.contains(&format!("name=\"{label}\"")),
                "the colours are missing {label}"
            );
        }
        // And the colours are read back as what they name.
        let styling =
            Styling::read(Some(&parsed(&colours)), Some(&parsed(&style)), &Theme::default());
        assert_eq!(
            styling.fill_of("node1", 0, 3).as_deref(),
            Some(Theme::default().color(Slot::Accent2).as_str())
        );
        let colorful = Styling::read(
            Some(&parsed(&colours_xml(Colouring::Colorful))),
            Some(&parsed(&style)),
            &Theme::default(),
        );
        assert_ne!(colorful.fill_of("node1", 0, 3), colorful.fill_of("node1", 1, 3));
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

        let read = read_data_model(&parsed(&xml));
        assert_eq!(read.arrangement, Some(Arrangement::Hierarchy));
        assert_eq!(read.drawing.as_deref(), Some("rId7"));
        assert_eq!(read.nodes.len(), 1, "the point the layout engine left behind is not a box");
        assert_eq!(read.nodes[0].text, "Head office");
        assert_eq!(read.nodes[0].children.len(), 1);
        // Two runs of one paragraph are one line, which is what the box says.
        assert_eq!(read.nodes[0].children[0].text, "North & South");
    }

    #[test]
    fn a_diagram_laid_out_by_a_layout_this_program_did_not_write_still_says_what_it_says() {
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
        let read = read_data_model(&parsed(&xml));
        assert_eq!(read.arrangement, None, "no arrangement here draws gears");
        assert_eq!(read.drawing, None);
        assert_eq!(read.nodes.len(), 1);
        assert_eq!(read.nodes[0].text, "Turn");
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
        let read = read_data_model(&parsed(&xml));
        assert_eq!(read.nodes.len(), 1);
        assert_eq!(read.nodes[0].text, "Round");
    }

    #[test]
    fn a_row_asked_to_be_a_hierarchy_hangs_the_rest_under_the_first() {
        let row = nodes(3);
        let tree = rehang(row.clone(), Arrangement::Hierarchy);
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].children.len(), 2);
        // A tree keeps its shape whatever it is asked to be.
        assert_eq!(rehang(tree.clone(), Arrangement::Process), tree);
        assert_eq!(rehang(row.clone(), Arrangement::Process), row);
    }
}
