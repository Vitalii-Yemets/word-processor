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
    /// The colour drawn behind the text, by the name the format uses — Word
    /// offers a fixed palette of them rather than arbitrary colours.
    pub highlight: Option<String>,
    /// Whether the run rides above or below the line.
    pub vertical_align: Option<VerticalAlignment>,
    /// Font name for Latin text.
    pub font: Option<String>,
    /// Marks the run as right-to-left. Without it, Arabic and Hebrew text is
    /// stored correctly but laid out in the wrong direction.
    pub right_to_left: Option<bool>,
    /// Language tag, which decides which dictionary proofing uses.
    pub language: Option<String>,
    /// The theme slot the colour is named after, when it is named rather than
    /// written out. Resolved against the document's theme, not here.
    pub color_theme: Option<crate::theme::ThemeColor>,
    /// The effect the letters are drawn with: a shadow, an outline, a glow or
    /// a reflection. See [`crate::effects`].
    pub effect: Option<crate::effects::TextEffect>,
    /// And the theme slot the font is named after.
    pub font_theme: Option<crate::theme::FontSlot>,
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
        // A colour written out and a colour named after the theme are two ways
        // of saying one thing, so they are inherited as one thing. A run that
        // names the theme's accent overrides a style that wrote a colour out,
        // and the other way round — taking them separately would leave the
        // written one from the style standing over the name from the run.
        let (color, color_theme) = if other.color.is_some() || other.color_theme.is_some() {
            (other.color.clone(), other.color_theme.clone())
        } else {
            (self.color.clone(), self.color_theme.clone())
        };
        let (font, font_theme) = if other.font.is_some() || other.font_theme.is_some() {
            (other.font.clone(), other.font_theme)
        } else {
            (self.font.clone(), self.font_theme)
        };

        Self {
            style: other.style.clone().or_else(|| self.style.clone()),
            color_theme,
            font_theme,
            bold: other.bold.or(self.bold),
            italic: other.italic.or(self.italic),
            strike: other.strike.or(self.strike),
            underline: other.underline.clone().or_else(|| self.underline.clone()),
            size_half_points: other.size_half_points.or(self.size_half_points),
            color,
            highlight: other.highlight.clone().or_else(|| self.highlight.clone()),
            vertical_align: other.vertical_align.or(self.vertical_align),
            font,
            right_to_left: other.right_to_left.or(self.right_to_left),
            language: other.language.clone().or_else(|| self.language.clone()),
            effect: other.effect.clone().or_else(|| self.effect.clone()),
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
    pub highlight: Option<String>,
    pub vertical_align: VerticalAlignment,
    pub font: Option<String>,
    pub language: Option<String>,
    /// The effect the letters are drawn with, if any.
    pub effect: Option<crate::effects::TextEffect>,
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
            highlight: None,
            vertical_align: VerticalAlignment::Baseline,
            font: None,
            language: None,
            effect: None,
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
    /// The lines drawn round the paragraph.
    pub borders: ParagraphBorders,
    /// The colour behind it, as six hex digits.
    pub shading: Option<String>,
    /// Where the tabs in this paragraph stop.
    ///
    /// Empty means the paragraph says nothing and the default grid applies,
    /// which is what a paragraph nobody has set tabs on does.
    pub tab_stops: Vec<TabStop>,
}

/// One place a tab reaches, and what happens to the text there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TabStop {
    /// How far from the left edge of the text area, in twentieths of a point.
    pub position: i32,
    pub alignment: TabAlignment,
    /// What fills the space the tab jumped over.
    pub leader: TabLeader,
}

/// What the text at a stop does about it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TabAlignment {
    /// The text starts at the stop, which is what a tab usually means.
    #[default]
    Start,
    /// It is centred on it.
    Center,
    /// It ends at it, which is how a page number is put against the margin.
    End,
    /// Its decimal point sits on it, which is how a column of figures is
    /// lined up.
    Decimal,
    /// Nothing is moved: a line is drawn down the page at that place.
    Bar,
    /// The stop somewhere before it is taken away. Word writes this to cancel
    /// a stop that a style put there.
    Clear,
}

/// What is drawn across the space a tab jumped.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TabLeader {
    /// Nothing, which is the usual.
    #[default]
    None,
    /// The row of dots that runs to a page number in a table of contents.
    Dot,
    Hyphen,
    Underscore,
    /// A middle dot, which Word calls "middle dot".
    MiddleDot,
}

/// The stop nearest a place, if any is within `slack` of it.
///
/// What a hand aiming at a marker on the ruler means: the nearest one, so long
/// as it is near enough to have been aimed at.
#[must_use]
pub fn nearest_stop(stops: &[TabStop], position: i32, slack: i32) -> Option<usize> {
    stops
        .iter()
        .enumerate()
        .filter(|(_, stop)| (stop.position - position).abs() <= slack)
        .min_by_key(|(_, stop)| (stop.position - position).abs())
        .map(|(index, _)| index)
}

impl TabAlignment {
    #[must_use]
    pub fn from_word(word: &str) -> Self {
        match word {
            "center" => Self::Center,
            "end" | "right" => Self::End,
            "decimal" => Self::Decimal,
            "bar" => Self::Bar,
            "clear" => Self::Clear,
            _ => Self::Start,
        }
    }

    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Center => "center",
            Self::End => "end",
            Self::Decimal => "decimal",
            Self::Bar => "bar",
            Self::Clear => "clear",
        }
    }
}

impl TabLeader {
    #[must_use]
    pub fn from_word(word: &str) -> Self {
        match word {
            "dot" => Self::Dot,
            "hyphen" => Self::Hyphen,
            "underscore" => Self::Underscore,
            "middleDot" => Self::MiddleDot,
            _ => Self::None,
        }
    }

    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Dot => "dot",
            Self::Hyphen => "hyphen",
            Self::Underscore => "underscore",
            Self::MiddleDot => "middleDot",
        }
    }

    /// The character drawn over and over to fill the space.
    #[must_use]
    pub fn character(self) -> Option<char> {
        match self {
            Self::None => None,
            Self::Dot => Some('.'),
            Self::Hyphen => Some('-'),
            Self::Underscore => Some('_'),
            Self::MiddleDot => Some('·'),
        }
    }
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
            borders: self.borders.overlaid_with(&other.borders),
            shading: other.shading.clone().or_else(|| self.shading.clone()),
            // Stops replace rather than merge: a paragraph that sets any of its
            // own is saying where its tabs go, not adding to what a style said.
            tab_stops: if other.tab_stops.is_empty() {
                self.tab_stops.clone()
            } else {
                other.tab_stops.clone()
            },
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
    pub borders: ParagraphBorders,
    pub shading: Option<String>,
    /// Where the tabs stop, after the style chain has had its say.
    pub tab_stops: Vec<TabStop>,
}

/// The lines drawn round a paragraph.
///
/// Separate from a table's borders because the two are different things in the
/// format and in meaning: `w:pBdr` has a "between" edge, drawn only where one
/// bordered paragraph meets the next, which a table has no use for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParagraphBorders {
    pub top: Option<Border>,
    pub start: Option<Border>,
    pub bottom: Option<Border>,
    pub end: Option<Border>,
    /// Drawn where one bordered paragraph meets the next rather than at the
    /// edge of each, so a run of them reads as one block.
    pub between: Option<Border>,
}

impl ParagraphBorders {
    /// Whether any edge is drawn at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// A single line on every edge: what Word's "All Borders" draws.
    #[must_use]
    pub fn box_all() -> Self {
        let line = Border { style: "single".to_owned(), size: 4, color: Some("auto".to_owned()) };
        Self {
            top: Some(line.clone()),
            start: Some(line.clone()),
            bottom: Some(line.clone()),
            end: Some(line.clone()),
            between: Some(line),
        }
    }

    /// One edge only, which is how Word's menu is mostly used.
    #[must_use]
    pub fn only(edge: BorderEdge) -> Self {
        let line =
            Some(Border { style: "single".to_owned(), size: 4, color: Some("auto".to_owned()) });
        let mut borders = Self::default();
        match edge {
            BorderEdge::Top => borders.top = line,
            BorderEdge::Bottom => borders.bottom = line,
            BorderEdge::Start => borders.start = line,
            BorderEdge::End => borders.end = line,
        }
        borders
    }

    /// Lays another set over this one, edge by edge.
    #[must_use]
    pub fn overlaid_with(&self, other: &Self) -> Self {
        Self {
            top: other.top.clone().or_else(|| self.top.clone()),
            start: other.start.clone().or_else(|| self.start.clone()),
            bottom: other.bottom.clone().or_else(|| self.bottom.clone()),
            end: other.end.clone().or_else(|| self.end.clone()),
            between: other.between.clone().or_else(|| self.between.clone()),
        }
    }
}

/// Which edge of a paragraph a border command means.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BorderEdge {
    Top,
    Bottom,
    Start,
    End,
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
    /// A picture sitting in the line of text.
    Picture(Picture),
    /// A shape or a text box sitting in the line of text.
    Shape(crate::shapes::Shape),
    /// A chart drawn in the line of text.
    ///
    /// Only the reference: the chart lives in a part of its own, the same way
    /// a picture does. See [`crate::chart`].
    Chart(ChartReference),
    /// An equation.
    ///
    /// Not inside the run when it is written: an equation is a sibling of the
    /// runs, in its own namespace. See [`crate::math`].
    Math(crate::math::Math),
    /// The little number that points at a note.
    ///
    /// The note itself is in another part of the package; this is only the
    /// mark in the text that says where it belongs.
    NoteReference {
        id: i32,
        endnote: bool,
    },
}

/// Which chart a drawing points at, and how much room it was given.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChartReference {
    /// The relationship of the main document that reaches the chart part.
    pub relationship: String,
    /// In English metric units, as DrawingML measures a drawing.
    pub width_emu: i64,
    pub height_emu: i64,
}

impl ChartReference {
    /// The width in points, which is what the layout works in.
    #[must_use]
    pub fn width_points(&self) -> f64 {
        self.width_emu as f64 / crate::EMU_PER_INCH as f64 * 72.0
    }

    #[must_use]
    pub fn height_points(&self) -> f64 {
        self.height_emu as f64 / crate::EMU_PER_INCH as f64 * 72.0
    }
}
/// A picture embedded in the document.
///
/// The bytes are not here: a picture is a part of the package, reached through
/// a relationship, and copying it into the model would mean carrying every
/// picture of a document in memory whether or not anything looked at it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Picture {
    /// The relationship the picture is embedded through, such as `rId7`.
    pub relationship: String,
    /// How big it should be drawn, in English Metric Units — 914400 to the inch.
    ///
    /// The size is the document's decision, not the file's: the same picture
    /// can appear twice at two sizes.
    pub width_emu: i64,
    pub height_emu: i64,
    /// What the picture shows, for anyone who cannot see it.
    pub description: Option<String>,
}

/// English Metric Units per inch, the unit drawings are measured in.
pub const EMU_PER_INCH: f64 = 914_400.0;

impl Picture {
    /// The width in points, which is the unit the rest of the layout works in.
    #[must_use]
    pub fn width_points(&self) -> f64 {
        self.width_emu as f64 / EMU_PER_INCH * 72.0
    }

    #[must_use]
    pub fn height_points(&self) -> f64 {
        self.height_emu as f64 / EMU_PER_INCH * 72.0
    }
}

/// A stretch of text sharing one set of character formatting.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Run {
    pub properties: RunProperties,
    pub content: Vec<RunContent>,
    /// The tracked change this run is part of, when it is inside one.
    ///
    /// Inserted text is wrapped in `w:ins` and deleted text in `w:del`; the
    /// deleted kind still holds its text, because a deletion that has not been
    /// accepted has not happened yet.
    pub revision: Option<Revision>,
    /// The field instruction this run is the result of, when it is inside one.
    ///
    /// A field is a `w:fldSimple` wrapped round the runs that show its last
    /// computed value — a page number, a date, a cross-reference. The text in
    /// those runs is a cached answer, not the question, so anything that wants
    /// to work the answer out afresh has to know the question, and this is it.
    pub field: Option<String>,
}

impl Run {
    /// A plain run of text with no formatting of its own.
    #[must_use]
    pub fn text(text: &str) -> Self {
        Self {
            properties: RunProperties::default(),
            content: vec![RunContent::Text(text.to_owned())],
            field: None,
            revision: None,
        }
    }

    /// A run that shows what a field works out, with the cached answer in it.
    #[must_use]
    pub fn field(instruction: &str, cached: &str) -> Self {
        Self {
            properties: RunProperties::default(),
            content: vec![RunContent::Text(cached.to_owned())],
            field: Some(instruction.to_owned()),
            revision: None,
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
        // Text somebody deleted is not what the document says. It is still
        // carried, because a deletion nobody has accepted has not happened —
        // but reading the document means reading it without that text.
        if self.revision.as_ref().is_some_and(|change| change.kind == RevisionKind::Deleted) {
            return String::new();
        }

        let mut out = String::new();
        for piece in &self.content {
            match piece {
                RunContent::Text(text) => out.push_str(text),
                RunContent::Break(_) => out.push('\n'),
                RunContent::Tab => out.push('\t'),
                // Neither a picture nor a note mark is text: both read as
                // nothing, the way they do when a document is copied into a
                // plain-text editor.
                RunContent::Picture(_)
                | RunContent::Shape(_)
                | RunContent::Chart(_)
                | RunContent::NoteReference { .. } => {}
                // An equation reads as the line it was typed on, which is
                // what a person searching for it would look for.
                RunContent::Math(math) => out.push_str(&math.plain_text()),
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
    /// The same paragraph, made an item of a list.
    ///
    /// The identifier names a list defined in the numbering part; the level is
    /// how deep the item sits, counted from zero.
    #[must_use]
    pub fn in_list(mut self, id: i32, level: u8) -> Self {
        self.properties.numbering = Some(NumberingReference { id, level });
        self
    }

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
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableCell {
    pub blocks: Vec<Block>,
    /// Width in twentieths of a point, when the cell states one.
    pub width: Option<i32>,
    /// How many columns of the grid this cell covers. Always at least one.
    pub span: u32,
    /// Whether the cell continues the one above rather than starting its own.
    ///
    /// A vertically merged cell is written as several cells, one per row, with
    /// all but the first marked as continuations. They hold no content and no
    /// line is drawn between them.
    pub merged_upwards: bool,
    pub borders: TableBorders,
}

impl Default for TableCell {
    fn default() -> Self {
        Self {
            blocks: Vec::new(),
            width: None,
            // A cell with no `w:gridSpan` covers exactly one column, and zero
            // would be a cell of no width at all.
            span: 1,
            merged_upwards: false,
            borders: TableBorders::default(),
        }
    }
}

/// One row of a table.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TableRow {
    pub cells: Vec<TableCell>,
    /// Height in twentieths of a point, when the row asks for one.
    pub height: Option<i32>,
    /// Whether the row is a header: repeated at the top of every page the
    /// table runs onto, and read as the names of the columns.
    pub is_header: bool,
}

/// A line along one side of a table or a cell.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Border {
    /// The style name, such as `single`. `none` and `nil` draw nothing.
    pub style: String,
    /// Width in eighths of a point, which is the unit the format uses.
    pub size: u32,
    /// Colour as six hex digits, or `auto`.
    pub color: Option<String>,
}

impl Border {
    /// Whether anything is actually drawn.
    #[must_use]
    pub fn is_visible(&self) -> bool {
        !matches!(self.style.as_str(), "" | "none" | "nil")
    }

    /// Thickness in points. Never zero for a visible border: a line the format
    /// says is there has to be seen.
    #[must_use]
    pub fn width_points(&self) -> f32 {
        (self.size as f32 / 8.0).max(0.5)
    }
}

/// The lines around and inside a table, or around one cell.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TableBorders {
    pub top: Option<Border>,
    pub start: Option<Border>,
    pub bottom: Option<Border>,
    pub end: Option<Border>,
    /// Between rows. Only a table states these; a cell has no inside.
    pub inside_horizontal: Option<Border>,
    /// Between columns.
    pub inside_vertical: Option<Border>,
}

impl TableBorders {
    /// A single line everywhere: what Word's "Table Grid" draws.
    #[must_use]
    pub fn grid() -> Self {
        let line = || Some(Border { style: "single".to_owned(), size: 4, color: None });
        Self {
            top: line(),
            start: line(),
            bottom: line(),
            end: line(),
            inside_horizontal: line(),
            inside_vertical: line(),
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Lays other borders over these, with the argument winning where it speaks.
    #[must_use]
    pub fn overlaid_with(&self, other: &Self) -> Self {
        let pick = |mine: &Option<Border>, theirs: &Option<Border>| theirs.clone().or(mine.clone());
        Self {
            top: pick(&self.top, &other.top),
            start: pick(&self.start, &other.start),
            bottom: pick(&self.bottom, &other.bottom),
            end: pick(&self.end, &other.end),
            inside_horizontal: pick(&self.inside_horizontal, &other.inside_horizontal),
            inside_vertical: pick(&self.inside_vertical, &other.inside_vertical),
        }
    }
}

/// A table.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Table {
    pub rows: Vec<TableRow>,
    /// Identifier of the table style, if one is applied.
    pub style: Option<String>,
    /// Column widths in twentieths of a point, from `w:tblGrid`.
    ///
    /// A table is a grid first and a set of cells second: a cell says how many
    /// columns it covers, not how wide it is, so the grid is what decides the
    /// geometry.
    pub grid: Vec<i32>,
    pub borders: TableBorders,
    /// Indent from the text margin, in twentieths of a point.
    pub indent: i32,
    /// Space kept clear inside a cell, in twentieths of a point.
    pub cell_margin_start: Option<i32>,
    pub cell_margin_end: Option<i32>,
}

impl TableCell {
    /// A cell holding blocks.
    #[must_use]
    pub fn from_blocks(blocks: Vec<Block>) -> Self {
        Self { blocks, ..Self::default() }
    }

    /// A cell holding one paragraph of plain text.
    #[must_use]
    pub fn text(text: &str) -> Self {
        Self::from_blocks(vec![Block::Paragraph(Paragraph::text(text))])
    }

    /// The same cell, covering several columns of the grid.
    #[must_use]
    pub fn spanning(mut self, columns: u32) -> Self {
        self.span = columns.max(1);
        self
    }
}

impl TableRow {
    #[must_use]
    pub fn from_cells(cells: Vec<TableCell>) -> Self {
        Self { cells, height: None, is_header: false }
    }

    /// A row of plain text cells.
    #[must_use]
    pub fn text(cells: &[&str]) -> Self {
        Self::from_cells(cells.iter().map(|text| TableCell::text(text)).collect())
    }
}

impl Table {
    #[must_use]
    pub fn from_rows(rows: Vec<TableRow>) -> Self {
        Self { rows, ..Self::default() }
    }

    #[must_use]
    pub fn with_style(mut self, style: &str) -> Self {
        self.style = Some(style.to_owned());
        self
    }

    /// Sets the column widths, in twentieths of a point.
    #[must_use]
    pub fn with_grid(mut self, grid: Vec<i32>) -> Self {
        self.grid = grid;
        self
    }

    #[must_use]
    pub fn with_borders(mut self, borders: TableBorders) -> Self {
        self.borders = borders;
        self
    }
}

/// Something that sits directly in the body or in a table cell.
///
/// The two variants are not the same size, and deliberately so. A body is
/// mostly paragraphs, so a paragraph is held directly: boxing it would put an
/// allocation and a pointer chase in the way of the common case to save a
/// handful of bytes. A table, which is rarer and much larger, is boxed.
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(clippy::large_enum_variant, reason = "a paragraph is the common case and is held inline")]
pub enum Block {
    Paragraph(Paragraph),
    Table(Box<Table>),
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

/// Whether a run rides above the line, below it, or on it.
///
/// A superscript is not a smaller font raised by hand: it is this property, and
/// a reader that treated it as one would lose it on the round trip.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VerticalAlignment {
    #[default]
    Baseline,
    Superscript,
    Subscript,
}

impl VerticalAlignment {
    /// Reads the `w:val` of a `w:vertAlign`.
    #[must_use]
    pub fn from_attribute(value: &str) -> Self {
        match value {
            "superscript" => Self::Superscript,
            "subscript" => Self::Subscript,
            _ => Self::Baseline,
        }
    }

    #[must_use]
    pub fn to_attribute(self) -> &'static str {
        match self {
            Self::Baseline => "baseline",
            Self::Superscript => "superscript",
            Self::Subscript => "subscript",
        }
    }
}

/// A change somebody made while changes were being tracked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Revision {
    pub kind: RevisionKind,
    pub author: String,
    /// The date the file records, which is an ISO 8601 timestamp.
    pub date: String,
    /// The number the file gives it, which is what accepting one names.
    pub id: i32,
}

/// Which way a tracked change went.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RevisionKind {
    /// Text somebody added, which is not yet part of the document proper.
    Inserted,
    /// Text somebody removed, which is still in the file until it is accepted.
    Deleted,
}

impl RevisionKind {
    /// The element that wraps runs of this kind.
    #[must_use]
    pub fn element(self) -> &'static str {
        match self {
            Self::Inserted => "ins",
            Self::Deleted => "del",
        }
    }
}
