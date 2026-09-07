//! The shape of a document's content.
//!
//! # Why almost everything is optional
//!
//! Formatting in a document is layered. A run inherits from its style, the style
//! from the style it is based on, and that chain ends at the document defaults.
//! At each layer a property may say "bold", "not bold", or nothing at all — and
//! those are three different things. `<w:b w:val="0"/>` is how a run switches off
//! bold that its style turned on; a run with no `w:b` at all inherits whatever
//! the style said.
//!
//! Collapsing "off" and "unset" into a plain `bool` would make those
//! indistinguishable, and any document that overrides its own style would come
//! out wrong. So authored properties are `Option`, and the resolved result —
//! after the whole chain has been walked — is not.

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

/// How a run is underlined.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Underline {
    /// Explicitly not underlined, which is different from saying nothing.
    #[default]
    None,
    Single,
    Double,
    Thick,
    Dotted,
    Dashed,
    Wave,
    /// A style this model does not name. Kept as written so nothing is lost.
    Other(String),
}

impl Underline {
    #[must_use]
    pub fn from_attribute(value: &str) -> Self {
        match value {
            "none" => Self::None,
            "single" => Self::Single,
            "double" => Self::Double,
            "thick" => Self::Thick,
            "dotted" => Self::Dotted,
            "dash" | "dashed" => Self::Dashed,
            "wave" => Self::Wave,
            other => Self::Other(other.to_owned()),
        }
    }

    #[must_use]
    pub fn to_attribute(&self) -> &str {
        match self {
            Self::None => "none",
            Self::Single => "single",
            Self::Double => "double",
            Self::Thick => "thick",
            Self::Dotted => "dotted",
            Self::Dashed => "dash",
            Self::Wave => "wave",
            Self::Other(value) => value,
        }
    }

    /// Whether anything is actually drawn.
    #[must_use]
    pub fn is_visible(&self) -> bool {
        !matches!(self, Self::None)
    }
}

/// How the line spacing value should be read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LineRule {
    /// A multiple of single spacing, in 240ths. 240 is single, 360 is one and a
    /// half.
    #[default]
    Auto,
    /// Exactly this height, even if the text does not fit.
    Exact,
    /// At least this height, growing for taller text.
    AtLeast,
}

/// Space between the lines of a paragraph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineSpacing {
    /// Twentieths of a point, or 240ths of single spacing when the rule is auto.
    pub value: i32,
    pub rule: LineRule,
}

/// Which list a paragraph belongs to, and how deep.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NumberingReference {
    /// Identifier of the numbering definition.
    pub id: i32,
    /// Nesting level, counted from zero.
    pub level: u8,
}

/// Character formatting as it was authored, before inheritance is applied.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunProperties {
    /// Identifier of the character style applied to the run.
    pub style: Option<String>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub strike: Option<bool>,
    pub underline: Option<Underline>,
    /// Size in half-points, which is how the format stores it — 24 means 12pt.
    pub size_half_points: Option<u32>,
    /// Colour as the six hex digits the format uses, or "auto".
    pub color: Option<String>,
    /// Font name for Latin text.
    pub font: Option<String>,
    /// Marks the run as right-to-left. Without it, Arabic and Hebrew text is
    /// stored correctly but laid out in the wrong direction.
    pub right_to_left: Option<bool>,
    /// Language tag, which decides which dictionary proofing uses.
    pub language: Option<String>,
}

impl RunProperties {
    /// Whether nothing at all is set, so an empty `w:rPr` can be left out.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Lays these properties over `self`, with the argument winning wherever it
    /// says something.
    #[must_use]
    pub fn overlaid_with(&self, other: &Self) -> Self {
        Self {
            style: other.style.clone().or_else(|| self.style.clone()),
            bold: other.bold.or(self.bold),
            italic: other.italic.or(self.italic),
            strike: other.strike.or(self.strike),
            underline: other.underline.clone().or_else(|| self.underline.clone()),
            size_half_points: other.size_half_points.or(self.size_half_points),
            color: other.color.clone().or_else(|| self.color.clone()),
            font: other.font.clone().or_else(|| self.font.clone()),
            right_to_left: other.right_to_left.or(self.right_to_left),
            language: other.language.clone().or_else(|| self.language.clone()),
        }
    }
}

/// Character formatting after the whole inheritance chain has been walked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedRunProperties {
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub right_to_left: bool,
    pub underline: Underline,
    pub size_half_points: u32,
    pub color: Option<String>,
    pub font: Option<String>,
    pub language: Option<String>,
}

impl Default for ResolvedRunProperties {
    fn default() -> Self {
        Self {
            bold: false,
            italic: false,
            strike: false,
            right_to_left: false,
            underline: Underline::None,
            // 10pt, the size a document falls back to when nothing sets one.
            size_half_points: 20,
            color: None,
            font: None,
            language: None,
        }
    }
}

impl ResolvedRunProperties {
    /// The size in points, which is what people think in.
    #[must_use]
    pub fn size_points(&self) -> f64 {
        f64::from(self.size_half_points) / 2.0
    }
}

/// Paragraph formatting as it was authored, before inheritance is applied.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParagraphProperties {
    /// Identifier of the paragraph style, such as `Heading1`.
    pub style: Option<String>,
    pub alignment: Option<Alignment>,
    /// Base direction of the paragraph. Set for Arabic and Hebrew text.
    pub right_to_left: Option<bool>,
    /// Indents in twentieths of a point. Negative values pull into the margin,
    /// which is why they are signed.
    pub indent_start: Option<i32>,
    pub indent_end: Option<i32>,
    /// Positive indents the first line, negative makes a hanging indent.
    pub indent_first_line: Option<i32>,
    pub space_before: Option<i32>,
    pub space_after: Option<i32>,
    pub line_spacing: Option<LineSpacing>,
    /// Keep this paragraph on the same page as the next one.
    pub keep_next: Option<bool>,
    /// Keep all of this paragraph's lines on one page.
    pub keep_lines: Option<bool>,
    pub page_break_before: Option<bool>,
    /// Prevent a single line being stranded at the top or bottom of a page.
    pub widow_control: Option<bool>,
    /// Heading depth, zero-based, which is what builds a table of contents.
    pub outline_level: Option<u8>,
    /// The list this paragraph belongs to.
    pub numbering: Option<NumberingReference>,
}

impl ParagraphProperties {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Lays these properties over `self`, with the argument winning wherever it
    /// says something.
    #[must_use]
    pub fn overlaid_with(&self, other: &Self) -> Self {
        Self {
            style: other.style.clone().or_else(|| self.style.clone()),
            alignment: other.alignment.or(self.alignment),
            right_to_left: other.right_to_left.or(self.right_to_left),
            indent_start: other.indent_start.or(self.indent_start),
            indent_end: other.indent_end.or(self.indent_end),
            indent_first_line: other.indent_first_line.or(self.indent_first_line),
            space_before: other.space_before.or(self.space_before),
            space_after: other.space_after.or(self.space_after),
            line_spacing: other.line_spacing.or(self.line_spacing),
            keep_next: other.keep_next.or(self.keep_next),
            keep_lines: other.keep_lines.or(self.keep_lines),
            page_break_before: other.page_break_before.or(self.page_break_before),
            widow_control: other.widow_control.or(self.widow_control),
            outline_level: other.outline_level.or(self.outline_level),
            numbering: other.numbering.or(self.numbering),
        }
    }
}

/// Paragraph formatting after the whole inheritance chain has been walked.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResolvedParagraphProperties {
    pub alignment: Alignment,
    pub right_to_left: bool,
    pub indent_start: i32,
    pub indent_end: i32,
    pub indent_first_line: i32,
    pub space_before: i32,
    pub space_after: i32,
    pub line_spacing: Option<LineSpacing>,
    pub keep_next: bool,
    pub keep_lines: bool,
    pub page_break_before: bool,
    pub widow_control: bool,
    pub outline_level: Option<u8>,
    pub numbering: Option<NumberingReference>,
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
    /// A plain run of text with no formatting of its own.
    #[must_use]
    pub fn text(text: &str) -> Self {
        Self {
            properties: RunProperties::default(),
            content: vec![RunContent::Text(text.to_owned())],
        }
    }

    /// The same run, made bold.
    #[must_use]
    pub fn bold(mut self) -> Self {
        self.properties.bold = Some(true);
        self
    }

    /// The same run, made italic.
    #[must_use]
    pub fn italic(mut self) -> Self {
        self.properties.italic = Some(true);
        self
    }

    /// The same run, underlined.
    #[must_use]
    pub fn underlined(mut self) -> Self {
        self.properties.underline = Some(Underline::Single);
        self
    }

    /// The same run struck through.
    #[must_use]
    pub fn struck_through(mut self) -> Self {
        self.properties.strike = Some(true);
        self
    }

    /// The same run at a given size, in points.
    #[must_use]
    pub fn sized(mut self, points: f64) -> Self {
        self.properties.size_half_points = Some((points * 2.0).round() as u32);
        self
    }

    /// The same run in a given colour, as six hex digits.
    #[must_use]
    pub fn colored(mut self, color: &str) -> Self {
        self.properties.color = Some(color.to_owned());
        self
    }

    /// The same run tagged with a language.
    #[must_use]
    pub fn in_language(mut self, language: &str) -> Self {
        self.properties.language = Some(language.to_owned());
        self
    }

    /// The same run marked as right-to-left.
    #[must_use]
    pub fn right_to_left(mut self) -> Self {
        self.properties.right_to_left = Some(true);
        self
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
    pub properties: ParagraphProperties,
    pub runs: Vec<Run>,
}

impl Paragraph {
    /// A paragraph holding one unformatted run.
    #[must_use]
    pub fn text(text: &str) -> Self {
        Self { properties: ParagraphProperties::default(), runs: vec![Run::text(text)] }
    }

    /// A paragraph built from several runs.
    #[must_use]
    pub fn from_runs(runs: Vec<Run>) -> Self {
        Self { properties: ParagraphProperties::default(), runs }
    }

    /// The same paragraph with a style applied.
    #[must_use]
    pub fn with_style(mut self, style: &str) -> Self {
        self.properties.style = Some(style.to_owned());
        self
    }

    /// The same paragraph with an alignment.
    #[must_use]
    pub fn with_alignment(mut self, alignment: Alignment) -> Self {
        self.properties.alignment = Some(alignment);
        self
    }

    /// The same paragraph laid out right-to-left.
    #[must_use]
    pub fn right_to_left(mut self) -> Self {
        self.properties.right_to_left = Some(true);
        self
    }

    /// The style applied to the paragraph, if any.
    #[must_use]
    pub fn style(&self) -> Option<&str> {
        self.properties.style.as_deref()
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
    #[must_use]
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
