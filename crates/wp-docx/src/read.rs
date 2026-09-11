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

use crate::theme;
use wp_xml::tree::Element;

use crate::model::{
    Alignment, Block, Body, Border, BreakKind, LineRule, LineSpacing, NumberingReference,
    Paragraph, ParagraphBorders, ParagraphProperties, Picture, Revision, RevisionKind, Run,
    RunContent, RunProperties, TabAlignment, TabLeader, TabStop, Table, TableBorders, TableCell,
    TableLook, TableRow, Underline, VerticalAlignment,
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
            "hyperlink" | "smartTag" | "sdt" | "sdtContent" | "bdo" | "dir" | "moveTo"
        )
}

/// Elements that hold blocks but add nothing themselves.
fn is_transparent_block(element: &Element) -> bool {
    element.namespace.as_deref() == Some(W)
        && matches!(element.local_name(), "sdt" | "sdtContent" | "ins" | "moveTo")
}

/// Reads the blocks of a part, whichever kind of part it is.
///
/// The main document keeps its blocks under `w:body`; a header or a footer has
/// no body at all and keeps them straight under `w:hdr` or `w:ftr`. One reader
/// for both, because the editor points at whichever part is being edited and
/// everything above this must not care which.
#[must_use]
pub fn read_part(root: &Element) -> Body {
    match find_body(root) {
        Some(body) => Body { blocks: read_blocks(body) },
        None => Body { blocks: read_blocks(root) },
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
            "tbl" => blocks.push(Block::Table(Box::new(read_table(child)))),
            _ if is_transparent_block(child) => collect_blocks(child, blocks),
            _ => {}
        }
    }
}

fn read_table(element: &Element) -> Table {
    let properties = element.child(Some(W), "tblPr");
    let style = properties
        .and_then(|properties| properties.child(Some(W), "tblStyle"))
        .and_then(value)
        .map(str::to_owned);

    let borders = properties
        .and_then(|properties| properties.child(Some(W), "tblBorders"))
        .map(read_table_borders)
        .unwrap_or_default();

    let indent = properties
        .and_then(|properties| properties.child(Some(W), "tblInd"))
        .and_then(|element| {
            element
                .attribute(Some(W), "w")
                .or_else(|| element.attribute(Some(W), "val"))
                .and_then(|text| text.parse().ok())
        })
        .unwrap_or(0);

    let margins = properties.and_then(|properties| properties.child(Some(W), "tblCellMar"));
    let cell_margin = |side: &str| {
        margins
            .and_then(|margins| margins.child(Some(W), side))
            .and_then(|element| element.attribute(Some(W), "w"))
            .and_then(|text| text.parse().ok())
    };

    let grid = element
        .child(Some(W), "tblGrid")
        .map(|grid| {
            grid.children_named(Some(W), "gridCol")
                .map(|column| {
                    column.attribute(Some(W), "w").and_then(|text| text.parse().ok()).unwrap_or(0)
                })
                .collect()
        })
        .unwrap_or_default();

    let rows = element.children_named(Some(W), "tr").map(read_table_row).collect();

    Table {
        rows,
        style,
        look: properties
            .and_then(|properties| properties.child(Some(W), "tblLook"))
            .map(read_table_look)
            .unwrap_or_default(),
        grid,
        borders,
        indent,
        cell_margin_start: cell_margin("start").or_else(|| cell_margin("left")),
        cell_margin_end: cell_margin("end").or_else(|| cell_margin("right")),
    }
}

fn read_table_row(row: &Element) -> TableRow {
    let height = row
        .child(Some(W), "trPr")
        .and_then(|properties| properties.child(Some(W), "trHeight"))
        .and_then(value)
        .and_then(|text| text.parse().ok());

    let is_header = row
        .child(Some(W), "trPr")
        .and_then(|properties| properties.child(Some(W), "tblHeader"))
        .is_some_and(on_off);
    TableRow {
        cells: row.children_named(Some(W), "tc").map(read_table_cell).collect(),
        height,
        is_header,
    }
}

fn read_table_cell(cell: &Element) -> TableCell {
    let properties = cell.child(Some(W), "tcPr");

    // A width of type "auto" is not a width: it asks for one to be worked out,
    // which is exactly what saying nothing means.
    let width = properties
        .and_then(|properties| properties.child(Some(W), "tcW"))
        .filter(|element| element.attribute(Some(W), "type") != Some("auto"))
        .and_then(|element| element.attribute(Some(W), "w"))
        .and_then(|text| text.parse().ok());

    let span = properties
        .and_then(|properties| properties.child(Some(W), "gridSpan"))
        .and_then(value)
        .and_then(|text| text.parse().ok())
        .unwrap_or(1)
        .max(1);

    // A merged cell is written once per row; only the first says "restart".
    let merged_upwards = properties
        .and_then(|properties| properties.child(Some(W), "vMerge"))
        .is_some_and(|element| value(element) != Some("restart"));

    let borders = properties
        .and_then(|properties| properties.child(Some(W), "tcBorders"))
        .map(read_table_borders)
        .unwrap_or_default();

    // The colour behind a cell, which is what a banded table is made of.
    let shading = properties
        .and_then(|properties| properties.child(Some(W), "shd"))
        .and_then(|element| element.attribute(Some(W), "fill"))
        .filter(|value| *value != "auto")
        .map(str::to_owned);

    TableCell { blocks: read_blocks(cell), width, span, merged_upwards, borders, shading }
}

/// Reads a `w:tblLook`.
///
/// Word 2007 wrote one hexadecimal number and nothing else; every version since
/// writes the attributes as well. A file may have either, so the number is read
/// where the attributes are missing. See [`TableLook`] on why two of the six are
/// written the other way up.
#[must_use]
pub fn read_table_look(element: &Element) -> TableLook {
    let bits = element
        .attribute(Some(W), "val")
        .and_then(|text| u32::from_str_radix(text.trim(), 16).ok());

    let flag = |name: &str, mask: u32, backwards: bool| {
        let stated =
            element.attribute(Some(W), name).map(|value| matches!(value, "1" | "true" | "on"));
        let set = match stated {
            Some(on) => on,
            None => match bits {
                Some(bits) => bits & mask != 0,
                // Nothing said at all: a table has bands and no special
                // columns, which is what an absent `w:tblLook` means.
                None => !backwards,
            },
        };
        if backwards {
            !set
        } else {
            set
        }
    };

    TableLook {
        first_row: flag("firstRow", 0x0020, false),
        last_row: flag("lastRow", 0x0040, false),
        first_column: flag("firstColumn", 0x0080, false),
        last_column: flag("lastColumn", 0x0100, false),
        banded_rows: flag("noHBand", 0x0200, true),
        banded_columns: flag("noVBand", 0x0400, true),
    }
}

/// Reads a `w:tblBorders` or `w:tcBorders`.
#[must_use]
pub fn read_table_borders(element: &Element) -> TableBorders {
    let side = |name: &str| element.child(Some(W), name).map(read_border);
    TableBorders {
        top: side("top"),
        start: side("start").or_else(|| side("left")),
        bottom: side("bottom"),
        end: side("end").or_else(|| side("right")),
        inside_horizontal: side("insideH"),
        inside_vertical: side("insideV"),
    }
}

/// Reads a `w:pBdr`.
fn read_paragraph_borders(element: &Element) -> ParagraphBorders {
    let side = |name: &str| element.child(Some(W), name).map(read_border);
    ParagraphBorders {
        top: side("top"),
        start: side("start").or_else(|| side("left")),
        bottom: side("bottom"),
        end: side("end").or_else(|| side("right")),
        between: side("between"),
    }
}

pub(crate) fn read_border(element: &Element) -> Border {
    Border {
        style: value(element).unwrap_or("single").to_owned(),
        // Four eighths of a point is the line Word draws when asked for one
        // without a size.
        size: element.attribute(Some(W), "sz").and_then(|text| text.parse().ok()).unwrap_or(4),
        color: element
            .attribute(Some(W), "color")
            .filter(|text| *text != "auto")
            .map(str::to_owned),
    }
}

pub(crate) fn read_paragraph(element: &Element) -> Paragraph {
    let properties =
        element.child(Some(W), "pPr").map(read_paragraph_properties).unwrap_or_default();

    let mut runs = Vec::new();
    collect_runs(element, &mut runs);
    Paragraph { properties, runs }
}

/// The tab stops of a paragraph, in the order they come along the line.
///
/// A stop with no position is dropped: it says nothing about where a tab would
/// land, and Word does not write one.
fn read_tab_stops(tabs: &Element) -> Vec<TabStop> {
    let mut stops: Vec<TabStop> = tabs
        .children_named(Some(W), "tab")
        .filter_map(|tab| {
            let position =
                tab.attribute(Some(W), "pos").and_then(|text| text.trim().parse::<i32>().ok())?;
            let alignment =
                tab.attribute(Some(W), "val").map_or(TabAlignment::Start, TabAlignment::from_word);
            let leader =
                tab.attribute(Some(W), "leader").map_or(TabLeader::None, TabLeader::from_word);
            Some(TabStop { position, alignment, leader })
        })
        .collect();
    stops.sort_by_key(|stop| stop.position);
    stops
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
            "contextualSpacing" => result.contextual_spacing = Some(on_off(property)),
            "mirrorIndents" => result.mirror_indents = Some(on_off(property)),
            "suppressLineNumbers" => result.suppress_line_numbers = Some(on_off(property)),
            "suppressAutoHyphens" => result.no_hyphenation = Some(on_off(property)),
            "pBdr" => result.borders = read_paragraph_borders(property),
            "tabs" => result.tab_stops = read_tab_stops(property),
            "shd" => {
                result.shading = property
                    .attribute(Some(W), "fill")
                    .filter(|value| *value != "auto")
                    .map(str::to_owned);
            }
            "outlineLvl" => {
                result.outline_level = value(property).and_then(|text| text.parse().ok());
            }
            "ind" => {
                // "start"/"end" are the current names; "left"/"right" are the
                // older ones Word still writes.
                result.indent_start =
                    signed(property, "start").or_else(|| signed(property, "left"));
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
    collect_runs_in_field(parent, runs, None);
}

/// The same, remembering which field the runs are the result of.
fn collect_runs_in_field(parent: &Element, runs: &mut Vec<Run>, field: Option<&str>) {
    collect_runs_within(parent, runs, field, None);
}

/// The same again, remembering the tracked change the runs are part of.
fn collect_runs_within(
    parent: &Element,
    runs: &mut Vec<Run>,
    field: Option<&str>,
    revision: Option<&Revision>,
) {
    // Fields written the long way are a run of markers among the ordinary
    // runs, so reading them means keeping track of where in one we are. See
    // [`crate::fields`].
    let mut open = crate::fields::Fields::default();

    for child in parent.child_elements() {
        // An equation is a sibling of the runs, in the namespace equations live
        // in, so it is met before the check that everything else is
        // WordprocessingML.
        if child.namespace.as_deref() == Some(crate::math::MATH_NAMESPACE) {
            match child.local_name() {
                "oMath" => runs.push(math_run(child, field, revision)),
                // A display equation is wrapped in a paragraph of its own and
                // holds the equation itself inside.
                "oMathPara" => {
                    for inner in child.child_elements() {
                        if inner.is(Some(crate::math::MATH_NAMESPACE), "oMath") {
                            runs.push(math_run(inner, field, revision));
                        }
                    }
                }
                _ => {}
            }
            continue;
        }
        if child.namespace.as_deref() != Some(W) {
            continue;
        }
        match child.local_name() {
            "r" => {
                // A marker run is not text: it says where a field starts, where
                // its instruction ends, and where the field ends.
                if let Some(marker) = crate::fields::marker_of(child) {
                    match marker {
                        crate::fields::Marker::Begin => open.begin(),
                        crate::fields::Marker::Separate => open.separate(),
                        crate::fields::Marker::End => {
                            open.end();
                        }
                    }
                    continue;
                }
                // Nor is the instruction itself.
                if let Some(text) = crate::fields::instruction_of(child) {
                    open.add_instruction(&text);
                    continue;
                }
                // Anything else inside an instruction is part of it rather than
                // part of the document, and is left out for the same reason.
                if open.in_instruction() {
                    continue;
                }

                let mut run = read_run(child);
                // The innermost field being answered wins: a page number inside
                // a table of contents entry belongs to the page number.
                run.field = open.current().or_else(|| field.map(str::to_owned));
                run.revision = revision.cloned();
                runs.push(run);
            }
            // Text somebody added or removed while changes were tracked. The
            // deleted kind keeps its text: a deletion nobody has accepted has
            // not happened, and throwing the words away would decide that for
            // them.
            "ins" | "del" => {
                let kind = if child.local_name() == "ins" {
                    RevisionKind::Inserted
                } else {
                    RevisionKind::Deleted
                };
                let change = Revision {
                    kind,
                    author: child.attribute(Some(W), "author").unwrap_or_default().to_owned(),
                    date: child.attribute(Some(W), "date").unwrap_or_default().to_owned(),
                    id: child
                        .attribute(Some(W), "id")
                        .and_then(|text| text.parse().ok())
                        .unwrap_or(0),
                };
                collect_runs_within(child, runs, field, Some(&change));
            }
            // A simple field holds the runs that show its last computed value.
            // They are read as ordinary runs so the cached answer is never
            // lost, and tagged with the instruction so it can be worked out
            // again by anything that knows how.
            "fldSimple" => {
                let instruction =
                    child.attribute(Some(W), "instr").unwrap_or_default().trim().to_owned();
                collect_runs_within(child, runs, Some(&instruction), revision);
            }
            _ if is_transparent_inline(child) => {
                collect_runs_within(child, runs, field, revision);
            }
            _ => {}
        }
    }
}

pub(crate) fn read_run(element: &Element) -> Run {
    let properties = element.child(Some(W), "rPr").map(read_run_properties).unwrap_or_default();

    let mut content = Vec::new();
    for child in element.child_elements() {
        if child.namespace.as_deref() != Some(W) {
            continue;
        }
        match child.local_name() {
            // `w:delText` is text that was deleted: the same thing as `w:t`,
            // named differently so a reader that ignores deletions can.
            "t" | "delText" => content.push(RunContent::Text(child.text_content())),
            "footnoteReference" | "endnoteReference" => {
                let id =
                    child.attribute(Some(W), "id").and_then(|text| text.parse().ok()).unwrap_or(0);
                content.push(RunContent::NoteReference {
                    id,
                    endnote: child.local_name() == "endnoteReference",
                });
            }
            "br" => {
                let kind = match child.attribute(Some(W), "type") {
                    Some("page") => BreakKind::Page,
                    Some("column") => BreakKind::Column,
                    _ => BreakKind::Line,
                };
                content.push(RunContent::Break(kind));
            }
            "tab" => content.push(RunContent::Tab),
            "ptab" => {
                // Word writes left, center or right; anything else is not an
                // alignment tab this program can place, and an ordinary tab is
                // the closest true thing.
                let alignment = match child.attribute(Some(W), "alignment") {
                    Some("center") => TabAlignment::Center,
                    Some("right") => TabAlignment::End,
                    _ => TabAlignment::Start,
                };
                content.push(RunContent::PositionTab(alignment));
            }
            // A picture arrives as a DrawingML tree, or as the older VML shape
            // that documents saved by earlier versions still carry.
            "drawing" | "pict" | "object" => {
                // A chart, a shape and a picture are the same wrapper round
                // different graphic data. Each reader is given the drawing in
                // turn, and each says nothing when it is not the one.
                if let Some(chart) = read_chart_reference(child) {
                    content.push(RunContent::Chart(chart));
                } else if let Some(shape) = crate::shapes::read_shape(child) {
                    content.push(RunContent::Shape(shape));
                } else if let Some(picture) = read_picture(child) {
                    content.push(RunContent::Picture(picture));
                }
            }
            _ => {}
        }
    }

    Run { properties, content, field: None, revision: None }
}

/// The namespace relationship references live in.
pub(crate) const RELATIONSHIPS: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// Reads a picture out of a drawing.
///
/// Searched by local name rather than by exact namespace. A drawing is written
/// in four namespaces at once, two of which have both a 2006 and a 2010 form,
/// and matching all of them exactly would reject perfectly ordinary documents
/// for no gain: no other element in a drawing is called `blip` or `extent`.
fn read_picture(drawing: &Element) -> Option<Picture> {
    let reference = find_by_local_name(drawing, "blip")
        .and_then(|blip| blip.attribute(Some(RELATIONSHIPS), "embed"))
        .or_else(|| {
            find_by_local_name(drawing, "imagedata")
                .and_then(|data| data.attribute(Some(RELATIONSHIPS), "id"))
        })?;

    let extent = find_by_local_name(drawing, "extent");
    let width_emu = extent
        .and_then(|element| element.attribute(None, "cx"))
        .and_then(|text| text.parse().ok())
        .unwrap_or(0);
    let height_emu = extent
        .and_then(|element| element.attribute(None, "cy"))
        .and_then(|text| text.parse().ok())
        .unwrap_or(0);

    let description = find_by_local_name(drawing, "docPr")
        .and_then(|element| {
            element.attribute(None, "descr").or_else(|| element.attribute(None, "name"))
        })
        .filter(|text| !text.is_empty())
        .map(str::to_owned);

    Some(Picture { relationship: reference.to_owned(), width_emu, height_emu, description })
}

/// The first descendant with a given local name, whatever namespace it is in.
fn find_by_local_name<'a>(element: &'a Element, local: &str) -> Option<&'a Element> {
    for child in element.child_elements() {
        if child.local_name() == local {
            return Some(child);
        }
        if let Some(found) = find_by_local_name(child, local) {
            return Some(found);
        }
    }
    None
}

/// Reads a `w:rPr`, wherever it appears — in a run or in a style.
#[must_use]
pub fn read_run_properties(properties: &Element) -> RunProperties {
    // The effects are in a namespace of their own, so the loop below — which
    // only looks at WordprocessingML — would never see them.
    let mut result = RunProperties {
        effect: crate::effects::read_effect(properties),
        // The OpenType features are in that same namespace of their own.
        open_type: crate::typography::read_open_type(properties),
        ..RunProperties::default()
    };

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
            "dstrike" => result.double_strike = Some(on_off(property)),
            "caps" => result.caps = Some(on_off(property)),
            "smallCaps" => result.small_caps = Some(on_off(property)),
            "vanish" => result.hidden = Some(on_off(property)),
            // How wide the letters are drawn, how far apart, how far off the
            // line, and from what size the font's own kerning is used. Each is
            // measured differently; see [`crate::typography`].
            "w" => result.scale = value(property).and_then(|v| v.parse().ok()),
            "spacing" => result.spacing_twentieths = value(property).and_then(|v| v.parse().ok()),
            "position" => {
                result.position_half_points = value(property).and_then(|v| v.parse().ok());
            }
            "kern" => result.kerning_half_points = value(property).and_then(|v| v.parse().ok()),
            "u" => {
                result.underline = Some(match value(property) {
                    // A bare <w:u/> with no value means a single underline.
                    None => Underline::Single,
                    Some(style) => Underline::from_attribute(style),
                });
                // The line may be a different colour from the text, which is
                // written beside the style rather than as an element of its own.
                result.underline_color = property
                    .attribute(Some(W), "color")
                    .filter(|text| *text != "auto")
                    .map(str::to_owned);
            }
            "sz" => result.size_half_points = value(property).and_then(|v| v.parse().ok()),
            "color" => {
                result.color = value(property).map(str::to_owned).filter(|text| text != "auto");
                // A colour can be named rather than written out, and then the
                // theme says what it is. `w:val` is written beside it as the
                // answer somebody worked out last, and is not to be trusted:
                // it is stale the moment the theme changes.
                result.color_theme = property
                    .attribute(Some(W), "themeColor")
                    .and_then(theme::Slot::from_word)
                    .map(|slot| theme::ThemeColor {
                        slot,
                        tint: hex_byte(property, "themeTint"),
                        shade: hex_byte(property, "themeShade"),
                    });
            }
            "highlight" => result.highlight = value(property).map(str::to_owned),
            "vertAlign" => {
                result.vertical_align = value(property).map(VerticalAlignment::from_attribute);
            }
            "lang" => result.language = value(property).map(str::to_owned),
            // The font differs per script; the Latin one is the name users
            // think of as "the font".
            "rFonts" => {
                result.font = property
                    .attribute(Some(W), "ascii")
                    .or_else(|| property.attribute(Some(W), "hAnsi"))
                    .or_else(|| property.attribute(Some(W), "cs"))
                    .map(str::to_owned);
                // And the same for the font: a document Word made names the
                // theme's slot rather than a typeface.
                result.font_theme = property
                    .attribute(Some(W), "asciiTheme")
                    .or_else(|| property.attribute(Some(W), "hAnsiTheme"))
                    .or_else(|| property.attribute(Some(W), "cstheme"))
                    .and_then(theme::FontSlot::from_word);
            }
            _ => {}
        }
    }

    result
}

/// One of the theme's tint or shade attributes, which are written as two hex
/// digits rather than as a number.
fn hex_byte(element: &Element, name: &str) -> Option<u8> {
    let text = element.attribute(Some(W), name)?;
    u8::from_str_radix(text.trim(), 16).ok()
}

/// An equation, as the one run of a paragraph that holds it.
///
/// A run rather than a shape of its own because everything that walks a
/// paragraph already walks its runs, and an equation takes one character of the
/// text exactly as a picture does.
fn math_run(element: &Element, field: Option<&str>, revision: Option<&Revision>) -> Run {
    Run {
        properties: RunProperties::default(),
        content: vec![RunContent::Math(crate::math::read_math(element))],
        field: field.map(str::to_owned),
        revision: revision.cloned(),
    }
}

/// Reads the chart a drawing points at, if it points at one.
///
/// Only the reference: the chart itself is a part of the package, and reading
/// it needs the package, which this layer does not have. What comes back is
/// which relationship to follow and how much room the drawing was given.
#[must_use]
fn read_chart_reference(drawing: &Element) -> Option<crate::model::ChartReference> {
    let reference = find_named(drawing, "chart")?;
    let id = reference.attribute(Some(RELATIONSHIPS), "id")?.to_owned();
    let (width, height) = drawing_extent(drawing);
    Some(crate::model::ChartReference { relationship: id, width_emu: width, height_emu: height })
}

/// The first element under one with a given local name, whatever its namespace.
fn find_named<'a>(element: &'a Element, local: &str) -> Option<&'a Element> {
    for child in element.child_elements() {
        if child.local_name() == local {
            return Some(child);
        }
        if let Some(found) = find_named(child, local) {
            return Some(found);
        }
    }
    None
}

/// How much room a drawing was given, in English metric units.
fn drawing_extent(drawing: &Element) -> (i64, i64) {
    let Some(extent) = find_named(drawing, "extent") else { return (0, 0) };
    let read = |name: &str| {
        extent.attribute(None, name).and_then(|text| text.trim().parse::<i64>().ok()).unwrap_or(0)
    };
    (read("cx"), read("cy"))
}
