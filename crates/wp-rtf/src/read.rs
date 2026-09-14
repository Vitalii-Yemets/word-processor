//! Reading RTF into the document model.
//!
//! # How the reader is shaped
//!
//! RTF is a stream with a stack: a group opened with `{` inherits the state
//! of the group it is in — which font, which size, whether bold — and gives
//! it back at `}`. So the reader is a stack of states and one pass over the
//! tokens, where a control word changes the state at the top, text goes into
//! the paragraph being built with the state at the top, and a group's end
//! pops the stack.
//!
//! Some groups are not text at all but tables the rest of the file refers
//! to — the fonts, the colours, the styles, the lists — and some are things
//! this reader does not carry: the metadata, the theme, the revision marks.
//! A group is a *destination*, and which destination a group is decides what
//! its text means. A group beginning `\*` that names a destination this
//! reader does not know is skipped whole, which is what `\*` is for: Word
//! writes twenty such groups at the top of every file, and a reader that
//! stopped at the first unknown one would read nothing.
//!
//! # What comes out
//!
//! A [`Body`] of paragraphs and tables, with the pictures and links found
//! along the way, which need a document to be put into: see
//! [`crate::open`].

use wp_docx::model::{
    Alignment, Block, Body, BreakKind, LineRule, LineSpacing, NumberingReference, Paragraph,
    ParagraphProperties, Run, RunContent, RunProperties, TabAlignment, TabLeader, TabStop, Table,
    TableCell, TableRow, Underline, VerticalAlignment,
};
use wp_text::Encoding;

use crate::lexer::{Lexer, Token};

/// The character that stands where a picture goes in the text, until the
/// picture is put in.
pub const PICTURE_MARK: char = '\u{FFFC}';

/// What was read.
#[derive(Debug, Default)]
pub struct Reading {
    pub body: Body,
    /// The pictures, each with where it goes: the paragraph, counted through
    /// the whole document in order, and the byte offset in it of the mark
    /// that stands for it.
    pub pictures: Vec<PictureFound>,
    /// The links, each with the paragraph and the range of text it covers.
    pub links: Vec<LinkFound>,
}

#[derive(Debug)]
pub struct PictureFound {
    pub paragraph: usize,
    pub offset: usize,
    pub bytes: Vec<u8>,
    /// `png` or `jpeg`.
    pub extension: &'static str,
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

/// What a group is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Destination {
    /// The text of the document.
    Body,
    FontTable,
    ColourTable,
    StyleSheet,
    ListTable,
    ListOverrideTable,
    Picture,
    /// The instruction of a field: `HYPERLINK "..."`.
    FieldInstruction,
    /// Anything this reader does not carry.
    Skip,
}

/// What is inherited from group to group.
#[derive(Clone, Debug)]
struct State {
    destination: Destination,
    chars: RunProperties,
    paragraph: ParagraphProperties,
    /// Which font of the table is in force, for its code page.
    font: Option<i32>,
    /// How many characters follow a `\u` as its stand-in: `\uc`.
    uc: usize,
    /// How many of them are still to be skipped after the last `\u`.
    skip: usize,
    /// Whether the paragraph is in a table.
    in_table: bool,
    /// The tab stop being described: its alignment and leader, until `\tx`
    /// says where it is.
    tab: (TabAlignment, TabLeader),
}

impl Default for State {
    fn default() -> Self {
        Self {
            destination: Destination::Body,
            chars: RunProperties::default(),
            paragraph: ParagraphProperties::default(),
            font: None,
            uc: 1,
            skip: 0,
            in_table: false,
            tab: (TabAlignment::Start, TabLeader::None),
        }
    }
}

/// One font of the font table.
#[derive(Clone, Debug, Default)]
struct Font {
    name: String,
    /// The code page its charset names, where it names one.
    code_page: Option<u16>,
}

/// One list of the list table: its id and whether it is bulleted.
#[derive(Clone, Debug, Default)]
struct List {
    id: i32,
    bullet: bool,
    /// Whether the first level has said what kind it is.
    kind_known: bool,
    /// How deep the stack was when `\list` was read.
    depth: usize,
}

/// A picture being read.
#[derive(Debug, Default)]
struct PictureBeingRead {
    extension: Option<&'static str>,
    hex: Vec<u8>,
    width_twips: Option<i64>,
    height_twips: Option<i64>,
    width_pixels: Option<i64>,
    height_pixels: Option<i64>,
}

/// A field being read: its instruction, and where its result began.
#[derive(Debug, Default)]
struct FieldBeingRead {
    instruction: String,
    paragraph: usize,
    start: usize,
    /// How deep the stack was when `\field` was read: the field ends when
    /// its group does.
    depth: usize,
}

struct Reader {
    stack: Vec<State>,
    /// The document's code page, from `\ansicpg`.
    code_page: Encoding,
    fonts: Vec<(i32, Font)>,
    colours: Vec<String>,
    styles: Vec<(i32, String)>,
    lists: Vec<List>,
    /// `\ls` numbers to list ids.
    overrides: Vec<(i32, i32)>,
    /// What is being built.
    body: Body,
    runs: Vec<Run>,
    /// The text of the current run, gathered until the formatting changes.
    text: String,
    /// The bytes of the current run's text not yet decoded, in the code page
    /// in force when they arrived.
    pending_bytes: Vec<u8>,
    pending_page: Encoding,
    /// How many bytes of text the current paragraph holds so far, for the
    /// pictures and links to be placed by.
    paragraph_length: usize,
    /// How many paragraphs have been finished, through the whole document.
    paragraphs_done: usize,
    /// The table being built, if a paragraph has been in one.
    table: Option<Table>,
    row: Vec<TableCell>,
    cell_blocks: Vec<Block>,
    /// The right edges of the cells of the row being described, in twips.
    cell_edges: Vec<i32>,
    row_left: i32,
    /// The font and colour table entries being read.
    font_being_read: Option<(i32, Font)>,
    colour_being_read: (Option<u8>, Option<u8>, Option<u8>),
    style_being_read: Option<(i32, String)>,
    list_being_read: Option<List>,
    override_being_read: (Option<i32>, Option<i32>),
    picture: Option<PictureBeingRead>,
    field: Option<FieldBeingRead>,
    /// The first half of a character written as two ``, waiting for the
    /// second.
    high_surrogate: Option<u16>,
    /// The groups deep inside a skipped one, so its end is known.
    pictures: Vec<PictureFound>,
    links: Vec<LinkFound>,
}

/// Reads a file into a body, with the pictures and links beside it.
#[must_use]
pub fn read(bytes: &[u8]) -> Reading {
    let mut reader = Reader {
        stack: vec![State::default()],
        code_page: Encoding::CodePage(1252),
        fonts: Vec::new(),
        colours: Vec::new(),
        styles: Vec::new(),
        lists: Vec::new(),
        overrides: Vec::new(),
        body: Body::default(),
        runs: Vec::new(),
        text: String::new(),
        pending_bytes: Vec::new(),
        pending_page: Encoding::CodePage(1252),
        paragraph_length: 0,
        paragraphs_done: 0,
        table: None,
        row: Vec::new(),
        cell_blocks: Vec::new(),
        cell_edges: Vec::new(),
        row_left: 0,
        font_being_read: None,
        colour_being_read: (None, None, None),
        style_being_read: None,
        list_being_read: None,
        override_being_read: (None, None),
        picture: None,
        field: None,
        high_surrogate: None,
        pictures: Vec::new(),
        links: Vec::new(),
    };
    let mut lexer = Lexer::new(bytes);
    // A group beginning `\*` whose destination is not known is skipped: this
    // is whether the last token opened a group and the one before that was
    // the star.
    let mut just_opened = false;
    let mut starred = false;
    while let Some(token) = lexer.next_token() {
        match token {
            Token::Open => {
                let top = reader.top().clone();
                reader.stack.push(top);
                just_opened = true;
                starred = false;
                continue;
            }
            Token::Close => {
                reader.close_group();
            }
            Token::Star if just_opened => {
                starred = true;
                continue;
            }
            Token::Control(word, number) => {
                if starred && !is_known_starred(&word) {
                    reader.top_mut().destination = Destination::Skip;
                } else {
                    reader.control(&word, number, just_opened);
                }
            }
            Token::Hex(byte) => reader.byte(byte),
            Token::Byte(byte) => reader.byte(byte),
            Token::Character(character) => reader.character(character),
            Token::Star => {}
        }
        just_opened = false;
        starred = false;
    }
    reader.finish()
}

/// The `\*` destinations this reader carries. Anything else after a star is
/// skipped, which is what the star says may be done.
fn is_known_starred(word: &str) -> bool {
    matches!(word, "listtable" | "listoverridetable" | "shppict" | "fldinst")
}

impl Reader {
    fn top(&self) -> &State {
        self.stack.last().expect("the document group")
    }

    fn top_mut(&mut self) -> &mut State {
        self.stack.last_mut().expect("the document group")
    }

    /// The code page the text of the current font is in.
    fn page_in_force(&self) -> Encoding {
        let font = self.top().font;
        font.and_then(|wanted| self.fonts.iter().find(|(index, _)| *index == wanted))
            .and_then(|(_, font)| font.code_page)
            .and_then(|number| Encoding::code_page(u32::from(number)))
            .unwrap_or(self.code_page)
    }

    // --- Text ----------------------------------------------------------------

    /// One byte of text, in the code page in force.
    fn byte(&mut self, byte: u8) {
        let top = self.top_mut();
        if top.skip > 0 {
            // The stand-in for a `\u` that has already been read.
            top.skip -= 1;
            return;
        }
        match self.top().destination {
            Destination::Body | Destination::FieldInstruction => {
                let page = self.page_in_force();
                if page != self.pending_page {
                    self.flush_bytes();
                    self.pending_page = page;
                }
                self.pending_bytes.push(byte);
            }
            Destination::FontTable => {
                if byte == b';' {
                    if let Some((index, font)) = self.font_being_read.take() {
                        self.fonts.push((index, font));
                    }
                } else if let Some((_, font)) = &mut self.font_being_read {
                    font.name.push(byte as char);
                }
            }
            Destination::ColourTable => {
                if byte == b';' {
                    let (red, green, blue) = self.colour_being_read;
                    self.colours.push(match (red, green, blue) {
                        (Some(red), Some(green), Some(blue)) => {
                            format!("{red:02X}{green:02X}{blue:02X}")
                        }
                        // The first entry is usually empty: the automatic colour.
                        _ => String::from("auto"),
                    });
                    self.colour_being_read = (None, None, None);
                }
            }
            Destination::StyleSheet => {
                if byte == b';' {
                    if let Some((index, name)) = self.style_being_read.take() {
                        self.styles.push((index, name.trim().to_owned()));
                    }
                } else if let Some((_, name)) = &mut self.style_being_read {
                    name.push(byte as char);
                }
            }
            Destination::Picture => {
                if byte.is_ascii_hexdigit() {
                    if let Some(picture) = &mut self.picture {
                        picture.hex.push(byte);
                    }
                }
            }
            Destination::ListTable | Destination::ListOverrideTable | Destination::Skip => {}
        }
    }

    /// A character that arrived whole: from `\u`, or a control symbol.
    fn character(&mut self, character: char) {
        match self.top().destination {
            Destination::Body | Destination::FieldInstruction => {
                self.flush_bytes();
                self.text.push(character);
                if self.top().destination == Destination::Body {
                    self.paragraph_length += character.len_utf8();
                }
            }
            Destination::FontTable => {
                if let Some((_, font)) = &mut self.font_being_read {
                    font.name.push(character);
                }
            }
            Destination::StyleSheet => {
                if let Some((_, name)) = &mut self.style_being_read {
                    name.push(character);
                }
            }
            _ => {}
        }
    }

    /// Decodes the bytes gathered so far into the current text.
    fn flush_bytes(&mut self) {
        if self.pending_bytes.is_empty() {
            return;
        }
        let decoded = self.pending_page.decode(&self.pending_bytes);
        self.pending_bytes.clear();
        if self.top().destination == Destination::Body {
            self.paragraph_length += decoded.len();
        }
        self.text.push_str(&decoded);
    }

    /// Ends the current run, if it holds anything.
    fn flush_run(&mut self) {
        self.flush_bytes();
        if self.text.is_empty() {
            return;
        }
        let text = core::mem::take(&mut self.text);
        if self.top().destination == Destination::FieldInstruction {
            if let Some(field) = &mut self.field {
                field.instruction.push_str(&text);
            }
            return;
        }
        let properties = self.top().chars.clone();
        self.runs.push(Run {
            properties,
            content: vec![RunContent::Text(text)],
            field: None,
            revision: None,
            format_change: None,
        });
    }

    /// Puts something that is not text into the paragraph.
    fn content(&mut self, content: RunContent, length: usize) {
        if self.top().destination != Destination::Body {
            return;
        }
        self.flush_run();
        let properties = self.top().chars.clone();
        self.runs.push(Run {
            properties,
            content: vec![content],
            field: None,
            revision: None,
            format_change: None,
        });
        self.paragraph_length += length;
    }

    // --- Paragraphs and tables -----------------------------------------------

    /// Ends the paragraph being built and puts it where it belongs.
    fn end_paragraph(&mut self) {
        self.flush_run();
        let runs = core::mem::take(&mut self.runs);
        let properties = self.top().paragraph.clone();
        let paragraph = Paragraph { properties, runs };
        self.paragraph_length = 0;
        self.paragraphs_done += 1;

        if self.top().in_table {
            self.cell_blocks.push(Block::Paragraph(paragraph));
        } else {
            self.end_table();
            self.body.blocks.push(Block::Paragraph(paragraph));
        }
    }

    /// `\cell`: the paragraph ends and so does the cell.
    fn end_cell(&mut self) {
        self.top_mut().in_table = true;
        self.end_paragraph();
        let blocks = core::mem::take(&mut self.cell_blocks);
        let column = self.row.len();
        let left = if column == 0 {
            self.row_left
        } else {
            self.cell_edges.get(column - 1).copied().unwrap_or(self.row_left)
        };
        let width =
            self.cell_edges.get(column).map(|right| right - left).filter(|width| *width > 0);
        self.row.push(TableCell { blocks, width, ..TableCell::default() });
    }

    /// `\row`: the row ends.
    fn end_row(&mut self) {
        if !self.cell_blocks.is_empty() || !self.runs.is_empty() || !self.text.is_empty() {
            self.end_cell();
        }
        let cells = core::mem::take(&mut self.row);
        if cells.is_empty() {
            return;
        }
        let table = self.table.get_or_insert_with(Table::default);
        if table.grid.is_empty() {
            table.grid = cells.iter().map(|cell| cell.width.unwrap_or(2880)).collect();
        }
        table.rows.push(TableRow { cells, ..TableRow::default() });
    }

    /// The table ends, if one was being built: it goes into the body before
    /// whatever comes next.
    fn end_table(&mut self) {
        if let Some(table) = self.table.take() {
            self.body.blocks.push(Block::Table(Box::new(table)));
        }
    }

    // --- Control words ---------------------------------------------------------

    #[allow(clippy::too_many_lines, reason = "one arm per control word is the readable shape")]
    fn control(&mut self, word: &str, number: Option<i32>, just_opened: bool) {
        let n = number.unwrap_or(0);
        let on = number != Some(0);
        // A control word ends the stand-in of a `\u`, whatever \uc said.
        self.top_mut().skip = 0;

        // Destinations, which a group is for. Inside a skipped group nothing
        // is a destination: the whole of it goes.
        if just_opened && self.top().destination != Destination::Skip {
            let destination = match word {
                "fonttbl" => Some(Destination::FontTable),
                "colortbl" => Some(Destination::ColourTable),
                "stylesheet" => Some(Destination::StyleSheet),
                "listtable" => Some(Destination::ListTable),
                "listoverridetable" => Some(Destination::ListOverrideTable),
                "pict" => Some(Destination::Picture),
                "fldinst" => Some(Destination::FieldInstruction),
                // Everything this reader does not carry through.
                "info" | "header" | "footer" | "headerl" | "headerr" | "headerf" | "footerl"
                | "footerr" | "footerf" | "footnote" | "pntext" | "listtext" | "nonshppict"
                | "shp" | "shpinst" | "xe" | "tc" | "bkmkstart" | "bkmkend" | "ftnsep"
                | "ftnsepc" | "aftnsep" | "aftnsepc" | "themedata" | "colorschememapping"
                | "latentstyles" | "datastore" | "rsidtbl" | "generator" | "xmlnstbl"
                | "pnseclvl" | "revtbl" | "userprops" | "docvar" | "mmathPr" | "background"
                | "template" | "annotation" | "atnid" | "atnauthor" | "object" | "objdata"
                | "result" | "fldrslt" => {
                    if word == "fldrslt" || word == "result" {
                        // The result of a field is text like any other.
                        None
                    } else {
                        Some(Destination::Skip)
                    }
                }
                _ => None,
            };
            if let Some(destination) = destination {
                // The tables' text is not the document's.
                self.flush_run();
                self.top_mut().destination = destination;
                match destination {
                    Destination::Picture => self.picture = Some(PictureBeingRead::default()),
                    Destination::FieldInstruction => {
                        if let Some(field) = &mut self.field {
                            field.instruction.clear();
                        }
                    }
                    _ => {}
                }
                return;
            }
        }

        match self.top().destination {
            Destination::Skip => return,
            Destination::FontTable => {
                match word {
                    "f" => {
                        if let Some((index, font)) = self.font_being_read.take() {
                            if !font.name.is_empty() {
                                self.fonts.push((index, font));
                            }
                        }
                        self.font_being_read = Some((n, Font::default()));
                    }
                    "fcharset" => {
                        if let Some((_, font)) = &mut self.font_being_read {
                            font.code_page = charset_code_page(n);
                        }
                    }
                    _ => {}
                }
                return;
            }
            Destination::ColourTable => {
                match word {
                    "red" => self.colour_being_read.0 = u8::try_from(n).ok(),
                    "green" => self.colour_being_read.1 = u8::try_from(n).ok(),
                    "blue" => self.colour_being_read.2 = u8::try_from(n).ok(),
                    _ => {}
                }
                return;
            }
            Destination::StyleSheet => {
                if word == "s" || word == "ds" || word == "ts" {
                    self.style_being_read = Some((n, String::new()));
                } else if word == "cs" {
                    // A character style: not a paragraph style, so not one
                    // `\s` can name.
                    self.style_being_read = None;
                }
                return;
            }
            Destination::ListTable => {
                match word {
                    "list" => {
                        self.list_being_read =
                            Some(List { depth: self.stack.len(), ..List::default() });
                    }
                    "listid" => {
                        if let Some(list) = &mut self.list_being_read {
                            list.id = n;
                        }
                    }
                    // The first level says what kind of list it is: 23 is
                    // a bullet, the rest are numbers of one kind or another.
                    "levelnfc" | "levelnfcn" => {
                        if let Some(list) = &mut self.list_being_read {
                            if !list.kind_known {
                                list.bullet = n == 23;
                                list.kind_known = true;
                            }
                        }
                    }
                    _ => {}
                }
                return;
            }
            Destination::ListOverrideTable => {
                match word {
                    "listoverride" => self.override_being_read = (None, None),
                    "listid" => self.override_being_read.0 = Some(n),
                    "ls" => {
                        self.override_being_read.1 = Some(n);
                        if let (Some(id), Some(ls)) = self.override_being_read {
                            self.overrides.push((ls, id));
                        }
                    }
                    _ => {}
                }
                return;
            }
            Destination::Picture => {
                let Some(picture) = &mut self.picture else { return };
                match word {
                    "pngblip" => picture.extension = Some("png"),
                    "jpegblip" => picture.extension = Some("jpeg"),
                    "picw" => picture.width_pixels = Some(i64::from(n)),
                    "pich" => picture.height_pixels = Some(i64::from(n)),
                    "picwgoal" => picture.width_twips = Some(i64::from(n)),
                    "pichgoal" => picture.height_twips = Some(i64::from(n)),
                    _ => {}
                }
                return;
            }
            Destination::FieldInstruction => return,
            Destination::Body => {}
        }

        // The text of the document.
        match word {
            "ansicpg" => {
                if let Some(page) = u32::try_from(n).ok().and_then(Encoding::code_page) {
                    self.code_page = page;
                }
            }
            "uc" => self.top_mut().uc = usize::try_from(n).unwrap_or(1),
            "u" => {
                // Signed sixteen bits, as the format writes them; a surrogate
                // is half a character and waits for its other half.
                let value = if n < 0 { n + 65536 } else { n };
                let unit = u16::try_from(value).unwrap_or(0xFFFD);
                let character = match (self.high_surrogate.take(), unit) {
                    (Some(high), 0xDC00..=0xDFFF) => {
                        char::decode_utf16([high, unit]).next().and_then(Result::ok)
                    }
                    (_, 0xD800..=0xDBFF) => {
                        self.high_surrogate = Some(unit);
                        None
                    }
                    (_, unit) => char::from_u32(u32::from(unit)),
                };
                if let Some(character) = character {
                    self.character(character);
                }
                // The stand-in that follows is skipped by `byte`.
                let uc = self.top().uc;
                self.top_mut().skip = uc;
            }

            // Paragraphs.
            "par" => self.end_paragraph(),
            "pard" => {
                // Being in a table is a paragraph property too, and Word
                // writes `\intbl` again after every `\pard` in one.
                self.top_mut().paragraph = ParagraphProperties::default();
                self.top_mut().in_table = false;
            }
            "plain" => self.top_mut().chars = RunProperties::default(),
            "sect" => self.end_paragraph(),
            "line" => self.content(RunContent::Break(BreakKind::Line), 1),
            "page" => self.content(RunContent::Break(BreakKind::Page), 1),
            "tab" => self.content(RunContent::Tab, 1),
            "emdash" => self.character('\u{2014}'),
            "endash" => self.character('\u{2013}'),
            "lquote" => self.character('\u{2018}'),
            "rquote" => self.character('\u{2019}'),
            "ldblquote" => self.character('\u{201C}'),
            "rdblquote" => self.character('\u{201D}'),
            "bullet" => self.character('\u{2022}'),
            "emspace" => self.character('\u{2003}'),
            "enspace" => self.character('\u{2002}'),
            "qmspace" => self.character('\u{2005}'),
            "zwj" => self.character('\u{200D}'),
            "zwnj" => self.character('\u{200C}'),
            "ltrmark" => self.character('\u{200E}'),
            "rtlmark" => self.character('\u{200F}'),

            // Tables.
            "trowd" => {
                self.cell_edges.clear();
                self.row_left = 0;
            }
            "trleft" => self.row_left = n,
            "cellx" => self.cell_edges.push(n),
            "intbl" => self.top_mut().in_table = true,
            "cell" => self.end_cell(),
            "row" => self.end_row(),

            // Paragraph formatting.
            "ql" => self.top_mut().paragraph.alignment = Some(Alignment::Start),
            "qc" => self.top_mut().paragraph.alignment = Some(Alignment::Center),
            "qr" => self.top_mut().paragraph.alignment = Some(Alignment::End),
            "qj" => self.top_mut().paragraph.alignment = Some(Alignment::Both),
            "li" => self.top_mut().paragraph.indent_start = Some(n),
            "ri" => self.top_mut().paragraph.indent_end = Some(n),
            "fi" => self.top_mut().paragraph.indent_first_line = Some(n),
            "sb" => self.top_mut().paragraph.space_before = Some(n),
            "sa" => self.top_mut().paragraph.space_after = Some(n),
            "sl" => {
                let spacing = if n == 0 {
                    None
                } else if n < 0 {
                    Some(LineSpacing { value: -n, rule: LineRule::Exact })
                } else {
                    Some(LineSpacing { value: n, rule: LineRule::AtLeast })
                };
                self.top_mut().paragraph.line_spacing = spacing;
            }
            "slmult" => {
                if on {
                    if let Some(spacing) = &mut self.top_mut().paragraph.line_spacing {
                        spacing.rule = LineRule::Auto;
                    }
                }
            }
            "keepn" => self.top_mut().paragraph.keep_next = Some(on),
            "keep" => self.top_mut().paragraph.keep_lines = Some(on),
            "pagebb" => self.top_mut().paragraph.page_break_before = Some(on),
            "widctlpar" => self.top_mut().paragraph.widow_control = Some(true),
            "nowidctlpar" => self.top_mut().paragraph.widow_control = Some(false),
            "outlinelevel" => {
                self.top_mut().paragraph.outline_level = u8::try_from(n).ok().filter(|l| *l < 9);
            }
            "contextualspace" => self.top_mut().paragraph.contextual_spacing = Some(on),
            "s" => {
                let name =
                    self.styles.iter().find(|(index, _)| *index == n).map(|(_, name)| name.clone());
                self.top_mut().paragraph.style = name.and_then(|name| style_id(&name));
            }
            "ls" => {
                let id = self
                    .overrides
                    .iter()
                    .find(|(ls, _)| *ls == n)
                    .and_then(|(_, id)| self.lists.iter().find(|list| list.id == *id))
                    .map_or(wp_docx::NUMBERED_LIST, |list| {
                        if list.bullet {
                            wp_docx::BULLET_LIST
                        } else {
                            wp_docx::NUMBERED_LIST
                        }
                    });
                let level = self.top().paragraph.numbering.map_or(0, |reference| reference.level);
                self.top_mut().paragraph.numbering = Some(NumberingReference { id, level });
            }
            "ilvl" => {
                let level = u8::try_from(n).unwrap_or(0).min(8);
                match &mut self.top_mut().paragraph.numbering {
                    Some(reference) => reference.level = level,
                    none => *none = Some(NumberingReference { id: wp_docx::NUMBERED_LIST, level }),
                }
            }
            "tqc" => self.top_mut().tab.0 = TabAlignment::Center,
            "tqr" => self.top_mut().tab.0 = TabAlignment::End,
            "tqdec" => self.top_mut().tab.0 = TabAlignment::Decimal,
            "tldot" => self.top_mut().tab.1 = TabLeader::Dot,
            "tlhyph" => self.top_mut().tab.1 = TabLeader::Hyphen,
            "tlul" => self.top_mut().tab.1 = TabLeader::Underscore,
            "tlmdot" => self.top_mut().tab.1 = TabLeader::MiddleDot,
            "tx" => {
                let (alignment, leader) = self.top().tab;
                self.top_mut().paragraph.tab_stops.push(TabStop { position: n, alignment, leader });
                self.top_mut().tab = (TabAlignment::Start, TabLeader::None);
            }

            // Character formatting.
            "b" => self.set_chars(|chars| chars.bold = Some(on)),
            "i" => self.set_chars(|chars| chars.italic = Some(on)),
            "strike" => self.set_chars(|chars| chars.strike = Some(on)),
            "striked" => self.set_chars(|chars| chars.double_strike = Some(on)),
            "ul" => self.set_chars(|chars| {
                chars.underline = Some(if on { Underline::Single } else { Underline::None });
            }),
            "ulnone" => self.set_chars(|chars| chars.underline = Some(Underline::None)),
            "uldb" => self.set_chars(|chars| chars.underline = Some(Underline::Double)),
            "ulth" => self.set_chars(|chars| chars.underline = Some(Underline::Thick)),
            "uld" => self.set_chars(|chars| chars.underline = Some(Underline::Dotted)),
            "uldash" => self.set_chars(|chars| chars.underline = Some(Underline::Dashed)),
            "ulwave" | "ulw" => self.set_chars(|chars| chars.underline = Some(Underline::Wave)),
            "fs" => self.set_chars(|chars| chars.size_half_points = u32::try_from(n).ok()),
            "f" => {
                self.top_mut().font = Some(n);
                let name = self
                    .fonts
                    .iter()
                    .find(|(index, _)| *index == n)
                    .map(|(_, font)| font.name.clone());
                self.set_chars(|chars| chars.font = name);
            }
            "cf" => {
                let colour = self.colours.get(usize::try_from(n).unwrap_or(usize::MAX)).cloned();
                self.set_chars(|chars| chars.color = colour.filter(|colour| colour != "auto"));
            }
            "highlight" | "cb" => {
                let colour = self.colours.get(usize::try_from(n).unwrap_or(usize::MAX)).cloned();
                let name = colour.and_then(|colour| highlight_name(&colour));
                self.set_chars(|chars| chars.highlight = name);
            }
            "super" => {
                self.set_chars(|chars| chars.vertical_align = Some(VerticalAlignment::Superscript))
            }
            "sub" => {
                self.set_chars(|chars| chars.vertical_align = Some(VerticalAlignment::Subscript))
            }
            "nosupersub" => {
                self.set_chars(|chars| chars.vertical_align = Some(VerticalAlignment::Baseline))
            }
            "caps" => self.set_chars(|chars| chars.caps = Some(on)),
            "scaps" => self.set_chars(|chars| chars.small_caps = Some(on)),
            "v" => self.set_chars(|chars| chars.hidden = Some(on)),
            "lang" => {
                let tag = language_tag(n).map(str::to_owned);
                self.set_chars(|chars| chars.language = tag);
            }

            // Fields: the instruction says what it is, the result is the text.
            "field" => {
                self.flush_run();
                self.field = Some(FieldBeingRead {
                    instruction: String::new(),
                    paragraph: self.paragraphs_done,
                    start: self.paragraph_length,
                    depth: self.stack.len(),
                });
            }
            _ => {}
        }
    }

    /// Changes the character formatting, ending the run the old one made.
    fn set_chars(&mut self, change: impl FnOnce(&mut RunProperties)) {
        self.flush_run();
        change(&mut self.top_mut().chars);
    }

    /// A group closed: whatever it was building is finished.
    fn close_group(&mut self) {
        if self.stack.len() <= 1 {
            return;
        }
        let closing = self.top().destination;
        let parent = self.stack[self.stack.len() - 2].destination;
        match closing {
            Destination::Body | Destination::FieldInstruction => self.flush_run(),
            Destination::FontTable => {
                if let Some((index, font)) = self.font_being_read.take() {
                    if !font.name.is_empty() {
                        self.fonts.push((index, font));
                    }
                }
            }
            Destination::ListTable => {
                // Only the whole list, not one of its levels: the group
                // `\list` was read in is the one that ends it.
                let whole = self
                    .list_being_read
                    .as_ref()
                    .is_some_and(|list| self.stack.len() <= list.depth);
                if whole {
                    if let Some(list) = self.list_being_read.take() {
                        if list.id != 0 {
                            self.lists.push(list);
                        }
                    }
                }
            }
            Destination::Picture => {
                if parent != Destination::Picture {
                    self.finish_picture();
                }
            }
            _ => {}
        }
        self.stack.pop();

        // The end of the field's own group: what its instruction said decides
        // what its result was.
        if self.field.as_ref().is_some_and(|field| self.stack.len() < field.depth) {
            self.finish_field();
        }
    }

    fn finish_field(&mut self) {
        let Some(field) = self.field.take() else { return };
        let instruction = field.instruction.trim();
        if let Some(rest) = instruction.strip_prefix("HYPERLINK") {
            let address = rest
                .split_whitespace()
                .find(|piece| !piece.starts_with('\\'))
                .map(|piece| piece.trim_matches('"').to_owned())
                .unwrap_or_default();
            if !address.is_empty() && field.paragraph == self.paragraphs_done {
                self.flush_run();
                self.links.push(LinkFound {
                    paragraph: field.paragraph,
                    start: field.start,
                    end: self.paragraph_length,
                    address,
                });
            }
        }
    }

    /// The picture group closed: its bytes become a picture, and a mark
    /// stands for it in the text.
    fn finish_picture(&mut self) {
        let Some(picture) = self.picture.take() else { return };
        let Some(extension) = picture.extension else { return };
        let bytes = decode_hex(&picture.hex);
        if bytes.is_empty() {
            return;
        }
        // Twips are what Word writes for the size on the page; pixels at
        // ninety-six to the inch stand in where it did not.
        let (width_emu, height_emu) = match (picture.width_twips, picture.height_twips) {
            (Some(width), Some(height)) if width > 0 && height > 0 => (width * 635, height * 635),
            _ => {
                let (width, height) = match (picture.width_pixels, picture.height_pixels) {
                    (Some(width), Some(height)) if width > 0 && height > 0 => (width, height),
                    _ => wp_image::decode(&bytes)
                        .map(|image| (image.width as i64, image.height as i64))
                        .unwrap_or((96, 96)),
                };
                (width * 9525, height * 9525)
            }
        };
        let offset = self.paragraph_length;
        self.pictures.push(PictureFound {
            paragraph: self.paragraphs_done,
            offset,
            bytes,
            extension,
            width_emu,
            height_emu,
        });
        // The mark goes in with the formatting of the group outside the
        // picture, which is the state under the one being closed.
        let properties = self.stack[self.stack.len() - 2].chars.clone();
        self.flush_run();
        self.runs.push(Run {
            properties,
            content: vec![RunContent::Text(PICTURE_MARK.to_string())],
            field: None,
            revision: None,
            format_change: None,
        });
        self.paragraph_length += PICTURE_MARK.len_utf8();
    }

    /// The end of the file: whatever was still being built is finished.
    fn finish(mut self) -> Reading {
        if !self.runs.is_empty() || !self.text.is_empty() || !self.pending_bytes.is_empty() {
            self.end_paragraph();
        }
        if !self.row.is_empty() {
            self.end_row();
        }
        self.end_table();
        if self.body.blocks.is_empty() {
            self.body.blocks.push(Block::Paragraph(Paragraph::default()));
        }
        Reading { body: self.body, pictures: self.pictures, links: self.links }
    }
}

/// Two hex digits at a time into bytes.
fn decode_hex(hex: &[u8]) -> Vec<u8> {
    hex.chunks_exact(2)
        .filter_map(|pair| {
            let text = core::str::from_utf8(pair).ok()?;
            u8::from_str_radix(text, 16).ok()
        })
        .collect()
}

/// The code page a font's charset names.
fn charset_code_page(charset: i32) -> Option<u16> {
    match charset {
        0 => Some(1252),
        238 => Some(1250),
        204 => Some(1251),
        161 => Some(1253),
        162 => Some(1254),
        177 => Some(1255),
        178 => Some(1256),
        186 => Some(1257),
        163 => Some(1258),
        255 => Some(437),
        _ => None,
    }
}

/// The style identifier for a stylesheet name, for the styles every
/// document here has.
fn style_id(name: &str) -> Option<String> {
    let lower = name.to_lowercase();
    if lower == "normal" {
        return None;
    }
    if let Some(number) = lower.strip_prefix("heading ") {
        if let Ok(level) = number.trim().parse::<u8>() {
            if (1..=9).contains(&level) {
                return Some(format!("Heading{level}"));
            }
        }
    }
    if lower == "title" {
        return Some("Title".to_owned());
    }
    // Anything else keeps its name, spaces and all taken out, which is how
    // Word makes an identifier of a name.
    let id: String = name.chars().filter(|c| c.is_alphanumeric()).collect();
    (!id.is_empty()).then_some(id)
}

/// Word's sixteen highlight colours, by the colour a file gives.
fn highlight_name(hex: &str) -> Option<String> {
    let name = match hex.to_uppercase().as_str() {
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

/// The language tag for a Windows language number, for the common ones.
fn language_tag(number: i32) -> Option<&'static str> {
    Some(match number {
        1033 => "en-US",
        2057 => "en-GB",
        1031 => "de-DE",
        1036 => "fr-FR",
        1034 | 3082 => "es-ES",
        1040 => "it-IT",
        1043 => "nl-NL",
        1046 => "pt-BR",
        2070 => "pt-PT",
        1049 => "ru-RU",
        1058 => "uk-UA",
        1045 => "pl-PL",
        1029 => "cs-CZ",
        1030 => "da-DK",
        1053 => "sv-SE",
        1044 => "nb-NO",
        1035 => "fi-FI",
        1038 => "hu-HU",
        1055 => "tr-TR",
        1032 => "el-GR",
        1037 => "he-IL",
        1025 => "ar-SA",
        1041 => "ja-JP",
        1042 => "ko-KR",
        2052 => "zh-CN",
        1028 => "zh-TW",
        1024 => return None,
        _ => return None,
    })
}
