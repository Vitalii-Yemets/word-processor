//! The styles of a package, folded.
//!
//! Every formatting in the format is a style: a paragraph names one, a span
//! names one, a table, a row, a cell, a frame, a section — each names a
//! style of its own family, and a style may name a parent whose properties
//! it inherits. The named ones are in `styles.xml`, with the page layouts
//! and the master pages that say what each page looks like; the automatic
//! ones, one for each distinct formatting, are in whichever part uses them.
//!
//! So everything is folded here, parent first, into what the model keeps:
//! a paragraph style into paragraph and run properties and a list and a
//! heading level, a text style into run properties, a cell style into lines
//! and a colour, a frame's style into its fill, its line and where it
//! floats, a page layout into a page.

use std::collections::HashMap;

use wp_docx::anchor::{Anchor, Placement, Relative, Wrap, WrapSide, USUAL_DEPTH};
use wp_docx::fonts::{FontClass, FontEntry};
use wp_docx::model::{
    Alignment, Border, LineRule, LineSpacing, ParagraphProperties, RunProperties, TabAlignment,
    TabLeader, TabStop, TableBorders, TextDirection, Underline, VerticalAlignment,
};
use wp_docx::sections::{NumberFormat, PageNumbering};
use wp_docx::table_properties::CellAlignment;
use wp_xml::tree::Element;

pub const TEXT: &str = "urn:oasis:names:tc:opendocument:xmlns:text:1.0";
pub const OFFICE: &str = "urn:oasis:names:tc:opendocument:xmlns:office:1.0";
pub const STYLE: &str = "urn:oasis:names:tc:opendocument:xmlns:style:1.0";
pub const TABLE: &str = "urn:oasis:names:tc:opendocument:xmlns:table:1.0";
pub const DRAW: &str = "urn:oasis:names:tc:opendocument:xmlns:drawing:1.0";
pub const FO: &str = "urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0";
pub const SVG: &str = "urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0";
pub const XLINK: &str = "http://www.w3.org/1999/xlink";
pub const DC: &str = "http://purl.org/dc/elements/1.1/";
pub const META: &str = "urn:oasis:names:tc:opendocument:xmlns:meta:1.0";
pub const CONFIG: &str = "urn:oasis:names:tc:opendocument:xmlns:config:1.0";

/// A paragraph style, resolved: what it says about a paragraph and about its
/// text, which list style it brings, and what it starts.
#[derive(Clone, Debug, Default)]
pub struct Resolved {
    pub paragraph: ParagraphProperties,
    pub text: RunProperties,
    pub list_style: Option<String>,
    /// Whether the paragraph style is a heading, and at what level.
    pub outline: Option<u8>,
    pub display_name: Option<String>,
    /// The page style a paragraph of this style begins a page with, and the
    /// number that page takes.
    pub master_page: Option<String>,
    pub page_number: Option<i32>,
}

/// A frame's or a shape's style: what is inside it, round it, and where it
/// floats.
#[derive(Clone, Debug, Default)]
pub struct Graphic {
    /// `None` for nothing inside; the colour otherwise.
    pub fill: Option<Option<String>>,
    /// The line round it, and how thick, in EMU; `None` for no line.
    pub stroke: Option<Option<(String, i64)>>,
    pub wrap: Option<String>,
    pub contour: bool,
    pub run_through: Option<String>,
    pub horizontal_pos: Option<String>,
    pub horizontal_rel: Option<String>,
    pub vertical_pos: Option<String>,
    pub vertical_rel: Option<String>,
    /// Room round it, in twips: left, right, top, bottom.
    pub margins: [Option<i32>; 4],
}

/// A cell's style: its lines, its colour, where its text sits and which way
/// it runs.
#[derive(Clone, Debug, Default)]
pub struct CellStyle {
    pub borders: TableBorders,
    pub shading: Option<String>,
    pub alignment: CellAlignment,
    pub direction: TextDirection,
}

/// A row's style: how tall, and whether that is the least or the most.
#[derive(Clone, Copy, Debug, Default)]
pub struct RowStyle {
    pub height: Option<i32>,
    pub exact: bool,
}

/// A table's style: how far it is in from the margin.
#[derive(Clone, Copy, Debug, Default)]
pub struct TableStyle {
    pub indent: Option<i32>,
}

/// A page layout: the paper, the margins as this format measures them, and
/// the header's and footer's heights, which a page with a header adds to its
/// margin.
#[derive(Clone, Debug, Default)]
pub struct PageLayout {
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub landscape: bool,
    /// Top, right, bottom, left, from the edge of the paper.
    pub margins: [Option<i32>; 4],
    pub header_height: Option<i32>,
    pub footer_height: Option<i32>,
    pub columns: Option<(usize, i32)>,
    pub number_format: Option<NumberFormat>,
}

/// A master page: its layout, the style of the page after it, and its
/// headers and footers, by which.
#[derive(Clone, Debug, Default)]
pub struct MasterPage {
    pub layout: Option<String>,
    pub next: Option<String>,
    /// The element each header and footer is, by its local name: `header`,
    /// `header-left`, `header-first`, and the same for the footer.
    pub furniture: Vec<(String, Element)>,
}

/// A named text style: what the model will define as a character style.
#[derive(Clone, Debug, Default)]
pub struct NamedText {
    pub name: String,
    pub display_name: String,
    pub parent: Option<String>,
    /// Its own properties, and with its parents'.
    pub own: RunProperties,
}

/// Everything the styles say, by family and name.
#[derive(Clone, Debug, Default)]
pub struct Styles {
    pub paragraph: HashMap<String, Resolved>,
    pub text: HashMap<String, RunProperties>,
    /// The text styles that are named, in the order they came, and which
    /// named style each automatic one builds on.
    pub named_text: Vec<NamedText>,
    pub text_parents: HashMap<String, String>,
    /// What each text style says itself, without its parents.
    pub text_own: HashMap<String, RunProperties>,
    /// Column widths in twips, by column style name.
    pub columns: HashMap<String, i32>,
    pub cells: HashMap<String, CellStyle>,
    pub rows: HashMap<String, RowStyle>,
    pub tables: HashMap<String, TableStyle>,
    pub graphics: HashMap<String, Graphic>,
    /// How many columns a section is set in, and the room between them.
    pub sections: HashMap<String, (usize, i32)>,
    /// Whether a list style's levels are bulleted, by list style name, from
    /// level one.
    pub lists: HashMap<String, Vec<bool>>,
    /// The fonts declared, by the name the styles use for them.
    pub fonts: HashMap<String, String>,
    /// And what each declaration says of its font, for the document's font
    /// table.
    pub font_table: Vec<FontEntry>,
    pub layouts: HashMap<String, PageLayout>,
    pub masters: Vec<(String, MasterPage)>,
}

impl Styles {
    /// Reads the styles of a part, on top of what is already known: the
    /// named styles from `styles.xml` first, then the automatic ones from
    /// the part that uses them, which may build on them.
    pub fn read(&mut self, root: &Element) {
        if let Some(fonts) = root.child(Some(OFFICE), "font-face-decls") {
            for face in fonts.children_named(Some(STYLE), "font-face") {
                let (Some(name), Some(family)) =
                    (face.attribute(Some(STYLE), "name"), face.attribute(Some(SVG), "font-family"))
                else {
                    continue;
                };
                let family = family.trim_matches(['\'', '"']).to_owned();
                self.fonts.insert(name.to_owned(), family.clone());
                // What the declaration says the font is like, for the
                // document's font table.
                let class = match face.attribute(Some(STYLE), "font-family-generic") {
                    Some("roman") => FontClass::Roman,
                    Some("swiss") => FontClass::Swiss,
                    Some("modern") => FontClass::Modern,
                    Some("script") => FontClass::Script,
                    Some("decorative") => FontClass::Decorative,
                    _ => FontClass::Auto,
                };
                let fixed_pitch =
                    face.attribute(Some(STYLE), "font-pitch").map(|pitch| pitch == "fixed");
                let charset =
                    (face.attribute(Some(STYLE), "font-charset") == Some("x-symbol")).then_some(2);
                if !self.font_table.iter().any(|entry| entry.name == family) {
                    self.font_table.push(FontEntry {
                        name: family,
                        alt_name: None,
                        panose: None,
                        charset,
                        class,
                        fixed_pitch,
                    });
                }
            }
        }
        for section in ["styles", "automatic-styles"] {
            let Some(styles) = root.child(Some(OFFICE), section) else { continue };
            let automatic = section == "automatic-styles";
            for default in styles.children_named(Some(STYLE), "default-style") {
                match default.attribute(Some(STYLE), "family") {
                    Some("paragraph") => {
                        let resolved = self.resolve(default, None, false);
                        self.paragraph.insert(String::new(), resolved);
                    }
                    Some("graphic") => {
                        let graphic = self.graphic(default, None);
                        self.graphics.insert(String::new(), graphic);
                    }
                    _ => {}
                }
            }
            for style in styles.children_named(Some(STYLE), "style") {
                self.style(style, automatic);
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
            for layout in styles.children_named(Some(STYLE), "page-layout") {
                let Some(name) = layout.attribute(Some(STYLE), "name") else { continue };
                self.layouts.insert(name.to_owned(), page_layout(layout));
            }
        }
        if let Some(masters) = root.child(Some(OFFICE), "master-styles") {
            for master in masters.children_named(Some(STYLE), "master-page") {
                let Some(name) = master.attribute(Some(STYLE), "name") else { continue };
                let furniture = master
                    .child_elements()
                    .filter(|child| child.namespace.as_deref() == Some(STYLE))
                    .filter(|child| {
                        matches!(
                            child.local_name(),
                            "header"
                                | "header-left"
                                | "header-first"
                                | "footer"
                                | "footer-left"
                                | "footer-first"
                        )
                    })
                    .filter(|child| child.attribute(Some(STYLE), "display") != Some("false"))
                    .map(|child| (child.local_name().to_owned(), child.clone()))
                    .collect();
                self.masters.push((
                    name.to_owned(),
                    MasterPage {
                        layout: master
                            .attribute(Some(STYLE), "page-layout-name")
                            .map(str::to_owned),
                        next: master.attribute(Some(STYLE), "next-style-name").map(str::to_owned),
                        furniture,
                    },
                ));
            }
        }
    }

    /// One style element, by its family.
    fn style(&mut self, style: &Element, automatic: bool) {
        let Some(name) = style.attribute(Some(STYLE), "name") else { return };
        let parent = style.attribute(Some(STYLE), "parent-style-name");
        match style.attribute(Some(STYLE), "family") {
            Some("paragraph") => {
                let resolved = self.resolve(style, parent, automatic);
                self.paragraph.insert(name.to_owned(), resolved);
            }
            Some("text") => {
                let mut text =
                    parent.and_then(|parent| self.text.get(parent).cloned()).unwrap_or_default();
                let mut own = RunProperties::default();
                if let Some(properties) = style.child(Some(STYLE), "text-properties") {
                    apply_text(&mut text, properties, &self.fonts);
                    apply_text(&mut own, properties, &self.fonts);
                }
                self.text_own.insert(name.to_owned(), own.clone());
                if automatic {
                    // The named style an automatic one builds on, however
                    // far back, is the style the run is in.
                    let named = parent.and_then(|parent| {
                        self.named_text
                            .iter()
                            .any(|named| named.name == parent)
                            .then(|| parent.to_owned())
                            .or_else(|| self.text_parents.get(parent).cloned())
                    });
                    if let Some(named) = named {
                        self.text_parents.insert(name.to_owned(), named);
                    }
                } else {
                    self.named_text.push(NamedText {
                        name: name.to_owned(),
                        display_name: style
                            .attribute(Some(STYLE), "display-name")
                            .map_or_else(|| name.replace("_20_", " "), str::to_owned),
                        parent: parent.map(str::to_owned),
                        own,
                    });
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
            Some("table-cell") => {
                let mut cell =
                    parent.and_then(|parent| self.cells.get(parent).cloned()).unwrap_or_default();
                if let Some(properties) = style.child(Some(STYLE), "table-cell-properties") {
                    apply_cell(&mut cell, properties);
                }
                self.cells.insert(name.to_owned(), cell);
            }
            Some("table-row") => {
                let mut row = RowStyle::default();
                if let Some(properties) = style.child(Some(STYLE), "table-row-properties") {
                    if let Some(height) =
                        properties.attribute(Some(STYLE), "row-height").and_then(twips)
                    {
                        row = RowStyle { height: Some(height), exact: true };
                    } else if let Some(height) =
                        properties.attribute(Some(STYLE), "min-row-height").and_then(twips)
                    {
                        row = RowStyle { height: Some(height), exact: false };
                    }
                }
                self.rows.insert(name.to_owned(), row);
            }
            Some("table") => {
                let indent = style
                    .child(Some(STYLE), "table-properties")
                    .and_then(|properties| properties.attribute(Some(FO), "margin-left"))
                    .and_then(twips);
                self.tables.insert(name.to_owned(), TableStyle { indent });
            }
            Some("graphic") => {
                let graphic = self.graphic(style, parent);
                self.graphics.insert(name.to_owned(), graphic);
            }
            Some("section") => {
                if let Some(columns) = style
                    .child(Some(STYLE), "section-properties")
                    .and_then(|properties| properties.child(Some(STYLE), "columns"))
                {
                    self.sections.insert(name.to_owned(), columns_of(columns));
                }
            }
            _ => {}
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
        // What begins a page belongs to the style that says so, not to the
        // styles built on it.
        if !automatic {
            resolved.master_page = None;
            resolved.page_number = None;
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
        if let Some(master) = style.attribute(Some(STYLE), "master-page-name") {
            resolved.master_page = (!master.is_empty()).then(|| master.to_owned());
        }
        if let Some(properties) = style.child(Some(STYLE), "paragraph-properties") {
            apply_paragraph(&mut resolved.paragraph, properties);
            if let Some(number) = properties
                .attribute(Some(STYLE), "page-number")
                .and_then(|value| value.parse::<i32>().ok())
            {
                resolved.page_number = Some(number);
            }
        }
        if let Some(properties) = style.child(Some(STYLE), "text-properties") {
            apply_text(&mut resolved.text, properties, &self.fonts);
        }
        resolved
    }

    /// A frame's style, its parent's under it.
    fn graphic(&self, style: &Element, parent: Option<&str>) -> Graphic {
        let mut graphic = parent
            .and_then(|parent| self.graphics.get(parent).cloned())
            .or_else(|| self.graphics.get("").cloned())
            .unwrap_or_default();
        let Some(properties) = style.child(Some(STYLE), "graphic-properties") else {
            return graphic;
        };
        let get = |namespace: &str, local: &str| properties.attribute(Some(namespace), local);
        match get(DRAW, "fill") {
            Some("none") => graphic.fill = Some(None),
            Some(_) => {
                graphic.fill = Some(get(DRAW, "fill-color").and_then(colour));
            }
            None => {
                if let Some(value) = get(FO, "background-color") {
                    graphic.fill = Some(colour(value));
                }
            }
        }
        let stroke_colour =
            get(SVG, "stroke-color").and_then(colour).unwrap_or_else(|| "000000".to_owned());
        let stroke_width =
            get(SVG, "stroke-width").and_then(twips).map_or(9525, |width| i64::from(width) * 635);
        match get(DRAW, "stroke") {
            Some("none") => graphic.stroke = Some(None),
            Some(_) => graphic.stroke = Some(Some((stroke_colour, stroke_width.max(3175)))),
            None => {
                if let Some(border) = get(FO, "border") {
                    graphic.stroke = Some(border_of(border).map(|line| {
                        let colour = line.color.unwrap_or_else(|| "000000".to_owned());
                        (colour, i64::from(line.size) * 1587)
                    }));
                }
            }
        }
        let text = |local: &str| get(STYLE, local).map(str::to_owned);
        graphic.wrap = text("wrap").or(graphic.wrap);
        graphic.contour = get(STYLE, "wrap-contour").map_or(graphic.contour, |on| on == "true");
        graphic.run_through = text("run-through").or(graphic.run_through);
        graphic.horizontal_pos = text("horizontal-pos").or(graphic.horizontal_pos);
        graphic.horizontal_rel = text("horizontal-rel").or(graphic.horizontal_rel);
        graphic.vertical_pos = text("vertical-pos").or(graphic.vertical_pos);
        graphic.vertical_rel = text("vertical-rel").or(graphic.vertical_rel);
        for (index, side) in
            ["margin-left", "margin-right", "margin-top", "margin-bottom"].iter().enumerate()
        {
            if let Some(value) = get(FO, side).and_then(twips) {
                graphic.margins[index] = Some(value);
            }
        }
        graphic
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

    #[must_use]
    pub fn master(&self, name: &str) -> Option<&MasterPage> {
        self.masters.iter().find(|(held, _)| held == name).map(|(_, master)| master)
    }

    /// The master page a document begins with: `Standard`, which is what
    /// every program writes, or else the first there is.
    #[must_use]
    pub fn first_master(&self) -> Option<&str> {
        self.masters
            .iter()
            .find(|(name, _)| name == "Standard")
            .or_else(|| self.masters.first())
            .map(|(name, _)| name.as_str())
    }
}

/// What the paragraph properties element says.
pub fn apply_paragraph(properties: &mut ParagraphProperties, element: &Element) {
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
    if let Some(value) = get(STYLE, "contextual-spacing") {
        properties.contextual_spacing = Some(value == "true");
    }
    if let Some(value) = get(STYLE, "writing-mode") {
        match value {
            "rl-tb" | "rl" => properties.right_to_left = Some(true),
            "lr-tb" | "lr" => properties.right_to_left = Some(false),
            _ => {}
        }
    }
    if let Some(value) = get(FO, "background-color") {
        properties.shading = colour(value);
    }
    let all = get(FO, "border").and_then(border_of);
    let side = |local: &str| get(FO, local).map_or_else(|| all.clone(), border_of);
    if get(FO, "border").is_some()
        || ["border-top", "border-bottom", "border-left", "border-right"]
            .iter()
            .any(|local| get(FO, local).is_some())
    {
        properties.borders.top = side("border-top");
        properties.borders.bottom = side("border-bottom");
        properties.borders.start = side("border-left");
        properties.borders.end = side("border-right");
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
pub fn apply_text(
    properties: &mut RunProperties,
    element: &Element,
    fonts: &HashMap<String, String>,
) {
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
    if let Some(value) = get(FO, "letter-spacing").and_then(twips) {
        properties.spacing_twentieths = Some(value);
    }
    if let (Some(language), Some(country)) = (get(FO, "language"), get(FO, "country")) {
        if language != "none" && language != "zxx" {
            properties.language = Some(format!("{language}-{country}"));
        }
    }
}

/// What a cell's properties say.
fn apply_cell(cell: &mut CellStyle, element: &Element) {
    let get = |namespace: &str, local: &str| element.attribute(Some(namespace), local);
    let all = get(FO, "border").map(|value| (value, get(STYLE, "border-line-width")));
    let side = |local: &str| {
        get(FO, &format!("border-{local}"))
            .map(|value| (value, get(STYLE, &format!("border-line-width-{local}"))))
            .or(all)
            .and_then(|(value, widths)| border_with_widths(value, widths))
    };
    if all.is_some()
        || ["top", "bottom", "left", "right"]
            .iter()
            .any(|local| get(FO, &format!("border-{local}")).is_some())
    {
        cell.borders = TableBorders {
            top: side("top"),
            bottom: side("bottom"),
            start: side("left"),
            end: side("right"),
            ..TableBorders::default()
        };
    }
    if let Some(value) = get(FO, "background-color") {
        cell.shading = colour(value);
    }
    if let Some(value) = get(STYLE, "vertical-align") {
        cell.alignment = match value {
            "middle" => CellAlignment::Middle,
            "bottom" => CellAlignment::Bottom,
            _ => CellAlignment::Top,
        };
    }
    if let Some(value) = get(STYLE, "writing-mode") {
        cell.direction = match value {
            "tb-rl" | "tb" => TextDirection::Down,
            "bt-lr" => TextDirection::Up,
            _ => TextDirection::Horizontal,
        };
    }
}

/// A page layout's paper, margins, header and footer, columns and numbers.
fn page_layout(layout: &Element) -> PageLayout {
    let mut page = PageLayout::default();
    if let Some(properties) = layout.child(Some(STYLE), "page-layout-properties") {
        let get = |local: &str| properties.attribute(Some(FO), local).and_then(twips);
        page.width = get("page-width");
        page.height = get("page-height");
        page.landscape =
            properties.attribute(Some(STYLE), "print-orientation") == Some("landscape");
        page.margins =
            [get("margin-top"), get("margin-right"), get("margin-bottom"), get("margin-left")];
        page.columns = properties.child(Some(STYLE), "columns").map(columns_of);
        page.number_format =
            properties.attribute(Some(STYLE), "num-format").map(|format| match format {
                "i" => NumberFormat::LowerRoman,
                "I" => NumberFormat::UpperRoman,
                "a" => NumberFormat::LowerLetter,
                "A" => NumberFormat::UpperLetter,
                _ => NumberFormat::Decimal,
            });
    }
    let height = |local: &str| {
        layout.child(Some(STYLE), local)?.child(Some(STYLE), "header-footer-properties").and_then(
            |properties| {
                properties
                    .attribute(Some(FO), "min-height")
                    .or_else(|| properties.attribute(Some(SVG), "height"))
                    .and_then(twips)
            },
        )
    };
    page.header_height = height("header-style");
    page.footer_height = height("footer-style");
    page
}

/// How many columns, and the room between them.
fn columns_of(columns: &Element) -> (usize, i32) {
    let count = columns
        .attribute(Some(FO), "column-count")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(1)
        .max(1);
    let gap = columns.attribute(Some(FO), "column-gap").and_then(twips).unwrap_or(720);
    (count, gap)
}

impl PageLayout {
    /// The page as Word measures it: the margins run to the text, so a page
    /// with a header has the header's height added to its top margin, and
    /// the header sits where this format's margin is.
    #[must_use]
    pub fn setup(&self, header: bool, footer: bool) -> PageSetup {
        let mut setup = PageSetup::default();
        if let (Some(width), Some(height)) = (self.width, self.height) {
            setup.width = width;
            setup.height = height;
        }
        setup.landscape = self.landscape;
        let [top, right, bottom, left] = self.margins;
        let top = top.unwrap_or(1440);
        let bottom = bottom.unwrap_or(1440);
        setup.margins = [
            top + if header { self.header_height.unwrap_or(0) } else { 0 },
            right.unwrap_or(1440),
            bottom + if footer { self.footer_height.unwrap_or(0) } else { 0 },
            left.unwrap_or(1440),
        ];
        setup.header_distance = top;
        setup.footer_distance = bottom;
        if let Some((count, gap)) = self.columns {
            setup.columns = count;
            setup.column_gap = gap;
        }
        setup.numbering = self
            .number_format
            .filter(|format| *format != NumberFormat::Decimal)
            .map(|format| PageNumbering { start: None, format });
        setup
    }
}

/// How a section's pages are set up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageSetup {
    pub width: i32,
    pub height: i32,
    pub landscape: bool,
    /// Top, right, bottom and left, to the text.
    pub margins: [i32; 4],
    pub header_distance: i32,
    pub footer_distance: i32,
    pub columns: usize,
    pub column_gap: i32,
    pub title_page: bool,
    pub numbering: Option<PageNumbering>,
}

impl Default for PageSetup {
    /// A page this format's defaults describe: A4, two centimetres round.
    fn default() -> Self {
        Self {
            width: 11_906,
            height: 16_838,
            landscape: false,
            margins: [1134, 1134, 1134, 1134],
            header_distance: 720,
            footer_distance: 720,
            columns: 1,
            column_gap: 720,
            title_page: false,
            numbering: None,
        }
    }
}

/// Where a frame floats and how text goes round it, from its style and its
/// own place.
#[must_use]
pub fn anchor_of(graphic: &Graphic, anchor_type: &str, x: Option<i32>, y: Option<i32>) -> Anchor {
    let page = anchor_type == "page";
    let horizontal_from = match graphic.horizontal_rel.as_deref() {
        Some("page") => Relative::Page,
        Some("page-content") | Some("page-content-start") => Relative::Margin,
        Some("page-start-margin") => Relative::LeftMargin,
        Some("page-end-margin") => Relative::RightMargin,
        Some("char") => Relative::Character,
        _ if page => Relative::Page,
        _ => Relative::Column,
    };
    let vertical_from = match graphic.vertical_rel.as_deref() {
        Some("page") => Relative::Page,
        Some("page-content") => Relative::Margin,
        Some("char") | Some("line") | Some("baseline") => Relative::Line,
        _ if page => Relative::Page,
        _ => Relative::Paragraph,
    };
    let aligned = |word: &str| Placement::Aligned(word.to_owned());
    let horizontal = match graphic.horizontal_pos.as_deref() {
        Some("left") => aligned("left"),
        Some("center") => aligned("center"),
        Some("right") => aligned("right"),
        Some("inside") => aligned("inside"),
        Some("outside") => aligned("outside"),
        _ => Placement::Offset(i64::from(x.unwrap_or(0)) * 635),
    };
    let vertical = match graphic.vertical_pos.as_deref() {
        Some("top") => aligned("top"),
        Some("middle") => aligned("center"),
        Some("bottom") => aligned("bottom"),
        _ => Placement::Offset(i64::from(y.unwrap_or(0)) * 635),
    };
    let (wrap, side) = match graphic.wrap.as_deref() {
        Some("none") => (Wrap::TopAndBottom, WrapSide::BothSides),
        Some("run-through") => (Wrap::None, WrapSide::BothSides),
        Some("left") => (Wrap::Square, WrapSide::Left),
        Some("right") => (Wrap::Square, WrapSide::Right),
        Some("dynamic") => (Wrap::Square, WrapSide::Largest),
        _ => (Wrap::Square, WrapSide::BothSides),
    };
    let wrap = if graphic.contour && wrap == Wrap::Square { Wrap::Tight } else { wrap };
    let [left, right, top, bottom] = graphic.margins.map(|side| i64::from(side.unwrap_or(0)) * 635);
    Anchor {
        wrap,
        side,
        behind_text: wrap == Wrap::None && graphic.run_through.as_deref() == Some("background"),
        horizontal_from,
        horizontal,
        vertical_from,
        vertical,
        distance: (left, right, top, bottom),
        depth: USUAL_DEPTH,
        ..Anchor::default()
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

#[must_use]
pub fn colour(value: &str) -> Option<String> {
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

/// A border as the format writes one — `0.5pt solid #000000` — as a line of
/// the model's; nothing for `none`.
#[must_use]
pub fn border_of(value: &str) -> Option<Border> {
    border_with_widths(value, None)
}

/// The same, with the widths of a double line's two lines and the gap
/// between them, which is where this format keeps how thick each line is.
fn border_with_widths(value: &str, widths: Option<&str>) -> Option<Border> {
    let mut width = None;
    let mut style = "solid";
    let mut colour_found = None;
    for word in value.split_whitespace() {
        if let Some(hex) = colour(word) {
            colour_found = Some(hex);
        } else if let Some(twips) = twips(word) {
            width = Some(twips);
        } else {
            style = match word {
                "none" | "hidden" => return None,
                other => other,
            };
        }
    }
    let name = match style {
        "double" => "double",
        "dotted" => "dotted",
        "dashed" => "dashed",
        "groove" => "threeDEngrave",
        "ridge" => "threeDEmboss",
        "inset" => "inset",
        "outset" => "outset",
        _ => "single",
    };
    // Eighths of a point: a double line by each of its lines.
    let inner = widths.and_then(|widths| widths.split_whitespace().next()).and_then(twips);
    let twips = match (name, inner) {
        ("double", Some(inner)) => inner,
        ("double", None) => width.unwrap_or(30) / 3,
        _ => width.unwrap_or(10),
    };
    let size = u32::try_from((twips * 8 + 10) / 20).unwrap_or(4).max(2);
    Some(Border::line(name, size, Some(colour_found.as_deref().unwrap_or("auto"))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_border_is_its_width_style_and_colour() {
        let line = border_of("0.5pt solid #000000").expect("a line");
        assert_eq!(
            (line.style.as_str(), line.size, line.color.as_deref()),
            ("single", 4, Some("000000"))
        );
        let double =
            border_with_widths("2.25pt double #ff0000", Some("0.0104in 0.0104in 0.0104in"))
                .expect("a line");
        assert_eq!((double.style.as_str(), double.size), ("double", 6));
        assert!(border_of("none").is_none());
    }

    #[test]
    fn a_page_with_a_header_has_it_in_its_top_margin() {
        let layout = PageLayout {
            width: Some(11_906),
            height: Some(16_838),
            margins: [Some(708), Some(1100), Some(708), Some(1300)],
            header_height: Some(292),
            footer_height: Some(492),
            ..PageLayout::default()
        };
        let setup = layout.setup(true, true);
        assert_eq!(setup.margins, [1000, 1100, 1200, 1300]);
        assert_eq!(setup.header_distance, 708);
        assert_eq!(layout.setup(false, false).margins[0], 708);
    }
}
