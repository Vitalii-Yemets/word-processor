//! Writing a document out as OpenDocument text.
//!
//! # What is written
//!
//! The package as the standard lays it out: the `mimetype` first and
//! uncompressed, so that a program can tell what the file is by reading
//! its first bytes; the manifest naming every part; `styles.xml` with the
//! defaults, the heading and title styles, the fonts and the page;
//! `content.xml` with an automatic style for every distinct formatting a
//! paragraph or a run has — `P1`, `T1`, as every writer of the format
//! names them — and the text using them; `meta.xml`; and the pictures in
//! `Pictures/`.

use std::collections::BTreeMap;

use wp_docx::links::Destination;
use wp_docx::model::{
    Alignment, Block, BreakKind, LineRule, Paragraph, ParagraphProperties, Run, RunContent,
    RunProperties, TabAlignment, TabLeader, Underline, VerticalAlignment,
};
use wp_docx::Document;
use wp_xml::{escape_attribute_value as attr, escape_text};

/// A picture the package holds: its name inside, its media type, and its
/// bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PictureFile {
    pub name: String,
    pub media_type: &'static str,
    pub bytes: Vec<u8>,
}

/// The parts of the package, before they are zipped.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Parts {
    pub content: String,
    pub styles: String,
    pub meta: String,
    pub pictures: Vec<PictureFile>,
}

const NAMESPACES: &str = "xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\" xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" xmlns:table=\"urn:oasis:names:tc:opendocument:xmlns:table:1.0\" xmlns:draw=\"urn:oasis:names:tc:opendocument:xmlns:drawing:1.0\" xmlns:fo=\"urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:meta=\"urn:oasis:names:tc:opendocument:xmlns:meta:1.0\" xmlns:svg=\"urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0\" office:version=\"1.3\"";

/// Writes the parts of a document.
#[must_use]
pub fn write(document: &Document, title: Option<&str>) -> Parts {
    let body = document.body();
    let links = document.hyperlinks();
    let mut writer = Writer {
        paragraph_styles: BTreeMap::new(),
        text_styles: BTreeMap::new(),
        fonts: Vec::new(),
        pictures: Vec::new(),
        columns: Vec::new(),
        out: String::new(),
    };

    // The body first, gathering the automatic styles it needs; the styles
    // then go above it.
    let mut paragraph_index = 0;
    let mut open_list: Option<(bool, u8)> = None;
    for block in &body.blocks {
        match block {
            Block::Paragraph(paragraph) => {
                let list =
                    paragraph.properties.numbering.map(|n| (n.id == wp_docx::BULLET_LIST, n.level));
                if list != open_list {
                    if open_list.is_some() {
                        writer.out.push_str("</text:list-item></text:list>\n");
                    }
                    if let Some((bullet, _)) = list {
                        writer.out.push_str(&format!(
                            "<text:list text:style-name=\"{}\"><text:list-item>",
                            if bullet { "LBullet" } else { "LNumber" }
                        ));
                    }
                    open_list = list;
                } else if list.is_some() {
                    writer.out.push_str("</text:list-item><text:list-item>");
                }
                let links: Vec<(usize, usize, String)> = links
                    .iter()
                    .filter(|link| link.paragraph == paragraph_index)
                    .filter_map(|link| match &link.destination {
                        Destination::Address(address) => {
                            Some((link.range.0, link.range.1, address.clone()))
                        }
                        Destination::Place(_) => None,
                    })
                    .collect();
                writer.paragraph(document, paragraph, &links);
                paragraph_index += 1;
            }
            Block::Table(table) => {
                if open_list.take().is_some() {
                    writer.out.push_str("</text:list-item></text:list>\n");
                }
                let table_index = writer.columns.len();
                let widths: Vec<i32> = if table.grid.is_empty() {
                    table
                        .rows
                        .first()
                        .map(|row| row.cells.iter().map(|c| c.width.unwrap_or(2880)).collect())
                        .unwrap_or_default()
                } else {
                    table.grid.clone()
                };
                writer.columns.push(widths.clone());
                writer.out.push_str(&format!(
                    "<table:table table:name=\"Table{}\" table:style-name=\"Table{}\">\n",
                    table_index + 1,
                    table_index + 1
                ));
                for (column, _) in widths.iter().enumerate() {
                    writer.out.push_str(&format!(
                        "<table:table-column table:style-name=\"Table{}.C{}\"/>\n",
                        table_index + 1,
                        column + 1
                    ));
                }
                for row in &table.rows {
                    writer.out.push_str("<table:table-row>\n");
                    for cell in &row.cells {
                        writer.out.push_str("<table:table-cell office:value-type=\"string\"");
                        if cell.span > 1 {
                            writer.out.push_str(&format!(
                                " table:number-columns-spanned=\"{}\"",
                                cell.span
                            ));
                        }
                        writer.out.push_str(">\n");
                        for block in &cell.blocks {
                            if let Block::Paragraph(paragraph) = block {
                                writer.paragraph(document, paragraph, &[]);
                                paragraph_index += 1;
                            }
                        }
                        writer.out.push_str("</table:table-cell>\n");
                        for _ in 1..cell.span {
                            writer.out.push_str("<table:covered-table-cell/>\n");
                        }
                    }
                    writer.out.push_str("</table:table-row>\n");
                }
                writer.out.push_str("</table:table>\n");
            }
        }
    }
    if open_list.is_some() {
        writer.out.push_str("</text:list-item></text:list>\n");
    }

    let mut content = String::new();
    content.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    content.push_str(&format!("<office:document-content {NAMESPACES}>\n"));
    content.push_str("<office:font-face-decls>\n");
    for font in &writer.fonts {
        content.push_str(&format!(
            "<style:font-face style:name=\"{0}\" svg:font-family=\"&apos;{0}&apos;\"/>\n",
            attr(font)
        ));
    }
    content.push_str("</office:font-face-decls>\n<office:automatic-styles>\n");
    for (style, name) in &writer.paragraph_styles {
        content.push_str(&format!("<style:style style:name=\"{name}\" style:family=\"paragraph\" style:parent-style-name=\"{}\">{}</style:style>\n", attr(&style.parent), style.xml));
    }
    for (style, name) in &writer.text_styles {
        content.push_str(&format!(
            "<style:style style:name=\"{name}\" style:family=\"text\">{style}</style:style>\n"
        ));
    }
    for (index, widths) in writer.columns.iter().enumerate() {
        let total: i32 = widths.iter().sum();
        content.push_str(&format!(
            "<style:style style:name=\"Table{}\" style:family=\"table\"><style:table-properties style:width=\"{}\" table:align=\"left\"/></style:style>\n",
            index + 1,
            length(total)
        ));
        for (column, width) in widths.iter().enumerate() {
            content.push_str(&format!(
                "<style:style style:name=\"Table{}.C{}\" style:family=\"table-column\"><style:table-column-properties style:column-width=\"{}\"/></style:style>\n",
                index + 1,
                column + 1,
                length(*width)
            ));
        }
    }
    content.push_str("<text:list-style style:name=\"LBullet\">");
    for level in 1..=9u32 {
        content.push_str(&format!(
            "<text:list-level-style-bullet text:level=\"{level}\" text:bullet-char=\"\u{2022}\"><style:list-level-properties text:list-level-position-and-space-mode=\"label-alignment\"><style:list-level-label-alignment text:label-followed-by=\"listtab\" text:list-tab-stop-position=\"{0}\" fo:text-indent=\"-0.25in\" fo:margin-left=\"{0}\"/></style:list-level-properties></text:list-level-style-bullet>",
            format!("{:.2}in", 0.5 * f64::from(level))
        ));
    }
    content.push_str("</text:list-style>\n<text:list-style style:name=\"LNumber\">");
    for level in 1..=9u32 {
        content.push_str(&format!(
            "<text:list-level-style-number text:level=\"{level}\" style:num-suffix=\".\" style:num-format=\"1\"><style:list-level-properties text:list-level-position-and-space-mode=\"label-alignment\"><style:list-level-label-alignment text:label-followed-by=\"listtab\" text:list-tab-stop-position=\"{0}\" fo:text-indent=\"-0.25in\" fo:margin-left=\"{0}\"/></style:list-level-properties></text:list-level-style-number>",
            format!("{:.2}in", 0.5 * f64::from(level))
        ));
    }
    content
        .push_str("</text:list-style>\n</office:automatic-styles>\n<office:body>\n<office:text>\n");
    content.push_str(&writer.out);
    content.push_str("</office:text>\n</office:body>\n</office:document-content>\n");

    let (width, height) = document.page_size();
    let (top, right, bottom, left) = document.page_margins();
    let mut styles = String::new();
    styles.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    styles.push_str(&format!("<office:document-styles {NAMESPACES}>\n"));
    styles.push_str("<office:font-face-decls><style:font-face style:name=\"Calibri\" svg:font-family=\"Calibri\"/><style:font-face style:name=\"Calibri Light\" svg:font-family=\"&apos;Calibri Light&apos;\"/></office:font-face-decls>\n");
    styles.push_str("<office:styles>\n");
    styles.push_str("<style:default-style style:family=\"paragraph\"><style:paragraph-properties fo:margin-bottom=\"8pt\" fo:line-height=\"107%\"/><style:text-properties style:font-name=\"Calibri\" fo:font-size=\"11pt\" fo:language=\"en\" fo:country=\"US\"/></style:default-style>\n");
    styles.push_str(
        "<style:style style:name=\"Standard\" style:family=\"paragraph\" style:class=\"text\"/>\n",
    );
    for (level, size, before) in
        [(1, 16, 12), (2, 13, 2), (3, 12, 2), (4, 11, 2), (5, 11, 2), (6, 11, 2)]
    {
        let colour = if level <= 2 { "#2F5496" } else { "#1F3763" };
        styles.push_str(&format!(
            "<style:style style:name=\"Heading_20_{level}\" style:display-name=\"Heading {level}\" style:family=\"paragraph\" style:parent-style-name=\"Standard\" style:next-style-name=\"Standard\" style:default-outline-level=\"{level}\" style:class=\"text\"><style:paragraph-properties fo:margin-top=\"{before}pt\" fo:margin-bottom=\"0pt\" fo:keep-with-next=\"always\"/><style:text-properties style:font-name=\"Calibri Light\" fo:font-size=\"{size}pt\" fo:color=\"{colour}\"{}/></style:style>\n",
            if level >= 4 { " fo:font-style=\"italic\"" } else { "" }
        ));
    }
    styles.push_str("<style:style style:name=\"Title\" style:display-name=\"Title\" style:family=\"paragraph\" style:parent-style-name=\"Standard\" style:next-style-name=\"Standard\" style:class=\"chapter\"><style:paragraph-properties fo:margin-bottom=\"4pt\"/><style:text-properties style:font-name=\"Calibri Light\" fo:font-size=\"28pt\"/></style:style>\n");
    styles.push_str("<style:style style:name=\"Internet_20_link\" style:display-name=\"Internet link\" style:family=\"text\"><style:text-properties fo:color=\"#0563C1\" style:text-underline-style=\"solid\" style:text-underline-width=\"auto\" style:text-underline-color=\"font-color\"/></style:style>\n");
    styles.push_str("</office:styles>\n<office:automatic-styles>\n");
    styles.push_str(&format!(
        "<style:page-layout style:name=\"Mpm1\"><style:page-layout-properties fo:page-width=\"{}\" fo:page-height=\"{}\" style:print-orientation=\"{}\" fo:margin-top=\"{}\" fo:margin-bottom=\"{}\" fo:margin-left=\"{}\" fo:margin-right=\"{}\"/></style:page-layout>\n",
        length(width),
        length(height),
        if width > height { "landscape" } else { "portrait" },
        length(top),
        length(bottom),
        length(left),
        length(right)
    ));
    styles.push_str("</office:automatic-styles>\n<office:master-styles><style:master-page style:name=\"Standard\" style:page-layout-name=\"Mpm1\"/></office:master-styles>\n</office:document-styles>\n");

    let mut meta = String::new();
    meta.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    meta.push_str(&format!("<office:document-meta {NAMESPACES}><office:meta><meta:generator>Word Processor</meta:generator>"));
    if let Some(title) = title {
        meta.push_str(&format!("<dc:title>{}</dc:title>", escape_text(title)));
    }
    meta.push_str("</office:meta></office:document-meta>\n");

    Parts { content, styles, meta, pictures: writer.pictures }
}

/// An automatic paragraph style: what it is based on, and what it says.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct ParagraphStyle {
    parent: String,
    xml: String,
}

struct Writer {
    paragraph_styles: BTreeMap<ParagraphStyle, String>,
    text_styles: BTreeMap<String, String>,
    fonts: Vec<String>,
    pictures: Vec<PictureFile>,
    /// The column widths of each table, for its styles.
    columns: Vec<Vec<i32>>,
    out: String,
}

impl Writer {
    fn paragraph(
        &mut self,
        document: &Document,
        paragraph: &Paragraph,
        links: &[(usize, usize, String)],
    ) {
        let properties = &paragraph.properties;
        let (parent, level) = match properties.style.as_deref() {
            Some("Title") => ("Title".to_owned(), None),
            Some(style) if style.starts_with("Heading") => {
                let level = style
                    .trim_start_matches("Heading")
                    .parse::<u8>()
                    .ok()
                    .filter(|l| (1..=6).contains(l));
                match level {
                    Some(level) => (format!("Heading_20_{level}"), Some(level)),
                    None => ("Standard".to_owned(), None),
                }
            }
            _ => ("Standard".to_owned(), None),
        };
        let xml = paragraph_properties_xml(properties);
        let style_name = if xml.is_empty() {
            parent.clone()
        } else {
            let key = ParagraphStyle { parent: parent.clone(), xml };
            let next = format!("P{}", self.paragraph_styles.len() + 1);
            self.paragraph_styles.entry(key).or_insert(next).clone()
        };

        match level {
            Some(level) => self.out.push_str(&format!(
                "<text:h text:style-name=\"{style_name}\" text:outline-level=\"{level}\">"
            )),
            None => self.out.push_str(&format!("<text:p text:style-name=\"{style_name}\">")),
        }
        let mut offset = 0;
        let mut open_link: Option<usize> = None;
        for run in &paragraph.runs {
            for piece in split_at_links(run, offset, links) {
                let (start, end) = (offset, offset + piece.length);
                let link = links.iter().position(|(from, to, _)| start >= *from && end <= *to);
                if link != open_link {
                    if open_link.is_some() {
                        self.out.push_str("</text:a>");
                    }
                    if let Some(index) = link {
                        self.out.push_str(&format!(
                            "<text:a xlink:type=\"simple\" xlink:href=\"{}\" text:style-name=\"Internet_20_link\">",
                            attr(&links[index].2)
                        ));
                    }
                    open_link = link;
                }
                self.run(document, &piece.run);
                offset = end;
            }
        }
        if open_link.is_some() {
            self.out.push_str("</text:a>");
        }
        self.out.push_str(if level.is_some() { "</text:h>\n" } else { "</text:p>\n" });
    }

    fn run(&mut self, document: &Document, run: &Run) {
        let xml = text_properties_xml(&run.properties);
        if let Some(font) = &run.properties.font {
            if !self.fonts.contains(font) {
                self.fonts.push(font.clone());
            }
        }
        let style = if xml.is_empty() {
            None
        } else {
            let next = format!("T{}", self.text_styles.len() + 1);
            Some(self.text_styles.entry(xml).or_insert(next).clone())
        };
        if let Some(style) = &style {
            self.out.push_str(&format!("<text:span text:style-name=\"{style}\">"));
        }
        for content in &run.content {
            match content {
                RunContent::Text(text) => self.text(text),
                RunContent::Tab | RunContent::PositionTab(_) => self.out.push_str("<text:tab/>"),
                RunContent::Break(BreakKind::Line) => self.out.push_str("<text:line-break/>"),
                RunContent::Break(BreakKind::Page | BreakKind::Column) => {
                    // A page break is a paragraph property in this format;
                    // in the middle of a paragraph the nearest thing is a
                    // line break.
                    self.out.push_str("<text:line-break/>");
                }
                RunContent::Picture(picture) => self.picture(document, picture),
                _ => {}
            }
        }
        if style.is_some() {
            self.out.push_str("</text:span>");
        }
    }

    /// Text with its runs of spaces written as the format wants them: one
    /// space as itself, the rest counted.
    fn text(&mut self, text: &str) {
        let mut spaces = 0;
        for character in text.chars() {
            if character == ' ' {
                spaces += 1;
                continue;
            }
            self.spaces(spaces);
            spaces = 0;
            match character {
                '\t' => self.out.push_str("<text:tab/>"),
                '\n' => self.out.push_str("<text:line-break/>"),
                other => self.out.push_str(&escape_text(&other.to_string())),
            }
        }
        self.spaces(spaces);
    }

    fn spaces(&mut self, count: usize) {
        match count {
            0 => {}
            1 => self.out.push(' '),
            more => self.out.push_str(&format!(" <text:s text:c=\"{}\"/>", more - 1)),
        }
    }

    fn picture(&mut self, document: &Document, picture: &wp_docx::model::Picture) {
        let Some(bytes) = document.embedded_part(&picture.relationship) else { return };
        let (extension, media_type) = match wp_image::Format::detect(bytes) {
            Some(wp_image::Format::Png) => ("png", "image/png"),
            Some(wp_image::Format::Jpeg) => ("jpg", "image/jpeg"),
            Some(wp_image::Format::Gif) => ("gif", "image/gif"),
            Some(wp_image::Format::Bmp) => ("bmp", "image/bmp"),
            Some(wp_image::Format::Tiff) => ("tif", "image/tiff"),
            Some(wp_image::Format::Emf) => ("emf", "image/x-emf"),
            Some(wp_image::Format::Wmf) => ("wmf", "image/x-wmf"),
            None => return,
        };
        let name = format!("Pictures/image{}.{extension}", self.pictures.len() + 1);
        self.pictures.push(PictureFile { name: name.clone(), media_type, bytes: bytes.to_vec() });
        self.out.push_str(&format!(
            "<draw:frame draw:name=\"Image{}\" text:anchor-type=\"as-char\" svg:width=\"{}\" svg:height=\"{}\"><draw:image xlink:href=\"{}\" xlink:type=\"simple\" xlink:show=\"embed\" xlink:actuate=\"onLoad\"/></draw:frame>",
            self.pictures.len(),
            length((picture.width_emu / 635) as i32),
            length((picture.height_emu / 635) as i32),
            attr(&name)
        ));
    }
}

/// The paragraph properties element for what a paragraph says beyond its
/// style, or nothing.
fn paragraph_properties_xml(properties: &ParagraphProperties) -> String {
    let mut attributes = Vec::new();
    match properties.alignment {
        Some(Alignment::Center) => attributes.push("fo:text-align=\"center\"".to_owned()),
        Some(Alignment::End) => attributes.push("fo:text-align=\"end\"".to_owned()),
        Some(Alignment::Both) => attributes.push("fo:text-align=\"justify\"".to_owned()),
        Some(Alignment::Start) => attributes.push("fo:text-align=\"start\"".to_owned()),
        None => {}
    }
    let in_list = properties.numbering.is_some();
    if let Some(twips) = properties.indent_start.filter(|_| !in_list) {
        attributes.push(format!("fo:margin-left=\"{}\"", length(twips)));
    }
    if let Some(twips) = properties.indent_end {
        attributes.push(format!("fo:margin-right=\"{}\"", length(twips)));
    }
    if let Some(twips) = properties.indent_first_line.filter(|_| !in_list) {
        attributes.push(format!("fo:text-indent=\"{}\"", length(twips)));
    }
    if let Some(twips) = properties.space_before {
        attributes.push(format!("fo:margin-top=\"{}\"", length(twips)));
    }
    if let Some(twips) = properties.space_after {
        attributes.push(format!("fo:margin-bottom=\"{}\"", length(twips)));
    }
    if let Some(spacing) = properties.line_spacing {
        match spacing.rule {
            LineRule::Auto => attributes
                .push(format!("fo:line-height=\"{}%\"", (f64::from(spacing.value) / 2.4).round())),
            LineRule::Exact => {
                attributes.push(format!("fo:line-height=\"{}\"", length(spacing.value)))
            }
            LineRule::AtLeast => {
                attributes.push(format!("style:line-height-at-least=\"{}\"", length(spacing.value)))
            }
        }
    }
    if properties.page_break_before == Some(true) {
        attributes.push("fo:break-before=\"page\"".to_owned());
    }
    if properties.keep_next == Some(true) {
        attributes.push("fo:keep-with-next=\"always\"".to_owned());
    }
    if properties.keep_lines == Some(true) {
        attributes.push("fo:keep-together=\"always\"".to_owned());
    }
    let mut stops = String::new();
    for stop in &properties.tab_stops {
        let kind = match stop.alignment {
            TabAlignment::Center => " style:type=\"center\"",
            TabAlignment::End => " style:type=\"right\"",
            TabAlignment::Decimal => " style:type=\"char\" style:char=\".\"",
            _ => "",
        };
        let leader = match stop.leader {
            TabLeader::Dot => " style:leader-style=\"dotted\" style:leader-text=\".\"",
            TabLeader::Hyphen => " style:leader-style=\"dash\" style:leader-text=\"-\"",
            TabLeader::Underscore => " style:leader-style=\"solid\" style:leader-text=\"_\"",
            _ => "",
        };
        stops.push_str(&format!(
            "<style:tab-stop style:position=\"{}\"{kind}{leader}/>",
            length(stop.position)
        ));
    }
    if attributes.is_empty() && stops.is_empty() {
        return String::new();
    }
    if stops.is_empty() {
        format!("<style:paragraph-properties {}/>", attributes.join(" "))
    } else {
        format!("<style:paragraph-properties {}><style:tab-stops>{stops}</style:tab-stops></style:paragraph-properties>", attributes.join(" "))
    }
}

/// The text properties element for what a run says, or nothing.
fn text_properties_xml(properties: &RunProperties) -> String {
    let mut attributes = Vec::new();
    match properties.bold {
        Some(true) => attributes.push("fo:font-weight=\"bold\"".to_owned()),
        Some(false) => attributes.push("fo:font-weight=\"normal\"".to_owned()),
        None => {}
    }
    match properties.italic {
        Some(true) => attributes.push("fo:font-style=\"italic\"".to_owned()),
        Some(false) => attributes.push("fo:font-style=\"normal\"".to_owned()),
        None => {}
    }
    match &properties.underline {
        Some(Underline::None) => attributes.push("style:text-underline-style=\"none\"".to_owned()),
        Some(Underline::Double) => attributes.push("style:text-underline-style=\"solid\" style:text-underline-type=\"double\" style:text-underline-width=\"auto\"".to_owned()),
        Some(Underline::Thick) => attributes.push("style:text-underline-style=\"solid\" style:text-underline-width=\"bold\"".to_owned()),
        Some(Underline::Dotted) => attributes.push("style:text-underline-style=\"dotted\" style:text-underline-width=\"auto\"".to_owned()),
        Some(Underline::Dashed) => attributes.push("style:text-underline-style=\"dash\" style:text-underline-width=\"auto\"".to_owned()),
        Some(Underline::Wave) => attributes.push("style:text-underline-style=\"wave\" style:text-underline-width=\"auto\"".to_owned()),
        Some(_) => attributes.push("style:text-underline-style=\"solid\" style:text-underline-width=\"auto\"".to_owned()),
        None => {}
    }
    if properties.strike == Some(true) {
        attributes.push("style:text-line-through-style=\"solid\"".to_owned());
    }
    if properties.double_strike == Some(true) {
        attributes.push(
            "style:text-line-through-style=\"solid\" style:text-line-through-type=\"double\""
                .to_owned(),
        );
    }
    if let Some(size) = properties.size_half_points {
        attributes.push(format!("fo:font-size=\"{}pt\"", f64::from(size) / 2.0));
    }
    if let Some(font) = &properties.font {
        attributes.push(format!("style:font-name=\"{}\"", attr(font)));
    }
    if let Some(colour) = &properties.color {
        attributes.push(format!("fo:color=\"#{colour}\""));
    }
    if let Some(hex) = properties.highlight.as_deref().and_then(highlight_hex) {
        attributes.push(format!("fo:background-color=\"#{hex}\""));
    }
    match properties.vertical_align {
        Some(VerticalAlignment::Superscript) => {
            attributes.push("style:text-position=\"super 58%\"".to_owned())
        }
        Some(VerticalAlignment::Subscript) => {
            attributes.push("style:text-position=\"sub 58%\"".to_owned())
        }
        _ => {}
    }
    if properties.caps == Some(true) {
        attributes.push("fo:text-transform=\"uppercase\"".to_owned());
    }
    if properties.small_caps == Some(true) {
        attributes.push("fo:font-variant=\"small-caps\"".to_owned());
    }
    if properties.hidden == Some(true) {
        attributes.push("text:display=\"none\"".to_owned());
    }
    if attributes.is_empty() {
        String::new()
    } else {
        format!("<style:text-properties {}/>", attributes.join(" "))
    }
}

/// A run, or part of one, and how many bytes of the paragraph's text it is.
struct Piece {
    run: Run,
    length: usize,
}

fn split_at_links(run: &Run, offset: usize, links: &[(usize, usize, String)]) -> Vec<Piece> {
    let mut pieces = Vec::new();
    let mut at = offset;
    for content in &run.content {
        match content {
            RunContent::Text(text) => {
                let mut cuts: Vec<usize> = links
                    .iter()
                    .flat_map(|(from, to, _)| [*from, *to])
                    .filter(|edge| *edge > at && *edge < at + text.len())
                    .map(|edge| edge - at)
                    .filter(|cut| text.is_char_boundary(*cut))
                    .collect();
                cuts.sort_unstable();
                cuts.dedup();
                let mut from = 0;
                for cut in cuts.into_iter().chain([text.len()]) {
                    if cut > from {
                        pieces.push(Piece {
                            run: Run {
                                properties: run.properties.clone(),
                                content: vec![RunContent::Text(text[from..cut].to_owned())],
                                field: None,
                                revision: None,
                                format_change: None,
                            },
                            length: cut - from,
                        });
                    }
                    from = cut;
                }
                at += text.len();
            }
            other => {
                let length = match other {
                    RunContent::Tab | RunContent::PositionTab(_) | RunContent::Break(_) => 1,
                    RunContent::Picture(_)
                    | RunContent::Shape(_)
                    | RunContent::Group(_)
                    | RunContent::Chart(_)
                    | RunContent::Diagram(_)
                    | RunContent::Ink(_) => 1,
                    _ => 0,
                };
                pieces.push(Piece {
                    run: Run {
                        properties: run.properties.clone(),
                        content: vec![other.clone()],
                        field: None,
                        revision: None,
                        format_change: None,
                    },
                    length,
                });
                at += length;
            }
        }
    }
    pieces
}

/// Twips as the format writes a length: inches, short.
fn length(twips: i32) -> String {
    let inches = f64::from(twips) / 1440.0;
    let text = format!("{inches:.4}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    format!("{}in", if text.is_empty() { "0" } else { text })
}

fn highlight_hex(name: &str) -> Option<&'static str> {
    Some(match name {
        "yellow" => "FFFF00",
        "green" => "00FF00",
        "cyan" => "00FFFF",
        "magenta" => "FF00FF",
        "blue" => "0000FF",
        "red" => "FF0000",
        "darkBlue" => "000080",
        "darkCyan" => "008080",
        "darkGreen" => "008000",
        "darkMagenta" => "800080",
        "darkRed" => "800000",
        "darkYellow" => "808000",
        "darkGray" => "808080",
        "lightGray" => "C0C0C0",
        "black" => "000000",
        "white" => "FFFFFF",
        _ => return None,
    })
}
