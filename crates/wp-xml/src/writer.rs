//! Serializing XML.
//!
//! The writer checks what it is asked to produce: names must be valid, end tags
//! must match, and text is always escaped. Producing a malformed part would not
//! surface here but much later, as a document Word refuses to open — with
//! nothing to point at.

use crate::escape::{escape_attribute_value, escape_text};
use crate::name::is_valid_name;
use crate::{Error, ErrorKind, Event, Position, StartTag};

/// Builds an XML document as text.
#[derive(Debug, Default)]
pub struct Writer {
    out: String,
    /// Written names of the elements still open.
    open: Vec<String>,
}

impl Writer {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts a document with room reserved for roughly the expected size.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self { out: String::with_capacity(capacity), open: Vec::new() }
    }

    /// Writes the XML declaration.
    ///
    /// The encoding is always UTF-8 because that is what this writer produces,
    /// and it is what every part of a `.docx` uses.
    pub fn write_declaration(&mut self, standalone: Option<bool>) {
        self.out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"");
        match standalone {
            Some(true) => self.out.push_str(" standalone=\"yes\""),
            Some(false) => self.out.push_str(" standalone=\"no\""),
            None => {}
        }
        self.out.push_str("?>");
    }

    /// Writes `<name ...>` and records the element as open.
    pub fn write_start(&mut self, name: &str, attributes: &[(&str, &str)]) -> Result<(), Error> {
        self.write_tag(name, attributes, false)?;
        self.open.push(name.to_owned());
        Ok(())
    }

    /// Writes `<name ... />`.
    pub fn write_empty(&mut self, name: &str, attributes: &[(&str, &str)]) -> Result<(), Error> {
        self.write_tag(name, attributes, true)
    }

    /// Writes `</name>`, checking that it closes the innermost open element.
    pub fn write_end(&mut self, name: &str) -> Result<(), Error> {
        match self.open.pop() {
            Some(expected) if expected == name => {
                self.out.push_str("</");
                self.out.push_str(name);
                self.out.push('>');
                Ok(())
            }
            Some(expected) => Err(self.error(ErrorKind::MismatchedEndTag {
                expected,
                found: name.to_owned(),
            })),
            None => Err(self.error(ErrorKind::UnexpectedEndTag(name.to_owned()))),
        }
    }

    /// Writes character data, escaping it.
    pub fn write_text(&mut self, text: &str) {
        self.out.push_str(&escape_text(text));
    }

    /// Writes a CDATA section.
    pub fn write_cdata(&mut self, text: &str) -> Result<(), Error> {
        // A section cannot contain its own terminator, and splitting it silently
        // would change where the boundaries are.
        if text.contains("]]>") {
            return Err(self.error(ErrorKind::CDataEndInText));
        }
        self.out.push_str("<![CDATA[");
        self.out.push_str(text);
        self.out.push_str("]]>");
        Ok(())
    }

    /// Writes a comment.
    pub fn write_comment(&mut self, text: &str) -> Result<(), Error> {
        if text.contains("--") || text.ends_with('-') {
            return Err(self.error(ErrorKind::IllegalComment));
        }
        self.out.push_str("<!--");
        self.out.push_str(text);
        self.out.push_str("-->");
        Ok(())
    }

    /// Writes a processing instruction.
    pub fn write_processing_instruction(&mut self, target: &str, data: &str) -> Result<(), Error> {
        if !is_valid_name(target) {
            return Err(self.error(ErrorKind::InvalidName(target.to_owned())));
        }
        if target.eq_ignore_ascii_case("xml") {
            return Err(self.error(ErrorKind::MalformedDeclaration("\"xml\" is a reserved target")));
        }
        if data.contains("?>") {
            return Err(self.error(ErrorKind::Unterminated { construct: "processing instruction" }));
        }
        self.out.push_str("<?");
        self.out.push_str(target);
        if !data.is_empty() {
            self.out.push(' ');
            self.out.push_str(data);
        }
        self.out.push_str("?>");
        Ok(())
    }

    /// Writes a document type declaration verbatim.
    pub fn write_doctype(&mut self, content: &str) {
        self.out.push_str("<!DOCTYPE");
        self.out.push_str(content);
        self.out.push('>');
    }

    /// Writes back an event produced by [`crate::Reader`].
    ///
    /// This is what makes a parse-and-rewrite round trip possible, which is how
    /// the document model proves it did not lose anything.
    pub fn write_event(&mut self, event: &Event<'_>) -> Result<(), Error> {
        match event {
            Event::Declaration { standalone, .. } => {
                self.write_declaration(*standalone);
                Ok(())
            }
            Event::DocType { content } => {
                self.write_doctype(content);
                Ok(())
            }
            Event::Start(tag) => {
                self.write_resolved_tag(tag, false)?;
                self.open.push(tag.name.to_written());
                Ok(())
            }
            Event::Empty(tag) => self.write_resolved_tag(tag, true),
            Event::End(name) => self.write_end(&name.to_written()),
            Event::Text(text) => {
                self.write_text(text);
                Ok(())
            }
            Event::CData(text) => self.write_cdata(text),
            Event::Comment(text) => self.write_comment(text),
            Event::ProcessingInstruction { target, data } => {
                self.write_processing_instruction(target, data)
            }
        }
    }

    /// Finishes the document and returns it.
    pub fn finish(self) -> Result<String, Error> {
        if !self.open.is_empty() {
            let position = position_in(&self.out);
            return Err(Error {
                kind: ErrorKind::UnclosedElements(self.open),
                position,
            });
        }
        Ok(self.out)
    }

    /// The text written so far, for inspection mid-way.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.out
    }

    // --- Internals ---------------------------------------------------------

    fn write_tag(
        &mut self,
        name: &str,
        attributes: &[(&str, &str)],
        self_closing: bool,
    ) -> Result<(), Error> {
        if !is_valid_name(name) {
            return Err(self.error(ErrorKind::InvalidName(name.to_owned())));
        }

        self.out.push('<');
        self.out.push_str(name);
        for (attribute_name, value) in attributes {
            if !is_valid_name(attribute_name) {
                return Err(self.error(ErrorKind::InvalidName((*attribute_name).to_owned())));
            }
            self.out.push(' ');
            self.out.push_str(attribute_name);
            self.out.push_str("=\"");
            self.out.push_str(&escape_attribute_value(value));
            self.out.push('"');
        }
        self.out.push_str(if self_closing { "/>" } else { ">" });
        Ok(())
    }

    /// Writes a tag that came from the reader, restoring its namespace
    /// declarations.
    ///
    /// Declarations are written before the ordinary attributes. XML attaches no
    /// meaning to attribute order, and this is where documents put them anyway.
    fn write_resolved_tag(&mut self, tag: &StartTag<'_>, self_closing: bool) -> Result<(), Error> {
        let written_name = tag.name.to_written();
        if !is_valid_name(&written_name) {
            return Err(self.error(ErrorKind::InvalidName(written_name)));
        }

        self.out.push('<');
        self.out.push_str(&written_name);

        for (prefix, uri) in &tag.declarations {
            self.out.push(' ');
            match prefix {
                Some(prefix) => {
                    self.out.push_str("xmlns:");
                    self.out.push_str(prefix);
                }
                None => self.out.push_str("xmlns"),
            }
            self.out.push_str("=\"");
            self.out.push_str(&escape_attribute_value(uri));
            self.out.push('"');
        }

        for attribute in &tag.attributes {
            let attribute_name = attribute.name.to_written();
            if !is_valid_name(&attribute_name) {
                return Err(self.error(ErrorKind::InvalidName(attribute_name)));
            }
            self.out.push(' ');
            self.out.push_str(&attribute_name);
            self.out.push_str("=\"");
            self.out.push_str(&escape_attribute_value(&attribute.value));
            self.out.push('"');
        }

        self.out.push_str(if self_closing { "/>" } else { ">" });
        Ok(())
    }

    /// Builds an error positioned at the end of what has been written so far.
    fn error(&self, kind: ErrorKind) -> Error {
        Error { kind, position: position_in(&self.out) }
    }
}

/// The position of the end of a string, so writer errors point somewhere real.
fn position_in(text: &str) -> Position {
    let line = text.matches('\n').count() + 1;
    let column = match text.rfind('\n') {
        Some(index) => text[index + 1..].chars().count() + 1,
        None => text.chars().count() + 1,
    };
    Position { offset: text.len(), line, column }
}
