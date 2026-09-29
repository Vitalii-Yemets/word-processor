//! Reading the binary document into the model.
//!
//! # How a binary document is put together
//!
//! The text is one long run of characters, a paragraph ending in a carriage
//! return and a table cell in a bell — but it is not in the file in order.
//! The piece table says where each stretch of it is and whether that
//! stretch is one byte a character or two. The formatting is not on the
//! text either: it is in pages of five hundred and twelve bytes, each
//! covering a range of file positions, one set of pages for paragraphs and
//! one for runs, found through a table of which page covers which range.
//! Every property is a sprm, and a paragraph's or a run's formatting is its
//! style's sprms with its own on top. Lists, fonts, styles and section
//! properties are tables of their own in the second stream; pictures are in
//! a third, behind a header and inside a drawing record.
//!
//! # Stories
//!
//! The main text is only the first of the text's stories: after it come the
//! footnotes, the headers and footers, the comments, the endnotes and the
//! words in text boxes, each where the block's lengths say, each cut into
//! its entries by a table of positions — which note begins where, which
//! header is which. Each is read the same way, paragraph by paragraph, into
//! paragraphs and tables of its own, and put where it belongs: a note's
//! words with the note, a header's with its section.
//!
//! The main text's marks say where the rest is anchored: a note's number
//! where the note is, a drawing where it floats from. Bookmarks, comments'
//! ranges and sections are positions in the main text, kept in tables of
//! their own; reading the main text notes where each of its characters ends
//! up in the document, and those tables are read through that.
//!
//! # Encrypted files
//!
//! Every stream but the first few bytes of the main one is enciphered where
//! it lies. With the password, each is deciphered from its first byte — see
//! [`wp_crypt::binary`] — and then read as any other file.

use std::collections::HashMap;
use std::ops::Range;

use wp_docx::anchor::Anchor;
use wp_docx::fonts::FontEntry;
use wp_docx::furniture::{Furniture, Which};
use wp_docx::model::{
    Block, Body, BreakKind, FormatChange, NumberingReference, Paragraph, ParagraphProperties,
    Revision, RevisionKind, Run, RunContent, RunProperties,
};
use wp_docx::properties::Properties;
use wp_docx::TextPosition;
use wp_ole::CompoundFile;

use crate::fib::{Base, Fib, Table};
use crate::format::{self, Bins, Font, Kind, Lists, Pages, Style};
use crate::plc::{self, Plc};
use crate::sections::{self, PageSetup};
use crate::shapes::{self, Drawings};
use crate::sprm::{self, Sprm};
use crate::tables::{Level, RowDefinition};
use crate::text::{self, Char};
use crate::Error;

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
    pub links: Vec<LinkFound>,
    pub bookmarks: Vec<BookmarkFound>,
    pub comments: Vec<CommentFound>,
    /// What the notes say, for the marks already in the body.
    pub notes: Vec<NoteFound>,
    /// The sections, in order; there is always at least one.
    pub sections: Vec<SectionFound>,
    /// Whether left-hand and right-hand pages have headers and footers of
    /// their own.
    pub facing_pages: bool,
    /// The font table: what the file says of each font it names.
    pub fonts: Vec<FontEntry>,
    /// What the document says about itself.
    pub properties: Properties,
}

#[derive(Debug)]
pub struct PictureFound {
    pub paragraph: usize,
    pub offset: usize,
    pub bytes: Vec<u8>,
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
    /// An ISO 8601 timestamp, or nothing where the file gave no date — which
    /// before Word 2002 it never did.
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
    /// The paragraph it ends with, counted through the whole document; the
    /// last section ends with the document and has none.
    pub last_paragraph: Option<usize>,
    pub page: PageSetup,
    /// Its own headers and footers. One it does not have follows the section
    /// before it, as in Word.
    pub furniture: Vec<FurnitureFound>,
}

/// Reads a document's bytes.
pub fn read(bytes: &[u8]) -> Result<Reading, Error> {
    read_with_password(bytes, None)
}

/// Reads a document's bytes, deciphering them with the password if they are
/// encrypted. An encrypted document without one is [`Error::Encrypted`].
pub fn read_with_password(bytes: &[u8], password: Option<&str>) -> Result<Reading, Error> {
    let file = CompoundFile::open(bytes.to_vec())?;
    let mut word = file.stream("WordDocument").ok_or(Error::Malformed("no WordDocument stream"))?;
    let base = Base::parse(&word)?;
    let mut table = if base.old {
        Vec::new()
    } else {
        let name = if base.table_stream_one { "1Table" } else { "0Table" };
        file.stream(name)
            .or_else(|| file.stream("1Table"))
            .or_else(|| file.stream("0Table"))
            .ok_or(Error::Malformed("no table stream"))?
    };
    let mut data = file.stream("Data").unwrap_or_default();
    if base.encrypted {
        let password = password.ok_or(Error::Encrypted)?;
        decipher(&base, password, &mut word, &mut table, &mut data)?;
    }
    // Word 6 kept everything in the one stream.
    if base.old {
        table.clone_from(&word);
        data.clone_from(&word);
    }
    let fib = Fib::parse(&word)?;
    let text = text::text_of(&word, &table, &fib)?;
    let styles = format::styles_of(&table, &fib);
    let fonts = format::fonts_of(&table, &fib);
    let lists = format::lists_of(&table, &fib);
    let paragraph_bins = format::bins_of(&word, &table, &fib, Kind::Paragraph);
    let character_bins = format::bins_of(&word, &table, &fib, Kind::Character);
    let drawings = Drawings::parse(&table, &fib);
    let starts = fib.story_starts();
    let table_of = |which: Table| {
        fib.table(which).and_then(|(offset, length)| table.get(offset..offset + length))
    };
    let authors = table_of(Table::RevisionAuthors)
        .map(|bytes| plc::strings(bytes, fib.base.old).into_iter().map(|(name, _)| name).collect())
        .unwrap_or_default();
    let dop = table_of(Table::Dop).unwrap_or(&[]);
    let facing_pages = dop.first().is_some_and(|flags| flags & 1 != 0);
    let separators = dop.get(1).copied().unwrap_or(0);

    // The notes: where each is marked in the main text, and where its words
    // are in its own story.
    let notes_of = |references: Table, texts: Table| -> Vec<(u32, Range<u32>)> {
        let (Some(references), Some(texts)) = (table_of(references), table_of(texts)) else {
            return Vec::new();
        };
        let references = Plc::parse(references, 2);
        let texts = Plc::parse(texts, 0);
        (0..references.len())
            .filter_map(|index| {
                Some((references.start(index)?, texts.start(index)?..texts.end(index)?))
            })
            .collect()
    };
    let footnotes = notes_of(Table::FootnoteReferences, Table::FootnoteTexts);
    let endnotes = notes_of(Table::EndnoteReferences, Table::EndnoteTexts);
    let mut note_marks = HashMap::new();
    for (index, (at, _)) in footnotes.iter().enumerate() {
        note_marks.insert(*at, (index as i32 + 1, false));
    }
    for (index, (at, _)) in endnotes.iter().enumerate() {
        note_marks.insert(*at, (index as i32 + 1, true));
    }

    let context = Context {
        word: &word,
        table: &table,
        data: &data,
        fib: &fib,
        text,
        styles,
        fonts,
        lists,
        paragraph_bins,
        character_bins,
        drawings,
        authors,
        note_marks,
        starts,
    };
    let mut reader = Reader { context: &context, pages: Pages::default(), changes: Vec::new() };

    let sections = sections::sections_of(&word, &table, &fib);
    let section_ends: Vec<u32> = sections.iter().map(|(end, _)| *end).collect();
    let main = reader.story(Story::Main, starts[0]..starts[1], &section_ends);

    let mut notes = Vec::new();
    for (endnote, list, from) in [(false, &footnotes, starts[1]), (true, &endnotes, starts[5])] {
        let story = if endnote { Story::Endnote } else { Story::Footnote };
        for (index, (_, range)) in list.iter().enumerate() {
            let built = reader.story(story, from + range.start..from + range.end, &[]);
            notes.push(NoteFound {
                id: index as i32 + 1,
                endnote,
                body: built.body(),
                pictures: built.pictures,
            });
        }
    }

    let position =
        |cp: u32| main.positions.get(cp as usize).copied().unwrap_or_else(|| main.end_position());
    let comments = comments_of(&mut reader, &table_of, &position);
    let bookmarks = bookmarks_of(&table_of, fib.base.old, starts[1], &position);

    let furniture = sections::furniture_of(&table, &fib, &sections, separators);
    let last = sections.len().saturating_sub(1);
    let mut found_sections = Vec::with_capacity(sections.len().max(1));
    for (index, (end, page)) in sections.into_iter().enumerate() {
        let mut found = SectionFound {
            last_paragraph: (index < last).then(|| position(end.saturating_sub(1)).paragraph),
            page,
            furniture: Vec::new(),
        };
        for (kind, which, start, end) in furniture.get(index).cloned().unwrap_or_default() {
            let built = reader.story(Story::Header, starts[2] + start..starts[2] + end, &[]);
            found.furniture.push(FurnitureFound {
                kind,
                which,
                body: built.body(),
                pictures: built.pictures,
            });
        }
        found_sections.push(found);
    }
    if found_sections.is_empty() {
        found_sections.push(SectionFound::default());
    }

    let mut body = Body { blocks: main.blocks };
    if body.blocks.is_empty() {
        body.blocks.push(Block::Paragraph(Paragraph::default()));
    }
    Ok(Reading {
        body,
        pictures: main.pictures,
        links: main.links,
        bookmarks,
        comments,
        notes,
        sections: found_sections,
        facing_pages,
        fonts: context.fonts.iter().map(|font| font.entry.clone()).collect(),
        properties: crate::summary::properties_of(&file),
    })
}

/// Deciphers the streams in place: the main stream but its first bytes,
/// which were never enciphered, and the other two whole.
fn decipher(
    base: &Base,
    password: &str,
    word: &mut [u8],
    table: &mut [u8],
    data: &mut [u8],
) -> Result<(), Error> {
    let cipher = if base.obfuscated {
        wp_crypt::binary::xor_stream_cipher(password, base.key)?
    } else {
        let description =
            table.get(..base.key as usize).ok_or(Error::Truncated("encryption description"))?;
        wp_crypt::binary::rc4_stream_cipher(description, password)?
    };
    let readable = base.readable().min(word.len());
    let kept = word[..readable].to_vec();
    cipher.apply(word);
    word[..readable].copy_from_slice(&kept);
    if !base.old {
        cipher.apply(table);
        cipher.apply(data);
    }
    Ok(())
}

/// The comments: where each is marked, who wrote it and when, what it covers,
/// and its words.
fn comments_of<'t>(
    reader: &mut Reader<'_>,
    table_of: &dyn Fn(Table) -> Option<&'t [u8]>,
    position: &dyn Fn(u32) -> TextPosition,
) -> Vec<CommentFound> {
    let context = reader.context;
    let old = context.fib.base.old;
    let (Some(references), Some(texts)) =
        (table_of(Table::CommentReferences), table_of(Table::CommentTexts))
    else {
        return Vec::new();
    };
    let references = Plc::parse(references, if old { 20 } else { 30 });
    let texts = Plc::parse(texts, 0);
    let authors = table_of(Table::CommentAuthors)
        .map(|bytes| plc::run_of_strings(bytes, old))
        .unwrap_or_default();
    // The ranges: bookmarks of their own, each tagged with a number a
    // comment names.
    let tags: Vec<i32> = table_of(Table::CommentBookmarks)
        .filter(|_| !old)
        .map(|bytes| {
            plc::strings(bytes, false)
                .into_iter()
                .map(|(_, extra)| plc::u32_at(&extra, 2) as i32)
                .collect()
        })
        .unwrap_or_default();
    let starts = table_of(Table::CommentBookmarkStarts).map(|bytes| Plc::parse(bytes, 4));
    let ends = table_of(Table::CommentBookmarkEnds).map(|bytes| Plc::parse(bytes, 0));
    let extra = table_of(Table::CommentsExtra).filter(|_| !old);
    let from = context.starts[4];

    let mut out = Vec::new();
    for index in 0..references.len() {
        let (Some(at), Some(entry)) = (references.start(index), references.entry(index)) else {
            continue;
        };
        let (Some(text_start), Some(text_end)) = (texts.start(index), texts.end(index)) else {
            continue;
        };
        let (author_index, tag) = if old {
            (plc::u16_at(entry, 10), plc::u32_at(entry, 16) as i32)
        } else {
            (plc::u16_at(entry, 20), plc::u32_at(entry, 26) as i32)
        };
        let author = authors.get(usize::from(author_index)).cloned().unwrap_or_default();
        let date = extra
            .and_then(|bytes| bytes.get(index * 18..index * 18 + 4))
            .map(|four| format::dttm(u32::from_le_bytes([four[0], four[1], four[2], four[3]])))
            .unwrap_or_default();
        let mut range = (position(at), position(at));
        if tag != -1 {
            if let (Some(mark), Some(starts), Some(ends)) =
                (tags.iter().position(|held| *held == tag), &starts, &ends)
            {
                let end_index = starts.entry(mark).map(|entry| usize::from(plc::u16_at(entry, 0)));
                if let (Some(start), Some(end)) =
                    (starts.start(mark), end_index.and_then(|at| ends.start(at)))
                {
                    range = (position(start), position(end));
                }
            }
        }
        let built = reader.story(Story::Comment, from + text_start..from + text_end, &[]);
        out.push(CommentFound {
            start: range.0,
            end: range.1,
            author,
            date,
            body: built.body(),
            pictures: built.pictures,
        });
    }
    out
}

/// The bookmarks in the main text: their names, and where each begins and
/// ends.
fn bookmarks_of<'t>(
    table_of: &dyn Fn(Table) -> Option<&'t [u8]>,
    old: bool,
    main_end: u32,
    position: &dyn Fn(u32) -> TextPosition,
) -> Vec<BookmarkFound> {
    let (Some(names), Some(starts), Some(ends)) = (
        table_of(Table::BookmarkNames),
        table_of(Table::BookmarkStarts),
        table_of(Table::BookmarkEnds),
    ) else {
        return Vec::new();
    };
    let names = plc::strings(names, old);
    let starts = Plc::parse(starts, 4);
    let ends = Plc::parse(ends, 0);
    let mut out = Vec::new();
    for (index, (name, _)) in names.into_iter().enumerate() {
        let (Some(start), Some(entry)) = (starts.start(index), starts.entry(index)) else {
            continue;
        };
        let Some(end) = ends.start(usize::from(plc::u16_at(entry, 0))) else { continue };
        if name.is_empty() || start >= main_end || end > main_end {
            continue;
        }
        out.push(BookmarkFound { name, start: position(start), end: position(end) });
    }
    out
}

// --- Stories ------------------------------------------------------------------------

/// Everything the stories are read from.
struct Context<'a> {
    word: &'a [u8],
    table: &'a [u8],
    data: &'a [u8],
    fib: &'a Fib,
    text: Vec<Char>,
    styles: Vec<Style>,
    fonts: Vec<Font>,
    lists: Lists,
    paragraph_bins: Bins,
    character_bins: Bins,
    drawings: Drawings,
    /// Who made tracked changes, by the number the changes name them with.
    authors: Vec<String>,
    /// The notes' marks in the main text: each note's number, and whether
    /// it is an endnote.
    note_marks: HashMap<u32, (i32, bool)>,
    /// Where each story begins.
    starts: [u32; 9],
}

/// Which story is being read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Story {
    Main,
    Footnote,
    Endnote,
    Header,
    Comment,
    TextBox,
    HeaderTextBox,
}

/// What reading a story comes to.
#[derive(Debug, Default)]
struct Built {
    blocks: Vec<Block>,
    pictures: Vec<PictureFound>,
    links: Vec<LinkFound>,
    /// Where each of the story's characters begins, as the document counts.
    positions: Vec<TextPosition>,
    paragraphs: usize,
}

impl Built {
    fn body(&self) -> Body {
        let mut blocks = self.blocks.clone();
        if blocks.is_empty() {
            blocks.push(Block::Paragraph(Paragraph::default()));
        }
        Body { blocks }
    }

    fn end_position(&self) -> TextPosition {
        self.positions.last().copied().unwrap_or_default()
    }
}

/// The reading of every story, and what they share: the formatting pages
/// already read, and the tracked changes already numbered.
struct Reader<'a> {
    context: &'a Context<'a>,
    pages: Pages,
    changes: Vec<(Option<RevisionKind>, String, String)>,
}

impl<'a> Reader<'a> {
    fn story(&mut self, story: Story, mut range: Range<u32>, section_ends: &[u32]) -> Built {
        let context = self.context;
        // A header's or a text box's story written with an empty paragraph
        // after its own — as LibreOffice writes every one — has one paragraph
        // more than it shows, and LibreOffice leaves it out when it reads one.
        let ends_twice = |end: usize| {
            end >= 2
                && context
                    .text
                    .get(end - 2..end)
                    .is_some_and(|last| last.iter().all(|item| item.character == '\r'))
        };
        let extra = matches!(story, Story::Header | Story::TextBox | Story::HeaderTextBox);
        if extra && range.end > range.start + 1 && ends_twice(range.end as usize) {
            range.end -= 1;
        }
        let subdoc = match story {
            Story::Main => context.starts[0],
            Story::Header => context.starts[2],
            _ => range.start,
        };
        let mut builder = Builder {
            reader: self,
            story,
            subdoc,
            built: Built::default(),
            levels: Vec::new(),
            fields: Vec::new(),
        };
        builder.build(range, section_ends);
        builder.built
    }

    /// Who made a change, when, and its number: one number for one person's
    /// one change, so the runs of it go back inside one wrapper.
    fn change_number(
        &mut self,
        kind: Option<RevisionKind>,
        author: u16,
        date: u32,
    ) -> (String, String, i32) {
        let author = self
            .context
            .authors
            .get(usize::from(author))
            .cloned()
            .unwrap_or_else(|| "Unknown".to_owned());
        let date = format::dttm(date);
        let index = match self
            .changes
            .iter()
            .position(|(was, by, at)| *was == kind && *by == author && *at == date)
        {
            Some(index) => index,
            None => {
                self.changes.push((kind, author.clone(), date.clone()));
                self.changes.len() - 1
            }
        };
        (author, date, i32::try_from(index + 1).unwrap_or(i32::MAX))
    }
}

/// A field being read: its code, and where its result began.
struct Field {
    code: String,
    in_code: bool,
    paragraph: usize,
    start: usize,
    /// The run of the paragraph its result begins with.
    first_run: usize,
    /// Whether a paragraph ended inside it.
    spans: bool,
}

/// How the characters of a stretch are formatted: the model's properties,
/// and what else the sprms say of them.
#[derive(Clone, Debug, Default, PartialEq)]
struct CharFormat {
    properties: RunProperties,
    special: bool,
    picture: u32,
    revision: Option<(RevisionKind, u16, u32)>,
    format_change: Option<(u16, u32)>,
    symbol: Option<(u16, u16)>,
    charset: u8,
}

struct Builder<'r, 'a> {
    reader: &'r mut Reader<'a>,
    story: Story,
    /// Where the story's part of the text begins, which is what the tables
    /// of drawings count from.
    subdoc: u32,
    built: Built,
    levels: Vec<Level>,
    fields: Vec<Field>,
}

impl Builder<'_, '_> {
    /// Walks the story, paragraph by paragraph.
    fn build(&mut self, range: Range<u32>, section_ends: &[u32]) {
        let context = self.reader.context;
        let to = (range.end as usize).min(context.text.len());
        let from = (range.start as usize).min(to);
        let mut start = from;
        for index in from..to {
            let ends = match context.text[index].character {
                '\r' | '\u{7}' => true,
                '\u{c}' => self.story == Story::Main && section_ends.contains(&(index as u32 + 1)),
                _ => false,
            };
            if ends {
                self.paragraph(start..index + 1, true);
                start = index + 1;
            }
        }
        if start < to {
            self.paragraph(start..to, false);
        }
        self.close_tables_to(0);
    }

    /// Where the next character goes, between paragraphs.
    fn here(&self) -> TextPosition {
        TextPosition::new(self.built.paragraphs, 0)
    }

    /// One paragraph: its formatting from its mark's page, its runs from
    /// the character pages, and its place in a table if it is in one.
    fn paragraph(&mut self, range: Range<usize>, marked: bool) {
        let context = self.reader.context;
        let chars = &context.text[range.clone()];
        let Some(mark) = chars.last().copied() else { return };
        let old = context.fib.base.old;

        // The paragraph's own sprms, and the style they sit on.
        let (istd, papx) = context
            .paragraph_bins
            .page_for(mark.fc)
            .and_then(|page| {
                let page = self.reader.pages.get(context.word, page, Kind::Paragraph, old);
                page.entry_for(mark.fc).map(|(_, _, istd, grpprl)| (istd, grpprl.to_vec()))
            })
            .unwrap_or((0, Vec::new()));
        let sprms = sprm::parse(&papx);
        let mut properties = ParagraphProperties::default();
        let mut base = RunProperties::default();
        let mut list = self.apply_style(istd, &mut properties, &mut base);
        properties.style = style_id(&context.styles, istd);
        let (mut in_table, mut depth, mut row_end, mut inner_cell, mut inner_row_end) =
            (false, 0usize, false, false, false);
        for sprm in &sprms {
            match sprm.code {
                sprm::P_IN_TABLE => in_table = sprm.on(),
                sprm::P_ITAP => depth = sprm.u32() as usize,
                sprm::P_TABLE_ROW_END => row_end = sprm.on(),
                sprm::P_INNER_CELL => inner_cell = sprm.on(),
                sprm::P_INNER_ROW_END => inner_row_end = sprm.on(),
                _ => {}
            }
        }
        list.take(&sprms);
        format::apply_paragraph(&mut properties, &sprms);
        properties.numbering = list.reference(&context.lists, properties.numbering);

        // How deep in tables it is, and whether it ends a cell or a row.
        let marked_cell = marked && mark.character == '\u{7}';
        let depth = if in_table || depth > 0 || marked_cell { depth.clamp(1, 64) } else { 0 };
        let ends_row = if depth > 1 { inner_row_end } else { row_end && depth == 1 };
        let ends_cell = if depth > 1 { inner_cell } else { marked_cell };
        self.close_tables_to(depth);
        while self.levels.len() < depth {
            self.levels.push(Level::default());
        }

        // A row's end is not a paragraph: it is where the row's cells are
        // gathered up, as the row's sprms describe them.
        if ends_row {
            if let Some(level) = self.levels.last_mut() {
                level.finish_row(&RowDefinition::from_sprms(&sprms));
            }
            let here = self.here();
            self.built.positions.extend(core::iter::repeat_n(here, chars.len()));
            return;
        }

        let content = if marked { &chars[..chars.len() - 1] } else { chars };
        let paragraph = self.built.paragraphs;
        let (runs, length) = self.runs(range.start, content, &base, paragraph);
        if marked {
            self.built.positions.push(TextPosition::new(paragraph, length));
        }
        for field in &mut self.fields {
            field.spans = true;
        }
        self.built.paragraphs += 1;
        let paragraph = Block::Paragraph(Paragraph { properties, runs });
        match self.levels.last_mut() {
            Some(level) => {
                level.cell_blocks.push(paragraph);
                if ends_cell {
                    level.end_cell();
                }
            }
            None => self.built.blocks.push(paragraph),
        }
    }

    /// Finishes the tables deeper than a paragraph now being read, each put
    /// in the cell it is in.
    fn close_tables_to(&mut self, depth: usize) {
        while self.levels.len() > depth {
            let Some(level) = self.levels.pop() else { break };
            let Some(table) = level.into_table() else { continue };
            let block = Block::Table(Box::new(table));
            match self.levels.last_mut() {
                Some(outer) => outer.cell_blocks.push(block),
                None => self.built.blocks.push(block),
            }
        }
    }

    /// A style's formatting, its base's under it, onto a paragraph's
    /// properties and the run properties its text starts from; and what the
    /// styles say of its list.
    fn apply_style(
        &self,
        istd: u16,
        paragraph: &mut ParagraphProperties,
        chars: &mut RunProperties,
    ) -> ListSprms {
        let context = self.reader.context;
        let mut list = ListSprms::default();
        for style in style_chain(&context.styles, istd).iter().rev() {
            let paragraph_sprms = sprm::parse(&style.papx);
            list.take(&paragraph_sprms);
            format::apply_paragraph(paragraph, &paragraph_sprms);
            format::apply_character(chars, &sprm::parse(&style.chpx), &context.fonts);
        }
        list
    }

    /// The runs of a paragraph: the characters cut where their formatting
    /// changes, with the special ones — tabs, breaks, fields, notes' marks,
    /// pictures and drawings — made into what they are. Returns the runs and
    /// how long the paragraph is as the document counts.
    fn runs(
        &mut self,
        first: usize,
        chars: &[Char],
        base: &RunProperties,
        paragraph: usize,
    ) -> (Vec<Run>, usize) {
        let context = self.reader.context;
        let mut runs: Vec<Run> = Vec::new();
        let mut text = String::new();
        // The formatting in force, the range of file positions it covers,
        // and the change and the change of formatting it makes the runs.
        let mut current: Option<(u32, u32, CharFormat)> = None;
        let mut marks: (Option<Revision>, Option<FormatChange>) = (None, None);
        let mut offset = 0usize;

        for (index, item) in chars.iter().enumerate() {
            let cp = (first + index) as u32;
            self.built.positions.push(TextPosition::new(paragraph, offset));
            let outside =
                current.as_ref().is_none_or(|(from, to, _)| item.fc < *from || item.fc >= *to);
            if outside {
                let (from, to, format) = self.format_at(item.fc, base);
                if current.as_ref().is_none_or(|(_, _, held)| *held != format) {
                    flush(&mut runs, &mut text, current.as_ref().map(|(_, _, held)| held), &marks);
                    marks = self.marks_of(&format, base);
                }
                current = Some((from, to, format));
            }
            let Some((_, _, format)) = &current else { continue };
            let deleted = format.revision.is_some_and(|(kind, ..)| kind == RevisionKind::Deleted);
            let counted = |length: usize| if deleted { 0 } else { length };

            // Fields: the code between the begin and the separator, the
            // result between the separator and the end.
            match item.character {
                '\u{13}' => {
                    flush(&mut runs, &mut text, Some(format), &marks);
                    self.fields.push(Field {
                        code: String::new(),
                        in_code: true,
                        paragraph,
                        start: offset,
                        first_run: runs.len(),
                        spans: false,
                    });
                    continue;
                }
                '\u{14}' => {
                    flush(&mut runs, &mut text, Some(format), &marks);
                    if let Some(field) = self.fields.last_mut() {
                        field.in_code = false;
                        field.start = offset;
                        field.first_run = runs.len();
                    }
                    continue;
                }
                '\u{15}' => {
                    flush(&mut runs, &mut text, Some(format), &marks);
                    if let Some(field) = self.fields.pop() {
                        self.end_field(field, &mut runs, paragraph, offset);
                    }
                    continue;
                }
                _ => {}
            }
            if self.fields.iter().any(|field| field.in_code) {
                // The code is text; the marks of the field's own data are
                // not part of it.
                if let Some(field) = self.fields.iter_mut().rev().find(|field| field.in_code) {
                    if (item.character as u32) >= 0x20 {
                        field.code.push(item.character);
                    }
                }
                continue;
            }

            // A note's mark in the main text, whatever character marks it.
            if self.story == Story::Main {
                if let Some(&(id, endnote)) = context.note_marks.get(&cp) {
                    flush(&mut runs, &mut text, Some(format), &marks);
                    runs.push(run_of(format, &marks, RunContent::NoteReference { id, endnote }));
                    offset += counted(1);
                    continue;
                }
            }

            let special = format.special;
            match item.character {
                '\t' => {
                    flush(&mut runs, &mut text, Some(format), &marks);
                    runs.push(run_of(format, &marks, RunContent::Tab));
                    offset += counted(1);
                }
                '\u{b}' | '\u{c}' | '\u{e}' => {
                    flush(&mut runs, &mut text, Some(format), &marks);
                    let kind = match item.character {
                        '\u{b}' => BreakKind::Line,
                        '\u{c}' => BreakKind::Page,
                        _ => BreakKind::Column,
                    };
                    runs.push(run_of(format, &marks, RunContent::Break(kind)));
                    offset += counted(1);
                }
                '\u{1}' if special && !deleted => {
                    flush(&mut runs, &mut text, Some(format), &marks);
                    if let Some((bytes, extension, width_emu, height_emu)) = shapes::inline_picture(
                        context.data,
                        context.word,
                        format.picture,
                        &context.drawings,
                    ) {
                        self.built.pictures.push(PictureFound {
                            paragraph,
                            offset,
                            bytes,
                            extension,
                            width_emu,
                            height_emu,
                            anchor: None,
                        });
                        runs.push(run_of(format, &marks, RunContent::Text(PICTURE_MARK.into())));
                        offset += 1;
                    }
                }
                '\u{2}' if special && matches!(self.story, Story::Footnote | Story::Endnote) => {
                    flush(&mut runs, &mut text, Some(format), &marks);
                    let endnote = self.story == Story::Endnote;
                    runs.push(run_of(format, &marks, RunContent::NoteReference { id: 0, endnote }));
                    offset += counted(1);
                }
                '\u{8}' if special && !deleted => {
                    flush(&mut runs, &mut text, Some(format), &marks);
                    if self.drawing(cp, paragraph, offset, format, &marks, &mut runs) {
                        offset += 1;
                    }
                }
                '(' if special && format.symbol.is_some() => {
                    let Some((font, character)) = format.symbol else { continue };
                    flush(&mut runs, &mut text, Some(format), &marks);
                    let Some(character) = char::from_u32(u32::from(character)) else { continue };
                    let mut symbol = run_of(format, &marks, RunContent::Text(character.into()));
                    if let Some(font) = context.fonts.get(usize::from(font)) {
                        symbol.properties.font = Some(font.name.clone());
                    }
                    runs.push(symbol);
                    offset += counted(character.len_utf8());
                }
                '\u{1e}' => {
                    text.push('\u{2011}');
                    offset += counted('\u{2011}'.len_utf8());
                }
                '\u{1f}' => {
                    text.push('\u{00AD}');
                    offset += counted('\u{00AD}'.len_utf8());
                }
                // The other marks: comments', separators', and whatever else
                // the format keeps for itself.
                c if (c as u32) < 0x20 || (special && c != ' ') => {}
                c => {
                    // A Word 6 byte in a font of another alphabet is a letter
                    // of that alphabet.
                    let c = if item.narrow && context.fib.base.old {
                        context
                            .word
                            .get(item.fc as usize)
                            .and_then(|byte| text::in_character_set(*byte, format.charset))
                            .unwrap_or(c)
                    } else {
                        c
                    };
                    text.push(c);
                    offset += counted(c.len_utf8());
                }
            }
        }
        flush(&mut runs, &mut text, current.as_ref().map(|(_, _, held)| held), &marks);
        (runs, offset)
    }

    /// A field has ended: a link over its result, where it is one in the
    /// main text, or its instruction on the runs of its result, where its
    /// result is in the paragraph it began in.
    fn end_field(&mut self, field: Field, runs: &mut [Run], paragraph: usize, offset: usize) {
        let code = field.code.trim();
        if field.in_code || field.spans || field.paragraph != paragraph || code.is_empty() {
            return;
        }
        let keyword = code.split_whitespace().next().unwrap_or_default().to_uppercase();
        if keyword == "HYPERLINK" && self.story == Story::Main {
            let address = link_address(&code["HYPERLINK".len()..]);
            if !address.is_empty() && offset > field.start {
                self.built.links.push(LinkFound {
                    paragraph,
                    start: field.start,
                    end: offset,
                    address,
                });
            }
            return;
        }
        // A drawing or an object wrapped in a field of its own, for readers
        // that know fields and not drawings: the drawing is what is meant.
        if matches!(keyword.as_str(), "SHAPE" | "EMBED" | "INCLUDEPICTURE") {
            return;
        }
        for run in runs.iter_mut().skip(field.first_run) {
            let picture = run.content.iter().any(|content| {
                matches!(content, RunContent::Text(text) if text.starts_with(PICTURE_MARK))
                    || matches!(content, RunContent::Shape(_) | RunContent::NoteReference { .. })
            });
            if run.field.is_none() && !picture {
                run.field = Some(code.to_owned());
            }
        }
    }

    /// A floating drawing anchored here: a picture, a shape or a text box.
    /// Whether one was put in.
    fn drawing(
        &mut self,
        cp: u32,
        paragraph: usize,
        offset: usize,
        format: &CharFormat,
        marks: &(Option<Revision>, Option<FormatChange>),
        runs: &mut Vec<Run>,
    ) -> bool {
        let context = self.reader.context;
        let header = self.story == Story::Header;
        if !matches!(self.story, Story::Main | Story::Header) {
            return false;
        }
        let Some(placed) = context.drawings.placed_at(header, cp - self.subdoc) else {
            return false;
        };
        let Some(drawn) = context.drawings.drawn(placed.spid) else { return false };
        if drawn.grouped() {
            return false;
        }
        if let (Some(pib), None) = (drawn.picture(), drawn.text_story()) {
            let Some((bytes, extension)) = context.drawings.stored_picture(pib, context.word)
            else {
                return false;
            };
            let (width_emu, height_emu) = shapes::size_of(&placed);
            self.built.pictures.push(PictureFound {
                paragraph,
                offset,
                bytes,
                extension,
                width_emu,
                height_emu,
                anchor: Some(shapes::anchor_of(&placed, drawn)),
            });
            runs.push(run_of(format, marks, RunContent::Text(PICTURE_MARK.into())));
            return true;
        }
        let text = drawn.text_story().map(|story| self.text_box(story, placed.spid, header));
        let Some(shape) = shapes::shape_of(&placed, drawn, text.unwrap_or_default()) else {
            return false;
        };
        runs.push(run_of(format, marks, RunContent::Shape(Box::new(shape))));
        true
    }

    /// A text box's words: its story, found by the drawing's number or by its
    /// place among the text boxes, read as any other.
    fn text_box(&mut self, story: u32, spid: u32, header: bool) -> Vec<Paragraph> {
        let context = self.reader.context;
        let (which, from, kind) = if header {
            (Table::HeaderTextBoxTexts, context.starts[7], Story::HeaderTextBox)
        } else {
            (Table::TextBoxTexts, context.starts[6], Story::TextBox)
        };
        let Some(bytes) = context
            .fib
            .table(which)
            .and_then(|(offset, length)| context.table.get(offset..offset + length))
        else {
            return Vec::new();
        };
        let plc = Plc::parse(bytes, 22);
        let index = (0..plc.len())
            .find(|index| plc.entry(*index).is_some_and(|entry| plc::u32_at(entry, 14) == spid))
            .unwrap_or(story as usize - 1);
        let (Some(start), Some(end)) = (plc.start(index), plc.end(index)) else {
            return Vec::new();
        };
        let built = self.reader.story(kind, from + start..from + end, &[]);
        let mut paragraphs = Vec::new();
        collect_paragraphs(&built.blocks, &mut paragraphs);
        paragraphs
    }

    /// The formatting at a file position, and the range it covers.
    fn format_at(&mut self, fc: u32, base: &RunProperties) -> (u32, u32, CharFormat) {
        let context = self.reader.context;
        let old = context.fib.base.old;
        let found = context.character_bins.page_for(fc).and_then(|page| {
            let page = self.reader.pages.get(context.word, page, Kind::Character, old);
            page.entry_for(fc).map(|(from, to, _, grpprl)| (from, to, grpprl.to_vec()))
        });
        let (from, to, chpx) = found.unwrap_or((fc, fc + 1, Vec::new()));
        let sprms = sprm::parse(&chpx);
        let mut format = CharFormat { properties: base.clone(), ..CharFormat::default() };
        // A character style under the run's own sprms.
        if let Some(style) = sprms.iter().find(|sprm| sprm.code == sprm::C_ISTD) {
            for style in style_chain(&context.styles, style.u16()).iter().rev() {
                format::apply_character(
                    &mut format.properties,
                    &sprm::parse(&style.chpx),
                    &context.fonts,
                );
            }
        }
        format::apply_character(&mut format.properties, &sprms, &context.fonts);
        let (mut inserted, mut deleted) = (false, false);
        let (mut author, mut date, mut deleted_author, mut deleted_date) = (0, 0, None, None);
        for sprm in &sprms {
            match sprm.code {
                sprm::C_SPECIAL => format.special = sprm.on(),
                sprm::C_PICTURE => format.picture = sprm.u32(),
                sprm::C_INSERTED => inserted = sprm.on(),
                sprm::C_DELETED => deleted = sprm.on(),
                sprm::C_REVISION_AUTHOR => author = sprm.u16(),
                sprm::C_REVISION_DATE => date = sprm.u32(),
                sprm::C_DELETED_AUTHOR => deleted_author = Some(sprm.u16()),
                sprm::C_DELETED_DATE => deleted_date = Some(sprm.u32()),
                sprm::C_FORMAT_CHANGE | sprm::C_FORMAT_CHANGE_90 => {
                    let operand = sprm.operand;
                    if operand.first().is_some_and(|on| *on != 0) {
                        format.format_change =
                            Some((plc::u16_at(operand, 1), plc::u32_at(operand, 3)));
                    }
                }
                sprm::C_SYMBOL => {
                    format.symbol =
                        Some((plc::u16_at(sprm.operand, 0), plc::u16_at(sprm.operand, 2)));
                }
                _ => {}
            }
        }
        format.revision = if deleted {
            Some((
                RevisionKind::Deleted,
                deleted_author.unwrap_or(author),
                deleted_date.unwrap_or(date),
            ))
        } else if inserted {
            Some((RevisionKind::Inserted, author, date))
        } else {
            None
        };
        format.charset = format
            .properties
            .font
            .as_ref()
            .and_then(|name| context.fonts.iter().find(|font| font.name == *name))
            .map_or(0, |font| font.charset);
        (from, to, format)
    }

    /// The tracked change and the change of formatting the runs of a
    /// stretch are part of.
    fn marks_of(
        &mut self,
        format: &CharFormat,
        base: &RunProperties,
    ) -> (Option<Revision>, Option<FormatChange>) {
        let revision = format.revision.map(|(kind, author, date)| {
            let (author, date, id) = self.reader.change_number(Some(kind), author, date);
            Revision { kind, author, date, id }
        });
        // What the formatting was before is not in the file: what rejecting
        // the change puts back is the style's.
        let format_change = format.format_change.map(|(author, date)| {
            let (author, date, id) = self.reader.change_number(None, author, date);
            FormatChange { author, date, id, before: Box::new(base.clone()) }
        });
        (revision, format_change)
    }
}

/// Writes out the text gathered so far as a run.
fn flush(
    runs: &mut Vec<Run>,
    text: &mut String,
    format: Option<&CharFormat>,
    marks: &(Option<Revision>, Option<FormatChange>),
) {
    let Some(format) = format else { return };
    if text.is_empty() {
        return;
    }
    runs.push(run_of(format, marks, RunContent::Text(core::mem::take(text))));
}

/// A run holding one thing, formatted as the text here is.
fn run_of(
    format: &CharFormat,
    marks: &(Option<Revision>, Option<FormatChange>),
    content: RunContent,
) -> Run {
    Run {
        properties: format.properties.clone(),
        content: vec![content],
        field: None,
        revision: marks.0.clone(),
        format_change: marks.1.clone(),
    }
}

/// Every paragraph in some blocks, tables' included.
fn collect_paragraphs(blocks: &[Block], out: &mut Vec<Paragraph>) {
    for block in blocks {
        match block {
            Block::Paragraph(paragraph) => out.push(paragraph.clone()),
            Block::Table(table) => {
                for row in &table.rows {
                    for cell in &row.cells {
                        collect_paragraphs(&cell.blocks, out);
                    }
                }
            }
        }
    }
}

/// A style and the styles it is based on, the style first.
fn style_chain(styles: &[Style], istd: u16) -> Vec<&Style> {
    let mut chain = Vec::new();
    let mut current = istd;
    while let Some(style) = styles.get(usize::from(current)) {
        chain.push(style);
        if style.base == 0x0FFF || style.base == current || chain.len() > 20 {
            break;
        }
        current = style.base;
    }
    chain
}

/// The identifier of the document style a style maps to: the headings and
/// the title, which Word numbers the same in every language.
fn style_id(styles: &[Style], istd: u16) -> Option<String> {
    let style = styles.get(usize::from(istd))?;
    match style.sti {
        1..=9 => return Some(format!("Heading{}", style.sti)),
        62 => return Some("Title".to_owned()),
        _ => {}
    }
    let name = style.name.to_lowercase();
    if let Some(level) = name.strip_prefix("heading ") {
        if let Ok(level) = level.trim().parse::<u8>() {
            if (1..=9).contains(&level) {
                return Some(format!("Heading{level}"));
            }
        }
    }
    (name == "title").then(|| "Title".to_owned())
}

/// What a paragraph's sprms and its styles' say of its list: Word 97's list
/// by number, or Word 6's numbering described on the paragraph.
#[derive(Clone, Copy, Debug, Default)]
struct ListSprms {
    ilfo: Option<u16>,
    /// Word 6's: which of its kinds of numbering, and whether the numbers
    /// are bullets.
    numbered: Option<u8>,
    bullet: bool,
}

impl ListSprms {
    fn take(&mut self, sprms: &[Sprm<'_>]) {
        for sprm in sprms {
            match sprm.code {
                sprm::P_ILFO => self.ilfo = Some(sprm.u16()),
                sprm::P_NUMBERED_LEVEL => self.numbered = Some(sprm.byte()),
                sprm::P_ANLD => self.bullet = sprm.operand.first() == Some(&23),
                _ => {}
            }
        }
    }

    /// The list the paragraph is in, if any: Word 97's by its number, or
    /// Word 6's numbered and bulleted paragraphs. Word 6's outline numbering
    /// of headings is not a list here.
    fn reference(
        &self,
        lists: &Lists,
        level: Option<NumberingReference>,
    ) -> Option<NumberingReference> {
        let level = level.map_or(0, |reference| reference.level);
        if let Some(ilfo) = self.ilfo.filter(|ilfo| *ilfo != 0) {
            let bullet = lists.is_bullet(ilfo, level).unwrap_or(false);
            let id = if bullet { wp_docx::BULLET_LIST } else { wp_docx::NUMBERED_LIST };
            return Some(NumberingReference { id, level });
        }
        if self.ilfo.is_none() && matches!(self.numbered, Some(10 | 11)) {
            let id = if self.bullet { wp_docx::BULLET_LIST } else { wp_docx::NUMBERED_LIST };
            return Some(NumberingReference { id, level: 0 });
        }
        None
    }
}

/// The address a HYPERLINK field names: what is quoted, or the first word
/// that is not a switch; a place in the document, `\l`, after a `#`.
fn link_address(rest: &str) -> String {
    let mut words = Vec::new();
    let mut rest = rest.trim();
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix('"') {
            let (quoted, remainder) = after.split_once('"').unwrap_or((after, ""));
            words.push(quoted.to_owned());
            rest = remainder.trim_start();
        } else {
            let (word, remainder) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
            words.push(word.to_owned());
            rest = remainder.trim_start();
        }
    }
    let mut address = String::new();
    let mut place = None;
    let mut words = words.into_iter();
    while let Some(word) = words.next() {
        match word.as_str() {
            "\\l" => place = words.next(),
            "\\o" | "\\t" => {
                words.next();
            }
            switch if switch.starts_with('\\') => {}
            _ if address.is_empty() => address = word,
            _ => {}
        }
    }
    match place {
        Some(place) => format!("{address}#{place}"),
        None => address,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_links_address_is_what_it_names_and_where_in_it() {
        assert_eq!(link_address(" \"https://example.com/\" "), "https://example.com/");
        assert_eq!(link_address(" \\l \"marked_place\" "), "#marked_place");
        assert_eq!(link_address(" \"page.htm\" \\l \"top\" \\o \"tip\""), "page.htm#top");
        assert_eq!(link_address(" https://example.com/a "), "https://example.com/a");
    }
}
