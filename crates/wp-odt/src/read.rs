//! Reading an OpenDocument text into the model.
//!
//! # How the format is put together
//!
//! A ZIP with the parts of the document as XML: `content.xml` holds the
//! text and the automatic styles it uses — one per distinct formatting,
//! named `P1`, `T3` — and `styles.xml` the named styles the text refers to,
//! the page layouts, the master pages with their headers and footers, and
//! the fonts. `meta.xml` is what the document says about itself, and
//! `settings.xml` how it was last looked at. See [`crate::styles`] for how
//! the styles are folded.
//!
//! # Stories
//!
//! Most of what is not the document's own text is written where it
//! belongs: a note's words in the middle of the sentence that points at it,
//! a comment's where it is anchored, a text box's inside the frame that is
//! its box; a header's in its master page. Each is a *story*, read exactly
//! as the document's own text is — paragraphs, lists, tables — into
//! paragraphs of its own and put where it belongs. Places in the document's
//! own text — a bookmark's, a comment's range, where a page style begins —
//! are counted as the document will count them once the pictures are in:
//! a picture, a drawing and a note's mark are one character each, and
//! deleted text is no characters at all.
//!
//! # Tracked changes
//!
//! Kept apart from the text: a list of every change at the head of it, each
//! with who and when, and marks in the text — the start and the end of an
//! insertion or a change of formatting, and the point a deletion was
//! taken from, whose words are in the list.

use std::collections::HashMap;

use wp_docx::fills::Fill;
use wp_docx::furniture::{Furniture, Which};
use wp_docx::model::{
    Block, Body, BreakKind, FormatChange, NumberingReference, Paragraph, Revision, RevisionKind,
    Run, RunContent, RunProperties, Table, TableCell, TableRow,
};
use wp_docx::properties::Properties;
use wp_docx::sections::{PageNumbering, Start};
use wp_docx::shapes::Shape;
use wp_docx::TextPosition;
use wp_xml::tree::{Element, Node, XmlTree};

use crate::styles::{
    anchor_of, twips, PageSetup, Styles, CONFIG, DC, DRAW, META, OFFICE, SVG, TABLE, TEXT, XLINK,
};

/// The character that stands where a picture goes in the text, until the
/// picture is put in.
pub const PICTURE_MARK: char = '\u{FFFC}';

/// What was read.
#[derive(Debug, Default)]
pub struct Reading {
    pub body: Body,
    /// The pictures, each with where it goes: the paragraph, counted through
    /// the whole document in order, and the offset in it of the mark that
    /// stands for it.
    pub pictures: Vec<PictureFound>,
    pub links: Vec<LinkFound>,
    pub bookmarks: Vec<BookmarkFound>,
    pub comments: Vec<CommentFound>,
    /// What the notes say, for the marks already in the body.
    pub notes: Vec<NoteFound>,
    /// The sections, in order; there is always at least one.
    pub sections: Vec<SectionFound>,
    /// Whether left-hand pages have headers and footers of their own.
    pub facing_pages: bool,
    /// The named character styles, which runs name.
    pub styles: Vec<StyleFound>,
    /// The table of contents: where it goes and what it gathers.
    pub contents: Option<ContentsFound>,
    /// Whether changes were being recorded.
    pub tracking: bool,
    pub properties: Properties,
    /// How it was last looked at: the zoom, and whether it asks to be opened
    /// only for reading.
    pub zoom: Option<u32>,
    pub read_only: bool,
    /// The font table: what the declarations say of each font.
    pub fonts: Vec<wp_docx::fonts::FontEntry>,
}

#[derive(Debug)]
pub struct PictureFound {
    pub paragraph: usize,
    pub offset: usize,
    /// The name of the file in the package the picture is.
    pub name: String,
    pub width_emu: i64,
    pub height_emu: i64,
    /// Where it floats, for a picture that is not in the line.
    pub anchor: Option<wp_docx::anchor::Anchor>,
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
    /// An ISO 8601 timestamp, or nothing.
    pub date: String,
    pub body: Body,
    pub pictures: Vec<PictureFound>,
}

#[derive(Debug)]
pub struct NoteFound {
    pub id: i32,
    pub endnote: bool,
    pub body: Body,
    pub pictures: Vec<PictureFound>,
}

/// One of a section's headers or footers.
#[derive(Debug)]
pub struct FurnitureFound {
    pub kind: Furniture,
    pub which: Which,
    pub body: Body,
    pub pictures: Vec<PictureFound>,
}

#[derive(Debug, Default)]
pub struct SectionFound {
    /// The paragraph it ends with; the last section has none.
    pub last_paragraph: Option<usize>,
    pub start: Start,
    pub page: PageSetup,
    /// Its own headers and footers; none means the section before it's.
    pub furniture: Vec<FurnitureFound>,
}

#[derive(Debug)]
pub struct StyleFound {
    pub id: String,
    pub name: String,
    pub based_on: Option<String>,
    pub run: RunProperties,
}

#[derive(Debug)]
pub struct ContentsFound {
    /// The paragraph it goes before.
    pub paragraph: usize,
    /// How many heading levels it gathers.
    pub levels: u8,
    /// The page numbers its entries showed, in order.
    pub pages: Vec<usize>,
}

/// Reads the parts of a document.
pub fn read(
    content: &str,
    styles_xml: Option<&str>,
    meta: Option<&str>,
    settings: Option<&str>,
) -> Result<Reading, wp_xml::Error> {
    let content = XmlTree::parse(content)?;
    let styles_tree = styles_xml.map(XmlTree::parse).transpose()?;
    // The masters' own automatic styles are theirs: the text's may use the
    // same names for other things.
    let mut master_styles = Styles::default();
    if let Some(tree) = &styles_tree {
        master_styles.read(&tree.root);
    }
    let mut styles = master_styles.clone();
    styles.read(&content.root);

    let mut reader = Reader::new(&styles, true);
    let mut body = Body::default();
    if let Some(text) =
        content.root.child(Some(OFFICE), "body").and_then(|body| body.child(Some(OFFICE), "text"))
    {
        reader.tracked_changes(text);
        body.blocks = reader.blocks(text, &[]);
    }
    if body.blocks.is_empty() {
        body.blocks.push(Block::Paragraph(Paragraph::default()));
        reader.story_mut().paragraphs += 1;
    }
    let paragraphs = reader.story().paragraphs;
    let sections = reader.sections(&master_styles, paragraphs);
    let facing_pages = styles
        .masters
        .iter()
        .any(|(_, master)| master.furniture.iter().any(|(which, _)| which.ends_with("-left")));
    let styles_found = character_styles(&styles);
    let properties = meta.map(properties_of).unwrap_or_default();
    let (zoom, read_only) = settings.map(settings_of).unwrap_or((None, false));
    let story = reader.stories.pop().unwrap_or_default();
    Ok(Reading {
        body,
        pictures: story.pictures,
        links: reader.links,
        bookmarks: reader.bookmarks,
        comments: reader.comments,
        notes: reader.notes,
        sections,
        facing_pages,
        styles: styles_found,
        contents: reader.contents,
        tracking: reader.tracking,
        properties,
        zoom,
        read_only,
        fonts: styles.font_table.clone(),
    })
}

/// What `meta.xml` says of the document.
fn properties_of(meta: &str) -> Properties {
    let mut properties = Properties::default();
    let Ok(tree) = XmlTree::parse(meta) else { return properties };
    let Some(meta) = tree.root.child(Some(OFFICE), "meta") else { return properties };
    let text = |namespace: &str, local: &str| {
        meta.child(Some(namespace), local).map(|child| child.text_content().trim().to_owned())
    };
    properties.title = text(DC, "title").unwrap_or_default();
    properties.subject = text(DC, "subject").unwrap_or_default();
    properties.description = text(DC, "description").unwrap_or_default();
    properties.author = text(META, "initial-creator").unwrap_or_default();
    properties.last_modified_by = text(DC, "creator").unwrap_or_default();
    properties.created = text(META, "creation-date").map(|date| iso(&date)).unwrap_or_default();
    properties.modified = text(DC, "date").map(|date| iso(&date)).unwrap_or_default();
    properties.keywords = meta
        .children_named(Some(META), "keyword")
        .map(|keyword| keyword.text_content().trim().to_owned())
        .filter(|keyword| !keyword.is_empty())
        .collect::<Vec<_>>()
        .join(", ");
    properties
}

/// The zoom and the read-only wish, from `settings.xml`.
fn settings_of(settings: &str) -> (Option<u32>, bool) {
    let Ok(tree) = XmlTree::parse(settings) else { return (None, false) };
    let mut items = Vec::new();
    collect_items(&tree.root, &mut items);
    let find =
        |name: &str| items.iter().find(|(held, _)| held == name).map(|(_, value)| value.as_str());
    let zoom = find("ZoomFactor").and_then(|value| value.parse::<u32>().ok()).filter(|z| *z > 0);
    (zoom, find("LoadReadonly") == Some("true"))
}

fn collect_items(element: &Element, out: &mut Vec<(String, String)>) {
    for child in element.child_elements() {
        if child.namespace.as_deref() == Some(CONFIG) && child.local_name() == "config-item" {
            if let Some(name) = child.attribute(Some(CONFIG), "name") {
                out.push((name.to_owned(), child.text_content().trim().to_owned()));
            }
        } else {
            collect_items(child, out);
        }
    }
}

/// A date as this format writes one, as the model does: seconds, and in
/// universal time where nothing else is said.
fn iso(date: &str) -> String {
    let date = date.trim();
    let (whole, _) = date.split_once('.').unwrap_or((date, ""));
    if whole.len() == 19 {
        format!("{whole}Z")
    } else {
        whole.to_owned()
    }
}

/// The identifier Word gives a style of this name: the words run together,
/// each begun with a capital.
fn style_id(name: &str) -> String {
    let mut id = String::new();
    for word in name.split_whitespace() {
        let mut characters = word.chars().filter(|c| c.is_alphanumeric());
        if let Some(first) = characters.next() {
            id.extend(first.to_uppercase());
            id.extend(characters);
        }
    }
    if id.is_empty() {
        "Style".to_owned()
    } else {
        id
    }
}

/// The named text styles, as the model's character styles.
fn character_styles(styles: &Styles) -> Vec<StyleFound> {
    styles
        .named_text
        .iter()
        .map(|named| StyleFound {
            id: style_id(&named.display_name),
            name: named.display_name.clone(),
            based_on: named.parent.as_ref().and_then(|parent| {
                styles
                    .named_text
                    .iter()
                    .find(|other| other.name == *parent)
                    .map(|other| style_id(&other.display_name))
            }),
            run: named.own.clone(),
        })
        .collect()
}

// --- The text -------------------------------------------------------------------------

/// A list the reader is inside: its style, and how deep.
#[derive(Clone, Debug)]
struct ListContext {
    style: Option<String>,
    level: u8,
}

/// One story being read: how many of its paragraphs are done, and its
/// pictures.
#[derive(Debug, Default)]
struct Story {
    paragraphs: usize,
    pictures: Vec<PictureFound>,
}

/// A change the list at the head of the text describes.
#[derive(Clone, Debug)]
struct Change {
    kind: Option<RevisionKind>,
    author: String,
    date: String,
    id: i32,
    /// For a deletion, what was deleted.
    deleted: Option<Element>,
}

/// A place a section may begin: the paragraph, the page style begun there
/// with the number its first page takes, and the columns a section of its
/// own begins there or, as `Some(None)`, the columns it ends.
type Boundary = (usize, Option<(String, Option<i32>)>, Option<Option<(usize, i32)>>);

/// Where a section begins: the paragraph, how, its master page, its columns
/// and the number its first page takes.
type SectionStart = (usize, Start, String, Option<(usize, i32)>, Option<i32>);

/// Where a page style begins, or a section of its own.
#[derive(Clone, Debug)]
enum Mark {
    Page(String, Option<i32>),
    Columns(usize, i32),
    End,
}

/// A paragraph being built.
struct Building {
    runs: Vec<Run>,
    text: String,
    /// How long it is as the document counts.
    offset: usize,
    /// The formatting the text here has.
    chars: RunProperties,
    /// What the paragraph's style gives its text.
    base: RunProperties,
    /// The field whose result is being read.
    field: Option<String>,
    /// Whether this is deleted text, which counts for nothing.
    deleted: bool,
}

struct Reader<'a> {
    styles: &'a Styles,
    /// Whether this is the document's own text rather than a master page's.
    main: bool,
    stories: Vec<Story>,
    links: Vec<LinkFound>,
    bookmarks: Vec<BookmarkFound>,
    open_bookmarks: HashMap<String, TextPosition>,
    comments: Vec<CommentFound>,
    open_comments: HashMap<String, usize>,
    notes: Vec<NoteFound>,
    footnotes: i32,
    endnotes: i32,
    changes: HashMap<String, Change>,
    active: Vec<String>,
    tracking: bool,
    marks: Vec<(usize, Mark)>,
    contents: Option<ContentsFound>,
    /// Drawings anchored to the page, for the next paragraph to carry.
    waiting: Vec<Element>,
}

impl<'a> Reader<'a> {
    fn new(styles: &'a Styles, main: bool) -> Self {
        Self {
            styles,
            main,
            stories: vec![Story::default()],
            links: Vec::new(),
            bookmarks: Vec::new(),
            open_bookmarks: HashMap::new(),
            comments: Vec::new(),
            open_comments: HashMap::new(),
            notes: Vec::new(),
            footnotes: 0,
            endnotes: 0,
            changes: HashMap::new(),
            active: Vec::new(),
            tracking: false,
            marks: Vec::new(),
            contents: None,
            waiting: Vec::new(),
        }
    }

    fn story(&self) -> &Story {
        self.stories.last().expect("a story")
    }

    fn story_mut(&mut self) -> &mut Story {
        self.stories.last_mut().expect("a story")
    }

    /// Whether the text being read is the document's own.
    fn in_main(&self) -> bool {
        self.main && self.stories.len() == 1
    }

    /// Reads some blocks as a story of their own: their paragraphs counted
    /// from nought, their pictures their own.
    fn story_of(&mut self, element: &Element) -> (Vec<Block>, Vec<PictureFound>) {
        self.stories.push(Story::default());
        let active = core::mem::take(&mut self.active);
        let mut blocks = self.blocks(element, &[]);
        if blocks.is_empty() {
            blocks.push(Block::Paragraph(Paragraph::default()));
        }
        self.active = active;
        let story = self.stories.pop().unwrap_or_default();
        (blocks, story.pictures)
    }

    /// The list of changes at the head of the text.
    fn tracked_changes(&mut self, text: &Element) {
        let Some(list) = text.child(Some(TEXT), "tracked-changes") else { return };
        self.tracking = list.attribute(Some(TEXT), "track-changes") == Some("true");
        for (index, region) in list.children_named(Some(TEXT), "changed-region").enumerate() {
            let Some(id) = region
                .attribute(Some(TEXT), "id")
                .or_else(|| region.attribute(Some("http://www.w3.org/XML/1998/namespace"), "id"))
            else {
                continue;
            };
            let Some(what) = region.child_elements().next() else { continue };
            let kind = match what.local_name() {
                "insertion" => Some(RevisionKind::Inserted),
                "deletion" => Some(RevisionKind::Deleted),
                _ => None,
            };
            let info = what.child(Some(OFFICE), "change-info");
            let text = |namespace: &str, local: &str| {
                info.and_then(|info| info.child(Some(namespace), local))
                    .map(|child| child.text_content().trim().to_owned())
                    .unwrap_or_default()
            };
            self.changes.insert(
                id.to_owned(),
                Change {
                    kind,
                    author: text(DC, "creator"),
                    date: iso(&text(DC, "date")),
                    id: i32::try_from(index + 1).unwrap_or(i32::MAX),
                    deleted: (kind == Some(RevisionKind::Deleted)).then(|| what.clone()),
                },
            );
        }
    }

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
                (Some(TEXT), "section") => {
                    let columns = child
                        .attribute(Some(TEXT), "style-name")
                        .and_then(|name| self.styles.sections.get(name).copied());
                    let here = self.story().paragraphs;
                    let own = self.in_main() && columns.is_some_and(|(count, _)| count > 1);
                    if let (true, Some((count, gap))) = (own, columns) {
                        self.marks.push((here, Mark::Columns(count, gap)));
                    }
                    blocks.extend(self.blocks(child, lists));
                    if own {
                        let here = self.story().paragraphs;
                        self.marks.push((here, Mark::End));
                    }
                }
                (Some(TEXT), "table-of-content") if self.in_main() => {
                    self.contents_at(child);
                }
                (Some(TEXT), "index-body")
                | (Some(TEXT), "table-of-content")
                | (Some(TEXT), "illustration-index")
                | (Some(TEXT), "table-index")
                | (Some(TEXT), "object-index")
                | (Some(TEXT), "user-index")
                | (Some(TEXT), "alphabetical-index")
                | (Some(TEXT), "bibliography") => {
                    let body = child.child(Some(TEXT), "index-body").unwrap_or(child);
                    blocks.extend(self.blocks(body, lists));
                }
                (Some(DRAW), _) => self.waiting.push(child.clone()),
                _ => {}
            }
        }
        blocks
    }

    /// A table of contents: where it goes, how deep it gathers, and the page
    /// numbers its entries showed. The first one only: a document has one.
    fn contents_at(&mut self, element: &Element) {
        if self.contents.is_some() {
            return;
        }
        let levels = element
            .child(Some(TEXT), "table-of-content-source")
            .and_then(|source| source.attribute(Some(TEXT), "outline-level"))
            .and_then(|value| value.parse::<u8>().ok())
            .unwrap_or(3)
            .clamp(1, 9);
        let pages = element
            .child(Some(TEXT), "index-body")
            .map(|body| {
                body.children_named(Some(TEXT), "p")
                    .filter_map(|entry| {
                        let text = entry.text_content();
                        let last = text.rsplit(['\t', ' ']).next()?.trim();
                        last.parse::<usize>().ok()
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.contents = Some(ContentsFound { paragraph: self.story().paragraphs, levels, pages });
    }

    fn paragraph(&mut self, element: &Element, lists: &[ListContext]) -> Paragraph {
        let style_name = element.attribute(Some(TEXT), "style-name").unwrap_or("");
        let resolved = self.styles.paragraph_style(style_name);
        let mut properties = resolved.paragraph.clone();
        if let (true, Some(master)) = (self.in_main(), &resolved.master_page) {
            let here = self.story().paragraphs;
            self.marks.push((here, Mark::Page(master.clone(), resolved.page_number)));
        }

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
        // The level is the paragraph's in this format, and what a table of
        // contents gathers by.
        if let Some(level) = level.filter(|level| (1..=9).contains(level)) {
            properties.outline_level = Some(level - 1);
        }

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

        let mut building = Building {
            runs: Vec::new(),
            text: String::new(),
            offset: 0,
            chars: resolved.text.clone(),
            base: resolved.text.clone(),
            field: None,
            deleted: false,
        };
        // Drawings anchored to the page ride on the first paragraph after
        // them.
        for drawing in core::mem::take(&mut self.waiting) {
            self.drawing(&drawing, &mut building);
        }
        self.inline(element, &mut building);
        self.flush(&mut building);
        if properties.right_to_left == Some(true) {
            for run in &mut building.runs {
                if run.plain_text().chars().any(right_to_left_letter) {
                    run.properties.right_to_left = Some(true);
                }
            }
        }
        self.story_mut().paragraphs += 1;
        Paragraph { properties, runs: building.runs }
    }

    /// Where the text being read is, as the document will count it.
    fn here(&self, building: &Building) -> TextPosition {
        TextPosition::new(self.story().paragraphs, building.offset)
    }

    /// The change and the change of formatting the text here is part of.
    fn marks_here(&self, building: &Building) -> (Option<Revision>, Option<FormatChange>) {
        let revision = |change: &Change, kind: RevisionKind| Revision {
            kind,
            author: change.author.clone(),
            date: change.date.clone(),
            id: change.id,
        };
        if building.deleted {
            let change = self.active.last().and_then(|id| self.changes.get(id));
            return (change.map(|change| revision(change, RevisionKind::Deleted)), None);
        }
        let mut inserted = None;
        let mut formatted = None;
        for id in &self.active {
            let Some(change) = self.changes.get(id) else { continue };
            match change.kind {
                Some(RevisionKind::Inserted) => {
                    inserted = Some(revision(change, RevisionKind::Inserted))
                }
                Some(RevisionKind::Deleted) => {}
                None => {
                    formatted = Some(FormatChange {
                        author: change.author.clone(),
                        date: change.date.clone(),
                        id: change.id,
                        before: Box::new(building.base.clone()),
                    });
                }
            }
        }
        (inserted, formatted)
    }

    fn run(&self, building: &Building, content: RunContent) -> Run {
        let (revision, format_change) = self.marks_here(building);
        Run {
            properties: building.chars.clone(),
            content: vec![content],
            field: building.field.clone(),
            revision,
            format_change,
        }
    }

    fn flush(&self, building: &mut Building) {
        if building.text.is_empty() {
            return;
        }
        let text = core::mem::take(&mut building.text);
        let run = self.run(building, RunContent::Text(text));
        building.runs.push(run);
    }

    /// Something that is not text, which the document counts as one.
    fn put(&self, building: &mut Building, content: RunContent) {
        self.flush(building);
        let run = self.run(building, content);
        building.runs.push(run);
        if !building.deleted {
            building.offset += 1;
        }
    }

    /// The inline content of an element: text, spans, links, fields, notes,
    /// comments, bookmarks, changes, drawings.
    fn inline(&mut self, element: &Element, building: &mut Building) {
        for node in &element.children {
            match node {
                Node::Text(piece) | Node::CData(piece) => {
                    // Whitespace folds as it does on a page: the format says
                    // so, and writes `text:s` for the spaces that count.
                    let after_space = building.text.ends_with(' ')
                        || (building.text.is_empty() && building.runs.is_empty());
                    let folded = fold(piece, after_space);
                    if !building.deleted {
                        building.offset += folded.len();
                    }
                    building.text.push_str(&folded);
                }
                Node::Element(child) => self.inline_element(child, building),
                _ => {}
            }
        }
    }

    fn inline_element(&mut self, child: &Element, building: &mut Building) {
        match (child.namespace.as_deref(), child.local_name()) {
            (Some(TEXT), "span") => {
                let inner = self.span_properties(child, building);
                if inner != building.chars {
                    self.flush(building);
                }
                let held = core::mem::replace(&mut building.chars, inner);
                self.inline(child, building);
                self.flush(building);
                building.chars = held;
            }
            (Some(TEXT), "a") => {
                let address = child.attribute(Some(XLINK), "href").unwrap_or("").to_owned();
                if self.in_main() && building.field.is_none() {
                    let start = building.offset;
                    let paragraph = self.story().paragraphs;
                    self.inline(child, building);
                    if !address.is_empty() && building.offset > start {
                        self.links.push(LinkFound {
                            paragraph,
                            start,
                            end: building.offset,
                            address,
                        });
                    }
                } else {
                    // Outside the document's own text a link is a field.
                    self.flush(building);
                    let held = building.field.replace(format!("HYPERLINK \"{address}\""));
                    self.inline(child, building);
                    self.flush(building);
                    building.field = held;
                }
            }
            (Some(TEXT), "s") => {
                let count = child
                    .attribute(Some(TEXT), "c")
                    .and_then(|c| c.parse::<usize>().ok())
                    .unwrap_or(1);
                for _ in 0..count {
                    building.text.push(' ');
                }
                if !building.deleted {
                    building.offset += count;
                }
            }
            (Some(TEXT), "tab") => self.put(building, RunContent::Tab),
            (Some(TEXT), "line-break") => self.put(building, RunContent::Break(BreakKind::Line)),
            (Some(TEXT), "note") => self.note(child, building),
            (Some(OFFICE), "annotation") => self.comment(child, building),
            (Some(OFFICE), "annotation-end") => {
                if let Some(index) = child
                    .attribute(Some(OFFICE), "name")
                    .and_then(|name| self.open_comments.remove(name))
                {
                    let here = self.here(building);
                    if let Some(comment) = self.comments.get_mut(index) {
                        comment.end = here;
                    }
                }
            }
            (Some(TEXT), "bookmark") => {
                if let (true, Some(name)) = (self.in_main(), child.attribute(Some(TEXT), "name")) {
                    let here = self.here(building);
                    self.bookmarks.push(BookmarkFound {
                        name: name.to_owned(),
                        start: here,
                        end: here,
                    });
                }
            }
            (Some(TEXT), "bookmark-start") => {
                if let (true, Some(name)) = (self.in_main(), child.attribute(Some(TEXT), "name")) {
                    let here = self.here(building);
                    self.open_bookmarks.insert(name.to_owned(), here);
                }
            }
            (Some(TEXT), "bookmark-end") => {
                if let Some(name) = child.attribute(Some(TEXT), "name") {
                    if let Some(start) = self.open_bookmarks.remove(name) {
                        let end = self.here(building);
                        self.bookmarks.push(BookmarkFound { name: name.to_owned(), start, end });
                    }
                }
            }
            (Some(TEXT), "change-start") => {
                if let Some(id) = child.attribute(Some(TEXT), "change-id") {
                    self.flush(building);
                    self.active.push(id.to_owned());
                }
            }
            (Some(TEXT), "change-end") => {
                if let Some(id) = child.attribute(Some(TEXT), "change-id") {
                    self.flush(building);
                    self.active.retain(|held| held != id);
                }
            }
            (Some(TEXT), "change") => self.deletion(child, building),
            (Some(TEXT), "soft-page-break")
            | (Some(TEXT), "reference-mark")
            | (Some(TEXT), "reference-mark-start")
            | (Some(TEXT), "reference-mark-end")
            | (Some(TEXT), "toc-mark")
            | (Some(TEXT), "alphabetical-index-mark") => {}
            (Some(DRAW), _) => self.drawing(child, building),
            (Some(TEXT), local) => match field_instruction(local, child) {
                Some(instruction) if building.field.is_none() => {
                    self.flush(building);
                    building.field = Some(instruction);
                    self.inline(child, building);
                    self.flush(building);
                    building.field = None;
                }
                _ => self.inline(child, building),
            },
            _ => self.inline(child, building),
        }
    }

    /// The formatting a span gives its text. A span in a named style is in
    /// that style, which the paragraph's style's own text formatting does
    /// not override; an automatic style built on one is that style with
    /// changes of its own.
    fn span_properties(&self, span: &Element, building: &Building) -> RunProperties {
        let Some(name) = span.attribute(Some(TEXT), "style-name") else {
            return building.chars.clone();
        };
        let styles = self.styles;
        let named = if styles.named_text.iter().any(|named| named.name == name) {
            Some(name.to_owned())
        } else {
            styles.text_parents.get(name).cloned()
        };
        match named {
            Some(named) => {
                let style_properties = styles.text_style(&named).cloned().unwrap_or_default();
                let mut properties = unset(&building.chars, &style_properties);
                if named != name {
                    if let Some(own) = styles.text_own.get(name) {
                        properties = merge(&properties, own);
                    }
                }
                let display = styles
                    .named_text
                    .iter()
                    .find(|style| style.name == named)
                    .map_or_else(|| named.clone(), |style| style.display_name.clone());
                properties.style = Some(style_id(&display));
                properties
            }
            None => match styles.text_style(name) {
                Some(style) => merge(&building.chars, &merge(&building.base, style)),
                None => building.chars.clone(),
            },
        }
    }

    /// A note: its mark here, and its words as a story of their own, begun
    /// with the note's own mark as Word's are.
    fn note(&mut self, element: &Element, building: &mut Building) {
        let endnote = element.attribute(Some(TEXT), "note-class") == Some("endnote");
        let Some(body) = element.child(Some(TEXT), "note-body") else { return };
        if !self.in_main() {
            // A note inside a note, a header or a comment has nowhere to go:
            // its words are left out rather than put in the wrong place.
            return;
        }
        let id = if endnote {
            self.endnotes += 1;
            self.endnotes
        } else {
            self.footnotes += 1;
            self.footnotes
        };
        self.put(building, RunContent::NoteReference { id, endnote });
        let (mut blocks, mut pictures) = self.story_of(body);
        if let Some(Block::Paragraph(first)) = blocks.first_mut() {
            first.runs.insert(
                0,
                Run {
                    properties: RunProperties::default(),
                    content: vec![RunContent::NoteReference { id: 0, endnote }],
                    field: None,
                    revision: None,
                    format_change: None,
                },
            );
            for picture in pictures.iter_mut().filter(|picture| picture.paragraph == 0) {
                picture.offset += 1;
            }
        }
        self.notes.push(NoteFound { id, endnote, body: Body { blocks }, pictures });
    }

    /// A comment: its author, its date and its words, anchored here and
    /// running to its end mark if it has one.
    fn comment(&mut self, element: &Element, building: &mut Building) {
        if !self.in_main() {
            return;
        }
        let text = |namespace: &str, local: &str| {
            element
                .child(Some(namespace), local)
                .map(|child| child.text_content().trim().to_owned())
                .unwrap_or_default()
        };
        let author = text(DC, "creator");
        let date = iso(&text(DC, "date"));
        let here = self.here(building);
        let (blocks, pictures) = self.story_of(element);
        let index = self.comments.len();
        self.comments.push(CommentFound {
            start: here,
            end: here,
            author,
            date,
            body: Body { blocks },
            pictures,
        });
        if let Some(name) = element.attribute(Some(OFFICE), "name") {
            self.open_comments.insert(name.to_owned(), index);
        }
    }

    /// Where a deletion was: its words put back here, marked deleted, counting
    /// for nothing.
    fn deletion(&mut self, element: &Element, building: &mut Building) {
        let Some(id) = element.attribute(Some(TEXT), "change-id") else { return };
        let Some(deleted) = self.changes.get(id).and_then(|change| change.deleted.clone()) else {
            return;
        };
        self.flush(building);
        let was_deleted = core::mem::replace(&mut building.deleted, true);
        self.active.push(id.to_owned());
        let mut paragraphs = Vec::new();
        collect_paragraph_elements(&deleted, &mut paragraphs);
        for paragraph in paragraphs {
            self.inline(paragraph, building);
        }
        self.flush(building);
        self.active.retain(|held| held != id);
        building.deleted = was_deleted;
    }

    /// A drawing: a picture in a frame, a text box, a shape.
    fn drawing(&mut self, element: &Element, building: &mut Building) {
        let anchor_type = element.attribute(Some(TEXT), "anchor-type").unwrap_or("paragraph");
        let graphic = element
            .attribute(Some(DRAW), "style-name")
            .and_then(|name| self.styles.graphics.get(name))
            .cloned()
            .unwrap_or_default();
        let length = |local: &str| element.attribute(Some(SVG), local).and_then(twips);
        let anchor = (anchor_type != "as-char")
            .then(|| anchor_of(&graphic, anchor_type, length("x"), length("y")));
        let size = |local: &str| length(local).map(|twips| i64::from(twips) * 635);
        let name = element.attribute(Some(DRAW), "name").unwrap_or_default().to_owned();

        match element.local_name() {
            "frame" => {
                if let Some(text_box) = element.child(Some(DRAW), "text-box") {
                    let (blocks, _) = self.story_of(text_box);
                    let mut shape = Shape {
                        preset: "rect".to_owned(),
                        width_emu: size("width").unwrap_or(1_828_800),
                        height_emu: size("height")
                            .or_else(|| {
                                text_box
                                    .attribute(Some(FO_NS), "min-height")
                                    .and_then(twips)
                                    .map(|t| i64::from(t) * 635)
                            })
                            .unwrap_or(914_400),
                        name: if name.is_empty() { "Text Box".to_owned() } else { name },
                        anchor,
                        text: paragraphs_of(&blocks),
                        ..Shape::default()
                    };
                    dress(&mut shape, &graphic);
                    self.put(building, RunContent::Shape(Box::new(shape)));
                    return;
                }
                let Some(href) = element
                    .child(Some(DRAW), "image")
                    .and_then(|image| image.attribute(Some(XLINK), "href"))
                else {
                    return;
                };
                if building.deleted {
                    return;
                }
                self.flush(building);
                let paragraph = self.story().paragraphs;
                let offset = building.offset;
                self.story_mut().pictures.push(PictureFound {
                    paragraph,
                    offset,
                    name: href.to_owned(),
                    width_emu: size("width").unwrap_or(96 * 9525),
                    height_emu: size("height").unwrap_or(96 * 9525),
                    anchor,
                });
                self.put(building, RunContent::Text(PICTURE_MARK.to_string()));
            }
            "custom-shape" | "rect" | "ellipse" | "line" => {
                let preset = match element.local_name() {
                    "rect" => Some("rect".to_owned()),
                    "ellipse" => Some("ellipse".to_owned()),
                    "line" => Some("line".to_owned()),
                    _ => element
                        .child(Some(DRAW), "enhanced-geometry")
                        .and_then(|geometry| geometry.attribute(Some(DRAW), "type"))
                        .and_then(preset_of),
                };
                let Some(preset) = preset else { return };
                let (mut width, mut height) =
                    (size("width").unwrap_or(0), size("height").unwrap_or(0));
                let (mut across, mut down) = (false, false);
                if preset == "line" {
                    let point = |local: &str| size(local).unwrap_or(0);
                    width = (point("x2") - point("x1")).abs();
                    height = (point("y2") - point("y1")).abs();
                    across = point("x2") < point("x1");
                    down = point("y2") < point("y1");
                }
                let has_text = element
                    .children_named(Some(TEXT), "p")
                    .any(|p| !p.text_content().trim().is_empty());
                let text = if has_text {
                    let (blocks, _) = self.story_of(element);
                    paragraphs_of(&blocks)
                } else {
                    Vec::new()
                };
                let mut shape = Shape {
                    preset,
                    width_emu: width,
                    height_emu: height,
                    name: if name.is_empty() { "Shape".to_owned() } else { name },
                    anchor,
                    flipped_across: across,
                    flipped_down: down,
                    text,
                    ..Shape::default()
                };
                dress(&mut shape, &graphic);
                self.put(building, RunContent::Shape(Box::new(shape)));
            }
            _ => {}
        }
    }

    fn table(&mut self, element: &Element) -> Block {
        let mut grid: Vec<i32> = Vec::new();
        let mut columns = Vec::new();
        collect_named(
            element,
            TABLE,
            "table-column",
            &["table-columns", "table-header-columns", "table-column-group"],
            &mut columns,
        );
        for column in columns {
            let repeat = column
                .attribute(Some(TABLE), "number-columns-repeated")
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(1)
                .min(1024);
            let width = column
                .attribute(Some(TABLE), "style-name")
                .and_then(|name| self.styles.columns.get(name).copied())
                .unwrap_or(2880);
            grid.extend(core::iter::repeat_n(width, repeat));
        }
        let mut row_elements: Vec<(&Element, bool)> = Vec::new();
        collect_rows(element, false, &mut row_elements);

        // Cells merged down: which column they begin at, how many they cover,
        // and how many rows are still to be covered.
        let mut down: Vec<(usize, u32, usize)> = Vec::new();
        let mut rows = Vec::new();
        for (row, header) in row_elements {
            let mut cells = Vec::new();
            let mut column = 0usize;
            let mut skip = 0u32;
            let mut started = Vec::new();
            for cell in row.child_elements() {
                let covered = match cell.local_name() {
                    "table-cell" => false,
                    "covered-table-cell" => true,
                    _ => continue,
                };
                let repeat = cell
                    .attribute(Some(TABLE), "number-columns-repeated")
                    .and_then(|value| value.parse::<usize>().ok())
                    .unwrap_or(1)
                    .min(1024);
                for _ in 0..repeat {
                    if covered {
                        if skip > 0 {
                            skip -= 1;
                            column += 1;
                            continue;
                        }
                        if let Some(&(_, span, _)) =
                            down.iter().find(|(from, _, left)| *from == column && *left > 0)
                        {
                            // The rest of a cell merged down into this row.
                            let width = grid
                                .get(column..column + span as usize)
                                .map(|widths| widths.iter().sum());
                            self.story_mut().paragraphs += 1;
                            cells.push(TableCell {
                                blocks: vec![Block::Paragraph(Paragraph::default())],
                                width,
                                span,
                                merged_upwards: true,
                                ..TableCell::default()
                            });
                            skip = span - 1;
                        }
                        column += 1;
                        continue;
                    }
                    let span = cell
                        .attribute(Some(TABLE), "number-columns-spanned")
                        .and_then(|value| value.parse::<u32>().ok())
                        .unwrap_or(1)
                        .max(1);
                    let rows_spanned = cell
                        .attribute(Some(TABLE), "number-rows-spanned")
                        .and_then(|value| value.parse::<usize>().ok())
                        .unwrap_or(1);
                    let mut blocks = self.blocks(cell, &[]);
                    if blocks.is_empty() {
                        blocks.push(Block::Paragraph(Paragraph::default()));
                        self.story_mut().paragraphs += 1;
                    }
                    let width =
                        grid.get(column..column + span as usize).map(|widths| widths.iter().sum());
                    let style = cell
                        .attribute(Some(TABLE), "style-name")
                        .and_then(|name| self.styles.cells.get(name))
                        .cloned()
                        .unwrap_or_default();
                    cells.push(TableCell {
                        blocks,
                        width,
                        span,
                        borders: style.borders,
                        shading: style.shading,
                        vertical: style.alignment,
                        direction: style.direction,
                        ..TableCell::default()
                    });
                    if rows_spanned > 1 {
                        started.push((column, span, rows_spanned - 1));
                    }
                    skip = span - 1;
                    column += 1;
                }
            }
            for merge in &mut down {
                merge.2 = merge.2.saturating_sub(1);
            }
            down.retain(|merge| merge.2 > 0);
            down.extend(started);
            let style = row
                .attribute(Some(TABLE), "style-name")
                .and_then(|name| self.styles.rows.get(name))
                .copied()
                .unwrap_or_default();
            rows.push(TableRow {
                cells,
                height: style.height,
                height_exact: style.exact,
                is_header: header,
            });
        }
        let indent = element
            .attribute(Some(TABLE), "style-name")
            .and_then(|name| self.styles.tables.get(name))
            .and_then(|style| style.indent)
            .unwrap_or(0);
        Block::Table(Box::new(Table { rows, grid, indent, ..Table::default() }))
    }

    /// The document's sections: where a page style begins, and where a
    /// section of its own in columns begins and ends, each with its page,
    /// its columns and its master page's headers and footers.
    fn sections(&mut self, masters: &Styles, paragraphs: usize) -> Vec<SectionFound> {
        // The boundaries, in order, with what each begins.
        let mut boundaries: Vec<Boundary> = Vec::new();
        for (at, mark) in core::mem::take(&mut self.marks) {
            let at = at.min(paragraphs);
            let entry = match boundaries.iter_mut().find(|(held, ..)| *held == at) {
                Some(entry) => entry,
                None => {
                    boundaries.push((at, None, None));
                    boundaries.last_mut().expect("just pushed")
                }
            };
            match mark {
                Mark::Page(master, number) => entry.1 = Some((master, number)),
                Mark::Columns(count, gap) => entry.2 = Some(Some((count, gap))),
                Mark::End => entry.2 = Some(None),
            }
        }
        boundaries.sort_by_key(|(at, ..)| *at);

        let mut master = masters.first_master().unwrap_or("Standard").to_owned();
        let mut columns: Option<(usize, i32)> = None;
        let mut number = None;
        // What the first paragraph says is where the document begins.
        if let Some(first) = boundaries.first().filter(|(at, ..)| *at == 0) {
            if let Some((name, page)) = &first.1 {
                master.clone_from(name);
                number = *page;
            }
            if let Some(inner) = first.2 {
                columns = inner;
            }
        }
        let mut starts: Vec<SectionStart> =
            vec![(0, Start::NextPage, master.clone(), columns, number)];
        for (at, page, section) in
            boundaries.into_iter().filter(|(at, ..)| *at > 0 && *at < paragraphs)
        {
            let start = if page.is_some() { Start::NextPage } else { Start::Continuous };
            if let Some((name, page_number)) = page {
                master = name;
                number = page_number;
            } else {
                number = None;
            }
            if let Some(inner) = section {
                columns = inner;
            }
            starts.push((at, start, master.clone(), columns, number));
        }

        let mut out = Vec::with_capacity(starts.len());
        let mut previous_master: Option<String> = None;
        for (index, (_, start, master_name, columns, number)) in starts.iter().enumerate() {
            let last_paragraph = starts.get(index + 1).map(|(at, ..)| at - 1);
            let first = masters.master(master_name).cloned().unwrap_or_default();
            // A master page followed by another is a first page of its own.
            let following = first
                .next
                .as_deref()
                .filter(|next| next != master_name)
                .and_then(|next| masters.master(next).cloned());
            let pages = following.clone().unwrap_or_else(|| first.clone());
            let has = |master: &crate::styles::MasterPage, prefix: &str| {
                master.furniture.iter().any(|(which, _)| which.starts_with(prefix))
            };
            let layout = pages
                .layout
                .as_deref()
                .and_then(|name| masters.layouts.get(name))
                .cloned()
                .unwrap_or_default();
            let mut page = layout.setup(has(&pages, "header"), has(&pages, "footer"));
            page.title_page = following.is_some()
                || first.furniture.iter().any(|(which, _)| which.ends_with("-first"));
            if let Some((count, gap)) = columns {
                page.columns = *count;
                page.column_gap = *gap;
            }
            if let Some(number) = number {
                page.numbering = Some(PageNumbering {
                    start: Some(*number),
                    format: page.numbering.map(|numbering| numbering.format).unwrap_or_default(),
                });
            }
            // The same master as the section before: its headers carry on.
            let furniture = if previous_master.as_deref() == Some(master_name.as_str()) {
                Vec::new()
            } else {
                furniture_of(masters, &first, following.as_ref())
            };
            previous_master = Some(master_name.clone());
            out.push(SectionFound { last_paragraph, start: *start, page, furniture });
        }
        out
    }
}

/// A master page's headers and footers — and, when it is a first page of
/// its own followed by another, the other's as every page after the first.
fn furniture_of(
    styles: &Styles,
    first: &crate::styles::MasterPage,
    following: Option<&crate::styles::MasterPage>,
) -> Vec<FurnitureFound> {
    let mut reader = Reader::new(styles, false);
    let mut out = Vec::new();
    let mut read =
        |element: &Element, kind: Furniture, which: Which, out: &mut Vec<FurnitureFound>| {
            let (blocks, pictures) = reader.story_of(element);
            out.push(FurnitureFound { kind, which, body: Body { blocks }, pictures });
        };
    let every = following.unwrap_or(first);
    for (local, element) in &every.furniture {
        let (kind, which) = match local.as_str() {
            "header" => (Furniture::Header, Which::Default),
            "header-left" => (Furniture::Header, Which::Even),
            "header-first" if following.is_none() => (Furniture::Header, Which::First),
            "footer" => (Furniture::Footer, Which::Default),
            "footer-left" => (Furniture::Footer, Which::Even),
            "footer-first" if following.is_none() => (Furniture::Footer, Which::First),
            _ => continue,
        };
        read(element, kind, which, &mut out);
    }
    if following.is_some() {
        for (local, element) in &first.furniture {
            let kind = match local.as_str() {
                "header" => Furniture::Header,
                "footer" => Furniture::Footer,
                _ => continue,
            };
            read(element, kind, Which::First, &mut out);
        }
        // A first page with nothing of its own at the top or the bottom has
        // nothing there, rather than the pages' after it.
        for kind in [Furniture::Header, Furniture::Footer] {
            if !out.iter().any(|found| found.kind == kind && found.which == Which::First) {
                out.push(FurnitureFound {
                    kind,
                    which: Which::First,
                    body: Body { blocks: vec![Block::Paragraph(Paragraph::default())] },
                    pictures: Vec::new(),
                });
            }
        }
    }
    out
}

/// The instruction Word would write for a field of this format's, where it
/// has one.
fn field_instruction(local: &str, element: &Element) -> Option<String> {
    Some(match local {
        "page-number" => "PAGE".to_owned(),
        "page-count" => "NUMPAGES".to_owned(),
        "word-count" => "NUMWORDS".to_owned(),
        "character-count" => "NUMCHARS".to_owned(),
        "date" => "DATE".to_owned(),
        "time" => "TIME".to_owned(),
        "title" => "TITLE".to_owned(),
        "subject" => "SUBJECT".to_owned(),
        "description" => "COMMENTS".to_owned(),
        "keywords" => "KEYWORDS".to_owned(),
        "initial-creator" => "AUTHOR".to_owned(),
        "author-name" => "USERNAME".to_owned(),
        "creator" => "LASTSAVEDBY".to_owned(),
        "file-name" => "FILENAME".to_owned(),
        "creation-date" => "CREATEDATE".to_owned(),
        "modification-date" => "SAVEDATE".to_owned(),
        "print-date" => "PRINTDATE".to_owned(),
        "sequence" => {
            let name = element.attribute(Some(TEXT), "name").unwrap_or("Figure");
            format!("SEQ {name} \\* ARABIC")
        }
        "bookmark-ref" => {
            let name = element.attribute(Some(TEXT), "ref-name")?;
            match element.attribute(Some(TEXT), "reference-format") {
                Some("page") => format!("PAGEREF {name} \\h"),
                _ => format!("REF {name} \\h"),
            }
        }
        _ => return None,
    })
}

/// The preset a custom shape's type names: this format's own names, Word's
/// presets as LibreOffice keeps them, and the Office shape numbers.
fn preset_of(kind: &str) -> Option<String> {
    if let Some(preset) = kind.strip_prefix("ooxml-") {
        return Some(preset.to_owned());
    }
    if let Some(number) = kind.strip_prefix("mso-spt").and_then(|n| n.parse::<i64>().ok()) {
        return wp_docx::shapes::office_preset(number).map(str::to_owned);
    }
    Some(
        match kind {
            "rectangle" => "rect",
            "round-rectangle" => "roundRect",
            "ellipse" | "circle" => "ellipse",
            "diamond" => "diamond",
            "isosceles-triangle" => "triangle",
            "right-triangle" => "rtTriangle",
            "parallelogram" => "parallelogram",
            "trapezoid" => "trapezoid",
            "hexagon" => "hexagon",
            "octagon" => "octagon",
            "cross" => "plus",
            "star4" => "star4",
            "star5" => "star5",
            "star8" => "star8",
            "right-arrow" => "rightArrow",
            "left-arrow" => "leftArrow",
            "up-arrow" => "upArrow",
            "down-arrow" => "downArrow",
            "left-right-arrow" => "leftRightArrow",
            "up-down-arrow" => "upDownArrow",
            "heart" => "heart",
            "sun" => "sun",
            "moon" => "moon",
            "smiley" => "smileyFace",
            "can" => "can",
            "cube" => "cube",
            _ => return None,
        }
        .to_owned(),
    )
}

/// A shape's fill and line from its style.
fn dress(shape: &mut Shape, graphic: &crate::styles::Graphic) {
    shape.fill = match &graphic.fill {
        Some(Some(colour)) => Fill::Solid(wp_docx::colour::Colour::rgb(colour)),
        _ => Fill::None,
    };
    match &graphic.stroke {
        Some(Some((colour, width))) => {
            shape.outline = Some(wp_docx::colour::Colour::rgb(colour));
            shape.outline_emu = *width;
        }
        _ => {
            shape.outline = None;
            shape.outline_emu = 0;
        }
    }
}

/// The namespace of the formatting attributes, which a text box's minimum
/// height is in.
const FO_NS: &str = crate::styles::FO;

/// Every paragraph in some blocks, tables' included.
fn paragraphs_of(blocks: &[Block]) -> Vec<Paragraph> {
    let mut out = Vec::new();
    for block in blocks {
        match block {
            Block::Paragraph(paragraph) => {
                if !paragraph.runs.is_empty() || blocks.len() == 1 {
                    out.push(paragraph.clone());
                }
            }
            Block::Table(table) => {
                for row in &table.rows {
                    for cell in &row.cells {
                        out.extend(paragraphs_of(&cell.blocks));
                    }
                }
            }
        }
    }
    out
}

/// The paragraphs inside an element, however deep.
fn collect_paragraph_elements<'e>(element: &'e Element, out: &mut Vec<&'e Element>) {
    for child in element.child_elements() {
        if child.namespace.as_deref() == Some(TEXT) && matches!(child.local_name(), "p" | "h") {
            out.push(child);
        } else {
            collect_paragraph_elements(child, out);
        }
    }
}

/// The elements of a name, looking inside the groups that hold them.
fn collect_named<'e>(
    element: &'e Element,
    namespace: &str,
    local: &str,
    groups: &[&str],
    out: &mut Vec<&'e Element>,
) {
    for child in element.child_elements() {
        if child.namespace.as_deref() != Some(namespace) {
            continue;
        }
        if child.local_name() == local {
            out.push(child);
        } else if groups.contains(&child.local_name()) {
            collect_named(child, namespace, local, groups, out);
        }
    }
}

/// A table's rows, the header rows marked as such.
fn collect_rows<'e>(element: &'e Element, header: bool, out: &mut Vec<(&'e Element, bool)>) {
    for child in element.child_elements() {
        if child.namespace.as_deref() != Some(TABLE) {
            continue;
        }
        match child.local_name() {
            "table-row" => out.push((child, header)),
            "table-header-rows" => collect_rows(child, true, out),
            "table-rows" | "table-row-group" => collect_rows(child, header, out),
            _ => {}
        }
    }
}

/// Whether a letter is of an alphabet written right to left.
fn right_to_left_letter(letter: char) -> bool {
    matches!(letter, '\u{0590}'..='\u{08FF}' | '\u{FB1D}'..='\u{FEFF}')
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
        spacing_twentieths: over.spacing_twentieths.or(under.spacing_twentieths),
        language: over.language.clone().or_else(|| under.language.clone()),
        ..under.clone()
    }
}

/// Properties with what a style says taken out, so that the style shows
/// through them.
fn unset(properties: &RunProperties, style: &RunProperties) -> RunProperties {
    let mut out = properties.clone();
    let clear = |said: bool, value: &mut Option<bool>| {
        if said {
            *value = None;
        }
    };
    clear(style.bold.is_some(), &mut out.bold);
    clear(style.italic.is_some(), &mut out.italic);
    clear(style.strike.is_some(), &mut out.strike);
    clear(style.double_strike.is_some(), &mut out.double_strike);
    clear(style.caps.is_some(), &mut out.caps);
    clear(style.small_caps.is_some(), &mut out.small_caps);
    clear(style.hidden.is_some(), &mut out.hidden);
    if style.underline.is_some() {
        out.underline = None;
    }
    if style.size_half_points.is_some() {
        out.size_half_points = None;
    }
    if style.color.is_some() {
        out.color = None;
    }
    if style.highlight.is_some() {
        out.highlight = None;
    }
    if style.vertical_align.is_some() {
        out.vertical_align = None;
    }
    if style.font.is_some() {
        out.font = None;
    }
    if style.language.is_some() {
        out.language = None;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_date_is_made_the_models() {
        assert_eq!(iso("2024-03-05T10:30:00"), "2024-03-05T10:30:00Z");
        assert_eq!(iso("2024-03-05T10:30:00.123456789"), "2024-03-05T10:30:00Z");
        assert_eq!(iso("2024-03-05T10:30:00+02:00"), "2024-03-05T10:30:00+02:00");
    }

    #[test]
    fn a_shapes_type_is_the_preset_it_names() {
        assert_eq!(preset_of("ooxml-ellipse").as_deref(), Some("ellipse"));
        assert_eq!(preset_of("rectangle").as_deref(), Some("rect"));
        assert_eq!(preset_of("mso-spt202").as_deref(), Some("rect"));
        assert_eq!(preset_of("non-primitive"), None);
    }

    #[test]
    fn a_style_is_named_as_word_names_it() {
        assert_eq!(style_id("Strong Red"), "StrongRed");
        assert_eq!(style_id("Emphasis"), "Emphasis");
    }
}
