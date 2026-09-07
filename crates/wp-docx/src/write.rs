//! Generating `document.xml` from a body.
//!
//! Used for documents this program creates. A document that was *opened* is
//! saved from its original bytes instead — see [`crate::Document`] — because
//! regenerating it from this model would discard everything the model does not
//! yet represent.

use wp_xml::{Error, Writer};

use crate::model::{Block, Body, BreakKind, Paragraph, Run, RunContent, RunProperties, Table};
use crate::read::W;

/// Page width of A4 in twentieths of a point, the unit the format uses.
const A4_WIDTH_TWIPS: &str = "11906";
/// Page height of A4 in the same unit.
const A4_HEIGHT_TWIPS: &str = "16838";
/// One inch of margin, in the same unit.
const MARGIN_TWIPS: &str = "1440";

/// Writes a complete `document.xml` for a body.
pub fn document_xml(body: &Body) -> Result<String, Error> {
    let mut writer = Writer::with_capacity(2048);
    writer.write_declaration(Some(true));
    writer.write_start("w:document", &[("xmlns:w", W)])?;
    writer.write_start("w:body", &[])?;

    write_blocks(&mut writer, &body.blocks)?;
    write_section_properties(&mut writer)?;

    writer.write_end("w:body")?;
    writer.write_end("w:document")?;
    writer.finish()
}

fn write_blocks(writer: &mut Writer, blocks: &[Block]) -> Result<(), Error> {
    for block in blocks {
        match block {
            Block::Paragraph(paragraph) => write_paragraph(writer, paragraph)?,
            Block::Table(table) => write_table(writer, table)?,
        }
    }
    Ok(())
}

fn write_paragraph(writer: &mut Writer, paragraph: &Paragraph) -> Result<(), Error> {
    writer.write_start("w:p", &[])?;

    let has_properties =
        paragraph.style.is_some() || paragraph.alignment.is_some() || paragraph.right_to_left;
    if has_properties {
        writer.write_start("w:pPr", &[])?;
        if let Some(style) = &paragraph.style {
            writer.write_empty("w:pStyle", &[("w:val", style)])?;
        }
        // Direction is written before alignment so that a reader applying them
        // in order resolves "start" against the right base direction.
        if paragraph.right_to_left {
            writer.write_empty("w:bidi", &[])?;
        }
        if let Some(alignment) = paragraph.alignment {
            writer.write_empty("w:jc", &[("w:val", alignment.to_attribute())])?;
        }
        writer.write_end("w:pPr")?;
    }

    for run in &paragraph.runs {
        write_run(writer, run)?;
    }

    writer.write_end("w:p")
}

fn write_run(writer: &mut Writer, run: &Run) -> Result<(), Error> {
    writer.write_start("w:r", &[])?;

    if !run.properties.is_default() {
        write_run_properties(writer, &run.properties)?;
    }

    for piece in &run.content {
        match piece {
            RunContent::Text(text) => {
                // xml:space="preserve" is not optional. Without it a leading or
                // trailing space is collapsed away, and two words run together.
                writer.write_start("w:t", &[("xml:space", "preserve")])?;
                writer.write_text(text);
                writer.write_end("w:t")?;
            }
            RunContent::Break(BreakKind::Line) => writer.write_empty("w:br", &[])?,
            RunContent::Break(BreakKind::Page) => {
                writer.write_empty("w:br", &[("w:type", "page")])?;
            }
            RunContent::Break(BreakKind::Column) => {
                writer.write_empty("w:br", &[("w:type", "column")])?;
            }
            RunContent::Tab => writer.write_empty("w:tab", &[])?,
        }
    }

    writer.write_end("w:r")
}

fn write_run_properties(writer: &mut Writer, properties: &RunProperties) -> Result<(), Error> {
    writer.write_start("w:rPr", &[])?;

    if let Some(style) = &properties.style {
        writer.write_empty("w:rStyle", &[("w:val", style)])?;
    }
    if let Some(font) = &properties.font {
        // All four scripts get the same family: without w:cs, right-to-left and
        // East Asian text would silently fall back to a different font.
        writer.write_empty(
            "w:rFonts",
            &[("w:ascii", font), ("w:hAnsi", font), ("w:cs", font), ("w:eastAsia", font)],
        )?;
    }
    if properties.bold {
        writer.write_empty("w:b", &[])?;
        // Complex-script text takes its weight from w:bCs, not w:b.
        writer.write_empty("w:bCs", &[])?;
    }
    if properties.italic {
        writer.write_empty("w:i", &[])?;
        writer.write_empty("w:iCs", &[])?;
    }
    if properties.strike {
        writer.write_empty("w:strike", &[])?;
    }
    if properties.underline {
        writer.write_empty("w:u", &[("w:val", "single")])?;
    }
    if let Some(color) = &properties.color {
        writer.write_empty("w:color", &[("w:val", color)])?;
    }
    if let Some(half_points) = properties.size_half_points {
        let size = half_points.to_string();
        writer.write_empty("w:sz", &[("w:val", &size)])?;
        writer.write_empty("w:szCs", &[("w:val", &size)])?;
    }
    if let Some(language) = &properties.language {
        writer.write_empty("w:lang", &[("w:val", language)])?;
    }
    if properties.right_to_left {
        writer.write_empty("w:rtl", &[])?;
    }

    writer.write_end("w:rPr")
}

fn write_table(writer: &mut Writer, table: &Table) -> Result<(), Error> {
    writer.write_start("w:tbl", &[])?;

    writer.write_start("w:tblPr", &[])?;
    if let Some(style) = &table.style {
        writer.write_empty("w:tblStyle", &[("w:val", style)])?;
    }
    writer.write_empty("w:tblW", &[("w:w", "0"), ("w:type", "auto")])?;
    writer.write_end("w:tblPr")?;

    for row in &table.rows {
        writer.write_start("w:tr", &[])?;
        for cell in &row.cells {
            writer.write_start("w:tc", &[])?;
            writer.write_start("w:tcPr", &[])?;
            writer.write_empty("w:tcW", &[("w:w", "0"), ("w:type", "auto")])?;
            writer.write_end("w:tcPr")?;

            if cell.blocks.is_empty() {
                // A cell must contain at least one paragraph; Word rejects a
                // document where one does not.
                writer.write_empty("w:p", &[])?;
            } else {
                write_blocks(writer, &cell.blocks)?;
            }

            writer.write_end("w:tc")?;
        }
        writer.write_end("w:tr")?;
    }

    writer.write_end("w:tbl")
}

/// Writes the section properties: page size and margins.
fn write_section_properties(writer: &mut Writer) -> Result<(), Error> {
    writer.write_start("w:sectPr", &[])?;
    writer.write_empty("w:pgSz", &[("w:w", A4_WIDTH_TWIPS), ("w:h", A4_HEIGHT_TWIPS)])?;
    writer.write_empty(
        "w:pgMar",
        &[
            ("w:top", MARGIN_TWIPS),
            ("w:right", MARGIN_TWIPS),
            ("w:bottom", MARGIN_TWIPS),
            ("w:left", MARGIN_TWIPS),
            ("w:header", "708"),
            ("w:footer", "708"),
            ("w:gutter", "0"),
        ],
    )?;
    writer.write_end("w:sectPr")
}
