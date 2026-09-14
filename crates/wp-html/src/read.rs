//! Reading a page into the document model.
//!
//! # What a page from Word looks like
//!
//! A head with a hundred lines of `<style>` — a class for every paragraph
//! style, an `@list` rule for every list level — and a body where every
//! paragraph is `<p class=MsoNormal style='...'>`, every run is a `<span
//! style='...'>`, every list item carries `mso-list:l0 level1 lfo1` and its
//! bullet inside `<![if !supportLists]>`, every picture is a VML shape in a
//! conditional comment with an `<img>` after it for everyone else, and the
//! whole thing is wrapped in `<o:p>` and `<w:...>` tags nothing else
//! understands. The formatting is in three places at once: the class rule,
//! the style attribute, and the tag. This reader folds the three together
//! in that order and reads the page anybody else wrote the same way,
//! because a page from anybody else is the same thing with less of it.
//!
//! # How it is shaped
//!
//! A stack of open elements, each carrying the character formatting in
//! force inside it — inherited from the one outside and changed by its own
//! rules — and a paragraph being built. A block tag begins a paragraph, a
//! table tag begins a table, and text goes into whatever paragraph is open,
//! with its whitespace folded as a browser folds it.

use wp_docx::model::{
    Alignment, Block, Body, BreakKind, LineRule, LineSpacing, NumberingReference, Paragraph,
    ParagraphProperties, Run, RunContent, RunProperties, Table, TableCell, TableRow, Underline,
    VerticalAlignment,
};

use crate::css::{self, Sheet};
use crate::tokens::{Token, Tokenizer};

/// The character that stands where a picture goes in the text, until the
/// picture is put in.
pub const PICTURE_MARK: char = '\u{FFFC}';

/// What was read.
#[derive(Debug, Default)]
pub struct Reading {
    pub body: Body,
    /// The pictures, each with where it goes — the paragraph, counted
    /// through the whole document, and the byte offset of its mark — and
    /// where its bytes are.
    pub pictures: Vec<PictureFound>,
    /// The links, each with the paragraph and the range of text it covers.
    pub links: Vec<LinkFound>,
    /// What the page called itself.
    pub title: Option<String>,
}

#[derive(Debug)]
pub struct PictureFound {
    pub paragraph: usize,
    pub offset: usize,
    /// The `src` as written: a path beside the page, an address, or a
    /// `data:` URI holding the bytes.
    pub source: String,
    /// The size the page asks for, where it asks.
    pub width_emu: Option<i64>,
    pub height_emu: Option<i64>,
}

#[derive(Debug)]
pub struct LinkFound {
    pub paragraph: usize,
    pub start: usize,
    pub end: usize,
    pub address: String,
}

/// One open element and the formatting in force inside it.
#[derive(Clone, Debug)]
struct Frame {
    tag: String,
    chars: RunProperties,
    /// Where a link began, for an `<a>`.
    link: Option<(String, usize, usize)>,
    /// Whether whitespace is kept as written, inside `<pre>`.
    preformatted: bool,
}

/// A list being read: what kind, and how deep.
#[derive(Clone, Copy, Debug)]
struct ListFrame {
    bullet: bool,
    level: u8,
}

struct Reader {
    sheet: Sheet,
    frames: Vec<Frame>,
    lists: Vec<ListFrame>,
    /// The paragraph being built, if one is, and whether it is kept when
    /// nothing goes into it: a `<p>` is a paragraph however empty, a `<div>`
    /// round other things is not.
    paragraph: Option<(ParagraphProperties, Vec<Run>)>,
    keep_empty: bool,
    /// The text of the current run, gathered until the formatting changes.
    text: String,
    text_chars: RunProperties,
    /// How many bytes of text the current paragraph holds so far.
    paragraph_length: usize,
    paragraphs_done: usize,
    /// Whether the last thing put in the paragraph was a space, so that a
    /// run of whitespace folds to one.
    after_space: bool,
    body: Body,
    /// The table being built, and the row and cell in it.
    table: Option<Table>,
    row: Option<Vec<TableCell>>,
    cell: Option<(Vec<Block>, Option<i32>)>,
    /// How deep inside conditionals that hide their contents.
    hidden_depth: usize,
    conditional_depth: usize,
    /// Whether the head or a title is being read.
    in_title: bool,
    title: String,
    pictures: Vec<PictureFound>,
    links: Vec<LinkFound>,
}

/// Reads a page into a body, with the pictures and links beside it.
#[must_use]
pub fn read(html: &str) -> Reading {
    let mut reader = Reader {
        sheet: Sheet::default(),
        frames: vec![Frame {
            tag: "html".to_owned(),
            chars: RunProperties::default(),
            link: None,
            preformatted: false,
        }],
        lists: Vec::new(),
        paragraph: None,
        keep_empty: false,
        text: String::new(),
        text_chars: RunProperties::default(),
        paragraph_length: 0,
        paragraphs_done: 0,
        after_space: true,
        body: Body::default(),
        table: None,
        row: None,
        cell: None,
        hidden_depth: 0,
        conditional_depth: 0,
        in_title: false,
        title: String::new(),
        pictures: Vec::new(),
        links: Vec::new(),
    };
    let mut tokenizer = Tokenizer::new(html);
    while let Some(token) = tokenizer.next_token() {
        match token {
            Token::Open { name, attributes, self_closing } => {
                reader.open(&name, &attributes);
                if self_closing || is_void(&name) {
                    reader.close(&name);
                }
            }
            Token::Close(name) => reader.close(&name),
            Token::Text(text) => reader.text(&text),
            Token::Style(text) => reader.sheet.read(&text),
            Token::Comment => {}
            Token::ConditionalOpen(condition) => {
                reader.conditional_depth += 1;
                // What Word shows only to readers that do not know lists,
                // footnotes or comments is the fallback for what it has
                // already written another way.
                if matches!(
                    condition.as_str(),
                    "!supportLists"
                        | "!supportFootnotes"
                        | "!supportAnnotations"
                        | "!supportFields"
                ) {
                    reader.hidden_depth = reader.conditional_depth;
                }
            }
            Token::ConditionalClose => {
                if reader.hidden_depth == reader.conditional_depth {
                    reader.hidden_depth = 0;
                }
                reader.conditional_depth = reader.conditional_depth.saturating_sub(1);
            }
        }
    }
    reader.finish()
}

/// Tags that never hold anything and are never closed.
fn is_void(name: &str) -> bool {
    matches!(name, "br" | "img" | "hr" | "meta" | "link" | "input" | "col" | "area" | "base")
}

/// Tags that begin a paragraph.
fn is_block(name: &str) -> bool {
    matches!(
        name,
        "p" | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "div"
            | "li"
            | "pre"
            | "blockquote"
            | "center"
            | "address"
            | "dt"
            | "dd"
            | "section"
            | "article"
            | "header"
            | "footer"
            | "main"
            | "aside"
            | "nav"
            | "figure"
            | "figcaption"
    )
}

impl Reader {
    fn top(&self) -> &Frame {
        self.frames.last().expect("the page")
    }

    fn hidden(&self) -> bool {
        self.hidden_depth > 0
    }

    // --- Tags ------------------------------------------------------------------

    fn open(&mut self, name: &str, attributes: &[(String, String)]) {
        let attribute = |wanted: &str| {
            attributes.iter().find(|(held, _)| held == wanted).map(|(_, value)| value.as_str())
        };
        let classes: Vec<String> =
            attribute("class").unwrap_or("").split_whitespace().map(str::to_owned).collect();
        let mut declarations = self.sheet.declarations_for(name, &classes);
        if let Some(style) = attribute("style") {
            declarations.extend(css::parse_declarations(style));
        }

        match name {
            "title" => {
                self.in_title = true;
                return;
            }
            "head" | "script" | "xml" => {}
            "table" => {
                self.end_paragraph();
                if self.table.is_some() {
                    // A table inside a table: its cells' text joins the cell
                    // it is in, one paragraph each. See the roadmap.
                } else {
                    self.table = Some(Table::default());
                }
            }
            "tr" => {
                if self.table.is_some() && self.cell.is_none() {
                    self.end_row();
                    self.row = Some(Vec::new());
                }
            }
            "td" | "th" => {
                if self.table.is_some() && self.cell.is_none() {
                    self.end_paragraph();
                    if self.row.is_none() {
                        self.row = Some(Vec::new());
                    }
                    let width = declarations
                        .iter()
                        .rev()
                        .find(|(name, _)| name == "width")
                        .and_then(|(_, value)| css::twips(value))
                        .or_else(|| attribute("width").and_then(css::twips));
                    self.cell = Some((Vec::new(), width));
                }
            }
            "ul" | "ol" | "menu" => {
                self.end_paragraph();
                let level = self.lists.len().min(8) as u8;
                self.lists.push(ListFrame { bullet: name != "ol", level });
            }
            "br" => {
                let page = declarations
                    .iter()
                    .any(|(name, value)| name == "page-break-before" && value == "always");
                self.content(RunContent::Break(if page {
                    BreakKind::Page
                } else {
                    BreakKind::Line
                }));
                return;
            }
            "img" => {
                if let Some(source) = attribute("src") {
                    self.picture(source, attributes, &declarations);
                }
                return;
            }
            _ => {}
        }

        if is_block(name) {
            self.begin_paragraph(name, &classes, &declarations);
        }

        // The character formatting inside this element: what was in force
        // outside it, changed by the tag and by its rules.
        let mut chars = self.top().chars.clone();
        match name {
            "b" | "strong" => chars.bold = Some(true),
            "i" | "em" | "cite" | "var" => chars.italic = Some(true),
            "u" | "ins" => chars.underline = Some(Underline::Single),
            "s" | "strike" | "del" => chars.strike = Some(true),
            "sup" => chars.vertical_align = Some(VerticalAlignment::Superscript),
            "sub" => chars.vertical_align = Some(VerticalAlignment::Subscript),
            "code" | "tt" | "kbd" | "samp" | "pre" => chars.font = Some("Courier New".to_owned()),
            "h1" => chars.size_half_points = chars.size_half_points.or(Some(32)),
            "font" => {
                if let Some(face) = attribute("face").and_then(css::font_family) {
                    chars.font = Some(face);
                }
                if let Some(colour) = attribute("color").and_then(css::colour) {
                    chars.color = Some(colour);
                }
                if let Some(size) = attribute("size").and_then(|s| s.parse::<i32>().ok()) {
                    chars.size_half_points = Some(match size {
                        1 => 16,
                        2 => 20,
                        3 => 24,
                        4 => 28,
                        5 => 36,
                        6 => 48,
                        _ => 72,
                    });
                }
            }
            _ => {}
        }
        apply_character_declarations(&mut chars, &declarations);

        let link = if name == "a" {
            attribute("href")
                .filter(|href| !href.is_empty())
                .map(|href| (href.to_owned(), self.paragraphs_done, self.paragraph_length))
        } else {
            None
        };
        let preformatted = self.top().preformatted || name == "pre";
        self.frames.push(Frame { tag: name.to_owned(), chars, link, preformatted });
    }

    fn close(&mut self, name: &str) {
        if name == "title" {
            self.in_title = false;
            return;
        }
        // The nearest open element of this name closes, and everything
        // opened inside it that was never closed closes with it — which is
        // how a browser reads `<b>bold <i>both</b>`.
        let Some(depth) = self.frames.iter().rposition(|frame| frame.tag == name) else {
            match name {
                "tr" => self.end_row(),
                "td" | "th" => self.end_cell(),
                "table" => self.end_table(),
                _ => {}
            }
            return;
        };
        while self.frames.len() > depth {
            let frame = self.frames.pop().expect("a frame");
            self.flush_run();
            if let Some((address, paragraph, start)) = frame.link {
                if paragraph == self.paragraphs_done && self.paragraph_length > start {
                    self.links.push(LinkFound {
                        paragraph,
                        start,
                        end: self.paragraph_length,
                        address,
                    });
                }
            }
            if is_block(&frame.tag) {
                self.end_paragraph();
            }
            match frame.tag.as_str() {
                "td" | "th" => self.end_cell(),
                "tr" => self.end_row(),
                "table" => self.end_table(),
                "ul" | "ol" | "menu" => {
                    self.lists.pop();
                }
                _ => {}
            }
        }
    }

    // --- Paragraphs ------------------------------------------------------------

    fn begin_paragraph(
        &mut self,
        name: &str,
        classes: &[String],
        declarations: &[(String, String)],
    ) {
        self.end_paragraph();
        let style = match name {
            "h1" => Some("Heading1".to_owned()),
            "h2" => Some("Heading2".to_owned()),
            "h3" => Some("Heading3".to_owned()),
            "h4" => Some("Heading4".to_owned()),
            "h5" => Some("Heading5".to_owned()),
            "h6" => Some("Heading6".to_owned()),
            _ => None,
        };
        let mut properties = ParagraphProperties { style, ..ParagraphProperties::default() };
        if classes.iter().any(|class| class == "MsoTitle") {
            properties.style = Some("Title".to_owned());
        }
        if name == "center" {
            properties.alignment = Some(Alignment::Center);
        }
        if let Some(list) = self.lists.last() {
            if name == "li" {
                properties.numbering = Some(NumberingReference {
                    id: if list.bullet { wp_docx::BULLET_LIST } else { wp_docx::NUMBERED_LIST },
                    level: list.level,
                });
            }
        }
        apply_paragraph_declarations(&mut properties, declarations, &self.sheet);
        self.paragraph = Some((properties, Vec::new()));
        self.keep_empty = !matches!(
            name,
            "div"
                | "section"
                | "article"
                | "header"
                | "footer"
                | "main"
                | "aside"
                | "nav"
                | "figure"
        );
        self.paragraph_length = 0;
        self.after_space = true;
    }

    /// The paragraph a piece of text goes into, begun if there is none.
    fn ensure_paragraph(&mut self) {
        if self.paragraph.is_none() {
            self.paragraph = Some((ParagraphProperties::default(), Vec::new()));
            self.keep_empty = false;
            self.paragraph_length = 0;
            self.after_space = true;
        }
    }

    fn end_paragraph(&mut self) {
        self.flush_run();
        let Some((properties, mut runs)) = self.paragraph.take() else { return };
        // Trailing whitespace is not part of a paragraph.
        if let Some(last) = runs.last_mut() {
            if let Some(RunContent::Text(text)) = last.content.last_mut() {
                let trimmed = text.trim_end_matches(' ').len();
                text.truncate(trimmed);
            }
            if last.content.iter().all(|c| matches!(c, RunContent::Text(t) if t.is_empty())) {
                runs.pop();
            }
        }
        self.paragraph_length = 0;
        self.after_space = true;
        if runs.is_empty() && !self.keep_empty {
            return;
        }
        let paragraph = Paragraph { properties, runs };
        self.paragraphs_done += 1;
        if let Some((blocks, _)) = &mut self.cell {
            blocks.push(Block::Paragraph(paragraph));
        } else {
            self.end_table();
            self.body.blocks.push(Block::Paragraph(paragraph));
        }
    }

    // --- Tables ----------------------------------------------------------------

    fn end_cell(&mut self) {
        self.end_paragraph();
        let Some((mut blocks, width)) = self.cell.take() else { return };
        if blocks.is_empty() {
            blocks.push(Block::Paragraph(Paragraph::default()));
            self.paragraphs_done += 1;
        }
        let cell = TableCell { blocks, width, ..TableCell::default() };
        self.row.get_or_insert_with(Vec::new).push(cell);
    }

    fn end_row(&mut self) {
        self.end_cell();
        let Some(cells) = self.row.take() else { return };
        if cells.is_empty() {
            return;
        }
        if let Some(table) = &mut self.table {
            if table.grid.is_empty() {
                table.grid = cells.iter().map(|cell| cell.width.unwrap_or(2880)).collect();
            }
            table.rows.push(TableRow { cells, ..TableRow::default() });
        }
    }

    fn end_table(&mut self) {
        self.end_row();
        if let Some(table) = self.table.take() {
            if !table.rows.is_empty() {
                self.body.blocks.push(Block::Table(Box::new(table)));
            }
        }
    }

    // --- Text ------------------------------------------------------------------

    fn text(&mut self, text: &str) {
        if self.in_title {
            self.title.push_str(text);
            return;
        }
        if self.hidden() || matches!(self.top().tag.as_str(), "head" | "script" | "xml") {
            return;
        }
        let preformatted = self.top().preformatted;
        let mut folded = String::with_capacity(text.len());
        let mut last_was_space = self.after_space;
        for character in text.chars() {
            if preformatted {
                if character == '\n' {
                    self.push_text(&folded);
                    folded.clear();
                    self.content(RunContent::Break(BreakKind::Line));
                } else {
                    folded.push(character);
                }
                continue;
            }
            // Whitespace folds to one space, and none at the start of a
            // paragraph, which is how a browser lays out what is written
            // across lines for readability.
            if character.is_whitespace() && character != '\u{00A0}' {
                if !last_was_space {
                    folded.push(' ');
                    last_was_space = true;
                }
                continue;
            }
            folded.push(character);
            last_was_space = false;
        }
        self.push_text(&folded);
    }

    /// Text into the current run, starting a new run where the formatting
    /// changed.
    fn push_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        // Nothing but a space, before anything: dropped — and whitespace
        // between blocks begins no paragraph of its own.
        if text == " " && (self.after_space || self.paragraph.is_none()) {
            return;
        }
        self.ensure_paragraph();
        let chars = self.top().chars.clone();
        if chars != self.text_chars {
            self.flush_run();
            self.text_chars = chars;
        }
        let text = if self.after_space { text.trim_start_matches(' ') } else { text };
        if text.is_empty() {
            return;
        }
        self.text.push_str(text);
        self.paragraph_length += text.len();
        self.after_space = text.ends_with(' ');
    }

    fn flush_run(&mut self) {
        if self.text.is_empty() {
            return;
        }
        let text = core::mem::take(&mut self.text);
        let Some((_, runs)) = &mut self.paragraph else { return };
        runs.push(Run {
            properties: self.text_chars.clone(),
            content: vec![RunContent::Text(text)],
            field: None,
            revision: None,
            format_change: None,
        });
    }

    fn content(&mut self, content: RunContent) {
        if self.hidden() {
            return;
        }
        self.ensure_paragraph();
        self.flush_run();
        let chars = self.top().chars.clone();
        let Some((_, runs)) = &mut self.paragraph else { return };
        runs.push(Run {
            properties: chars,
            content: vec![content],
            field: None,
            revision: None,
            format_change: None,
        });
        self.paragraph_length += 1;
        self.after_space = true;
    }

    fn picture(
        &mut self,
        source: &str,
        attributes: &[(String, String)],
        declarations: &[(String, String)],
    ) {
        if self.hidden() {
            return;
        }
        let attribute = |wanted: &str| {
            attributes.iter().find(|(held, _)| held == wanted).map(|(_, value)| value.as_str())
        };
        let declared = |wanted: &str| {
            declarations
                .iter()
                .rev()
                .find(|(name, _)| name == wanted)
                .map(|(_, value)| value.as_str())
        };
        let size = |name: &str| {
            declared(name)
                .and_then(css::twips)
                .or_else(|| attribute(name).and_then(css::twips))
                .map(|twips| i64::from(twips) * 635)
        };
        let (width_emu, height_emu) = (size("width"), size("height"));
        self.ensure_paragraph();
        self.flush_run();
        let offset = self.paragraph_length;
        self.pictures.push(PictureFound {
            paragraph: self.paragraphs_done,
            offset,
            source: source.to_owned(),
            width_emu,
            height_emu,
        });
        let chars = self.top().chars.clone();
        if let Some((_, runs)) = &mut self.paragraph {
            runs.push(Run {
                properties: chars,
                content: vec![RunContent::Text(PICTURE_MARK.to_string())],
                field: None,
                revision: None,
                format_change: None,
            });
        }
        self.paragraph_length += PICTURE_MARK.len_utf8();
        self.after_space = false;
    }

    fn finish(mut self) -> Reading {
        self.end_paragraph();
        self.end_table();
        if self.body.blocks.is_empty() {
            self.body.blocks.push(Block::Paragraph(Paragraph::default()));
        }
        let title = self.title.trim().to_owned();
        Reading {
            body: self.body,
            pictures: self.pictures,
            links: self.links,
            title: (!title.is_empty()).then_some(title),
        }
    }
}

/// What CSS says about the characters.
fn apply_character_declarations(chars: &mut RunProperties, declarations: &[(String, String)]) {
    for (name, value) in declarations {
        let value = value.trim();
        match name.as_str() {
            "font-weight" => {
                chars.bold = Some(
                    value == "bold"
                        || value == "bolder"
                        || value.parse::<u32>().is_ok_and(|w| w >= 600),
                );
            }
            "font-style" => chars.italic = Some(value == "italic" || value == "oblique"),
            "text-decoration" | "text-decoration-line" => {
                if value.contains("underline") {
                    chars.underline = Some(Underline::Single);
                }
                if value.contains("line-through") {
                    chars.strike = Some(true);
                }
                if value == "none" {
                    chars.underline = Some(Underline::None);
                    chars.strike = Some(false);
                }
            }
            "font-size" => {
                let half_points = match value {
                    "xx-small" => Some(14),
                    "x-small" => Some(16),
                    "small" => Some(20),
                    "medium" => Some(24),
                    "large" => Some(28),
                    "x-large" => Some(36),
                    "xx-large" => Some(48),
                    other => css::twips(other).map(|twips| (twips / 10).max(2) as u32),
                };
                if let Some(size) = half_points {
                    chars.size_half_points = Some(size);
                }
            }
            "font-family" => {
                if let Some(family) = css::font_family(value) {
                    // Word's bullet font is not the text's font.
                    if family != "Symbol" && family != "Wingdings" {
                        chars.font = Some(family);
                    }
                }
            }
            "font" => {
                // The shorthand Word uses for its bullets: a size and a family.
                if let Some((size, family)) = value.split_once(' ') {
                    if let Some(twips) = css::twips(size) {
                        chars.size_half_points = Some((twips / 10).max(2) as u32);
                    }
                    if let Some(family) = css::font_family(family) {
                        chars.font = Some(family);
                    }
                }
            }
            "color" => chars.color = css::colour(value),
            "background" | "background-color" => {
                chars.highlight = css::colour(value).and_then(|hex| highlight_name(&hex));
            }
            "vertical-align" => {
                chars.vertical_align = match value {
                    "super" => Some(VerticalAlignment::Superscript),
                    "sub" => Some(VerticalAlignment::Subscript),
                    "baseline" => Some(VerticalAlignment::Baseline),
                    _ => chars.vertical_align,
                };
            }
            "text-transform" => chars.caps = Some(value == "uppercase"),
            "font-variant" => chars.small_caps = Some(value == "small-caps"),
            "display" if value == "none" => chars.hidden = Some(true),
            _ => {}
        }
    }
}

/// What CSS says about the paragraph.
fn apply_paragraph_declarations(
    properties: &mut ParagraphProperties,
    declarations: &[(String, String)],
    sheet: &Sheet,
) {
    for (name, value) in declarations {
        let value = value.trim();
        match name.as_str() {
            "text-align" => {
                properties.alignment = match value {
                    "center" => Some(Alignment::Center),
                    "right" | "end" => Some(Alignment::End),
                    "justify" => Some(Alignment::Both),
                    "left" | "start" => Some(Alignment::Start),
                    _ => properties.alignment,
                };
            }
            "margin-left" => properties.indent_start = css::twips(value),
            "margin-right" => properties.indent_end = css::twips(value),
            "margin-top" => properties.space_before = css::twips(value),
            "margin-bottom" => properties.space_after = css::twips(value),
            "margin" => {
                let parts: Vec<Option<i32>> = value.split_whitespace().map(css::twips).collect();
                let (top, right, bottom, left) = match parts.as_slice() {
                    [all] => (*all, *all, *all, *all),
                    [vertical, horizontal] => (*vertical, *horizontal, *vertical, *horizontal),
                    [top, horizontal, bottom] => (*top, *horizontal, *bottom, *horizontal),
                    [top, right, bottom, left, ..] => (*top, *right, *bottom, *left),
                    [] => continue,
                };
                properties.space_before = top;
                properties.indent_end = right;
                properties.space_after = bottom;
                properties.indent_start = left;
            }
            "text-indent" => properties.indent_first_line = css::twips(value),
            "line-height" => {
                properties.line_spacing = if value == "normal" {
                    None
                } else if let Some(percent) = value.strip_suffix('%') {
                    percent.trim().parse::<f32>().ok().map(|percent| LineSpacing {
                        value: (percent * 2.4).round() as i32,
                        rule: LineRule::Auto,
                    })
                } else if let Ok(multiple) = value.parse::<f32>() {
                    Some(LineSpacing {
                        value: (multiple * 240.0).round() as i32,
                        rule: LineRule::Auto,
                    })
                } else {
                    css::twips(value)
                        .map(|twips| LineSpacing { value: twips, rule: LineRule::AtLeast })
                };
            }
            "mso-line-height-rule" => {
                if value == "exactly" {
                    if let Some(spacing) = &mut properties.line_spacing {
                        if spacing.rule == LineRule::AtLeast {
                            spacing.rule = LineRule::Exact;
                        }
                    }
                }
            }
            "page-break-before" => properties.page_break_before = Some(value == "always"),
            "page-break-after" => {}
            "mso-list" => {
                // `l0 level1 lfo1`: which list, how deep. Whether it is
                // bulleted is what the sheet's `@list` rule says.
                let mut pieces = value.split_whitespace();
                let list = pieces.next().unwrap_or("");
                let level = pieces
                    .find_map(|piece| piece.strip_prefix("level"))
                    .and_then(|n| n.parse::<u8>().ok())
                    .unwrap_or(1);
                let bullet = sheet.list_is_bulleted(list, level).unwrap_or(false);
                properties.numbering = Some(NumberingReference {
                    id: if bullet { wp_docx::BULLET_LIST } else { wp_docx::NUMBERED_LIST },
                    level: level.saturating_sub(1).min(8),
                });
                // Word's list paragraphs carry the hanging indent of the list
                // in their style; the list draws its own.
                properties.indent_start = None;
                properties.indent_first_line = None;
            }
            _ => {}
        }
    }
}

/// Word's sixteen highlight colours, by the colour a page gives.
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
