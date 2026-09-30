//! Every property container this program writes, in the order the schema
//! wants its children.
//!
//! # Why the orders are written out here
//!
//! A test that asked the writer's own lists whether the writer kept to them
//! would pass with a wrong list, and a wrong list is what the writer had: the
//! paragraph's without half the schema, the run's with the strike in front
//! of the capitals, the section's without the header and footer references.
//! So these are the sequences as the standard gives them — ECMA-376 Part 1,
//! the transitional complex types `CT_PPr`, `CT_RPr` (with the marks of a
//! change `CT_ParaRPr` puts in front), `CT_TblPr`, `CT_TrPr`, `CT_TcPr` and
//! `CT_SectPr` — with Word 2010's additions to a run's properties in the
//! order [MS-DOCX] gives them, after the standard's own. The writer is held to
//! them.
//!
//! # What is looked at
//!
//! One test for each kind of container, which builds it with everything the
//! writer can put in it — from the model, and by the editing commands called
//! in an order that is nobody's schema — and says what comes out. And a
//! walker, which takes the documents those make and a document with nearly
//! everything in it, saves them, and looks at every `pPr`, `rPr`, `tblPr`,
//! `trPr`, `tcPr` and `sectPr` in every part of the saved file.

use wp_docx::appearance::{LineNumbers, Restart};
use wp_docx::cells::CellEdge;
use wp_docx::eastasian::{CombineBrackets, EastAsianLayout};
use wp_docx::effects::{Effect, TextEffect};
use wp_docx::furniture::{Furniture, Preset, Which};
use wp_docx::model::{
    Alignment, Block, Body, Border, CellMargins, LineRule, LineSpacing, NumberingReference,
    Paragraph, ParagraphBorders, ParagraphProperties, Run, RunContent, RunProperties, TabAlignment,
    TabLeader, TabStop, Table, TableBorders, TableCell, TableFit, TableLook, TableRow,
    TextDirection, Underline, VerticalAlignment,
};
use wp_docx::pageborders::PageBorders;
use wp_docx::sections::{NumberFormat, PageNumbering, Start};
use wp_docx::table_properties::CellAlignment;
use wp_docx::theme::{FontSlot, Slot, ThemeColor};
use wp_docx::typography::{Ligatures, NumberForms, NumberSpacing, OpenType};
use wp_docx::{Document, StyleDefinition, TextPosition};
use wp_xml::tree::{Element, XmlTree};

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const W14: &str = "http://schemas.microsoft.com/office/word/2010/wordml";

/// `CT_PPr`: `CT_PPrBase`, then the paragraph mark's run properties, the
/// section a paragraph ends, and the record of a change.
const PARAGRAPH: &[&str] = &[
    "pStyle",
    "keepNext",
    "keepLines",
    "pageBreakBefore",
    "framePr",
    "widowControl",
    "numPr",
    "suppressLineNumbers",
    "pBdr",
    "shd",
    "tabs",
    "suppressAutoHyphens",
    "kinsoku",
    "wordWrap",
    "overflowPunct",
    "topLinePunct",
    "autoSpaceDE",
    "autoSpaceDN",
    "bidi",
    "adjustRightInd",
    "snapToGrid",
    "spacing",
    "ind",
    "contextualSpacing",
    "mirrorIndents",
    "suppressOverlap",
    "jc",
    "textDirection",
    "textAlignment",
    "textboxTightWrap",
    "outlineLvl",
    "divId",
    "cnfStyle",
    "rPr",
    "sectPr",
    "pPrChange",
];

/// `EG_RPrBase` with `CT_ParaRPr`'s four marks in front and the record of a
/// change behind.
const RUN: &[&str] = &[
    "ins",
    "del",
    "moveFrom",
    "moveTo",
    "rStyle",
    "rFonts",
    "b",
    "bCs",
    "i",
    "iCs",
    "caps",
    "smallCaps",
    "strike",
    "dstrike",
    "outline",
    "shadow",
    "emboss",
    "imprint",
    "noProof",
    "snapToGrid",
    "vanish",
    "webHidden",
    "color",
    "spacing",
    "w",
    "kern",
    "position",
    "sz",
    "szCs",
    "highlight",
    "u",
    "effect",
    "bdr",
    "shd",
    "fitText",
    "vertAlign",
    "rtl",
    "cs",
    "em",
    "lang",
    "eastAsianLayout",
    "specVanish",
    "oMath",
    "rPrChange",
];

/// Word 2010's additions to a run's properties, after the standard's and
/// before the record of a change.
const RUN_EXTENSIONS: &[&str] = &[
    "glow",
    "shadow",
    "reflection",
    "textOutline",
    "textFill",
    "scene3d",
    "props3d",
    "ligatures",
    "numForm",
    "numSpacing",
    "stylisticSets",
    "cntxtAlts",
];

/// `CT_TblPr`.
const TABLE: &[&str] = &[
    "tblStyle",
    "tblpPr",
    "tblOverlap",
    "bidiVisual",
    "tblStyleRowBandSize",
    "tblStyleColBandSize",
    "tblW",
    "jc",
    "tblCellSpacing",
    "tblInd",
    "tblBorders",
    "shd",
    "tblLayout",
    "tblCellMar",
    "tblLook",
    "tblCaption",
    "tblDescription",
    "tblPrChange",
];

/// `CT_TrPr`.
const ROW: &[&str] = &[
    "cnfStyle",
    "divId",
    "gridBefore",
    "gridAfter",
    "wBefore",
    "wAfter",
    "cantSplit",
    "trHeight",
    "tblHeader",
    "tblCellSpacing",
    "jc",
    "hidden",
    "ins",
    "del",
    "trPrChange",
];

/// `CT_TcPr`.
const CELL: &[&str] = &[
    "cnfStyle",
    "tcW",
    "gridSpan",
    "hMerge",
    "vMerge",
    "tcBorders",
    "shd",
    "noWrap",
    "tcMar",
    "textDirection",
    "tcFitText",
    "vAlign",
    "hideMark",
    "headers",
    "cellIns|cellDel|cellMerge",
    "tcPrChange",
];

/// `CT_SectPr`: the references first, in any order among themselves.
const SECTION: &[&str] = &[
    "headerReference|footerReference",
    "footnotePr",
    "endnotePr",
    "type",
    "pgSz",
    "pgMar",
    "paperSrc",
    "pgBorders",
    "lnNumType",
    "pgNumType",
    "cols",
    "formProt",
    "vAlign",
    "noEndnote",
    "titlePg",
    "textDirection",
    "bidi",
    "rtlGutter",
    "docGrid",
    "printerSettings",
    "sectPrChange",
];

fn sequence(container: &str) -> Option<&'static [&'static str]> {
    Some(match container {
        "pPr" => PARAGRAPH,
        "rPr" => RUN,
        "tblPr" => TABLE,
        "trPr" => ROW,
        "tcPr" => CELL,
        "sectPr" => SECTION,
        _ => return None,
    })
}

/// Where a child stands in its container's sequence, or `None` for a child
/// the schema does not put there. Word 2010's run properties stand between
/// the standard's last and the record of a change.
fn rank(container: &str, order: &[&str], child: &Element) -> Option<usize> {
    let local = child.local_name();
    match child.namespace.as_deref() {
        Some(W) => order
            .iter()
            .position(|entry| entry.split('|').any(|name| name == local))
            .map(|at| at * 100),
        Some(W14) if container == "rPr" => {
            let at = RUN_EXTENSIONS.iter().position(|name| *name == local)?;
            let change = order.iter().position(|entry| *entry == "rPrChange")?;
            Some(change * 100 - 50 + at)
        }
        _ => None,
    }
}

/// A child's name as the lists write it.
fn name_of(child: &Element) -> String {
    match child.namespace.as_deref() {
        Some(W) => child.local_name().to_owned(),
        Some(W14) => format!("w14:{}", child.local_name()),
        _ => child.name.clone(),
    }
}

/// Everything out of order, or out of place, at or under an element.
fn faults_under(part: &str, element: &Element, out: &mut Vec<String>) {
    if element.namespace.as_deref() == Some(W) {
        let container = element.local_name();
        if let Some(order) = sequence(container) {
            let mut last: Option<(usize, String)> = None;
            for child in element.child_elements() {
                match rank(container, order, child) {
                    None => out.push(format!(
                        "{part}: a {container} holds {}, which its schema does not",
                        name_of(child)
                    )),
                    Some(at) => {
                        if let Some((before, name)) = &last {
                            if at < *before {
                                out.push(format!(
                                    "{part}: in a {container}, {} comes after {name}: {:?}",
                                    name_of(child),
                                    names(element)
                                ));
                                continue;
                            }
                        }
                        last = Some((at, name_of(child)));
                    }
                }
            }
        }
    }
    for child in element.child_elements() {
        faults_under(part, child, out);
    }
}

/// The names of an element's children, as the lists write them.
fn names(element: &Element) -> Vec<String> {
    element.child_elements().map(name_of).collect()
}

/// Every part of a document as it is saved, read back.
///
/// A part that does not parse fails here, by name. Passing over it was how a
/// styles part written with a prefix it never declared went unseen: the
/// walker found nothing out of order in a part it had not looked at.
fn saved_parts(document: &Document) -> Vec<(String, XmlTree)> {
    let bytes = document.save().expect("saving");
    let package = wp_opc::Package::open(&bytes).expect("a package");
    package
        .content_parts()
        .filter(|entry| entry.name.ends_with(".xml"))
        .map(|entry| {
            let name = &entry.name;
            let text = match package.xml_part(name) {
                Some(Ok(text)) => text,
                other => panic!("{name} cannot be read as text: {other:?}"),
            };
            let tree = XmlTree::parse(&text)
                .unwrap_or_else(|error| panic!("{name} does not parse: {error:?}"));
            (name.clone(), tree)
        })
        .collect()
}

/// Every fault in every part of a document as it is saved.
fn faults(document: &Document) -> Vec<String> {
    let mut out = Vec::new();
    for (name, tree) in saved_parts(document) {
        faults_under(&name, &tree.root, &mut out);
    }
    out
}

fn assert_in_order(document: &Document) {
    let found = faults(document);
    assert!(found.is_empty(), "{}", found.join("\n"));
}

/// The one element of a name at or under a root, found depth first.
fn first<'a>(root: &'a Element, local: &str) -> Option<&'a Element> {
    if root.namespace.as_deref() == Some(W) && root.local_name() == local {
        return Some(root);
    }
    root.child_elements().find_map(|child| first(child, local))
}

/// A document of a few paragraphs, as a person would have it open.
fn document(paragraphs: &[&str]) -> Document {
    let mut body = Body::default();
    for text in paragraphs {
        body.blocks.push(Block::Paragraph(Paragraph::text(text)));
    }
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

fn line() -> Border {
    Border::line("single", 4, Some("000000"))
}

fn every_paragraph_property() -> ParagraphProperties {
    ParagraphProperties {
        style: Some("Heading1".to_owned()),
        alignment: Some(Alignment::Center),
        right_to_left: Some(false),
        indent_start: Some(720),
        indent_end: Some(360),
        indent_first_line: Some(-360),
        space_before: Some(120),
        space_after: Some(60),
        line_spacing: Some(LineSpacing { value: 360, rule: LineRule::Auto }),
        keep_next: Some(true),
        keep_lines: Some(true),
        page_break_before: Some(false),
        widow_control: Some(true),
        outline_level: Some(1),
        numbering: Some(NumberingReference { id: wp_docx::BULLET_LIST, level: 0 }),
        borders: ParagraphBorders::box_all(),
        shading: Some("FFFF00".to_owned()),
        tab_stops: vec![TabStop {
            position: 1440,
            alignment: TabAlignment::Center,
            leader: TabLeader::Dot,
        }],
        contextual_spacing: Some(true),
        mirror_indents: Some(true),
        suppress_line_numbers: Some(true),
        no_hyphenation: Some(true),
    }
}

fn every_run_property() -> RunProperties {
    RunProperties {
        style: Some("Emphasis".to_owned()),
        bold: Some(true),
        italic: Some(true),
        strike: Some(true),
        underline: Some(Underline::Double),
        size_half_points: Some(28),
        color: Some("C00000".to_owned()),
        highlight: Some("yellow".to_owned()),
        vertical_align: Some(VerticalAlignment::Superscript),
        font: Some("Cambria".to_owned()),
        right_to_left: Some(false),
        language: Some("en-GB".to_owned()),
        color_theme: Some(ThemeColor { slot: Slot::Accent1, tint: Some(0x99), shade: None }),
        effect: Some(TextEffect::plain(Effect::Glow)),
        font_theme: Some(FontSlot::Major),
        double_strike: Some(false),
        caps: Some(true),
        small_caps: Some(false),
        hidden: Some(false),
        no_proof: Some(true),
        underline_color: Some("0070C0".to_owned()),
        scale: Some(150),
        spacing_twentieths: Some(-20),
        position_half_points: Some(6),
        kerning_half_points: Some(16),
        open_type: Some(OpenType {
            ligatures: Ligatures::StandardContextual,
            number_spacing: NumberSpacing::Tabular,
            number_forms: NumberForms::OldStyle,
            stylistic_sets: vec![3],
            contextual_alternates: true,
        }),
        east_asian_layout: Some(EastAsianLayout {
            horizontal_in_vertical: true,
            fit_in_line: true,
            two_lines_in_one: false,
            brackets: CombineBrackets::None,
        }),
    }
}

/// A table with everything its model can say, of each of its three kinds of
/// properties.
fn every_table_property() -> Table {
    let cell = |text: &str| TableCell {
        width: Some(2000),
        borders: TableBorders::grid(),
        shading: Some("D9E2F3".to_owned()),
        direction: TextDirection::Up,
        margins: CellMargins { top: Some(40), start: Some(80), bottom: Some(40), end: Some(80) },
        vertical: CellAlignment::Bottom,
        ..TableCell::text(text)
    };
    let mut merged = cell("across");
    merged.span = 2;
    let mut top = TableRow::from_cells(vec![merged, cell("alone")]);
    top.height = Some(400);
    top.height_exact = true;
    top.is_header = true;
    let mut under = cell("under");
    under.merged_upwards = false;
    let mut below = cell("below");
    below.merged_upwards = true;
    let bottom = TableRow::from_cells(vec![under, cell("middle"), below]);
    let mut table = Table::from_rows(vec![top, bottom])
        .with_style("TableGrid")
        .with_grid(vec![2000, 2000, 2000])
        .with_borders(TableBorders::grid());
    table.fit = TableFit::Fixed;
    table.cell_spacing = Some(20);
    table.cell_margins =
        CellMargins { top: Some(0), start: Some(108), bottom: Some(0), end: Some(108) };
    table.indent = -108;
    table.look = TableLook { last_row: true, banded_columns: true, ..TableLook::default() };
    table
}

// --- The paragraph ------------------------------------------------------------

#[test]
fn a_paragraphs_properties_from_the_model_come_out_in_the_schemas_order() {
    let element =
        wp_docx::edit::paragraph_properties_element(&every_paragraph_property(), Some("w"));
    assert_eq!(
        names(&element),
        [
            "pStyle",
            "keepNext",
            "keepLines",
            "pageBreakBefore",
            "widowControl",
            "numPr",
            "suppressLineNumbers",
            "pBdr",
            "shd",
            "tabs",
            "suppressAutoHyphens",
            "bidi",
            "spacing",
            "ind",
            "contextualSpacing",
            "mirrorIndents",
            "jc",
            "outlineLvl",
        ]
    );
}

/// The paragraph's properties set the way a person sets them, a dialog and a
/// button at a time, in no order the schema has.
fn paragraph_set_piece_by_piece() -> Document {
    let mut document = document(&["The paragraph", "The next one"]);
    document.set_caret(TextPosition::new(0, 3));
    let piece = |change: ParagraphProperties| change;
    document.set_alignment_here(Alignment::End);
    document.set_paragraph_format(&piece(ParagraphProperties {
        contextual_spacing: Some(true),
        mirror_indents: Some(true),
        ..ParagraphProperties::default()
    }));
    document.set_line_spacing_here(Some(LineSpacing { value: 240, rule: LineRule::AtLeast }));
    document.set_indents_here(720, -360, 360);
    document.set_shading_here(Some("FFFF00"));
    document.set_paragraph_format(&piece(ParagraphProperties {
        keep_next: Some(true),
        keep_lines: Some(true),
        page_break_before: Some(true),
        widow_control: Some(false),
        suppress_line_numbers: Some(true),
        no_hyphenation: Some(true),
        space_before: Some(240),
        // Said again here: the dialog says a paragraph's level every time,
        // and a change that names none makes it body text.
        outline_level: Some(2),
        ..ParagraphProperties::default()
    }));
    document.set_tab_stops_here(&[TabStop {
        position: 2880,
        alignment: TabAlignment::Decimal,
        leader: TabLeader::None,
    }]);
    document.set_borders_here(&ParagraphBorders::box_all());
    document.set_list_here(Some(NumberingReference { id: wp_docx::NUMBERED_LIST, level: 1 }));
    document.set_paragraph_style_here(Some("Heading2"));
    // And the end of a section on it, last of all.
    let end = document.paragraph_text(0).expect("the paragraph").len();
    document.set_caret(TextPosition::new(0, end));
    assert!(document.insert_section_break(Start::Continuous));
    document
}

#[test]
fn a_paragraphs_properties_set_one_at_a_time_come_out_in_the_schemas_order() {
    let document = paragraph_set_piece_by_piece();
    let parts = saved_parts(&document);
    let (_, main) = parts.iter().find(|(name, _)| name == "word/document.xml").expect("the body");
    let properties = first(&main.root, "pPr").expect("the paragraph's properties");
    assert_eq!(
        names(properties),
        [
            "pStyle",
            "keepNext",
            "keepLines",
            "pageBreakBefore",
            "widowControl",
            "numPr",
            "suppressLineNumbers",
            "pBdr",
            "shd",
            "tabs",
            "suppressAutoHyphens",
            "spacing",
            "ind",
            "contextualSpacing",
            "mirrorIndents",
            "jc",
            "outlineLvl",
            "sectPr",
        ]
    );
    assert_in_order(&document);
}

// --- The run ------------------------------------------------------------------

#[test]
fn a_runs_properties_from_the_model_come_out_in_the_schemas_order() {
    let element = wp_docx::edit::run_properties_element(&every_run_property(), Some("w"));
    assert_eq!(
        names(&element),
        [
            "rStyle",
            "rFonts",
            "b",
            "bCs",
            "i",
            "iCs",
            "caps",
            "smallCaps",
            "strike",
            "dstrike",
            "noProof",
            "vanish",
            "color",
            "spacing",
            "w",
            "kern",
            "position",
            "sz",
            "szCs",
            "highlight",
            "u",
            "vertAlign",
            "rtl",
            "lang",
            "eastAsianLayout",
            "w14:glow",
            "w14:ligatures",
            "w14:numForm",
            "w14:numSpacing",
            "w14:stylisticSets",
            "w14:cntxtAlts",
        ]
    );
}

/// The run's properties set a few at a time over a selection, in no order the
/// schema has: the language first and the style last, a text effect in the
/// middle, and the formatting tracked from halfway through.
fn run_set_piece_by_piece() -> Document {
    let mut document = document(&["Some words to format"]);
    document.set_caret(TextPosition::new(0, 0));
    document.extend_selection_to(TextPosition::new(0, 10));
    let every = every_run_property();
    let pieces = [
        RunProperties { language: every.language.clone(), ..RunProperties::default() },
        RunProperties { east_asian_layout: every.east_asian_layout, ..RunProperties::default() },
        RunProperties { open_type: every.open_type.clone(), ..RunProperties::default() },
        RunProperties { highlight: every.highlight.clone(), ..RunProperties::default() },
        RunProperties { vertical_align: every.vertical_align, ..RunProperties::default() },
        RunProperties { size_half_points: every.size_half_points, ..RunProperties::default() },
        RunProperties { color: every.color.clone(), ..RunProperties::default() },
        RunProperties { kerning_half_points: Some(20), ..RunProperties::default() },
        RunProperties { position_half_points: Some(4), ..RunProperties::default() },
        RunProperties { spacing_twentieths: Some(10), ..RunProperties::default() },
        RunProperties { scale: Some(80), ..RunProperties::default() },
    ];
    for piece in &pieces {
        assert!(document.set_character_format(piece));
    }
    assert!(document.set_text_effect(Effect::Glow));
    document.set_tracking_changes(true);
    let rest = [
        RunProperties { no_proof: Some(true), hidden: Some(true), ..RunProperties::default() },
        RunProperties { small_caps: Some(true), caps: Some(false), ..RunProperties::default() },
        RunProperties {
            double_strike: Some(true),
            strike: Some(false),
            ..RunProperties::default()
        },
        RunProperties { italic: Some(true), bold: Some(true), ..RunProperties::default() },
        RunProperties { font: every.font.clone(), ..RunProperties::default() },
        RunProperties {
            underline: every.underline.clone(),
            underline_color: every.underline_color.clone(),
            ..RunProperties::default()
        },
        RunProperties { right_to_left: Some(false), ..RunProperties::default() },
        RunProperties { style: every.style.clone(), ..RunProperties::default() },
    ];
    for piece in &rest {
        assert!(document.set_character_format(piece));
    }
    document
}

#[test]
fn a_runs_properties_set_a_few_at_a_time_come_out_in_the_schemas_order() {
    let document = run_set_piece_by_piece();
    let parts = saved_parts(&document);
    let (_, main) = parts.iter().find(|(name, _)| name == "word/document.xml").expect("the body");
    let properties = first(&main.root, "rPr").expect("the run's properties");
    assert_eq!(
        names(properties),
        [
            "rStyle",
            "rFonts",
            "b",
            "bCs",
            "i",
            "iCs",
            "caps",
            "smallCaps",
            "strike",
            "dstrike",
            "noProof",
            "vanish",
            "color",
            "spacing",
            "w",
            "kern",
            "position",
            "sz",
            "szCs",
            "highlight",
            "u",
            "vertAlign",
            "rtl",
            "cs",
            "lang",
            "eastAsianLayout",
            "w14:glow",
            "w14:ligatures",
            "w14:numForm",
            "w14:numSpacing",
            "w14:stylisticSets",
            "w14:cntxtAlts",
            "rPrChange",
        ]
    );
    assert_in_order(&document);
}

#[test]
fn a_link_puts_its_look_among_what_the_run_already_says() {
    let mut document = document(&["See the site here"]);
    document.set_caret(TextPosition::new(0, 8));
    document.extend_selection_to(TextPosition::new(0, 12));
    assert!(document.set_character_format(&RunProperties {
        size_half_points: Some(24),
        language: Some("en-GB".to_owned()),
        font: Some("Arial".to_owned()),
        ..RunProperties::default()
    }));
    assert!(document.add_hyperlink("https://example.com/", ""));
    assert_in_order(&document);
}

// --- The table, its rows and its cells ------------------------------------------

#[test]
fn a_tables_properties_from_the_model_come_out_in_the_schemas_order() {
    let element = wp_docx::edit::table_element(&every_table_property(), Some("w"));
    let properties = element.child(Some(W), "tblPr").expect("the table's properties");
    assert_eq!(
        names(properties),
        [
            "tblStyle",
            "tblW",
            "tblCellSpacing",
            "tblInd",
            "tblBorders",
            "tblLayout",
            "tblCellMar",
            "tblLook"
        ]
    );
    let row = element.child(Some(W), "tr").expect("a row");
    assert_eq!(names(row.child(Some(W), "trPr").expect("the row's")), ["trHeight", "tblHeader"]);
    let mut cells = row.children_named(Some(W), "tc");
    let spanning = cells.next().expect("a cell across two columns");
    assert_eq!(
        names(spanning.child(Some(W), "tcPr").expect("its properties")),
        ["tcW", "gridSpan", "tcBorders", "shd", "tcMar", "textDirection", "vAlign"]
    );
    let merged = cells.next().expect("a cell merged with the one below it");
    assert_eq!(
        names(merged.child(Some(W), "tcPr").expect("its properties")),
        ["tcW", "vMerge", "tcBorders", "shd", "tcMar", "textDirection", "vAlign"]
    );

    let mut body = Body::default();
    body.blocks.push(Block::Table(Box::new(every_table_property())));
    assert_in_order(&Document::create(&body).expect("a document"));
}

/// A table put in by the command, and every property of it, its first row and
/// its first cell set from the Table Properties dialog and the ribbon, in no
/// order the schema has.
fn table_set_piece_by_piece() -> Document {
    let mut document = document(&["Before the table"]);
    document.set_caret(TextPosition::new(0, 0));
    assert!(document.insert_table(2, 3));
    // The table's own.
    assert!(document.set_table_alt_text("Totals", "What each month came to"));
    assert!(document.set_table_look(TableLook {
        first_row: true,
        last_row: false,
        first_column: true,
        last_column: false,
        banded_rows: true,
        banded_columns: false,
    }));
    assert!(document.set_table_cell_margins(CellMargins {
        top: Some(20),
        start: Some(100),
        bottom: Some(20),
        end: Some(100),
    }));
    assert!(document.set_table_fit(TableFit::Fixed));
    assert!(document.set_table_borders(&TableBorders::grid()));
    assert!(document.set_table_indent(360));
    assert!(document.set_table_cell_spacing(Some(30)));
    assert!(document.set_table_alignment(Alignment::Center));
    assert!(document.set_table_style(Some("TableGrid")));
    // The first row's.
    assert!(document.set_table_header_row(true));
    assert!(document.set_row_can_break(false));
    assert!(document.set_table_row_height(Some(500), true));
    // The first cell's.
    assert!(document.set_cell_alignment(CellAlignment::Middle));
    assert!(document.set_cell_direction(TextDirection::Down));
    assert!(document.set_cell_shading(Some("FFC000")));
    let here = document.caret();
    assert!(document.set_cell_edge(here, CellEdge::Bottom, Some(&line())));
    assert!(document.set_cell_width(Some(1800)));
    document
}

#[test]
fn a_tables_properties_set_one_at_a_time_come_out_in_the_schemas_order() {
    let document = table_set_piece_by_piece();
    let parts = saved_parts(&document);
    let (_, main) = parts.iter().find(|(name, _)| name == "word/document.xml").expect("the body");
    let table = first(&main.root, "tbl").expect("the table");
    assert_eq!(
        names(table.child(Some(W), "tblPr").expect("the table's properties")),
        [
            "tblStyle",
            "tblW",
            "jc",
            "tblCellSpacing",
            "tblInd",
            "tblBorders",
            "tblLayout",
            "tblCellMar",
            "tblLook",
            "tblCaption",
            "tblDescription",
        ]
    );
    let row = table.child(Some(W), "tr").expect("the first row");
    assert_eq!(
        names(row.child(Some(W), "trPr").expect("the row's")),
        ["cantSplit", "trHeight", "tblHeader"]
    );
    let cell = row.child(Some(W), "tc").expect("the first cell");
    assert_eq!(
        names(cell.child(Some(W), "tcPr").expect("the cell's")),
        ["tcW", "tcBorders", "shd", "textDirection", "vAlign"]
    );
    assert_in_order(&document);
}

// --- The section ----------------------------------------------------------------

/// The page set up a setting at a time, the first page's header last of all
/// but one — which is how a `w:titlePg` came to stand between the references.
fn section_set_piece_by_piece() -> Document {
    let mut document = document(&["The only section"]);
    document.set_caret(TextPosition::new(0, 0));
    assert!(document.set_columns(2, 360));
    assert!(document
        .set_furniture(Furniture::Footer, Preset::PageNumber, Alignment::Center, "")
        .expect("a footer"));
    assert!(document.set_text_direction(TextDirection::Down));
    assert!(document
        .set_page_numbering(PageNumbering { start: Some(3), format: NumberFormat::LowerRoman }));
    assert!(document.set_line_numbers(Some(LineNumbers {
        count_by: 5,
        start: 1,
        restart: Restart::NewPage,
        distance: None,
    })));
    assert!(document.set_page_borders(&PageBorders::box_all(&line())));
    assert!(document.set_furniture_distances(600, 600));
    assert!(document.set_page_margins(1000, 1000, 1000, 1000));
    assert!(document.set_different_first_page(true));
    assert!(document
        .set_furniture_for(Furniture::Header, Which::First, Preset::Text, Alignment::Start, "First")
        .expect("a first page's header"));
    assert!(document
        .set_furniture(Furniture::Header, Preset::Text, Alignment::Start, "Every page")
        .expect("a header"));
    assert!(document.set_page_size(12240, 15840));
    document
}

#[test]
fn a_sections_properties_set_one_at_a_time_come_out_in_the_schemas_order() {
    let document = section_set_piece_by_piece();
    let parts = saved_parts(&document);
    let (_, main) = parts.iter().find(|(name, _)| name == "word/document.xml").expect("the body");
    let section = first(&main.root, "sectPr").expect("the section's properties");
    let found = names(section);
    // The three references, in whatever order among themselves, then the
    // rest in the schema's.
    let (references, rest) = found.split_at(3);
    let mut references = references.to_vec();
    references.sort();
    assert_eq!(references, ["footerReference", "headerReference", "headerReference"]);
    assert_eq!(
        rest,
        [
            "pgSz",
            "pgMar",
            "pgBorders",
            "lnNumType",
            "pgNumType",
            "cols",
            "titlePg",
            "textDirection"
        ]
    );
    assert_in_order(&document);
}

// --- Everything at once ------------------------------------------------------

#[test]
fn the_walker_finds_a_property_out_of_order_or_out_of_place() {
    // Written by hand the way the writer used to write it, so that a walker
    // that looked at nothing would fail here rather than pass everywhere.
    let bytes = document(&["Words"]).save().expect("saving");
    let mut package = wp_opc::Package::open(&bytes).expect("a package");
    let main = "word/document.xml";
    let text = package.xml_part(main).expect("the body").expect("readable");
    let marked = text.replacen(
        "<w:r>",
        "<w:r><w:rPr><w:strike/><w:caps/><w:w w:val=\"90\"/><w:spacing w:val=\"4\"/>\
         <w:uColor/></w:rPr>",
        1,
    );
    package.set_part(main, marked.into_bytes());
    let bytes = package.save().expect("saving the package");
    let found = faults(&Document::open(&bytes).expect("reopening"));
    assert_eq!(found.len(), 3, "{found:#?}");
    assert!(found[0].contains("caps comes after strike"), "{}", found[0]);
    assert!(found[1].contains("spacing comes after w"), "{}", found[1]);
    assert!(found[2].contains("holds uColor"), "{}", found[2]);
}

#[test]
#[should_panic(expected = "word/styles.xml does not parse")]
fn the_walker_fails_on_a_part_that_does_not_parse() {
    // A style with Word 2010's ligatures and no declaration of the prefix,
    // as the styles part was written before G20: passed over, it was a part
    // with nothing out of order in it.
    let bytes = document(&["Words"]).save().expect("saving");
    let mut package = wp_opc::Package::open(&bytes).expect("a package");
    let part = "word/styles.xml";
    let text = package.xml_part(part).expect("the styles").expect("readable");
    let broken = text.replacen(
        "</w:styles>",
        "<w:style w:type=\"character\" w:styleId=\"Ligated\"><w:rPr>\
         <w14:ligatures w14:val=\"standard\"/></w:rPr></w:style></w:styles>",
        1,
    );
    package.set_part(part, broken.into_bytes());
    let bytes = package.save().expect("saving the package");
    faults(&Document::open(&bytes).expect("it opens, with the default styles"));
}

/// A body with everything the model can say, a paragraph and a run of it
/// with every property, a table with every property of each kind, and runs
/// of formatting tracked as changed.
fn rich_body() -> Body {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("Title").with_style("Title")));
    let mut paragraph = Paragraph::from_runs(vec![
        Run::text("plain "),
        Run {
            properties: every_run_property(),
            content: vec![RunContent::Text("formatted".to_owned())],
            field: None,
            revision: None,
            format_change: Some(wp_docx::model::FormatChange {
                author: "Somebody".to_owned(),
                date: "2026-09-30T00:00:00Z".to_owned(),
                id: 1,
                before: Box::new(every_run_property()),
            }),
        },
    ]);
    paragraph.properties = every_paragraph_property();
    body.blocks.push(Block::Paragraph(paragraph));
    body.blocks.push(Block::Table(Box::new(every_table_property())));
    body.blocks.push(Block::Paragraph(Paragraph::text("After the table")));
    body
}

#[test]
fn nothing_the_writer_makes_has_a_property_out_of_order() {
    // Each of the documents above, which between them are every command that
    // writes into one of these containers, and the model written whole.
    for document in [
        paragraph_set_piece_by_piece(),
        run_set_piece_by_piece(),
        table_set_piece_by_piece(),
        section_set_piece_by_piece(),
        Document::create(&rich_body()).expect("a document"),
    ] {
        assert_in_order(&document);
    }

    // And one with nearly everything done to it: the rich body opened, its
    // styles and defaults changed, comments, notes, headers, links, lists,
    // tables and sections added, and some of it pasted again.
    let bytes = Document::create(&rich_body()).expect("a document").save().expect("saving");
    let mut document = Document::open(&bytes).expect("reopening");
    assert!(document.set_style(&StyleDefinition {
        id: "Quote".to_owned(),
        name: "Quote".to_owned(),
        based_on: Some("Normal".to_owned()),
        next: None,
        paragraph: every_paragraph_property(),
        run: every_run_property(),
    }));
    assert!(document.set_default_paragraph_format(&ParagraphProperties {
        keep_lines: Some(true),
        widow_control: Some(true),
        alignment: Some(Alignment::Both),
        space_after: Some(120),
        ..ParagraphProperties::default()
    }));
    assert!(document.set_default_character_format(&RunProperties {
        language: Some("en-US".to_owned()),
        color: Some("333333".to_owned()),
        font: Some("Georgia".to_owned()),
        ..RunProperties::default()
    }));
    // Still there after the defaults. A styles part that does not parse is
    // taken for none by the next command that edits it, which starts again
    // from the default styles; so a style written with a prefix it never
    // declared was lost here, and the part saved after it parsed.
    assert!(document.styles().get("Quote").is_some(), "the style set from its definition was lost");
    document.set_caret(TextPosition::new(0, 0));
    document.extend_selection_to(TextPosition::new(0, 5));
    document.add_comment("A remark", "Somebody", "2026-09-30T00:00:00Z").expect("a comment");
    document.clear_selection();
    document.set_caret(TextPosition::new(0, 5));
    document.add_note(wp_docx::notes::Kind::Footnote, "A note").expect("a footnote");
    assert!(document
        .set_furniture(Furniture::Header, Preset::PageOfTotal, Alignment::End, "")
        .expect("a header"));
    let last = document.paragraph_count() - 1;
    document.set_caret(TextPosition::new(last, 0));
    assert!(document.insert_section_break(Start::OddPage));
    assert!(document.set_different_first_page(true));
    assert!(document
        .set_furniture_for(Furniture::Footer, Which::First, Preset::Text, Alignment::Center, "1")
        .expect("a first page's footer"));
    document.set_caret(TextPosition::new(1, 0));
    document.extend_selection_to(TextPosition::new(1, 5));
    let copied = document.copy_selection();
    document.set_caret(TextPosition::new(0, 5));
    assert!(document.paste_blocks(&copied));
    assert_in_order(&document);
}
