//! Reading a document body out of the element tree.
//!
//! Only the constructs that carry text are recognized. Everything else is
//! stepped over rather than rejected: a document is full of markup that does not
//! affect what the text says — proofing marks, bookmarks, revision boundaries —
//! and refusing to read a file because of one would be useless behaviour.
//!
//! Deleted text (`w:delText`, inside `w:del`) is skipped rather than stepped
//! into. It is not part of the document as it currently reads, and including it
//! would put text a colleague removed back into the output.

use wp_xml::tree::Element;

use crate::model::{
    Alignment, Block, BreakKind, Body, Paragraph, Run, RunContent, RunProperties, Table, TableCell,
    TableRow,
};

/// The WordprocessingML namespace.
pub const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

/// Reads a `w:val` attribute.
fn value(element: &Element) -> Option<&str> {
    element.attribute(Some(W), "val")
}

/// Reads an on/off property.
///
/// `<w:b/>` means bold. So does `<w:b w:val="1"/>`. But `<w:b w:val="0"/>` means
/// *not* bold — which matters, because that is how a run switches off something
/// its style turned on.
fn is_on(element: &Element) -> bool {
    match value(element) {
        None => true,
        Some("0" | "false" | "off") => false,
        Some(_) => true,
    }
}

/// Elements that hold runs but add nothing to the text themselves.
fn is_transparent_inline(element: &Element) -> bool {
    element.namespace.as_deref() == Some(W)
        && matches!(
            element.local_name(),
            "hyperlink" | "ins" | "smartTag" | "sdt" | "sdtContent" | "bdo" | "dir" | "moveTo"
        )
}

/// Elements that hold blocks but add nothing themselves.
fn is_transparent_block(element: &Element) -> bool {
    element.namespace.as_deref() == Some(W)
        && matches!(element.local_name(), "sdt" | "sdtContent" | "ins" | "moveTo")
}

/// Reads the body out of a `w:document` element.
///
/// A document with no `w:body` yields an empty body rather than an error: the
/// part is still valid XML, and there is nothing to show.
#[must_use]
pub fn read_document(root: &Element) -> Body {
    match find_body(root) {
        Some(body) => Body { blocks: read_blocks(body) },
        None => Body::default(),
    }
}

/// Finds `w:body` anywhere under the root.
pub fn find_body(root: &Element) -> Option<&Element> {
    if root.is(Some(W), "body") {
        return Some(root);
    }
    root.child_elements().find_map(find_body)
}

/// Finds `w:body`, mutably.
pub fn find_body_mut(root: &mut Element) -> Option<&mut Element> {
    if root.is(Some(W), "body") {
        return Some(root);
    }
    root.child_elements_mut().find_map(find_body_mut)
}

fn read_blocks(parent: &Element) -> Vec<Block> {
    let mut blocks = Vec::new();
    collect_blocks(parent, &mut blocks);
    blocks
}

fn collect_blocks(parent: &Element, blocks: &mut Vec<Block>) {
    for child in parent.child_elements() {
        if child.namespace.as_deref() != Some(W) {
            continue;
        }
        match child.local_name() {
            "p" => blocks.push(Block::Paragraph(read_paragraph(child))),
            "tbl" => blocks.push(Block::Table(read_table(child))),
            _ if is_transparent_block(child) => collect_blocks(child, blocks),
            _ => {}
        }
    }
}

fn read_table(element: &Element) -> Table {
    let style = element
        .child(Some(W), "tblPr")
        .and_then(|properties| properties.child(Some(W), "tblStyle"))
        .and_then(value)
        .map(str::to_owned);

    let rows = element
        .children_named(Some(W), "tr")
        .map(|row| TableRow {
            cells: row
                .children_named(Some(W), "tc")
                .map(|cell| TableCell { blocks: read_blocks(cell) })
                .collect(),
        })
        .collect();

    Table { rows, style }
}

fn read_paragraph(element: &Element) -> Paragraph {
    let mut paragraph = Paragraph::default();

    if let Some(properties) = element.child(Some(W), "pPr") {
        for property in properties.child_elements() {
            if property.namespace.as_deref() != Some(W) {
                continue;
            }
            match property.local_name() {
                "pStyle" => paragraph.style = value(property).map(str::to_owned),
                "jc" => paragraph.alignment = value(property).and_then(Alignment::from_attribute),
                "bidi" => paragraph.right_to_left = is_on(property),
                _ => {}
            }
        }
    }

    collect_runs(element, &mut paragraph.runs);
    paragraph
}

fn collect_runs(parent: &Element, runs: &mut Vec<Run>) {
    for child in parent.child_elements() {
        if child.namespace.as_deref() != Some(W) {
            continue;
        }
        match child.local_name() {
            "r" => runs.push(read_run(child)),
            _ if is_transparent_inline(child) => collect_runs(child, runs),
            _ => {}
        }
    }
}

fn read_run(element: &Element) -> Run {
    let mut run = Run::default();

    if let Some(properties) = element.child(Some(W), "rPr") {
        run.properties = read_run_properties(properties);
    }

    for child in element.child_elements() {
        if child.namespace.as_deref() != Some(W) {
            continue;
        }
        match child.local_name() {
            "t" => run.content.push(RunContent::Text(child.text_content())),
            "br" => {
                let kind = match child.attribute(Some(W), "type") {
                    Some("page") => BreakKind::Page,
                    Some("column") => BreakKind::Column,
                    _ => BreakKind::Line,
                };
                run.content.push(RunContent::Break(kind));
            }
            "tab" => run.content.push(RunContent::Tab),
            _ => {}
        }
    }

    run
}

fn read_run_properties(properties: &Element) -> RunProperties {
    let mut result = RunProperties::default();

    for property in properties.child_elements() {
        if property.namespace.as_deref() != Some(W) {
            continue;
        }
        match property.local_name() {
            "b" => result.bold = is_on(property),
            "i" => result.italic = is_on(property),
            "strike" => result.strike = is_on(property),
            "rtl" => result.right_to_left = is_on(property),
            // Underline carries a style, and "none" means no underline.
            "u" => result.underline = !matches!(value(property), None | Some("none")),
            "sz" => result.size_half_points = value(property).and_then(|v| v.parse().ok()),
            "color" => result.color = value(property).map(str::to_owned),
            "rStyle" => result.style = value(property).map(str::to_owned),
            "lang" => result.language = value(property).map(str::to_owned),
            // The font differs per script; the Latin one is the name users
            // think of as "the font".
            "rFonts" => {
                result.font = property
                    .attribute(Some(W), "ascii")
                    .or_else(|| property.attribute(Some(W), "hAnsi"))
                    .or_else(|| property.attribute(Some(W), "cs"))
                    .map(str::to_owned);
            }
            _ => {}
        }
    }

    result
}
