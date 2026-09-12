//! The properties of a table, its rows and its cells.
//!
//! # What is here
//!
//! Everything Word's Table Properties dialog can change: how the table sits on
//! the page and how wide it is, how tall a row is and whether it may break
//! across a page, how wide a cell is and where its text sits, and what a reader
//! who cannot see the table is told about it.
//!
//! A header row is here for two reasons and not one. It repeats the row at the
//! top of every page the table runs onto, which is what it looks like it does.
//! It also tells a screen reader that those cells are the names of the columns,
//! without which the table is a grid of values meaning nothing — see
//! [`crate::accessibility`].

use wp_xml::tree::Element;

use crate::history::EditKind;
use crate::model::{Alignment, TableLook};
use crate::{edit, read, Document};

/// Where the text sits up and down inside a cell.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CellAlignment {
    #[default]
    Top,
    Middle,
    Bottom,
}

impl CellAlignment {
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Top => "top",
            Self::Middle => "center",
            Self::Bottom => "bottom",
        }
    }

    #[must_use]
    pub fn from_word(word: &str) -> Self {
        match word {
            "center" => Self::Middle,
            "bottom" => Self::Bottom,
            _ => Self::Top,
        }
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Top => "Align Top",
            Self::Middle => "Align Middle",
            Self::Bottom => "Align Bottom",
        }
    }

    pub const ALL: &'static [Self] = &[Self::Top, Self::Middle, Self::Bottom];
}

/// The order the schema wants the children of `w:tblPr` in.
const TABLE_PROPERTY_ORDER: &[&str] = &[
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
];

/// And of `w:trPr`.
const ROW_PROPERTY_ORDER: &[&str] = &[
    "cnfStyle",
    "divId",
    "gridBefore",
    "gridAfter",
    "wBefore",
    "wAfter",
    "cantSplit",
    "trHeight",
    "tblHeader",
];

/// And of `w:tcPr`.
const CELL_PROPERTY_ORDER: &[&str] = &[
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
];

impl Document {
    /// How the table at the caret sits across the page.
    #[must_use]
    pub fn table_alignment(&self) -> Option<Alignment> {
        let table = self.table_element_here()?;
        let value = table
            .child(Some(read::W), "tblPr")?
            .child(Some(read::W), "jc")?
            .attribute(Some(read::W), "val")?;
        Alignment::from_attribute(value)
    }

    /// Puts the table at the caret to one side of the page or the middle.
    pub fn set_table_alignment(&mut self, alignment: Alignment) -> bool {
        self.change_table(|table, prefix| {
            let properties = table_properties(table, prefix);
            properties.remove_children_named(Some(read::W), "jc");
            let mut element = Element::new(&edit::name_with(prefix, "jc"), Some(read::W));
            element.set_namespaced_attribute(
                &edit::name_with(prefix, "val"),
                read::W,
                alignment.to_attribute(),
            );
            edit::insert_ordered(properties, element, TABLE_PROPERTY_ORDER);
        })
    }

    /// The style the table at the caret names, if it names one.
    #[must_use]
    pub fn table_style(&self) -> Option<String> {
        let table = self.table_element_here()?;
        table
            .child(Some(read::W), "tblPr")?
            .child(Some(read::W), "tblStyle")?
            .attribute(Some(read::W), "val")
            .map(str::to_owned)
    }

    /// Gives it one, or takes the one it has away.
    pub fn set_table_style(&mut self, style: Option<&str>) -> bool {
        let style = style.map(str::to_owned);
        self.change_table(|table, prefix| {
            let properties = table_properties(table, prefix);
            properties.remove_children_named(Some(read::W), "tblStyle");
            let Some(style) = &style else { return };
            let mut element = Element::new(&edit::name_with(prefix, "tblStyle"), Some(read::W));
            element.set_namespaced_attribute(&edit::name_with(prefix, "val"), read::W, style);
            edit::insert_ordered(properties, element, TABLE_PROPERTY_ORDER);
        })
    }

    /// Which parts of the table at the caret its style may treat specially.
    ///
    /// Word's Table Style Options. See [`TableLook`] for what the six mean and
    /// why two of them are written the other way up in the file.
    #[must_use]
    pub fn table_look(&self) -> Option<TableLook> {
        let table = self.table_element_here()?;
        Some(
            table
                .child(Some(read::W), "tblPr")
                .and_then(|properties| properties.child(Some(read::W), "tblLook"))
                .map(read::read_table_look)
                .unwrap_or_default(),
        )
    }

    /// Says which of them it may.
    pub fn set_table_look(&mut self, wanted: TableLook) -> bool {
        self.change_table(|table, prefix| {
            let properties = table_properties(table, prefix);
            properties.remove_children_named(Some(read::W), "tblLook");

            let name = |local: &str| edit::name_with(prefix, local);
            let mut element = Element::new(&name("tblLook"), Some(read::W));

            // The number as well as the attributes, because a reader that
            // knows only the old form must see the same table as one that
            // knows the new. Word writes both, for the same reason.
            let mut bits = 0x0000u32;
            for (mask, on) in [
                (0x0020, wanted.first_row),
                (0x0040, wanted.last_row),
                (0x0080, wanted.first_column),
                (0x0100, wanted.last_column),
                (0x0200, !wanted.banded_rows),
                (0x0400, !wanted.banded_columns),
            ] {
                if on {
                    bits |= mask;
                }
            }
            element.set_namespaced_attribute(&name("val"), read::W, &format!("{bits:04X}"));

            for (local, on) in [
                ("firstRow", wanted.first_row),
                ("lastRow", wanted.last_row),
                ("firstColumn", wanted.first_column),
                ("lastColumn", wanted.last_column),
                ("noHBand", !wanted.banded_rows),
                ("noVBand", !wanted.banded_columns),
            ] {
                element.set_namespaced_attribute(&name(local), read::W, if on { "1" } else { "0" });
            }
            edit::insert_ordered(properties, element, TABLE_PROPERTY_ORDER);
        })
    }

    /// Whether the first row of the table at the caret is a header.
    #[must_use]
    pub fn table_header_row(&self) -> Option<bool> {
        let table = self.table_element_here()?;
        let row = table.child(Some(read::W), "tr")?;
        Some(
            row.child(Some(read::W), "trPr")
                .and_then(|properties| properties.child(Some(read::W), "tblHeader"))
                .is_some(),
        )
    }

    /// Marks the first row of the table as a header, or stops it being one.
    pub fn set_table_header_row(&mut self, header: bool) -> bool {
        self.change_table(|table, prefix| {
            let Some(row) = table.child_mut(Some(read::W), "tr") else { return };
            let properties = row_properties(row, prefix);
            properties.remove_children_named(Some(read::W), "tblHeader");
            if header {
                let element = Element::new(&edit::name_with(prefix, "tblHeader"), Some(read::W));
                edit::insert_ordered(properties, element, ROW_PROPERTY_ORDER);
            }
        })
    }

    /// How tall the row at the caret is asked to be, in twentieths of a point.
    #[must_use]
    pub fn table_row_height(&self) -> Option<i32> {
        let position = self.table_here()?;
        let table = self.table_element_here()?;
        let row = table.children_named(Some(read::W), "tr").nth(position.row)?;
        row.child(Some(read::W), "trPr")?
            .child(Some(read::W), "trHeight")?
            .attribute(Some(read::W), "val")?
            .parse()
            .ok()
    }

    /// Whether the row's height is a ceiling rather than a floor.
    ///
    /// Word offers both and they are not the same thing at all: text that does
    /// not fit an exact height is cut off, and text that does not fit a
    /// minimum pushes the row taller.
    #[must_use]
    pub fn table_row_height_is_exact(&self) -> bool {
        let Some(position) = self.table_here() else { return false };
        let Some(table) = self.table_element_here() else { return false };
        table
            .children_named(Some(read::W), "tr")
            .nth(position.row)
            .and_then(|row| row.child(Some(read::W), "trPr"))
            .and_then(|properties| properties.child(Some(read::W), "trHeight"))
            .and_then(|height| height.attribute(Some(read::W), "hRule"))
            == Some("exact")
    }

    /// Sets how tall the row at the caret is, or lets it find its own height.
    pub fn set_table_row_height(&mut self, twips: Option<i32>, exact: bool) -> bool {
        let Some(position) = self.table_here() else { return false };
        self.change_table(move |table, prefix| {
            let Some(row) = rows_mut(table).nth(position.row) else { return };
            let properties = row_properties(row, prefix);
            properties.remove_children_named(Some(read::W), "trHeight");
            let Some(twips) = twips else { return };

            let mut element = Element::new(&edit::name_with(prefix, "trHeight"), Some(read::W));
            element.set_namespaced_attribute(
                &edit::name_with(prefix, "val"),
                read::W,
                &twips.to_string(),
            );
            let rule = if exact { "exact" } else { "atLeast" };
            element.set_namespaced_attribute(&edit::name_with(prefix, "hRule"), read::W, rule);
            edit::insert_ordered(properties, element, ROW_PROPERTY_ORDER);
        })
    }

    /// Where the text sits up and down in the cell at the caret.
    #[must_use]
    pub fn cell_alignment(&self) -> Option<CellAlignment> {
        let position = self.table_here()?;
        let table = self.table_element_here()?;
        let row = table.children_named(Some(read::W), "tr").nth(position.row)?;
        let cell = row.children_named(Some(read::W), "tc").nth(position.column)?;
        let value = cell
            .child(Some(read::W), "tcPr")
            .and_then(|properties| properties.child(Some(read::W), "vAlign"))
            .and_then(|element| element.attribute(Some(read::W), "val"))
            .unwrap_or("top");
        Some(CellAlignment::from_word(value))
    }

    /// Sets where the text sits up and down the cell at the caret.
    ///
    /// The one cell, as Word's buttons do with no selection — and as the other
    /// half of the same question already did: which way up the text is set is
    /// [`Document::set_cell_direction`], and it would be strange for one of the
    /// two to change a row and the other a cell.
    pub fn set_cell_alignment(&mut self, alignment: CellAlignment) -> bool {
        let Some(position) = self.table_here() else { return false };
        self.change_table(move |table, prefix| {
            let Some(row) = rows_mut(table).nth(position.row) else { return };
            if let Some(cell) = cells_mut(row).nth(position.column) {
                let properties = cell_properties(cell, prefix);
                properties.remove_children_named(Some(read::W), "vAlign");
                if alignment == CellAlignment::Top {
                    return;
                }
                let mut element = Element::new(&edit::name_with(prefix, "vAlign"), Some(read::W));
                element.set_namespaced_attribute(
                    &edit::name_with(prefix, "val"),
                    read::W,
                    alignment.word(),
                );
                edit::insert_ordered(properties, element, CELL_PROPERTY_ORDER);
            }
        })
    }

    /// Colours the cell at the caret, or takes its colour off.
    ///
    /// Word's Shading, on the Table Design tab. It colours the cell rather than
    /// the paragraph inside it: a cell is what a table is made of, and a colour
    /// on the paragraph would stop at the ends of the text rather than filling
    /// the cell.
    pub fn set_cell_shading(&mut self, fill: Option<&str>) -> bool {
        let Some(position) = self.table_here() else { return false };
        let fill = fill.map(str::to_owned);
        self.change_table(move |table, prefix| {
            let Some(row) = rows_mut(table).nth(position.row) else { return };
            let Some(cell) = cells_mut(row).nth(position.column) else { return };

            let properties = cell_properties(cell, prefix);
            properties.remove_children_named(Some(read::W), "shd");
            let Some(fill) = &fill else { return };

            let name = |local: &str| edit::name_with(prefix, local);
            let mut element = Element::new(&name("shd"), Some(read::W));
            element.set_namespaced_attribute(&name("val"), read::W, "clear");
            element.set_namespaced_attribute(&name("color"), read::W, "auto");
            element.set_namespaced_attribute(&name("fill"), read::W, fill);
            edit::insert_ordered(properties, element, CELL_PROPERTY_ORDER);
        })
    }

    /// The colour behind the cell at the caret, when it has one of its own.
    #[must_use]
    pub fn cell_shading(&self) -> Option<String> {
        let position = self.table_here()?;
        let table = self.table_element_here()?;
        let row = table.children_named(Some(read::W), "tr").nth(position.row)?;
        let cell = row.children_named(Some(read::W), "tc").nth(position.column)?;
        cell.child(Some(read::W), "tcPr")?
            .child(Some(read::W), "shd")?
            .attribute(Some(read::W), "fill")
            .filter(|fill| *fill != "auto")
            .map(str::to_owned)
    }

    /// The widths of the columns of the table round a paragraph, in twentieths
    /// of a point.
    ///
    /// Round a paragraph rather than at the caret, because what asks is a
    /// pointer over a line of some table that the caret is nowhere near. Empty
    /// when there is no table there, and when the table states no grid — which
    /// is a table whose columns nobody has settled.
    #[must_use]
    pub fn table_grid_at(&self, paragraph: usize) -> Vec<i32> {
        let Some(place) = self.table_at(paragraph) else { return Vec::new() };
        let Some(table) = edit::element_at_path(&self.tree().root, &place.table) else {
            return Vec::new();
        };
        let Some(grid) = table.child(Some(read::W), "tblGrid") else { return Vec::new() };
        grid.children_named(Some(read::W), "gridCol")
            .map(|column| {
                column
                    .attribute(Some(read::W), "w")
                    .and_then(|text| text.parse::<i32>().ok())
                    .unwrap_or(0)
            })
            .collect()
    }

    /// The `w:tbl` the caret is in, for reading.
    fn table_element_here(&self) -> Option<&Element> {
        let position = self.table_here()?;
        edit::element_at_path(&self.tree().root, &position.table)
    }

    /// Changes the table at the caret.
    fn change_table(&mut self, change: impl FnOnce(&mut Element, Option<&str>)) -> bool {
        let Some(position) = self.table_here() else { return false };
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);
        let prefix = self.prefix();

        let Some(table) = edit::element_at_path_mut(&mut self.tree_mut().root, &position.table)
        else {
            return false;
        };

        change(table, prefix.as_deref());
        self.mark_modified();
        true
    }
}

/// The rows of a table, to be changed.
fn rows_mut(table: &mut Element) -> impl Iterator<Item = &mut Element> {
    table.child_elements_mut().filter(|child| child.is(Some(read::W), "tr"))
}

/// The cells of a row, to be changed.
fn cells_mut(row: &mut Element) -> impl Iterator<Item = &mut Element> {
    row.child_elements_mut().filter(|child| child.is(Some(read::W), "tc"))
}

/// A table's properties, made if they are missing.
fn table_properties<'a>(table: &'a mut Element, prefix: Option<&str>) -> &'a mut Element {
    if table.child(Some(read::W), "tblPr").is_none() {
        table.insert_element(0, Element::new(&edit::name_with(prefix, "tblPr"), Some(read::W)));
    }
    table.child_mut(Some(read::W), "tblPr").expect("just inserted, or already there")
}

/// A row's properties, made if they are missing.
fn row_properties<'a>(row: &'a mut Element, prefix: Option<&str>) -> &'a mut Element {
    if row.child(Some(read::W), "trPr").is_none() {
        row.insert_element(0, Element::new(&edit::name_with(prefix, "trPr"), Some(read::W)));
    }
    row.child_mut(Some(read::W), "trPr").expect("just inserted, or already there")
}

/// A cell's properties, made if they are missing.
fn cell_properties<'a>(cell: &'a mut Element, prefix: Option<&str>) -> &'a mut Element {
    if cell.child(Some(read::W), "tcPr").is_none() {
        cell.insert_element(0, Element::new(&edit::name_with(prefix, "tcPr"), Some(read::W)));
    }
    cell.child_mut(Some(read::W), "tcPr").expect("just inserted, or already there")
}

impl Document {
    /// How wide the table at the caret asks to be, as a percentage of the text
    /// area, or `None` where it says nothing and fits itself to its contents.
    ///
    /// Word's Table Properties offers inches as well; a percentage is the one
    /// that survives a change of paper, and is what its "Preferred width"
    /// means in a document meant to be printed on either size.
    #[must_use]
    pub fn table_width_percent(&self) -> Option<i32> {
        let table = self.table_element_here()?;
        let width = table.child(Some(read::W), "tblPr")?.child(Some(read::W), "tblW")?;
        if width.attribute(Some(read::W), "type") != Some("pct") {
            return None;
        }
        // The format counts in fiftieths of a per cent, which is a unit nobody
        // types and everybody has to divide by.
        width.attribute(Some(read::W), "w")?.parse::<i32>().ok().map(|fiftieths| fiftieths / 50)
    }

    /// Sets that width, or takes it away so the table fits its contents.
    pub fn set_table_width_percent(&mut self, percent: Option<i32>) -> bool {
        self.change_table(move |table, prefix| {
            let properties = table_properties(table, prefix);
            properties.remove_children_named(Some(read::W), "tblW");

            let mut element = Element::new(&edit::name_with(prefix, "tblW"), Some(read::W));
            match percent {
                Some(percent) => {
                    let fiftieths = percent.clamp(1, 100) * 50;
                    element.set_namespaced_attribute(
                        &edit::name_with(prefix, "w"),
                        read::W,
                        &fiftieths.to_string(),
                    );
                    element.set_namespaced_attribute(
                        &edit::name_with(prefix, "type"),
                        read::W,
                        "pct",
                    );
                }
                // "Auto" is how the format says "work it out", and it is a
                // width of type auto rather than no width at all.
                None => {
                    element.set_namespaced_attribute(&edit::name_with(prefix, "w"), read::W, "0");
                    element.set_namespaced_attribute(
                        &edit::name_with(prefix, "type"),
                        read::W,
                        "auto",
                    );
                }
            }
            edit::insert_ordered(properties, element, TABLE_PROPERTY_ORDER);
        })
    }

    /// The room the table at the caret keeps clear inside every one of its
    /// cells, in twentieths of a point.
    ///
    /// What the table states, which is not the same as what is used: a side it
    /// says nothing about is Word's own default, and a cell may state its own.
    /// See [`crate::model::CellMargins`].
    #[must_use]
    pub fn table_cell_margins(&self) -> crate::model::CellMargins {
        self.table_element_here()
            .and_then(|table| table.child(Some(read::W), "tblPr"))
            .and_then(|properties| properties.child(Some(read::W), "tblCellMar"))
            .map(read::read_cell_margins)
            .unwrap_or_default()
    }

    /// Sets it.
    pub fn set_table_cell_margins(&mut self, margins: crate::model::CellMargins) -> bool {
        self.change_table(move |table, prefix| {
            let properties = table_properties(table, prefix);
            properties.remove_children_named(Some(read::W), "tblCellMar");
            if margins.is_empty() {
                return;
            }
            let element = edit::cell_margins_element("tblCellMar", &margins, prefix);
            edit::insert_ordered(properties, element, TABLE_PROPERTY_ORDER);
        })
    }

    /// How much room the table at the caret leaves between its cells, in
    /// twentieths of a point, or `None` where it leaves none.
    #[must_use]
    pub fn table_cell_spacing(&self) -> Option<i32> {
        self.table_element_here()?
            .child(Some(read::W), "tblPr")?
            .child(Some(read::W), "tblCellSpacing")
            .filter(|spacing| spacing.attribute(Some(read::W), "type") != Some("pct"))
            .and_then(|spacing| spacing.attribute(Some(read::W), "w"))
            .and_then(|text| text.parse::<i32>().ok())
            .filter(|twips| *twips > 0)
    }

    /// Sets it, or takes it away so the cells touch again.
    pub fn set_table_cell_spacing(&mut self, twips: Option<i32>) -> bool {
        self.change_table(move |table, prefix| {
            let properties = table_properties(table, prefix);
            properties.remove_children_named(Some(read::W), "tblCellSpacing");
            let Some(twips) = twips.filter(|twips| *twips > 0) else { return };
            let element = edit::measured(prefix, "tblCellSpacing", twips);
            edit::insert_ordered(properties, element, TABLE_PROPERTY_ORDER);
        })
    }

    /// Which way up the text in the cell at the caret is set.
    #[must_use]
    pub fn cell_direction(&self) -> Option<crate::model::TextDirection> {
        let position = self.table_here()?;
        let table = self.table_element_here()?;
        let row = table.children_named(Some(read::W), "tr").nth(position.row)?;
        let cell = row.children_named(Some(read::W), "tc").nth(position.column)?;
        Some(
            cell.child(Some(read::W), "tcPr")
                .and_then(|properties| properties.child(Some(read::W), "textDirection"))
                .and_then(|element| element.attribute(Some(read::W), "val"))
                .map(crate::model::TextDirection::from_word)
                .unwrap_or_default(),
        )
    }

    /// Turns it, which is what Word's Text Direction button does.
    ///
    /// The one cell the caret is in, rather than the whole row: which way up a
    /// heading is set is a decision about that heading, and a row of headings
    /// turned together is several presses in Word too.
    pub fn set_cell_direction(&mut self, direction: crate::model::TextDirection) -> bool {
        let Some(position) = self.table_here() else { return false };
        self.change_table(move |table, prefix| {
            let Some(row) = rows_mut(table).nth(position.row) else { return };
            let Some(cell) = cells_mut(row).nth(position.column) else { return };

            let properties = cell_properties(cell, prefix);
            properties.remove_children_named(Some(read::W), "textDirection");
            // The ordinary way up is what a cell that says nothing means, so it
            // is said by saying nothing.
            if !direction.is_turned() {
                return;
            }
            let mut element =
                Element::new(&edit::name_with(prefix, "textDirection"), Some(read::W));
            element.set_namespaced_attribute(
                &edit::name_with(prefix, "val"),
                read::W,
                direction.word(),
            );
            edit::insert_ordered(properties, element, CELL_PROPERTY_ORDER);
        })
    }

    /// Which of Word's three AutoFits the table at the caret is set to.
    #[must_use]
    pub fn table_fit(&self) -> crate::model::TableFit {
        self.table_here()
            .and_then(|_| self.table_element_here())
            .map(read::read_table_fit)
            .unwrap_or_default()
    }

    /// Sets it, which is what Word's AutoFit menu does.
    ///
    /// Fitting to contents clears the width every cell states for itself,
    /// because those are preferences and a preference is exactly what stops a
    /// table hugging its text. That is Word's own mechanism, and it is why the
    /// command has anything to do: without it the table would keep the widths
    /// it was written with and the menu would be three rows that change a file
    /// and change nothing anybody can see.
    pub fn set_table_fit(&mut self, fit: crate::model::TableFit) -> bool {
        use crate::model::TableFit;

        self.change_table(move |table, prefix| {
            let name = |local: &str| edit::name_with(prefix, local);
            let properties = table_properties(table, prefix);

            // How wide the table would like to be.
            properties.remove_children_named(Some(read::W), "tblW");
            let mut width = Element::new(&name("tblW"), Some(read::W));
            match fit {
                TableFit::Window(percent) => {
                    let fiftieths = percent.clamp(1, 100) * 50;
                    width.set_namespaced_attribute(&name("w"), read::W, &fiftieths.to_string());
                    width.set_namespaced_attribute(&name("type"), read::W, "pct");
                }
                // "Auto" is how the format says "work it out", and it is a width
                // of type auto rather than no width at all.
                _ => {
                    width.set_namespaced_attribute(&name("w"), read::W, "0");
                    width.set_namespaced_attribute(&name("type"), read::W, "auto");
                }
            }
            edit::insert_ordered(properties, width, TABLE_PROPERTY_ORDER);

            // And whether its columns may be worked out at all.
            properties.remove_children_named(Some(read::W), "tblLayout");
            if fit == TableFit::Fixed {
                let mut layout = Element::new(&name("tblLayout"), Some(read::W));
                layout.set_namespaced_attribute(&name("type"), read::W, "fixed");
                edit::insert_ordered(properties, layout, TABLE_PROPERTY_ORDER);
            }

            if fit != TableFit::Contents {
                return;
            }
            // Nothing preferred, so the text decides.
            for row in rows_mut(table) {
                for cell in cells_mut(row) {
                    let properties = cell_properties(cell, prefix);
                    properties.remove_children_named(Some(read::W), "tcW");
                    let mut width = Element::new(&name("tcW"), Some(read::W));
                    width.set_namespaced_attribute(&name("w"), read::W, "0");
                    width.set_namespaced_attribute(&name("type"), read::W, "auto");
                    edit::insert_ordered(properties, width, CELL_PROPERTY_ORDER);
                }
            }
        })
    }

    /// Writes the widths of the columns of the table at the caret, in twentieths
    /// of a point.
    ///
    /// The grid and every cell's stated width together, because they are two
    /// records of one thing and a document where they disagree is a document
    /// two programs lay out differently. What this is for is freezing the
    /// columns where they are: Word's Fixed Column Width keeps the widths in
    /// front of you, and the widths in front of you are the laid-out ones
    /// rather than whatever the file was last written with.
    pub fn set_table_grid(&mut self, widths: &[i32]) -> bool {
        if widths.is_empty() || widths.iter().any(|width| *width <= 0) {
            return false;
        }
        let widths = widths.to_vec();
        self.change_table(move |table, prefix| {
            let name = |local: &str| edit::name_with(prefix, local);

            table.remove_children_named(Some(read::W), "tblGrid");
            let mut grid = Element::new(&name("tblGrid"), Some(read::W));
            for width in &widths {
                let mut column = Element::new(&name("gridCol"), Some(read::W));
                column.set_namespaced_attribute(&name("w"), read::W, &width.to_string());
                grid.push_element(column);
            }
            // The grid goes after the properties and before the first row.
            let at = usize::from(table.child(Some(read::W), "tblPr").is_some());
            table.insert_element(at, grid);

            // A cell covering several columns is as wide as all of them.
            for row in rows_mut(table) {
                let mut at = 0usize;
                for cell in cells_mut(row) {
                    let span = cell
                        .child(Some(read::W), "tcPr")
                        .and_then(|properties| properties.child(Some(read::W), "gridSpan"))
                        .and_then(|span| span.attribute(Some(read::W), "val"))
                        .and_then(|text| text.parse::<usize>().ok())
                        .unwrap_or(1)
                        .max(1);
                    let width: i32 = widths.iter().skip(at).take(span).sum();
                    at += span;
                    if width <= 0 {
                        continue;
                    }

                    let properties = cell_properties(cell, prefix);
                    properties.remove_children_named(Some(read::W), "tcW");
                    let mut stated = Element::new(&name("tcW"), Some(read::W));
                    stated.set_namespaced_attribute(&name("w"), read::W, &width.to_string());
                    stated.set_namespaced_attribute(&name("type"), read::W, "dxa");
                    edit::insert_ordered(properties, stated, CELL_PROPERTY_ORDER);
                }
            }
        })
    }

    /// How far the table at the caret is set in from the left margin, in
    /// twentieths of a point.
    #[must_use]
    pub fn table_indent(&self) -> i32 {
        self.table_element_here()
            .and_then(|table| table.child(Some(read::W), "tblPr"))
            .and_then(|properties| properties.child(Some(read::W), "tblInd"))
            .and_then(|indent| {
                indent
                    .attribute(Some(read::W), "w")
                    .or_else(|| indent.attribute(Some(read::W), "val"))
            })
            .and_then(|text| text.parse().ok())
            .unwrap_or(0)
    }

    /// Sets that indent.
    pub fn set_table_indent(&mut self, twips: i32) -> bool {
        let twips = twips.clamp(0, 22 * 1440);
        self.change_table(move |table, prefix| {
            let properties = table_properties(table, prefix);
            properties.remove_children_named(Some(read::W), "tblInd");

            let mut element = Element::new(&edit::name_with(prefix, "tblInd"), Some(read::W));
            element.set_namespaced_attribute(
                &edit::name_with(prefix, "w"),
                read::W,
                &twips.to_string(),
            );
            element.set_namespaced_attribute(&edit::name_with(prefix, "type"), read::W, "dxa");
            edit::insert_ordered(properties, element, TABLE_PROPERTY_ORDER);
        })
    }

    /// Whether the row at the caret may be broken across a page boundary.
    ///
    /// The format says the opposite — `w:cantSplit` — and Word's tick box says
    /// "Allow row to break across pages". This follows the tick box, because
    /// that is what anybody reading the code alongside the dialog will expect.
    #[must_use]
    pub fn row_can_break(&self) -> bool {
        let Some(position) = self.table_here() else { return true };
        let Some(table) = self.table_element_here() else { return true };
        let Some(row) = table.children_named(Some(read::W), "tr").nth(position.row) else {
            return true;
        };
        row.child(Some(read::W), "trPr")
            .and_then(|properties| properties.child(Some(read::W), "cantSplit"))
            .is_none()
    }

    /// Lets the row at the caret break across pages, or stops it.
    pub fn set_row_can_break(&mut self, can_break: bool) -> bool {
        let Some(position) = self.table_here() else { return false };
        self.change_table(move |table, prefix| {
            let Some(row) = rows_mut(table).nth(position.row) else { return };
            let properties = row_properties(row, prefix);
            properties.remove_children_named(Some(read::W), "cantSplit");
            if !can_break {
                let element = Element::new(&edit::name_with(prefix, "cantSplit"), Some(read::W));
                edit::insert_ordered(properties, element, ROW_PROPERTY_ORDER);
            }
        })
    }

    /// How wide the cell at the caret asks to be, in twentieths of a point, or
    /// `None` where it says nothing.
    #[must_use]
    pub fn cell_width(&self) -> Option<i32> {
        let position = self.table_here()?;
        let table = self.table_element_here()?;
        let row = table.children_named(Some(read::W), "tr").nth(position.row)?;
        let cell = row.children_named(Some(read::W), "tc").nth(position.column)?;
        let width = cell.child(Some(read::W), "tcPr")?.child(Some(read::W), "tcW")?;
        // A width of type auto is a width to be worked out, which is what
        // saying nothing means.
        if width.attribute(Some(read::W), "type") == Some("auto") {
            return None;
        }
        width.attribute(Some(read::W), "w")?.parse().ok()
    }

    /// Sets that width, or takes it away.
    pub fn set_cell_width(&mut self, twips: Option<i32>) -> bool {
        let Some(position) = self.table_here() else { return false };
        self.change_table(move |table, prefix| {
            let Some(row) = rows_mut(table).nth(position.row) else { return };
            let Some(cell) = cells_mut(row).nth(position.column) else { return };
            let properties = cell_properties(cell, prefix);
            properties.remove_children_named(Some(read::W), "tcW");

            let mut element = Element::new(&edit::name_with(prefix, "tcW"), Some(read::W));
            match twips {
                Some(twips) => {
                    element.set_namespaced_attribute(
                        &edit::name_with(prefix, "w"),
                        read::W,
                        &twips.clamp(0, 22 * 1440).to_string(),
                    );
                    element.set_namespaced_attribute(
                        &edit::name_with(prefix, "type"),
                        read::W,
                        "dxa",
                    );
                }
                None => {
                    element.set_namespaced_attribute(&edit::name_with(prefix, "w"), read::W, "0");
                    element.set_namespaced_attribute(
                        &edit::name_with(prefix, "type"),
                        read::W,
                        "auto",
                    );
                }
            }
            edit::insert_ordered(properties, element, CELL_PROPERTY_ORDER);
        })
    }

    /// What the table at the caret is called and what it says, for a reader
    /// that cannot see it.
    ///
    /// Word's Alt Text tab. A table of figures is unreadable to somebody
    /// listening to the document rather than looking at it, and this is the
    /// only thing that helps.
    #[must_use]
    pub fn table_alt_text(&self) -> (String, String) {
        let read_one = |local: &str| {
            self.table_element_here()
                .and_then(|table| table.child(Some(read::W), "tblPr"))
                .and_then(|properties| properties.child(Some(read::W), local))
                .and_then(|element| element.attribute(Some(read::W), "val"))
                .unwrap_or_default()
                .to_owned()
        };
        (read_one("tblCaption"), read_one("tblDescription"))
    }

    /// Sets both.
    pub fn set_table_alt_text(&mut self, title: &str, description: &str) -> bool {
        let title = title.trim().to_owned();
        let description = description.trim().to_owned();
        self.change_table(move |table, prefix| {
            let properties = table_properties(table, prefix);
            for (local, value) in [("tblCaption", &title), ("tblDescription", &description)] {
                properties.remove_children_named(Some(read::W), local);
                if value.is_empty() {
                    continue;
                }
                let mut element = Element::new(&edit::name_with(prefix, local), Some(read::W));
                element.set_namespaced_attribute(&edit::name_with(prefix, "val"), read::W, value);
                // Neither is in the order list: they came later than it, and
                // the schema puts them at the end of the properties.
                properties.push_element(element);
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_cell_alignment_survives_being_written_and_read_back() {
        for alignment in CellAlignment::ALL {
            assert_eq!(CellAlignment::from_word(alignment.word()), *alignment);
        }
    }

    #[test]
    fn a_word_nobody_here_knows_means_the_top() {
        assert_eq!(CellAlignment::from_word("something else"), CellAlignment::Top);
    }

    #[test]
    fn every_alignment_says_what_it_is_called() {
        for alignment in CellAlignment::ALL {
            assert!(!alignment.label().is_empty());
        }
    }

    /// A document with one three by three table, the caret in its first cell.
    fn with_table() -> Document {
        let mut body = crate::model::Body::default();
        body.blocks.push(crate::model::Block::Paragraph(crate::model::Paragraph::text("Before")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let mut document = Document::open(&bytes).expect("reopening");
        assert!(document.insert_table(3, 3), "the table went nowhere");
        document
    }

    /// The table at the caret, as the model reads it.
    fn table_of(document: &Document) -> crate::model::Table {
        let (_, table) = document
            .body()
            .blocks
            .into_iter()
            .enumerate()
            .find_map(|(at, block)| match block {
                crate::model::Block::Table(table) => Some((at, *table)),
                crate::model::Block::Paragraph(_) => None,
            })
            .expect("a table");
        table
    }

    #[test]
    fn every_autofit_survives_being_written_and_read_back() {
        for fit in [
            crate::model::TableFit::Contents,
            crate::model::TableFit::Window(100),
            crate::model::TableFit::Fixed,
        ] {
            let mut document = with_table();
            assert!(document.set_table_fit(fit), "nothing was written for {fit:?}");

            let bytes = document.save().expect("saving");
            let reopened = Document::open(&bytes).expect("reopening");
            assert_eq!(table_of(&reopened).fit, fit, "{fit:?} came back as something else");
        }
    }

    #[test]
    fn fitting_to_contents_clears_the_width_every_cell_states() {
        // A stated width is a preference, and a preference is what stops a
        // table hugging its text. Word clears them; so does this.
        let mut document = with_table();
        assert!(table_of(&document).rows[0].cells[0].width.is_some(), "a new cell states none");

        document.set_table_fit(crate::model::TableFit::Contents);
        let table = table_of(&document);
        assert!(
            table.rows.iter().flat_map(|row| &row.cells).all(|cell| cell.width.is_none()),
            "a cell still states a width of its own"
        );
    }

    #[test]
    fn fixing_the_columns_writes_the_widths_into_the_grid_and_the_cells() {
        let mut document = with_table();
        assert!(document.set_table_grid(&[1000, 2000, 3000]));

        let table = table_of(&document);
        assert_eq!(table.grid, vec![1000, 2000, 3000]);
        let stated: Vec<Option<i32>> = table.rows[0].cells.iter().map(|cell| cell.width).collect();
        assert_eq!(stated, vec![Some(1000), Some(2000), Some(3000)]);
    }

    #[test]
    fn a_grid_of_nothing_is_refused_rather_than_written() {
        let mut document = with_table();
        assert!(!document.set_table_grid(&[]));
        assert!(!document.set_table_grid(&[100, 0, 100]));
    }

    #[test]
    fn the_orders_name_what_this_module_writes() {
        assert!(TABLE_PROPERTY_ORDER.contains(&"jc"));
        assert!(ROW_PROPERTY_ORDER.contains(&"trHeight"));
        assert!(ROW_PROPERTY_ORDER.contains(&"tblHeader"));
        assert!(CELL_PROPERTY_ORDER.contains(&"vAlign"));
    }

    #[test]
    fn a_rows_height_comes_before_its_header_flag() {
        let at =
            |name: &str| ROW_PROPERTY_ORDER.iter().position(|entry| *entry == name).expect(name);
        assert!(at("trHeight") < at("tblHeader"), "the schema wants them this way round");
    }
}
