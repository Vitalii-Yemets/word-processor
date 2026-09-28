//! Writing the binary document: a Word 97-2003 `.doc`.
//!
//! # What is written
//!
//! [MS-DOC] as Word 97 wrote it, which every Word since opens and so does
//! every other program that opens `.doc` at all: the compound file with a
//! `WordDocument` stream, a `1Table` stream and, where there are pictures, a
//! `Data` stream. In the first, the File Information Block, the text as one
//! piece of two bytes a character, the section's properties, and the pages of
//! character and paragraph formatting; in the second, everything found
//! through the block — the piece table, the bin tables that say which page
//! covers which text, the stylesheet, the font table, the two lists, the
//! fields, the section table and the document's properties; in the third,
//! each picture behind the header that says how big it is drawn.
//!
//! What goes into the text is what the model holds that the format has a
//! place for: paragraphs with their formatting and styles, runs with theirs,
//! tabs and breaks, tables of one level, bulleted and numbered lists, links as
//! `HYPERLINK` fields, and pictures in the line that are PNG or JPEG. The
//! rest — shapes, charts, text boxes, headers and footers, footnotes,
//! comments, tracked changes — is what the roadmap names as not done.
//!
//! # Why one piece
//!
//! A file saved quickly by Word has its text scattered in pieces, each
//! appended where the last save left off; a file written whole has one.
//! Written whole, the character positions and the file positions are the same
//! numbers twice over, which is what makes the rest simple to get right.

use wp_docx::links::Destination;
use wp_docx::model::{
    Alignment, Block, BreakKind, LineRule, Paragraph, ParagraphProperties, RunContent,
    RunProperties, TabAlignment, TabLeader, Table, Underline, VerticalAlignment,
};
use wp_docx::styles::StyleKind;
use wp_docx::Document;

use crate::sprm;

/// Where the text begins in the `WordDocument` stream: past the File
/// Information Block, which is nine hundred bytes for a Word 97 file.
const TEXT_AT: u32 = 0x0400;

/// How many offset pairs a Word 97 block has.
const PAIRS: usize = 93;

/// The pairs this writer fills in, by their place in the block.
mod pair {
    pub const STSHF_ORIG: usize = 0;
    pub const STSHF: usize = 1;
    pub const PLCF_SED: usize = 6;
    pub const PLCF_BTE_CHPX: usize = 12;
    pub const PLCF_BTE_PAPX: usize = 13;
    pub const STTBF_FFN: usize = 15;
    pub const PLCF_FLD_MOM: usize = 16;
    pub const DOP: usize = 31;
    pub const CLX: usize = 33;
    pub const PLF_LST: usize = 73;
    pub const PLF_LFO: usize = 74;
}

/// The paragraph mark, the cell mark, and the field characters.
const PARAGRAPH_MARK: u16 = 0x000D;
const CELL_MARK: u16 = 0x0007;
const FIELD_BEGIN: u16 = 0x0013;
const FIELD_SEPARATOR: u16 = 0x0014;
const FIELD_END: u16 = 0x0015;
const PICTURE: u16 = 0x0001;

/// The two lists, by the number a paragraph names them with.
const BULLETS: u16 = 1;
const NUMBERS: u16 = 2;

/// The styles Word 97 keeps places for, whether a document uses them or not:
/// Normal, the nine headings, Default Paragraph Font and four more.
const FIXED_STYLES: u16 = 15;
const DEFAULT_PARAGRAPH_FONT: u16 = 10;

/// The document as a Word 97-2003 file.
#[must_use]
pub fn write(document: &Document) -> Vec<u8> {
    let mut writer = Writer::new(document);
    writer.body();
    writer.finish()
}

/// One stretch of text with one set of character formatting.
struct Run {
    start: u32,
    end: u32,
    grpprl: Vec<u8>,
}

/// One paragraph: where it is, its style and its own formatting.
struct Para {
    start: u32,
    end: u32,
    istd: u16,
    grpprl: Vec<u8>,
}

/// A style as it goes into the stylesheet.
struct StyleEntry {
    id: String,
    name: String,
    /// 1 for a paragraph style, 2 for a character one.
    kind: u16,
    sti: u16,
    base: u16,
    papx: Vec<u8>,
    chpx: Vec<u8>,
}

struct Writer<'a> {
    document: &'a Document,
    text: Vec<u16>,
    runs: Vec<Run>,
    paragraphs: Vec<Para>,
    /// Every field character's position and its entry in the field table.
    fields: Vec<(u32, [u8; 2])>,
    data: Vec<u8>,
    pictures: usize,
    fonts: Vec<String>,
    /// Which style each identifier is, by its place in the stylesheet.
    style_places: Vec<(String, u16)>,
    styles: Vec<StyleEntry>,
    links: Vec<wp_docx::links::Link>,
    /// Which paragraph of the document the next one is, counted as the
    /// links count them: every paragraph, in tables or not.
    paragraph_index: usize,
}

impl<'a> Writer<'a> {
    fn new(document: &'a Document) -> Self {
        let defaults = document.styles().resolve_run(None, &RunProperties::default());
        let font = defaults.font.clone().unwrap_or_else(|| "Times New Roman".to_owned());
        let mut writer = Self {
            document,
            text: Vec::new(),
            runs: Vec::new(),
            paragraphs: Vec::new(),
            fields: Vec::new(),
            data: Vec::new(),
            pictures: 0,
            // The default font first, and the two the lists and the symbol
            // fonts of old documents lean on.
            fonts: vec![font, "Symbol".to_owned()],
            style_places: Vec::new(),
            styles: Vec::new(),
            links: document.hyperlinks(),
            paragraph_index: 0,
        };
        writer.plan_styles();
        writer
    }

    // --- The styles ------------------------------------------------------------

    /// Settles which styles go into the stylesheet and where: Normal first,
    /// the headings in the nine places Word keeps for them, Default Paragraph
    /// Font at ten, and every other style the text uses — with the styles
    /// each is based on — after the fifteen fixed places.
    fn plan_styles(&mut self) {
        let styles = self.document.styles();
        let mut wanted: Vec<String> = Vec::new();
        let body = self.document.body();
        for paragraph in body.paragraphs() {
            if let Some(id) = &paragraph.properties.style {
                wanted.push(id.clone());
            }
            for run in &paragraph.runs {
                if let Some(id) = &run.properties.style {
                    wanted.push(id.clone());
                }
            }
        }
        // And what those are based on, all the way down.
        let mut index = 0;
        while index < wanted.len() {
            if let Some(base) = styles.get(&wanted[index]).and_then(|style| style.based_on.clone())
            {
                if !wanted.contains(&base) {
                    wanted.push(base);
                }
            }
            index += 1;
        }
        let default_paragraph =
            styles.default_style(StyleKind::Paragraph).map(|style| style.id.clone());

        let mut next = FIXED_STYLES;
        for id in &wanted {
            if self.style_places.iter().any(|(held, _)| held == id) {
                continue;
            }
            if Some(id) == default_paragraph.as_ref() {
                self.style_places.push((id.clone(), 0));
                continue;
            }
            if let Some(level) = heading_level(id) {
                self.style_places.push((id.clone(), u16::from(level)));
                continue;
            }
            let Some(style) = styles.get(id) else { continue };
            if !matches!(style.kind, StyleKind::Paragraph | StyleKind::Character) {
                continue;
            }
            self.style_places.push((id.clone(), next));
            next += 1;
        }
    }

    /// The place of a style in the stylesheet, where it has one.
    fn istd(&self, id: &str) -> Option<u16> {
        self.style_places.iter().find(|(held, _)| held == id).map(|(_, place)| *place)
    }

    /// Builds the stylesheet's entries, once the fonts the text uses are
    /// known — a style's font is a number in the font table.
    fn build_styles(&mut self) {
        let document = self.document;
        let styles = document.styles();
        let places = self.style_places.clone();

        // Normal: what the document's defaults resolve to, which is where a
        // `.doc` keeps what a `.docx` keeps in its defaults.
        let defaults = styles.resolve_run(None, &RunProperties::default());
        let paragraph_defaults = styles.resolve_paragraph(&ParagraphProperties::default());
        let mut normal_chpx = Vec::new();
        let font = defaults.font.clone().unwrap_or_default();
        let ftc = self.font_index(&font);
        push(&mut normal_chpx, sprm::C_FONT, &ftc.to_le_bytes());
        push(&mut normal_chpx, sprm::C_FONT_EAST, &ftc.to_le_bytes());
        push(&mut normal_chpx, sprm::C_FONT_OTHER, &ftc.to_le_bytes());
        push(&mut normal_chpx, sprm::C_SIZE, &(defaults.size_half_points as u16).to_le_bytes());
        let normal_paragraph = ParagraphProperties {
            space_after: Some(paragraph_defaults.space_after),
            space_before: Some(paragraph_defaults.space_before),
            line_spacing: paragraph_defaults.line_spacing,
            ..ParagraphProperties::default()
        };
        let normal_papx = self.paragraph_sprms(&normal_paragraph);
        let normal_name = "Normal".to_owned();
        let mut entries = vec![StyleEntry {
            id: String::new(),
            name: normal_name,
            kind: 1,
            sti: 0,
            base: 0x0FFF,
            papx: normal_papx,
            chpx: normal_chpx,
        }];
        entries.push(StyleEntry {
            id: String::new(),
            name: "Default Paragraph Font".to_owned(),
            kind: 2,
            sti: 65,
            base: 0x0FFF,
            papx: Vec::new(),
            chpx: Vec::new(),
        });

        for (id, place) in places {
            if place == 0 {
                continue;
            }
            let Some(style) = styles.get(&id) else { continue };
            let kind = if style.kind == StyleKind::Character { 2 } else { 1 };
            let base = style
                .based_on
                .as_deref()
                .and_then(|base| self.istd(base))
                .unwrap_or(if kind == 1 { 0 } else { DEFAULT_PARAGRAPH_FONT });
            let mut paragraph = style.paragraph.clone();
            paragraph.style = None;
            let papx = if kind == 1 { self.paragraph_sprms(&paragraph) } else { Vec::new() };
            let mut run = style.run.clone();
            run.style = None;
            let chpx = self.character_sprms(&run);
            let name = match heading_level(&id) {
                Some(level) => format!("heading {level}"),
                None => style.name.clone().unwrap_or_else(|| id.clone()),
            };
            let sti = match heading_level(&id) {
                Some(level) => u16::from(level),
                None => 0x0FFE,
            };
            entries.push(StyleEntry { id: id.clone(), name, kind, sti, base, papx, chpx });
        }
        self.styles = entries;
    }

    /// The stylesheet: its header, then each place in order — an empty one
    /// where nothing is kept.
    fn stylesheet(&self) -> Vec<u8> {
        let highest = self.style_places.iter().map(|(_, place)| *place).max().unwrap_or(0);
        let count = (highest + 1).max(FIXED_STYLES);
        let mut out = Vec::new();
        // The header: its own length, then how many styles, how long a
        // style's fixed part is, that the names are written, the built-in
        // styles this writer knows, how many places are fixed, and the
        // default fonts.
        out.extend(18u16.to_le_bytes());
        out.extend(count.to_le_bytes());
        out.extend(10u16.to_le_bytes());
        out.extend(1u16.to_le_bytes());
        out.extend(0x005Bu16.to_le_bytes());
        out.extend(FIXED_STYLES.to_le_bytes());
        out.extend(0u16.to_le_bytes());
        out.extend([0u8; 6]);

        for place in 0..count {
            let entry = match place {
                0 => self.styles.first(),
                DEFAULT_PARAGRAPH_FONT => self.styles.get(1),
                _ => self
                    .style_places
                    .iter()
                    .find(|(_, held)| *held == place)
                    .and_then(|(id, _)| self.styles.iter().find(|entry| entry.id == *id)),
            };
            let Some(entry) = entry else {
                out.extend(0u16.to_le_bytes());
                continue;
            };
            let std = standard_style(entry, place);
            out.extend((std.len() as u16).to_le_bytes());
            out.extend(std);
        }
        out
    }

    // --- The fonts --------------------------------------------------------------

    fn font_index(&mut self, name: &str) -> u16 {
        if name.is_empty() {
            return 0;
        }
        if let Some(found) = self.fonts.iter().position(|held| held.eq_ignore_ascii_case(name)) {
            return found as u16;
        }
        self.fonts.push(name.to_owned());
        (self.fonts.len() - 1) as u16
    }

    /// The font table: a count, no extra data, and each font's entry.
    fn font_table(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend((self.fonts.len() as u16).to_le_bytes());
        out.extend(0u16.to_le_bytes());
        for name in &self.fonts {
            let mut entry = Vec::new();
            // What kind of font: variable pitch, TrueType, a Roman face, or
            // a decorative one for the symbol fonts.
            let family: u8 = if name == "Symbol" { 5 << 4 } else { 1 << 4 };
            entry.push(0x02 | 0x04 | family);
            entry.extend(400i16.to_le_bytes());
            entry.push(if name == "Symbol" { 2 } else { 0 });
            entry.push(0);
            entry.extend([0u8; 10]);
            entry.extend([0u8; 24]);
            for unit in name.encode_utf16() {
                entry.extend(unit.to_le_bytes());
            }
            entry.extend(0u16.to_le_bytes());
            out.push(entry.len() as u8);
            out.extend(entry);
        }
        out
    }

    // --- The text -------------------------------------------------------------

    fn body(&mut self) {
        let document = self.document;
        let blocks = &document.body().blocks;
        for (index, block) in blocks.iter().enumerate() {
            match block {
                Block::Paragraph(paragraph) => self.paragraph(paragraph, None),
                Block::Table(table) => {
                    self.table(table);
                    // A document cannot end in a table: the last character of
                    // the text is a paragraph mark outside one.
                    if index + 1 == blocks.len() {
                        self.paragraph(&Paragraph::default(), None);
                    }
                }
            }
        }
        if self.text.is_empty() {
            self.paragraph(&Paragraph::default(), None);
        }
    }

    fn cp(&self) -> u32 {
        self.text.len() as u32
    }

    /// Puts characters in with one set of character formatting.
    fn put(&mut self, units: &[u16], grpprl: &[u8]) {
        if units.is_empty() {
            return;
        }
        let start = self.cp();
        self.text.extend_from_slice(units);
        let end = self.cp();
        match self.runs.last_mut() {
            Some(last) if last.end == start && last.grpprl == grpprl => last.end = end,
            _ => self.runs.push(Run { start, end, grpprl: grpprl.to_vec() }),
        }
    }

    /// A field character, which carries the mark that it is one.
    fn field_character(&mut self, unit: u16, fld: [u8; 2], grpprl: &[u8]) {
        let mut special = grpprl.to_vec();
        push(&mut special, sprm::C_SPECIAL, &[1]);
        self.fields.push((self.cp(), fld));
        self.put(&[unit], &special);
    }

    /// One paragraph: its runs, the links round them, its mark, and its
    /// formatting on the mark.
    fn paragraph(&mut self, paragraph: &Paragraph, cell: Option<bool>) {
        let start = self.cp();
        let links: Vec<(usize, usize, String)> = self
            .links
            .iter()
            .filter(|link| link.paragraph == self.paragraph_index)
            .filter_map(|link| match &link.destination {
                Destination::Address(address) => {
                    Some((link.range.0, link.range.1, address.clone()))
                }
                Destination::Place(_) => None,
            })
            .collect();
        self.paragraph_index += 1;

        let mut offset = 0usize;
        let mut open: Option<usize> = None;
        for run in &paragraph.runs {
            let grpprl = self.character_sprms(&run.properties);
            for content in &run.content {
                match content {
                    RunContent::Text(text) => {
                        for character in text.chars() {
                            // A link begins or ends here.
                            let here = links
                                .iter()
                                .position(|(from, to, _)| offset >= *from && offset < *to);
                            if here != open {
                                if open.is_some() {
                                    self.field_character(FIELD_END, [0x15, 0x80], &grpprl);
                                }
                                if let Some(index) = here {
                                    self.field_character(FIELD_BEGIN, [0x13, 88], &grpprl);
                                    let code = format!(" HYPERLINK \"{}\" ", links[index].2);
                                    let units: Vec<u16> = code.encode_utf16().collect();
                                    self.put(&units, &grpprl);
                                    self.field_character(FIELD_SEPARATOR, [0x14, 0xFF], &grpprl);
                                }
                                open = here;
                            }
                            let unit = match character {
                                '\u{00AD}' => vec![0x001F],
                                '\u{2011}' => vec![0x001E],
                                '\n' => vec![0x000B],
                                other => {
                                    let mut buffer = [0u16; 2];
                                    other.encode_utf16(&mut buffer).to_vec()
                                }
                            };
                            self.put(&unit, &grpprl);
                            offset += character.len_utf8();
                        }
                    }
                    RunContent::Tab | RunContent::PositionTab(_) => {
                        self.put(&[0x0009], &grpprl);
                        offset += 1;
                    }
                    RunContent::Break(kind) => {
                        let unit = match kind {
                            BreakKind::Line => 0x000B,
                            BreakKind::Page => 0x000C,
                            BreakKind::Column => 0x000E,
                        };
                        self.put(&[unit], &grpprl);
                        offset += 1;
                    }
                    RunContent::Picture(picture) => {
                        if picture.anchor.is_none() {
                            if let Some(bytes) = self.document.embedded_part(&picture.relationship)
                            {
                                if let Some(at) =
                                    self.picture(bytes, picture.width_emu, picture.height_emu)
                                {
                                    let mut special = grpprl.clone();
                                    push(&mut special, sprm::C_SPECIAL, &[1]);
                                    push(&mut special, sprm::C_PICTURE, &at.to_le_bytes());
                                    self.put(&[PICTURE], &special);
                                }
                            }
                        }
                        offset += 1;
                    }
                    RunContent::Shape(_)
                    | RunContent::Group(_)
                    | RunContent::Chart(_)
                    | RunContent::Diagram(_)
                    | RunContent::Ink(_) => offset += 1,
                    _ => {}
                }
            }
        }
        if open.is_some() {
            self.field_character(FIELD_END, [0x15, 0x80], &[]);
        }

        // The mark, a cell's where the paragraph ends a cell.
        let mark = if cell == Some(true) { CELL_MARK } else { PARAGRAPH_MARK };
        self.put(&[mark], &[]);

        let mut grpprl = self.paragraph_sprms(&paragraph.properties);
        if cell.is_some() {
            push(&mut grpprl, sprm::P_IN_TABLE, &[1]);
        }
        let istd = paragraph.properties.style.as_deref().and_then(|id| self.istd(id)).unwrap_or(0);
        self.paragraphs.push(Para { start, end: self.cp(), istd, grpprl });
    }

    /// A table: each cell's paragraphs, the last of each ending in a cell
    /// mark, and each row ended by a mark of its own that carries the row's
    /// cell edges and borders.
    fn table(&mut self, table: &Table) {
        for row in &table.rows {
            let mut edges: Vec<i16> = vec![-108];
            let mut edge = -108i32;
            for (column, cell) in row.cells.iter().enumerate() {
                let blocks: Vec<&Paragraph> = cell
                    .blocks
                    .iter()
                    .filter_map(|block| match block {
                        Block::Paragraph(paragraph) => Some(paragraph),
                        // A table in a cell is one level too deep for this
                        // writer; its words are kept, flattened into the cell.
                        Block::Table(inner) => {
                            let _ = inner;
                            None
                        }
                    })
                    .collect();
                if blocks.is_empty() {
                    self.paragraph(&Paragraph::default(), Some(true));
                } else {
                    for (index, paragraph) in blocks.iter().enumerate() {
                        self.paragraph(paragraph, Some(index + 1 == blocks.len()));
                    }
                }
                let width = cell.width.or_else(|| table.grid.get(column).copied()).unwrap_or(2880);
                edge += width;
                edges.push(edge.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16);
            }

            // The row's own mark.
            let start = self.cp();
            self.put(&[CELL_MARK], &[]);
            let mut grpprl = Vec::new();
            push(&mut grpprl, sprm::P_IN_TABLE, &[1]);
            push(&mut grpprl, sprm::P_TABLE_ROW_END, &[1]);
            push(&mut grpprl, 0x9602, &108i16.to_le_bytes());
            // Lines round every cell, as a new table here has them.
            let single = [0x04u8, 0x01, 0x00, 0x00];
            let mut borders = Vec::new();
            for _ in 0..6 {
                borders.extend(single);
            }
            grpprl.extend(0xD605u16.to_le_bytes());
            grpprl.push(borders.len() as u8);
            grpprl.extend(&borders);
            // The definition: how many cells, where each edge is, and each
            // cell's own lines.
            let count = edges.len() - 1;
            let mut definition = vec![count as u8];
            for edge in &edges {
                definition.extend(edge.to_le_bytes());
            }
            for _ in 0..count {
                definition.extend(0u16.to_le_bytes());
                definition.extend(0u16.to_le_bytes());
                for _ in 0..4 {
                    definition.extend(single);
                }
            }
            // Its length is written one more than what follows, as the
            // format has it; a byte of nothing after it keeps a reader that
            // takes the length at its word from reading into anything else.
            grpprl.extend(sprm::T_DEFINITION.to_le_bytes());
            grpprl.extend((definition.len() as u16 + 1).to_le_bytes());
            grpprl.extend(&definition);
            grpprl.push(0);
            self.paragraphs.push(Para { start, end: self.cp(), istd: 0, grpprl });
        }
    }

    // --- Formatting as sprms --------------------------------------------------------

    fn character_sprms(&mut self, properties: &RunProperties) -> Vec<u8> {
        let mut out = Vec::new();
        if let Some(istd) = properties.style.as_deref().and_then(|id| self.istd(id)) {
            push(&mut out, sprm::C_ISTD, &istd.to_le_bytes());
        }
        let toggles = [
            (properties.bold, sprm::C_BOLD),
            (properties.italic, sprm::C_ITALIC),
            (properties.strike, sprm::C_STRIKE),
            (properties.double_strike, sprm::C_DOUBLE_STRIKE),
            (properties.small_caps, sprm::C_SMALL_CAPS),
            (properties.caps, sprm::C_CAPS),
            (properties.hidden, sprm::C_HIDDEN),
        ];
        for (value, code) in toggles {
            if let Some(on) = value {
                push(&mut out, code, &[u8::from(on)]);
            }
        }
        if let Some(underline) = &properties.underline {
            let kul = match underline {
                Underline::None => 0,
                Underline::Single | Underline::Other(_) => 1,
                Underline::Double => 3,
                Underline::Dotted => 4,
                Underline::Thick => 6,
                Underline::Dashed => 7,
                Underline::Wave => 11,
            };
            push(&mut out, sprm::C_UNDERLINE, &[kul]);
        }
        if let Some(size) = properties.size_half_points {
            push(&mut out, sprm::C_SIZE, &(size.min(3276) as u16).to_le_bytes());
        }
        if let Some(font) = &properties.font {
            let ftc = self.font_index(font);
            push(&mut out, sprm::C_FONT, &ftc.to_le_bytes());
            push(&mut out, sprm::C_FONT_EAST, &ftc.to_le_bytes());
            push(&mut out, sprm::C_FONT_OTHER, &ftc.to_le_bytes());
        }
        if let Some(colour) = properties.color.as_deref().and_then(rgb) {
            push(&mut out, sprm::C_COLOUR, &[colour.0, colour.1, colour.2, 0]);
        }
        if let Some(index) = properties.highlight.as_deref().and_then(highlight_index) {
            push(&mut out, sprm::C_HIGHLIGHT, &[index]);
        }
        if let Some(align) = properties.vertical_align {
            let iss = match align {
                VerticalAlignment::Superscript => 1,
                VerticalAlignment::Subscript => 2,
                VerticalAlignment::Baseline => 0,
            };
            push(&mut out, sprm::C_SUPER_SUB, &[iss]);
        }
        if let Some(position) = properties.position_half_points {
            push(&mut out, sprm::C_POSITION, &(position as i16).to_le_bytes());
        }
        if let Some(lid) = properties.language.as_deref().and_then(language_number) {
            push(&mut out, sprm::C_LANGUAGE, &lid.to_le_bytes());
        }
        out
    }

    fn paragraph_sprms(&mut self, properties: &ParagraphProperties) -> Vec<u8> {
        let mut out = Vec::new();
        if let Some(alignment) = properties.alignment {
            let jc = match alignment {
                Alignment::Start => 0,
                Alignment::Center => 1,
                Alignment::End => 2,
                Alignment::Both => 3,
            };
            push(&mut out, sprm::P_JC_OLD, &[jc]);
            push(&mut out, sprm::P_JC, &[jc]);
        }
        // The indents twice over: the Word 97 sprms, and the ones Word 2000
        // replaced them with, as Word itself writes a `.doc`.
        if let Some(indent) = properties.indent_start {
            let value = clamp16(indent);
            push(&mut out, 0x840F, &value.to_le_bytes());
            push(&mut out, sprm::P_DXA_LEFT, &value.to_le_bytes());
        }
        if let Some(indent) = properties.indent_end {
            let value = clamp16(indent);
            push(&mut out, 0x840E, &value.to_le_bytes());
            push(&mut out, sprm::P_DXA_RIGHT, &value.to_le_bytes());
        }
        if let Some(indent) = properties.indent_first_line {
            let value = clamp16(indent);
            push(&mut out, 0x8411, &value.to_le_bytes());
            push(&mut out, sprm::P_DXA_LEFT1, &value.to_le_bytes());
        }
        if let Some(space) = properties.space_before {
            push(&mut out, sprm::P_DYA_BEFORE, &(space.clamp(0, 0xFFFF) as u16).to_le_bytes());
        }
        if let Some(space) = properties.space_after {
            push(&mut out, sprm::P_DYA_AFTER, &(space.clamp(0, 0xFFFF) as u16).to_le_bytes());
        }
        if let Some(spacing) = properties.line_spacing {
            let (line, multiple): (i16, i16) = match spacing.rule {
                LineRule::Auto => (clamp16(spacing.value), 1),
                LineRule::AtLeast => (clamp16(spacing.value), 0),
                LineRule::Exact => (-clamp16(spacing.value), 0),
            };
            let mut operand = line.to_le_bytes().to_vec();
            operand.extend(multiple.to_le_bytes());
            push(&mut out, sprm::P_DYA_LINE, &operand);
        }
        let toggles = [
            (properties.keep_lines, sprm::P_KEEP),
            (properties.keep_next, sprm::P_KEEP_FOLLOW),
            (properties.page_break_before, sprm::P_PAGE_BREAK_BEFORE),
            (properties.widow_control, sprm::P_WIDOW_CONTROL),
        ];
        for (value, code) in toggles {
            if let Some(on) = value {
                push(&mut out, code, &[u8::from(on)]);
            }
        }
        if let Some(level) = properties.outline_level {
            push(&mut out, sprm::P_OUTLINE_LEVEL, &[level.min(9)]);
        }
        if let Some(numbering) = properties.numbering {
            let list = if numbering.id == wp_docx::BULLET_LIST { BULLETS } else { NUMBERS };
            push(&mut out, sprm::P_ILVL, &[numbering.level.min(8)]);
            push(&mut out, sprm::P_ILFO, &list.to_le_bytes());
        }
        let stops: Vec<_> = properties
            .tab_stops
            .iter()
            .filter(|stop| stop.alignment != TabAlignment::Clear)
            .take(64)
            .collect();
        if !stops.is_empty() {
            // Stops added, none taken away: the count taken away, the count
            // added, where each goes, and what each is.
            let mut operand = vec![0u8, stops.len() as u8];
            for stop in &stops {
                operand.extend(clamp16(stop.position).to_le_bytes());
            }
            for stop in &stops {
                let jc = match stop.alignment {
                    TabAlignment::Center => 1,
                    TabAlignment::End => 2,
                    TabAlignment::Decimal => 3,
                    TabAlignment::Bar => 4,
                    TabAlignment::Start | TabAlignment::Clear => 0,
                };
                let leader = match stop.leader {
                    TabLeader::Dot => 1,
                    TabLeader::Hyphen => 2,
                    TabLeader::Underscore => 3,
                    TabLeader::MiddleDot => 5,
                    TabLeader::None => 0,
                };
                operand.push(jc | (leader << 3));
            }
            out.extend(0xC60Du16.to_le_bytes());
            out.push(operand.len() as u8);
            out.extend(operand);
        }
        out
    }

    // --- Pictures ---------------------------------------------------------------------

    /// A picture into the data stream, behind the header that says how big it
    /// is drawn and inside the drawing records that hold its bytes. Gives
    /// back where it went, or nothing for a format the header cannot hold.
    fn picture(&mut self, bytes: &[u8], width_emu: i64, height_emu: i64) -> Option<u32> {
        let (kind, instance, bse_type) = match wp_image::Format::detect(bytes)? {
            wp_image::Format::Png => (0xF01Eu16, 0x6E0u16, 6u8),
            wp_image::Format::Jpeg => (0xF01D, 0x46A, 5),
            _ => return None,
        };
        self.pictures += 1;
        let uid = identifier(bytes);

        // The picture's own record: its identifier, a tag, and the file.
        let mut blip_body = uid.to_vec();
        blip_body.push(0xFF);
        blip_body.extend_from_slice(bytes);
        let blip = record(instance << 4, kind, &blip_body);

        // The store entry that holds it.
        let mut bse = vec![bse_type, bse_type];
        bse.extend(uid);
        bse.extend(0x00FFu16.to_le_bytes());
        bse.extend((blip.len() as u32).to_le_bytes());
        bse.extend(1u32.to_le_bytes());
        bse.extend(0u32.to_le_bytes());
        bse.extend([0u8, 0, 0, 0]);
        bse.extend(&blip);
        let bse = record((u16::from(bse_type) << 4) | 0x2, 0xF007, &bse);

        // The shape the picture is drawn as: a picture frame, whose one
        // property names the first entry of the store that follows it.
        let mut fsp = (1024 + self.pictures as u32).to_le_bytes().to_vec();
        fsp.extend(0x0000_0A00u32.to_le_bytes());
        let fsp = record((75 << 4) | 0x2, 0xF00A, &fsp);
        let mut fopt = 0x4104u16.to_le_bytes().to_vec();
        fopt.extend(1u32.to_le_bytes());
        let fopt = record((1 << 4) | 0x3, 0xF00B, &fopt);
        let mut shape = fsp;
        shape.extend(fopt);
        let mut drawing = record(0x000F, 0xF004, &shape);
        drawing.extend(bse);

        // The header: how long, how long itself, that a drawing follows, the
        // size to draw it at in twips, and a scale of a thousandth.
        let mut header = Vec::with_capacity(68);
        header.extend(((68 + drawing.len()) as u32).to_le_bytes());
        header.extend(0x44u16.to_le_bytes());
        header.extend(0x0064i16.to_le_bytes());
        header.extend([0u8; 6]);
        header.extend([0u8; 14]);
        header.extend(clamp16((width_emu / 635) as i32).to_le_bytes());
        header.extend(clamp16((height_emu / 635) as i32).to_le_bytes());
        header.extend(1000u16.to_le_bytes());
        header.extend(1000u16.to_le_bytes());
        header.extend([0u8; 8]);
        header.extend([0u8; 2]);
        header.extend([0u8; 16]);
        header.extend([0u8; 4]);
        header.extend(0u16.to_le_bytes());
        debug_assert_eq!(header.len(), 68);

        let at = self.data.len() as u32;
        self.data.extend(header);
        self.data.extend(drawing);
        Some(at)
    }

    // --- Putting the file together ------------------------------------------------------

    fn finish(mut self) -> Vec<u8> {
        self.build_styles();
        let ccp = self.cp();
        let text_end = TEXT_AT + ccp * 2;
        let fc = |cp: u32| TEXT_AT + cp * 2;

        // The WordDocument stream: the block, the text, the section's
        // properties, and the formatting pages.
        let mut word = vec![0u8; TEXT_AT as usize];
        for unit in &self.text {
            word.extend(unit.to_le_bytes());
        }
        let section_at = word.len() as u32;
        let section = self.section_sprms();
        word.extend((section.len() as u16).to_le_bytes());
        word.extend(&section);
        pad_to(&mut word, 512);

        let character_runs: Vec<(u32, u32, Vec<u8>)> =
            self.runs.iter().map(|run| (fc(run.start), fc(run.end), run.grpprl.clone())).collect();
        let first_page = (word.len() / 512) as u32;
        let (pages, character_bins) = character_pages(&character_runs, first_page);
        word.extend(pages);
        let paragraph_entries: Vec<(u32, u32, Vec<u8>)> = self
            .paragraphs
            .iter()
            .map(|para| {
                let mut papx = para.istd.to_le_bytes().to_vec();
                papx.extend(&para.grpprl);
                (fc(para.start), fc(para.end), papx)
            })
            .collect();
        let first_page = (word.len() / 512) as u32;
        let (pages, paragraph_bins) = paragraph_pages(&paragraph_entries, first_page);
        word.extend(pages);
        // Word's smallest stream is a sector's worth of the big ones: a
        // `WordDocument` kept in the small-stream store is one some readers
        // will not look for there.
        if word.len() < 4096 {
            word.resize(4096, 0);
        }

        // The table stream.
        let mut table = Vec::new();
        let mut pairs = [(0u32, 0u32); PAIRS];

        let stylesheet = self.stylesheet();
        pairs[pair::STSHF] = place(&mut table, &stylesheet);
        pairs[pair::STSHF_ORIG] = pairs[pair::STSHF];

        // The piece table: one piece, two bytes a character, from where the
        // text begins.
        let mut plc = Vec::new();
        plc.extend(0u32.to_le_bytes());
        plc.extend(ccp.to_le_bytes());
        plc.extend(0u16.to_le_bytes());
        plc.extend(TEXT_AT.to_le_bytes());
        plc.extend(0u16.to_le_bytes());
        let mut clx = vec![0x02];
        clx.extend((plc.len() as u32).to_le_bytes());
        clx.extend(plc);
        pairs[pair::CLX] = place(&mut table, &clx);

        pairs[pair::PLCF_BTE_CHPX] = place(&mut table, &bin_table(&character_bins, text_end));
        pairs[pair::PLCF_BTE_PAPX] = place(&mut table, &bin_table(&paragraph_bins, text_end));
        pairs[pair::STTBF_FFN] = place(&mut table, &self.font_table());

        // The one section: from the start to the end, and where its
        // properties are.
        let mut sed = Vec::new();
        sed.extend(0u32.to_le_bytes());
        sed.extend(ccp.to_le_bytes());
        sed.extend(0u16.to_le_bytes());
        sed.extend(section_at.to_le_bytes());
        sed.extend(0u16.to_le_bytes());
        sed.extend(0xFFFF_FFFFu32.to_le_bytes());
        pairs[pair::PLCF_SED] = place(&mut table, &sed);

        if !self.fields.is_empty() {
            let mut plcfld = Vec::new();
            for (cp, _) in &self.fields {
                plcfld.extend(cp.to_le_bytes());
            }
            plcfld.extend(ccp.to_le_bytes());
            for (_, fld) in &self.fields {
                plcfld.extend(fld);
            }
            pairs[pair::PLCF_FLD_MOM] = place(&mut table, &plcfld);
        }

        let (lists, levels, overrides) = lists();
        pairs[pair::PLF_LST] = place(&mut table, &lists);
        table.extend(levels);
        pairs[pair::PLF_LFO] = place(&mut table, &overrides);

        pairs[pair::DOP] = place(&mut table, &dop());

        let fib = file_information_block(word.len() as u32, ccp, &pairs, self.pictures > 0);
        word[..fib.len()].copy_from_slice(&fib);

        let mut file = wp_ole::Builder::new();
        file.stream("WordDocument", word);
        file.stream("1Table", table);
        if !self.data.is_empty() {
            file.stream("Data", self.data);
        }
        file.build()
    }

    /// The page: its size, its margins, and which way round it is.
    fn section_sprms(&self) -> Vec<u8> {
        let (width, height) = self.document.page_size();
        let (top, right, bottom, left) = self.document.page_margins();
        let mut out = Vec::new();
        push(&mut out, sprm::S_ORIENTATION, &[if width > height { 2 } else { 1 }]);
        push(&mut out, sprm::S_PAGE_WIDTH, &(width.clamp(0, 0xFFFF) as u16).to_le_bytes());
        push(&mut out, sprm::S_PAGE_HEIGHT, &(height.clamp(0, 0xFFFF) as u16).to_le_bytes());
        push(&mut out, sprm::S_MARGIN_LEFT, &(left.clamp(0, 0xFFFF) as u16).to_le_bytes());
        push(&mut out, sprm::S_MARGIN_RIGHT, &(right.clamp(0, 0xFFFF) as u16).to_le_bytes());
        push(&mut out, sprm::S_MARGIN_TOP, &clamp16(top).to_le_bytes());
        push(&mut out, sprm::S_MARGIN_BOTTOM, &clamp16(bottom).to_le_bytes());
        out
    }
}

// --- The structures, each on its own --------------------------------------------------

/// One style's entry: its fixed part, its name, and its formatting.
fn standard_style(entry: &StyleEntry, place: u16) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend((entry.sti & 0x0FFF).to_le_bytes());
    out.extend(((entry.kind & 0xF) | (entry.base << 4)).to_le_bytes());
    let cupx: u16 = if entry.kind == 1 { 2 } else { 1 };
    let next = if entry.kind == 1 {
        if place == 0 || entry.sti <= 9 {
            0
        } else {
            place
        }
    } else {
        0
    };
    out.extend((cupx | (next << 4)).to_le_bytes());
    let size_at = out.len();
    out.extend(0u16.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    let name: Vec<u16> = entry.name.encode_utf16().collect();
    out.extend((name.len() as u16).to_le_bytes());
    for unit in &name {
        out.extend(unit.to_le_bytes());
    }
    out.extend(0u16.to_le_bytes());

    let upx = |out: &mut Vec<u8>, bytes: &[u8]| {
        if out.len() % 2 == 1 {
            out.push(0);
        }
        out.extend((bytes.len() as u16).to_le_bytes());
        out.extend_from_slice(bytes);
    };
    if entry.kind == 1 {
        let mut papx = place.to_le_bytes().to_vec();
        papx.extend(&entry.papx);
        upx(&mut out, &papx);
    }
    upx(&mut out, &entry.chpx);
    if out.len() % 2 == 1 {
        out.push(0);
    }
    let size = out.len() as u16;
    out[size_at..size_at + 2].copy_from_slice(&size.to_le_bytes());
    out
}

/// The two lists: a bullet at every level, and a number and a full stop.
///
/// The table of lists, the levels that follow it in the stream, and the
/// table of overrides a paragraph names its list by.
fn lists() -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let mut table = 2u16.to_le_bytes().to_vec();
    let mut levels = Vec::new();
    for id in [1i32, 2] {
        table.extend(id.to_le_bytes());
        table.extend((id + 100).to_le_bytes());
        for _ in 0..9 {
            table.extend(0x0FFFu16.to_le_bytes());
        }
        table.push(0);
        table.push(0);

        for level in 0..9u8 {
            let bullet = id == 1;
            let indent = 720 * (i16::from(level) + 1);
            let mut papx = Vec::new();
            push(&mut papx, 0x840F, &indent.to_le_bytes());
            push(&mut papx, 0x8411, &(-360i16).to_le_bytes());
            let chpx = Vec::new();
            let text: Vec<u16> = if bullet { vec![0x2022] } else { vec![u16::from(level), 0x002E] };

            levels.extend(1i32.to_le_bytes());
            levels.push(if bullet { 23 } else { 0 });
            levels.push(0);
            let mut numbers = [0u8; 9];
            if !bullet {
                numbers[0] = 1;
            }
            levels.extend(numbers);
            levels.push(0);
            levels.extend(0i32.to_le_bytes());
            levels.extend(0u32.to_le_bytes());
            levels.push(chpx.len() as u8);
            levels.push(papx.len() as u8);
            levels.push(0);
            levels.push(0);
            levels.extend(&papx);
            levels.extend(&chpx);
            levels.extend((text.len() as u16).to_le_bytes());
            for unit in text {
                levels.extend(unit.to_le_bytes());
            }
        }
    }

    let mut overrides = 2u32.to_le_bytes().to_vec();
    for id in [1i32, 2] {
        overrides.extend(id.to_le_bytes());
        overrides.extend(0u32.to_le_bytes());
        overrides.extend(0u32.to_le_bytes());
        overrides.extend([0u8, 0, 0, 0]);
    }
    for _ in 0..2 {
        overrides.extend(0u32.to_le_bytes());
    }
    (table, levels, overrides)
}

/// The document's properties: Word 97's five hundred bytes, of which the
/// ones that matter to a new document are widow control, the first footnote
/// and endnote number, and the default tab stop.
fn dop() -> Vec<u8> {
    let mut out = vec![0u8; 500];
    out[0] = 0x02;
    out[2..4].copy_from_slice(&(1u16 << 2).to_le_bytes());
    out[10..12].copy_from_slice(&720u16.to_le_bytes());
    out[52..54].copy_from_slice(&(1u16 << 2).to_le_bytes());
    out
}

/// The File Information Block of a Word 97 file.
fn file_information_block(
    stream_length: u32,
    ccp: u32,
    pairs: &[(u32, u32); PAIRS],
    pictures: bool,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(900);
    out.extend(0xA5ECu16.to_le_bytes());
    out.extend(0x00C1u16.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    out.extend(0x0409u16.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    // The tables in 1Table, the extended characters, and whether there are
    // pictures.
    let mut flags: u16 = 0x0200 | 0x1000;
    if pictures {
        flags |= 0x0008;
    }
    out.extend(flags.to_le_bytes());
    out.extend(0x00BFu16.to_le_bytes());
    out.extend(0u32.to_le_bytes());
    out.push(0);
    out.push(0);
    // Four fields the format calls reserved, which Word 97 still fills with
    // what Word 6 kept there — where the text begins and where it ends — and
    // which other readers still read.
    out.extend([0u8; 4]);
    out.extend(TEXT_AT.to_le_bytes());
    out.extend((TEXT_AT + ccp * 2).to_le_bytes());
    debug_assert_eq!(out.len(), 32);

    out.extend(14u16.to_le_bytes());
    out.extend([0u8; 28]);

    out.extend(22u16.to_le_bytes());
    let mut longs = [0u32; 22];
    longs[0] = stream_length;
    longs[3] = ccp;
    for long in longs {
        out.extend(long.to_le_bytes());
    }

    out.extend((PAIRS as u16).to_le_bytes());
    for (offset, length) in pairs {
        out.extend(offset.to_le_bytes());
        out.extend(length.to_le_bytes());
    }
    out.extend(0u16.to_le_bytes());
    out
}

/// The pages of character formatting, and which file position each begins
/// at.
fn character_pages(runs: &[(u32, u32, Vec<u8>)], first_page: u32) -> (Vec<u8>, Vec<(u32, u32)>) {
    let mut pages = Vec::new();
    let mut bins = Vec::new();
    let mut at = 0;
    while at < runs.len() {
        let mut count = 0;
        // As many runs as fit: their positions, a byte each saying where
        // their formatting is, and the formatting from the end of the page
        // down.
        while at + count < runs.len() {
            let trying = &runs[at..at + count + 1];
            if character_page(trying).is_none() {
                break;
            }
            count += 1;
        }
        let count = count.max(1);
        let page = character_page(&runs[at..at + count]).unwrap_or_else(|| vec![0; 512]);
        bins.push((runs[at].0, first_page + (pages.len() / 512) as u32));
        pages.extend(page);
        at += count;
    }
    (pages, bins)
}

fn character_page(runs: &[(u32, u32, Vec<u8>)]) -> Option<Vec<u8>> {
    let count = runs.len();
    let mut page = vec![0u8; 512];
    let head = 4 * (count + 1) + count;
    let mut top = 511usize;
    for (index, (start, end, grpprl)) in runs.iter().enumerate() {
        page[index * 4..index * 4 + 4].copy_from_slice(&start.to_le_bytes());
        if index + 1 == count {
            page[(index + 1) * 4..(index + 1) * 4 + 4].copy_from_slice(&end.to_le_bytes());
        }
        if grpprl.is_empty() {
            continue;
        }
        let size = 1 + grpprl.len();
        let mut start_at = top.checked_sub(size)?;
        start_at &= !1;
        if start_at < head || grpprl.len() > 255 {
            return None;
        }
        page[start_at] = grpprl.len() as u8;
        page[start_at + 1..start_at + size].copy_from_slice(grpprl);
        page[4 * (count + 1) + index] = (start_at / 2) as u8;
        top = start_at;
    }
    page[511] = count as u8;
    Some(page)
}

/// The pages of paragraph formatting, and which file position each begins
/// at.
fn paragraph_pages(
    paragraphs: &[(u32, u32, Vec<u8>)],
    first_page: u32,
) -> (Vec<u8>, Vec<(u32, u32)>) {
    let mut pages = Vec::new();
    let mut bins = Vec::new();
    let mut at = 0;
    while at < paragraphs.len() {
        let mut count = 0;
        while at + count < paragraphs.len() {
            if paragraph_page(&paragraphs[at..at + count + 1]).is_none() {
                break;
            }
            count += 1;
        }
        let count = count.max(1);
        let page = paragraph_page(&paragraphs[at..at + count]).unwrap_or_else(|| vec![0; 512]);
        bins.push((paragraphs[at].0, first_page + (pages.len() / 512) as u32));
        pages.extend(page);
        at += count;
    }
    (pages, bins)
}

fn paragraph_page(paragraphs: &[(u32, u32, Vec<u8>)]) -> Option<Vec<u8>> {
    let count = paragraphs.len();
    let mut page = vec![0u8; 512];
    let head = 4 * (count + 1) + 13 * count;
    let mut top = 511usize;
    for (index, (start, end, papx)) in paragraphs.iter().enumerate() {
        page[index * 4..index * 4 + 4].copy_from_slice(&start.to_le_bytes());
        if index + 1 == count {
            page[(index + 1) * 4..(index + 1) * 4 + 4].copy_from_slice(&end.to_le_bytes());
        }
        // A length in words: an odd length is written as its words and read
        // as one byte short; an even one behind a zero.
        let mut entry = Vec::new();
        if papx.len() % 2 == 1 {
            entry.push(papx.len().div_ceil(2) as u8);
        } else {
            entry.push(0);
            entry.push((papx.len() / 2) as u8);
        }
        entry.extend_from_slice(papx);
        if papx.len() > 500 {
            return None;
        }
        let mut start_at = top.checked_sub(entry.len())?;
        start_at &= !1;
        if start_at < head {
            return None;
        }
        page[start_at..start_at + entry.len()].copy_from_slice(&entry);
        page[4 * (count + 1) + index * 13] = (start_at / 2) as u8;
        top = start_at;
    }
    page[511] = count as u8;
    Some(page)
}

/// A bin table: where each page's range begins, where the last one ends, and
/// each page's number.
fn bin_table(bins: &[(u32, u32)], end: u32) -> Vec<u8> {
    let mut out = Vec::new();
    for (fc, _) in bins {
        out.extend(fc.to_le_bytes());
    }
    out.extend(end.to_le_bytes());
    for (_, page) in bins {
        out.extend(page.to_le_bytes());
    }
    out
}

/// Puts a structure at the end of the table stream, giving back where it is
/// and how long, which is the pair the block keeps for it.
fn place(table: &mut Vec<u8>, bytes: &[u8]) -> (u32, u32) {
    let at = table.len() as u32;
    table.extend_from_slice(bytes);
    (at, bytes.len() as u32)
}

/// A drawing record: its version and instance, its type, its length.
fn record(version_instance: u16, kind: u16, body: &[u8]) -> Vec<u8> {
    let mut out = version_instance.to_le_bytes().to_vec();
    out.extend(kind.to_le_bytes());
    out.extend((body.len() as u32).to_le_bytes());
    out.extend_from_slice(body);
    out
}

/// A picture's identifier: sixteen bytes that are the same for the same
/// picture, which is all a reader asks of them.
fn identifier(bytes: &[u8]) -> [u8; 16] {
    let mut out = [0u8; 16];
    let mut hash: u64 = 0xCBF2_9CE4_8422_2325;
    for (index, byte) in bytes.iter().enumerate() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01B3);
        out[index % 16] ^= (hash >> 32) as u8;
    }
    out[..8].copy_from_slice(&hash.to_le_bytes());
    out
}

fn push(out: &mut Vec<u8>, code: u16, operand: &[u8]) {
    out.extend(code.to_le_bytes());
    out.extend_from_slice(operand);
}

fn pad_to(bytes: &mut Vec<u8>, boundary: usize) {
    let wanted = bytes.len().div_ceil(boundary) * boundary;
    bytes.resize(wanted, 0);
}

fn clamp16(value: i32) -> i16 {
    value.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
}

/// The level of a heading style, from its identifier.
fn heading_level(id: &str) -> Option<u8> {
    let level: u8 = id.strip_prefix("Heading")?.parse().ok()?;
    (1..=9).contains(&level).then_some(level)
}

fn rgb(hex: &str) -> Option<(u8, u8, u8)> {
    let value = u32::from_str_radix(hex.trim_start_matches('#'), 16).ok()?;
    (hex.trim_start_matches('#').len() == 6).then_some((
        (value >> 16) as u8,
        (value >> 8) as u8,
        value as u8,
    ))
}

/// The index of a highlight among Word's sixteen.
fn highlight_index(name: &str) -> Option<u8> {
    (1..=16).find(|index| sprm::highlight_by_index(*index) == Some(name))
}

/// The Windows language number of a language tag, for the ones the reader
/// knows back.
fn language_number(tag: &str) -> Option<u16> {
    (1u16..0x7FFF).find(|number| crate::read::language_tag(*number) == Some(tag))
}
