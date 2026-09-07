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
    Alignment, Block, BreakKind, Body, LineRule, LineSpacing, NumberingReference, Paragraph,
    ParagraphProperties, Run, RunContent, RunProperties, Table, TableCell, TableRow, Underline,
};

/// The WordprocessingML namespace.
pub const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

/// Reads a `w:val` attribute.
pub(crate) fn value(element: &Element) -> Option<&str> {
    element.attribute(Some(W), "val")
}

/// Reads an on/off property.
///
/// `<w:b/>` means bold. So does `<w:b w:val="1"/>`. But `<w:b w:val="0"/>` means
/// *not* bold — which matters, because that is how a run switches off something
/// its style turned on.
pub(crate) fn on_off(element: &Element) -> bool {
    !matches!(value(element), Some("0" | "false" | "off"))
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
    let properties = element
        .child(Some(W), "pPr")
        .map(read_paragraph_properties)
        .unwrap_or_default();

    let mut runs = Vec::new();
    collect_runs(element, &mut runs);
    Paragraph { properties, runs }
}

/// Reads a `w:pPr`, wherever it appears — in a paragraph or in a style.
#[must_use]
pub fn read_paragraph_properties(properties: &Element) -> ParagraphProperties {
    let mut result = ParagraphProperties::default();

    for property in properties.child_elements() {
        if property.namespace.as_deref() != Some(W) {
            continue;
        }
        match property.local_name() {
            "pStyle" => result.style = value(property).map(str::to_owned),
            "jc" => result.alignment = value(property).and_then(Alignment::from_attribute),
            "bidi" => result.right_to_left = Some(on_off(property)),
            "keepNext" => result.keep_next = Some(on_off(property)),
            "keepLines" => result.keep_lines = Some(on_off(property)),
            "pageBreakBefore" => result.page_break_before = Some(on_off(property)),
            "widowControl" => result.widow_control = Some(on_off(property)),
            "outlineLvl" => {
                result.outline_level = value(property).and_then(|text| text.parse().ok());
            }
            "ind" => {
                // "start"/"end" are the current names; "left"/"right" are the
                // older ones Word still writes.
                result.indent_start = signed(property, "start").or_else(|| signed(property, "left"));
                result.indent_end = signed(property, "end").or_else(|| signed(property, "right"));
                result.indent_first_line = match signed(property, "hanging") {
                    // A hanging indent is a negative first-line indent.
                    Some(hanging) => Some(-hanging),
                    None => signed(property, "firstLine"),
                };
            }
            "spacing" => {
                result.space_before = signed(property, "before");
                result.space_after = signed(property, "after");
                if let Some(line) = signed(property, "line") {
                    let rule = match property.attribute(Some(W), "lineRule") {
                        Some("exact") => LineRule::Exact,
                        Some("atLeast") => LineRule::AtLeast,
                        _ => LineRule::Auto,
                    };
                    result.line_spacing = Some(LineSpacing { value: line, rule });
                }
            }
            "numPr" => {
                let id = property
                    .child(Some(W), "numId")
                    .and_then(value)
                    .and_then(|text| text.parse().ok());
                let level = property
                    .child(Some(W), "ilvl")
                    .and_then(value)
                    .and_then(|text| text.parse().ok())
                    .unwrap_or(0);
                if let Some(id) = id {
                    result.numbering = Some(NumberingReference { id, level });
                }
            }
            _ => {}
        }
    }

    result
}

/// Reads a signed measurement attribute.
fn signed(element: &Element, name: &str) -> Option<i32> {
    element.attribute(Some(W), name).and_then(|text| text.parse().ok())
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
    let properties =
        element.child(Some(W), "rPr").map(read_run_properties).unwrap_or_default();

    let mut content = Vec::new();
    for child in element.child_elements() {
        if child.namespace.as_deref() != Some(W) {
            continue;
        }
        match child.local_name() {
            "t" => content.push(RunContent::Text(child.text_content())),
            "br" => {
                let kind = match child.attribute(Some(W), "type") {
                    Some("page") => BreakKind::Page,
                    Some("column") => BreakKind::Column,
                    _ => BreakKind::Line,
                };
                content.push(RunContent::Break(kind));
            }
            "tab" => content.push(RunContent::Tab),
            _ => {}
        }
    }

    Run { properties, content }
}

/// Reads a `w:rPr`, wherever it appears — in a run or in a style.
#[must_use]
pub fn read_run_properties(properties: &Element) -> RunProperties {
    let mut result = RunProperties::default();

    for property in properties.child_elements() {
        if property.namespace.as_deref() != Some(W) {
            continue;
        }
        match property.local_name() {
            "rStyle" => result.style = value(property).map(str::to_owned),
            "b" => result.bold = Some(on_off(property)),
            "i" => result.italic = Some(on_off(property)),
            "strike" => result.strike = Some(on_off(property)),
            "rtl" => result.right_to_left = Some(on_off(property)),
            "u" => {
                result.underline = Some(match value(property) {
                    // A bare <w:u/> with no value means a single underline.
                    None => Underline::Single,
                    Some(style) => Underline::from_attribute(style),
                });
            }
            "sz" => result.size_half_points = value(property).and_then(|v| v.parse().ok()),
            "color" => result.color = value(property).map(str::to_owned),
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
