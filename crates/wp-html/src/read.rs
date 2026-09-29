//! Reading a page into the document model.
//!
//! # What a page from Word looks like
//!
//! A head with a hundred lines of `<style>` — a class for every paragraph
//! style, an `@list` rule for every list level, an `@page` rule for every
//! section, an `@font-face` rule for every font — and a body where every
//! paragraph is `<p class=MsoNormal style='...'>`, every run is a `<span
//! style='...'>`, every list item carries `mso-list:l0 level1 lfo1` and its
//! bullet inside `<![if !supportLists]>`, every drawing is VML in a
//! conditional comment with something plainer after it for everyone else,
//! the footnotes and the comments come at the end in divisions of their own,
//! the headers and footers are in a file beside the page, and the whole
//! thing is wrapped in `<o:p>` and `<w:...>` tags nothing else understands.
//! The formatting is in three places at once: the class rule, the style
//! attribute, and the tag. This reader folds the three together in that
//! order and reads the page anybody else wrote the same way, because a page
//! from anybody else is the same thing with less of it.
//!
//! # How it is shaped
//!
//! A stack of open elements, each carrying the character formatting in
//! force inside it — inherited from the one outside and changed by its own
//! rules — and a paragraph being built. A block tag begins a paragraph, a
//! table tag begins a table, and text goes into whatever paragraph is open,
//! with its whitespace folded as a browser folds it.
//!
//! Text goes into a *story*: the document's own, or a footnote's, a
//! comment's, a header's. A division that is one of those begins a story of
//! its own, read the same way and put where it belongs when the division
//! ends.

use std::collections::HashMap;

use wp_docx::anchor::{Anchor, Placement, Relative, Wrap, WrapSide, USUAL_DEPTH};
use wp_docx::colour::Colour;
use wp_docx::fills::Fill;
use wp_docx::fonts::{FontClass, FontEntry};
use wp_docx::furniture::{Furniture, Which};
use wp_docx::model::{
    Alignment, Block, Body, Border, BreakKind, LineRule, LineSpacing, NumberingReference,
    Paragraph, ParagraphBorders, ParagraphProperties, Run, RunContent, RunProperties, Table,
    TableBorders, TableCell, TableRow, Underline, VerticalAlignment,
};
use wp_docx::sections::Start;
use wp_docx::shapes::Shape;
use wp_docx::table_properties::CellAlignment;
use wp_docx::{StyleKind, TextPosition};

use crate::css::{self, Declaration, Sheet};
use crate::tokens::{Token, Tokenizer};

/// The character that stands where a picture goes in the text, until the
/// picture is put in.
pub const PICTURE_MARK: char = '\u{FFFC}';

/// What was read.
///
/// Every place in it is counted the way the document will count it once the
/// pictures are in: a picture, a note's mark and a drawing are one character
/// each.
#[derive(Debug, Default)]
pub struct Reading {
    pub body: Body,
    /// The pictures, each with where it goes — the paragraph, counted
    /// through the whole document, and the offset of its mark — and where
    /// its bytes are.
    pub pictures: Vec<PictureFound>,
    /// The links, each with the paragraph and the range of text it covers.
    pub links: Vec<LinkFound>,
    pub bookmarks: Vec<BookmarkFound>,
    pub comments: Vec<CommentFound>,
    /// What the notes say, for the marks already in the body.
    pub notes: Vec<NoteFound>,
    /// The sections, in order; there is always at least one.
    pub sections: Vec<SectionFound>,
    /// Headers and footers the page holds by their names, for a page that
    /// names them: Word writes them in a file of their own beside the page.
    pub furniture: Vec<FurniturePart>,
    /// The styles the sheet describes as Word's.
    pub styles: Vec<StyleFound>,
    /// What the page says about its fonts.
    pub fonts: Vec<FontEntry>,
    /// The colour behind the whole page.
    pub page_colour: Option<String>,
    /// What the page called itself.
    pub title: Option<String>,
}

#[derive(Clone, Debug)]
pub struct PictureFound {
    pub paragraph: usize,
    pub offset: usize,
    /// The `src` as written: a path beside the page, an address, or a
    /// `data:` URI holding the bytes.
    pub source: String,
    /// The size the page asks for, where it asks.
    pub width_emu: Option<i64>,
    pub height_emu: Option<i64>,
    /// Where it floats, for a picture Word drew as a floating drawing.
    pub anchor: Option<Anchor>,
}

#[derive(Debug)]
pub struct LinkFound {
    pub paragraph: usize,
    pub start: usize,
    pub end: usize,
    pub address: String,
}

#[derive(Debug)]
pub struct BookmarkFound {
    pub name: String,
    pub start: TextPosition,
    pub end: TextPosition,
}

#[derive(Debug)]
pub struct CommentFound {
    pub start: TextPosition,
    pub end: TextPosition,
    pub author: String,
    /// An ISO 8601 timestamp, or nothing where the page gave no date.
    pub date: String,
    pub body: Body,
    pub pictures: Vec<PictureFound>,
}

#[derive(Debug)]
pub struct NoteFound {
    /// The number its mark in the body carries.
    pub id: i32,
    pub endnote: bool,
    pub body: Body,
    pub pictures: Vec<PictureFound>,
}

#[derive(Debug, Default)]
pub struct SectionFound {
    /// The paragraph it ends with, counted through the whole document; the
    /// last section ends with the document and has none.
    pub last_paragraph: Option<usize>,
    /// How it begins.
    pub start: Start,
    pub page: PageSetup,
    /// Headers and footers the page held itself.
    pub furniture: Vec<FurnitureFound>,
    /// Headers and footers named from the section's `@page` rule, in a file
    /// beside the page.
    pub linked: Vec<FurnitureLink>,
}

/// How a section's pages are set up, as far as the page says.
#[derive(Clone, Debug, Default)]
pub struct PageSetup {
    pub width: Option<i32>,
    pub height: Option<i32>,
    /// Top, right, bottom and left.
    pub margins: [Option<i32>; 4],
    pub header_distance: Option<i32>,
    pub footer_distance: Option<i32>,
    /// How many columns, and the gap between them.
    pub columns: Option<(usize, i32)>,
    /// Whether its first page has a header and footer of its own.
    pub title_page: bool,
}

#[derive(Debug)]
pub struct FurnitureFound {
    pub kind: Furniture,
    pub which: Which,
    pub body: Body,
    pub pictures: Vec<PictureFound>,
}

/// A header or footer named by its file and its name in it:
/// `mso-header: url("page_files/header.htm") h1`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FurnitureLink {
    pub kind: Furniture,
    pub which: Which,
    pub url: String,
    pub id: String,
}

/// A header or footer a page holds under its name.
#[derive(Debug)]
pub struct FurniturePart {
    pub id: String,
    pub body: Body,
    pub pictures: Vec<PictureFound>,
}

#[derive(Debug)]
pub struct StyleFound {
    pub id: String,
    pub name: String,
    pub kind: StyleKind,
    pub based_on: Option<String>,
    pub next: Option<String>,
    pub paragraph: ParagraphProperties,
    pub run: RunProperties,
    /// The lines a table style draws round and through its cells.
    pub table_borders: TableBorders,
}

/// One open element and what is in force inside it.
#[derive(Clone, Debug, Default)]
struct Frame {
    tag: String,
    chars: RunProperties,
    /// Where a link began, for an `<a>`, in the story it is in.
    link: Option<(String, usize, usize)>,
    /// Whether whitespace is kept as written, inside `<pre>`.
    preformatted: bool,
    /// Whether the text inside runs right to left: `dir` and `direction`.
    backwards: bool,
    /// The alignment a paragraph inside takes unless it says its own:
    /// `align` on a division or a cell.
    alignment: Option<Alignment>,
    /// The lines and the colour a division draws round the paragraphs in it.
    boxed: Option<(ParagraphBorders, Option<String>)>,
    /// Whether the text inside is not the document's: a note's number, a
    /// comment's mark.
    suppressed: bool,
    /// The comment this element is the range of, and where it began.
    comment: Option<(usize, TextPosition)>,
    /// The bookmark this element names, and where it began.
    bookmark: Option<(String, TextPosition)>,
    /// Whether a story began with this element.
    story: bool,
    /// Whether it was opened where the page hides what it holds: kept only
    /// so that its end closes it, and nothing else.
    inert: bool,
}

/// A list being read: what kind, and how deep.
#[derive(Clone, Copy, Debug)]
struct ListFrame {
    bullet: bool,
    level: u8,
}

/// What a story is, which decides where it goes when it ends.
#[derive(Clone, Debug, PartialEq, Eq)]
enum StoryKind {
    Main,
    Note {
        id: i32,
        endnote: bool,
    },
    Comment {
        number: usize,
    },
    /// A header or footer under its name, as Word's file of them holds it.
    Part {
        id: String,
    },
    /// A header or footer in the page itself, as LibreOffice writes one.
    Furniture {
        kind: Furniture,
    },
    /// Something that is not the document's at all: the rule over the notes.
    Dropped,
}

/// Text being read into one place.
#[derive(Debug)]
struct Story {
    kind: StoryKind,
    blocks: Vec<Block>,
    /// The paragraph being built, and whether it is kept when nothing goes
    /// into it: a `<p>` is a paragraph however empty, a `<div>` round other
    /// things is not.
    paragraph: Option<(ParagraphProperties, Vec<Run>)>,
    keep_empty: bool,
    paragraph_length: usize,
    paragraphs_done: usize,
    /// Whether the last thing put in the paragraph was a space, so that a
    /// run of whitespace folds to one.
    after_space: bool,
    /// The tables being built, outermost first.
    tables: Vec<TableBuild>,
    pictures: Vec<PictureFound>,
}

impl Story {
    fn new(kind: StoryKind) -> Self {
        Self {
            kind,
            blocks: Vec::new(),
            paragraph: None,
            keep_empty: false,
            paragraph_length: 0,
            paragraphs_done: 0,
            after_space: true,
            tables: Vec::new(),
            pictures: Vec::new(),
        }
    }
}

/// A table being read.
#[derive(Debug, Default)]
struct TableBuild {
    table: Table,
    rows: Vec<RowBuild>,
    row: Option<RowBuild>,
    cell: Option<CellBuild>,
    /// The widths `<col>` gives the columns.
    columns: Vec<Option<i32>>,
    /// The line every cell has when the table says `border`.
    lines: Option<Border>,
    /// Whether the rows being read are the table's head.
    in_head: bool,
}

#[derive(Debug, Default)]
struct RowBuild {
    cells: Vec<CellBuild>,
    height: Option<i32>,
    header: bool,
    vertical: Option<CellAlignment>,
}

#[derive(Debug)]
struct CellBuild {
    blocks: Vec<Block>,
    width: Option<i32>,
    colspan: u32,
    rowspan: u32,
    borders: TableBorders,
    shading: Option<String>,
    vertical: Option<CellAlignment>,
}

struct Reader {
    sheet: Sheet,
    frames: Vec<Frame>,
    lists: Vec<ListFrame>,
    stories: Vec<Story>,
    /// The text of the current run, gathered until the formatting changes.
    text: String,
    text_chars: RunProperties,
    /// How deep inside conditionals that hide their contents.
    hidden_depth: usize,
    conditional_depth: usize,
    /// Whether the last conditional comment drew something in VML, so the
    /// plainer version after it is not drawn again.
    drew_vml: bool,
    /// Whether the head or a title is being read.
    in_title: bool,
    title: String,
    /// The paragraph and character styles by the classes that name them.
    style_classes: Option<HashMap<String, (StyleKind, String)>>,
    links: Vec<LinkFound>,
    bookmarks: Vec<BookmarkFound>,
    /// The notes by the name their mark gives them, with their numbers.
    note_keys: HashMap<String, (i32, bool)>,
    footnotes: i32,
    endnotes: i32,
    notes: Vec<NoteFound>,
    /// The comments' ranges by number, and their authors and dates.
    comment_ranges: HashMap<usize, (TextPosition, TextPosition)>,
    comment_dates: HashMap<usize, String>,
    comment_authors: HashMap<usize, String>,
    comments_read: usize,
    comments: Vec<(usize, Body, Vec<PictureFound>)>,
    /// Sections: the one being read, by its `@page` name, and how it began.
    section_name: Option<String>,
    section_start: Start,
    section_furniture: Vec<FurnitureFound>,
    sections: Vec<SectionFound>,
    /// How the next section begins, from the break that ended this one.
    next_start: Option<Start>,
    furniture: Vec<FurniturePart>,
    page_colour: Option<String>,
    /// Drawings a Word page numbers from one, for the depth each floats at.
    drawings: u32,
    /// The names of the VML drawings drawn, whose plainer copies for other
    /// readers — `v:shape`, `v:shapes` — are not drawn again.
    drawn: Vec<String>,
    /// The comments' authors' initials, from their marks: `KS_1`.
    comment_initials: HashMap<usize, String>,
}

/// Reads a page into a body, with everything that goes beside it.
#[must_use]
pub fn read(html: &str) -> Reading {
    let mut reader = Reader::new(Sheet::default());
    reader.run(html);
    reader.finish()
}

impl Reader {
    fn new(sheet: Sheet) -> Self {
        Self {
            sheet,
            frames: vec![Frame { tag: "html".to_owned(), ..Frame::default() }],
            lists: Vec::new(),
            stories: vec![Story::new(StoryKind::Main)],
            text: String::new(),
            text_chars: RunProperties::default(),
            hidden_depth: 0,
            conditional_depth: 0,
            drew_vml: false,
            in_title: false,
            title: String::new(),
            style_classes: None,
            links: Vec::new(),
            bookmarks: Vec::new(),
            note_keys: HashMap::new(),
            footnotes: 0,
            endnotes: 0,
            notes: Vec::new(),
            comment_ranges: HashMap::new(),
            comment_dates: HashMap::new(),
            comment_authors: HashMap::new(),
            comments_read: 0,
            comments: Vec::new(),
            section_name: None,
            section_start: Start::NextPage,
            section_furniture: Vec::new(),
            sections: Vec::new(),
            next_start: None,
            furniture: Vec::new(),
            page_colour: None,
            drawings: 0,
            drawn: Vec::new(),
            comment_initials: HashMap::new(),
        }
    }

    fn run(&mut self, html: &str) {
        let mut tokenizer = Tokenizer::new(html);
        while let Some(token) = tokenizer.next_token() {
            match token {
                Token::Open { name, attributes, self_closing } => {
                    self.open(&name, &attributes);
                    if self_closing || is_void(&name) {
                        self.close(&name);
                    }
                }
                Token::Close(name) => self.close(&name),
                Token::Text(text) => self.text(&text),
                Token::Style(text) => self.sheet.read(&text),
                Token::Comment => {}
                Token::Hidden { condition, inner } => self.hidden_block(&condition, &inner),
                Token::ConditionalOpen(condition) => {
                    self.conditional_depth += 1;
                    // What Word shows only to readers that do not know lists,
                    // footnotes, comments, Office or VML is the fallback for
                    // what it has already written another way.
                    let fallback = condition.starts_with("!support")
                        || condition == "!mso"
                        || (condition == "!vml" && self.drew_vml);
                    if fallback && self.hidden_depth == 0 {
                        self.hidden_depth = self.conditional_depth;
                    }
                    if condition == "!vml" {
                        self.drew_vml = false;
                    }
                }
                Token::ConditionalClose => {
                    if self.hidden_depth == self.conditional_depth {
                        self.hidden_depth = 0;
                    }
                    self.conditional_depth = self.conditional_depth.saturating_sub(1);
                }
            }
        }
    }

    fn top(&self) -> &Frame {
        self.frames.last().expect("the page")
    }

    fn story(&self) -> &Story {
        self.stories.last().expect("the page's story")
    }

    fn story_mut(&mut self) -> &mut Story {
        self.stories.last_mut().expect("the page's story")
    }

    fn in_main_story(&self) -> bool {
        self.stories.len() == 1
    }

    fn hidden(&self) -> bool {
        self.hidden_depth > 0
    }

    /// Where the text of the story being read has got to.
    fn here(&self) -> TextPosition {
        let story = self.story();
        TextPosition::new(story.paragraphs_done, story.paragraph_length)
    }

    /// The classes of the sheet that are Word's styles, by class.
    fn style_of_class(&mut self, class: &str) -> Option<(StyleKind, String)> {
        if self.style_classes.is_none() {
            let found = word_styles(&self.sheet)
                .into_iter()
                .filter_map(|(class, style)| Some((class?, (style.kind, style.id))))
                .collect();
            self.style_classes = Some(found);
        }
        self.style_classes.as_ref().and_then(|classes| classes.get(class).cloned())
    }

    // --- What Word hides from everybody else ------------------------------------

    /// A conditional comment's contents: VML drawings, table styles, settings.
    fn hidden_block(&mut self, condition: &str, inner: &str) {
        if condition.contains("vml") {
            self.drew_vml = self.vml(inner);
            return;
        }
        if condition.contains("mso") {
            // Word's table styles are in a style block only Office reads.
            let mut tokenizer = Tokenizer::new(inner);
            while let Some(token) = tokenizer.next_token() {
                if let Token::Style(text) = token {
                    self.sheet.read(&text);
                    self.style_classes = None;
                }
            }
        }
    }

    // --- Tags ------------------------------------------------------------------

    #[allow(clippy::too_many_lines, reason = "one arm per tag is the readable shape")]
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
        let declared = |wanted: &str| declared(&declarations, wanted).map(str::to_owned);

        // Inside what the page hides, a tag is kept only so that its end has
        // something to close: a table there is not a table of the document's.
        // So is the plainer copy of a drawing already drawn from its VML.
        let copy_of_drawn = ["v:shape", "v:shapes"].iter().any(|wanted| {
            attribute(wanted).is_some_and(|names| {
                names.split_whitespace().any(|name| self.drawn.iter().any(|drawn| drawn == name))
            })
        });
        if self.hidden() || copy_of_drawn || self.top().suppressed {
            let parent = self.top();
            let frame = Frame {
                tag: name.to_owned(),
                chars: parent.chars.clone(),
                // What is inside it is hidden by the condition, not by it:
                // Word opens a fallback's wrapper where it hides it and puts
                // what everyone reads inside. A copy of a drawing is not read.
                suppressed: copy_of_drawn || parent.suppressed,
                inert: true,
                ..Frame::default()
            };
            self.frames.push(frame);
            return;
        }

        // Which way the text inside runs.
        let parent = self.top().clone();
        let backwards = match attribute("dir").map(str::to_ascii_lowercase).as_deref() {
            Some("rtl") => true,
            Some("ltr") => false,
            _ => match declared("direction").as_deref() {
                Some("rtl") => true,
                Some("ltr") => false,
                _ => parent.backwards,
            },
        };

        match name {
            "title" => {
                self.in_title = true;
                return;
            }
            "body" => {
                self.page_colour = attribute("bgcolor")
                    .and_then(css::colour)
                    .or_else(|| declared("background").and_then(|value| css::colour(&value)))
                    .or_else(|| declared("background-color").and_then(|value| css::colour(&value)));
            }
            "table" => {
                self.end_paragraph();
                let mut build = TableBuild::default();
                build.table.style =
                    classes.iter().find_map(|class| match self.style_of_class(class) {
                        Some((StyleKind::Table, id)) => Some(id),
                        _ => None,
                    });
                build.table.borders = table_borders_from(&declarations);
                // `border=1`: a line round every cell, as a browser draws.
                let width = attribute("border").and_then(|value| value.trim().parse::<u32>().ok());
                if let Some(pixels) = width.filter(|pixels| *pixels > 0) {
                    let line = Border::line("single", (pixels * 6).min(96), None);
                    build.lines = Some(line.clone());
                    if build.table.borders.is_empty() {
                        build.table.borders = TableBorders {
                            top: Some(line.clone()),
                            start: Some(line.clone()),
                            bottom: Some(line.clone()),
                            end: Some(line.clone()),
                            inside_horizontal: Some(line.clone()),
                            inside_vertical: Some(line),
                        };
                    }
                }
                if attribute("width").is_some_and(|value| value.trim().ends_with('%')) {
                    let percent = attribute("width")
                        .and_then(|value| value.trim().trim_end_matches('%').parse::<i32>().ok());
                    if let Some(percent) = percent {
                        build.table.fit = wp_docx::model::TableFit::Window(percent.min(100));
                    }
                }
                self.story_mut().tables.push(build);
            }
            "col" => {
                let width = attribute("width").and_then(css::twips);
                if let Some(table) = self.story_mut().tables.last_mut() {
                    let span = attribute("span").and_then(|value| value.parse::<usize>().ok());
                    for _ in 0..span.unwrap_or(1).max(1) {
                        table.columns.push(width);
                    }
                }
            }
            "thead" => {
                if let Some(table) = self.story_mut().tables.last_mut() {
                    table.in_head = true;
                }
            }
            "tr" => {
                self.end_paragraph();
                let height = declared("height")
                    .and_then(|value| css::twips(&value))
                    .or_else(|| attribute("height").and_then(css::twips));
                let vertical = attribute("valign").and_then(cell_alignment);
                let story = self.story_mut();
                if let Some(table) = story.tables.last_mut() {
                    story.paragraphs_done += Self::end_row_of(table);
                    let header = table.in_head;
                    table.row = Some(RowBuild { height, header, vertical, ..RowBuild::default() });
                }
            }
            "td" | "th" => {
                self.end_paragraph();
                let width = declared("width")
                    .and_then(|value| css::twips(&value))
                    .or_else(|| attribute("width").and_then(css::twips));
                let span = |value: Option<&str>| {
                    value.and_then(|value| value.trim().parse::<u32>().ok()).unwrap_or(1).max(1)
                };
                let mut borders = table_borders_from(&declarations);
                let shading = attribute("bgcolor")
                    .and_then(css::colour)
                    .or_else(|| declared("background").and_then(|value| css::colour(&value)))
                    .or_else(|| declared("background-color").and_then(|value| css::colour(&value)));
                let vertical = attribute("valign")
                    .or(declared("vertical-align").as_deref())
                    .and_then(cell_alignment);
                let (colspan, rowspan) = (span(attribute("colspan")), span(attribute("rowspan")));
                let story = self.story_mut();
                if let Some(table) = story.tables.last_mut() {
                    story.paragraphs_done += Self::end_cell_of(table);
                    if table.row.is_none() {
                        table.row = Some(RowBuild::default());
                    }
                    if borders.is_empty() {
                        if let Some(line) = &table.lines {
                            borders = TableBorders {
                                top: Some(line.clone()),
                                start: Some(line.clone()),
                                bottom: Some(line.clone()),
                                end: Some(line.clone()),
                                ..TableBorders::default()
                            };
                        }
                    }
                    table.cell = Some(CellBuild {
                        blocks: Vec::new(),
                        width,
                        colspan,
                        rowspan,
                        borders,
                        shading,
                        vertical,
                    });
                }
            }
            "ul" | "ol" | "menu" => {
                self.end_paragraph();
                let level = self.lists.len().min(8) as u8;
                self.lists.push(ListFrame { bullet: name != "ol", level });
            }
            "br" => {
                // The break Word ends a section with begins the next one, and
                // is not a break in the text.
                if let Some(kind) =
                    declared("mso-break-type").filter(|kind| kind == "section-break")
                {
                    let _ = kind;
                    let page = declared("page-break-before").is_some_and(|value| value == "always");
                    self.next_start = Some(if page { Start::NextPage } else { Start::Continuous });
                    return;
                }
                let page = declared("page-break-before").is_some_and(|value| value == "always");
                self.content(
                    RunContent::Break(if page { BreakKind::Page } else { BreakKind::Line }),
                    1,
                );
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

        // A division that is a story of its own: a note, a comment, a header.
        let story =
            if name == "div" { self.story_of_division(attributes, &declarations) } else { None };
        if let Some(kind) = &story {
            self.end_paragraph();
            self.stories.push(Story::new(kind.clone()));
        }

        // A division that begins one of Word's sections.
        if name == "div" && story.is_none() && self.in_main_story() {
            if let Some(page) = classes.iter().find_map(|class| self.page_of_class(class)) {
                self.begin_section(page);
            }
        }

        let mut frame = Frame {
            tag: name.to_owned(),
            chars: parent.chars.clone(),
            link: None,
            preformatted: parent.preformatted || name == "pre",
            backwards,
            alignment: parent.alignment,
            boxed: parent.boxed.clone(),
            suppressed: parent.suppressed,
            comment: None,
            bookmark: None,
            story: story.is_some(),
            inert: false,
        };
        if story.is_some() {
            // A story is formatted from nothing, whatever it sits in.
            frame.chars = RunProperties::default();
            frame.alignment = None;
            frame.boxed = None;
            frame.suppressed = false;
        }

        // A block's alignment and its box, for the paragraphs inside it.
        if matches!(name, "div" | "td" | "th" | "center" | "blockquote" | "body" | "section") {
            if let Some(alignment) = attribute("align").and_then(alignment_of) {
                frame.alignment = Some(alignment);
            }
            if name == "center" {
                frame.alignment = Some(Alignment::Center);
            }
            if matches!(name, "div" | "blockquote" | "section") {
                let borders = paragraph_borders_from(&declarations);
                let fill = declared("background")
                    .or_else(|| declared("background-color"))
                    .and_then(|value| css::colour(&value));
                if !borders.is_empty() || fill.is_some() {
                    frame.boxed = Some((borders, fill));
                }
            }
        }

        if is_block(name) && story.is_none() {
            let styled = attribute("style").is_some_and(|style| {
                css::parse_declarations(style).iter().any(|(held, _)| held == "text-align")
            });
            let align = attribute("align").filter(|_| !styled);
            self.begin_paragraph(name, &classes, &declarations, &frame, align);
        }

        // The character formatting inside this element: what was in force
        // outside it, changed by the tag and by its rules.
        let chars = &mut frame.chars;
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
        apply_character_declarations(chars, &declarations, !is_block_tag(name));
        if let Some(lang) = attribute("lang") {
            chars.language = language_tag(lang);
        }
        if !is_block_tag(name) {
            match attribute("dir").map(str::to_ascii_lowercase).as_deref() {
                Some("rtl") => chars.right_to_left = Some(true),
                Some("ltr") => chars.right_to_left = None,
                _ => {}
            }
        }
        if matches!(name, "span" | "a") {
            for class in &classes {
                if let Some((StyleKind::Character, id)) = self.style_of_class(class) {
                    frame.chars.style = Some(id);
                }
            }
        }
        // A comment's mark in the text, and the characters Word writes for
        // a note's number or a comment's: not the document's words.
        let special = declared("mso-special-character")
            .is_some_and(|value| !value.contains("line-break") && !value.contains("tab"));
        if special || classes.iter().any(|class| class == "MsoCommentReference") {
            frame.suppressed = true;
        }
        if let Some(author) = declared("mso-comment-author") {
            if let StoryKind::Comment { number } = self.story().kind {
                self.comment_authors.insert(number, css::unquote(&author));
            }
        }

        if name == "a" {
            self.anchor(attributes, &declarations, &mut frame);
        }
        self.frames.push(frame);
    }

    /// An `<a>`: a link, a bookmark, a note's mark or a comment's range.
    fn anchor(
        &mut self,
        attributes: &[(String, String)],
        declarations: &[Declaration],
        frame: &mut Frame,
    ) {
        let attribute = |wanted: &str| {
            attributes.iter().find(|(held, _)| held == wanted).map(|(_, value)| value.as_str())
        };
        let href = attribute("href").unwrap_or("");
        let class = attribute("class").unwrap_or("");

        // A note's mark: in the document's own text, the place of the note;
        // in the note, the note's own number.
        let key = declared(declarations, "mso-footnote-id")
            .or_else(|| declared(declarations, "mso-endnote-id"))
            .map(str::to_owned)
            .or_else(|| note_key(href));
        if let Some(key) = key {
            frame.suppressed = true;
            if !self.in_main_story() {
                return;
            }
            let endnote = key.starts_with("edn") || key.starts_with("sdendnote");
            let entry = match self.note_keys.get(&key) {
                Some(entry) => *entry,
                None => {
                    let id = if endnote {
                        self.endnotes += 1;
                        self.endnotes
                    } else {
                        self.footnotes += 1;
                        self.footnotes
                    };
                    self.note_keys.insert(key, (id, endnote));
                    (id, endnote)
                }
            };
            self.content(RunContent::NoteReference { id: entry.0, endnote: entry.1 }, 1);
            return;
        }
        if class.starts_with("sdfootnote") || class.starts_with("sdendnote") || note_back(href) {
            frame.suppressed = true;
            return;
        }

        // A comment's range: `mso-comment-reference:KS_1`, the author's
        // initials and the comment's number.
        if let Some(reference) = declared(declarations, "mso-comment-reference") {
            if let Some((initials, number)) = reference.rsplit_once('_') {
                let Ok(number) = number.parse::<usize>() else { return };
                if let Some(date) = declared(declarations, "mso-comment-date") {
                    self.comment_dates.insert(number, comment_date(date));
                }
                if !initials.is_empty() {
                    self.comment_initials.insert(number, initials.to_owned());
                }
                if let Some(author) = declared(declarations, "mso-comment-author") {
                    self.comment_authors.insert(number, css::unquote(author));
                }
                if self.in_main_story() {
                    frame.comment = Some((number, self.here()));
                }
            }
            return;
        }

        if !href.is_empty() {
            frame.link = Some((
                href.to_owned(),
                self.story().paragraphs_done,
                self.story().paragraph_length,
            ));
        }
        if let Some(name) = attribute("name").filter(|name| is_bookmark_name(name)) {
            if self.in_main_story() {
                frame.bookmark = Some((name.to_owned(), self.here()));
            }
        }
    }

    /// Which story a division is, if it is one.
    fn story_of_division(
        &mut self,
        attributes: &[(String, String)],
        declarations: &[Declaration],
    ) -> Option<StoryKind> {
        let attribute = |wanted: &str| {
            attributes.iter().find(|(held, _)| held == wanted).map(|(_, value)| value.as_str())
        };
        let id = attribute("id").unwrap_or("");
        match declared(declarations, "mso-element") {
            Some("footnote" | "endnote") | None if self.note_keys.contains_key(id) => {
                let (id, endnote) = self.note_keys[id];
                Some(StoryKind::Note { id, endnote })
            }
            Some("comment") => {
                self.comments_read += 1;
                Some(StoryKind::Comment { number: self.comments_read })
            }
            Some("header" | "footer") => Some(StoryKind::Part { id: id.to_owned() }),
            Some(element) if element.contains("separator") || element.contains("notice") => {
                Some(StoryKind::Dropped)
            }
            _ => match attribute("title") {
                Some("header") if self.in_main_story() => {
                    Some(StoryKind::Furniture { kind: Furniture::Header })
                }
                Some("footer") if self.in_main_story() => {
                    Some(StoryKind::Furniture { kind: Furniture::Footer })
                }
                _ => None,
            },
        }
    }

    /// The `@page` a division of that class is set on: `div.WordSection1
    /// {page:WordSection1}`.
    fn page_of_class(&self, class: &str) -> Option<String> {
        self.sheet.rules().iter().rev().find_map(|rule| {
            (rule.class.as_deref() == Some(class))
                .then(|| declared(&rule.declarations, "page").map(str::to_owned))
                .flatten()
        })
    }

    /// A section of Word's begins: the one before it ends with the last
    /// paragraph read.
    fn begin_section(&mut self, page: String) {
        self.end_paragraph();
        self.close_tables();
        if self.section_name.is_some() && self.stories[0].paragraphs_done > 0 {
            let last = self.stories[0].paragraphs_done - 1;
            let ended = self.section_found(Some(last));
            self.sections.push(ended);
            self.section_start = self.next_start.take().unwrap_or(Start::NextPage);
        }
        self.section_name = Some(page);
    }

    /// The section being read, as it stands.
    fn section_found(&mut self, last_paragraph: Option<usize>) -> SectionFound {
        let name = self.section_name.clone().unwrap_or_default();
        let declarations = self.sheet.page(&name);
        SectionFound {
            last_paragraph,
            start: self.section_start,
            page: page_setup(&declarations),
            furniture: core::mem::take(&mut self.section_furniture),
            linked: furniture_links(&declarations),
        }
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
        if depth == 0 {
            return;
        }
        while self.frames.len() > depth {
            let frame = self.frames.pop().expect("a frame");
            self.flush_run();
            if frame.inert {
                continue;
            }
            if let Some((address, paragraph, start)) = frame.link {
                let story = self.story();
                if paragraph == story.paragraphs_done && story.paragraph_length > start {
                    if self.in_main_story() {
                        self.links.push(LinkFound {
                            paragraph,
                            start,
                            end: story.paragraph_length,
                            address,
                        });
                    } else {
                        self.link_as_field(start, &address);
                    }
                }
            }
            if let Some((number, start)) = frame.comment {
                let end = self.here();
                let range = self.comment_ranges.entry(number).or_insert((start, end));
                range.0 = range.0.min(start);
                range.1 = range.1.max(end);
            }
            if let Some((name, start)) = frame.bookmark {
                let end = self.here();
                self.bookmarks.push(BookmarkFound { name, start, end });
            }
            if is_block(&frame.tag) && !frame.story {
                self.end_paragraph();
            }
            match frame.tag.as_str() {
                "td" | "th" => self.end_cell(),
                "tr" => self.end_row(),
                "table" => self.end_table(),
                "thead" => {
                    let story = self.story_mut();
                    if let Some(table) = story.tables.last_mut() {
                        story.paragraphs_done += Self::end_row_of(table);
                        table.in_head = false;
                    }
                }
                "ul" | "ol" | "menu" => {
                    self.lists.pop();
                }
                _ => {}
            }
            if frame.story {
                self.finish_story();
            }
        }
    }

    /// A link in a note, a comment or a header is the field Word writes one
    /// as: the runs it covers carry `HYPERLINK`.
    fn link_as_field(&mut self, start: usize, address: &str) {
        let instruction = match address.strip_prefix('#') {
            Some(place) => format!("HYPERLINK \\l \"{place}\""),
            None => format!("HYPERLINK \"{address}\""),
        };
        let Some((_, runs)) = &mut self.story_mut().paragraph else { return };
        let mut at = 0usize;
        for run in runs.iter_mut() {
            let length: usize = run
                .content
                .iter()
                .map(|content| match content {
                    RunContent::Text(text) => text.len(),
                    _ => 1,
                })
                .sum();
            if at >= start {
                run.field = Some(instruction.clone());
            }
            at += length;
        }
    }

    // --- Paragraphs ------------------------------------------------------------

    fn begin_paragraph(
        &mut self,
        name: &str,
        classes: &[String],
        declarations: &[Declaration],
        frame: &Frame,
        align: Option<&str>,
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
        for class in classes {
            if let Some((StyleKind::Paragraph, id)) = self.style_of_class(class) {
                properties.style = Some(id).filter(|id| id != "Normal");
            }
        }
        if classes.iter().any(|class| class == "MsoTitle") {
            properties.style = Some("Title".to_owned());
        }
        properties.alignment = frame.alignment;
        if let Some(list) = self.lists.last() {
            if name == "li" {
                properties.numbering = Some(NumberingReference {
                    id: if list.bullet { wp_docx::BULLET_LIST } else { wp_docx::NUMBERED_LIST },
                    level: list.level,
                });
            }
        }
        apply_paragraph_declarations(&mut properties, declarations, &self.sheet);
        // `align` says more than the rules for the tag, and less than the
        // element's own style — which has been left out of it when it says.
        if let Some(alignment) = align.and_then(alignment_of) {
            properties.alignment = Some(alignment);
        }
        // Lines and a colour of its own, or those of the division it is in.
        let own = paragraph_borders_from(declarations);
        let fill = declared(declarations, "background")
            .or_else(|| declared(declarations, "background-color"))
            .and_then(css::colour);
        match (&frame.boxed, own.is_empty() && fill.is_none()) {
            (Some((borders, shading)), true) => {
                properties.borders = borders.clone();
                properties.shading = shading.clone();
            }
            _ => {
                properties.borders = own;
                properties.shading = fill;
            }
        }
        if frame.backwards {
            properties.right_to_left = Some(true);
        }
        let story = self.story_mut();
        story.paragraph = Some((properties, Vec::new()));
        story.keep_empty = !matches!(
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
                | "blockquote"
        );
        story.paragraph_length = 0;
        story.after_space = true;
    }

    /// The paragraph a piece of text goes into, begun if there is none.
    fn ensure_paragraph(&mut self) {
        if self.story().paragraph.is_some() {
            return;
        }
        let frame = self.top();
        let mut properties = ParagraphProperties {
            alignment: frame.alignment,
            right_to_left: frame.backwards.then_some(true),
            ..ParagraphProperties::default()
        };
        if let Some((borders, shading)) = &frame.boxed {
            properties.borders = borders.clone();
            properties.shading = shading.clone();
        }
        let story = self.story_mut();
        story.paragraph = Some((properties, Vec::new()));
        story.keep_empty = false;
        story.paragraph_length = 0;
        story.after_space = true;
    }

    fn end_paragraph(&mut self) {
        self.flush_run();
        let story = self.story_mut();
        let Some((mut properties, mut runs)) = story.paragraph.take() else { return };
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
        story.paragraph_length = 0;
        story.after_space = true;
        if runs.is_empty() && !story.keep_empty {
            return;
        }
        // Left and right are the page's; the model's start and end are the
        // text's, which in text running right to left are the other way round.
        if properties.right_to_left == Some(true) {
            properties.alignment = match properties.alignment {
                Some(Alignment::Start) => Some(Alignment::End),
                Some(Alignment::End) => Some(Alignment::Start),
                other => other,
            };
            core::mem::swap(&mut properties.indent_start, &mut properties.indent_end);
            let borders = &mut properties.borders;
            core::mem::swap(&mut borders.start, &mut borders.end);
        }
        story.paragraphs_done += 1;
        self.put_block(Block::Paragraph(Paragraph { properties, runs }));
    }

    /// A finished block goes into the innermost table cell that is open, or
    /// into the story — before a table still being built, which is where a
    /// browser puts what a table holds outside its cells.
    fn put_block(&mut self, block: Block) {
        let story = self.story_mut();
        for table in story.tables.iter_mut().rev() {
            if let Some(cell) = &mut table.cell {
                cell.blocks.push(block);
                return;
            }
        }
        story.blocks.push(block);
    }

    // --- Tables ----------------------------------------------------------------

    /// Ends the cell being read. Returns how many paragraphs it was given:
    /// an empty cell still holds one, and it is counted like any other.
    fn end_cell_of(table: &mut TableBuild) -> usize {
        let Some(mut cell) = table.cell.take() else { return 0 };
        let added = usize::from(cell.blocks.is_empty());
        if added > 0 {
            cell.blocks.push(Block::Paragraph(Paragraph::default()));
        }
        table.row.get_or_insert_with(RowBuild::default).cells.push(cell);
        added
    }

    fn end_row_of(table: &mut TableBuild) -> usize {
        let added = Self::end_cell_of(table);
        if let Some(row) = table.row.take() {
            if !row.cells.is_empty() {
                table.rows.push(row);
            }
        }
        added
    }

    fn end_cell(&mut self) {
        self.end_paragraph();
        let story = self.story_mut();
        if let Some(table) = story.tables.last_mut() {
            story.paragraphs_done += Self::end_cell_of(table);
        }
    }

    fn end_row(&mut self) {
        self.end_cell();
        let story = self.story_mut();
        if let Some(table) = story.tables.last_mut() {
            story.paragraphs_done += Self::end_row_of(table);
        }
    }

    fn end_table(&mut self) {
        self.end_row();
        let Some(build) = self.story_mut().tables.pop() else { return };
        if let Some(table) = build.into_table() {
            self.put_block(Block::Table(Box::new(table)));
        }
    }

    fn close_tables(&mut self) {
        while !self.story().tables.is_empty() {
            self.end_table();
        }
    }

    // --- Text ------------------------------------------------------------------

    fn text(&mut self, text: &str) {
        if self.in_title {
            self.title.push_str(text);
            return;
        }
        if self.hidden()
            || self.top().suppressed
            || matches!(self.top().tag.as_str(), "head" | "script" | "xml" | "style")
        {
            return;
        }
        let preformatted = self.top().preformatted;
        let mut folded = String::with_capacity(text.len());
        let mut last_was_space = self.story().after_space;
        for character in text.chars() {
            if preformatted {
                if character == '\n' {
                    self.push_text(&folded);
                    folded.clear();
                    self.content(RunContent::Break(BreakKind::Line), 1);
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
        if text == " " && (self.story().after_space || self.story().paragraph.is_none()) {
            return;
        }
        self.ensure_paragraph();
        let chars = self.top().chars.clone();
        if chars != self.text_chars {
            self.flush_run();
            self.text_chars = chars;
        }
        let text = if self.story().after_space { text.trim_start_matches(' ') } else { text };
        if text.is_empty() {
            return;
        }
        self.text.push_str(text);
        let story = self.story_mut();
        story.paragraph_length += text.len();
        story.after_space = text.ends_with(' ');
    }

    fn flush_run(&mut self) {
        if self.text.is_empty() {
            return;
        }
        let text = core::mem::take(&mut self.text);
        let properties = self.text_chars.clone();
        let Some((_, runs)) = &mut self.story_mut().paragraph else { return };
        runs.push(Run {
            properties,
            content: vec![RunContent::Text(text)],
            field: None,
            revision: None,
            format_change: None,
        });
    }

    /// Something that is not text into the paragraph, as many characters long
    /// as the document will count it.
    fn content(&mut self, content: RunContent, length: usize) {
        if self.hidden() {
            return;
        }
        let content_is_break = matches!(content, RunContent::Break(_));
        self.ensure_paragraph();
        self.flush_run();
        let chars = self.top().chars.clone();
        let story = self.story_mut();
        let Some((_, runs)) = &mut story.paragraph else { return };
        runs.push(Run {
            properties: chars,
            content: vec![content],
            field: None,
            revision: None,
            format_change: None,
        });
        story.paragraph_length += length;
        // A break ends a line, and a space after it is not wanted; a note's
        // mark or a drawing is a character like any other.
        story.after_space = content_is_break;
    }

    fn picture(
        &mut self,
        source: &str,
        attributes: &[(String, String)],
        declarations: &[Declaration],
    ) {
        if self.hidden() || self.top().suppressed {
            return;
        }
        let attribute = |wanted: &str| {
            attributes.iter().find(|(held, _)| held == wanted).map(|(_, value)| value.as_str())
        };
        let size = |name: &str| {
            declared(declarations, name)
                .and_then(css::twips)
                .or_else(|| attribute(name).and_then(css::twips))
                .map(|twips| i64::from(twips) * 635)
        };
        self.place_picture(source, size("width"), size("height"), None);
    }

    /// Puts a picture's mark into the story's text, and the picture beside it.
    fn place_picture(
        &mut self,
        source: &str,
        width_emu: Option<i64>,
        height_emu: Option<i64>,
        anchor: Option<Anchor>,
    ) {
        self.ensure_paragraph();
        self.flush_run();
        let chars = self.top().chars.clone();
        let story = self.story_mut();
        story.pictures.push(PictureFound {
            paragraph: story.paragraphs_done,
            offset: story.paragraph_length,
            source: source.to_owned(),
            width_emu,
            height_emu,
            anchor,
        });
        if let Some((_, runs)) = &mut story.paragraph {
            runs.push(Run {
                properties: chars,
                content: vec![RunContent::Text(PICTURE_MARK.to_string())],
                field: None,
                revision: None,
                format_change: None,
            });
        }
        // One character, as the picture that takes its place will be.
        story.paragraph_length += 1;
        story.after_space = false;
    }

    // --- Drawings -------------------------------------------------------------

    /// Word's drawings, in VML: a shape, a text box, a line, a picture in a
    /// frame. Returns whether anything was drawn.
    fn vml(&mut self, inner: &str) -> bool {
        if self.hidden() || self.top().suppressed {
            return false;
        }
        let mut drew = false;
        for element in vml_elements(inner) {
            self.drawings += 1;
            if self.vml_element(&element) {
                drew = true;
                let id = element.attributes.iter().find(|(name, _)| name == "id");
                if let Some((_, id)) = id {
                    self.drawn.push(id.clone());
                }
            }
        }
        drew
    }

    fn vml_element(&mut self, element: &VmlElement) -> bool {
        let attribute = |wanted: &str| {
            element
                .attributes
                .iter()
                .find(|(held, _)| held == wanted)
                .map(|(_, value)| value.as_str())
        };
        let style = css::parse_declarations(attribute("style").unwrap_or(""));
        let length = |name: &str| {
            declared(&style, name).and_then(css::twips).map(|twips| i64::from(twips) * 635)
        };
        let (width, height) = (length("width"), length("height"));
        let floating = declared(&style, "position").is_some_and(|value| value == "absolute");
        let anchor = floating.then(|| vml_anchor(&style, element.wrap.as_ref(), self.drawings));

        if let Some(source) = &element.image {
            self.place_picture(source, width, height, anchor);
            return true;
        }
        let kind = match element.tag.as_str() {
            "v:rect" => Some(1),
            "v:roundrect" => Some(2),
            "v:oval" => Some(3),
            "v:line" => Some(20),
            _ => attribute("type")
                .and_then(|kind| kind.trim_start_matches('#').strip_prefix("_x0000_t"))
                .and_then(|number| number.parse::<i64>().ok())
                .or_else(|| attribute("o:spt").and_then(|number| number.parse::<i64>().ok())),
        };
        let Some(preset) = kind.and_then(preset_of) else { return false };
        let line = preset == "line" || preset == "straightConnector1";
        let (width_emu, height_emu) = if line {
            vml_line_size(attribute("from"), attribute("to"))
        } else {
            (width.unwrap_or(0), height.unwrap_or(0))
        };
        let fill = if line || attribute("filled").is_some_and(|value| value.starts_with('f')) {
            Fill::None
        } else {
            let colour =
                attribute("fillcolor").and_then(vml_colour).unwrap_or_else(|| "FFFFFF".to_owned());
            Fill::Solid(Colour::rgb(&colour))
        };
        let stroked = !attribute("stroked").is_some_and(|value| value.starts_with('f'));
        let outline = stroked.then(|| {
            Colour::rgb(
                &attribute("strokecolor")
                    .and_then(vml_colour)
                    .unwrap_or_else(|| "000000".to_owned()),
            )
        });
        let outline_emu = if stroked {
            attribute("strokeweight")
                .and_then(css::twips)
                .map_or(9525, |twips| i64::from(twips) * 635)
        } else {
            0
        };
        let text = element
            .textbox
            .as_deref()
            .map(|html| read_fragment(html, &self.sheet))
            .unwrap_or_default();
        let rotation = declared(&style, "rotation")
            .and_then(|value| value.trim_end_matches("fd").trim().parse::<f64>().ok())
            .map_or(0, |degrees| (degrees * 60_000.0) as i32);
        let flip = declared(&style, "flip").unwrap_or("");
        let shape = Shape {
            preset: preset.to_owned(),
            width_emu,
            height_emu,
            fill,
            outline,
            outline_emu,
            ink: None,
            text,
            name: attribute("alt").map_or_else(|| "Shape".to_owned(), str::to_owned),
            id: self.drawings,
            anchor,
            rotation,
            flipped_across: flip.contains('x'),
            flipped_down: flip.contains('y'),
            ..Shape::default()
        };
        self.content(RunContent::Shape(Box::new(shape)), 1);
        true
    }

    // --- The ends of stories ---------------------------------------------------

    fn finish_story(&mut self) {
        self.end_paragraph();
        self.close_tables();
        if self.stories.len() <= 1 {
            return;
        }
        let Some(story) = self.stories.pop() else { return };
        let mut blocks = story.blocks;
        if blocks.is_empty() {
            blocks.push(Block::Paragraph(Paragraph::default()));
        }
        let body = Body { blocks };
        let pictures = story.pictures;
        match story.kind {
            StoryKind::Main | StoryKind::Dropped => {}
            StoryKind::Note { id, endnote } => {
                self.notes.push(NoteFound { id, endnote, body, pictures });
            }
            StoryKind::Comment { number } => self.comments.push((number, body, pictures)),
            StoryKind::Part { id } => self.furniture.push(FurniturePart { id, body, pictures }),
            StoryKind::Furniture { kind } => {
                self.section_furniture.push(FurnitureFound {
                    kind,
                    which: Which::Default,
                    body,
                    pictures,
                });
            }
        }
    }

    fn finish(mut self) -> Reading {
        while self.stories.len() > 1 {
            self.finish_story();
        }
        self.end_paragraph();
        self.close_tables();
        let main = self.stories.pop().expect("the page's story");
        let mut blocks = main.blocks;
        if blocks.is_empty() {
            blocks.push(Block::Paragraph(Paragraph::default()));
        }
        let last = self.section_found(None);
        self.sections.push(last);

        let comments = core::mem::take(&mut self.comments)
            .into_iter()
            .map(|(number, body, pictures)| {
                let at = TextPosition::default();
                let (start, end) = self.comment_ranges.get(&number).copied().unwrap_or((at, at));
                CommentFound {
                    start,
                    end,
                    // Who wrote it, or at least their initials.
                    author: self
                        .comment_authors
                        .get(&number)
                        .or_else(|| self.comment_initials.get(&number))
                        .cloned()
                        .unwrap_or_default(),
                    date: self.comment_dates.get(&number).cloned().unwrap_or_default(),
                    body,
                    pictures,
                }
            })
            .collect();
        let styles = word_styles(&self.sheet).into_iter().map(|(_, style)| style).collect();
        let fonts = self.sheet.fonts.iter().filter_map(|rule| font_entry(rule)).collect();
        let title = self.title.trim().to_owned();
        Reading {
            body: Body { blocks },
            pictures: main.pictures,
            links: self.links,
            bookmarks: self.bookmarks,
            comments,
            notes: self.notes,
            sections: self.sections,
            furniture: self.furniture,
            styles,
            fonts,
            page_colour: self.page_colour,
            title: (!title.is_empty()).then_some(title),
        }
    }
}

impl TableBuild {
    /// The table as the model has it: each cell where its columns are, the
    /// ones merged down written again under it as continuations, and the
    /// columns' widths from whatever gives them.
    fn into_table(mut self) -> Option<Table> {
        if self.cell.is_some() || self.row.is_some() {
            let _ = Reader::end_row_of(&mut self);
        }
        if self.rows.is_empty() {
            return None;
        }
        // The cells merged down from above, by column: how many columns, how
        // many rows more, and their lines.
        let mut carried: Vec<Option<(u32, u32, TableBorders)>> = Vec::new();
        let mut rows = Vec::new();
        let mut widths: Vec<Option<i32>> = self.columns.clone();
        let mut spanned: Vec<(usize, u32, i32)> = Vec::new();
        for row in self.rows {
            let mut cells = Vec::new();
            let mut column = 0usize;
            let mut pending = row.cells.into_iter();
            loop {
                if let Some(Some((span, left, borders))) = carried.get(column).cloned() {
                    cells.push(TableCell {
                        span,
                        merged_upwards: true,
                        borders: borders.clone(),
                        ..TableCell::default()
                    });
                    carried[column] = (left > 1).then_some((span, left - 1, borders));
                    column += span as usize;
                    continue;
                }
                let Some(cell) = pending.next() else {
                    // Cells merged down past the end of a short row.
                    if carried.iter().skip(column).any(Option::is_some) {
                        column += 1;
                        continue;
                    }
                    break;
                };
                if carried.len() < column + cell.colspan as usize {
                    carried.resize(column + cell.colspan as usize, None);
                }
                if cell.rowspan > 1 {
                    carried[column] = Some((cell.colspan, cell.rowspan - 1, cell.borders.clone()));
                }
                if let Some(width) = cell.width {
                    if cell.colspan == 1 {
                        if widths.len() <= column {
                            widths.resize(column + 1, None);
                        }
                        widths[column] = widths[column].or(Some(width));
                    } else {
                        spanned.push((column, cell.colspan, width));
                    }
                }
                cells.push(TableCell {
                    blocks: cell.blocks,
                    width: cell.width,
                    span: cell.colspan,
                    merged_upwards: false,
                    borders: cell.borders,
                    shading: cell.shading,
                    vertical: cell.vertical.or(row.vertical).unwrap_or_default(),
                    ..TableCell::default()
                });
                column += cell.colspan as usize;
            }
            rows.push(TableRow {
                cells,
                height: row.height,
                height_exact: false,
                is_header: row.header,
            });
        }
        let columns = rows
            .iter()
            .map(|row| row.cells.iter().map(|cell| cell.span as usize).sum::<usize>())
            .max()
            .unwrap_or(0);
        widths.resize(columns, None);
        // A width across several columns shares out among those it covers
        // that nothing else gave one.
        for (first, span, width) in spanned {
            let covered = first..(first + span as usize).min(columns);
            let known: i32 = widths[covered.clone()].iter().flatten().sum();
            let unknown = widths[covered.clone()].iter().filter(|width| width.is_none()).count();
            if unknown > 0 {
                let each = ((width - known) / i32::try_from(unknown).unwrap_or(1)).max(1);
                for slot in &mut widths[covered] {
                    slot.get_or_insert(each);
                }
            }
        }
        self.table.grid = widths.into_iter().map(|width| width.unwrap_or(2880)).collect();
        self.table.rows = rows;
        Some(self.table)
    }
}

/// Reads a piece of a page — the words in a text box — into paragraphs,
/// with the sheet of the page it is in.
fn read_fragment(html: &str, sheet: &Sheet) -> Vec<Paragraph> {
    let mut reader = Reader::new(sheet.clone());
    reader.run(html);
    reader
        .finish()
        .body
        .blocks
        .into_iter()
        .flat_map(|block| match block {
            Block::Paragraph(paragraph) => vec![paragraph],
            Block::Table(table) => table
                .rows
                .into_iter()
                .flat_map(|row| row.cells)
                .flat_map(|cell| cell.blocks)
                .filter_map(|block| match block {
                    Block::Paragraph(paragraph) => Some(paragraph),
                    Block::Table(_) => None,
                })
                .collect(),
        })
        .collect()
}

/// One drawing of a VML block, with what it holds.
#[derive(Debug, Default)]
struct VmlElement {
    tag: String,
    attributes: Vec<(String, String)>,
    /// The picture in it: `v:imagedata`.
    image: Option<String>,
    /// The page inside it, for a text box: `v:textbox`.
    textbox: Option<String>,
    /// How text wraps round it: `w:wrap`.
    wrap: Option<(String, String)>,
}

/// The drawings of a VML block, in order — the ones Word defines for the
/// others to name, `v:shapetype`, and the groups, left out.
fn vml_elements(inner: &str) -> Vec<VmlElement> {
    const DRAWINGS: &[&str] = &["v:shape", "v:rect", "v:roundrect", "v:oval", "v:line"];
    let lower = inner.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut from = 0usize;
    while from < lower.len() {
        let Some((at, tag)) = DRAWINGS
            .iter()
            .filter_map(|tag| {
                let mut search = from;
                // `<v:shape` is also the start of `<v:shapetype`.
                while let Some(found) = lower[search..].find(&format!("<{tag}")) {
                    let at = search + found;
                    let next = lower.as_bytes().get(at + 1 + tag.len()).copied();
                    if next.is_some_and(|byte| {
                        byte.is_ascii_whitespace() || byte == b'>' || byte == b'/'
                    }) {
                        return Some((at, *tag));
                    }
                    search = at + 1;
                }
                None
            })
            .min_by_key(|(at, _)| *at)
        else {
            break;
        };
        // Inside a group: the group is left out whole.
        let group = lower[from..at].rfind("<v:group").map(|found| from + found);
        if let Some(start) = group {
            if lower[start..at].find("</v:group>").is_none() {
                from = lower[at..].find("</v:group>").map_or(lower.len(), |end| at + end + 10);
                continue;
            }
        }
        let open_end = lower[at..].find('>').map_or(lower.len(), |end| at + end + 1);
        let self_closing = lower[..open_end].ends_with("/>");
        let close = format!("</{tag}>");
        let end = if self_closing {
            open_end
        } else {
            lower[open_end..].find(&close).map_or(lower.len(), |found| open_end + found)
        };
        let mut element = VmlElement { tag: (*tag).to_owned(), ..VmlElement::default() };
        let mut tokenizer = Tokenizer::new(&inner[at..open_end]);
        if let Some(Token::Open { attributes, .. }) = tokenizer.next_token() {
            element.attributes = attributes;
        }
        let body = &inner[open_end..end];
        let body_lower = &lower[open_end..end];
        let mut tokens = Tokenizer::new(body);
        while let Some(token) = tokens.next_token() {
            if let Token::Open { name, attributes, .. } = token {
                let get = |wanted: &str| {
                    attributes
                        .iter()
                        .find(|(held, _)| held == wanted)
                        .map(|(_, value)| value.clone())
                };
                match name.as_str() {
                    "v:imagedata" => element.image = get("src"),
                    "w:wrap" => {
                        element.wrap = Some((
                            get("type").unwrap_or_default(),
                            get("side").unwrap_or_default(),
                        ));
                    }
                    _ => {}
                }
            }
        }
        if let Some(start) = body_lower.find("<v:textbox") {
            let inside = body_lower[start..].find('>').map(|end| start + end + 1);
            let finish = body_lower.find("</v:textbox>");
            if let (Some(inside), Some(finish)) = (inside, finish) {
                if finish > inside {
                    element.textbox = Some(body[inside..finish].to_owned());
                }
            }
        }
        out.push(element);
        from = if self_closing { end } else { end + close.len() };
    }
    out
}

/// Where a VML drawing floats: its offsets and what they are measured
/// from, how the text wraps round it, and whether it is behind the text.
fn vml_anchor(style: &[Declaration], wrap: Option<&(String, String)>, number: u32) -> Anchor {
    let length = |name: &str| {
        declared(style, name).and_then(css::twips).map_or(0, |twips| i64::from(twips) * 635)
    };
    let horizontal_from = match declared(style, "mso-position-horizontal-relative") {
        Some("margin") => Relative::Margin,
        Some("page") => Relative::Page,
        Some("char") => Relative::Character,
        Some("left-margin-area") => Relative::LeftMargin,
        Some("right-margin-area") => Relative::RightMargin,
        _ => Relative::Column,
    };
    let vertical_from = match declared(style, "mso-position-vertical-relative") {
        Some("margin") => Relative::Margin,
        Some("page") => Relative::Page,
        Some("line") => Relative::Line,
        Some("top-margin-area") => Relative::TopMargin,
        Some("bottom-margin-area") => Relative::BottomMargin,
        _ => Relative::Paragraph,
    };
    let aligned = |value: Option<&str>| match value {
        Some(word @ ("left" | "center" | "right" | "inside" | "outside" | "top" | "bottom")) => {
            Some(Placement::Aligned(word.to_owned()))
        }
        _ => None,
    };
    let horizontal = aligned(declared(style, "mso-position-horizontal"))
        .unwrap_or_else(|| Placement::Offset(length("margin-left") + length("left")));
    let vertical = aligned(declared(style, "mso-position-vertical"))
        .unwrap_or_else(|| Placement::Offset(length("margin-top") + length("top")));
    let z = declared(style, "z-index").and_then(|value| value.parse::<i64>().ok()).unwrap_or(0);
    let (kind, side) = wrap.cloned().unwrap_or_default();
    let wrap = match kind.as_str() {
        "square" => Wrap::Square,
        "tight" => Wrap::Tight,
        "through" => Wrap::Through,
        "topAndBottom" => Wrap::TopAndBottom,
        "none" => Wrap::None,
        // Word writes no `w:wrap` for a drawing in front of the text, and
        // says square in the style for one wrapped square.
        _ if declared(style, "mso-wrap-style").is_some_and(|value| value == "square")
            && wrap.is_some() =>
        {
            Wrap::Square
        }
        _ => Wrap::None,
    };
    let side = match side.as_str() {
        "left" => WrapSide::Left,
        "right" => WrapSide::Right,
        "largest" => WrapSide::Largest,
        _ => WrapSide::BothSides,
    };
    Anchor {
        wrap,
        side,
        behind_text: z < 0,
        horizontal_from,
        horizontal,
        vertical_from,
        vertical,
        depth: USUAL_DEPTH.saturating_add(number),
        ..Anchor::default()
    }
}

/// How long and how tall a line from one point to another is.
fn vml_line_size(from: Option<&str>, to: Option<&str>) -> (i64, i64) {
    let point = |value: Option<&str>| {
        let (x, y) = value?.split_once(',')?;
        Some((i64::from(css::twips(x)?) * 635, i64::from(css::twips(y)?) * 635))
    };
    match (point(from), point(to)) {
        (Some((x1, y1)), Some((x2, y2))) => ((x2 - x1).abs(), (y2 - y1).abs()),
        _ => (0, 0),
    }
}

/// A VML colour: a name, `#rrggbb`, or `#rgb`, with Word's note of where it
/// came from after it — `#4472c4 [3204]`.
fn vml_colour(value: &str) -> Option<String> {
    css::colour(value.split_whitespace().next()?)
}

/// The shape a VML type's number names, as the model names it. What has no
/// name here — a freeform, WordArt — is not drawn.
fn preset_of(kind: i64) -> Option<&'static str> {
    Some(match kind {
        1 => "rect",
        2 => "roundRect",
        3 => "ellipse",
        4 => "diamond",
        5 => "triangle",
        6 => "rtTriangle",
        7 => "parallelogram",
        8 => "trapezoid",
        9 => "hexagon",
        10 => "octagon",
        11 => "plus",
        12 => "star5",
        13 => "rightArrow",
        15 => "homePlate",
        16 => "cube",
        20 => "line",
        21 => "plaque",
        22 => "can",
        23 => "donut",
        32 => "straightConnector1",
        55 => "chevron",
        56 => "pentagon",
        58 => "star8",
        66 => "leftArrow",
        67 => "downArrow",
        68 => "upArrow",
        69 => "leftRightArrow",
        70 => "upDownArrow",
        73 => "lightningBolt",
        74 => "heart",
        96 => "smileyFace",
        183 => "sun",
        184 => "moon",
        202 => "rect",
        _ => return None,
    })
}

/// A style as the sheet has it: the class that names it, the style, and the
/// names of the style it is based on and of the one that follows it.
type StyleRule = (Option<String>, StyleFound, Option<String>, Option<String>);

/// The styles a sheet describes as Word's, with the class that names each:
/// every class whose name Word gives its styles, `MsoTitle`, or that says
/// what it is called, `mso-style-name`, and the headings by their tags.
fn word_styles(sheet: &Sheet) -> Vec<(Option<String>, StyleFound)> {
    let mut found: Vec<StyleRule> = Vec::new();
    for rule in sheet.rules() {
        let named = declared(&rule.declarations, "mso-style-name").map(css::unquote);
        let (kind, id, class) = match (&rule.tag, &rule.class) {
            (Some(tag), None) if tag.len() == 2 && tag.starts_with('h') => {
                let level = &tag[1..];
                if !level.chars().all(|c| c.is_ascii_digit()) {
                    continue;
                }
                (StyleKind::Paragraph, format!("Heading{level}"), None)
            }
            (tag, Some(class)) => {
                let word = class.starts_with("Mso") || named.is_some();
                if !word || matches!(class.as_str(), "MsoChpDefault" | "MsoPapDefault") {
                    continue;
                }
                let kind = match tag.as_deref() {
                    Some("span" | "a") => StyleKind::Character,
                    Some("table") => StyleKind::Table,
                    Some("p" | "li" | "div" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6") | None => {
                        StyleKind::Paragraph
                    }
                    Some(_) => continue,
                };
                (kind, class_style_id(class), Some(class.clone()))
            }
            _ => continue,
        };
        if found.iter().any(|(_, style, _, _)| style.id == id) {
            continue;
        }
        let name = named.clone().unwrap_or_else(|| built_in_name(&id));
        let mut paragraph = ParagraphProperties::default();
        let mut run = RunProperties::default();
        if kind != StyleKind::Character {
            apply_paragraph_declarations(&mut paragraph, &rule.declarations, sheet);
            paragraph.borders = paragraph_borders_from(&rule.declarations);
        }
        apply_character_declarations(&mut run, &rule.declarations, kind == StyleKind::Character);
        let table_borders = if kind == StyleKind::Table {
            table_borders_from(&rule.declarations)
        } else {
            TableBorders::default()
        };
        let parent = declared(&rule.declarations, "mso-style-parent").map(css::unquote);
        let next = declared(&rule.declarations, "mso-style-next").map(css::unquote);
        found.push((
            class,
            StyleFound {
                id,
                name,
                kind,
                based_on: None,
                next: None,
                paragraph,
                run,
                table_borders,
            },
            parent,
            next,
        ));
    }
    // What they are based on and followed by, by name.
    let by_name: HashMap<String, String> = found
        .iter()
        .map(|(_, style, _, _)| (style.name.to_lowercase(), style.id.clone()))
        .collect();
    found
        .into_iter()
        .map(|(class, mut style, parent, next)| {
            let find = |name: Option<String>| {
                name.filter(|name| !name.is_empty())
                    .and_then(|name| by_name.get(&name.to_lowercase()).cloned())
            };
            style.based_on = find(parent).filter(|id| *id != style.id);
            style.next = find(next);
            (class, style)
        })
        .collect()
}

/// The identifier a class names a style by: Word's own for its built-in
/// ones, and the class itself for the rest.
fn class_style_id(class: &str) -> String {
    match class {
        "MsoNormal" => "Normal".to_owned(),
        "MsoNormalTable" => "TableNormal".to_owned(),
        _ => class.strip_prefix("Mso").unwrap_or(class).to_owned(),
    }
}

/// The name of one of Word's built-in styles by its identifier: `Heading1`
/// is "heading 1", `ListParagraph` is "List Paragraph".
fn built_in_name(id: &str) -> String {
    if let Some(level) = id.strip_prefix("Heading") {
        return format!("heading {level}");
    }
    match id {
        "TableNormal" => return "Normal Table".to_owned(),
        "TableGrid" => return "Table Grid".to_owned(),
        _ => {}
    }
    let mut name = String::new();
    for (index, character) in id.chars().enumerate() {
        if index > 0 && character.is_uppercase() {
            name.push(' ');
        }
        name.push(character);
    }
    name
}

/// What an `@font-face` rule of Word's says about a font.
fn font_entry(declarations: &[Declaration]) -> Option<FontEntry> {
    let name = declared(declarations, "font-family").map(css::unquote)?;
    if name.is_empty() {
        return None;
    }
    let panose = declared(declarations, "panose-1").and_then(|numbers| {
        let bytes: Vec<u8> =
            numbers.split_whitespace().filter_map(|number| number.parse().ok()).collect();
        (bytes.len() == 10).then(|| bytes.iter().map(|byte| format!("{byte:02X}")).collect())
    });
    Some(FontEntry {
        name,
        alt_name: declared(declarations, "mso-font-alt")
            .map(css::unquote)
            .and_then(|names| names.split(',').next().map(css::unquote))
            .filter(|first| !first.is_empty()),
        panose,
        charset: declared(declarations, "mso-font-charset").and_then(|value| value.parse().ok()),
        class: declared(declarations, "mso-generic-font-family")
            .map_or(FontClass::Auto, FontClass::from_word),
        fixed_pitch: declared(declarations, "mso-font-pitch").and_then(|pitch| match pitch {
            "fixed" => Some(true),
            "variable" => Some(false),
            _ => None,
        }),
    })
}

/// A section's page from its `@page` rule.
fn page_setup(declarations: &[Declaration]) -> PageSetup {
    let mut setup = PageSetup::default();
    if let Some(size) = declared(declarations, "size") {
        let lengths: Vec<i32> = size.split_whitespace().filter_map(css::twips).collect();
        if let [width, height] = lengths[..] {
            setup.width = Some(width);
            setup.height = Some(height);
        }
    }
    if declared(declarations, "mso-page-orientation").is_some_and(|value| value == "landscape") {
        if let (Some(width), Some(height)) = (setup.width, setup.height) {
            if width < height {
                setup.width = Some(height);
                setup.height = Some(width);
            }
        }
    }
    for (name, value) in declarations {
        match name.as_str() {
            "margin" => {
                let parts: Vec<Option<i32>> = value.split_whitespace().map(css::twips).collect();
                setup.margins = match parts.as_slice() {
                    [all] => [*all; 4],
                    [vertical, horizontal] => [*vertical, *horizontal, *vertical, *horizontal],
                    [top, horizontal, bottom] => [*top, *horizontal, *bottom, *horizontal],
                    [top, right, bottom, left, ..] => [*top, *right, *bottom, *left],
                    [] => setup.margins,
                };
            }
            "margin-top" => setup.margins[0] = css::twips(value),
            "margin-right" => setup.margins[1] = css::twips(value),
            "margin-bottom" => setup.margins[2] = css::twips(value),
            "margin-left" => setup.margins[3] = css::twips(value),
            "mso-header-margin" => setup.header_distance = css::twips(value),
            "mso-footer-margin" => setup.footer_distance = css::twips(value),
            "mso-title-page" => setup.title_page = value == "yes",
            "mso-columns" => {
                // `2 even .5in`: how many, whether alike, and the gap.
                let mut parts = value.split_whitespace();
                let count = parts.next().and_then(|count| count.parse::<usize>().ok());
                let gap = parts.nth(1).and_then(css::twips).unwrap_or(720);
                setup.columns = count.filter(|count| *count > 1).map(|count| (count, gap));
            }
            _ => {}
        }
    }
    setup
}

/// The headers and footers an `@page` rule names in the file beside the page.
fn furniture_links(declarations: &[Declaration]) -> Vec<FurnitureLink> {
    let mut out = Vec::new();
    for (name, value) in declarations {
        let (kind, which) = match name.as_str() {
            "mso-header" => (Furniture::Header, Which::Default),
            "mso-footer" => (Furniture::Footer, Which::Default),
            "mso-first-header" => (Furniture::Header, Which::First),
            "mso-first-footer" => (Furniture::Footer, Which::First),
            "mso-even-header" => (Furniture::Header, Which::Even),
            "mso-even-footer" => (Furniture::Footer, Which::Even),
            _ => continue,
        };
        // `url("page_files/header.htm") h1`
        let Some(inside) = value.split_once("url(").map(|(_, rest)| rest) else { continue };
        let Some((url, rest)) = inside.split_once(')') else { continue };
        let id = rest.trim().to_owned();
        if id.is_empty() {
            continue;
        }
        out.push(FurnitureLink { kind, which, url: css::unquote(url), id });
    }
    out
}

/// A note's mark's name, from where it leads: `#_ftn1`, `#sdfootnote1sym`.
fn note_key(href: &str) -> Option<String> {
    let target = href.strip_prefix('#')?.trim_start_matches('_');
    if let Some(key) = target.strip_suffix("sym") {
        return (key.starts_with("sdfootnote") || key.starts_with("sdendnote"))
            .then(|| key.to_owned());
    }
    let digits = target.strip_prefix("ftn").or_else(|| target.strip_prefix("edn"))?;
    (!digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit())).then(|| target.to_owned())
}

/// Whether a link leads back from a note to its mark: `#_ftnref1`.
fn note_back(href: &str) -> bool {
    let target = href.trim_start_matches('#').trim_start_matches('_');
    target.starts_with("ftnref")
        || target.starts_with("ednref")
        || (target.starts_with("sdfootnote") || target.starts_with("sdendnote"))
            && target.ends_with("anc")
}

/// Whether a name is a bookmark's, rather than one of the anchors notes and
/// comments are joined up with.
fn is_bookmark_name(name: &str) -> bool {
    const INTERNAL: &[&str] =
        &["_ftn", "_edn", "_msocom", "_msoanchor", "_com_", "sdfootnote", "sdendnote"];
    !name.is_empty() && !INTERNAL.iter().any(|prefix| name.starts_with(prefix))
}

/// Word's date on a comment, `20240305T1030`, as an ISO 8601 timestamp.
fn comment_date(value: &str) -> String {
    let digits: String = value.chars().filter(char::is_ascii_digit).collect();
    if digits.len() < 12 {
        return String::new();
    }
    format!(
        "{}-{}-{}T{}:{}:00Z",
        &digits[0..4],
        &digits[4..6],
        &digits[6..8],
        &digits[8..10],
        &digits[10..12]
    )
}

/// A language as the model names it: `EN-US` is `en-US`.
fn language_tag(lang: &str) -> Option<String> {
    let mut parts = lang.trim().split('-');
    let language = parts.next()?.to_ascii_lowercase();
    if language.is_empty() || !language.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let rest: Vec<String> = parts.map(str::to_ascii_uppercase).collect();
    Some(if rest.is_empty() { language } else { format!("{language}-{}", rest.join("-")) })
}

/// What CSS says last about one property.
fn declared<'a>(declarations: &'a [Declaration], name: &str) -> Option<&'a str> {
    declarations.iter().rev().find(|(held, _)| held == name).map(|(_, value)| value.as_str())
}

/// An alignment as `align` gives it.
fn alignment_of(value: &str) -> Option<Alignment> {
    match value.trim().to_ascii_lowercase().as_str() {
        "center" | "middle" => Some(Alignment::Center),
        "right" => Some(Alignment::End),
        "justify" => Some(Alignment::Both),
        "left" => Some(Alignment::Start),
        _ => None,
    }
}

/// Where the text sits down a cell: `valign`, `vertical-align`.
fn cell_alignment(value: &str) -> Option<CellAlignment> {
    match value.trim().to_ascii_lowercase().as_str() {
        "top" => Some(CellAlignment::Top),
        "middle" | "center" => Some(CellAlignment::Middle),
        "bottom" => Some(CellAlignment::Bottom),
        _ => None,
    }
}

/// One line as CSS writes it: `solid windowtext 1.0pt`, `1px solid #000`,
/// `none`. Nothing for a value that says nothing about a line.
fn border_of(value: &str) -> Option<Border> {
    let mut style = None;
    let mut size = None;
    let mut colour = None;
    for word in value.split_whitespace() {
        let lower = word.to_ascii_lowercase();
        match lower.as_str() {
            "none" | "hidden" => style = Some("none"),
            "solid" => style = Some("single"),
            "double" => style = Some("double"),
            "dotted" => style = Some("dotted"),
            "dashed" => style = Some("dashed"),
            "groove" => style = Some("threeDEngrave"),
            "ridge" => style = Some("threeDEmboss"),
            "inset" => style = Some("inset"),
            "outset" => style = Some("outset"),
            "thin" => size = Some(4),
            "medium" => size = Some(12),
            "thick" => size = Some(18),
            _ => {
                if let Some(twips) = css::twips(word)
                    .filter(|_| word.chars().next().is_some_and(|c| c.is_ascii_digit() || c == '.'))
                {
                    // Eighths of a point, from twentieths.
                    size = Some(u32::try_from((twips * 2 / 5).max(2)).unwrap_or(2));
                } else if let Some(hex) = css::colour(word) {
                    colour = Some(hex);
                }
            }
        }
    }
    let style = style?;
    Some(Border::line(style, size.unwrap_or(4), colour.as_deref()))
}

/// The four sides CSS gives an element, each from the precise value Word
/// writes for Office — `mso-border-alt` — over the rounded one it writes for
/// browsers.
fn sides_from(declarations: &[Declaration]) -> [Option<Border>; 4] {
    let mut sides: [Option<Border>; 4] = [None, None, None, None];
    let names = ["top", "right", "bottom", "left"];
    for (name, value) in declarations {
        let apply = |sides: &mut [Option<Border>; 4], which: Option<usize>| {
            let Some(border) = border_of(value) else { return };
            match which {
                Some(index) => sides[index] = Some(border),
                None => {
                    for side in sides.iter_mut() {
                        *side = Some(border.clone());
                    }
                }
            }
        };
        match name.as_str() {
            "border" => apply(&mut sides, None),
            other => {
                if let Some(side) = other.strip_prefix("border-") {
                    if let Some(index) = names.iter().position(|held| *held == side) {
                        apply(&mut sides, Some(index));
                    }
                }
            }
        }
    }
    // The precise ones, after, so that they hold.
    for (name, value) in declarations {
        let which = match name.as_str() {
            "mso-border-alt" => None,
            other => {
                match other.strip_prefix("mso-border-").and_then(|rest| rest.strip_suffix("-alt")) {
                    Some(side) => match names.iter().position(|held| *held == side) {
                        Some(index) => Some(index),
                        None => continue,
                    },
                    None => continue,
                }
            }
        };
        let Some(border) = border_of(value) else { continue };
        match which {
            Some(index) => sides[index] = Some(border),
            None => {
                for side in &mut sides {
                    *side = Some(border.clone());
                }
            }
        }
    }
    // A side that says it has none has none.
    sides.map(|side| side.filter(Border::is_visible))
}

fn paragraph_borders_from(declarations: &[Declaration]) -> ParagraphBorders {
    let [top, right, bottom, left] = sides_from(declarations);
    ParagraphBorders { top, start: left, bottom, end: right, between: None }
}

fn table_borders_from(declarations: &[Declaration]) -> TableBorders {
    let [top, right, bottom, left] = sides_from(declarations);
    let inside =
        |name: &str| declared(declarations, name).and_then(border_of).filter(Border::is_visible);
    TableBorders {
        top,
        start: left,
        bottom,
        end: right,
        inside_horizontal: inside("mso-border-insideh"),
        inside_vertical: inside("mso-border-insidev"),
    }
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

/// Tags whose CSS is about a block rather than the characters in it: their
/// `background` is the paragraph's or the cell's, not a highlight.
fn is_block_tag(name: &str) -> bool {
    is_block(name)
        || matches!(
            name,
            "body" | "html" | "table" | "tr" | "td" | "th" | "thead" | "tbody" | "ul" | "ol"
        )
}

/// What CSS says about the characters. The background of a block is not a
/// highlight, so it is only read for the characters of an inline element.
fn apply_character_declarations(
    chars: &mut RunProperties,
    declarations: &[Declaration],
    inline: bool,
) {
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
            "background" | "background-color" if inline => {
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
    declarations: &[Declaration],
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
            "direction" => {
                properties.right_to_left = match value {
                    "rtl" => Some(true),
                    _ => None,
                };
            }
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
