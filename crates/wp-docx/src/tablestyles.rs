//! The table styles a document can be given, and writing one into it.
//!
//! # Why they have to be written rather than chosen
//!
//! A table names its style by identifier — `w:tblStyle w:val="GridTable4"` —
//! and that identifier has to resolve to a definition in `styles.xml`, or the
//! table has no style at all. Word ships its gallery inside the program and
//! copies whichever one is used into the document. There is nowhere else to put
//! them, so this does the same: choosing one writes it in if it is not there
//! already, and a document that arrives with its own keeps them.
//!
//! # What a definition is made of
//!
//! An ordinary cell's formatting, and then one `w:tblStylePr` for each part of
//! the table that is dressed differently — see [`crate::styles::Conditional`].
//! The parts are only used where the table's own `w:tblLook` says they may be,
//! which is what Word's Table Style Options tick boxes decide.

use wp_xml::tree::{Element, XmlTree};

use crate::styles::Conditional;
use crate::{edit, read, Document, EditKind};

/// One line of a table style's borders.
#[derive(Clone, Copy, Debug)]
pub struct Line {
    /// The style name the file uses, such as `single`.
    pub style: &'static str,
    /// Width in eighths of a point.
    pub size: u32,
    /// Six hex digits, or "auto".
    pub color: &'static str,
}

/// What a style says about one part of a table.
#[derive(Clone, Copy, Debug, Default)]
pub struct Part {
    /// The colour behind the cells, as six hex digits.
    pub shading: Option<&'static str>,
    pub bold: bool,
    /// The colour of the text, as six hex digits.
    pub color: Option<&'static str>,
}

/// One of the styles the gallery offers.
#[derive(Clone, Copy, Debug)]
pub struct TableStyle {
    /// The identifier a table names, which is Word's own for the same style.
    pub id: &'static str,
    /// What the gallery calls it, which is Word's name for it.
    pub name: &'static str,
    /// The lines round and inside every cell, when it has any.
    pub lines: Option<Line>,
    /// The ordinary cell, and then the parts that are not ordinary.
    pub parts: &'static [(Conditional, Part)],
}

impl Document {
    /// Whether the document already carries a style.
    #[must_use]
    pub fn has_style(&self, id: &str) -> bool {
        self.styles().all().iter().any(|style| style.id == id)
    }

    /// Writes a table style into the document, if it is not there already.
    ///
    /// Returns whether anything was written. A style that is there is left
    /// exactly as it is: a document that came from Word carries Word's own
    /// definition, and overwriting it with this one would change how the
    /// document looks in the program it was made in.
    pub fn add_table_style(&mut self, wanted: &TableStyle) -> bool {
        if self.has_style(wanted.id) {
            return false;
        }
        let Some(mut tree) = self.styles_tree_for_tables() else { return false };

        let prefix = edit::prefix_for(&tree.root, crate::WORDPROCESSING_NAMESPACE);
        tree.root.push_element(definition(wanted, prefix.as_deref()));

        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);
        self.save_styles_for_tables(&tree);
        self.mark_modified();
        true
    }
}

/// A whole `w:style` element for one table style.
fn definition(wanted: &TableStyle, prefix: Option<&str>) -> Element {
    let name = |local: &str| edit::name_with(prefix, local);
    let valued = |local: &str, value: &str| {
        let mut element = Element::new(&name(local), Some(read::W));
        element.set_namespaced_attribute(&name("val"), read::W, value);
        element
    };

    let mut style = Element::new(&name("style"), Some(read::W));
    style.set_namespaced_attribute(&name("type"), read::W, "table");
    style.set_namespaced_attribute(&name("styleId"), read::W, wanted.id);
    style.push_element(valued("name", wanted.name));
    style.push_element(valued("basedOn", "TableNormal"));
    style.push_element(Element::new(&name("uiPriority"), Some(read::W)));

    // The table's own properties: the lines, which every cell gets.
    let mut properties = Element::new(&name("tblPr"), Some(read::W));
    if let Some(line) = wanted.lines {
        properties.push_element(borders("tblBorders", line, prefix));
    }
    style.push_element(properties);

    for (kind, part) in wanted.parts {
        style.push_element(part_element(*kind, *part, prefix));
    }
    style
}

/// A `w:tblBorders` or `w:tcBorders` with the same line on every edge and
/// through the middle.
fn borders(local: &str, line: Line, prefix: Option<&str>) -> Element {
    let name = |local: &str| edit::name_with(prefix, local);
    let mut element = Element::new(&name(local), Some(read::W));
    for edge in ["top", "left", "bottom", "right", "insideH", "insideV"] {
        let mut side = Element::new(&name(edge), Some(read::W));
        side.set_namespaced_attribute(&name("val"), read::W, line.style);
        side.set_namespaced_attribute(&name("sz"), read::W, &line.size.to_string());
        side.set_namespaced_attribute(&name("space"), read::W, "0");
        side.set_namespaced_attribute(&name("color"), read::W, line.color);
        element.push_element(side);
    }
    element
}

/// One `w:tblStylePr`.
fn part_element(kind: Conditional, part: Part, prefix: Option<&str>) -> Element {
    let name = |local: &str| edit::name_with(prefix, local);
    let mut element = Element::new(&name("tblStylePr"), Some(read::W));
    element.set_namespaced_attribute(&name("type"), read::W, kind.word());

    if part.bold || part.color.is_some() {
        let mut run = Element::new(&name("rPr"), Some(read::W));
        if part.bold {
            run.push_element(Element::new(&name("b"), Some(read::W)));
        }
        if let Some(colour) = part.color {
            let mut element = Element::new(&name("color"), Some(read::W));
            element.set_namespaced_attribute(&name("val"), read::W, colour);
            run.push_element(element);
        }
        element.push_element(run);
    }

    if let Some(fill) = part.shading {
        let mut cell = Element::new(&name("tcPr"), Some(read::W));
        let mut shading = Element::new(&name("shd"), Some(read::W));
        shading.set_namespaced_attribute(&name("val"), read::W, "clear");
        shading.set_namespaced_attribute(&name("color"), read::W, "auto");
        shading.set_namespaced_attribute(&name("fill"), read::W, fill);
        cell.push_element(shading);
        element.push_element(cell);
    }
    element
}

impl Document {
    /// The styles part as a tree, or the default one where there is none.
    fn styles_tree_for_tables(&self) -> Option<XmlTree> {
        crate::related_tree(
            self.package(),
            self.main_part(),
            crate::STYLES_RELATIONSHIP,
            "word/styles.xml",
        )
    }

    /// And writing it back where it came from.
    fn save_styles_for_tables(&mut self, tree: &XmlTree) {
        let Ok(xml) = tree.to_xml() else { return };
        let main_part = self.main_part().to_owned();
        let target = self
            .package()
            .relationships(&main_part)
            .ok()
            .and_then(|relationships| {
                let found = relationships.single_by_type(crate::STYLES_RELATIONSHIP)?;
                found.resolved_target(&main_part)?.ok()
            })
            .unwrap_or_else(|| "word/styles.xml".to_owned());

        self.package_mut().add_part(&target, crate::STYLES_CONTENT_TYPE, xml.into_bytes());
        self.styles =
            crate::styles::Styles::parse(&tree.root).with_theme(self.styles.theme().clone());
    }
}
