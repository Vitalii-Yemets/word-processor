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
use crate::model::Alignment;
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

    /// Sets where the text sits in every cell of the row at the caret.
    pub fn set_cell_alignment(&mut self, alignment: CellAlignment) -> bool {
        let Some(position) = self.table_here() else { return false };
        self.change_table(move |table, prefix| {
            let Some(row) = rows_mut(table).nth(position.row) else { return };
            for cell in cells_mut(row) {
                let properties = cell_properties(cell, prefix);
                properties.remove_children_named(Some(read::W), "vAlign");
                if alignment == CellAlignment::Top {
                    continue;
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
