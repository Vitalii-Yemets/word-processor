//! The spreadsheet behind a chart.
//!
//! # Why a chart carries a workbook
//!
//! Word draws a chart from the numbers cached in the chart part, and keeps
//! the spreadsheet those numbers came from beside it, in
//! `word/embeddings`. That workbook is what opens when somebody asks Word
//! to edit the data, and what Word rewrites the caches from afterwards. A
//! chart with no workbook draws, prints and reads correctly everywhere, but
//! Word's Edit Data does nothing for it — so every chart made here is given
//! one, laid out the way Word lays it out: the categories down column A,
//! one series per column after it with its name in the first row.
//!
//! # And why it is read
//!
//! Because not every writer caches the numbers. A chart part may say only
//! where its numbers are — `Sheet1!$B$2:$B$5` — and leave the values to the
//! workbook; Word reads them from there, and a reader that does not draws
//! an empty chart. So when a cache is missing, the reference is followed
//! into the workbook.
//!
//! A workbook is a package like the document itself, and is read and
//! written with the same code.

use std::collections::HashMap;

use wp_opc::{Package, Relationships, TargetMode};
use wp_xml::tree::{Element, XmlTree};

use crate::chart::{Chart, Kind};

/// What the package calls a workbook part.
pub const WORKBOOK_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";
/// The relationship from a chart to the workbook behind it.
pub const PACKAGE_RELATIONSHIP: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/package";

const SPREADSHEET_NAMESPACE: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const RELATIONSHIPS_NAMESPACE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const WORKBOOK_RELATIONSHIP: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
const WORKSHEET_RELATIONSHIP: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet";
const SHARED_STRINGS_RELATIONSHIP: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings";
const WORKBOOK_PART_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml";
const WORKSHEET_PART_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml";
const SHARED_STRINGS_PART_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml";

/// What a cell holds.
#[derive(Clone, Debug, PartialEq)]
pub enum Cell {
    Number(f64),
    Text(String),
}

impl Cell {
    /// The number, or nothing for words.
    #[must_use]
    pub fn number(&self) -> Option<f64> {
        match self {
            Self::Number(value) => Some(*value),
            Self::Text(text) => text.trim().parse().ok(),
        }
    }

    /// The words, or the number written out.
    #[must_use]
    pub fn text(&self) -> String {
        match self {
            Self::Number(value) => crate::numberformat::general(*value),
            Self::Text(text) => text.clone(),
        }
    }
}

/// One sheet of a workbook: its cells by row and column, both counted from
/// nought, and its name.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sheet {
    pub name: String,
    cells: HashMap<(u32, u32), Cell>,
}

impl Sheet {
    /// The cells a reference names, in the order it names them: down a
    /// column or along a row.
    ///
    /// The reference is written the way a chart writes one —
    /// `Sheet1!$B$2:$B$5`, or one cell as `Sheet1!$B$1` — and a reference to
    /// another sheet, or one nothing here can read, names no cells.
    #[must_use]
    pub fn range(&self, reference: &str) -> Vec<Option<&Cell>> {
        let Some((sheet, cells)) = split_reference(reference) else { return Vec::new() };
        if !sheet.eq_ignore_ascii_case(&self.name) {
            return Vec::new();
        }
        let (from, to) = match cells.split_once(':') {
            Some((from, to)) => (from, to),
            None => (cells, cells),
        };
        let (Some((from_column, from_row)), Some((to_column, to_row))) =
            (cell_address(from), cell_address(to))
        else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for row in from_row.min(to_row)..=from_row.max(to_row) {
            for column in from_column.min(to_column)..=from_column.max(to_column) {
                out.push(self.cells.get(&(row, column)));
            }
        }
        out
    }

    /// Whether the sheet has any cell at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }
}

/// The sheet and the cells of a reference: `Sheet1!$B$2:$B$5` is the sheet
/// `Sheet1` and the cells `$B$2:$B$5`; a sheet with a space in its name is
/// quoted.
fn split_reference(reference: &str) -> Option<(String, &str)> {
    let (sheet, cells) = reference.trim().rsplit_once('!')?;
    let sheet = sheet.trim_matches('\'').replace("''", "'");
    Some((sheet, cells))
}

/// A cell's column and row from its address, both from nought: `$B$2` is
/// column 1, row 1.
fn cell_address(address: &str) -> Option<(u32, u32)> {
    let mut column = 0u32;
    let mut row = 0u32;
    let mut seen_letter = false;
    let mut seen_digit = false;
    for character in address.chars() {
        match character {
            '$' => {}
            'A'..='Z' | 'a'..='z' if !seen_digit => {
                seen_letter = true;
                column = column * 26 + (character.to_ascii_uppercase() as u32 - 'A' as u32 + 1);
            }
            '0'..='9' if seen_letter => {
                seen_digit = true;
                row = row * 10 + character.to_digit(10)?;
            }
            _ => return None,
        }
    }
    if !seen_letter || !seen_digit || column == 0 || row == 0 {
        return None;
    }
    Some((column - 1, row - 1))
}

/// A column's letters from its number: nought is `A`, twenty-six `AA`.
fn column_letters(mut column: u32) -> String {
    let mut letters = Vec::new();
    loop {
        letters.push(char::from(b'A' + (column % 26) as u8));
        if column < 26 {
            break;
        }
        column = column / 26 - 1;
    }
    letters.iter().rev().collect()
}

/// The reference a chart writes for a run of cells down a column of the
/// first sheet, rows counted from one.
#[must_use]
pub fn column_reference(column: u32, first_row: u32, last_row: u32) -> String {
    let letters = column_letters(column);
    if first_row == last_row {
        return format!("Sheet1!${letters}${first_row}");
    }
    format!("Sheet1!${letters}${first_row}:${letters}${last_row}")
}

/// The workbook behind a chart, as the bytes of a spreadsheet file.
///
/// Laid out as Word lays it out: the categories down column A from the
/// second row, and one series per column after it, its name in the first
/// row and its numbers under. A scatter or bubble chart has no categories:
/// its first column is the x values, and a bubble chart's series take two
/// columns each, the y values and the sizes.
#[must_use]
pub fn write_workbook(chart: &Chart) -> Vec<u8> {
    let sheet = sheet_of(chart);
    let mut strings: Vec<String> = Vec::new();
    let mut string_index = |text: &str| -> usize {
        match strings.iter().position(|known| known == text) {
            Some(at) => at,
            None => {
                strings.push(text.to_owned());
                strings.len() - 1
            }
        }
    };

    // The cells row by row, which is the order a sheet is written in.
    let mut rows: Vec<(u32, Vec<(u32, &Cell)>)> = Vec::new();
    let mut ordered: Vec<(&(u32, u32), &Cell)> = sheet.cells.iter().collect();
    ordered.sort_by_key(|((row, column), _)| (*row, *column));
    for ((row, column), cell) in ordered {
        match rows.last_mut() {
            Some((last, cells)) if last == row => cells.push((*column, cell)),
            _ => rows.push((*row, vec![(*column, cell)])),
        }
    }

    let mut data = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheetData>"#,
    );
    for (row, cells) in &rows {
        data.push_str(&format!("<row r=\"{}\">", row + 1));
        for (column, cell) in cells {
            let address = format!("{}{}", column_letters(*column), row + 1);
            match cell {
                Cell::Number(value) => {
                    data.push_str(&format!("<c r=\"{address}\"><v>{value}</v></c>"));
                }
                Cell::Text(text) => {
                    let index = string_index(text);
                    data.push_str(&format!("<c r=\"{address}\" t=\"s\"><v>{index}</v></c>"));
                }
            }
        }
        data.push_str("</row>");
    }
    data.push_str("</sheetData></worksheet>");

    let mut shared = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main""#,
    );
    shared.push_str(&format!(" count=\"{0}\" uniqueCount=\"{0}\">", strings.len()));
    for text in &strings {
        shared.push_str("<si><t xml:space=\"preserve\">");
        shared.push_str(&crate::chart::escape(text));
        shared.push_str("</t></si>");
    }
    shared.push_str("</sst>");

    let workbook = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="{SPREADSHEET_NAMESPACE}" xmlns:r="{RELATIONSHIPS_NAMESPACE}"><sheets><sheet name="Sheet1" sheetId="1" r:id="rId1"/></sheets></workbook>"#
    );

    let mut package = Package::empty();
    package.add_part("xl/workbook.xml", WORKBOOK_PART_TYPE, workbook.into_bytes());
    package.add_part("xl/worksheets/sheet1.xml", WORKSHEET_PART_TYPE, data.into_bytes());
    package.add_part("xl/sharedStrings.xml", SHARED_STRINGS_PART_TYPE, shared.into_bytes());

    let mut root = Relationships::new("");
    root.add(WORKBOOK_RELATIONSHIP, "xl/workbook.xml", TargetMode::Internal);
    let mut inside = Relationships::new("xl/workbook.xml");
    inside.add(WORKSHEET_RELATIONSHIP, "worksheets/sheet1.xml", TargetMode::Internal);
    inside.add(SHARED_STRINGS_RELATIONSHIP, "sharedStrings.xml", TargetMode::Internal);
    // Neither can fail: the names are this module's own and well formed.
    let _ = package.set_relationships(&root);
    let _ = package.set_relationships(&inside);
    package.save().unwrap_or_default()
}

/// The chart's numbers laid out as cells.
fn sheet_of(chart: &Chart) -> Sheet {
    let mut cells = HashMap::new();
    let mut put = |row: u32, column: u32, cell: Cell| {
        cells.insert((row, column), cell);
    };
    match chart.kind {
        Kind::Scatter | Kind::Bubble => {
            put(0, 0, Cell::Text("X".to_owned()));
            let mut column = 1;
            for series in &chart.series {
                put(0, column, Cell::Text(series.name.clone()));
                for (row, value) in series.values.iter().enumerate() {
                    put(row as u32 + 1, column, Cell::Number(*value));
                }
                for (row, x) in series.xs.iter().enumerate() {
                    put(row as u32 + 1, 0, Cell::Number(*x));
                }
                if chart.kind == Kind::Bubble {
                    put(0, column + 1, Cell::Text(format!("{} size", series.name)));
                    for (row, size) in series.sizes.iter().enumerate() {
                        put(row as u32 + 1, column + 1, Cell::Number(*size));
                    }
                    column += 1;
                }
                column += 1;
            }
        }
        _ => {
            for (row, name) in chart.categories.iter().enumerate() {
                put(row as u32 + 1, 0, Cell::Text(name.clone()));
            }
            for (column, series) in chart.series.iter().enumerate() {
                put(0, column as u32 + 1, Cell::Text(series.name.clone()));
                for (row, value) in series.values.iter().enumerate() {
                    put(row as u32 + 1, column as u32 + 1, Cell::Number(*value));
                }
            }
        }
    }
    Sheet { name: "Sheet1".to_owned(), cells }
}

/// Reads the first sheet of a workbook.
#[must_use]
pub fn read_first_sheet(bytes: &[u8]) -> Option<Sheet> {
    let package = Package::open(bytes).ok()?;
    let root = package.relationships("").ok()?;
    let workbook_part = root.single_by_type(WORKBOOK_RELATIONSHIP)?.resolved_target("")?.ok()?;
    let workbook = parse_part(&package, &workbook_part)?;
    let relationships = package.relationships(&workbook_part).ok()?;

    // The first sheet the workbook names, followed through its relationship.
    let sheets = workbook.root.child(Some(SPREADSHEET_NAMESPACE), "sheets")?;
    let first = sheets.child(Some(SPREADSHEET_NAMESPACE), "sheet")?;
    let name = first.attribute(None, "name").unwrap_or("Sheet1").to_owned();
    let id = first.attribute(Some(RELATIONSHIPS_NAMESPACE), "id")?;
    let sheet_part = relationships.by_id(id)?.resolved_target(&workbook_part)?.ok()?;
    let sheet = parse_part(&package, &sheet_part)?;

    let strings = relationships
        .single_by_type(SHARED_STRINGS_RELATIONSHIP)
        .and_then(|found| found.resolved_target(&workbook_part)?.ok())
        .and_then(|part| parse_part(&package, &part))
        .map(|tree| shared_strings(&tree.root))
        .unwrap_or_default();

    let mut cells = HashMap::new();
    let data = sheet.root.child(Some(SPREADSHEET_NAMESPACE), "sheetData")?;
    for row in data.children_named(Some(SPREADSHEET_NAMESPACE), "row") {
        for cell in row.children_named(Some(SPREADSHEET_NAMESPACE), "c") {
            let Some((column, at)) = cell.attribute(None, "r").and_then(cell_address) else {
                continue;
            };
            let value = cell
                .child(Some(SPREADSHEET_NAMESPACE), "v")
                .map(Element::text_content)
                .unwrap_or_default();
            let held = match cell.attribute(None, "t") {
                // Shared strings are numbered; inline ones and formula
                // strings are written where they stand.
                Some("s") => Cell::Text(
                    value
                        .trim()
                        .parse::<usize>()
                        .ok()
                        .and_then(|index| strings.get(index).cloned())
                        .unwrap_or_default(),
                ),
                Some("inlineStr") => Cell::Text(
                    cell.child(Some(SPREADSHEET_NAMESPACE), "is")
                        .map(Element::text_content)
                        .unwrap_or_default(),
                ),
                Some("str") => Cell::Text(value),
                Some("b") => Cell::Number(if value.trim() == "1" { 1.0 } else { 0.0 }),
                _ => match value.trim().parse::<f64>() {
                    Ok(number) => Cell::Number(number),
                    Err(_) => continue,
                },
            };
            cells.insert((at, column), held);
        }
    }
    Some(Sheet { name, cells })
}

/// A part of a package parsed as XML, or nothing when it is not there or is
/// not XML.
fn parse_part(package: &Package, name: &str) -> Option<XmlTree> {
    let text = package.xml_part(name)?.ok()?;
    XmlTree::parse(&text).ok()
}

/// The shared strings, in the order they are numbered.
fn shared_strings(root: &Element) -> Vec<String> {
    root.children_named(Some(SPREADSHEET_NAMESPACE), "si")
        .map(|item| {
            // A string may be one run of text or several, each with its own
            // formatting; the words are all that is wanted.
            item.text_content()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chart::Series;

    fn chart() -> Chart {
        Chart {
            kind: Kind::Column,
            title: "Sales".to_owned(),
            categories: vec!["North".to_owned(), "South & East".to_owned()],
            series: vec![
                Series {
                    name: "Last year".to_owned(),
                    values: vec![3.0, 5.5],
                    ..Series::default()
                },
                Series {
                    name: "This year".to_owned(),
                    values: vec![4.0, 2.0],
                    ..Series::default()
                },
            ],
            ..Chart::default()
        }
    }

    #[test]
    fn a_cell_address_is_a_column_and_a_row() {
        assert_eq!(cell_address("$B$2"), Some((1, 1)));
        assert_eq!(cell_address("A1"), Some((0, 0)));
        assert_eq!(cell_address("AA10"), Some((26, 9)));
        assert_eq!(cell_address("B"), None);
        assert_eq!(cell_address("2"), None);
    }

    #[test]
    fn column_letters_go_round_at_z() {
        assert_eq!(column_letters(0), "A");
        assert_eq!(column_letters(25), "Z");
        assert_eq!(column_letters(26), "AA");
        assert_eq!(column_letters(27), "AB");
        assert_eq!(column_reference(1, 2, 5), "Sheet1!$B$2:$B$5");
        assert_eq!(column_reference(1, 1, 1), "Sheet1!$B$1");
    }

    #[test]
    fn a_workbook_written_reads_back_with_the_chart_in_it() {
        let bytes = write_workbook(&chart());
        let sheet = read_first_sheet(&bytes).expect("a sheet");
        assert_eq!(sheet.name, "Sheet1");

        let names: Vec<String> =
            sheet.range("Sheet1!$A$2:$A$3").iter().map(|cell| cell.unwrap().text()).collect();
        assert_eq!(names, vec!["North", "South & East"]);
        let last: Vec<f64> = sheet
            .range("Sheet1!$B$2:$B$3")
            .iter()
            .map(|cell| cell.unwrap().number().unwrap())
            .collect();
        assert_eq!(last, vec![3.0, 5.5]);
        assert_eq!(sheet.range("Sheet1!$C$1")[0].unwrap().text(), "This year");
    }

    #[test]
    fn a_reference_to_another_sheet_names_nothing() {
        let sheet = read_first_sheet(&write_workbook(&chart())).expect("a sheet");
        assert!(sheet.range("Sheet2!$A$1").is_empty());
        assert!(sheet.range("nonsense").is_empty());
    }

    #[test]
    fn a_quoted_sheet_name_is_read_without_its_quotes() {
        assert_eq!(split_reference("'My Sheet'!$A$1"), Some(("My Sheet".to_owned(), "$A$1")));
    }

    #[test]
    fn a_scatter_chart_keeps_its_x_values_in_the_first_column() {
        let mut chart = chart();
        chart.kind = Kind::Scatter;
        chart.series[0].xs = vec![1.0, 2.0];
        let sheet = read_first_sheet(&write_workbook(&chart)).expect("a sheet");
        let xs: Vec<f64> = sheet
            .range("Sheet1!$A$2:$A$3")
            .iter()
            .map(|cell| cell.unwrap().number().unwrap())
            .collect();
        assert_eq!(xs, vec![1.0, 2.0]);
    }

    #[test]
    fn a_workbook_is_a_package_of_three_parts() {
        let package = Package::open(&write_workbook(&chart())).expect("a package");
        assert!(package.part("xl/workbook.xml").is_some());
        assert!(package.part("xl/worksheets/sheet1.xml").is_some());
        assert!(package.part("xl/sharedStrings.xml").is_some());
        assert_eq!(package.content_type("xl/workbook.xml"), Some(WORKBOOK_PART_TYPE));
    }
}
