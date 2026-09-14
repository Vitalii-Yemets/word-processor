//! Reading an OpenDocument text into the model.
//!
//! # How the format is put together
//!
//! A ZIP with the parts of the document as XML: `content.xml` holds the
//! text and the automatic styles it uses — one per distinct formatting,
//! named `P1`, `T3` — and `styles.xml` the named styles the text refers to,
//! the page layout, and the fonts. A paragraph names its style, a span
//! names its, and the properties are in the style; a style may have a
//! parent whose properties it inherits. Lists are elements round their
//! items, tables are elements round their rows, pictures are frames with
//! an image inside pointing at a file in `Pictures/`.
//!
//! So the reader folds the styles first — every style with its parent's
//! properties under its own, whichever part it came from — and then walks
//! the body once, building paragraphs with the properties their styles
//! resolved to.

use std::collections::HashMap;

use wp_docx::model::{
    Alignment, Block, Body, BreakKind, LineRule, LineSpacing, NumberingReference, Paragraph,
    ParagraphProperties, Run, RunContent, RunProperties, TabAlignment, TabLeader, TabStop, Table,
    TableCell, TableRow, Underline, VerticalAlignment,
};
use wp_xml::tree::{Element, Node, XmlTree};

pub const TEXT: &str = "urn:oasis:names:tc:opendocument:xmlns:text:1.0";
pub const OFFICE: &str = "urn:oasis:names:tc:opendocument:xmlns:office:1.0";
pub const STYLE: &str = "urn:oasis:names:tc:opendocument:xmlns:style:1.0";
pub const TABLE: &str = "urn:oasis:names:tc:opendocument:xmlns:table:1.0";
pub const DRAW: &str = "urn:oasis:names:tc:opendocument:xmlns:drawing:1.0";
pub const FO: &str = "urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0";
pub const SVG: &str = "urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0";
pub const XLINK: &str = "http://www.w3.org/1999/xlink";

/// The character that stands where a picture goes in the text, until the
/// picture is put in.
pub const PICTURE_MARK: char = '\u{FFFC}';

/// What was read.
#[derive(Debug, Default)]
pub struct Reading {
    pub body: Body,
    pub pictures: Vec<PictureFound>,
    pub links: Vec<LinkFound>,
    /// The page: width, height, and the margins top, right, bottom, left,
    /// in twips.
    pub page: Option<(i32, i32, [i32; 4])>,
    pub title: Option<String>,
}

#[derive(Debug)]
pub struct PictureFound {
    pub paragraph: usize,
    pub offset: usize,
    /// The name of the file in the package the picture is.
    pub name: String,
    pub width_emu: i64,
    pub height_emu: i64,
}

#[derive(Debug)]
pub struct LinkFound {
    pub paragraph: usize,
    pub start: usize,
    pub end: usize,
    pub address: String,
}

/// A style, resolved: what it says about a paragraph and about its text,
/// and which list style it brings.
#[derive(Clone, Debug, Default)]
pub struct Resolved {
    pub paragraph: ParagraphProperties,
    pub text: RunProperties,
    pub list_style: Option<String>,
    /// Whether the paragraph style is a heading, and at what level.
    pub outline: Option<u8>,
    pub display_name: Option<String>,
}

/// Everything the styles say, by family and name.
#[derive(Debug, Default)]
pub struct Styles {
    paragraph: HashMap<String, Resolved>,
    text: HashMap<String, RunProperties>,
    /// Column widths in twips, by column style name.
    columns: HashMap<String, i32>,
    /// Whether a list style's levels are bulleted, by list style name, from
    /// level one.
    lists: HashMap<String, Vec<bool>>,
    /// The fonts declared, by the name the styles use for them.
    fonts: HashMap<String, String>,
}

impl Styles {
    /// Reads the styles of a part, on top of what is already known: the
    /// named styles from `styles.xml` first, then the automatic ones from
    /// `content.xml`, which may build on them.
    pub fn read(&mut self, root: &Element) {
        if let Some(fonts) = root.child(Some(OFFICE), "font-face-decls") {
            for face in fonts.children_named(Some(STYLE), "font-face") {
                let (Some(name), Some(family)) =
                    (face.attribute(Some(STYLE), "name"), face.attribute(Some(SVG), "font-family"))
                else {
                    continue;
                };
                self.fonts.insert(name.to_owned(), family.trim_matches(['\'', '"']).to_owned());
            }
        }
        for section in ["styles", "automatic-styles"] {
            let Some(styles) = root.child(Some(OFFICE), section) else { continue };
            for default in styles.children_named(Some(STYLE), "default-style") {
                if default.attribute(Some(STYLE), "family") == Some("paragraph") {
                    let resolved = self.resolve(default, None, false);
                    self.paragraph.insert(String::new(), resolved);
                }
            }
            for style in styles.children_named(Some(STYLE), "style") {
                let Some(name) = style.attribute(Some(STYLE), "name") else { continue };
                let parent = style.attribute(Some(STYLE), "parent-style-name");
                match style.attribute(Some(STYLE), "family") {
                    Some("paragraph") => {
                        let resolved = self.resolve(style, parent, section == "automatic-styles");
                        self.paragraph.insert(name.to_owned(), resolved);
                    }
                    Some("text") => {
                        let mut text = parent
                            .and_then(|parent| self.text.get(parent).cloned())
                            .unwrap_or_default();
                        if let Some(properties) = style.child(Some(STYLE), "text-properties") {
                            apply_text(&mut text, properties, &self.fonts);
                        }
                        self.text.insert(name.to_owned(), text);
                    }
                    Some("table-column") => {
                        if let Some(width) = style
                            .child(Some(STYLE), "table-column-properties")
                            .and_then(|p| p.attribute(Some(STYLE), "column-width"))
                            .and_then(twips)
                        {
                            self.columns.insert(name.to_owned(), width);
                        }
                    }
                    _ => {}
                }
            }
            for list in styles.children_named(Some(TEXT), "list-style") {
                let Some(name) = list.attribute(Some(STYLE), "name") else { continue };
                let mut levels = vec![false; 10];
                for level in list.child_elements() {
                    let bullet = level.local_name() == "list-level-style-bullet";
                    let index = level
                        .attribute(Some(TEXT), "level")
                        .and_then(|value| value.parse::<usize>().ok())
                        .unwrap_or(1);
                    if let Some(held) = levels.get_mut(index.saturating_sub(1)) {
                        *held = bullet;
                    }
                }
                self.lists.insert(name.to_owned(), levels);
            }
        }
    }

    /// A paragraph style's properties, its parent's under them. An
    /// automatic style is a named style with a few changes, so it keeps the
    /// name of the style it builds on: a heading with a page break is still
    /// a heading.
    fn resolve(&self, style: &Element, parent: Option<&str>, automatic: bool) -> Resolved {
        let mut resolved = parent
            .and_then(|parent| self.paragraph.get(parent).cloned())
            .or_else(|| self.paragraph.get("").cloned())
            .unwrap_or_default();
        if !automatic {
            if let Some(name) = style.attribute(Some(STYLE), "display-name") {
                resolved.display_name = Some(name.to_owned());
            } else if let Some(name) = style.attribute(Some(STYLE), "name") {
                resolved.display_name = Some(name.replace("_20_", " "));
            }
            // A style called "Heading 3" is a heading of level three whether
            // or not it says so.
            if let Some(level) = resolved
                .display_name
                .as_deref()
                .and_then(|name| name.strip_prefix("Heading "))
                .and_then(|rest| rest.parse::<u8>().ok())
            {
                resolved.outline = Some(level);
            }
        }
        if let Some(level) = style
            .attribute(Some(STYLE), "default-outline-level")
            .and_then(|value| value.parse::<u8>().ok())
        {
            resolved.outline = Some(level);
        }
        if let Some(list) = style.attribute(Some(STYLE), "list-style-name") {
            resolved.list_style = Some(list.to_owned());
        }
        if let Some(properties) = style.child(Some(STYLE), "paragraph-properties") {
            apply_paragraph(&mut resolved.paragraph, properties);
        }
        if let Some(properties) = style.child(Some(STYLE), "text-properties") {
            apply_text(&mut resolved.text, properties, &self.fonts);
        }
        resolved
    }

    #[must_use]
    pub fn paragraph_style(&self, name: &str) -> Resolved {
        self.paragraph
            .get(name)
            .cloned()
            .or_else(|| self.paragraph.get("").cloned())
            .unwrap_or_default()
    }

    #[must_use]
    pub fn text_style(&self, name: &str) -> Option<&RunProperties> {
        self.text.get(name)
    }

    #[must_use]
    pub fn list_is_bulleted(&self, name: &str, level: u8) -> Option<bool> {
        self.lists.get(name)?.get(usize::from(level)).copied()
    }
}

/// What the paragraph properties element says.
fn apply_paragraph(properties: &mut ParagraphProperties, element: &Element) {
    let get = |namespace: &str, local: &str| element.attribute(Some(namespace), local);
    if let Some(align) = get(FO, "text-align") {
        properties.alignment = Some(match align {
            "center" => Alignment::Center,
            "end" | "right" => Alignment::End,
            "justify" => Alignment::Both,
            _ => Alignment::Start,
        });
    }
    if let Some(value) = get(FO, "margin-left").and_then(twips) {
        properties.indent_start = Some(value);
    }
    if let Some(value) = get(FO, "margin-right").and_then(twips) {
        properties.indent_end = Some(value);
    }
    if let Some(value) = get(FO, "text-indent").and_then(twips) {
        properties.indent_first_line = Some(value);
    }
    if let Some(value) = get(FO, "margin-top").and_then(twips) {
        properties.space_before = Some(value);
    }
    if let Some(value) = get(FO, "margin-bottom").and_then(twips) {
        properties.space_after = Some(value);
    }
    if let Some(value) = get(FO, "line-height") {
        properties.line_spacing = if let Some(percent) = value.strip_suffix('%') {
            percent.trim().parse::<f32>().ok().map(|percent| LineSpacing {
                value: (percent * 2.4).round() as i32,
                rule: LineRule::Auto,
            })
        } else if value == "normal" {
            None
        } else {
            twips(value).map(|value| LineSpacing { value, rule: LineRule::Exact })
        };
    }
    if let Some(value) = get(STYLE, "line-height-at-least").and_then(twips) {
        properties.line_spacing = Some(LineSpacing { value, rule: LineRule::AtLeast });
    }
    if let Some(value) = get(FO, "break-before") {
        properties.page_break_before = Some(value == "page");
    }
    if let Some(value) = get(FO, "keep-with-next") {
        properties.keep_next = Some(value == "always");
    }
    if let Some(value) = get(FO, "keep-together") {
        properties.keep_lines = Some(value == "always");
    }
    if let Some(value) = get(FO, "widows") {
        properties.widow_control = Some(value != "0");
    }
    if let Some(stops) = element.child(Some(STYLE), "tab-stops") {
        properties.tab_stops = stops
            .children_named(Some(STYLE), "tab-stop")
            .filter_map(|stop| {
                let position = stop.attribute(Some(STYLE), "position").and_then(twips)?;
                let alignment = match stop.attribute(Some(STYLE), "type") {
                    Some("center") => TabAlignment::Center,
                    Some("right") => TabAlignment::End,
                    Some("char") => TabAlignment::Decimal,
                    _ => TabAlignment::Start,
                };
                let leader = match stop.attribute(Some(STYLE), "leader-text") {
                    Some(".") => TabLeader::Dot,
                    Some("-") => TabLeader::Hyphen,
                    Some("_") => TabLeader::Underscore,
                    _ => TabLeader::None,
                };
                Some(TabStop { position, alignment, leader })
            })
            .collect();
    }
}

/// What the text properties element says.
fn apply_text(properties: &mut RunProperties, element: &Element, fonts: &HashMap<String, String>) {
    let get = |namespace: &str, local: &str| element.attribute(Some(namespace), local);
    if let Some(value) = get(FO, "font-weight") {
        properties.bold = Some(value == "bold" || value.parse::<u32>().is_ok_and(|w| w >= 600));
    }
    if let Some(value) = get(FO, "font-style") {
        properties.italic = Some(value == "italic" || value == "oblique");
    }
    if let Some(value) = get(STYLE, "text-underline-style") {
        properties.underline = Some(match value {
            "none" => Underline::None,
            "dotted" => Underline::Dotted,
            "dash" | "long-dash" | "dot-dash" | "dot-dot-dash" => Underline::Dashed,
            "wave" => Underline::Wave,
            _ => {
                if get(STYLE, "text-underline-type") == Some("double") {
                    Underline::Double
                } else if get(STYLE, "text-underline-width")
                    .is_some_and(|w| w == "bold" || w == "thick")
                {
                    Underline::Thick
                } else {
                    Underline::Single
                }
            }
        });
    }
    if let Some(value) = get(STYLE, "text-line-through-style") {
        properties.strike = Some(value != "none");
        if get(STYLE, "text-line-through-type") == Some("double") {
            properties.double_strike = Some(true);
            properties.strike = Some(false);
        }
    }
    if let Some(value) = get(FO, "font-size") {
        if let Some(twips) = twips(value) {
            properties.size_half_points = Some((twips / 10).max(2) as u32);
        }
    }
    if let Some(name) = get(STYLE, "font-name") {
        properties.font = Some(fonts.get(name).cloned().unwrap_or_else(|| name.to_owned()));
    }
    if let Some(family) = get(FO, "font-family") {
        properties.font = Some(family.trim_matches(['\'', '"']).to_owned());
    }
    if let Some(value) = get(FO, "color") {
        properties.color = colour(value);
    }
    if let Some(value) = get(FO, "background-color") {
        properties.highlight = colour(value).and_then(|hex| highlight_name(&hex));
    }
    if let Some(value) = get(STYLE, "text-position") {
        let first = value.split_whitespace().next().unwrap_or("");
        properties.vertical_align = Some(match first {
            "super" => VerticalAlignment::Superscript,
            "sub" => VerticalAlignment::Subscript,
            other => {
                let percent: f32 = other.trim_end_matches('%').parse().unwrap_or(0.0);
                if percent > 0.0 {
                    VerticalAlignment::Superscript
                } else if percent < 0.0 {
                    VerticalAlignment::Subscript
                } else {
                    VerticalAlignment::Baseline
                }
            }
        });
    }
    if let Some(value) = get(FO, "text-transform") {
        properties.caps = Some(value == "uppercase");
    }
    if let Some(value) = get(FO, "font-variant") {
        properties.small_caps = Some(value == "small-caps");
    }
    if let Some(value) = get(TEXT, "display") {
        properties.hidden = Some(value == "none");
    }
    if let (Some(language), Some(country)) = (get(FO, "language"), get(FO, "country")) {
        if language != "none" && language != "zxx" {
            properties.language = Some(format!("{language}-{country}"));
        }
    }
}

/// A length as the format writes one, in twips: `2.54cm`, `1in`, `12pt`.
#[must_use]
pub fn twips(value: &str) -> Option<i32> {
    let value = value.trim();
    let at = value.find(|c: char| c.is_ascii_alphabetic() || c == '%')?;
    let number: f32 = value[..at].trim().parse().ok()?;
    let twips = match &value[at..] {
        "pt" => number * 20.0,
        "in" => number * 1440.0,
        "cm" => number * 566.93,
        "mm" => number * 56.693,
        "px" => number * 15.0,
        "pc" => number * 240.0,
        _ => return None,
    };
    Some(twips.round() as i32)
}

fn colour(value: &str) -> Option<String> {
    let hex = value.trim().strip_prefix('#')?;
    (hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit())).then(|| hex.to_uppercase())
}

fn highlight_name(hex: &str) -> Option<String> {
    let name = match hex {
        "FFFF00" => "yellow",
        "00FF00" => "green",
        "00FFFF" => "cyan",
        "FF00FF" => "magenta",
        "0000FF" => "blue",
        "FF0000" => "red",
        "000080" => "darkBlue",
        "008080" => "darkCyan",
        "008000" => "darkGreen",
        "800080" => "darkMagenta",
        "800000" => "darkRed",
        "808000" => "darkYellow",
        "808080" => "darkGray",
        "C0C0C0" => "lightGray",
        "000000" => "black",
        "FFFFFF" => "white",
        _ => return None,
    };
    Some(name.to_owned())
}

/// The page of the first master page: size and margins.
fn page_of(styles_root: &Element) -> Option<(i32, i32, [i32; 4])> {
    let master = styles_root
        .child(Some(OFFICE), "master-styles")?
        .children_named(Some(STYLE), "master-page")
        .next()?;
    let layout_name = master.attribute(Some(STYLE), "page-layout-name")?;
    let layout = styles_root
        .child(Some(OFFICE), "automatic-styles")?
        .children_named(Some(STYLE), "page-layout")
        .find(|layout| layout.attribute(Some(STYLE), "name") == Some(layout_name))?;
    let properties = layout.child(Some(STYLE), "page-layout-properties")?;
    let get = |local: &str| properties.attribute(Some(FO), local).and_then(twips);
    Some((
        get("page-width")?,
        get("page-height")?,
        [
            get("margin-top").unwrap_or(1440),
            get("margin-right").unwrap_or(1440),
            get("margin-bottom").unwrap_or(1440),
            get("margin-left").unwrap_or(1440),
        ],
    ))
}

// --- The body ------------------------------------------------------------------

struct Reader<'a> {
    styles: &'a Styles,
    body: Body,
    pictures: Vec<PictureFound>,
    links: Vec<LinkFound>,
    paragraphs_done: usize,
}

/// Reads the parts of a document into a body, with the pictures and links
/// beside it.
pub fn read(
    content: &str,
    styles_xml: Option<&str>,
    meta: Option<&str>,
) -> Result<Reading, wp_xml::Error> {
    let content = XmlTree::parse(content)?;
    let styles_tree = styles_xml.map(XmlTree::parse).transpose()?;
    let mut styles = Styles::default();
    if let Some(tree) = &styles_tree {
        styles.read(&tree.root);
    }
    styles.read(&content.root);
    let page = styles_tree.as_ref().and_then(|tree| page_of(&tree.root));
    let title = meta.and_then(|meta| XmlTree::parse(meta).ok()).and_then(|tree| {
        let meta = tree.root.child(Some(OFFICE), "meta")?;
        let title = meta.child(Some("http://purl.org/dc/elements/1.1/"), "title")?;
        let title = title.text_content();
        (!title.trim().is_empty()).then(|| title.trim().to_owned())
    });

    let mut reader = Reader {
        styles: &styles,
        body: Body::default(),
        pictures: Vec::new(),
        links: Vec::new(),
        paragraphs_done: 0,
    };
    if let Some(text) =
        content.root.child(Some(OFFICE), "body").and_then(|body| body.child(Some(OFFICE), "text"))
    {
        let blocks = reader.blocks(text, &[]);
        reader.body.blocks = blocks;
    }
    if reader.body.blocks.is_empty() {
        reader.body.blocks.push(Block::Paragraph(Paragraph::default()));
    }
    Ok(Reading { body: reader.body, pictures: reader.pictures, links: reader.links, page, title })
}

/// A list the reader is inside: its style, and how deep.
#[derive(Clone, Debug)]
struct ListContext {
    style: Option<String>,
    level: u8,
}

impl Reader<'_> {
    /// The blocks inside an element: paragraphs, headings, lists, tables,
    /// and the sections that hold more of the same.
    fn blocks(&mut self, element: &Element, lists: &[ListContext]) -> Vec<Block> {
        let mut blocks = Vec::new();
        for child in element.child_elements() {
            match (child.namespace.as_deref(), child.local_name()) {
                (Some(TEXT), "p") | (Some(TEXT), "h") => {
                    blocks.push(Block::Paragraph(self.paragraph(child, lists)));
                }
                (Some(TEXT), "list") => {
                    let style = child
                        .attribute(Some(TEXT), "style-name")
                        .map(str::to_owned)
                        .or_else(|| lists.last().and_then(|list| list.style.clone()));
                    let level = lists.last().map_or(0, |list| list.level + 1).min(8);
                    let mut inner = lists.to_vec();
                    inner.push(ListContext { style, level });
                    for item in child.child_elements() {
                        if matches!(item.local_name(), "list-item" | "list-header") {
                            blocks.extend(self.blocks(item, &inner));
                        }
                    }
                }
                (Some(TABLE), "table") => blocks.push(self.table(child)),
                (Some(TEXT), "section")
                | (Some(TEXT), "index-body")
                | (Some(TEXT), "table-of-content") => {
                    blocks.extend(self.blocks(child, lists));
                }
                _ => {}
            }
        }
        blocks
    }

    fn paragraph(&mut self, element: &Element, lists: &[ListContext]) -> Paragraph {
        let style_name = element.attribute(Some(TEXT), "style-name").unwrap_or("");
        let resolved = self.styles.paragraph_style(style_name);
        let mut properties = resolved.paragraph.clone();

        // A heading, by its own level or its style's.
        let level = element
            .attribute(Some(TEXT), "outline-level")
            .and_then(|value| value.parse::<u8>().ok())
            .or(resolved.outline);
        properties.style = match (element.local_name(), level, resolved.display_name.as_deref()) {
            (_, _, Some("Title")) => Some("Title".to_owned()),
            ("h", Some(level), _) if (1..=9).contains(&level) => Some(format!("Heading{level}")),
            (_, Some(level), Some(name))
                if name.to_lowercase().starts_with("heading") && (1..=9).contains(&level) =>
            {
                Some(format!("Heading{level}"))
            }
            _ => None,
        };

        // In a list, by the element round it or by its style's list.
        let list = lists.last().cloned().or_else(|| {
            resolved
                .list_style
                .as_ref()
                .map(|style| ListContext { style: Some(style.clone()), level: 0 })
        });
        if let Some(list) = list {
            let bullet = list
                .style
                .as_deref()
                .and_then(|style| self.styles.list_is_bulleted(style, list.level))
                .unwrap_or(true);
            properties.numbering = Some(NumberingReference {
                id: if bullet { wp_docx::BULLET_LIST } else { wp_docx::NUMBERED_LIST },
                level: list.level,
            });
            // The list draws its own indent.
            properties.indent_start = None;
            properties.indent_first_line = None;
        }

        let mut runs = Vec::new();
        let mut text = String::new();
        let mut offset = 0usize;
        let mut chars = resolved.text.clone();
        self.inline(element, &mut runs, &mut text, &mut offset, &mut chars, &resolved.text);
        flush(&mut runs, &mut text, &chars);
        self.paragraphs_done += 1;
        Paragraph { properties, runs }
    }

    /// The inline content of an element: text, spans, links, tabs, breaks,
    /// pictures.
    #[allow(clippy::too_many_arguments, reason = "the state of one paragraph being built")]
    fn inline(
        &mut self,
        element: &Element,
        runs: &mut Vec<Run>,
        text: &mut String,
        offset: &mut usize,
        chars: &mut RunProperties,
        base: &RunProperties,
    ) {
        for node in &element.children {
            match node {
                Node::Text(piece) | Node::CData(piece) => {
                    // Whitespace folds as it does on a page: the format says
                    // so, and writes `text:s` for the spaces that count.
                    let folded: String =
                        fold(piece, text.ends_with(' ') || (text.is_empty() && runs.is_empty()));
                    text.push_str(&folded);
                    *offset += folded.len();
                }
                Node::Element(child) => {
                    let paragraph_index = self.paragraphs_done;
                    match (child.namespace.as_deref(), child.local_name()) {
                        (Some(TEXT), "span") => {
                            let mut inner = chars.clone();
                            if let Some(style) = child
                                .attribute(Some(TEXT), "style-name")
                                .and_then(|name| self.styles.text_style(name))
                            {
                                inner = layered(base, style);
                                inner = merge(chars, &inner);
                            }
                            if inner != *chars {
                                flush(runs, text, chars);
                            }
                            let held = chars.clone();
                            *chars = inner;
                            self.inline(child, runs, text, offset, chars, base);
                            flush(runs, text, chars);
                            *chars = held;
                        }
                        (Some(TEXT), "a") => {
                            let address =
                                child.attribute(Some(XLINK), "href").unwrap_or("").to_owned();
                            let start = *offset;
                            self.inline(child, runs, text, offset, chars, base);
                            if !address.is_empty() && *offset > start {
                                self.links.push(LinkFound {
                                    paragraph: paragraph_index,
                                    start,
                                    end: *offset,
                                    address,
                                });
                            }
                        }
                        (Some(TEXT), "s") => {
                            let count = child
                                .attribute(Some(TEXT), "c")
                                .and_then(|c| c.parse::<usize>().ok())
                                .unwrap_or(1);
                            for _ in 0..count {
                                text.push(' ');
                            }
                            *offset += count;
                        }
                        (Some(TEXT), "tab") => {
                            flush(runs, text, chars);
                            runs.push(run(chars, RunContent::Tab));
                            *offset += 1;
                        }
                        (Some(TEXT), "line-break") => {
                            flush(runs, text, chars);
                            runs.push(run(chars, RunContent::Break(BreakKind::Line)));
                            *offset += 1;
                        }
                        (Some(DRAW), "frame") => {
                            if let Some(image) = child.child(Some(DRAW), "image") {
                                if let Some(name) = image.attribute(Some(XLINK), "href") {
                                    flush(runs, text, chars);
                                    let width = child
                                        .attribute(Some(SVG), "width")
                                        .and_then(twips)
                                        .map_or(96 * 9525, |t| i64::from(t) * 635);
                                    let height = child
                                        .attribute(Some(SVG), "height")
                                        .and_then(twips)
                                        .map_or(96 * 9525, |t| i64::from(t) * 635);
                                    self.pictures.push(PictureFound {
                                        paragraph: paragraph_index,
                                        offset: *offset,
                                        name: name.to_owned(),
                                        width_emu: width,
                                        height_emu: height,
                                    });
                                    runs.push(run(
                                        chars,
                                        RunContent::Text(PICTURE_MARK.to_string()),
                                    ));
                                    *offset += PICTURE_MARK.len_utf8();
                                }
                            }
                        }
                        // Notes, bookmarks, fields: the text they show, or
                        // nothing.
                        (Some(TEXT), "note")
                        | (Some(TEXT), "bookmark")
                        | (Some(TEXT), "bookmark-start")
                        | (Some(TEXT), "bookmark-end")
                        | (Some(TEXT), "soft-page-break") => {}
                        _ => self.inline(child, runs, text, offset, chars, base),
                    }
                }
                _ => {}
            }
        }
    }

    fn table(&mut self, element: &Element) -> Block {
        let mut grid: Vec<i32> = Vec::new();
        for column in element.children_named(Some(TABLE), "table-column") {
            let repeat = column
                .attribute(Some(TABLE), "number-columns-repeated")
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(1);
            let width = column
                .attribute(Some(TABLE), "style-name")
                .and_then(|name| self.styles.columns.get(name).copied())
                .unwrap_or(2880);
            for _ in 0..repeat {
                grid.push(width);
            }
        }
        let mut rows = Vec::new();
        let mut row_elements: Vec<&Element> = Vec::new();
        for child in element.child_elements() {
            match (child.namespace.as_deref(), child.local_name()) {
                (Some(TABLE), "table-row") => row_elements.push(child),
                (Some(TABLE), "table-header-rows") | (Some(TABLE), "table-rows") => {
                    row_elements.extend(child.children_named(Some(TABLE), "table-row"));
                }
                _ => {}
            }
        }
        for row in row_elements {
            let mut cells = Vec::new();
            for cell in row.child_elements() {
                if !matches!(cell.local_name(), "table-cell" | "covered-table-cell") {
                    continue;
                }
                let span = cell
                    .attribute(Some(TABLE), "number-columns-spanned")
                    .and_then(|value| value.parse::<u32>().ok())
                    .unwrap_or(1);
                let mut blocks = self.blocks(cell, &[]);
                if blocks.is_empty() {
                    blocks.push(Block::Paragraph(Paragraph::default()));
                    self.paragraphs_done += 1;
                }
                let column = cells.iter().map(|cell: &TableCell| cell.span as usize).sum::<usize>();
                let width =
                    grid.get(column..column + span as usize).map(|widths| widths.iter().sum());
                cells.push(TableCell { blocks, width, span, ..TableCell::default() });
            }
            rows.push(TableRow { cells, ..TableRow::default() });
        }
        Block::Table(Box::new(Table { rows, grid, ..Table::default() }))
    }
}

/// Whitespace folded as a page folds it.
fn fold(text: &str, after_space: bool) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last_space = after_space;
    for character in text.chars() {
        if character.is_whitespace() && character != '\u{00A0}' {
            if !last_space {
                out.push(' ');
                last_space = true;
            }
        } else {
            out.push(character);
            last_space = false;
        }
    }
    out
}

fn run(chars: &RunProperties, content: RunContent) -> Run {
    Run {
        properties: chars.clone(),
        content: vec![content],
        field: None,
        revision: None,
        format_change: None,
    }
}

fn flush(runs: &mut Vec<Run>, text: &mut String, chars: &RunProperties) {
    if text.is_empty() {
        return;
    }
    runs.push(run(chars, RunContent::Text(core::mem::take(text))));
}

/// A text style laid over the base the paragraph gives.
fn layered(base: &RunProperties, style: &RunProperties) -> RunProperties {
    merge(base, style)
}

/// One set of run properties over another: what the top says wins, what it
/// leaves unsaid comes from below.
fn merge(under: &RunProperties, over: &RunProperties) -> RunProperties {
    RunProperties {
        style: over.style.clone().or_else(|| under.style.clone()),
        bold: over.bold.or(under.bold),
        italic: over.italic.or(under.italic),
        strike: over.strike.or(under.strike),
        double_strike: over.double_strike.or(under.double_strike),
        underline: over.underline.clone().or_else(|| under.underline.clone()),
        size_half_points: over.size_half_points.or(under.size_half_points),
        color: over.color.clone().or_else(|| under.color.clone()),
        highlight: over.highlight.clone().or_else(|| under.highlight.clone()),
        vertical_align: over.vertical_align.or(under.vertical_align),
        font: over.font.clone().or_else(|| under.font.clone()),
        caps: over.caps.or(under.caps),
        small_caps: over.small_caps.or(under.small_caps),
        hidden: over.hidden.or(under.hidden),
        language: over.language.clone().or_else(|| under.language.clone()),
        ..under.clone()
    }
}
