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
//! this reader does not carry: the metadata, the theme. A group is a
//! *destination*, and which destination a group is decides what its text
//! means. A group beginning `\*` that names a destination this reader does
//! not know is skipped whole, which is what `\*` is for: Word writes twenty
//! such groups at the top of every file, and a reader that stopped at the
//! first unknown one would read nothing.
//!
//! # Stories
//!
//! The document's own text is not the only text in the file. A header, a
//! footnote, a comment and the words in a text box are each written where they
//! belong — the footnote in the middle of the sentence that points at it — as
//! a group of their own, and each is a *story*: paragraphs and tables of its
//! own, read exactly as the document's are and put somewhere else when its
//! group ends. The reader keeps a stack of them, and text goes into the one on
//! top.
//!
//! # Tables
//!
//! A table in RTF is paragraphs marked as being in one, with a mark at the end
//! of each cell and each row, and a description of the row — where each cell's
//! right edge is, how it is merged, bordered and shaded — before the row or
//! after it, or both. A table inside a cell is the same again one level
//! deeper: `\itap` says how deep a paragraph is, `\nestcell` and `\nestrow`
//! end the inner table's cells and rows, and the inner row's description comes
//! last, in a group of its own. So each level keeps the description it was
//! last given and uses it when the row ends, and the columns of a table are
//! worked out once it is finished, from every edge any of its rows has.
//!
//! # What comes out
//!
//! A [`Body`] of paragraphs and tables — with the notes' marks, the fields,
//! the tracked changes and the drawings already in its runs — and beside it
//! what needs a document to be put into: the pictures, the links, the
//! bookmarks, the comments, the notes' words, the sections with their headers
//! and footers and page setup, and the styles. See [`crate::open`].

use wp_docx::anchor::{Anchor, Placement, Relative, Wrap, WrapSide, USUAL_DEPTH};
use wp_docx::colour::Colour;
use wp_docx::fills::Fill;
use wp_docx::furniture::{Furniture, Which};
use wp_docx::model::{
    Alignment, Block, Body, Border, BreakKind, FormatChange, LineRule, LineSpacing,
    NumberingReference, Paragraph, ParagraphBorders, ParagraphProperties, Revision, RevisionKind,
    Run, RunContent, RunProperties, TabAlignment, TabLeader, TabStop, Table, TableBorders,
    TableCell, TableRow, Underline, VerticalAlignment,
};
use wp_docx::sections::{NumberFormat, PageNumbering, Start};
use wp_docx::shapes::Shape;
use wp_docx::table_properties::CellAlignment;
use wp_docx::{StyleKind, TextPosition};
use wp_text::Encoding;

use crate::lexer::{Lexer, Token};

/// The character that stands where a picture goes in the text, until the
/// picture is put in.
pub const PICTURE_MARK: char = '\u{FFFC}';

/// What was read.
///
/// Every place in it — a picture's, a link's, a bookmark's, a comment's — is
/// counted the way the document will count it once the pictures are in: a
/// picture, a note's mark and a drawing are one character each, and deleted
/// text is no characters at all.
#[derive(Debug, Default)]
pub struct Reading {
    pub body: Body,
    /// The pictures, each with where it goes: the paragraph, counted through
    /// the whole document in order, and the offset in it of the mark that
    /// stands for it.
    pub pictures: Vec<PictureFound>,
    /// The links, each with the paragraph and the range of text it covers.
    pub links: Vec<LinkFound>,
    pub bookmarks: Vec<BookmarkFound>,
    pub comments: Vec<CommentFound>,
    /// What the notes say, for the marks already in the body.
    pub notes: Vec<NoteFound>,
    /// The sections, in order; there is always at least one.
    pub sections: Vec<SectionFound>,
    /// Whether left-hand and right-hand pages have headers and footers of
    /// their own: `\facingp`.
    pub facing_pages: bool,
    /// Every style the stylesheet describes, with its formatting.
    pub styles: Vec<StyleFound>,
}

#[derive(Debug)]
pub struct PictureFound {
    pub paragraph: usize,
    pub offset: usize,
    pub bytes: Vec<u8>,
    /// `png`, `jpeg`, `emf`, `wmf` or `bmp`.
    pub extension: &'static str,
    pub width_emu: i64,
    pub height_emu: i64,
    /// Where it floats, for a picture that is a drawing on the page rather
    /// than a character in the line.
    pub anchor: Option<Anchor>,
}

#[derive(Debug)]
pub struct LinkFound {
    pub paragraph: usize,
    pub start: usize,
    pub end: usize,
    /// An address, or `#` and a bookmark's name for a place in the document.
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
    /// An ISO 8601 timestamp, or nothing where the file gave no date.
    pub date: String,
    pub body: Body,
    /// The pictures in it, placed in its own paragraphs.
    pub pictures: Vec<PictureFound>,
}

#[derive(Debug)]
pub struct NoteFound {
    /// The number its mark in the body carries.
    pub id: i32,
    pub endnote: bool,
    pub body: Body,
    /// The pictures in it, placed in its own paragraphs.
    pub pictures: Vec<PictureFound>,
}

/// One of a section's headers or footers.
#[derive(Debug)]
pub struct FurnitureFound {
    pub kind: Furniture,
    pub which: Which,
    pub body: Body,
    /// The pictures in it, placed in its own paragraphs.
    pub pictures: Vec<PictureFound>,
}

#[derive(Debug, Default)]
pub struct SectionFound {
    /// The paragraph it ends with, counted through the whole document; the
    /// last section ends with the document and has none.
    pub last_paragraph: Option<usize>,
    pub page: PageSetup,
    /// Its own headers and footers. One it does not have follows the section
    /// before it, as in Word.
    pub furniture: Vec<FurnitureFound>,
}

/// How a section's pages are set up, as far as the file says: whatever it
/// leaves out is left as the document has it.
#[derive(Clone, Debug, Default)]
pub struct PageSetup {
    pub width: Option<i32>,
    pub height: Option<i32>,
    /// Top, right, bottom and left.
    pub margins: [Option<i32>; 4],
    pub landscape: Option<bool>,
    pub header_distance: Option<i32>,
    pub footer_distance: Option<i32>,
    pub columns: Option<usize>,
    pub column_gap: Option<i32>,
    /// How the section begins.
    pub start: Option<Start>,
    /// Whether its first page has a header and footer of its own.
    pub title_page: Option<bool>,
    pub numbering: Option<PageNumbering>,
}

impl PageSetup {
    /// This section's setup, with the document's wherever the section says
    /// nothing of its own.
    fn over(&self, under: &Self) -> Self {
        let mut margins = self.margins;
        for (mine, theirs) in margins.iter_mut().zip(under.margins) {
            *mine = mine.or(theirs);
        }
        Self {
            width: self.width.or(under.width),
            height: self.height.or(under.height),
            margins,
            landscape: self.landscape.or(under.landscape),
            header_distance: self.header_distance.or(under.header_distance),
            footer_distance: self.footer_distance.or(under.footer_distance),
            columns: self.columns.or(under.columns),
            column_gap: self.column_gap.or(under.column_gap),
            start: self.start.or(under.start),
            title_page: self.title_page.or(under.title_page),
            numbering: self.numbering.or(under.numbering),
        }
    }

    /// The paper, width first, turned the way the section says.
    #[must_use]
    pub fn size(&self) -> Option<(i32, i32)> {
        let (width, height) = (self.width?, self.height?);
        let turned = self.landscape == Some(true) && width < height;
        Some(if turned { (height, width) } else { (width, height) })
    }
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

/// What a group is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Destination {
    /// Text, of the document or of one of the stories beside it.
    Body,
    FontTable,
    ColourTable,
    StyleSheet,
    ListTable,
    ListOverrideTable,
    Picture,
    /// The instruction of a field: `HYPERLINK "..."`, `PAGE`.
    FieldInstruction,
    /// A group whose text is a name or a number something else needs.
    Tag(Tag),
    /// The authors of the tracked changes, in the order they are numbered.
    RevisionTable,
    /// A drawing's description: where it is, and its properties by name.
    ShapeInstance,
    /// The name of one of a drawing's properties, and its value.
    ShapeName,
    ShapeValue,
    /// Anything this reader does not carry.
    Skip,
}

/// Which name or number a [`Destination::Tag`] holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tag {
    BookmarkStart,
    BookmarkEnd,
    CommentStart,
    CommentEnd,
    CommentAuthor,
    CommentReference,
    CommentDate,
}

/// Whether the text is somebody's tracked insertion or deletion, or has had
/// its formatting changed, whose, and when: the author is a number into the
/// revision table and the date is Word's packed one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Marks {
    inserted: bool,
    inserted_by: usize,
    inserted_at: i32,
    deleted: bool,
    deleted_by: usize,
    deleted_at: i32,
    formatted: bool,
    formatted_by: usize,
    formatted_at: i32,
}

/// The colour behind something, as RTF describes it: a background, a
/// foreground, and how much of the foreground shows.
#[derive(Clone, Debug, Default)]
struct Shading {
    background: Option<String>,
    foreground: Option<String>,
    /// In hundredths of a percent.
    percent: Option<i32>,
}

impl Shading {
    /// The one colour the model keeps: the background, or the foreground
    /// laid over it as far as the percentage says.
    fn fill(&self) -> Option<String> {
        let Some(percent) = self.percent.filter(|percent| *percent > 0) else {
            return self.background.clone();
        };
        let foreground = self.foreground.as_deref().unwrap_or("000000");
        if percent >= 10_000 {
            return Some(foreground.to_owned());
        }
        let background = self.background.as_deref().unwrap_or("FFFFFF");
        let channel = |hex: &str, at: usize| {
            hex.get(at..at + 2).and_then(|pair| i32::from_str_radix(pair, 16).ok()).unwrap_or(0)
        };
        let mixed: String = [0, 2, 4]
            .iter()
            .map(|&at| {
                let under = channel(background, at);
                let over = channel(foreground, at);
                format!("{:02X}", under + (over - under) * percent / 10_000)
            })
            .collect();
        Some(mixed)
    }
}

/// What is inherited from group to group.
#[derive(Clone, Debug)]
struct State {
    destination: Destination,
    chars: RunProperties,
    paragraph: ParagraphProperties,
    /// The paragraph's shading, until the paragraph ends.
    shading: Shading,
    /// Its indents from the left and the right of the page: `\li` and `\ri`,
    /// which are the start and end ones only in text that runs left to right.
    sides: (Option<i32>, Option<i32>),
    /// Which font of the table is in force, for its code page.
    font: Option<i32>,
    /// How many characters follow a `\u` as its stand-in: `\uc`.
    uc: usize,
    /// How many of them are still to be skipped after the last `\u`.
    skip: usize,
    /// Whether the paragraph is in a table, and how deep: `\intbl`, `\itap`.
    in_table: bool,
    itap: usize,
    /// Whether this group describes the row of a table inside a table:
    /// `\nesttableprops`.
    nested_properties: bool,
    /// The tab stop being described: its alignment and leader, until `\tx`
    /// says where it is.
    tab: (TabAlignment, TabLeader),
    marks: Marks,
    /// What the formatting was before a tracked change to it: `\oldcprops`.
    before: Option<RunProperties>,
    /// Whether this group is that description.
    old_properties: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            destination: Destination::Body,
            chars: RunProperties::default(),
            paragraph: ParagraphProperties::default(),
            shading: Shading::default(),
            sides: (None, None),
            font: None,
            uc: 1,
            skip: 0,
            in_table: false,
            itap: 0,
            nested_properties: false,
            tab: (TabAlignment::Start, TabLeader::None),
            marks: Marks::default(),
            before: None,
            old_properties: false,
        }
    }
}

/// One font of the font table.
#[derive(Clone, Debug, Default)]
struct Font {
    name: String,
    /// The code page its charset names, where it names one.
    code_page: Option<u16>,
    /// The bytes of its name not yet read: a name may be in its own
    /// character set, two bytes a character.
    name_bytes: Vec<u8>,
}

impl Font {
    /// The name's bytes so far read in the font's own code page, or the
    /// document's.
    fn finish_name(&mut self, document: Encoding) {
        if self.name_bytes.is_empty() {
            return;
        }
        let page = self
            .code_page
            .and_then(|number| Encoding::code_page(u32::from(number)))
            .unwrap_or(document);
        let bytes = core::mem::take(&mut self.name_bytes);
        self.name.push_str(&page.decode(&bytes));
    }
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

/// One entry of the stylesheet.
#[derive(Clone, Debug)]
struct StyleEntry {
    index: i32,
    kind: StyleKind,
    name: String,
    /// Bytes of the name not yet decoded.
    name_bytes: Vec<u8>,
    id: String,
    based_on: Option<i32>,
    next: Option<i32>,
    /// The list and level it puts a paragraph in: `\ls` and `\ilvl`, kept as
    /// numbers because the list table comes after the stylesheet.
    list: Option<i32>,
    level: u8,
    paragraph: ParagraphProperties,
    run: RunProperties,
    table_borders: TableBorders,
}

impl Default for StyleEntry {
    fn default() -> Self {
        Self {
            index: 0,
            kind: StyleKind::Paragraph,
            name: String::new(),
            name_bytes: Vec::new(),
            id: String::new(),
            based_on: None,
            next: None,
            list: None,
            level: 0,
            paragraph: ParagraphProperties::default(),
            run: RunProperties::default(),
            table_borders: TableBorders::default(),
        }
    }
}

/// The kinds of picture a file can hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PictureKind {
    Png,
    Jpeg,
    Emf,
    Wmf,
    /// A device-independent bitmap: a `.bmp` without its file header.
    Dib,
    /// One this reader cannot show.
    Other,
}

/// A picture being read.
#[derive(Debug)]
struct PictureBeingRead {
    kind: Option<PictureKind>,
    hex: Vec<u8>,
    /// The bytes given as they are, after `\bin`.
    raw: Vec<u8>,
    width_twips: Option<i64>,
    height_twips: Option<i64>,
    width_pixels: Option<i64>,
    height_pixels: Option<i64>,
    /// How far it is scaled, in percent.
    scale: (i64, i64),
}

impl Default for PictureBeingRead {
    fn default() -> Self {
        Self {
            kind: None,
            hex: Vec::new(),
            raw: Vec::new(),
            width_twips: None,
            height_twips: None,
            width_pixels: None,
            height_pixels: None,
            scale: (100, 100),
        }
    }
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
    /// Which story it is in.
    story: usize,
    /// Whether its result went on past the end of a paragraph, which a field
    /// in the model cannot.
    spans: bool,
}

/// A drawing being read: `\shp`.
#[derive(Debug, Default)]
struct ShapeBeingRead {
    depth: usize,
    /// Left, top, right and bottom, in twips.
    bounds: [i32; 4],
    /// What the position is measured from, across and down, where the file
    /// says it in control words rather than properties.
    across: Option<Relative>,
    down: Option<Relative>,
    wrap: i32,
    wrap_side: i32,
    behind: bool,
    z: i32,
    id: u32,
    /// Its properties, by name, as the file gives them.
    properties: Vec<(String, String)>,
    /// The name of the property being read.
    name: String,
    /// The words in it, for a text box.
    text: Vec<Paragraph>,
    /// The picture it frames.
    picture: Option<(Vec<u8>, &'static str)>,
}

impl ShapeBeingRead {
    fn property(&self, name: &str) -> Option<&str> {
        self.properties.iter().rev().find(|(found, _)| found == name).map(|(_, value)| value.trim())
    }

    fn number(&self, name: &str) -> Option<i64> {
        self.property(name).and_then(|value| value.parse().ok())
    }

    /// Where it floats.
    fn anchor(&self) -> Anchor {
        let horizontal_from = self.across.unwrap_or(match self.number("posrelh") {
            Some(0) => Relative::Margin,
            Some(1) => Relative::Page,
            Some(3) => Relative::Character,
            Some(4) => Relative::LeftMargin,
            Some(5) => Relative::RightMargin,
            Some(6) => Relative::InsideMargin,
            Some(7) => Relative::OutsideMargin,
            _ => Relative::Column,
        });
        let vertical_from = self.down.unwrap_or(match self.number("posrelv") {
            Some(0) => Relative::Margin,
            Some(1) => Relative::Page,
            Some(3) => Relative::Line,
            Some(4) => Relative::TopMargin,
            Some(5) => Relative::BottomMargin,
            Some(6) => Relative::InsideMargin,
            Some(7) => Relative::OutsideMargin,
            _ => Relative::Paragraph,
        });
        let aligned = |word: &str| Placement::Aligned(word.to_owned());
        let horizontal = match self.number("posh") {
            Some(1) => aligned("left"),
            Some(2) => aligned("center"),
            Some(3) => aligned("right"),
            Some(4) => aligned("inside"),
            Some(5) => aligned("outside"),
            _ => Placement::Offset(i64::from(self.bounds[0]) * EMU_PER_TWIP),
        };
        let vertical = match self.number("posv") {
            Some(1) => aligned("top"),
            Some(2) => aligned("center"),
            Some(3) => aligned("bottom"),
            Some(4) => aligned("inside"),
            Some(5) => aligned("outside"),
            _ => Placement::Offset(i64::from(self.bounds[1]) * EMU_PER_TWIP),
        };
        Anchor {
            wrap: match self.wrap {
                1 => Wrap::TopAndBottom,
                2 => Wrap::Square,
                4 => Wrap::Tight,
                5 => Wrap::Through,
                _ => Wrap::None,
            },
            side: match self.wrap_side {
                1 => WrapSide::Left,
                2 => WrapSide::Right,
                3 => WrapSide::Largest,
                _ => WrapSide::BothSides,
            },
            behind_text: self.behind || self.number("fBehindDocument") == Some(1),
            horizontal_from,
            horizontal,
            vertical_from,
            vertical,
            depth: USUAL_DEPTH.saturating_add(u32::try_from(self.z).unwrap_or(0)),
            ..Anchor::default()
        }
    }

    /// A colour property, which the file writes as a number with red in the
    /// lowest byte. Numbers past three bytes name a scheme or system colour,
    /// which is not one this can say.
    fn colour(&self, name: &str) -> Option<String> {
        let value = self.number(name).filter(|value| (0..0x0100_0000).contains(value))?;
        Some(format!("{:02X}{:02X}{:02X}", value & 0xFF, (value >> 8) & 0xFF, (value >> 16) & 0xFF))
    }
}

/// English metric units in a twip.
const EMU_PER_TWIP: i64 = 635;

/// How wide a cell is taken to be when the file never said.
const USUAL_CELL: i32 = 2880;

/// What a story is, which decides where it goes when it ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StoryKind {
    /// The document's own text.
    Main,
    Furniture(Furniture, Which),
    /// A note, whose mark is the run at this index of the document's runs.
    Note {
        reference: usize,
        endnote: bool,
    },
    /// A comment, anchored here unless the file gives it a range.
    Comment {
        at: TextPosition,
    },
    /// The words in a text box.
    TextBox,
}

/// Text being read into one place.
#[derive(Debug)]
struct Story {
    kind: StoryKind,
    /// How deep the stack was when its group opened: it ends with that group.
    depth: usize,
    blocks: Vec<Block>,
    runs: Vec<Run>,
    /// How long the paragraph being built is so far, as the document counts.
    paragraph_length: usize,
    /// How many paragraphs have been finished, tables' included.
    paragraphs_done: usize,
    /// The tables being built, outermost first.
    levels: Vec<Level>,
    /// The row each level was last described as having.
    definitions: Vec<RowDefinition>,
    /// The pictures in it, placed in its paragraphs.
    pictures: Vec<PictureFound>,
}

impl Story {
    fn new(kind: StoryKind, depth: usize) -> Self {
        Self {
            kind,
            depth,
            blocks: Vec::new(),
            runs: Vec::new(),
            paragraph_length: 0,
            paragraphs_done: 0,
            levels: Vec::new(),
            definitions: Vec::new(),
            pictures: Vec::new(),
        }
    }
}

/// One table being built.
#[derive(Debug, Default)]
struct Level {
    table: Table,
    /// Each finished row's cells' left and right edges, for the columns.
    edges: Vec<Vec<(i32, i32)>>,
    row: Vec<TableCell>,
    cell_blocks: Vec<Block>,
}

impl Level {
    /// The row ends: its cells take what the description says about them.
    fn finish_row(&mut self, definition: &RowDefinition) {
        let cells = core::mem::take(&mut self.row);
        if cells.is_empty() {
            return;
        }
        let mut kept: Vec<TableCell> = Vec::new();
        let mut edges: Vec<(i32, i32)> = Vec::new();
        let mut left = definition.left;
        for (index, mut cell) in cells.into_iter().enumerate() {
            let described = definition.cells.get(index);
            let right = described
                .map(|cell| cell.right)
                .filter(|right| *right > left)
                .unwrap_or(left + USUAL_CELL);
            if let Some(described) = described {
                // A cell merged into the one before it widens that one.
                if described.horizontal == Merge::Continue && !kept.is_empty() {
                    if let (Some(edge), Some(previous)) = (edges.last_mut(), kept.last_mut()) {
                        edge.1 = right;
                        previous.width = Some(right - edge.0);
                        if !cell.blocks.iter().all(|block| block.plain_text().is_empty()) {
                            previous.blocks.extend(cell.blocks);
                        }
                    }
                    left = right;
                    continue;
                }
                cell.borders = described.borders.clone();
                cell.shading = described.shading.fill();
                cell.merged_upwards = described.vertical == Merge::Continue;
                cell.vertical = described.alignment;
            }
            cell.width = Some(right - left);
            edges.push((left, right));
            kept.push(cell);
            left = right;
        }
        if self.table.rows.is_empty() {
            self.table.borders = definition.borders.clone();
            self.table.style = definition.style.clone();
        }
        self.table.rows.push(TableRow {
            cells: kept,
            height: definition.height,
            height_exact: definition.exact,
            is_header: definition.header,
        });
        self.edges.push(edges);
    }

    /// The table is finished: its columns are every edge any row has.
    fn into_table(mut self) -> Option<Table> {
        if !self.cell_blocks.is_empty() {
            let blocks = core::mem::take(&mut self.cell_blocks);
            self.row.push(TableCell { blocks, ..TableCell::default() });
        }
        if !self.row.is_empty() {
            self.finish_row(&RowDefinition::default());
        }
        if self.table.rows.is_empty() {
            return None;
        }
        let mut all: Vec<i32> = self.edges.iter().flatten().flat_map(|&(l, r)| [l, r]).collect();
        all.sort_unstable();
        all.dedup();
        self.table.grid = all.windows(2).map(|pair| pair[1] - pair[0]).collect();
        for (row, edges) in self.table.rows.iter_mut().zip(&self.edges) {
            for (cell, &(left, right)) in row.cells.iter_mut().zip(edges) {
                let from = all.iter().position(|edge| *edge == left).unwrap_or(0);
                let to = all.iter().position(|edge| *edge == right).unwrap_or(from + 1);
                cell.span = u32::try_from(to.saturating_sub(from)).unwrap_or(1).max(1);
            }
        }
        Some(self.table)
    }
}

/// How a cell is merged with its neighbour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Merge {
    #[default]
    None,
    /// The first of several merged cells.
    First,
    /// One merged into the cell before it, across or above.
    Continue,
}

/// A row as the file describes it: `\trowd` to the last `\cellx`.
#[derive(Clone, Debug, Default)]
struct RowDefinition {
    left: i32,
    cells: Vec<CellDefinition>,
    /// The cell being described, until `\cellx` says where it ends.
    pending: CellDefinition,
    borders: TableBorders,
    header: bool,
    height: Option<i32>,
    exact: bool,
    style: Option<String>,
}

#[derive(Clone, Debug, Default)]
struct CellDefinition {
    right: i32,
    horizontal: Merge,
    vertical: Merge,
    borders: TableBorders,
    shading: Shading,
    alignment: CellAlignment,
}

/// Which edge a border word is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Edge {
    Top,
    Left,
    Bottom,
    Right,
    /// Between rows, or between paragraphs.
    Horizontal,
    /// Between columns.
    Vertical,
}

/// What the border words that follow describe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BorderTarget {
    Paragraph(Edge),
    /// All four sides of the paragraph: `\box`.
    ParagraphBox,
    Cell(Edge),
    Row(Edge),
    /// The table a table style describes.
    StyleTable(Edge),
}

fn paragraph_edge(borders: &mut ParagraphBorders, edge: Edge) -> &mut Option<Border> {
    match edge {
        Edge::Top => &mut borders.top,
        Edge::Left => &mut borders.start,
        Edge::Bottom => &mut borders.bottom,
        Edge::Right => &mut borders.end,
        Edge::Horizontal | Edge::Vertical => &mut borders.between,
    }
}

fn table_edge(borders: &mut TableBorders, edge: Edge) -> &mut Option<Border> {
    match edge {
        Edge::Top => &mut borders.top,
        Edge::Left => &mut borders.start,
        Edge::Bottom => &mut borders.bottom,
        Edge::Right => &mut borders.end,
        Edge::Horizontal => &mut borders.inside_horizontal,
        Edge::Vertical => &mut borders.inside_vertical,
    }
}

struct Reader {
    stack: Vec<State>,
    /// The document's code page, from `\ansicpg`.
    code_page: Encoding,
    fonts: Vec<(i32, Font)>,
    colours: Vec<String>,
    styles: Vec<StyleEntry>,
    style_being_read: Option<StyleEntry>,
    /// How deep the stack was at the stylesheet's group, while it is read.
    stylesheet_depth: Option<usize>,
    lists: Vec<List>,
    /// `\ls` numbers to list ids.
    overrides: Vec<(i32, i32)>,
    /// What is being read into: the document first, and whatever story is
    /// being read on top of it.
    stories: Vec<Story>,
    /// The text of the current run, gathered until the formatting changes.
    text: String,
    /// The bytes of the current run's text not yet decoded, in the code page
    /// in force when they arrived.
    pending_bytes: Vec<u8>,
    pending_page: Encoding,
    font_being_read: Option<(i32, Font)>,
    colour_being_read: (Option<u8>, Option<u8>, Option<u8>),
    list_being_read: Option<List>,
    override_being_read: (Option<i32>, Option<i32>),
    picture: Option<PictureBeingRead>,
    /// The fields being read, innermost last.
    fields: Vec<FieldBeingRead>,
    /// The first half of a character written as two `\u`, waiting for the
    /// second.
    high_surrogate: Option<u16>,
    border: Option<BorderTarget>,
    shape: Option<ShapeBeingRead>,
    /// Who made the tracked changes, and each change already given a number:
    /// an insertion, a deletion, or — with no kind — a change of formatting.
    authors: Vec<String>,
    revisions: Vec<(Option<RevisionKind>, String, String)>,
    links: Vec<LinkFound>,
    bookmarks_open: Vec<(String, TextPosition)>,
    bookmarks: Vec<BookmarkFound>,
    comment_starts: Vec<(String, TextPosition)>,
    comment_ends: Vec<(String, TextPosition)>,
    comment_author: String,
    comment_reference: String,
    comment_date: Option<i32>,
    comments: Vec<CommentFound>,
    notes: Vec<NoteFound>,
    footnotes: i32,
    endnotes: i32,
    /// The page as the document describes it, and as the section being read
    /// does: `\sectd` goes back to the first.
    document_page: PageSetup,
    section: PageSetup,
    /// The section's own headers and footers, as they are read.
    furniture: Vec<FurnitureFound>,
    sections: Vec<SectionFound>,
    facing_pages: bool,
}

/// Reads a file into a body, with everything that goes beside it.
#[must_use]
pub fn read(bytes: &[u8]) -> Reading {
    let mut reader = Reader {
        stack: vec![State::default()],
        code_page: Encoding::CodePage(1252),
        fonts: Vec::new(),
        colours: Vec::new(),
        styles: Vec::new(),
        style_being_read: None,
        stylesheet_depth: None,
        lists: Vec::new(),
        overrides: Vec::new(),
        stories: vec![Story::new(StoryKind::Main, 0)],
        text: String::new(),
        pending_bytes: Vec::new(),
        pending_page: Encoding::CodePage(1252),
        font_being_read: None,
        colour_being_read: (None, None, None),
        list_being_read: None,
        override_being_read: (None, None),
        picture: None,
        fields: Vec::new(),
        high_surrogate: None,
        border: None,
        shape: None,
        authors: Vec::new(),
        revisions: Vec::new(),
        links: Vec::new(),
        bookmarks_open: Vec::new(),
        bookmarks: Vec::new(),
        comment_starts: Vec::new(),
        comment_ends: Vec::new(),
        comment_author: String::new(),
        comment_reference: String::new(),
        comment_date: None,
        comments: Vec::new(),
        notes: Vec::new(),
        footnotes: 0,
        endnotes: 0,
        document_page: PageSetup::default(),
        section: PageSetup::default(),
        furniture: Vec::new(),
        sections: Vec::new(),
        facing_pages: false,
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
                reader.group_opened();
                just_opened = true;
                starred = false;
                continue;
            }
            Token::Close => reader.close_group(),
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
            Token::Hex(byte) | Token::Byte(byte) => reader.byte(byte),
            Token::Binary(data) => {
                if reader.top().destination == Destination::Picture {
                    if let Some(picture) = &mut reader.picture {
                        picture.raw.extend_from_slice(&data);
                    }
                }
            }
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
    matches!(
        word,
        "listtable"
            | "listoverridetable"
            | "shppict"
            | "fldinst"
            | "cs"
            | "ts"
            | "revtbl"
            | "bkmkstart"
            | "bkmkend"
            | "atrfstart"
            | "atrfend"
            | "atnauthor"
            | "atnref"
            | "atndate"
            | "annotation"
            | "footnote"
            | "oldcprops"
            | "shpinst"
            | "nesttableprops"
    )
}

/// Whether a destination's text is gathered as text: the document's, or a
/// name or a value something else needs.
fn gathers_text(destination: Destination) -> bool {
    matches!(
        destination,
        Destination::Body
            | Destination::FieldInstruction
            | Destination::Tag(_)
            | Destination::RevisionTable
            | Destination::ShapeName
            | Destination::ShapeValue
    )
}

impl Reader {
    fn top(&self) -> &State {
        self.stack.last().expect("the document group")
    }

    fn top_mut(&mut self) -> &mut State {
        self.stack.last_mut().expect("the document group")
    }

    fn story(&self) -> &Story {
        self.stories.last().expect("the document's story")
    }

    fn story_mut(&mut self) -> &mut Story {
        self.stories.last_mut().expect("the document's story")
    }

    fn in_main_story(&self) -> bool {
        self.stories.len() == 1
    }

    /// Where the document's own text has got to.
    fn main_position(&self) -> TextPosition {
        let main = &self.stories[0];
        TextPosition::new(main.paragraphs_done, main.paragraph_length)
    }

    /// Whether text arriving now takes up room in the paragraph, as the
    /// document counts it: deleted text does not.
    fn counts(&self) -> bool {
        self.top().destination == Destination::Body && !self.top().marks.deleted
    }

    /// The code page the text of the current font is in.
    fn page_in_force(&self) -> Encoding {
        let font = self.top().font;
        font.and_then(|wanted| self.fonts.iter().find(|(index, _)| *index == wanted))
            .and_then(|(_, font)| font.code_page)
            .and_then(|number| Encoding::code_page(u32::from(number)))
            .unwrap_or(self.code_page)
    }

    fn colour(&self, index: i32) -> Option<String> {
        self.colours
            .get(usize::try_from(index).ok()?)
            .filter(|colour| colour.as_str() != "auto")
            .cloned()
    }

    fn style(&self, index: i32) -> Option<&StyleEntry> {
        self.styles.iter().find(|entry| entry.index == index)
    }

    /// A group was opened. In the stylesheet a group is a style.
    fn group_opened(&mut self) {
        if self.top().destination == Destination::StyleSheet
            && self.stylesheet_depth.is_some_and(|depth| self.stack.len() == depth + 1)
        {
            let top = self.top_mut();
            top.chars = RunProperties::default();
            top.paragraph = ParagraphProperties::default();
            top.shading = Shading::default();
            top.sides = (None, None);
            self.style_being_read = Some(StyleEntry::default());
        }
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
            Destination::RevisionTable if byte == b';' => {
                self.flush_bytes();
                let author = core::mem::take(&mut self.text);
                self.authors.push(author.trim().to_owned());
            }
            destination if gathers_text(destination) => {
                let page = self.page_in_force();
                if page != self.pending_page {
                    self.flush_bytes();
                    self.pending_page = page;
                }
                self.pending_bytes.push(byte);
            }
            Destination::FontTable => {
                if byte == b';' {
                    if let Some((index, mut font)) = self.font_being_read.take() {
                        font.finish_name(self.code_page);
                        self.fonts.push((index, font));
                    }
                } else if let Some((_, font)) = &mut self.font_being_read {
                    font.name_bytes.push(byte);
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
                    self.finish_style();
                } else if let Some(entry) = &mut self.style_being_read {
                    entry.name_bytes.push(byte);
                }
            }
            Destination::Picture if byte.is_ascii_hexdigit() => {
                if let Some(picture) = &mut self.picture {
                    picture.hex.push(byte);
                }
            }
            _ => {}
        }
    }

    /// A character that arrived whole: from `\u`, or a control symbol.
    fn character(&mut self, character: char) {
        match self.top().destination {
            destination if gathers_text(destination) => {
                self.flush_bytes();
                self.text.push(character);
                if self.counts() {
                    self.story_mut().paragraph_length += character.len_utf8();
                }
            }
            Destination::FontTable => {
                if let Some((_, font)) = &mut self.font_being_read {
                    font.finish_name(self.code_page);
                    font.name.push(character);
                }
            }
            Destination::StyleSheet => {
                let page = self.code_page;
                if let Some(entry) = &mut self.style_being_read {
                    let bytes = core::mem::take(&mut entry.name_bytes);
                    entry.name.push_str(&page.decode(&bytes));
                    entry.name.push(character);
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
        if self.counts() {
            self.story_mut().paragraph_length += decoded.len();
        }
        self.text.push_str(&decoded);
    }

    /// Ends the current run, if it holds anything.
    fn flush_run(&mut self) {
        self.flush_bytes();
        if self.text.is_empty() {
            return;
        }
        match self.top().destination {
            Destination::FieldInstruction => {
                let text = core::mem::take(&mut self.text);
                if let Some(field) = self.fields.last_mut() {
                    field.instruction.push_str(&text);
                }
            }
            Destination::Body => {
                let text = core::mem::take(&mut self.text);
                let run = self.run_of(RunContent::Text(text));
                self.story_mut().runs.push(run);
            }
            // A name or a value: it is taken whole when its group ends.
            _ => {}
        }
    }

    /// A run holding one thing, formatted as the text here is.
    fn run_of(&mut self, content: RunContent) -> Run {
        Run {
            properties: self.top().chars.clone(),
            content: vec![content],
            field: self.result_field(),
            revision: self.revision_here(),
            format_change: self.format_change_here(),
        }
    }

    /// Puts something that is not text into the paragraph.
    fn content(&mut self, content: RunContent, length: usize) {
        if self.top().destination != Destination::Body {
            return;
        }
        self.flush_run();
        let run = self.run_of(content);
        self.story_mut().runs.push(run);
        if self.counts() {
            self.story_mut().paragraph_length += length;
        }
    }

    /// The instruction of the field whose result is being read, for the runs
    /// that show it. A link in the document's own text is a link rather than a
    /// field, and is put in as one when it ends.
    fn result_field(&self) -> Option<String> {
        let field = self.fields.last()?;
        let instruction = field.instruction.trim();
        if field.story + 1 != self.stories.len() || field.spans || instruction.is_empty() {
            return None;
        }
        if field.story == 0 && instruction.starts_with("HYPERLINK") {
            return None;
        }
        // A drawing wrapped in a field of its own, for readers that know
        // fields and not drawings: the drawing is what is meant.
        if instruction.split_whitespace().next() == Some("SHAPE") {
            return None;
        }
        Some(instruction.to_owned())
    }

    /// The tracked change the text here is part of.
    fn revision_here(&mut self) -> Option<Revision> {
        let marks = self.top().marks;
        let (kind, author, date) = if marks.deleted {
            (RevisionKind::Deleted, marks.deleted_by, marks.deleted_at)
        } else if marks.inserted {
            (RevisionKind::Inserted, marks.inserted_by, marks.inserted_at)
        } else {
            return None;
        };
        let (author, date, id) = self.change_number(Some(kind), author, date);
        Some(Revision { kind, author, date, id })
    }

    /// The tracked change to the formatting of the text here, with what the
    /// formatting was before it.
    fn format_change_here(&mut self) -> Option<FormatChange> {
        let marks = self.top().marks;
        if !marks.formatted {
            return None;
        }
        let before = Box::new(self.top().before.clone().unwrap_or_default());
        let (author, date, id) = self.change_number(None, marks.formatted_by, marks.formatted_at);
        Some(FormatChange { author, date, id, before })
    }

    /// Who made a change, when, and its number: one number for one person's
    /// one change, so the runs of it go back inside one wrapper.
    fn change_number(
        &mut self,
        kind: Option<RevisionKind>,
        author: usize,
        date: i32,
    ) -> (String, String, i32) {
        let author = self.authors.get(author).cloned().unwrap_or_else(|| "Unknown".to_owned());
        let date = dttm(date);
        let index = match self
            .revisions
            .iter()
            .position(|(was, by, at)| *was == kind && *by == author && *at == date)
        {
            Some(index) => index,
            None => {
                self.revisions.push((kind, author.clone(), date.clone()));
                self.revisions.len() - 1
            }
        };
        (author, date, i32::try_from(index + 1).unwrap_or(i32::MAX))
    }

    // --- Paragraphs and tables -----------------------------------------------

    /// How deep in tables the paragraph being read is.
    fn paragraph_depth(&self) -> usize {
        let top = self.top();
        if top.in_table {
            top.itap.max(1)
        } else {
            0
        }
    }

    /// Ends the paragraph being built and puts it where it belongs.
    fn end_paragraph(&mut self) {
        let depth = self.paragraph_depth();
        self.end_paragraph_at(depth);
    }

    /// The paragraph's properties as the model keeps them.
    ///
    /// RTF says where a paragraph is aligned and indented, and which side a
    /// border is on, by the page: left and right. The model says it by the
    /// text: where a line starts and where it ends, which in text that runs
    /// right to left are the other way round. So which is which is only known
    /// once the paragraph has said which way it runs, and that can come after
    /// the rest. `\lin` and `\rin` are the start and end already, and Word
    /// writes them after the others; where they are given they hold.
    fn resolved_paragraph(&self) -> ParagraphProperties {
        let top = self.top();
        let mut properties = top.paragraph.clone();
        if let Some(fill) = top.shading.fill() {
            properties.shading = Some(fill);
        }
        let backwards = properties.right_to_left == Some(true);
        let (left, right) = top.sides;
        let (start, end) = if backwards { (right, left) } else { (left, right) };
        properties.indent_start = properties.indent_start.or(start);
        properties.indent_end = properties.indent_end.or(end);
        if backwards {
            properties.alignment = match properties.alignment {
                Some(Alignment::Start) => Some(Alignment::End),
                Some(Alignment::End) => Some(Alignment::Start),
                other => other,
            };
            let borders = &mut properties.borders;
            core::mem::swap(&mut borders.start, &mut borders.end);
        }
        properties
    }

    fn end_paragraph_at(&mut self, depth: usize) {
        self.flush_run();
        let properties = self.resolved_paragraph();
        let index = self.stories.len() - 1;
        let story = &mut self.stories[index];
        let mut runs = core::mem::take(&mut story.runs);
        // A field begun in this paragraph and still open goes on past its
        // end, which a field in the model cannot: its result stays as text.
        for field in &mut self.fields {
            if field.story == index && !field.spans && field.paragraph == story.paragraphs_done {
                field.spans = true;
                let instruction = field.instruction.trim();
                for run in &mut runs {
                    if run.field.as_deref() == Some(instruction) {
                        run.field = None;
                    }
                }
            }
        }
        story.paragraph_length = 0;
        story.paragraphs_done += 1;
        self.put_block(Block::Paragraph(Paragraph { properties, runs }), depth);
    }

    /// Puts a finished block into the story, in the table it is in if it is
    /// in one.
    fn put_block(&mut self, block: Block, depth: usize) {
        self.close_tables_to(depth);
        let story = self.story_mut();
        while story.levels.len() < depth {
            story.levels.push(Level::default());
        }
        match depth.checked_sub(1) {
            None => story.blocks.push(block),
            Some(level) => story.levels[level].cell_blocks.push(block),
        }
    }

    /// Finishes the tables deeper than a paragraph that has come out of them.
    fn close_tables_to(&mut self, depth: usize) {
        let story = self.story_mut();
        while story.levels.len() > depth {
            let Some(level) = story.levels.pop() else { break };
            if let Some(table) = level.into_table() {
                let block = Block::Table(Box::new(table));
                match story.levels.last_mut() {
                    Some(outer) => outer.cell_blocks.push(block),
                    None => story.blocks.push(block),
                }
            }
        }
    }

    /// How deep the table a cell or row mark ends is: `\cell` and `\row` are
    /// the outermost table's, `\nestcell` and `\nestrow` the one the
    /// paragraph is in.
    fn mark_depth(&self, nested: bool) -> usize {
        if nested {
            self.paragraph_depth().max(2)
        } else {
            1
        }
    }

    /// `\cell`: the paragraph ends and so does the cell.
    fn end_cell(&mut self, nested: bool) {
        let depth = self.mark_depth(nested);
        self.end_paragraph_at(depth);
        let level = &mut self.story_mut().levels[depth - 1];
        let blocks = core::mem::take(&mut level.cell_blocks);
        level.row.push(TableCell { blocks, ..TableCell::default() });
    }

    /// `\row`: the row ends, and takes the description it was last given.
    fn end_row(&mut self, nested: bool) {
        let depth = self.mark_depth(nested);
        let unfinished =
            self.story().levels.get(depth - 1).is_some_and(|level| !level.cell_blocks.is_empty())
                || !self.story().runs.is_empty()
                || !self.text.is_empty()
                || !self.pending_bytes.is_empty();
        if unfinished {
            self.end_cell(nested);
        }
        let definition = self.story().definitions.get(depth - 1).cloned().unwrap_or_default();
        if let Some(level) = self.story_mut().levels.get_mut(depth - 1) {
            level.finish_row(&definition);
        }
    }

    /// Which table a row description is about: the outermost, unless the
    /// group says it describes an inner one.
    fn definition_mut(&mut self) -> &mut RowDefinition {
        let level = if self.top().nested_properties { self.paragraph_depth().max(2) } else { 1 };
        let story = self.story_mut();
        while story.definitions.len() < level {
            story.definitions.push(RowDefinition::default());
        }
        &mut story.definitions[level - 1]
    }

    // --- Sections ------------------------------------------------------------

    /// `\sect`: the section ends, with the paragraph it ends.
    fn end_section(&mut self) {
        if !self.in_main_story() {
            return;
        }
        let main = &self.stories[0];
        let nothing_since = main.runs.is_empty()
            && self.text.is_empty()
            && self.pending_bytes.is_empty()
            && main.levels.is_empty();
        let last_ended = main.paragraphs_done.checked_sub(1);
        // A writer that ends a paragraph and then the section ends one
        // paragraph, not two: the break goes on the paragraph just ended.
        let reuse = nothing_since
            && matches!(main.blocks.last(), Some(Block::Paragraph(_)))
            && last_ended.is_some()
            && self.sections.last().and_then(|section| section.last_paragraph) != last_ended;
        if !reuse {
            self.end_paragraph_at(0);
        }
        let last = self.stories[0].paragraphs_done.saturating_sub(1);
        self.sections.push(SectionFound {
            last_paragraph: Some(last),
            page: self.section.over(&self.document_page),
            furniture: core::mem::take(&mut self.furniture),
        });
    }

    // --- Control words ---------------------------------------------------------

    fn control(&mut self, word: &str, number: Option<i32>, just_opened: bool) {
        let n = number.unwrap_or(0);
        let on = number != Some(0);
        // A control word ends the stand-in of a `\u`, whatever \uc said.
        self.top_mut().skip = 0;

        // Destinations, which a group is for. Inside a skipped group nothing
        // is a destination: the whole of it goes.
        if just_opened && self.top().destination != Destination::Skip && self.open_destination(word)
        {
            return;
        }

        let destination = self.top().destination;
        if destination == Destination::Skip {
            return;
        }
        // Characters written as numbers mean the same wherever they are.
        match word {
            "uc" => {
                self.top_mut().uc = usize::try_from(n).unwrap_or(1);
                return;
            }
            "u" => {
                self.unicode(n);
                return;
            }
            _ => {}
        }

        match destination {
            Destination::Skip
            | Destination::FieldInstruction
            | Destination::Tag(_)
            | Destination::RevisionTable
            | Destination::ShapeName
            | Destination::ShapeValue => {}
            Destination::FontTable => self.font_word(word, n),
            Destination::ColourTable => match word {
                "red" => self.colour_being_read.0 = u8::try_from(n).ok(),
                "green" => self.colour_being_read.1 = u8::try_from(n).ok(),
                "blue" => self.colour_being_read.2 = u8::try_from(n).ok(),
                _ => {}
            },
            Destination::StyleSheet => {
                if !self.style_word(word, n) {
                    self.formatting(word, number, n, on);
                }
            }
            Destination::ListTable => self.list_word(word, n),
            Destination::ListOverrideTable => match word {
                "listoverride" => self.override_being_read = (None, None),
                "listid" => self.override_being_read.0 = Some(n),
                "ls" => {
                    self.override_being_read.1 = Some(n);
                    if let (Some(id), Some(ls)) = self.override_being_read {
                        self.overrides.push((ls, id));
                    }
                }
                _ => {}
            },
            Destination::Picture => self.picture_word(word, n),
            Destination::ShapeInstance => self.shape_word(word, n),
            Destination::Body => self.body_word(word, number, n, on),
        }
    }

    /// A word that opens a group of its own kind. Returns whether it was one.
    fn open_destination(&mut self, word: &str) -> bool {
        let parent = self.top().destination;
        let destination = match word {
            "fonttbl" => Destination::FontTable,
            "colortbl" => Destination::ColourTable,
            "stylesheet" => Destination::StyleSheet,
            "listtable" => Destination::ListTable,
            "listoverridetable" => Destination::ListOverrideTable,
            "pict" => Destination::Picture,
            "fldinst" => Destination::FieldInstruction,
            "revtbl" => Destination::RevisionTable,
            "bkmkstart" => Destination::Tag(Tag::BookmarkStart),
            "bkmkend" => Destination::Tag(Tag::BookmarkEnd),
            "atrfstart" => Destination::Tag(Tag::CommentStart),
            "atrfend" => Destination::Tag(Tag::CommentEnd),
            "atnauthor" => Destination::Tag(Tag::CommentAuthor),
            "atnref" => Destination::Tag(Tag::CommentReference),
            "atndate" => Destination::Tag(Tag::CommentDate),
            "sn" | "sv" if parent != Destination::ShapeInstance => Destination::Skip,
            "sn" => Destination::ShapeName,
            "sv" => Destination::ShapeValue,
            // A drawing: its description, and the words in it.
            "shp" if self.shape.is_none() && parent == Destination::Body => {
                self.flush_run();
                self.shape = Some(ShapeBeingRead {
                    depth: self.stack.len(),
                    wrap: 2,
                    ..ShapeBeingRead::default()
                });
                Destination::ShapeInstance
            }
            "shpinst" if self.shape.is_some() => Destination::ShapeInstance,
            "shptxt" if self.shape.is_some() && parent == Destination::ShapeInstance => {
                self.open_story(StoryKind::TextBox);
                return true;
            }
            // The stories beside the document's own text.
            "header" | "headerr" | "headerl" | "headerf" | "footer" | "footerr" | "footerl"
            | "footerf"
                if self.in_main_story() && parent == Destination::Body =>
            {
                let kind =
                    if word.starts_with("header") { Furniture::Header } else { Furniture::Footer };
                let which = match word.as_bytes().last() {
                    Some(b'l') => Which::Even,
                    Some(b'f') => Which::First,
                    _ => Which::Default,
                };
                self.flush_run();
                self.open_story(StoryKind::Furniture(kind, which));
                return true;
            }
            "footnote" if self.in_main_story() && parent == Destination::Body => {
                self.flush_run();
                // The mark goes in now, where the note is; which kind of note
                // it is, and so its number, is known when the note ends.
                let run = self.run_of(RunContent::NoteReference { id: 0, endnote: false });
                let counts = self.counts();
                let main = &mut self.stories[0];
                main.runs.push(run);
                if counts {
                    main.paragraph_length += 1;
                }
                let reference = main.runs.len() - 1;
                self.open_story(StoryKind::Note { reference, endnote: false });
                return true;
            }
            "annotation" if self.in_main_story() && parent == Destination::Body => {
                self.flush_run();
                self.comment_reference.clear();
                self.comment_date = None;
                let at = self.main_position();
                self.open_story(StoryKind::Comment { at });
                return true;
            }
            // What the formatting was before a tracked change to it: the
            // group's own words describe it, from nothing.
            "oldcprops" => {
                self.flush_run();
                let top = self.top_mut();
                top.old_properties = true;
                top.chars = RunProperties::default();
                return true;
            }
            // The description of a table inside a table's row, which comes
            // after the row rather than before it.
            "nesttableprops" => {
                self.top_mut().nested_properties = true;
                return true;
            }
            // Everything this reader does not carry through.
            "info" | "header" | "footer" | "headerl" | "headerr" | "headerf" | "footerl"
            | "footerr" | "footerf" | "footnote" | "annotation" | "pntext" | "listtext"
            | "nonshppict" | "shp" | "shpinst" | "shptxt" | "shprslt" | "shpgrp" | "do" | "xe"
            | "tc" | "ftnsep" | "ftnsepc" | "aftnsep" | "aftnsepc" | "themedata"
            | "colorschememapping" | "latentstyles" | "datastore" | "rsidtbl" | "generator"
            | "xmlnstbl" | "pnseclvl" | "userprops" | "docvar" | "mmathPr" | "background"
            | "template" | "atnid" | "atnicn" | "atnparent" | "atntime" | "objdata"
            | "nonesttables" => Destination::Skip,
            _ => return false,
        };
        // The tables' text is not the document's.
        self.flush_run();
        self.top_mut().destination = destination;
        match destination {
            Destination::Picture => self.picture = Some(PictureBeingRead::default()),
            Destination::FieldInstruction => {
                if let Some(field) = self.fields.last_mut() {
                    field.instruction.clear();
                }
            }
            Destination::StyleSheet => self.stylesheet_depth = Some(self.stack.len()),
            _ => {}
        }
        true
    }

    /// A story begins: text goes into it until its group ends, formatted
    /// from nothing.
    fn open_story(&mut self, kind: StoryKind) {
        let depth = self.stack.len();
        self.stories.push(Story::new(kind, depth));
        let top = self.top_mut();
        top.destination = Destination::Body;
        top.chars = RunProperties::default();
        top.paragraph = ParagraphProperties::default();
        top.shading = Shading::default();
        top.sides = (None, None);
        top.in_table = false;
        top.itap = 0;
        top.nested_properties = false;
        top.marks = Marks::default();
    }

    /// `\u`: a character by its number.
    fn unicode(&mut self, n: i32) {
        // Signed sixteen bits, as the format writes them; a surrogate is
        // half a character and waits for its other half.
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

    fn font_word(&mut self, word: &str, n: i32) {
        match word {
            "f" => {
                if let Some((index, mut font)) = self.font_being_read.take() {
                    font.finish_name(self.code_page);
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
    }

    fn list_word(&mut self, word: &str, n: i32) {
        match word {
            "list" => {
                self.list_being_read = Some(List { depth: self.stack.len(), ..List::default() });
            }
            "listid" => {
                if let Some(list) = &mut self.list_being_read {
                    list.id = n;
                }
            }
            // The first level says what kind of list it is: 23 is a bullet,
            // the rest are numbers of one kind or another.
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
    }

    fn picture_word(&mut self, word: &str, n: i32) {
        let Some(picture) = &mut self.picture else { return };
        match word {
            "pngblip" => picture.kind = Some(PictureKind::Png),
            "jpegblip" => picture.kind = Some(PictureKind::Jpeg),
            "emfblip" => picture.kind = Some(PictureKind::Emf),
            "wmetafile" => picture.kind = Some(PictureKind::Wmf),
            "dibitmap" => picture.kind = Some(PictureKind::Dib),
            "wbitmap" | "macpict" | "pmmetafile" => picture.kind = Some(PictureKind::Other),
            "picw" => picture.width_pixels = Some(i64::from(n)),
            "pich" => picture.height_pixels = Some(i64::from(n)),
            "picwgoal" => picture.width_twips = Some(i64::from(n)),
            "pichgoal" => picture.height_twips = Some(i64::from(n)),
            "picscalex" if n > 0 => picture.scale.0 = i64::from(n),
            "picscaley" if n > 0 => picture.scale.1 = i64::from(n),
            _ => {}
        }
    }

    fn shape_word(&mut self, word: &str, n: i32) {
        let Some(shape) = &mut self.shape else { return };
        match word {
            "shpleft" => shape.bounds[0] = n,
            "shptop" => shape.bounds[1] = n,
            "shpright" => shape.bounds[2] = n,
            "shpbottom" => shape.bounds[3] = n,
            "shpbxpage" => shape.across = Some(Relative::Page),
            "shpbxmargin" => shape.across = Some(Relative::Margin),
            "shpbxcolumn" => shape.across = Some(Relative::Column),
            "shpbxignore" => shape.across = None,
            "shpbypage" => shape.down = Some(Relative::Page),
            "shpbymargin" => shape.down = Some(Relative::Margin),
            "shpbypara" => shape.down = Some(Relative::Paragraph),
            "shpbyignore" => shape.down = None,
            "shpwr" => shape.wrap = n,
            "shpwrk" => shape.wrap_side = n,
            "shpfblwtxt" => shape.behind = n == 1,
            "shpz" => shape.z = n,
            "shplid" => shape.id = u32::try_from(n).unwrap_or(0),
            _ => {}
        }
    }

    /// A word only the stylesheet has. Returns whether it was one.
    fn style_word(&mut self, word: &str, n: i32) -> bool {
        let edge = match word {
            "trbrdrt" => Some(Edge::Top),
            "trbrdrl" => Some(Edge::Left),
            "trbrdrb" => Some(Edge::Bottom),
            "trbrdrr" => Some(Edge::Right),
            "trbrdrh" => Some(Edge::Horizontal),
            "trbrdrv" => Some(Edge::Vertical),
            _ => None,
        };
        if let Some(edge) = edge {
            self.set_border_target(BorderTarget::StyleTable(edge));
            return true;
        }
        let kind = match word {
            "s" => Some(StyleKind::Paragraph),
            "cs" => Some(StyleKind::Character),
            "ts" => Some(StyleKind::Table),
            "ds" => Some(StyleKind::Other),
            _ => None,
        };
        let entry = self.style_being_read.get_or_insert_with(StyleEntry::default);
        if let Some(kind) = kind {
            entry.index = n;
            entry.kind = kind;
            return true;
        }
        // 222 is the number that means none.
        let named = (n != 222).then_some(n);
        match word {
            "sbasedon" => entry.based_on = named,
            "snext" => entry.next = named,
            "ls" => entry.list = Some(n),
            "ilvl" => entry.level = u8::try_from(n).unwrap_or(0).min(8),
            _ => return false,
        }
        true
    }

    /// A style's entry ends at its name's semicolon.
    fn finish_style(&mut self) {
        let Some(mut entry) = self.style_being_read.take() else { return };
        let bytes = core::mem::take(&mut entry.name_bytes);
        entry.name.push_str(&self.code_page.decode(&bytes));
        entry.name = entry.name.trim().to_owned();
        if entry.name.is_empty() {
            return;
        }
        entry.id = style_id(&entry.name, entry.index);
        entry.paragraph = self.resolved_paragraph();
        entry.run = self.top().chars.clone();
        self.styles.push(entry);
    }

    /// The document's own text.
    #[allow(clippy::too_many_lines, reason = "one arm per control word is the readable shape")]
    fn body_word(&mut self, word: &str, number: Option<i32>, n: i32, on: bool) {
        match word {
            "ansicpg" => {
                if let Some(page) = u32::try_from(n).ok().and_then(Encoding::code_page) {
                    self.code_page = page;
                }
            }

            // Paragraphs.
            "par" => self.end_paragraph(),
            "pard" => {
                // Being in a table is a paragraph property too, and Word
                // writes `\intbl` again after every `\pard` in one.
                let top = self.top_mut();
                top.paragraph = ParagraphProperties::default();
                top.shading = Shading::default();
                top.sides = (None, None);
                top.in_table = false;
                top.itap = 0;
            }
            "plain" => {
                self.flush_run();
                let top = self.top_mut();
                top.chars = RunProperties::default();
                top.marks = Marks::default();
                top.before = None;
            }
            "line" => self.content(RunContent::Break(BreakKind::Line), 1),
            "page" => self.content(RunContent::Break(BreakKind::Page), 1),
            "column" => self.content(RunContent::Break(BreakKind::Column), 1),
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
            // The page number, the way the oldest writers put one in a
            // header: a field, as it is everywhere else.
            "chpgn" => {
                self.flush_run();
                let mut run = self.run_of(RunContent::Text("1".to_owned()));
                run.field = Some("PAGE".to_owned());
                self.story_mut().runs.push(run);
                if self.counts() {
                    self.story_mut().paragraph_length += 1;
                }
            }
            // The numbers of notes and the marks of comments, which the
            // document makes of its own.
            "chftn" | "chatn" => {}
            "ftnalt" => {
                if let Some(StoryKind::Note { endnote, .. }) =
                    self.stories.last_mut().map(|story| &mut story.kind)
                {
                    *endnote = true;
                }
            }

            // Tables.
            "trowd" => {
                *self.definition_mut() = RowDefinition::default();
                self.border = None;
            }
            "trleft" => self.definition_mut().left = n,
            "trhdr" => self.definition_mut().header = true,
            "trrh" => {
                let definition = self.definition_mut();
                definition.height = (n != 0).then_some(n.abs());
                definition.exact = n < 0;
            }
            "ts" => {
                let style = self.style(n).map(|entry| entry.id.clone());
                self.definition_mut().style = style;
            }
            "cellx" => {
                let definition = self.definition_mut();
                let mut cell = core::mem::take(&mut definition.pending);
                cell.right = n;
                definition.cells.push(cell);
                self.border = None;
            }
            "clmgf" => self.definition_mut().pending.horizontal = Merge::First,
            "clmrg" => self.definition_mut().pending.horizontal = Merge::Continue,
            "clvmgf" => self.definition_mut().pending.vertical = Merge::First,
            "clvmrg" => self.definition_mut().pending.vertical = Merge::Continue,
            "clvertalt" => self.definition_mut().pending.alignment = CellAlignment::Top,
            "clvertalc" => self.definition_mut().pending.alignment = CellAlignment::Middle,
            "clvertalb" => self.definition_mut().pending.alignment = CellAlignment::Bottom,
            "clcbpat" | "clcbpatraw" => {
                let colour = self.colour(n);
                self.definition_mut().pending.shading.background = colour;
            }
            "clcfpat" | "clcfpatraw" => {
                let colour = self.colour(n);
                self.definition_mut().pending.shading.foreground = colour;
            }
            "clshdng" | "clshdngraw" => self.definition_mut().pending.shading.percent = Some(n),
            "clbrdrt" => self.set_border_target(BorderTarget::Cell(Edge::Top)),
            "clbrdrl" => self.set_border_target(BorderTarget::Cell(Edge::Left)),
            "clbrdrb" => self.set_border_target(BorderTarget::Cell(Edge::Bottom)),
            "clbrdrr" => self.set_border_target(BorderTarget::Cell(Edge::Right)),
            "trbrdrt" => self.set_border_target(BorderTarget::Row(Edge::Top)),
            "trbrdrl" => self.set_border_target(BorderTarget::Row(Edge::Left)),
            "trbrdrb" => self.set_border_target(BorderTarget::Row(Edge::Bottom)),
            "trbrdrr" => self.set_border_target(BorderTarget::Row(Edge::Right)),
            "trbrdrh" => self.set_border_target(BorderTarget::Row(Edge::Horizontal)),
            "trbrdrv" => self.set_border_target(BorderTarget::Row(Edge::Vertical)),
            "intbl" => self.top_mut().in_table = true,
            "itap" => self.top_mut().itap = usize::try_from(n).unwrap_or(0),
            "cell" => self.end_cell(false),
            "nestcell" => self.end_cell(true),
            "row" => self.end_row(false),
            "nestrow" => self.end_row(true),

            // Styles, by their numbers in the stylesheet.
            "s" => {
                let id = self
                    .style(n)
                    .filter(|entry| entry.kind == StyleKind::Paragraph)
                    .map(|entry| entry.id.clone());
                self.top_mut().paragraph.style = id.filter(|id| id != "Normal");
            }
            "cs" => {
                let id = self
                    .style(n)
                    .filter(|entry| entry.kind == StyleKind::Character)
                    .map(|entry| entry.id.clone());
                self.set_chars(|chars| chars.style = id);
            }

            // Sections and the page, for the whole document and then for
            // each section.
            "sect" => self.end_section(),
            "sectd" => self.section = PageSetup::default(),
            "facingp" => self.facing_pages = true,
            "paperw" => self.document_page.width = Some(n),
            "paperh" => self.document_page.height = Some(n),
            "margt" => self.document_page.margins[0] = Some(n),
            "margr" => self.document_page.margins[1] = Some(n),
            "margb" => self.document_page.margins[2] = Some(n),
            "margl" => self.document_page.margins[3] = Some(n),
            "landscape" => self.document_page.landscape = Some(true),
            "pgwsxn" => self.section.width = Some(n),
            "pghsxn" => self.section.height = Some(n),
            "margtsxn" => self.section.margins[0] = Some(n),
            "margrsxn" => self.section.margins[1] = Some(n),
            "margbsxn" => self.section.margins[2] = Some(n),
            "marglsxn" => self.section.margins[3] = Some(n),
            "lndscpsxn" => self.section.landscape = Some(true),
            "headery" => self.section.header_distance = Some(n),
            "footery" => self.section.footer_distance = Some(n),
            "cols" => self.section.columns = usize::try_from(n).ok().filter(|count| *count > 0),
            "colsx" => self.section.column_gap = Some(n),
            "sbknone" => self.section.start = Some(Start::Continuous),
            "sbkcol" => self.section.start = Some(Start::NextColumn),
            "sbkpage" => self.section.start = Some(Start::NextPage),
            "sbkeven" => self.section.start = Some(Start::EvenPage),
            "sbkodd" => self.section.start = Some(Start::OddPage),
            "titlepg" => self.section.title_page = Some(on),
            "pgnrestart" => {
                let numbering = self.section.numbering.get_or_insert_with(PageNumbering::default);
                numbering.start = numbering.start.or(Some(1));
            }
            "pgnstarts" => {
                self.section.numbering.get_or_insert_with(PageNumbering::default).start = Some(n);
            }
            "pgncont" => {
                if let Some(numbering) = &mut self.section.numbering {
                    numbering.start = None;
                }
            }
            "pgndec" | "pgnucrm" | "pgnlcrm" | "pgnucltr" | "pgnlcltr" => {
                let format = match word {
                    "pgnucrm" => NumberFormat::UpperRoman,
                    "pgnlcrm" => NumberFormat::LowerRoman,
                    "pgnucltr" => NumberFormat::UpperLetter,
                    "pgnlcltr" => NumberFormat::LowerLetter,
                    _ => NumberFormat::Decimal,
                };
                self.section.numbering.get_or_insert_with(PageNumbering::default).format = format;
            }

            // Fields: the instruction says what it is, the result is the text.
            "field" => {
                self.flush_run();
                let story = self.stories.len() - 1;
                let (paragraph, start) = {
                    let current = self.story();
                    (current.paragraphs_done, current.paragraph_length)
                };
                self.fields.push(FieldBeingRead {
                    instruction: String::new(),
                    paragraph,
                    start,
                    depth: self.stack.len(),
                    story,
                    spans: false,
                });
            }
            _ => self.formatting(word, number, n, on),
        }
    }

    /// How a paragraph or its characters look: the words the document's text
    /// and the stylesheet's entries share.
    #[allow(clippy::too_many_lines, reason = "one arm per control word is the readable shape")]
    fn formatting(&mut self, word: &str, number: Option<i32>, n: i32, on: bool) {
        match word {
            // Paragraph formatting.
            "ql" => self.top_mut().paragraph.alignment = Some(Alignment::Start),
            "qc" => self.top_mut().paragraph.alignment = Some(Alignment::Center),
            "qr" => self.top_mut().paragraph.alignment = Some(Alignment::End),
            "qj" | "qd" => self.top_mut().paragraph.alignment = Some(Alignment::Both),
            // The indents from the page's left and right, and from where the
            // text starts and ends: see `resolved_paragraph`.
            "li" => self.top_mut().sides.0 = Some(n),
            "ri" => self.top_mut().sides.1 = Some(n),
            "lin" => self.top_mut().paragraph.indent_start = Some(n),
            "rin" => self.top_mut().paragraph.indent_end = Some(n),
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
            "rtlpar" => self.top_mut().paragraph.right_to_left = Some(true),
            "ltrpar" => self.top_mut().paragraph.right_to_left = None,
            "ls" => {
                let id = self.list_id(n);
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
            "cbpat" => self.top_mut().shading.background = self.colour(n),
            "cfpat" => self.top_mut().shading.foreground = self.colour(n),
            "shading" => self.top_mut().shading.percent = Some(n),

            // Borders: which edge, and then what the line is.
            "brdrt" => self.set_border_target(BorderTarget::Paragraph(Edge::Top)),
            "brdrb" => self.set_border_target(BorderTarget::Paragraph(Edge::Bottom)),
            "brdrl" => self.set_border_target(BorderTarget::Paragraph(Edge::Left)),
            "brdrr" => self.set_border_target(BorderTarget::Paragraph(Edge::Right)),
            "brdrbtw" => self.set_border_target(BorderTarget::Paragraph(Edge::Horizontal)),
            "box" => self.set_border_target(BorderTarget::ParagraphBox),
            "brdrtbl" => self.clear_border(),
            "brdrw" => {
                let size = u32::try_from(n.max(1) * 2 / 5).unwrap_or(1).max(1);
                self.change_border(|border| border.size = size);
            }
            "brdrcf" => {
                let colour = self.colour(n);
                self.change_border(|border| border.color = colour.clone());
            }
            "brdrsh" => self.change_border(|border| {
                border.shadow = true;
                if !border.is_visible() {
                    "single".clone_into(&mut border.style);
                }
            }),
            "brdrframe" => self.change_border(|border| border.frame = true),
            _ if border_style(word).is_some() => {
                let style = border_style(word).unwrap_or("single");
                self.change_border(|border| style.clone_into(&mut border.style));
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
            "ulc" => {
                let colour = self.colour(n);
                self.set_chars(|chars| chars.underline_color = colour);
            }
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
                let colour = self.colour(n);
                self.set_chars(|chars| chars.color = colour);
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
            "up" => self.set_chars(|chars| chars.position_half_points = Some(n)),
            "dn" => self.set_chars(|chars| chars.position_half_points = Some(-n)),
            "expndtw" => self.set_chars(|chars| chars.spacing_twentieths = Some(n)),
            // In quarters of a point.
            "expnd" => self.set_chars(|chars| chars.spacing_twentieths = Some(n * 5)),
            "charscalex" => self.set_chars(|chars| chars.scale = u32::try_from(n).ok()),
            "kerning" => {
                self.set_chars(|chars| {
                    chars.kerning_half_points = u32::try_from(n).ok().filter(|size| *size > 0);
                });
            }
            "caps" => self.set_chars(|chars| chars.caps = Some(on)),
            "scaps" => self.set_chars(|chars| chars.small_caps = Some(on)),
            "v" => self.set_chars(|chars| chars.hidden = Some(on)),
            "noproof" => self.set_chars(|chars| chars.no_proof = Some(on)),
            "lang" => {
                let tag = language_tag(n).map(str::to_owned);
                self.set_chars(|chars| chars.language = tag);
            }
            // Which way the text runs. Word writes both for every run, the
            // one that holds last.
            "rtlch" => self.set_chars(|chars| chars.right_to_left = Some(true)),
            "ltrch" => self.set_chars(|chars| chars.right_to_left = None),

            // Tracked changes: whose, and when.
            "revised" => self.set_marks(|marks| marks.inserted = on),
            "revauth" => {
                self.set_marks(|marks| marks.inserted_by = usize::try_from(n).unwrap_or(0))
            }
            "revdttm" => self.set_marks(|marks| marks.inserted_at = n),
            "deleted" => self.set_marks(|marks| marks.deleted = on),
            "revauthdel" => {
                self.set_marks(|marks| marks.deleted_by = usize::try_from(n).unwrap_or(0));
            }
            "revdttmdel" => self.set_marks(|marks| marks.deleted_at = n),
            "crauth" => self.set_marks(|marks| {
                marks.formatted = true;
                marks.formatted_by = usize::try_from(n).unwrap_or(0);
            }),
            "crdate" => self.set_marks(|marks| {
                marks.formatted = true;
                marks.formatted_at = n;
            }),
            _ => {
                let _ = number;
            }
        }
    }

    /// The list an `\ls` number names.
    fn list_id(&self, ls: i32) -> i32 {
        self.overrides
            .iter()
            .find(|(number, _)| *number == ls)
            .and_then(|(_, id)| self.lists.iter().find(|list| list.id == *id))
            .map_or(wp_docx::NUMBERED_LIST, |list| {
                if list.bullet {
                    wp_docx::BULLET_LIST
                } else {
                    wp_docx::NUMBERED_LIST
                }
            })
    }

    /// Changes the character formatting, ending the run the old one made.
    fn set_chars(&mut self, change: impl FnOnce(&mut RunProperties)) {
        self.flush_run();
        change(&mut self.top_mut().chars);
    }

    fn set_marks(&mut self, change: impl FnOnce(&mut Marks)) {
        self.flush_run();
        change(&mut self.top_mut().marks);
    }

    /// The border words that follow are about this edge, which starts again
    /// from no line at all.
    fn set_border_target(&mut self, target: BorderTarget) {
        self.border = Some(target);
        self.for_each_border(|slot| *slot = Some(Border::line("none", 4, None)));
    }

    /// The edge the words are about says it has no line of its own.
    fn clear_border(&mut self) {
        self.for_each_border(|slot| *slot = None);
    }

    fn change_border(&mut self, change: impl Fn(&mut Border)) {
        self.for_each_border(|slot| {
            if let Some(border) = slot {
                change(border);
            }
        });
    }

    /// Every border the words are about: one edge, or four for a box.
    fn for_each_border(&mut self, mut each: impl FnMut(&mut Option<Border>)) {
        let Some(target) = self.border else { return };
        match target {
            BorderTarget::Paragraph(edge) => {
                each(paragraph_edge(&mut self.top_mut().paragraph.borders, edge));
            }
            BorderTarget::ParagraphBox => {
                let borders = &mut self.top_mut().paragraph.borders;
                for edge in [Edge::Top, Edge::Left, Edge::Bottom, Edge::Right] {
                    each(paragraph_edge(borders, edge));
                }
            }
            BorderTarget::Cell(edge) => {
                each(table_edge(&mut self.definition_mut().pending.borders, edge));
            }
            BorderTarget::Row(edge) => each(table_edge(&mut self.definition_mut().borders, edge)),
            BorderTarget::StyleTable(edge) => {
                if let Some(entry) = &mut self.style_being_read {
                    each(table_edge(&mut entry.table_borders, edge));
                }
            }
        }
    }

    // --- The ends of groups ----------------------------------------------------

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
                if let Some((index, mut font)) = self.font_being_read.take() {
                    font.finish_name(self.code_page);
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
            Destination::StyleSheet => {
                // An entry whose name had no semicolon after it.
                if self.stylesheet_depth.is_some_and(|depth| self.stack.len() == depth + 1) {
                    self.finish_style();
                }
                if self.stylesheet_depth == Some(self.stack.len()) {
                    self.stylesheet_depth = None;
                    self.style_being_read = None;
                }
            }
            Destination::Picture if parent != Destination::Picture => self.finish_picture(),
            Destination::Tag(tag) if parent != closing => {
                self.flush_bytes();
                let value = core::mem::take(&mut self.text);
                self.finish_tag(tag, value.trim());
            }
            Destination::ShapeName if parent != closing => {
                self.flush_bytes();
                let name = core::mem::take(&mut self.text);
                if let Some(shape) = &mut self.shape {
                    shape.name = name.trim().to_owned();
                }
            }
            Destination::ShapeValue if parent != closing => {
                self.flush_bytes();
                let value = core::mem::take(&mut self.text);
                if let Some(shape) = &mut self.shape {
                    let name = core::mem::take(&mut shape.name);
                    shape.properties.push((name, value));
                }
            }
            _ => {}
        }
        // The formatting a tracked change replaced, for the group it is in.
        if self.top().old_properties && !self.stack[self.stack.len() - 2].old_properties {
            let before = self.top().chars.clone();
            let index = self.stack.len() - 2;
            self.stack[index].before = Some(before);
        }
        // A story ends with its group, while the group's state is still
        // there for its last paragraph.
        if self.stories.len() > 1 && self.story().depth == self.stack.len() {
            self.finish_story();
        }
        self.stack.pop();

        // A drawing ends with its group, and goes where the group was.
        if self.shape.as_ref().is_some_and(|shape| self.stack.len() < shape.depth) {
            self.finish_shape();
        }
        // The end of a field's own group: what its instruction said decides
        // what its result was.
        while self.fields.last().is_some_and(|field| self.stack.len() < field.depth) {
            self.finish_field();
        }
    }

    fn finish_tag(&mut self, tag: Tag, value: &str) {
        let main = self.in_main_story();
        let at = self.main_position();
        match tag {
            Tag::BookmarkStart if main && !value.is_empty() => {
                self.bookmarks_open.push((value.to_owned(), at));
            }
            Tag::BookmarkEnd if main => {
                if let Some(index) = self.bookmarks_open.iter().rposition(|(name, _)| name == value)
                {
                    let (name, start) = self.bookmarks_open.remove(index);
                    self.bookmarks.push(BookmarkFound { name, start, end: at });
                }
            }
            Tag::CommentStart if main => self.comment_starts.push((value.to_owned(), at)),
            Tag::CommentEnd if main => self.comment_ends.push((value.to_owned(), at)),
            Tag::CommentAuthor => value.clone_into(&mut self.comment_author),
            Tag::CommentReference => value.clone_into(&mut self.comment_reference),
            Tag::CommentDate => self.comment_date = value.parse().ok(),
            _ => {}
        }
    }

    /// A story's group closed: it goes where its kind says.
    fn finish_story(&mut self) {
        let unfinished = !self.story().runs.is_empty()
            || !self.text.is_empty()
            || !self.pending_bytes.is_empty();
        if unfinished {
            self.end_paragraph();
        }
        self.close_tables_to(0);
        let Some(story) = self.stories.pop() else { return };
        let pictures = story.pictures;
        let mut blocks = story.blocks;
        if blocks.is_empty() {
            blocks.push(Block::Paragraph(Paragraph::default()));
        }
        let body = Body { blocks };
        match story.kind {
            StoryKind::Main => {}
            StoryKind::Furniture(kind, which) => {
                self.furniture.push(FurnitureFound { kind, which, body, pictures });
            }
            StoryKind::Note { reference, endnote } => {
                let id = if endnote {
                    self.endnotes += 1;
                    self.endnotes
                } else {
                    self.footnotes += 1;
                    self.footnotes
                };
                if let Some(run) = self.stories[0].runs.get_mut(reference) {
                    run.content = vec![RunContent::NoteReference { id, endnote }];
                }
                self.notes.push(NoteFound { id, endnote, body, pictures });
            }
            StoryKind::Comment { at } => {
                let reference = core::mem::take(&mut self.comment_reference);
                let find = |marks: &[(String, TextPosition)]| {
                    marks
                        .iter()
                        .rev()
                        .find(|(name, _)| !reference.is_empty() && *name == reference)
                        .map(|(_, place)| *place)
                };
                let (start, end) = match (find(&self.comment_starts), find(&self.comment_ends)) {
                    (Some(start), Some(end)) if start <= end => (start, end),
                    _ => (at, at),
                };
                self.comments.push(CommentFound {
                    start,
                    end,
                    author: core::mem::take(&mut self.comment_author),
                    date: self.comment_date.take().map(dttm).unwrap_or_default(),
                    body,
                    pictures,
                });
            }
            StoryKind::TextBox => {
                if let Some(shape) = &mut self.shape {
                    shape.text = body
                        .blocks
                        .into_iter()
                        .filter_map(|block| match block {
                            Block::Paragraph(paragraph) => Some(paragraph),
                            Block::Table(_) => None,
                        })
                        .collect();
                }
            }
        }
    }

    fn finish_field(&mut self) {
        let Some(field) = self.fields.pop() else { return };
        // A link in the document's own text is put in as a link; anywhere
        // else its runs carry it as the field it is.
        if field.story != 0 || field.spans || field.paragraph != self.stories[0].paragraphs_done {
            return;
        }
        if let Some(address) = link_address(field.instruction.trim()) {
            self.links.push(LinkFound {
                paragraph: field.paragraph,
                start: field.start,
                end: self.stories[0].paragraph_length,
                address,
            });
        }
    }

    /// The picture group closed: its bytes become a picture, and a mark
    /// stands for it in the text.
    fn finish_picture(&mut self) {
        let Some(picture) = self.picture.take() else { return };
        let Some(kind) = picture.kind else { return };
        let measures = picture_measures(&picture);
        let bytes = if picture.raw.is_empty() { decode_hex(&picture.hex) } else { picture.raw };
        let (bytes, extension) = match kind {
            PictureKind::Png => (bytes, "png"),
            PictureKind::Jpeg => (bytes, "jpeg"),
            PictureKind::Emf => (bytes, "emf"),
            PictureKind::Wmf => (bytes, "wmf"),
            PictureKind::Dib => match bitmap_file(&bytes) {
                Some(file) => (file, "bmp"),
                None => return,
            },
            PictureKind::Other => return,
        };
        if bytes.is_empty() {
            return;
        }
        // A picture a drawing frames goes with the drawing.
        if let Some(shape) = &mut self.shape {
            shape.picture = Some((bytes, extension));
            return;
        }
        let metafile = matches!(kind, PictureKind::Emf | PictureKind::Wmf);
        let (width_emu, height_emu) = picture_size(&measures, &bytes, metafile);
        // The mark goes in with the formatting of the group outside the
        // picture, which is the state under the one being closed.
        let properties = self.stack[self.stack.len() - 2].chars.clone();
        self.place_picture(bytes, extension, (width_emu, height_emu), None, properties);
    }

    /// Puts a picture's mark into the text of the story it is in, and the
    /// picture beside it. The words in a text box are inside a drawing, where
    /// no place can be counted to, and a picture among them is not carried.
    fn place_picture(
        &mut self,
        bytes: Vec<u8>,
        extension: &'static str,
        (width_emu, height_emu): (i64, i64),
        anchor: Option<Anchor>,
        properties: RunProperties,
    ) {
        if self.story().kind == StoryKind::TextBox {
            return;
        }
        self.flush_run();
        let story = self.story_mut();
        story.pictures.push(PictureFound {
            paragraph: story.paragraphs_done,
            offset: story.paragraph_length,
            bytes,
            extension,
            width_emu,
            height_emu,
            anchor,
        });
        story.runs.push(Run {
            properties,
            content: vec![RunContent::Text(PICTURE_MARK.to_string())],
            field: None,
            revision: None,
            format_change: None,
        });
        // One character, as the picture that takes its place will be.
        story.paragraph_length += 1;
    }

    /// The drawing's group closed: it becomes a shape, or a picture if it
    /// frames one, where it was in the text.
    fn finish_shape(&mut self) {
        let Some(shape) = self.shape.take() else { return };
        let kind =
            shape.number("shapeType").unwrap_or(if shape.picture.is_some() { 75 } else { 1 });
        let [left, top, right, bottom] = shape.bounds;
        let width_emu = i64::from((right - left).max(0)) * EMU_PER_TWIP;
        let height_emu = i64::from((bottom - top).max(0)) * EMU_PER_TWIP;
        // Word writes a drawing in the line as a floating one that says so.
        let anchor = (shape.number("fPseudoInline") != Some(1)).then(|| shape.anchor());

        if let Some((bytes, extension)) = shape.picture.clone() {
            let properties = self.top().chars.clone();
            self.place_picture(bytes, extension, (width_emu, height_emu), anchor, properties);
            return;
        }
        let Some(preset) = wp_docx::shapes::office_preset(kind) else { return };
        let line = matches!(preset, "line" | "straightConnector1");
        let fill = if line || shape.number("fFilled") == Some(0) {
            Fill::None
        } else {
            Fill::Solid(Colour::rgb(
                &shape.colour("fillColor").unwrap_or_else(|| "FFFFFF".to_owned()),
            ))
        };
        let lined = shape.number("fLine") != Some(0);
        let outline = lined.then(|| {
            Colour::rgb(&shape.colour("lineColor").unwrap_or_else(|| "000000".to_owned()))
        });
        // A turn is a whole number of degrees and a fraction in sixty-five
        // thousand five hundred and thirty-sixths.
        let rotation = shape
            .number("rotation")
            .map_or(0, |turn| i32::try_from(turn * 60_000 / 65_536).unwrap_or(0));
        let name = shape.property("wzName").map_or_else(
            || if kind == 202 { "Text Box".to_owned() } else { "Shape".to_owned() },
            str::to_owned,
        );
        let drawn = Shape {
            preset: preset.to_owned(),
            width_emu,
            height_emu,
            fill,
            outline,
            outline_emu: if lined { shape.number("lineWidth").unwrap_or(9525) } else { 0 },
            ink: None,
            name,
            id: shape.id,
            anchor,
            description: shape.property("wzDescription").unwrap_or_default().to_owned(),
            rotation,
            flipped_across: shape.number("fFlipH") == Some(1),
            flipped_down: shape.number("fFlipV") == Some(1),
            text: shape.text,
            ..Shape::default()
        };
        self.content(RunContent::Shape(Box::new(drawn)), 1);
    }

    /// The end of the file: whatever was still being built is finished.
    fn finish(mut self) -> Reading {
        // Anything a file cut short left open.
        while self.stories.len() > 1 {
            self.finish_story();
        }
        let unfinished = !self.stories[0].runs.is_empty()
            || !self.text.is_empty()
            || !self.pending_bytes.is_empty();
        if unfinished {
            self.end_paragraph();
        }
        self.close_tables_to(0);
        let main = self.stories.swap_remove(0);
        let pictures = main.pictures;
        let mut blocks = main.blocks;
        if blocks.is_empty() {
            blocks.push(Block::Paragraph(Paragraph::default()));
        }
        let paragraphs = main.paragraphs_done.max(1);

        // The last section ends with the document. A break at the very end
        // leaves nothing after it, and that section is the last one.
        let mut last = SectionFound {
            last_paragraph: None,
            page: self.section.over(&self.document_page),
            furniture: core::mem::take(&mut self.furniture),
        };
        if let Some(ended) = self.sections.pop() {
            if ended.last_paragraph.is_some_and(|paragraph| paragraph + 1 >= paragraphs) {
                if last.furniture.is_empty() {
                    last.furniture = ended.furniture;
                }
                last.page = ended.page;
            } else {
                self.sections.push(ended);
            }
        }
        self.sections.push(last);

        let styles = self.styles_found();
        Reading {
            body: Body { blocks },
            pictures,
            links: self.links,
            bookmarks: self.bookmarks,
            comments: self.comments,
            notes: self.notes,
            sections: self.sections,
            facing_pages: self.facing_pages,
            styles,
        }
    }

    /// The stylesheet's styles, with their names for one another.
    fn styles_found(&self) -> Vec<StyleFound> {
        let id_of = |index: Option<i32>| {
            index.and_then(|wanted| self.style(wanted)).map(|entry| entry.id.clone())
        };
        self.styles
            .iter()
            .filter(|entry| entry.kind != StyleKind::Other)
            .map(|entry| {
                let mut paragraph = entry.paragraph.clone();
                if let Some(list) = entry.list {
                    paragraph.numbering =
                        Some(NumberingReference { id: self.list_id(list), level: entry.level });
                }
                StyleFound {
                    id: entry.id.clone(),
                    name: entry.name.clone(),
                    kind: entry.kind,
                    based_on: id_of(entry.based_on).filter(|id| *id != entry.id),
                    next: id_of(entry.next),
                    paragraph,
                    run: entry.run.clone(),
                    table_borders: entry.table_borders.clone(),
                }
            })
            .collect()
    }
}

/// The address a `HYPERLINK` field goes to: its address, or `#` and the
/// place in the document `\l` names, or both.
fn link_address(instruction: &str) -> Option<String> {
    let rest = instruction.strip_prefix("HYPERLINK")?;
    let mut address = None;
    let mut place = None;
    let mut pieces = arguments(rest).into_iter();
    while let Some(piece) = pieces.next() {
        match piece.as_str() {
            "\\l" => place = pieces.next(),
            "\\o" | "\\t" => {
                pieces.next();
            }
            switch if switch.starts_with('\\') => {}
            _ => {
                if address.is_none() {
                    address = Some(piece);
                }
            }
        }
    }
    let found = match (address, place) {
        (Some(address), Some(place)) => format!("{address}#{place}"),
        (Some(address), None) => address,
        (None, Some(place)) => format!("#{place}"),
        (None, None) => return None,
    };
    (!found.is_empty() && found != "#").then_some(found)
}

/// A field instruction's arguments: words, or anything in quotes, where a
/// doubled backslash is one.
fn arguments(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut characters = text.chars().peekable();
    while let Some(&next) = characters.peek() {
        if next.is_whitespace() {
            characters.next();
            continue;
        }
        let mut piece = String::new();
        if next == '"' {
            characters.next();
            while let Some(character) = characters.next() {
                match character {
                    '"' => break,
                    '\\' if characters.peek() == Some(&'\\') => {
                        characters.next();
                        piece.push('\\');
                    }
                    other => piece.push(other),
                }
            }
        } else {
            while let Some(&character) = characters.peek() {
                if character.is_whitespace() {
                    break;
                }
                piece.push(character);
                characters.next();
            }
        }
        out.push(piece);
    }
    out
}

/// Word's packed date: minutes, hours, day, month and years since 1900, in
/// six, five, five, four and nine bits.
fn dttm(value: i32) -> String {
    let value = u32::from_ne_bytes(value.to_ne_bytes());
    let minute = value & 0x3F;
    let hour = (value >> 6) & 0x1F;
    let day = (value >> 11) & 0x1F;
    let month = (value >> 16) & 0x0F;
    let year = 1900 + ((value >> 20) & 0x1FF);
    if day == 0 || !(1..=12).contains(&month) {
        return String::new();
    }
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:00Z")
}

/// How big a picture is, in English metric units: twips are what Word writes
/// for the size on the page, and what it has been scaled by; a metafile's
/// own size is in hundredths of a millimetre, and a bitmap's in pixels at
/// ninety-six to the inch.
fn picture_size(measures: &Measures, bytes: &[u8], metafile: bool) -> (i64, i64) {
    let (scale_x, scale_y) = measures.scale;
    let (width, height) = match (measures.width_twips, measures.height_twips) {
        (Some(width), Some(height)) if width > 0 && height > 0 => {
            (width * EMU_PER_TWIP, height * EMU_PER_TWIP)
        }
        _ => match (measures.width_pixels, measures.height_pixels) {
            (Some(width), Some(height)) if width > 0 && height > 0 && metafile => {
                (width * 360, height * 360)
            }
            (Some(width), Some(height)) if width > 0 && height > 0 => (width * 9525, height * 9525),
            _ => {
                let (width, height) = wp_image::decode(bytes)
                    .map(|image| {
                        (
                            i64::try_from(image.width).unwrap_or(96),
                            i64::try_from(image.height).unwrap_or(96),
                        )
                    })
                    .unwrap_or((96, 96));
                (width * 9525, height * 9525)
            }
        },
    };
    (width * scale_x / 100, height * scale_y / 100)
}

/// The numbers a picture's size is worked out from.
struct Measures {
    width_twips: Option<i64>,
    height_twips: Option<i64>,
    width_pixels: Option<i64>,
    height_pixels: Option<i64>,
    scale: (i64, i64),
}

fn picture_measures(picture: &PictureBeingRead) -> Measures {
    Measures {
        width_twips: picture.width_twips,
        height_twips: picture.height_twips,
        width_pixels: picture.width_pixels,
        height_pixels: picture.height_pixels,
        scale: picture.scale,
    }
}

/// A device-independent bitmap made a `.bmp` file: the fourteen bytes of
/// file header it was written without, which say where the pixels begin.
fn bitmap_file(dib: &[u8]) -> Option<Vec<u8>> {
    let word = |at: usize| dib.get(at..at + 2).map(|b| u32::from(u16::from_le_bytes([b[0], b[1]])));
    let long =
        |at: usize| dib.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    let header = long(0)?;
    let (bits, colours, entry, masks) = if header == 12 {
        // The old core header: three bytes to a colour.
        (word(10)?, 0, 3, 0)
    } else {
        let compression = long(16)?;
        let masks = if header == 40 && compression == 3 { 12 } else { 0 };
        (word(14)?, long(32)?, 4, masks)
    };
    let palette = match (colours, bits) {
        (0, 1..=8) => 1u32 << bits,
        (count, _) => count,
    };
    let offset = 14 + header + palette * entry + masks;
    let size = 14 + u32::try_from(dib.len()).ok()?;
    let mut file = Vec::with_capacity(dib.len() + 14);
    file.extend_from_slice(b"BM");
    file.extend_from_slice(&size.to_le_bytes());
    file.extend_from_slice(&[0; 4]);
    file.extend_from_slice(&offset.to_le_bytes());
    file.extend_from_slice(dib);
    Some(file)
}

/// The line a border word names, as the model names it.
fn border_style(word: &str) -> Option<&'static str> {
    Some(match word {
        "brdrs" | "brdrhair" => "single",
        "brdrth" => "thick",
        "brdrdb" => "double",
        "brdrdot" => "dotted",
        "brdrdash" => "dashed",
        "brdrdashsm" => "dashSmallGap",
        "brdrdashd" => "dotDash",
        "brdrdashdd" => "dotDotDash",
        "brdrdashdotstr" => "dashDotStroked",
        "brdrtriple" => "triple",
        "brdrtnthsg" => "thinThickSmallGap",
        "brdrthtnsg" => "thickThinSmallGap",
        "brdrtnthtnsg" => "thinThickThinSmallGap",
        "brdrtnthmg" => "thinThickMediumGap",
        "brdrthtnmg" => "thickThinMediumGap",
        "brdrtnthtnmg" => "thinThickThinMediumGap",
        "brdrtnthlg" => "thinThickLargeGap",
        "brdrthtnlg" => "thickThinLargeGap",
        "brdrtnthtnlg" => "thinThickThinLargeGap",
        "brdrwavy" => "wave",
        "brdrwavydb" => "doubleWave",
        "brdremboss" => "threeDEmboss",
        "brdrengrave" => "threeDEngrave",
        "brdrinset" => "inset",
        "brdroutset" => "outset",
        "brdrnone" => "none",
        "brdrnil" => "nil",
        _ => return None,
    })
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
        128 => Some(932),
        129 => Some(949),
        134 => Some(936),
        136 => Some(950),
        _ => None,
    }
}

/// The identifier Word gives a style of this name: the words run together,
/// each begun with a capital, which is `Heading1` for "heading 1" and
/// `FootnoteText` for "footnote text" — and a few built-in ones Word names
/// otherwise.
fn style_id(name: &str, index: i32) -> String {
    match name.to_lowercase().as_str() {
        "normal table" => return "TableNormal".to_owned(),
        "annotation text" => return "CommentText".to_owned(),
        "annotation reference" => return "CommentReference".to_owned(),
        "annotation subject" => return "CommentSubject".to_owned(),
        _ => {}
    }
    let mut id = String::new();
    for word in name.split_whitespace() {
        let mut characters = word.chars().filter(|c| c.is_alphanumeric());
        if let Some(first) = characters.next() {
            id.extend(first.to_uppercase());
            id.extend(characters);
        }
    }
    if id.is_empty() {
        format!("Style{index}")
    } else {
        id
    }
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
        _ => return None,
    })
}
