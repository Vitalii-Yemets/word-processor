//! The layout language: how a diagram's words become its picture.
//!
//! # What the language is
//!
//! A layout definition (`layout1.xml`) is a small program. Its statements
//! are layout nodes, each of which may draw one shape and each of which
//! arranges its children by an *algorithm* — in a line, in a circle, in a
//! tree, in rows that turn back on themselves, as the levels of a pyramid,
//! or by placing each child where its constraints say. `forEach` walks the
//! data model — the words — and makes a layout node for every point it
//! visits; `choose` picks between arrangements by asking questions of the
//! model, how many children a point has, how deep it stands, which way the
//! diagram reads. *Constraints* say how big things are, mostly as fractions
//! of one another; *rules* say what may give way when the words do not fit.
//!
//! Word runs this program every time the words change, which is what makes
//! SmartArt SmartArt: a box added is a picture re-laid out, not a box
//! squeezed in. The drawing Word leaves behind (see [`super`]) is the
//! output of one such run, and a file with no drawing, or a diagram whose
//! words have changed here since, has to be laid out by running the
//! program — which is what this module does.
//!
//! # What runs here
//!
//! The walk over the model is complete: every axis, every point type, every
//! function a condition may ask. Of the algorithms, the six that draw
//! nearly every diagram in the gallery — linear, composite, hierarchy,
//! cycle, snake and pyramid — and the two that draw nothing but a shape or
//! the words in one; connectors are drawn as the shape their node names or
//! as a line. Constraints are read for every type and resolved in the order
//! written, with a reference to another node's size worked out from that
//! node; the sizes a linear or a cyclic arrangement shares out are solved
//! together, because one node's width may be stated as a fraction of
//! another's that is itself being shared out. A rule that lets the font
//! shrink is followed down to its floor by measuring the words against the
//! room — measured by an estimate of the font, because no font is in reach
//! where a package is read.
//!
//! What does not run is named in the roadmap: the three-dimensional
//! parameters, the animation ones, and the finer points of Word's own
//! fitting, which is done against real fonts and against rules this program
//! reads but applies plainly.

use std::collections::HashMap;

use wp_xml::tree::Element;

use super::DIAGRAM;
use crate::EMU_PER_INCH;

/// One point of the data model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Point {
    pub id: String,
    pub kind: PointKind,
    /// The words, one entry per paragraph, with the paragraph's level.
    pub text: Vec<(u8, String)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PointKind {
    Doc,
    Node,
    Assistant,
    ParentTransition,
    SiblingTransition,
    /// What the layout engine left behind last time, which is not a word.
    Presentation,
}

impl PointKind {
    fn from_word(word: Option<&str>) -> Self {
        match word {
            Some("doc") => Self::Doc,
            Some("asst") => Self::Assistant,
            Some("parTrans") => Self::ParentTransition,
            Some("sibTrans") => Self::SiblingTransition,
            Some("pres") => Self::Presentation,
            _ => Self::Node,
        }
    }
}

/// The data model as the language walks it: the points, who hangs under
/// whom, and the transitions each point owns.
#[derive(Clone, Debug, Default)]
pub(crate) struct Model {
    pub points: Vec<Point>,
    children: Vec<Vec<usize>>,
    parent: Vec<Option<usize>>,
    parent_transition: Vec<Option<usize>>,
    sibling_transition: Vec<Option<usize>>,
    /// Which point each transition belongs to.
    owner: HashMap<usize, usize>,
    doc: Option<usize>,
    /// What the document point says about the diagram: which way it reads,
    /// how a hierarchy branches, and the rest of `presLayoutVars`.
    pub vars: HashMap<String, String>,
}

impl Model {
    /// Reads a data model part.
    pub(crate) fn read(root: &Element) -> Self {
        let mut model = Self::default();
        let mut by_id: HashMap<String, usize> = HashMap::new();
        if let Some(list) = root.child(Some(DIAGRAM), "ptLst") {
            for element in list.children_named(Some(DIAGRAM), "pt") {
                let Some(id) = element.attribute(None, "modelId") else { continue };
                let kind = PointKind::from_word(element.attribute(None, "type"));
                let text = element.child(Some(DIAGRAM), "t").map(paragraphs_of).unwrap_or_default();
                if kind == PointKind::Doc {
                    model.doc = Some(model.points.len());
                    if let Some(set) = element.child(Some(DIAGRAM), "prSet") {
                        if let Some(vars) = set.child(Some(DIAGRAM), "presLayoutVars") {
                            for var in vars.child_elements() {
                                if let Some(value) = var.attribute(None, "val") {
                                    model
                                        .vars
                                        .insert(var.local_name().to_owned(), value.to_owned());
                                }
                            }
                        }
                    }
                }
                by_id.insert(id.to_owned(), model.points.len());
                model.points.push(Point { id: id.to_owned(), kind, text });
            }
        }
        let count = model.points.len();
        model.children = vec![Vec::new(); count];
        model.parent = vec![None; count];
        model.parent_transition = vec![None; count];
        model.sibling_transition = vec![None; count];

        // Who hangs under whom, in the order the connections give.
        let mut links: Vec<Link> = Vec::new();
        if let Some(list) = root.child(Some(DIAGRAM), "cxnLst") {
            for connection in list.children_named(Some(DIAGRAM), "cxn") {
                if !matches!(connection.attribute(None, "type"), None | Some("parOf")) {
                    continue;
                }
                let find = |name: &str| {
                    connection.attribute(None, name).and_then(|id| by_id.get(id).copied())
                };
                let (Some(source), Some(destination)) = (find("srcId"), find("destId")) else {
                    continue;
                };
                if source == destination {
                    continue;
                }
                let order = connection
                    .attribute(None, "srcOrd")
                    .and_then(|value| value.parse::<i64>().ok())
                    .unwrap_or(0);
                links.push(Link {
                    source,
                    destination,
                    order,
                    parent_transition: find("parTransId"),
                    sibling_transition: find("sibTransId"),
                });
            }
        }
        links.sort_by_key(|link| (link.source, link.order));
        for Link { source, destination, parent_transition, sibling_transition, .. } in links {
            if model.parent[destination].is_some() {
                continue;
            }
            model.children[source].push(destination);
            model.parent[destination] = Some(source);
            model.parent_transition[destination] = parent_transition;
            model.sibling_transition[destination] = sibling_transition;
            for transition in [parent_transition, sibling_transition].into_iter().flatten() {
                model.owner.insert(transition, destination);
            }
        }
        model
    }

    /// The document point, or the first point when the model has none.
    pub(crate) fn root(&self) -> Option<usize> {
        self.doc.or_else(|| (!self.points.is_empty()).then_some(0))
    }

    fn siblings(&self, point: usize) -> &[usize] {
        match self.parent[point] {
            Some(parent) => &self.children[parent],
            None => &[],
        }
    }

    fn position_among_siblings(&self, point: usize) -> usize {
        self.siblings(point).iter().position(|sibling| *sibling == point).unwrap_or(0)
    }

    fn descendants(&self, point: usize, out: &mut Vec<usize>, depth: usize) {
        if depth > 32 {
            return;
        }
        for child in &self.children[point] {
            out.push(*child);
            self.descendants(*child, out, depth + 1);
        }
    }

    fn depth_of(&self, point: usize) -> usize {
        let mut depth = 0;
        let mut at = point;
        while let Some(parent) = self.parent[at] {
            depth += 1;
            at = parent;
            if depth > 64 {
                break;
            }
        }
        depth
    }

    fn max_depth_under(&self, point: usize) -> usize {
        self.children[point].iter().map(|child| 1 + self.max_depth_under(*child)).max().unwrap_or(0)
    }

    /// The points an axis reaches from one, before the point type is asked.
    fn along(&self, from: usize, axis: &str) -> Vec<usize> {
        match axis {
            "self" => vec![from],
            "ch" => self.children[from].clone(),
            "des" => {
                let mut out = Vec::new();
                self.descendants(from, &mut out, 0);
                out
            }
            "desOrSelf" => {
                let mut out = vec![from];
                self.descendants(from, &mut out, 0);
                out
            }
            "par" => self.parent[from].into_iter().collect(),
            "ancst" => {
                let mut out = Vec::new();
                let mut at = from;
                while let Some(parent) = self.parent[at] {
                    out.push(parent);
                    at = parent;
                    if out.len() > 64 {
                        break;
                    }
                }
                out
            }
            "ancstOrSelf" => {
                let mut out = vec![from];
                out.extend(self.along(from, "ancst"));
                out
            }
            "followSib" => {
                let siblings = self.siblings(from);
                let at = self.position_among_siblings(from);
                siblings.get(at + 1..).map(<[usize]>::to_vec).unwrap_or_default()
            }
            "precedSib" => {
                let siblings = self.siblings(from);
                let at = self.position_among_siblings(from);
                siblings[..at.min(siblings.len())].to_vec()
            }
            "follow" => {
                let mut out = Vec::new();
                for sibling in self.along(from, "followSib") {
                    out.push(sibling);
                    self.descendants(sibling, &mut out, 0);
                }
                out
            }
            "preced" => {
                let mut out = Vec::new();
                for sibling in self.along(from, "precedSib") {
                    out.push(sibling);
                    self.descendants(sibling, &mut out, 0);
                }
                out
            }
            "root" => self.root().into_iter().collect(),
            _ => Vec::new(),
        }
    }

    /// The points an axis reaches, of a type, from a start for a count.
    ///
    /// A transition is not in the tree — it belongs to the point whose
    /// connection names it — so asking for transitions along an axis means
    /// the transitions of the points the axis reaches: the sibling
    /// transition *after* a point, or the parent transition into it.
    pub(crate) fn select(&self, from: usize, spec: &Selection) -> Vec<usize> {
        let mut points: Vec<usize> = Vec::new();
        let steps = spec.axis.len().max(1);
        let mut current = vec![from];
        for step in 0..steps {
            let axis = spec.axis.get(step).map_or("self", String::as_str);
            let wanted = spec.pt_type.get(step).map_or("all", String::as_str);
            let mut next = Vec::new();
            for point in &current {
                let reached = self.along(*point, axis);
                match wanted {
                    "sibTrans" => {
                        // The transition after each point reached — except
                        // that the one after the current point is asked for
                        // as `followSib`, which is how Word's own layouts put
                        // an arrow between a box and the next: the transition
                        // stands between the point and its following sibling,
                        // and there is none after the last.
                        let candidates: Vec<usize> = if axis == "followSib" {
                            if reached.is_empty() {
                                Vec::new()
                            } else {
                                vec![*point]
                            }
                        } else {
                            reached
                        };
                        for candidate in candidates {
                            if let Some(transition) = self.sibling_transition[candidate] {
                                next.push(transition);
                            }
                        }
                    }
                    "parTrans" => {
                        for candidate in reached {
                            if let Some(transition) = self.parent_transition[candidate] {
                                next.push(transition);
                            }
                        }
                    }
                    _ => {
                        for candidate in reached {
                            if self.matches(candidate, wanted) {
                                next.push(candidate);
                            }
                        }
                    }
                }
            }
            // The start, the count and the step apply to each axis in turn.
            let start = spec.st.get(step).copied().unwrap_or(1).max(1) as usize - 1;
            let count = spec.cnt.get(step).copied().unwrap_or(0).max(0) as usize;
            let stride = spec.step.get(step).copied().unwrap_or(1).max(1) as usize;
            let mut taken: Vec<usize> = next.into_iter().skip(start).step_by(stride).collect();
            if count > 0 {
                taken.truncate(count);
            }
            current = taken;
            points.clone_from(&current);
        }
        points
    }

    /// Whether a point is of a named type.
    fn matches(&self, point: usize, wanted: &str) -> bool {
        let kind = self.points[point].kind;
        match wanted {
            "all" => true,
            "node" => matches!(kind, PointKind::Node | PointKind::Assistant),
            "nonAsst" | "norm" => kind == PointKind::Node,
            "asst" => kind == PointKind::Assistant,
            "nonNorm" => kind == PointKind::Assistant,
            "doc" => kind == PointKind::Doc,
            "parTrans" => kind == PointKind::ParentTransition,
            "sibTrans" => kind == PointKind::SiblingTransition,
            "pres" => kind == PointKind::Presentation,
            _ => true,
        }
    }
}

/// One connection of the model: who hangs under whom, in what order, with
/// the two transitions the connection names.
struct Link {
    source: usize,
    destination: usize,
    order: i64,
    parent_transition: Option<usize>,
    sibling_transition: Option<usize>,
}

/// The paragraphs of a text body, each with its level.
fn paragraphs_of(body: &Element) -> Vec<(u8, String)> {
    body.child_elements()
        .filter(|child| child.local_name() == "p")
        .map(|paragraph| {
            let level = paragraph
                .child_elements()
                .find(|child| child.local_name() == "pPr")
                .and_then(|properties| properties.attribute(None, "lvl"))
                .and_then(|value| value.parse().ok())
                .unwrap_or(0);
            let mut line = String::new();
            gather_text(paragraph, &mut line);
            (level, line)
        })
        // A paragraph with nothing in it is not a word: a transition's body
        // has one, and a transition has nothing to say.
        .filter(|(_, line)| !line.trim().is_empty())
        .collect()
}

fn gather_text(element: &Element, out: &mut String) {
    for child in element.child_elements() {
        if child.local_name() == "t" {
            out.push_str(&child.text_content());
        } else {
            gather_text(child, out);
        }
    }
}

/// Which points a `forEach`, a `presOf` or a condition reaches: an axis (or
/// several, walked in turn), a point type for each, and a start, count and
/// step for each.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Selection {
    axis: Vec<String>,
    pt_type: Vec<String>,
    st: Vec<i64>,
    cnt: Vec<i64>,
    step: Vec<i64>,
}

impl Selection {
    fn read(element: &Element, default_axis: &str) -> Self {
        let words = |name: &str| -> Vec<String> {
            element
                .attribute(None, name)
                .map(|value| value.split_whitespace().map(str::to_owned).collect())
                .unwrap_or_default()
        };
        let numbers = |name: &str| -> Vec<i64> {
            element
                .attribute(None, name)
                .map(|value| value.split_whitespace().filter_map(|n| n.parse().ok()).collect())
                .unwrap_or_default()
        };
        let mut axis = words("axis");
        if axis.is_empty() {
            axis.push(default_axis.to_owned());
        }
        Self {
            axis,
            pt_type: words("ptType"),
            st: numbers("st"),
            cnt: numbers("cnt"),
            step: numbers("step"),
        }
    }
}

/// One statement of a layout definition.
#[derive(Clone, Debug)]
enum Statement {
    Node(Box<LayoutNode>),
    ForEach(ForEach),
    Choose(Choose),
}

/// A layout node: a shape, an algorithm, and what it presents.
#[derive(Clone, Debug, Default)]
struct LayoutNode {
    name: String,
    style_label: String,
    algorithm: Option<Algorithm>,
    shape: Option<ShapeSpec>,
    presents: Option<Selection>,
    constraints: Vec<Constraint>,
    rules: Vec<Rule>,
    vars: HashMap<String, String>,
    children: Vec<Statement>,
}

#[derive(Clone, Debug, Default)]
struct Algorithm {
    kind: String,
    params: HashMap<String, String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ShapeSpec {
    pub preset: String,
    pub hidden: bool,
    pub z_order: i64,
}

#[derive(Clone, Debug, Default)]
struct ForEach {
    name: String,
    /// The name of another `forEach` whose walk and statements this one
    /// repeats: how a hierarchy goes down another level.
    reference: String,
    selection: Selection,
    hide_last_transition: bool,
    children: Vec<Statement>,
}

#[derive(Clone, Debug, Default)]
struct Choose {
    branches: Vec<(Condition, Vec<Statement>)>,
    otherwise: Vec<Statement>,
}

#[derive(Clone, Debug, Default)]
struct Condition {
    selection: Selection,
    function: String,
    argument: String,
    operator: String,
    value: String,
}

/// A constraint: what is set, on whom, from what.
#[derive(Clone, Debug, Default)]
struct Constraint {
    kind: String,
    target: String,
    target_name: String,
    target_pt_type: String,
    reference: String,
    reference_target: String,
    reference_name: String,
    reference_pt_type: String,
    operator: String,
    value: Option<f64>,
    factor: f64,
}

/// A rule: what may give way when the words do not fit, and how far. Of
/// its parts, the type and the floor are what is followed: a font that may
/// shrink shrinks to the floor the rule states.
#[derive(Clone, Debug, Default)]
struct Rule {
    kind: String,
    value: Option<f64>,
}

/// A layout definition, read.
#[derive(Clone, Debug)]
pub(crate) struct Definition {
    root: LayoutNode,
    pub vars: HashMap<String, String>,
    /// Every named `forEach`, for the ones that refer to one by name.
    named: HashMap<String, ForEach>,
}

impl Definition {
    pub(crate) fn read(root: &Element) -> Option<Self> {
        let node = root
            .child_elements()
            .find(|child| child.local_name() == "layoutNode")
            .map(read_layout_node)?;
        let vars = node.vars.clone();
        let mut named = HashMap::new();
        gather_named(&node.children, &mut named);
        Some(Self { root: node, vars, named })
    }
}

fn gather_named(statements: &[Statement], out: &mut HashMap<String, ForEach>) {
    for statement in statements {
        match statement {
            Statement::Node(node) => gather_named(&node.children, out),
            Statement::ForEach(each) => {
                if !each.name.is_empty() && each.reference.is_empty() {
                    out.insert(each.name.clone(), each.clone());
                }
                gather_named(&each.children, out);
            }
            Statement::Choose(choose) => {
                for (_, statements) in &choose.branches {
                    gather_named(statements, out);
                }
                gather_named(&choose.otherwise, out);
            }
        }
    }
}

fn read_statements(parent: &Element) -> Vec<Statement> {
    parent
        .child_elements()
        .filter_map(|child| match child.local_name() {
            "layoutNode" => Some(Statement::Node(Box::new(read_layout_node(child)))),
            "forEach" => Some(Statement::ForEach(ForEach {
                name: child.attribute(None, "name").unwrap_or("").to_owned(),
                reference: child.attribute(None, "ref").unwrap_or("").to_owned(),
                selection: Selection::read(child, "self"),
                hide_last_transition: matches!(
                    child.attribute(None, "hideLastTrans"),
                    None | Some("1" | "true")
                ),
                children: read_statements(child),
            })),
            "choose" => {
                let mut choose = Choose::default();
                for branch in child.child_elements() {
                    match branch.local_name() {
                        "if" => choose.branches.push((
                            Condition {
                                selection: Selection::read(branch, "self"),
                                function: branch.attribute(None, "func").unwrap_or("").to_owned(),
                                argument: branch.attribute(None, "arg").unwrap_or("").to_owned(),
                                operator: branch.attribute(None, "op").unwrap_or("equ").to_owned(),
                                value: branch.attribute(None, "val").unwrap_or("").to_owned(),
                            },
                            read_statements(branch),
                        )),
                        "else" => choose.otherwise = read_statements(branch),
                        _ => {}
                    }
                }
                Some(Statement::Choose(choose))
            }
            _ => None,
        })
        .collect()
}

fn read_layout_node(element: &Element) -> LayoutNode {
    let number = |element: &Element, name: &str| -> Option<f64> {
        element.attribute(None, name).and_then(|value| value.trim().parse().ok())
    };
    let mut node = LayoutNode {
        name: element.attribute(None, "name").unwrap_or("").to_owned(),
        style_label: element.attribute(None, "styleLbl").unwrap_or("").to_owned(),
        ..LayoutNode::default()
    };
    for child in element.child_elements() {
        match child.local_name() {
            "alg" => {
                let mut algorithm = Algorithm {
                    kind: child.attribute(None, "type").unwrap_or("").to_owned(),
                    params: HashMap::new(),
                };
                for param in child.child_elements().filter(|p| p.local_name() == "param") {
                    if let (Some(kind), Some(value)) =
                        (param.attribute(None, "type"), param.attribute(None, "val"))
                    {
                        algorithm.params.insert(kind.to_owned(), value.to_owned());
                    }
                }
                node.algorithm = Some(algorithm);
            }
            "shape" => {
                node.shape = Some(ShapeSpec {
                    preset: child.attribute(None, "type").unwrap_or("").to_owned(),
                    hidden: matches!(child.attribute(None, "hideGeom"), Some("1" | "true")),
                    z_order: child
                        .attribute(None, "zOrderOff")
                        .and_then(|value| value.parse().ok())
                        .unwrap_or(0),
                });
            }
            "presOf" => node.presents = Some(Selection::read(child, "self")),
            "varLst" => {
                for var in child.child_elements() {
                    if let Some(value) = var.attribute(None, "val") {
                        node.vars.insert(var.local_name().to_owned(), value.to_owned());
                    }
                }
            }
            "constrLst" => {
                for constraint in child.child_elements().filter(|c| c.local_name() == "constr") {
                    let word = |name: &str, fallback: &str| {
                        constraint.attribute(None, name).unwrap_or(fallback).to_owned()
                    };
                    node.constraints.push(Constraint {
                        kind: word("type", ""),
                        target: word("for", "self"),
                        target_name: word("forName", ""),
                        target_pt_type: word("ptType", "all"),
                        reference: word("refType", ""),
                        reference_target: word("refFor", "self"),
                        reference_name: word("refForName", ""),
                        reference_pt_type: word("refPtType", "all"),
                        operator: word("op", "none"),
                        value: number(constraint, "val"),
                        factor: number(constraint, "fact").unwrap_or(1.0),
                    });
                }
            }
            "ruleLst" => {
                for rule in child.child_elements().filter(|c| c.local_name() == "rule") {
                    node.rules.push(Rule {
                        kind: rule.attribute(None, "type").unwrap_or("").to_owned(),
                        value: number(rule, "val"),
                    });
                }
            }
            _ => {}
        }
    }
    node.children = read_statements(element);
    node
}

/// One node of the presentation tree: a layout node as it was made for one
/// point of the model, with everything worked out for it.
#[derive(Clone, Debug, Default)]
pub(crate) struct Laid {
    pub name: String,
    pub style_label: String,
    pub shape: Option<ShapeSpec>,
    /// The points it presents: the words drawn in its shape.
    pub presents: Vec<usize>,
    /// The point it was made for, which is what its colour is counted by.
    pub point: usize,
    algorithm: Option<Algorithm>,
    constraints: Vec<Constraint>,
    rules: Vec<Rule>,
    /// The values its constraints came to, by type.
    pub values: HashMap<String, f64>,
    /// Which of its sizes are to be shared out equally with its siblings'.
    equal: Vec<String>,
    /// Where it stands, in English metric units from the diagram's corner.
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// How big it came to before it was fitted into its room, for the
    /// arrangements that scale as a whole.
    natural_width: f64,
    natural_height: f64,
    /// The two ends of a connector drawn as a line, from the parent's foot
    /// to the child's head.
    line: Option<((f64, f64), (f64, f64))>,
    /// Turned, in degrees clockwise.
    pub rotation: f64,
    /// The font the words are drawn at, in points, once fitted.
    pub font_size: f64,
    pub children: Vec<Laid>,
    /// Which of the forEach's points this was, from nought, and how many
    /// there were: what a colour list counts by.
    pub index: usize,
    pub count: usize,
}

/// Where a presentation node is drawn from, for the drawing part and the
/// group alike: one shape with its words.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Drawn {
    pub point_id: String,
    pub preset: String,
    pub style_label: String,
    pub x: i64,
    pub y: i64,
    pub width: i64,
    pub height: i64,
    pub rotation: f64,
    /// The words, one entry per paragraph, with the paragraph's level.
    pub text: Vec<(u8, String)>,
    pub font_size: f64,
    /// Where the words go, when it is not the shape's own rectangle.
    pub text_rect: Option<(i64, i64, i64, i64)>,
    /// The room left between the shape's edge and its words, left and
    /// right.
    pub margins: (i64, i64),
    pub horizontal: String,
    pub z_order: i64,
    /// The node's place among the ones made by the same walk, and how many
    /// there were: what a colour list counts by.
    pub index: usize,
    pub count: usize,
}

/// A point-and-a-half of margin, which is what a box leaves round its words
/// when the layout says nothing.
const DEFAULT_MARGIN: f64 = EMU_PER_INCH as f64 * 0.05;
/// A point, in English metric units.
const EMU_PER_POINT: f64 = 12700.0;

/// Lays a model out by a definition inside a rectangle, and says what to
/// draw.
pub(crate) fn lay_out(
    definition: &Definition,
    model: &Model,
    width: i64,
    height: i64,
) -> Vec<Drawn> {
    let Some(root_point) = model.root() else { return Vec::new() };
    let mut vars = model.vars.clone();
    for (name, value) in &definition.vars {
        vars.entry(name.clone()).or_insert_with(|| value.clone());
    }
    let context = Context { model, vars: &vars, named: &definition.named };
    let mut tree = context.expand(&definition.root, root_point, 0, 1, 0);
    tree.width = width as f64;
    tree.height = height as f64;
    let mut engine = Engine { model, vars: &vars };
    engine.size(&mut tree);
    engine.place(&mut tree, 0.0, 0.0, width as f64, height as f64);
    // The words are fitted last, into the room each box came to have.
    engine.fit_all(&mut tree);

    let mut out = Vec::new();
    collect(&tree, model, &mut out);
    out.sort_by_key(|drawn| drawn.z_order);
    out
}

/// What a walk over the definition needs at hand.
struct Context<'a> {
    model: &'a Model,
    vars: &'a HashMap<String, String>,
    named: &'a HashMap<String, ForEach>,
}

impl Context<'_> {
    /// Makes the presentation node of a layout node for one point.
    fn expand(
        &self,
        node: &LayoutNode,
        point: usize,
        index: usize,
        count: usize,
        depth: usize,
    ) -> Laid {
        let presents = node
            .presents
            .as_ref()
            .map(|selection| self.model.select(point, selection))
            .unwrap_or_default();
        let mut laid = Laid {
            name: node.name.clone(),
            style_label: node.style_label.clone(),
            shape: node.shape.clone(),
            presents,
            point,
            algorithm: node.algorithm.clone(),
            constraints: node.constraints.clone(),
            rules: node.rules.clone(),
            index,
            count,
            ..Laid::default()
        };
        laid.children = self.expand_all(&node.children, point, depth + 1);
        laid
    }

    fn expand_all(&self, statements: &[Statement], point: usize, depth: usize) -> Vec<Laid> {
        if depth > 64 {
            return Vec::new();
        }
        let mut out = Vec::new();
        for statement in statements {
            match statement {
                Statement::Node(node) => out.push(self.expand(node, point, 0, 1, depth)),
                Statement::ForEach(each) => {
                    // One that names another walks as that one walks.
                    let each = match self.named.get(&each.reference) {
                        Some(named) if !each.reference.is_empty() => named,
                        _ => each,
                    };
                    let mut points = self.model.select(point, &each.selection);
                    // The transition after the last point stands between it
                    // and nothing, and is not drawn unless asked for.
                    if each.hide_last_transition
                        && each.selection.pt_type.iter().any(|kind| kind == "sibTrans")
                    {
                        let model = self.model;
                        points.retain(|transition| {
                            model
                                .owner
                                .get(transition)
                                .is_none_or(|owner| !model.along(*owner, "followSib").is_empty())
                        });
                    }
                    let count = points.len();
                    for (index, each_point) in points.into_iter().enumerate() {
                        for statement in &each.children {
                            match statement {
                                Statement::Node(node) => {
                                    out.push(self.expand(node, each_point, index, count, depth));
                                }
                                other => out.extend(self.expand_all(
                                    std::slice::from_ref(other),
                                    each_point,
                                    depth + 1,
                                )),
                            }
                        }
                    }
                }
                Statement::Choose(choose) => {
                    let chosen = choose
                        .branches
                        .iter()
                        .find(|(condition, _)| self.holds(condition, point))
                        .map(|(_, statements)| statements)
                        .unwrap_or(&choose.otherwise);
                    out.extend(self.expand_all(chosen, point, depth + 1));
                }
            }
        }
        out
    }

    /// Whether a condition holds for a point.
    fn holds(&self, condition: &Condition, point: usize) -> bool {
        let selected = self.model.select(point, &condition.selection);
        let compare = |left: f64| -> bool {
            let right: f64 = condition.value.trim().parse().unwrap_or(0.0);
            match condition.operator.as_str() {
                "gte" => left >= right,
                "gt" => left > right,
                "lte" => left <= right,
                "lt" => left < right,
                "neq" => (left - right).abs() > f64::EPSILON,
                _ => (left - right).abs() < f64::EPSILON,
            }
        };
        let truth = |held: bool| -> bool {
            let wanted = matches!(condition.value.as_str(), "1" | "true");
            match condition.operator.as_str() {
                "neq" => held != wanted,
                _ => held == wanted,
            }
        };
        match condition.function.as_str() {
            "cnt" => compare(selected.len() as f64),
            "depth" => {
                let at = selected.first().copied().unwrap_or(point);
                compare(self.model.depth_of(at) as f64)
            }
            "maxDepth" => {
                let at = selected.first().copied().unwrap_or(point);
                compare(self.model.max_depth_under(at) as f64)
            }
            "pos" => {
                let at = selected.first().copied().unwrap_or(point);
                compare(self.model.position_among_siblings(at) as f64 + 1.0)
            }
            "revPos" => {
                let at = selected.first().copied().unwrap_or(point);
                let count = self.model.siblings(at).len();
                compare((count - self.model.position_among_siblings(at)) as f64)
            }
            "posEven" => {
                let at = selected.first().copied().unwrap_or(point);
                truth((self.model.position_among_siblings(at) + 1) % 2 == 0)
            }
            "posOdd" => {
                let at = selected.first().copied().unwrap_or(point);
                truth((self.model.position_among_siblings(at) + 1) % 2 == 1)
            }
            "var" => {
                let value = self.vars.get(&condition.argument).map(String::as_str).unwrap_or("");
                match condition.operator.as_str() {
                    "neq" => value != condition.value,
                    _ => value == condition.value,
                }
            }
            _ => false,
        }
    }
}

/// What the sizing and placing needs at hand.
struct Engine<'a> {
    model: &'a Model,
    vars: &'a HashMap<String, String>,
}

/// The directions a linear arrangement runs in.
fn direction_of(params: &HashMap<String, String>, name: &str, fallback: &str) -> String {
    params.get(name).cloned().unwrap_or_else(|| fallback.to_owned())
}

impl Engine<'_> {
    /// Works out every node's size, bottom up, from its constraints and
    /// its algorithm.
    fn size(&mut self, node: &mut Laid) {
        // A node that is not the root starts as big as the room it is given,
        // and its constraints narrow that.
        for child in &mut node.children {
            if child.width <= 0.0 {
                child.width = node.width;
            }
            if child.height <= 0.0 {
                child.height = node.height;
            }
        }
        self.apply_constraints(node);
        for child in &mut node.children {
            self.size(child);
        }
        let kind = node.algorithm.as_ref().map(|a| a.kind.clone()).unwrap_or_default();
        match kind.as_str() {
            "hierChild" | "hierRoot" => self.measure_hierarchy(node),
            _ => {
                node.natural_width = node.width;
                node.natural_height = node.height;
            }
        }
    }

    /// Fits the words of every text node, once everything is placed.
    fn fit_all(&self, node: &mut Laid) {
        self.fit_text(node);
        for child in &mut node.children {
            self.fit_all(child);
        }
    }

    /// Sets the values a node's constraints ask for, on the node and on its
    /// children.
    fn apply_constraints(&self, node: &mut Laid) {
        let constraints = node.constraints.clone();
        for constraint in &constraints {
            let value = self.constraint_value(node, constraint);
            let unit_fraction = self.unit_fraction(node, constraint);
            let targets = target_indices(
                node,
                &constraint.target,
                &constraint.target_name,
                &constraint.target_pt_type,
                self.model,
            );
            for target in targets {
                let laid: &mut Laid = match target {
                    Target::Own => node,
                    Target::Child(index) => &mut node.children[index],
                    Target::Deep(path) => {
                        let mut at: &mut Laid = node;
                        for step in path {
                            at = &mut at.children[step];
                        }
                        at
                    }
                };
                if constraint.operator == "equ"
                    && constraint.reference.is_empty()
                    && constraint.value.is_none()
                {
                    laid.equal.push(constraint.kind.clone());
                    continue;
                }
                // A size stated as a fraction of a sibling's that is itself
                // being shared out is a fraction of the share: it is solved
                // with the shares rather than from a number not yet known.
                if let Some(fraction) = unit_fraction {
                    laid.values.insert(format!("{}:unit", constraint.kind), fraction);
                    continue;
                }
                let Some(value) = value else { continue };
                let entry = laid.values.entry(constraint.kind.clone()).or_insert(value);
                match constraint.operator.as_str() {
                    "gte" => *entry = entry.max(value),
                    "lte" => *entry = entry.min(value),
                    _ => *entry = value,
                }
                match constraint.kind.as_str() {
                    "w" => laid.width = laid.values["w"],
                    "h" => laid.height = laid.values["h"],
                    _ => {}
                }
            }
        }
    }

    /// What a constraint comes to: a stated value, or a fraction of what it
    /// refers to.
    fn constraint_value(&self, node: &Laid, constraint: &Constraint) -> Option<f64> {
        if constraint.reference.is_empty() {
            return constraint.value.map(|value| match constraint.kind.as_str() {
                // Font sizes are said in points; everything else in units.
                "primFontSz" | "secFontSz" => value,
                _ => value,
            });
        }
        let referenced = target_indices(
            node,
            &constraint.reference_target,
            &constraint.reference_name,
            &constraint.reference_pt_type,
            self.model,
        );
        let laid = referenced.into_iter().next().map(|target| match target {
            Target::Own => node,
            Target::Child(index) => &node.children[index],
            Target::Deep(path) => {
                let mut at = node;
                for step in path {
                    at = &at.children[step];
                }
                at
            }
        })?;
        let base = match constraint.reference.as_str() {
            "w" => Some(laid.values.get("w").copied().unwrap_or(laid.width)),
            "h" => Some(laid.values.get("h").copied().unwrap_or(laid.height)),
            other => laid.values.get(other).copied(),
        }?;
        // A font size is in points and everything else in units, so a
        // margin said as a fraction of the font is turned into units.
        let from_font = matches!(constraint.reference.as_str(), "primFontSz" | "secFontSz");
        let to_font = matches!(constraint.kind.as_str(), "primFontSz" | "secFontSz");
        let scale = if from_font && !to_font { EMU_PER_POINT } else { 1.0 };
        Some(base * constraint.factor * scale)
    }

    /// The fraction of a shared-out size a constraint asks for, when what it
    /// refers to is a child whose size is to be shared out equally with its
    /// siblings' rather than stated.
    fn unit_fraction(&self, node: &Laid, constraint: &Constraint) -> Option<f64> {
        if constraint.reference.is_empty() || !matches!(constraint.reference.as_str(), "w" | "h") {
            return None;
        }
        let referenced = target_indices(
            node,
            &constraint.reference_target,
            &constraint.reference_name,
            &constraint.reference_pt_type,
            self.model,
        );
        let Some(Target::Child(index)) = referenced.into_iter().next() else { return None };
        let child = &node.children[index];
        let shared = child.equal.contains(&constraint.reference)
            || child.values.contains_key(&format!("{}:unit", constraint.reference));
        shared.then_some(constraint.factor)
    }

    /// The natural size of a hierarchy, which is the size its parts add up
    /// to before it is scaled to fit.
    fn measure_hierarchy(&mut self, node: &mut Laid) {
        let kind = node.algorithm.as_ref().map(|a| a.kind.clone()).unwrap_or_default();
        let spacing = node.values.get("sp").copied().unwrap_or(node.height * 0.1);
        let sibling_spacing = node.values.get("sibSp").copied().unwrap_or(node.width * 0.1);
        if kind == "hierRoot" {
            // The root's own shape over its children: as wide as the wider
            // and as tall as both with the space between.
            let (mut width, mut height) = (0.0f64, 0.0f64);
            for child in &node.children {
                width = width.max(child.width);
                if height > 0.0 {
                    height += spacing;
                }
                height += child.height;
            }
            node.width = width.max(1.0);
            node.height = height.max(1.0);
            node.natural_width = node.width;
            node.natural_height = node.height;
        } else {
            // The children in a row: as wide as all of them with the gaps,
            // as tall as the tallest. The lines down to them take no room
            // of their own.
            let (mut width, mut height) = (0.0f64, 0.0f64);
            for child in node.children.iter().filter(|child| !is_connector(child)) {
                if width > 0.0 {
                    width += sibling_spacing;
                }
                width += child.width;
                height = height.max(child.height);
            }
            node.width = width.max(1.0);
            node.height = height.max(1.0);
            node.natural_width = node.width;
            node.natural_height = node.height;
        }
    }

    /// Fits the words of a text node into it by shrinking the font as far
    /// as the rules allow.
    fn fit_text(&self, node: &mut Laid) {
        let algorithm = node.algorithm.as_ref().map(|a| a.kind.as_str()).unwrap_or("");
        if algorithm != "tx" || node.presents.is_empty() {
            return;
        }
        let largest = node.values.get("primFontSz").copied().unwrap_or(18.0);
        let floor = node
            .rules
            .iter()
            .find(|rule| rule.kind == "primFontSz")
            .and_then(|rule| rule.value)
            .unwrap_or(largest);
        let left = node.values.get("lMarg").copied().unwrap_or(DEFAULT_MARGIN);
        let right = node.values.get("rMarg").copied().unwrap_or(DEFAULT_MARGIN);
        let top = node.values.get("tMarg").copied().unwrap_or(DEFAULT_MARGIN);
        let bottom = node.values.get("bMarg").copied().unwrap_or(DEFAULT_MARGIN);
        // A shape that narrows leaves less room across than its box: a
        // triangle half of it, a trapezoid most of it.
        let across = match node.shape.as_ref().map(|shape| shape.preset.as_str()) {
            Some("triangle") => 0.45,
            Some("trapezoid") => 0.75,
            Some("ellipse") => 0.8,
            _ => 1.0,
        };
        let room_width = (node.width * across - left - right).max(1.0);
        let room_height = (node.height - top - bottom).max(1.0);
        let text: Vec<String> = node
            .presents
            .iter()
            .flat_map(|point| self.model.points[*point].text.iter().map(|(_, line)| line.clone()))
            .collect();
        let mut size = largest;
        while size > floor && !fits(&text, size, room_width, room_height) {
            size = (size - 1.0).max(floor);
        }
        node.font_size = size;
    }

    /// Places a node and everything under it inside a rectangle, by the
    /// node's algorithm.
    fn place(&mut self, node: &mut Laid, x: f64, y: f64, width: f64, height: f64) {
        node.x = x;
        node.y = y;
        node.width = width;
        node.height = height;
        let kind = node.algorithm.as_ref().map(|a| a.kind.clone()).unwrap_or_default();
        let params = node.algorithm.as_ref().map(|a| a.params.clone()).unwrap_or_default();
        match kind.as_str() {
            "lin" => self.place_linear(node, &params),
            "composite" => self.place_composite(node),
            "hierRoot" => self.place_hierarchy_root(node),
            "hierChild" => self.place_hierarchy_children(node, &params),
            "cycle" => self.place_cycle(node, &params),
            "snake" => self.place_snake(node, &params),
            "pyra" => self.place_pyramid(node),
            _ => {
                // A shape, a spacer, a text node or a connector holds its
                // children, if it has any, where it is.
                for child in &mut node.children {
                    self.place(child, x, y, width, height);
                }
            }
        }
    }

    /// Children placed where their constraints put them, over the node.
    fn place_composite(&mut self, node: &mut Laid) {
        let (x, y, width, height) = (node.x, node.y, node.width, node.height);
        let mut children = std::mem::take(&mut node.children);
        for child in &mut children {
            let w = child.values.get("w").copied().unwrap_or(child.width.min(width).max(1.0));
            let h = child.values.get("h").copied().unwrap_or(child.height.min(height).max(1.0));
            let left = child
                .values
                .get("l")
                .copied()
                .or_else(|| child.values.get("ctrX").map(|centre| centre - w / 2.0))
                .or_else(|| child.values.get("r").map(|right| right - w))
                .unwrap_or(0.0);
            let top = child
                .values
                .get("t")
                .copied()
                .or_else(|| child.values.get("ctrY").map(|centre| centre - h / 2.0))
                .or_else(|| child.values.get("b").map(|bottom| bottom - h))
                .unwrap_or(0.0);
            self.place(child, x + left, y + top, w, h);
        }
        node.children = children;
    }

    /// Children one after another along a line, sharing the room out.
    fn place_linear(&mut self, node: &mut Laid, params: &HashMap<String, String>) {
        let direction = direction_of(params, "linDir", "fromL");
        let across = matches!(direction.as_str(), "fromL" | "fromR");
        let (x, y, width, height) = (node.x, node.y, node.width, node.height);
        let (room, cross) = if across { (width, height) } else { (height, width) };
        let spacing = node.values.get("sp").copied().unwrap_or(0.0);
        let mut children = std::mem::take(&mut node.children);
        if children.is_empty() {
            return;
        }

        // Every child's size along the line is a fixed part plus a share of
        // one unit — the unit being what an "equal" width comes to — and the
        // shares are solved together so the row fills the room exactly. A
        // child stated as a fraction of a node's width takes that fraction of
        // the unit.
        let main = if across { "w" } else { "h" };
        let mut fixed = 0.0f64;
        let mut shares = 0.0f64;
        let mut terms: Vec<(f64, f64)> = Vec::new();
        for child in &children {
            let stated = child.values.get(main).copied();
            let equal = child.equal.iter().any(|kind| kind == main);
            let fraction = child.values.get(&format!("{main}:unit")).copied();
            let term = match (fraction, equal, stated) {
                (Some(fraction), _, _) => (0.0, fraction),
                (None, true, _) | (None, false, None) => (0.0, 1.0),
                (None, false, Some(value)) => (value, 0.0),
            };
            fixed += term.0;
            shares += term.1;
            terms.push(term);
        }
        let gaps = spacing * (children.len() as f64 - 1.0);
        let unit = if shares > 0.0 { ((room - gaps - fixed) / shares).max(0.0) } else { 0.0 };
        let mut sizes: Vec<f64> = terms.iter().map(|(a, b)| a + b * unit).collect();
        let total: f64 = sizes.iter().sum::<f64>() + gaps;
        // Fixed sizes that overflow the room are scaled down together.
        let scale = if total > room && total > 0.0 { room / total } else { 1.0 };
        for size in &mut sizes {
            *size *= scale;
        }
        let spacing = spacing * scale;

        let mut along = 0.0f64;
        for (child, size) in children.iter_mut().zip(sizes) {
            let cross_size = child
                .values
                .get(if across { "h" } else { "w" })
                .copied()
                .unwrap_or(cross)
                .min(cross);
            let cross_offset = (cross - cross_size) / 2.0;
            let (cx, cy, cw, ch) = match direction.as_str() {
                "fromR" => (x + room - along - size, y + cross_offset, size, cross_size),
                "fromT" => (x + cross_offset, y + along, cross_size, size),
                "fromB" => (x + cross_offset, y + room - along - size, cross_size, size),
                _ => (x + along, y + cross_offset, size, cross_size),
            };
            self.place(child, cx, cy, cw, ch);
            along += size + spacing;
        }
        node.children = children;
    }

    /// A root over its children, scaled as a whole to fit its room.
    fn place_hierarchy_root(&mut self, node: &mut Laid) {
        let (x, y, width, height) = (node.x, node.y, node.width, node.height);
        let natural_width = node.natural_width.max(1.0);
        let natural_height = node.natural_height.max(1.0);
        let scale = (width / natural_width).min(height / natural_height).max(0.01);
        let spacing = node.values.get("sp").copied().unwrap_or(natural_height * 0.1) * scale;
        let mut children = std::mem::take(&mut node.children);
        let total: f64 = children.iter().map(|child| child.natural_height * scale).sum::<f64>()
            + spacing * (children.len() as f64 - 1.0).max(0.0);
        let mut top = y + (height - total).max(0.0) / 2.0;
        for child in &mut children {
            let w = child.natural_width * scale;
            let h = child.natural_height * scale;
            // The row of children is told how far above it the foot of the
            // box stands, for the lines it draws up to it.
            child.values.insert("spAbove".to_owned(), spacing);
            self.place(child, x + (width - w) / 2.0, top, w, h);
            top += h + spacing;
        }
        node.children = children;
    }

    /// Children in a row under their parent.
    fn place_hierarchy_children(&mut self, node: &mut Laid, params: &HashMap<String, String>) {
        let (x, y, width, height) = (node.x, node.y, node.width, node.height);
        let natural_width = node.natural_width.max(1.0);
        let scale = (width / natural_width).min(height / node.natural_height.max(1.0)).max(0.01);
        let spacing = node.values.get("sibSp").copied().unwrap_or(natural_width * 0.05) * scale;
        let mut children = std::mem::take(&mut node.children);
        let placed = children.iter().filter(|child| !is_connector(child)).count();
        let total: f64 = children
            .iter()
            .filter(|child| !is_connector(child))
            .map(|child| child.natural_width * scale)
            .sum::<f64>()
            + spacing * (placed as f64 - 1.0).max(0.0);
        let right_to_left = direction_of(params, "chDir", "horz") == "horz"
            && self.vars.get("dir").map(String::as_str) == Some("rev");
        let mut left = x + (width - total).max(0.0) / 2.0;
        let ordered: Vec<&mut Laid> = if right_to_left {
            children.iter_mut().filter(|child| !is_connector(child)).rev().collect()
        } else {
            children.iter_mut().filter(|child| !is_connector(child)).collect()
        };
        for child in ordered {
            let w = child.natural_width * scale;
            let h = (child.natural_height * scale).min(height);
            self.place(child, left, y, w, h);
            left += w + spacing;
        }

        // The line from the parent above to each child: down out of the
        // parent's foot, which is where the row begins, across, and down
        // into the head of the child's own box. A connector presents the
        // transition into a child, and the child's box is the first shape
        // of the subtree made for that child.
        let parent_x = x + width / 2.0;
        let spacing_above = node.values.get("spAbove").copied().unwrap_or(0.0);
        let model = self.model;
        let owners: Vec<(usize, Option<usize>)> = children
            .iter()
            .enumerate()
            .map(|(index, child)| {
                let owner = child
                    .presents
                    .first()
                    .and_then(|transition| model.owner.get(transition).copied());
                (index, owner)
            })
            .collect();
        for (index, owner) in owners {
            let Some(owner) = owner.filter(|_| is_connector(&children[index])) else { continue };
            let Some(head) = children
                .iter()
                .filter(|child| !is_connector(child))
                .find(|child| child.point == owner)
                .and_then(first_shape)
            else {
                continue;
            };
            children[index].line = Some(((parent_x, y - spacing_above), (head.0, head.1)));
        }
        node.children = children;
    }

    /// Children round a circle.
    fn place_cycle(&mut self, node: &mut Laid, params: &HashMap<String, String>) {
        let (x, y, width, height) = (node.x, node.y, node.width, node.height);
        let start: f64 = params.get("stAng").and_then(|v| v.parse().ok()).unwrap_or(0.0);
        let span: f64 = params.get("spanAng").and_then(|v| v.parse().ok()).unwrap_or(360.0);
        let centre_first = params.get("ctrShpMap").map(String::as_str) == Some("fNode");
        let mut children = std::mem::take(&mut node.children);
        if children.is_empty() {
            return;
        }
        let centre_x = x + width / 2.0;
        let centre_y = y + height / 2.0;

        // What goes round: every child, or every child but the first, which
        // stands in the middle.
        let first_in_middle = centre_first && children.len() > 1;
        let round = children.len() - usize::from(first_in_middle);
        // Each child's size: what it asked for, shared out equally when it
        // asked for nothing, and never more than fits.
        let mut sizes: Vec<(f64, f64)> = children
            .iter()
            .map(|child| {
                let w =
                    child.values.get("w").copied().unwrap_or(width / (round as f64 + 1.0).max(2.0));
                let h = child.values.get("h").copied().unwrap_or(w * 0.6);
                (w, h)
            })
            .collect();
        let widest = sizes.iter().map(|(w, _)| *w).fold(0.0, f64::max);
        let tallest = sizes.iter().map(|(_, h)| *h).fold(0.0, f64::max);
        let radius = node
            .values
            .get("diam")
            .map(|diameter| diameter / 2.0)
            .unwrap_or_else(|| ((width - widest) / 2.0).min((height - tallest) / 2.0).max(1.0));
        // A circle of shapes bigger than its room shrinks to it.
        let scale = ((width / 2.0) / (radius + widest / 2.0))
            .min((height / 2.0) / (radius + tallest / 2.0))
            .min(1.0);
        let radius = radius * scale;
        for size in &mut sizes {
            size.0 *= scale;
            size.1 *= scale;
        }

        let full = (span - 360.0).abs() < f64::EPSILON;
        let step = if round > 0 {
            if full {
                span / round as f64
            } else {
                span / (round as f64 - 1.0).max(1.0)
            }
        } else {
            0.0
        };
        let mut around = 0usize;
        for (index, child) in children.iter_mut().enumerate() {
            let (w, h) = sizes[index];
            if first_in_middle && index == 0 {
                self.place(child, centre_x - w / 2.0, centre_y - h / 2.0, w, h);
                continue;
            }
            let angle = (start + step * around as f64 - 90.0).to_radians();
            let cx = centre_x + radius * angle.cos();
            let cy = centre_y + radius * angle.sin();
            self.place(child, cx - w / 2.0, cy - h / 2.0, w, h);
            // A connector round the circle points the way round: it is
            // turned to lie along the tangent where it stands.
            let model = self.model;
            let is_connector = child.algorithm.as_ref().is_some_and(|a| a.kind == "conn")
                || child
                    .presents
                    .iter()
                    .any(|point| model.points[*point].kind == PointKind::SiblingTransition);
            if is_connector {
                child.rotation = start + step * around as f64;
            }
            around += 1;
        }
        node.children = children;
    }

    /// Children in rows that turn back on themselves.
    fn place_snake(&mut self, node: &mut Laid, params: &HashMap<String, String>) {
        let (x, y, width, height) = (node.x, node.y, node.width, node.height);
        let reverse_rows = params.get("contDir").map(String::as_str) == Some("revDir");
        let fixed: Option<usize> = match params.get("bkpt").map(String::as_str) {
            Some("fixed") => params.get("bkPtFixedVal").and_then(|v| v.parse().ok()),
            _ => None,
        };
        let mut children = std::mem::take(&mut node.children);
        // The shapes, and the spacers or connectors between them, which take
        // the room between two shapes in a row and none at a row's end.
        let model = self.model;
        let is_between = |child: &Laid| {
            child.presents.is_empty()
                || child
                    .presents
                    .iter()
                    .all(|point| model.points[*point].kind == PointKind::SiblingTransition)
        };
        let shapes: Vec<usize> =
            children.iter().enumerate().filter(|(_, c)| !is_between(c)).map(|(i, _)| i).collect();
        if shapes.is_empty() {
            node.children = children;
            return;
        }
        let count = shapes.len();
        let box_width = children[shapes[0]].values.get("w").copied().unwrap_or(width / 3.0);
        let box_height = children[shapes[0]].values.get("h").copied().unwrap_or(box_width * 0.6);
        let gap = children
            .iter()
            .find(|c| is_between(c))
            .and_then(|c| c.values.get("w").copied())
            .unwrap_or(box_width * 0.15);
        let row_gap = node.values.get("sp").copied().unwrap_or(box_height * 0.25);

        // How many to a row: what the layout fixes, or whatever count makes
        // the grid fit its room largest.
        let columns = fixed
            .unwrap_or_else(|| {
                let mut best = (1usize, 0.0f64);
                for columns in 1..=count {
                    let rows = count.div_ceil(columns);
                    let grid_width = columns as f64 * box_width + (columns as f64 - 1.0) * gap;
                    let grid_height = rows as f64 * box_height + (rows as f64 - 1.0) * row_gap;
                    let scale = (width / grid_width).min(height / grid_height);
                    if scale > best.1 {
                        best = (columns, scale);
                    }
                }
                best.0
            })
            .clamp(1, count);
        let rows = count.div_ceil(columns);
        let grid_width = columns as f64 * box_width + (columns as f64 - 1.0) * gap;
        let grid_height = rows as f64 * box_height + (rows as f64 - 1.0) * row_gap;
        let scale = (width / grid_width).min(height / grid_height).min(1.0);
        let (box_width, box_height, gap, row_gap) =
            (box_width * scale, box_height * scale, gap * scale, row_gap * scale);
        let left = x + (width - grid_width * scale) / 2.0;
        let top = y + (height - grid_height * scale) / 2.0;

        let mut shape_index = 0usize;
        for child in children.iter_mut() {
            if is_between(child) {
                // Between two shapes in a row: the gap. At a row's end: nothing.
                let previous = shape_index.saturating_sub(1);
                let column = previous % columns;
                let row = previous / columns;
                let ends_row = column + 1 == columns || shape_index == 0 || shape_index >= count;
                if ends_row {
                    child.width = 0.0;
                    child.height = 0.0;
                    child.x = left;
                    child.y = top;
                    continue;
                }
                let reversed = reverse_rows && row % 2 == 1;
                let slot_x = if reversed {
                    left + grid_width * scale - (column as f64 + 1.0) * (box_width + gap)
                } else {
                    left + column as f64 * (box_width + gap) + box_width
                };
                let cy = top + row as f64 * (box_height + row_gap) + box_height / 2.0;
                let h = (box_height * 0.3).max(1.0);
                self.place(child, slot_x, cy - h / 2.0, gap, h);
                if reversed {
                    child.rotation = 180.0;
                }
                continue;
            }
            let column = shape_index % columns;
            let row = shape_index / columns;
            let reversed = reverse_rows && row % 2 == 1;
            let cx = if reversed {
                left + grid_width * scale - (column as f64 + 1.0) * (box_width + gap) + gap
            } else {
                left + column as f64 * (box_width + gap)
            };
            let cy = top + row as f64 * (box_height + row_gap);
            self.place(child, cx, cy, box_width, box_height);
            shape_index += 1;
        }
        node.children = children;
    }

    /// Children as the levels of a pyramid, the first at the top.
    fn place_pyramid(&mut self, node: &mut Laid) {
        let (x, y, width, height) = (node.x, node.y, node.width, node.height);
        let mut children = std::mem::take(&mut node.children);
        let levels = children.len().max(1) as f64;
        let level_height = height / levels;
        for (index, child) in children.iter_mut().enumerate() {
            let top = y + level_height * index as f64;
            // Each level as wide at its foot as the pyramid is there.
            let foot = width * (index as f64 + 1.0) / levels;
            self.place(child, x + (width - foot) / 2.0, top, foot, level_height);
            if let Some(shape) = &mut child.shape {
                if shape.preset.is_empty()
                    || shape.preset == "trapezoid"
                    || shape.preset == "triangle"
                {
                    shape.preset =
                        if index == 0 { "triangle".to_owned() } else { "trapezoid".to_owned() };
                }
            }
            child.values.insert("pyraLevel".to_owned(), index as f64);
            child.values.insert("pyraLevels".to_owned(), levels);
        }
        node.children = children;
    }
}

/// Where a constraint points.
enum Target {
    Own,
    Child(usize),
    Deep(Vec<usize>),
}

/// The nodes a `for`/`forName`/`ptType` names, from a node.
fn target_indices(
    node: &Laid,
    target: &str,
    name: &str,
    pt_type: &str,
    model: &Model,
) -> Vec<Target> {
    let fits = |laid: &Laid| -> bool {
        if !name.is_empty() && laid.name != name {
            return false;
        }
        if pt_type == "all" {
            return true;
        }
        match laid.presents.first().or(Some(&laid.point)) {
            Some(point) => model.matches(*point, pt_type),
            None => true,
        }
    };
    match target {
        "self" => vec![Target::Own],
        "ch" => node
            .children
            .iter()
            .enumerate()
            .filter(|(_, child)| fits(child))
            .map(|(index, _)| Target::Child(index))
            .collect(),
        "des" => {
            let mut out = Vec::new();
            fn walk(
                laid: &Laid,
                path: &mut Vec<usize>,
                fits: &dyn Fn(&Laid) -> bool,
                out: &mut Vec<Target>,
            ) {
                for (index, child) in laid.children.iter().enumerate() {
                    path.push(index);
                    if fits(child) {
                        out.push(Target::Deep(path.clone()));
                    }
                    walk(child, path, fits, out);
                    path.pop();
                }
            }
            walk(node, &mut Vec::new(), &fits, &mut out);
            out
        }
        _ => Vec::new(),
    }
}

/// Whether words fit in a box at a font size, by an estimate of the font:
/// six tenths of an em per character, and a fifth of an em of leading.
fn fits(lines: &[String], size: f64, width: f64, height: f64) -> bool {
    let em = size * EMU_PER_POINT;
    let per_character = em * 0.6;
    let mut rows = 0.0f64;
    for line in lines {
        let needed = line.chars().count() as f64 * per_character;
        rows += (needed / width).ceil().max(1.0);
    }
    rows * em * 1.2 <= height
}

/// How many rows words take in a box at a font size, by the same estimate.
pub(crate) fn rows_of(lines: &[String], size: f64, width: f64) -> f64 {
    let per_character = size * EMU_PER_POINT * 0.6;
    lines
        .iter()
        .map(|line| {
            ((line.chars().count() as f64 * per_character) / width.max(1.0)).ceil().max(1.0)
        })
        .sum()
}

/// Whether a node is a connector, which is drawn between shapes and takes
/// no room among them.
fn is_connector(node: &Laid) -> bool {
    node.algorithm.as_ref().is_some_and(|algorithm| algorithm.kind == "conn")
}

/// The top middle of the first shape in a subtree, which is where a line
/// into it lands.
fn first_shape(node: &Laid) -> Option<(f64, f64)> {
    if node.shape.as_ref().is_some_and(|shape| !shape.preset.is_empty() && shape.preset != "none") {
        return Some((node.x + node.width / 2.0, node.y));
    }
    node.children.iter().find_map(first_shape)
}

/// How thick the line joining a box to the one above it is drawn.
const JOIN: f64 = EMU_PER_INCH as f64 / 72.0;

/// Gathers what is drawn out of a laid-out tree.
fn collect(node: &Laid, model: &Model, out: &mut Vec<Drawn>) {
    // A connector drawn as a line: down out of the parent, across, and down
    // into the child — bars, and as many as the line has straight parts,
    // every one carrying the transition's name.
    if let Some(((from_x, from_y), (to_x, to_y))) = node.line {
        let point_id =
            node.presents.first().map(|point| model.points[*point].id.clone()).unwrap_or_default();
        let middle = (from_y + to_y) / 2.0;
        let bar = |x: f64, y: f64, w: f64, h: f64| Drawn {
            point_id: point_id.clone(),
            preset: "rect".to_owned(),
            style_label: node.style_label.clone(),
            x: x.round() as i64,
            y: y.round() as i64,
            width: w.max(JOIN).round() as i64,
            height: h.max(JOIN).round() as i64,
            rotation: 0.0,
            text: Vec::new(),
            font_size: 0.0,
            text_rect: None,
            margins: (0, 0),
            horizontal: String::new(),
            z_order: -1,
            index: node.index,
            count: node.count,
        };
        out.push(bar(from_x - JOIN / 2.0, from_y, JOIN, middle - from_y));
        let (left, right) = (from_x.min(to_x), from_x.max(to_x));
        out.push(bar(left, middle - JOIN / 2.0, right - left, JOIN));
        out.push(bar(to_x - JOIN / 2.0, middle, JOIN, to_y - middle));
        return;
    }
    if let Some(shape) = &node.shape {
        if !shape.preset.is_empty()
            && shape.preset != "none"
            && node.width > 0.0
            && node.height > 0.0
        {
            let text: Vec<(u8, String)> = node
                .presents
                .iter()
                .flat_map(|point| model.points[*point].text.iter().cloned())
                .collect();
            let point_id = node
                .presents
                .first()
                .or(Some(&node.point))
                .map(|point| model.points[*point].id.clone())
                .unwrap_or_default();
            let horizontal = node
                .algorithm
                .as_ref()
                .and_then(|a| a.params.get("horzAlign").cloned())
                .unwrap_or_else(|| "ctr".to_owned());
            out.push(Drawn {
                point_id,
                preset: shape.preset.clone(),
                style_label: node.style_label.clone(),
                x: node.x.round() as i64,
                y: node.y.round() as i64,
                width: node.width.round() as i64,
                height: node.height.round() as i64,
                rotation: node.rotation,
                text,
                font_size: if node.font_size > 0.0 { node.font_size } else { 12.0 },
                text_rect: None,
                margins: (
                    node.values.get("lMarg").copied().unwrap_or(DEFAULT_MARGIN).round() as i64,
                    node.values.get("rMarg").copied().unwrap_or(DEFAULT_MARGIN).round() as i64,
                ),
                horizontal,
                z_order: shape.z_order,
                index: node.index,
                count: node.count,
            });
        }
    }
    for child in &node.children {
        collect(child, model, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_xml::tree::XmlTree;

    fn model_of(items: &[&str]) -> Model {
        let nodes: Vec<super::super::Node> =
            items.iter().map(|item| super::super::Node::new(item)).collect();
        let xml = super::super::data_model_xml(
            super::super::Arrangement::Process,
            super::super::Colouring::default(),
            &nodes,
            "rId1",
            false,
        );
        let tree = XmlTree::parse(&xml).expect("the model parses");
        Model::read(&tree.root)
    }

    #[test]
    fn a_model_reads_its_points_and_who_hangs_under_whom() {
        let model = model_of(&["One", "Two", "Three"]);
        let root = model.root().expect("a root");
        assert_eq!(model.points[root].kind, PointKind::Doc);
        assert_eq!(model.children[root].len(), 3);
        let first = model.children[root][0];
        assert_eq!(model.points[first].text, vec![(0, "One".to_owned())]);
        assert!(model.sibling_transition[first].is_some());
        assert!(model.parent_transition[first].is_some());
    }

    #[test]
    fn every_axis_reaches_what_it_says() {
        let model = model_of(&["One", "Two", "Three"]);
        let root = model.root().unwrap();
        let middle = model.children[root][1];
        let select = |axis: &str, kind: &str| {
            model.select(
                middle,
                &Selection {
                    axis: vec![axis.to_owned()],
                    pt_type: vec![kind.to_owned()],
                    ..Selection::default()
                },
            )
        };
        assert_eq!(select("self", "node"), vec![middle]);
        assert_eq!(select("par", "doc"), vec![root]);
        assert_eq!(select("followSib", "node").len(), 1);
        assert_eq!(select("precedSib", "node").len(), 1);
        assert_eq!(select("root", "all"), vec![root]);
        assert_eq!(select("ch", "node").len(), 0);
        // The transition after the middle point exists; after the last, none.
        assert_eq!(select("followSib", "sibTrans").len(), 1);
        let last = model.children[root][2];
        assert_eq!(
            model
                .select(
                    last,
                    &Selection {
                        axis: vec!["followSib".to_owned()],
                        pt_type: vec!["sibTrans".to_owned()],
                        ..Selection::default()
                    }
                )
                .len(),
            0
        );
        assert_eq!(
            model
                .select(
                    root,
                    &Selection {
                        axis: vec!["des".to_owned()],
                        pt_type: vec!["node".to_owned()],
                        ..Selection::default()
                    }
                )
                .len(),
            3
        );
    }

    #[test]
    fn a_start_and_a_count_narrow_a_selection() {
        let model = model_of(&["One", "Two", "Three", "Four"]);
        let root = model.root().unwrap();
        let picked = model.select(
            root,
            &Selection {
                axis: vec!["ch".to_owned()],
                pt_type: vec!["node".to_owned()],
                st: vec![2],
                cnt: vec![2],
                step: vec![1],
            },
        );
        assert_eq!(picked.len(), 2);
        assert_eq!(model.points[picked[0]].text[0].1, "Two");
    }

    #[test]
    fn conditions_ask_the_model_and_the_variables() {
        let model = model_of(&["One", "Two"]);
        let mut vars = HashMap::new();
        vars.insert("dir".to_owned(), "rev".to_owned());
        let named = HashMap::new();
        let context = Context { model: &model, vars: &vars, named: &named };
        let root = model.root().unwrap();
        let condition = |function: &str, axis: &str, kind: &str, op: &str, val: &str| Condition {
            selection: Selection {
                axis: vec![axis.to_owned()],
                pt_type: vec![kind.to_owned()],
                ..Selection::default()
            },
            function: function.to_owned(),
            argument: "dir".to_owned(),
            operator: op.to_owned(),
            value: val.to_owned(),
        };
        assert!(context.holds(&condition("cnt", "ch", "node", "equ", "2"), root));
        assert!(context.holds(&condition("cnt", "ch", "node", "gte", "1"), root));
        assert!(!context.holds(&condition("cnt", "ch", "node", "gt", "2"), root));
        assert!(context.holds(&condition("var", "self", "all", "equ", "rev"), root));
        let first = model.children[root][0];
        assert!(context.holds(&condition("pos", "self", "node", "equ", "1"), first));
        assert!(context.holds(&condition("revPos", "self", "node", "equ", "2"), first));
        assert!(context.holds(&condition("posOdd", "self", "node", "equ", "1"), first));
        assert!(context.holds(&condition("depth", "self", "node", "equ", "1"), first));
    }

    #[test]
    fn a_process_definition_lays_boxes_across_with_arrows_between() {
        let items = ["One", "Two", "Three"];
        let model = model_of(&items);
        let xml = super::super::layout_xml(super::super::Arrangement::Process);
        let tree = XmlTree::parse(&xml).expect("the layout parses");
        let definition = Definition::read(&tree.root).expect("a definition");
        let drawn = lay_out(&definition, &model, EMU_PER_INCH * 6, EMU_PER_INCH);
        let boxes: Vec<&Drawn> = drawn.iter().filter(|d| d.preset == "roundRect").collect();
        let arrows: Vec<&Drawn> = drawn.iter().filter(|d| d.preset == "rightArrow").collect();
        assert_eq!(boxes.len(), 3, "{drawn:?}");
        assert_eq!(arrows.len(), 2, "{drawn:?}");
        assert!(boxes[0].x < arrows[0].x && arrows[0].x < boxes[1].x);
        assert_eq!(boxes[0].width, boxes[1].width);
        assert!(boxes[2].x + boxes[2].width <= EMU_PER_INCH * 6 + 1);
        assert_eq!(boxes[0].text, vec![(0, "One".to_owned())]);
    }

    #[test]
    fn words_that_do_not_fit_shrink_the_font_to_its_floor() {
        let lines = vec!["A very long line of words that will not fit".to_owned()];
        assert!(fits(&lines, 8.0, EMU_PER_INCH as f64 * 3.0, EMU_PER_INCH as f64));
        assert!(!fits(&lines, 40.0, EMU_PER_INCH as f64, EMU_PER_INCH as f64 / 2.0));
    }
}
