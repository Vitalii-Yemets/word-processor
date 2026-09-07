//! The shape of a document's body.
//!
//! This is a reading model, covering the constructs that carry text: paragraphs,
//! the runs inside them, and tables. It is deliberately not the whole of
//! WordprocessingML — that is the next stage of the project — and it is never
//! used as the source of truth when saving a document that was opened. A file
//! read into this model and written back from it would lose everything the model
//! does not represent, so opened documents are saved from their original bytes
//! and only documents built here are generated from the model.

/// Where a paragraph's lines sit between the margins.
///
/// "Start" and "End" rather than "Left" and "Right": in a right-to-left
/// paragraph the start edge is the right one, and naming them by side would make
/// every Arabic or Hebrew document read backwards.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Alignment {
    #[default]
    Start,
    Center,
    End,
    /// Justified: stretched to both margins.
    Both,
}

impl Alignment {
    /// Reads the value of `w:jc`.
    #[must_use]
    pub fn from_attribute(value: &str) -> Option<Self> {
        match value {
            // "left" and "right" are the older names, still written by Word.
            "start" | "left" => Some(Self::Start),
            "center" => Some(Self::Center),
            "end" | "right" => Some(Self::End),
            "both" | "distribute" => Some(Self::Both),
            _ => None,
        }
    }

    /// The value to write for `w:jc`.
    #[must_use]
    pub fn to_attribute(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Center => "center",
            Self::End => "end",
            Self::Both => "both",
        }
    }
}

/// Character formatting of a run.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunProperties {
    pub bold: bool,
    pub italic: bool,
    /// Any underline style at all; the specific style is not modelled yet.
    pub underline: bool,
    pub strike: bool,
    /// Size in half-points, which is how the format stores it — 24 means 12pt.
    pub size_half_points: Option<u32>,
    /// Colour as the six hex digits the format uses, or "auto".
    pub color: Option<String>,
    /// Font name for Latin text.
    pub font: Option<String>,
    /// Marks the run as right-to-left. Without it, Arabic and Hebrew text is
    /// stored correctly but laid out in the wrong direction.
    pub right_to_left: bool,
    /// Language tag, which decides which dictionary proofing uses.
    pub language: Option<String>,
    /// Style identifier applied to the run, if any.
    pub style: Option<String>,
}

impl RunProperties {
    /// Whether anything is set, so an empty `w:rPr` can be left out.
    #[must_use]
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// What a break interrupts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BreakKind {
    /// A line break inside the same paragraph.
    #[default]
    Line,
    Page,
    Column,
}

/// A piece of a run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunContent {
    Text(String),
    Break(BreakKind),
    Tab,
}

/// A stretch of text sharing one set of character formatting.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Run {
    pub properties: RunProperties,
    pub content: Vec<RunContent>,
}

impl Run {
    /// A plain run of text with no formatting.
    #[must_use]
    pub fn text(text: &str) -> Self {
        Self {
            properties: RunProperties::default(),
            content: vec![RunContent::Text(text.to_owned())],
        }
    }

    /// The run's text, with breaks and tabs rendered as characters.
    #[must_use]
    pub fn plain_text(&self) -> String {
        let mut out = String::new();
        for piece in &self.content {
            match piece {
                RunContent::Text(text) => out.push_str(text),
                RunContent::Break(_) => out.push('\n'),
                RunContent::Tab => out.push('\t'),
            }
        }
        out
    }
}

/// A paragraph: the unit a document is built from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Paragraph {
    /// Identifier of the paragraph style, such as `Heading1`.
    pub style: Option<String>,
    pub alignment: Option<Alignment>,
    /// Marks the paragraph as right-to-left, which sets the base direction its
    /// text is laid out in.
    pub right_to_left: bool,
    pub runs: Vec<Run>,
}

impl Paragraph {
    /// A paragraph holding one unformatted run.
    #[must_use]
    pub fn text(text: &str) -> Self {
        Self { runs: vec![Run::text(text)], ..Self::default() }
    }

    /// The paragraph's text with formatting removed.
    #[must_use]
    pub fn plain_text(&self) -> String {
        self.runs.iter().map(Run::plain_text).collect()
    }
}

/// One cell of a table.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TableCell {
    pub blocks: Vec<Block>,
}

/// One row of a table.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TableRow {
    pub cells: Vec<TableCell>,
}

/// A table.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Table {
    pub rows: Vec<TableRow>,
    /// Identifier of the table style, if one is applied.
    pub style: Option<String>,
}

/// Something that sits directly in the body or in a table cell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    Paragraph(Paragraph),
    Table(Table),
}

impl Block {
    /// The block's text with formatting removed.
    #[must_use]
    pub fn plain_text(&self) -> String {
        match self {
            Self::Paragraph(paragraph) => paragraph.plain_text(),
            Self::Table(table) => table
                .rows
                .iter()
                .map(|row| {
                    row.cells
                        .iter()
                        .map(|cell| {
                            cell.blocks.iter().map(Block::plain_text).collect::<Vec<_>>().join(" ")
                        })
                        .collect::<Vec<_>>()
                        .join("\t")
                })
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }
}

/// The body of a document.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Body {
    pub blocks: Vec<Block>,
}

impl Body {
    /// The whole document's text, one block per line.
    #[must_use]
    pub fn plain_text(&self) -> String {
        self.blocks.iter().map(Block::plain_text).collect::<Vec<_>>().join("\n")
    }

    /// Every paragraph in the body, including those inside tables.
    pub fn paragraphs(&self) -> Vec<&Paragraph> {
        fn collect<'a>(blocks: &'a [Block], out: &mut Vec<&'a Paragraph>) {
            for block in blocks {
                match block {
                    Block::Paragraph(paragraph) => out.push(paragraph),
                    Block::Table(table) => {
                        for row in &table.rows {
                            for cell in &row.cells {
                                collect(&cell.blocks, out);
                            }
                        }
                    }
                }
            }
        }

        let mut out = Vec::new();
        collect(&self.blocks, &mut out);
        out
    }
}
