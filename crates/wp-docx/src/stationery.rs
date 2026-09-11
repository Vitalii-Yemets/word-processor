//! Envelopes and sheets of labels.
//!
//! # Why each of these is a document of its own
//!
//! Word adds an envelope to the front of the letter, as a section with its own
//! page size. Sections are how one document holds pages of two different
//! shapes, and this program has one section per document — so an envelope goes
//! into a document of its own, which is the other thing Word's dialog offers
//! and the one that prints without asking which pages to send where.
//!
//! # Where the measurements come from
//!
//! The paper sizes themselves. A DL envelope is 220 by 110 millimetres because
//! that is what DL means; a Com-10 is nine and a half inches by four and an
//! eighth. They are converted here rather than written as twentieths of a
//! point, so that the number in the code is the number on the packet.

use crate::model::{Alignment, Block, Body, Paragraph, ParagraphProperties, Run, RunProperties};
use crate::{Document, Error};

/// Twentieths of a point in a millimetre.
const TWIPS_PER_MM: f64 = 56.692_913;
/// And in an inch.
const TWIPS_PER_INCH: f64 = 1440.0;

/// A size of envelope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Envelope {
    pub name: &'static str,
    /// Across and down, in twentieths of a point. An envelope is wider than it
    /// is tall, which is what makes it landscape.
    pub width: i32,
    pub height: i32,
}

/// The envelopes offered, which are the ones Word lists first.
pub const ENVELOPES: &[Envelope] = &[
    Envelope { name: "DL (220 × 110 mm)", width: mm(220.0), height: mm(110.0) },
    Envelope { name: "C5 (229 × 162 mm)", width: mm(229.0), height: mm(162.0) },
    Envelope { name: "C6 (162 × 114 mm)", width: mm(162.0), height: mm(114.0) },
    Envelope { name: "Com-10 (9½ × 4⅛ in)", width: inches(9.5), height: inches(4.125) },
    Envelope { name: "Monarch (7½ × 3⅞ in)", width: inches(7.5), height: inches(3.875) },
];

/// A sheet of labels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LabelSheet {
    pub name: &'static str,
    /// How many labels across the sheet and down it.
    pub columns: usize,
    pub rows: usize,
    /// One label's size, in twentieths of a point.
    pub width: i32,
    pub height: i32,
    /// How much of the sheet is left blank round the labels.
    pub margin_top: i32,
    pub margin_left: i32,
}

/// The sheets offered. A4 throughout, which is what these numbers are for.
pub const LABEL_SHEETS: &[LabelSheet] = &[
    LabelSheet {
        name: "Address, 63.5 × 38.1 mm (3 × 7)",
        columns: 3,
        rows: 7,
        width: mm(63.5),
        height: mm(38.1),
        margin_top: mm(15.1),
        margin_left: mm(7.2),
    },
    LabelSheet {
        name: "Address, 99.1 × 38.1 mm (2 × 7)",
        columns: 2,
        rows: 7,
        width: mm(99.1),
        height: mm(38.1),
        margin_top: mm(15.1),
        margin_left: mm(4.7),
    },
    LabelSheet {
        name: "Address, 105 × 42.3 mm (2 × 7)",
        columns: 2,
        rows: 7,
        width: mm(105.0),
        height: mm(42.3),
        margin_top: mm(0.0),
        margin_left: mm(0.0),
    },
    LabelSheet {
        name: "Shipping, 99.1 × 67.7 mm (2 × 4)",
        columns: 2,
        rows: 4,
        width: mm(99.1),
        height: mm(67.7),
        margin_top: mm(13.0),
        margin_left: mm(4.7),
    },
];

/// A measurement in millimetres, as the format counts.
#[must_use]
const fn mm(value: f64) -> i32 {
    (value * TWIPS_PER_MM) as i32
}

/// And in inches.
#[must_use]
const fn inches(value: f64) -> i32 {
    (value * TWIPS_PER_INCH) as i32
}

/// A4, which is what a sheet of labels is printed on.
const A4_WIDTH: i32 = mm(210.0);
const A4_HEIGHT: i32 = mm(297.0);

impl Document {
    /// A document of one envelope.
    ///
    /// The sender's address in the top left and the delivery address in the
    /// middle, which is where a sorting machine looks for it.
    pub fn create_envelope(
        envelope: &Envelope,
        delivery: &str,
        sender: &str,
    ) -> Result<Self, Error> {
        let mut body = Body::default();

        for line in sender.split('\n').filter(|line| !line.trim().is_empty()) {
            body.blocks.push(address_line(line, Alignment::Start, 0, 18));
        }

        // Room between the two, then the delivery address indented to the
        // middle of the envelope.
        for _ in 0..4 {
            body.blocks.push(Block::Paragraph(Paragraph::text("")));
        }
        let indent = envelope.width / 3;
        for line in delivery.split('\n').filter(|line| !line.trim().is_empty()) {
            body.blocks.push(address_line(line, Alignment::Start, indent, 24));
        }

        let mut document = Self::create(&body)?;
        document.set_page_size(envelope.width, envelope.height);
        // Half an inch all round: an envelope has little to spare.
        document.set_page_margins(720, 720, 720, 720);
        Ok(document)
    }

    /// A document of one sheet of labels, every one saying the same thing.
    pub fn create_labels(sheet: &LabelSheet, text: &str) -> Result<Self, Error> {
        use crate::model::{Table, TableCell, TableRow};

        let lines: Vec<&str> = text.split('\n').collect();
        let cell = || TableCell {
            blocks: lines
                .iter()
                .map(|line| Block::Paragraph(address_line_paragraph(line, Alignment::Start, 0, 20)))
                .collect(),
            ..TableCell::default()
        };

        let row = TableRow {
            cells: (0..sheet.columns).map(|_| cell()).collect(),
            height: Some(sheet.height),
            is_header: false,
        };

        let mut body = Body::default();
        body.blocks.push(Block::Table(Box::new(Table {
            rows: (0..sheet.rows).map(|_| row.clone()).collect(),
            grid: (0..sheet.columns).map(|_| sheet.width).collect(),
            ..Table::default()
        })));

        let mut document = Self::create(&body)?;
        document.set_page_size(A4_WIDTH, A4_HEIGHT);
        // The margins are what leaves the labels where the sheet has them.
        document.set_page_margins(
            sheet.margin_top,
            sheet.margin_left,
            sheet.margin_top,
            sheet.margin_left,
        );
        Ok(document)
    }
}

/// One line of an address.
fn address_line(text: &str, alignment: Alignment, indent: i32, half_points: u32) -> Block {
    Block::Paragraph(address_line_paragraph(text, alignment, indent, half_points))
}

/// The same as a paragraph, for a cell to hold.
fn address_line_paragraph(
    text: &str,
    alignment: Alignment,
    indent: i32,
    half_points: u32,
) -> Paragraph {
    Paragraph {
        properties: ParagraphProperties {
            alignment: Some(alignment),
            indent_start: Some(indent),
            // An address is a block, so the lines sit together.
            space_after: Some(0),
            ..ParagraphProperties::default()
        },
        runs: vec![Run {
            properties: RunProperties {
                size_half_points: Some(half_points),
                ..RunProperties::default()
            },
            content: vec![crate::model::RunContent::Text(text.to_owned())],
            field: None,
            revision: None,
            format_change: None,
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dl_envelope_is_the_size_a_dl_envelope_is() {
        let dl = ENVELOPES[0];
        // 220 by 110 millimetres, within a twentieth of a point.
        assert!((dl.width - 12_472).abs() < 20, "got {}", dl.width);
        assert!((dl.height - 6_236).abs() < 20, "got {}", dl.height);
    }

    #[test]
    fn a_com_ten_is_nine_and_a_half_inches_across() {
        let com10 =
            ENVELOPES.iter().find(|entry| entry.name.starts_with("Com-10")).expect("Com-10");
        assert_eq!(com10.width, 13_680);
        assert_eq!(com10.height, 5_940);
    }

    #[test]
    fn every_envelope_is_wider_than_it_is_tall() {
        for envelope in ENVELOPES {
            assert!(envelope.width > envelope.height, "{} is not landscape", envelope.name);
        }
    }

    #[test]
    fn every_sheet_of_labels_fits_on_the_paper() {
        for sheet in LABEL_SHEETS {
            let across = sheet.width * sheet.columns as i32 + sheet.margin_left * 2;
            let down = sheet.height * sheet.rows as i32 + sheet.margin_top * 2;
            assert!(across <= A4_WIDTH + 20, "{} is {across} across", sheet.name);
            assert!(down <= A4_HEIGHT + 20, "{} is {down} down", sheet.name);
        }
    }

    #[test]
    fn every_sheet_has_labels_on_it() {
        for sheet in LABEL_SHEETS {
            assert!(sheet.rows > 0 && sheet.columns > 0, "{}", sheet.name);
        }
    }

    #[test]
    fn an_envelope_holds_both_addresses() {
        let envelope =
            Document::create_envelope(&ENVELOPES[0], "Ann Roe\n12 High Street\nLeeds", "A Company")
                .expect("an envelope");
        let text = envelope.plain_text();
        assert!(text.contains("A Company"), "the sender is missing: {text:?}");
        assert!(text.contains("12 High Street"), "the delivery address is missing: {text:?}");
    }

    #[test]
    fn an_envelope_is_the_size_of_the_envelope_it_was_asked_for() {
        let envelope = Document::create_envelope(&ENVELOPES[0], "Ann", "").expect("an envelope");
        assert_eq!(envelope.page_size(), (ENVELOPES[0].width, ENVELOPES[0].height));
    }

    #[test]
    fn the_delivery_address_is_further_in_than_the_sender() {
        let envelope =
            Document::create_envelope(&ENVELOPES[0], "Ann Roe", "A Company").expect("an envelope");
        let body = envelope.body();
        let Block::Paragraph(sender) = &body.blocks[0] else { panic!("a paragraph") };
        let delivery = body
            .blocks
            .iter()
            .find_map(|block| match block {
                Block::Paragraph(paragraph) if paragraph.plain_text() == "Ann Roe" => {
                    Some(paragraph)
                }
                _ => None,
            })
            .expect("the delivery address");
        assert!(delivery.properties.indent_start > sender.properties.indent_start);
    }

    #[test]
    fn a_sheet_of_labels_has_a_label_in_every_place() {
        let sheet = LABEL_SHEETS[0];
        let labels = Document::create_labels(&sheet, "Ann Roe").expect("labels");
        let Block::Table(table) = &labels.body().blocks[0] else { panic!("a table") };
        assert_eq!(table.rows.len(), sheet.rows);
        assert_eq!(table.rows[0].cells.len(), sheet.columns);
    }

    #[test]
    fn every_label_says_the_same_thing() {
        let labels = Document::create_labels(&LABEL_SHEETS[0], "Ann Roe").expect("labels");
        let Block::Table(table) = &labels.body().blocks[0] else { panic!("a table") };
        for row in &table.rows {
            for cell in &row.cells {
                let words: String =
                    cell.blocks.iter().map(Block::plain_text).collect::<Vec<_>>().join(
                        "
",
                    );
                assert!(words.contains("Ann Roe"));
            }
        }
    }

    #[test]
    fn a_label_of_several_lines_keeps_them_all() {
        let labels =
            Document::create_labels(&LABEL_SHEETS[0], "Ann Roe\n12 High Street").expect("labels");
        let Block::Table(table) = &labels.body().blocks[0] else { panic!("a table") };
        assert_eq!(table.rows[0].cells[0].blocks.len(), 2);
    }

    #[test]
    fn a_sheet_of_labels_is_on_a_four() {
        let labels = Document::create_labels(&LABEL_SHEETS[0], "Ann").expect("labels");
        assert_eq!(labels.page_size(), (A4_WIDTH, A4_HEIGHT));
    }

    #[test]
    fn an_envelope_and_a_sheet_of_labels_both_save() {
        Document::create_envelope(&ENVELOPES[0], "Ann", "Company")
            .expect("an envelope")
            .save()
            .expect("saving the envelope");
        Document::create_labels(&LABEL_SHEETS[0], "Ann")
            .expect("labels")
            .save()
            .expect("saving the labels");
    }
}
