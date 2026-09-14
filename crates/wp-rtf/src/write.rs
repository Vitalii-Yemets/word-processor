//! Writing a document out as RTF.
//!
//! # What is written
//!
//! What Word writes, as far as this program's model goes: the header with
//! the fonts, the colours, the styles and the two lists the document may
//! use; then the paragraphs with their formatting and their runs, the tables
//! row by row, the pictures as hex, and the links as fields. Characters the
//! Western code page has not got go out as `\u` with a question mark to
//! stand in, which is what every reader since 1997 understands.
//!
//! The file is one long line broken wherever it is convenient: RTF does not
//! care where its lines end.

use wp_docx::links::Destination;
use wp_docx::model::{
    Alignment, Block, LineRule, Paragraph, Run, RunContent, RunProperties, TabAlignment, TabLeader,
    Underline, VerticalAlignment,
};
use wp_docx::Document;
use wp_text::Encoding;

/// The document as RTF.
#[must_use]
pub fn write(document: &Document) -> Vec<u8> {
    let body = document.body();
    let mut writer = Writer {
        out: String::new(),
        fonts: vec!["Calibri".to_owned()],
        colours: Vec::new(),
        styles: Vec::new(),
        counts: [0, 0],
    };

    // Everything the body refers to, gathered first so the tables at the top
    // are complete before the text that uses them.
    for paragraph in body.paragraphs() {
        if let Some(style) = &paragraph.properties.style {
            if !writer.styles.contains(style) {
                writer.styles.push(style.clone());
            }
        }
        for run in &paragraph.runs {
            if let Some(font) = &run.properties.font {
                if !writer.fonts.iter().any(|held| held.eq_ignore_ascii_case(font)) {
                    writer.fonts.push(font.clone());
                }
            }
            if let Some(colour) = &run.properties.color {
                writer.colour_index(colour);
            }
            if let Some(name) = &run.properties.highlight {
                if let Some(hex) = highlight_hex(name) {
                    writer.colour_index(hex);
                }
            }
        }
    }

    writer.header();
    let links = document.hyperlinks();
    let mut paragraph_index = 0;
    for block in &body.blocks {
        match block {
            Block::Paragraph(paragraph) => {
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
                writer.paragraph(document, paragraph, &links, false);
                paragraph_index += 1;
            }
            Block::Table(table) => {
                for row in &table.rows {
                    writer.out.push_str("\\trowd\\trgaph108\\trleft-108");
                    let mut edge = -108;
                    for (column, cell) in row.cells.iter().enumerate() {
                        let width =
                            cell.width.or_else(|| table.grid.get(column).copied()).unwrap_or(2880);
                        edge += width;
                        writer.out.push_str(&format!("\\cellx{edge}"));
                    }
                    writer.out.push('\n');
                    for cell in &row.cells {
                        let mut first = true;
                        for block in &cell.blocks {
                            if let Block::Paragraph(paragraph) = block {
                                if !first {
                                    writer.out.push_str("\\par\n");
                                }
                                writer.paragraph(document, paragraph, &[], true);
                                paragraph_index += 1;
                                first = false;
                            }
                        }
                        writer.out.push_str("\\cell\n");
                    }
                    writer.out.push_str("\\row\n");
                }
            }
        }
    }
    writer.out.push('}');
    writer.out.into_bytes()
}

struct Writer {
    out: String,
    /// The font table, the default font first.
    fonts: Vec<String>,
    /// The colour table, as six hex digits each; the automatic colour is the
    /// empty entry before them.
    colours: Vec<String>,
    /// The paragraph styles the document uses, by identifier.
    styles: Vec<String>,
    /// How far the bulleted and the numbered list have counted.
    counts: [u32; 2],
}

impl Writer {
    /// The index of a colour in the table, adding it if it is not there.
    fn colour_index(&mut self, hex: &str) -> usize {
        let hex = hex.to_uppercase();
        if let Some(found) = self.colours.iter().position(|held| *held == hex) {
            return found + 1;
        }
        self.colours.push(hex);
        self.colours.len()
    }

    /// Everything before the text.
    fn header(&mut self) {
        self.out.push_str("{\\rtf1\\ansi\\ansicpg1252\\deff0\\deflang1033\\uc1\n");

        self.out.push_str("{\\fonttbl");
        for (index, font) in self.fonts.iter().enumerate() {
            self.out.push_str(&format!("{{\\f{index}\\fnil\\fcharset0 {font};}}"));
        }
        self.out.push_str("}\n");

        self.out.push_str("{\\colortbl ;");
        for hex in &self.colours {
            let (red, green, blue) = rgb(hex);
            self.out.push_str(&format!("\\red{red}\\green{green}\\blue{blue};"));
        }
        self.out.push_str("}\n");

        self.out.push_str("{\\stylesheet{\\s0\\snext0 Normal;}");
        for (index, style) in self.styles.iter().enumerate() {
            let name = style_name(style);
            self.out.push_str(&format!("{{\\s{}\\sbasedon0\\snext0 {name};}}", index + 1));
        }
        self.out.push_str("}\n");

        // The two lists a document here can be in: bullets, and numbers.
        self.out.push_str(
            "{\\*\\listtable{\\list\\listtemplateid1{\\listlevel\\levelnfc23\\levelnfcn23\\leveljc0\\levelfollow0\\levelstartat1\\levelindent0{\\leveltext\\'01\\u8226 ?;}{\\levelnumbers;}\\f0\\fi-360\\li720}\\listid1}",
        );
        self.out.push_str(
            "{\\list\\listtemplateid2{\\listlevel\\levelnfc0\\levelnfcn0\\leveljc0\\levelfollow0\\levelstartat1\\levelindent0{\\leveltext\\'02\\'00.;}{\\levelnumbers\\'01;}\\fi-360\\li720}\\listid2}}\n",
        );
        self.out.push_str(
            "{\\*\\listoverridetable{\\listoverride\\listid1\\listoverridecount0\\ls1}{\\listoverride\\listid2\\listoverridecount0\\ls2}}\n",
        );
        self.out.push_str("\\viewkind4\n");
    }

    /// One paragraph: its formatting, then its runs, then `\par` — unless it
    /// is in a table, where `\cell` ends it instead.
    fn paragraph(
        &mut self,
        document: &Document,
        paragraph: &Paragraph,
        links: &[(usize, usize, String)],
        in_table: bool,
    ) {
        let properties = &paragraph.properties;
        // The list's own text first, in a group of its own, as Word writes
        // it: a reader that does not know the list still shows the bullet.
        if let Some(numbering) = properties.numbering {
            let bulleted = numbering.id == wp_docx::BULLET_LIST;
            let slot = usize::from(!bulleted);
            self.counts[slot] += 1;
            let label =
                if bulleted { "\\'b7".to_owned() } else { format!("{}.", self.counts[slot]) };
            self.out.push_str(&format!("{{\\listtext\\pard\\plain {label}\\tab}}"));
        }
        self.out.push_str("\\pard\\plain");
        if in_table {
            self.out.push_str("\\intbl");
        }
        if let Some(style) = &properties.style {
            if let Some(index) = self.styles.iter().position(|held| held == style) {
                self.out.push_str(&format!("\\s{}", index + 1));
            }
        }
        match properties.alignment {
            Some(Alignment::Center) => self.out.push_str("\\qc"),
            Some(Alignment::End) => self.out.push_str("\\qr"),
            Some(Alignment::Both) => self.out.push_str("\\qj"),
            Some(Alignment::Start) => self.out.push_str("\\ql"),
            None => {}
        }
        if let Some(numbering) = properties.numbering {
            let ls = if numbering.id == wp_docx::BULLET_LIST { 1 } else { 2 };
            self.out.push_str(&format!("\\ls{ls}\\ilvl{}", numbering.level));
            if properties.indent_start.is_none() {
                self.out.push_str("\\fi-360\\li720");
            }
        }
        if let Some(indent) = properties.indent_start {
            self.out.push_str(&format!("\\li{indent}"));
        }
        if let Some(indent) = properties.indent_end {
            self.out.push_str(&format!("\\ri{indent}"));
        }
        if let Some(indent) = properties.indent_first_line {
            self.out.push_str(&format!("\\fi{indent}"));
        }
        if let Some(space) = properties.space_before {
            self.out.push_str(&format!("\\sb{space}"));
        }
        if let Some(space) = properties.space_after {
            self.out.push_str(&format!("\\sa{space}"));
        }
        if let Some(spacing) = properties.line_spacing {
            match spacing.rule {
                LineRule::Auto => self.out.push_str(&format!("\\sl{}\\slmult1", spacing.value)),
                LineRule::AtLeast => self.out.push_str(&format!("\\sl{}\\slmult0", spacing.value)),
                LineRule::Exact => self.out.push_str(&format!("\\sl-{}\\slmult0", spacing.value)),
            }
        }
        if properties.keep_next == Some(true) {
            self.out.push_str("\\keepn");
        }
        if properties.keep_lines == Some(true) {
            self.out.push_str("\\keep");
        }
        if properties.page_break_before == Some(true) {
            self.out.push_str("\\pagebb");
        }
        if properties.widow_control == Some(true) {
            self.out.push_str("\\widctlpar");
        }
        if let Some(level) = properties.outline_level {
            self.out.push_str(&format!("\\outlinelevel{level}"));
        }
        for stop in &properties.tab_stops {
            match stop.alignment {
                TabAlignment::Center => self.out.push_str("\\tqc"),
                TabAlignment::End => self.out.push_str("\\tqr"),
                TabAlignment::Decimal => self.out.push_str("\\tqdec"),
                TabAlignment::Start | TabAlignment::Bar | TabAlignment::Clear => {}
            }
            match stop.leader {
                TabLeader::Dot => self.out.push_str("\\tldot"),
                TabLeader::Hyphen => self.out.push_str("\\tlhyph"),
                TabLeader::Underscore => self.out.push_str("\\tlul"),
                TabLeader::MiddleDot => self.out.push_str("\\tlmdot"),
                TabLeader::None => {}
            }
            self.out.push_str(&format!("\\tx{}", stop.position));
        }
        self.out.push(' ');

        // The runs, with the links wrapped as fields round the runs they
        // cover — split where a link begins or ends inside one.
        let mut offset = 0;
        let mut open_link: Option<usize> = None;
        for run in &paragraph.runs {
            for piece in split_at_links(run, offset, links) {
                let (start, end) = (offset, offset + piece.length);
                let link = links.iter().position(|(from, to, _)| start >= *from && end <= *to);
                if link != open_link {
                    if open_link.is_some() {
                        self.out.push_str("}}}");
                    }
                    if let Some(index) = link {
                        let address = escape(&links[index].2);
                        self.out.push_str(&format!(
                            "{{\\field{{\\*\\fldinst{{HYPERLINK \"{address}\"}}}}{{\\fldrslt{{"
                        ));
                    }
                    open_link = link;
                }
                self.run(document, &piece.run);
                offset = end;
            }
        }
        if open_link.is_some() {
            self.out.push_str("}}}");
        }
        if !in_table {
            self.out.push_str("\\par\n");
        }
    }

    /// One run: a group with its formatting and its text.
    fn run(&mut self, document: &Document, run: &Run) {
        let properties = &run.properties;
        self.out.push('{');
        let before = self.out.len();
        self.character_properties(properties);
        // The space that ends the last control word is part of it, not of
        // the text; with no control word there is nothing to end.
        if self.out.len() > before {
            self.out.push(' ');
        }
        for content in &run.content {
            match content {
                RunContent::Text(text) => self.out.push_str(&escape(text)),
                RunContent::Tab | RunContent::PositionTab(_) => self.out.push_str("\\tab "),
                RunContent::Break(kind) => match kind {
                    wp_docx::model::BreakKind::Line => self.out.push_str("\\line "),
                    wp_docx::model::BreakKind::Page => self.out.push_str("\\page "),
                    wp_docx::model::BreakKind::Column => self.out.push_str("\\column "),
                },
                RunContent::Picture(picture) => self.picture(document, picture),
                _ => {}
            }
        }
        self.out.push('}');
    }

    /// The character formatting of a run, as control words.
    fn character_properties(&mut self, properties: &RunProperties) {
        if properties.bold == Some(true) {
            self.out.push_str("\\b");
        }
        if properties.italic == Some(true) {
            self.out.push_str("\\i");
        }
        match properties.underline {
            Some(Underline::Single) => self.out.push_str("\\ul"),
            Some(Underline::Double) => self.out.push_str("\\uldb"),
            Some(Underline::Thick) => self.out.push_str("\\ulth"),
            Some(Underline::Dotted) => self.out.push_str("\\uld"),
            Some(Underline::Dashed) => self.out.push_str("\\uldash"),
            Some(Underline::Wave) => self.out.push_str("\\ulwave"),
            Some(Underline::Other(_)) => self.out.push_str("\\ul"),
            Some(Underline::None) | None => {}
        }
        if properties.strike == Some(true) {
            self.out.push_str("\\strike");
        }
        if properties.double_strike == Some(true) {
            self.out.push_str("\\striked1");
        }
        if let Some(size) = properties.size_half_points {
            self.out.push_str(&format!("\\fs{size}"));
        }
        if let Some(font) = &properties.font {
            if let Some(index) = self.fonts.iter().position(|held| held.eq_ignore_ascii_case(font))
            {
                self.out.push_str(&format!("\\f{index}"));
            }
        }
        if let Some(colour) = &properties.color {
            let index = self.colour_index(colour);
            self.out.push_str(&format!("\\cf{index}"));
        }
        if let Some(hex) = properties.highlight.as_deref().and_then(highlight_hex) {
            let index = self.colour_index(hex);
            self.out.push_str(&format!("\\highlight{index}"));
        }
        match properties.vertical_align {
            Some(VerticalAlignment::Superscript) => self.out.push_str("\\super"),
            Some(VerticalAlignment::Subscript) => self.out.push_str("\\sub"),
            Some(VerticalAlignment::Baseline) | None => {}
        }
        if properties.caps == Some(true) {
            self.out.push_str("\\caps");
        }
        if properties.small_caps == Some(true) {
            self.out.push_str("\\scaps");
        }
        if properties.hidden == Some(true) {
            self.out.push_str("\\v");
        }
    }

    /// A picture, as the hex of its bytes.
    fn picture(&mut self, document: &Document, picture: &wp_docx::model::Picture) {
        let Some(bytes) = document.embedded_part(&picture.relationship) else { return };
        let blip = match wp_image::Format::detect(bytes) {
            Some(wp_image::Format::Png) => "\\pngblip",
            Some(wp_image::Format::Jpeg) => "\\jpegblip",
            // The other formats have no place in an RTF picture that every
            // reader knows; they are left out rather than written wrongly.
            _ => return,
        };
        let (width, height) = wp_image::decode(bytes)
            .map(|image| (image.width as i64, image.height as i64))
            .unwrap_or((picture.width_emu / 9525, picture.height_emu / 9525));
        self.out.push_str(&format!(
            "{{\\pict{blip}\\picw{width}\\pich{height}\\picwgoal{}\\pichgoal{}\n",
            picture.width_emu / 635,
            picture.height_emu / 635
        ));
        for (index, byte) in bytes.iter().enumerate() {
            if index > 0 && index % 64 == 0 {
                self.out.push('\n');
            }
            self.out.push_str(&format!("{byte:02x}"));
        }
        self.out.push_str("}\n");
    }
}

/// A run, or part of one, and how many bytes of the paragraph's text it is.
struct Piece {
    run: Run,
    length: usize,
}

/// Cuts a run where a link begins or ends inside it, so that each piece is
/// wholly in a link or wholly out of one.
fn split_at_links(run: &Run, offset: usize, links: &[(usize, usize, String)]) -> Vec<Piece> {
    let mut pieces = Vec::new();
    let mut at = offset;
    for content in &run.content {
        match content {
            RunContent::Text(text) => {
                // Every boundary inside the text, in order.
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
                // A tab and a break are one character of the text; so is a
                // drawing, which the document counts as one.
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

/// Text as RTF writes it: the three special characters escaped, the
/// characters that have control words of their own written as those, the
/// Western code page's characters as hex, and everything else as `\u`.
fn escape(text: &str) -> String {
    let western = Encoding::CodePage(1252);
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '\\' => out.push_str("\\\\"),
            '{' => out.push_str("\\{"),
            '}' => out.push_str("\\}"),
            '\u{00A0}' => out.push_str("\\~"),
            '\u{2014}' => out.push_str("\\emdash "),
            '\u{2013}' => out.push_str("\\endash "),
            '\u{2018}' => out.push_str("\\lquote "),
            '\u{2019}' => out.push_str("\\rquote "),
            '\u{201C}' => out.push_str("\\ldblquote "),
            '\u{201D}' => out.push_str("\\rdblquote "),
            '\u{2022}' => out.push_str("\\bullet "),
            '\u{00AD}' => out.push_str("\\-"),
            '\u{2011}' => out.push_str("\\_"),
            '\t' => out.push_str("\\tab "),
            '\n' => out.push_str("\\line "),
            c if (c as u32) < 0x80 => out.push(c),
            c => {
                let (bytes, lost) = western.encode(&c.to_string(), false);
                if lost == 0 && bytes.len() == 1 {
                    out.push_str(&format!("\\'{:02x}", bytes[0]));
                } else {
                    // Signed sixteen bits, and a question mark for readers
                    // that cannot read it; a character past the plane is two.
                    let mut units = [0u16; 2];
                    for unit in c.encode_utf16(&mut units) {
                        let signed = *unit as i16;
                        out.push_str(&format!("\\u{signed}?"));
                    }
                }
            }
        }
    }
    out
}

/// The three parts of a six-digit colour.
fn rgb(hex: &str) -> (u8, u8, u8) {
    let value = u32::from_str_radix(hex, 16).unwrap_or(0);
    ((value >> 16) as u8, (value >> 8) as u8, value as u8)
}

/// The name of a style, from its identifier: "Heading1" is "heading 1".
fn style_name(id: &str) -> String {
    if let Some(level) = id.strip_prefix("Heading") {
        if level.chars().all(|c| c.is_ascii_digit()) && !level.is_empty() {
            return format!("heading {level}");
        }
    }
    id.to_owned()
}

/// The colour of one of Word's highlight names.
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
