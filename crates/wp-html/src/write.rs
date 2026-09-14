//! Writing a document out as a page.
//!
//! # What is written
//!
//! A page Word reads back as the document it was, and a browser shows as
//! one: the head with a `<style>` block naming the styles the document
//! uses, as Word's own pages do; paragraphs as `<p>` with their class and
//! their formatting in a `style` attribute; runs as `<span>` with theirs;
//! lists as `<ul>` and `<ol>`; tables as `<table>` with widths; pictures as
//! `<img>` beside the page in a folder named after it, which is what Word
//! calls a Web Page — or inside the file, for the single-file kind.

use wp_docx::links::Destination;
use wp_docx::model::{
    Alignment, Block, BreakKind, LineRule, Paragraph, Run, RunContent, RunProperties, Underline,
    VerticalAlignment,
};
use wp_docx::Document;

/// A picture the page refers to: its name beside the page, its kind, and
/// its bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PictureFile {
    pub name: String,
    pub content_type: &'static str,
    pub bytes: Vec<u8>,
}

/// A page and the pictures it refers to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page {
    pub html: String,
    pub pictures: Vec<PictureFile>,
}

/// Writes the document as a page, with its pictures in a folder beside it
/// named after the page — `letter_files` for `letter.htm`, as Word names
/// it.
#[must_use]
pub fn write(document: &Document, name: &str, title: Option<&str>) -> Page {
    let folder = format!("{}_files", name.rsplit_once('.').map_or(name, |(stem, _)| stem));
    let mut writer = Writer { out: String::new(), folder, pictures: Vec::new(), counts: [0, 0] };
    let body = document.body();
    let links = document.hyperlinks();

    writer.out.push_str(
        "<html xmlns:o=\"urn:schemas-microsoft-com:office:office\" xmlns:w=\"urn:schemas-microsoft-com:office:word\" xmlns=\"http://www.w3.org/TR/REC-html40\">\n<head>\n<meta http-equiv=\"Content-Type\" content=\"text/html; charset=utf-8\">\n<meta name=\"Generator\" content=\"Word Processor\">\n",
    );
    writer.out.push_str(&format!("<title>{}</title>\n", escape(title.unwrap_or(name))));
    writer.out.push_str("<style>\n");
    writer.out.push_str("p.MsoNormal, li.MsoNormal, div.MsoNormal {margin:0in 0in 8pt 0in; line-height:107%; font-size:11pt; font-family:Calibri, sans-serif;}\n");
    writer.out.push_str("h1 {margin:12pt 0in 0in 0in; font-size:16pt; font-family:Calibri, sans-serif; color:#2F5496; font-weight:normal;}\n");
    writer.out.push_str("h2 {margin:2pt 0in 0in 0in; font-size:13pt; font-family:Calibri, sans-serif; color:#2F5496; font-weight:normal;}\n");
    writer.out.push_str("h3 {margin:2pt 0in 0in 0in; font-size:12pt; font-family:Calibri, sans-serif; color:#1F3763; font-weight:normal;}\n");
    writer.out.push_str("p.MsoTitle {margin:0in 0in 4pt 0in; font-size:28pt; font-family:Calibri, sans-serif; letter-spacing:-.5pt;}\n");
    writer.out.push_str("p.MsoListParagraph {margin-left:.5in;}\n");
    writer.out.push_str("table.MsoTableGrid {border-collapse:collapse;} table.MsoTableGrid td {border:solid windowtext 1pt; padding:0in 5.4pt;}\n");
    writer.out.push_str("</style>\n</head>\n<body lang=\"EN-US\">\n<div class=\"WordSection1\">\n");

    let mut paragraph_index = 0;
    let mut open_list: Option<(bool, u8)> = None;
    for block in &body.blocks {
        match block {
            Block::Paragraph(paragraph) => {
                // Lists are `<ul>` and `<ol>` round their items, opened when
                // the first item comes and closed when something else does.
                let list = paragraph
                    .properties
                    .numbering
                    .map(|numbering| (numbering.id == wp_docx::BULLET_LIST, numbering.level));
                if list != open_list {
                    if let Some((bullet, _)) = open_list {
                        writer.out.push_str(if bullet { "</ul>\n" } else { "</ol>\n" });
                    }
                    if let Some((bullet, level)) = list {
                        let indent = 0.5 * f32::from(level + 1);
                        writer.out.push_str(&format!(
                            "<{} style=\"margin-left:{indent}in\">\n",
                            if bullet { "ul" } else { "ol" }
                        ));
                    }
                    open_list = list;
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
                writer.paragraph(document, paragraph, &links, list.is_some());
                paragraph_index += 1;
            }
            Block::Table(table) => {
                if let Some((bullet, _)) = open_list.take() {
                    writer.out.push_str(if bullet { "</ul>\n" } else { "</ol>\n" });
                }
                writer.out.push_str(
                    "<table class=\"MsoTableGrid\" border=\"1\" cellspacing=\"0\" cellpadding=\"0\" style=\"border-collapse:collapse\">\n",
                );
                for row in &table.rows {
                    writer.out.push_str("<tr>\n");
                    for (column, cell) in row.cells.iter().enumerate() {
                        let width = cell.width.or_else(|| table.grid.get(column).copied());
                        match width {
                            Some(twips) => writer.out.push_str(&format!(
                                "<td width=\"{}\" valign=\"top\" style=\"width:{:.2}pt\">\n",
                                twips / 15,
                                f64::from(twips) / 20.0
                            )),
                            None => writer.out.push_str("<td valign=\"top\">\n"),
                        }
                        for block in &cell.blocks {
                            if let Block::Paragraph(paragraph) = block {
                                writer.paragraph(document, paragraph, &[], false);
                                paragraph_index += 1;
                            }
                        }
                        writer.out.push_str("</td>\n");
                    }
                    writer.out.push_str("</tr>\n");
                }
                writer.out.push_str("</table>\n");
            }
        }
    }
    if let Some((bullet, _)) = open_list {
        writer.out.push_str(if bullet { "</ul>\n" } else { "</ol>\n" });
    }
    writer.out.push_str("</div>\n</body>\n</html>\n");
    Page { html: writer.out, pictures: writer.pictures }
}

struct Writer {
    out: String,
    folder: String,
    pictures: Vec<PictureFile>,
    counts: [u32; 2],
}

impl Writer {
    fn paragraph(
        &mut self,
        document: &Document,
        paragraph: &Paragraph,
        links: &[(usize, usize, String)],
        in_list: bool,
    ) {
        let properties = &paragraph.properties;
        let (tag, class) = match properties.style.as_deref() {
            Some("Heading1") => ("h1", None),
            Some("Heading2") => ("h2", None),
            Some("Heading3") => ("h3", None),
            Some("Heading4") => ("h4", None),
            Some("Heading5") => ("h5", None),
            Some("Heading6") => ("h6", None),
            Some("Title") => ("p", Some("MsoTitle")),
            _ if in_list => ("li", Some("MsoNormal")),
            _ => ("p", Some("MsoNormal")),
        };
        let mut style = Vec::new();
        match properties.alignment {
            Some(Alignment::Center) => style.push("text-align:center".to_owned()),
            Some(Alignment::End) => style.push("text-align:right".to_owned()),
            Some(Alignment::Both) => style.push("text-align:justify".to_owned()),
            Some(Alignment::Start) | None => {}
        }
        if let Some(twips) = properties.indent_start.filter(|_| !in_list) {
            style.push(format!("margin-left:{}pt", pt(twips)));
        }
        if let Some(twips) = properties.indent_end {
            style.push(format!("margin-right:{}pt", pt(twips)));
        }
        if let Some(twips) = properties.indent_first_line.filter(|_| !in_list) {
            style.push(format!("text-indent:{}pt", pt(twips)));
        }
        if let Some(twips) = properties.space_before {
            style.push(format!("margin-top:{}pt", pt(twips)));
        }
        if let Some(twips) = properties.space_after {
            style.push(format!("margin-bottom:{}pt", pt(twips)));
        }
        if let Some(spacing) = properties.line_spacing {
            match spacing.rule {
                LineRule::Auto => {
                    style
                        .push(format!("line-height:{}%", (f64::from(spacing.value) / 2.4).round()));
                }
                LineRule::AtLeast => style.push(format!("line-height:{}pt", pt(spacing.value))),
                LineRule::Exact => {
                    style.push(format!("line-height:{}pt", pt(spacing.value)));
                    style.push("mso-line-height-rule:exactly".to_owned());
                }
            }
        }
        if properties.page_break_before == Some(true) {
            style.push("page-break-before:always".to_owned());
        }

        self.out.push('<');
        self.out.push_str(tag);
        if let Some(class) = class {
            self.out.push_str(&format!(" class=\"{class}\""));
        }
        if !style.is_empty() {
            // Single quotes round the attribute, as Word writes it, because a
            // font name inside is written with double ones.
            self.out.push_str(&format!(" style='{}'", style.join(";").replace('\x27', "&#39;")));
        }
        self.out.push('>');

        let mut offset = 0;
        let mut open_link: Option<usize> = None;
        for run in &paragraph.runs {
            for piece in split_at_links(run, offset, links) {
                let (start, end) = (offset, offset + piece.length);
                let link = links.iter().position(|(from, to, _)| start >= *from && end <= *to);
                if link != open_link {
                    if open_link.is_some() {
                        self.out.push_str("</a>");
                    }
                    if let Some(index) = link {
                        self.out.push_str(&format!("<a href=\"{}\">", escape(&links[index].2)));
                    }
                    open_link = link;
                }
                self.run(document, &piece.run);
                offset = end;
            }
        }
        if open_link.is_some() {
            self.out.push_str("</a>");
        }
        if paragraph.runs.is_empty() {
            self.out.push_str("&nbsp;");
        }
        self.out.push_str(&format!("</{tag}>\n"));
        let _ = &mut self.counts;
    }

    fn run(&mut self, document: &Document, run: &Run) {
        let style = character_style(&run.properties);
        let mut opened: Vec<&str> = Vec::new();
        if run.properties.bold == Some(true) {
            self.out.push_str("<b>");
            opened.push("b");
        }
        if run.properties.italic == Some(true) {
            self.out.push_str("<i>");
            opened.push("i");
        }
        if matches!(
            run.properties.underline,
            Some(
                Underline::Single
                    | Underline::Double
                    | Underline::Thick
                    | Underline::Dotted
                    | Underline::Dashed
                    | Underline::Wave
                    | Underline::Other(_)
            )
        ) {
            self.out.push_str("<u>");
            opened.push("u");
        }
        if run.properties.strike == Some(true) || run.properties.double_strike == Some(true) {
            self.out.push_str("<s>");
            opened.push("s");
        }
        match run.properties.vertical_align {
            Some(VerticalAlignment::Superscript) => {
                self.out.push_str("<sup>");
                opened.push("sup");
            }
            Some(VerticalAlignment::Subscript) => {
                self.out.push_str("<sub>");
                opened.push("sub");
            }
            _ => {}
        }
        if !style.is_empty() {
            self.out
                .push_str(&format!("<span style='{}'>", style.join(";").replace('\x27', "&#39;")));
            opened.push("span");
        }
        for content in &run.content {
            match content {
                RunContent::Text(text) => self.out.push_str(&escape(text)),
                RunContent::Tab | RunContent::PositionTab(_) => {
                    self.out.push_str(
                        "<span style=\"mso-tab-count:1\">&nbsp;&nbsp;&nbsp;&nbsp;</span>",
                    );
                }
                RunContent::Break(BreakKind::Line) => self.out.push_str("<br>"),
                RunContent::Break(BreakKind::Page | BreakKind::Column) => {
                    self.out.push_str("<br clear=\"all\" style=\"page-break-before:always\">");
                }
                RunContent::Picture(picture) => self.picture(document, picture),
                _ => {}
            }
        }
        for tag in opened.iter().rev() {
            self.out.push_str(&format!("</{tag}>"));
        }
    }

    fn picture(&mut self, document: &Document, picture: &wp_docx::model::Picture) {
        let Some(bytes) = document.embedded_part(&picture.relationship) else { return };
        let (extension, content_type) = match wp_image::Format::detect(bytes) {
            Some(wp_image::Format::Png) => ("png", "image/png"),
            Some(wp_image::Format::Jpeg) => ("jpg", "image/jpeg"),
            Some(wp_image::Format::Gif) => ("gif", "image/gif"),
            Some(wp_image::Format::Bmp) => ("bmp", "image/bmp"),
            Some(wp_image::Format::Tiff) => ("tif", "image/tiff"),
            Some(wp_image::Format::Emf) => ("emf", "image/x-emf"),
            Some(wp_image::Format::Wmf) => ("wmf", "image/x-wmf"),
            None => return,
        };
        let name = format!("{}/image{:03}.{extension}", self.folder, self.pictures.len() + 1);
        self.pictures.push(PictureFile { name: name.clone(), content_type, bytes: bytes.to_vec() });
        let width = picture.width_emu / 9525;
        let height = picture.height_emu / 9525;
        self.out.push_str(&format!(
            "<img width=\"{width}\" height=\"{height}\" src=\"{}\" style=\"width:{:.2}pt;height:{:.2}pt\"{}>",
            escape(&name),
            picture.width_emu as f64 / 12700.0,
            picture.height_emu as f64 / 12700.0,
            picture
                .description
                .as_deref()
                .map(|alt| format!(" alt=\"{}\"", escape(alt)))
                .unwrap_or_default()
        ));
    }
}

/// The character formatting a `<span>` has to carry, past what the tags
/// round it say.
fn character_style(properties: &RunProperties) -> Vec<String> {
    let mut style = Vec::new();
    if let Some(size) = properties.size_half_points {
        style.push(format!("font-size:{}pt", f64::from(size) / 2.0));
    }
    if let Some(font) = &properties.font {
        style.push(format!("font-family:\"{font}\""));
    }
    if let Some(colour) = &properties.color {
        style.push(format!("color:#{colour}"));
    }
    if let Some(hex) = properties.highlight.as_deref().and_then(highlight_hex) {
        style.push(format!("background:#{hex}"));
    }
    if properties.caps == Some(true) {
        style.push("text-transform:uppercase".to_owned());
    }
    if properties.small_caps == Some(true) {
        style.push("font-variant:small-caps".to_owned());
    }
    if properties.hidden == Some(true) {
        style.push("display:none".to_owned());
    }
    if properties.bold == Some(false) {
        style.push("font-weight:normal".to_owned());
    }
    style
}

/// A run, or part of one, and how many bytes of the paragraph's text it is.
struct Piece {
    run: Run,
    length: usize,
}

/// Cuts a run where a link begins or ends inside it.
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

/// Twips as points, written short.
fn pt(twips: i32) -> String {
    let points = f64::from(twips) / 20.0;
    if points.fract() == 0.0 {
        format!("{points:.0}")
    } else {
        format!("{points:.2}").trim_end_matches('0').to_owned()
    }
}

/// Text as a page writes it: the four characters that mean something
/// escaped, and the non-breaking space named so it survives.
#[must_use]
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\u{00A0}' => out.push_str("&nbsp;"),
            other => out.push(other),
        }
    }
    out
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
