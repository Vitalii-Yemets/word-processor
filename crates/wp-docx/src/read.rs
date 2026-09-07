//! Reading a document body out of `document.xml`.
//!
//! Only the constructs that carry text are recognized. Everything else is
//! stepped over rather than rejected: a document is full of markup that does not
//! affect what the text says — proofing marks, bookmarks, revision boundaries —
//! and refusing to read a file because of one would be useless behaviour.
//!
//! Two things are deliberately skipped rather than stepped into. Deleted text
//! (`w:delText`, inside `w:del`) is not part of the document as it currently
//! reads; including it would put text a colleague removed back into the output.

use wp_xml::{Event, StartTag};

use crate::model::{
    Alignment, Block, BreakKind, Body, Paragraph, Run, RunContent, RunProperties, Table, TableCell,
    TableRow,
};

/// The WordprocessingML namespace.
pub const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

/// Whether a tag is the given WordprocessingML element.
fn is(tag: &StartTag<'_>, local: &str) -> bool {
    tag.namespace == Some(W) && tag.name.local == local
}

/// Reads a `w:val` attribute.
fn value<'a>(tag: &'a StartTag<'a>) -> Option<&'a str> {
    tag.attribute(Some(W), "val")
}

/// Reads an on/off property.
///
/// `<w:b/>` means bold. So does `<w:b w:val="1"/>`. But `<w:b w:val="0"/>` means
/// *not* bold — which matters, because that is how a run switches off something
/// its style turned on.
fn is_on(tag: &StartTag<'_>) -> bool {
    match value(tag) {
        None => true,
        Some("0" | "false" | "off") => false,
        Some(_) => true,
    }
}

/// Elements that hold runs but add nothing to the text themselves.
fn is_transparent_inline(tag: &StartTag<'_>) -> bool {
    tag.namespace == Some(W)
        && matches!(
            tag.name.local,
            "hyperlink" | "ins" | "smartTag" | "sdt" | "sdtContent" | "bdo" | "dir" | "moveTo"
        )
}

/// Elements that hold blocks but add nothing themselves.
fn is_transparent_block(tag: &StartTag<'_>) -> bool {
    tag.namespace == Some(W)
        && matches!(tag.name.local, "sdt" | "sdtContent" | "ins" | "moveTo")
}

/// Walks a flat list of events as if it were a tree.
struct Cursor<'a, 'e> {
    events: &'e [Event<'a>],
    index: usize,
}

impl<'a, 'e> Cursor<'a, 'e> {
    fn peek(&self) -> Option<&'e Event<'a>> {
        self.events.get(self.index)
    }

    fn advance(&mut self) {
        self.index += 1;
    }

    /// Steps over the rest of an element whose start has already been consumed.
    fn skip_to_end(&mut self) {
        let mut depth = 1usize;
        while let Some(event) = self.peek() {
            self.advance();
            match event {
                Event::Start(_) => depth += 1,
                Event::End(_) => {
                    depth -= 1;
                    if depth == 0 {
                        return;
                    }
                }
                _ => {}
            }
        }
    }

    /// Consumes the end tag of the element being parsed, if it is next.
    fn consume_end(&mut self) {
        if matches!(self.peek(), Some(Event::End(_))) {
            self.advance();
        }
    }

    // --- Blocks ------------------------------------------------------------

    fn parse_block_content(&mut self, blocks: &mut Vec<Block>) {
        while let Some(event) = self.peek() {
            match event {
                Event::End(_) => return,
                Event::Start(tag) if is(tag, "p") => {
                    self.advance();
                    blocks.push(Block::Paragraph(self.parse_paragraph()));
                }
                Event::Empty(tag) if is(tag, "p") => {
                    self.advance();
                    blocks.push(Block::Paragraph(Paragraph::default()));
                }
                Event::Start(tag) if is(tag, "tbl") => {
                    self.advance();
                    blocks.push(Block::Table(self.parse_table()));
                }
                Event::Start(tag) if is_transparent_block(tag) => {
                    self.advance();
                    self.parse_block_content(blocks);
                    self.consume_end();
                }
                Event::Start(_) => {
                    self.advance();
                    self.skip_to_end();
                }
                _ => self.advance(),
            }
        }
    }

    // --- Tables ------------------------------------------------------------

    fn parse_table(&mut self) -> Table {
        let mut table = Table::default();

        while let Some(event) = self.peek() {
            match event {
                Event::End(_) => {
                    self.advance();
                    break;
                }
                Event::Start(tag) if is(tag, "tblPr") => {
                    self.advance();
                    table.style = self.parse_table_properties();
                }
                Event::Start(tag) if is(tag, "tr") => {
                    self.advance();
                    table.rows.push(self.parse_table_row());
                }
                Event::Start(_) => {
                    self.advance();
                    self.skip_to_end();
                }
                _ => self.advance(),
            }
        }

        table
    }

    fn parse_table_properties(&mut self) -> Option<String> {
        let mut style = None;
        while let Some(event) = self.peek() {
            match event {
                Event::End(_) => {
                    self.advance();
                    break;
                }
                Event::Empty(tag) | Event::Start(tag) if is(tag, "tblStyle") => {
                    style = value(tag).map(str::to_owned);
                    let was_start = matches!(event, Event::Start(_));
                    self.advance();
                    if was_start {
                        self.skip_to_end();
                    }
                }
                Event::Start(_) => {
                    self.advance();
                    self.skip_to_end();
                }
                _ => self.advance(),
            }
        }
        style
    }

    fn parse_table_row(&mut self) -> TableRow {
        let mut row = TableRow::default();

        while let Some(event) = self.peek() {
            match event {
                Event::End(_) => {
                    self.advance();
                    break;
                }
                Event::Start(tag) if is(tag, "tc") => {
                    self.advance();
                    let mut cell = TableCell::default();
                    self.parse_block_content(&mut cell.blocks);
                    self.consume_end();
                    row.cells.push(cell);
                }
                Event::Start(_) => {
                    self.advance();
                    self.skip_to_end();
                }
                _ => self.advance(),
            }
        }

        row
    }

    // --- Paragraphs --------------------------------------------------------

    fn parse_paragraph(&mut self) -> Paragraph {
        let mut paragraph = Paragraph::default();
        self.parse_paragraph_content(&mut paragraph);
        self.consume_end();
        paragraph
    }

    fn parse_paragraph_content(&mut self, paragraph: &mut Paragraph) {
        while let Some(event) = self.peek() {
            match event {
                Event::End(_) => return,
                Event::Start(tag) if is(tag, "pPr") => {
                    self.advance();
                    self.parse_paragraph_properties(paragraph);
                }
                Event::Start(tag) if is(tag, "r") => {
                    self.advance();
                    paragraph.runs.push(self.parse_run());
                }
                Event::Empty(tag) if is(tag, "r") => {
                    self.advance();
                    paragraph.runs.push(Run::default());
                }
                Event::Start(tag) if is_transparent_inline(tag) => {
                    self.advance();
                    self.parse_paragraph_content(paragraph);
                    self.consume_end();
                }
                Event::Start(_) => {
                    self.advance();
                    self.skip_to_end();
                }
                _ => self.advance(),
            }
        }
    }

    fn parse_paragraph_properties(&mut self, paragraph: &mut Paragraph) {
        while let Some(event) = self.peek() {
            let tag = match event {
                Event::End(_) => {
                    self.advance();
                    return;
                }
                Event::Start(tag) | Event::Empty(tag) => tag,
                _ => {
                    self.advance();
                    continue;
                }
            };
            let was_start = matches!(event, Event::Start(_));

            if tag.namespace == Some(W) {
                match tag.name.local {
                    "pStyle" => paragraph.style = value(tag).map(str::to_owned),
                    "jc" => paragraph.alignment = value(tag).and_then(Alignment::from_attribute),
                    "bidi" => paragraph.right_to_left = is_on(tag),
                    _ => {}
                }
            }

            self.advance();
            if was_start {
                self.skip_to_end();
            }
        }
    }

    // --- Runs --------------------------------------------------------------

    fn parse_run(&mut self) -> Run {
        let mut run = Run::default();

        while let Some(event) = self.peek() {
            match event {
                Event::End(_) => {
                    self.advance();
                    break;
                }
                Event::Start(tag) if is(tag, "rPr") => {
                    self.advance();
                    run.properties = self.parse_run_properties();
                }
                Event::Start(tag) if is(tag, "t") => {
                    self.advance();
                    let text = self.read_text();
                    run.content.push(RunContent::Text(text));
                }
                Event::Empty(tag) if is(tag, "t") => {
                    self.advance();
                    run.content.push(RunContent::Text(String::new()));
                }
                Event::Empty(tag) | Event::Start(tag) if is(tag, "br") => {
                    let kind = match tag.attribute(Some(W), "type") {
                        Some("page") => BreakKind::Page,
                        Some("column") => BreakKind::Column,
                        _ => BreakKind::Line,
                    };
                    run.content.push(RunContent::Break(kind));
                    let was_start = matches!(event, Event::Start(_));
                    self.advance();
                    if was_start {
                        self.skip_to_end();
                    }
                }
                Event::Empty(tag) | Event::Start(tag) if is(tag, "tab") => {
                    run.content.push(RunContent::Tab);
                    let was_start = matches!(event, Event::Start(_));
                    self.advance();
                    if was_start {
                        self.skip_to_end();
                    }
                }
                Event::Start(_) => {
                    self.advance();
                    self.skip_to_end();
                }
                _ => self.advance(),
            }
        }

        run
    }

    fn parse_run_properties(&mut self) -> RunProperties {
        let mut properties = RunProperties::default();

        while let Some(event) = self.peek() {
            let tag = match event {
                Event::End(_) => {
                    self.advance();
                    return properties;
                }
                Event::Start(tag) | Event::Empty(tag) => tag,
                _ => {
                    self.advance();
                    continue;
                }
            };
            let was_start = matches!(event, Event::Start(_));

            if tag.namespace == Some(W) {
                match tag.name.local {
                    "b" => properties.bold = is_on(tag),
                    "i" => properties.italic = is_on(tag),
                    "strike" => properties.strike = is_on(tag),
                    "rtl" => properties.right_to_left = is_on(tag),
                    // Underline carries a style, and "none" means no underline.
                    "u" => properties.underline = !matches!(value(tag), None | Some("none")),
                    "sz" => properties.size_half_points = value(tag).and_then(|v| v.parse().ok()),
                    "color" => properties.color = value(tag).map(str::to_owned),
                    "rStyle" => properties.style = value(tag).map(str::to_owned),
                    "lang" => properties.language = value(tag).map(str::to_owned),
                    // The font differs per script; the Latin one is the name
                    // users think of as "the font".
                    "rFonts" => {
                        properties.font = tag
                            .attribute(Some(W), "ascii")
                            .or_else(|| tag.attribute(Some(W), "hAnsi"))
                            .or_else(|| tag.attribute(Some(W), "cs"))
                            .map(str::to_owned);
                    }
                    _ => {}
                }
            }

            self.advance();
            if was_start {
                self.skip_to_end();
            }
        }

        properties
    }

    /// Reads the text of a `w:t`, up to its end tag.
    fn read_text(&mut self) -> String {
        let mut text = String::new();
        while let Some(event) = self.peek() {
            match event {
                Event::End(_) => {
                    self.advance();
                    break;
                }
                Event::Text(chunk) => {
                    text.push_str(chunk);
                    self.advance();
                }
                Event::CData(chunk) => {
                    text.push_str(chunk);
                    self.advance();
                }
                Event::Start(_) => {
                    self.advance();
                    self.skip_to_end();
                }
                _ => self.advance(),
            }
        }
        text
    }
}

/// Reads a body out of the events of a `document.xml`.
///
/// A document with no `w:body` yields an empty body rather than an error: the
/// part is still valid XML, and there is nothing to show.
#[must_use]
pub fn parse_body(events: &[Event<'_>]) -> Body {
    let mut index = 0usize;
    while index < events.len() {
        if let Event::Start(tag) = &events[index] {
            if is(tag, "body") {
                index += 1;
                break;
            }
        }
        index += 1;
    }

    let mut cursor = Cursor { events, index };
    let mut blocks = Vec::new();
    cursor.parse_block_content(&mut blocks);
    Body { blocks }
}
