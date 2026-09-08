//! The pull parser.
//!
//! Events borrow from the input rather than copying it, so walking a large
//! `document.xml` allocates only for the few values that genuinely change —
//! attribute lists, and text that contained an entity reference.

use std::borrow::Cow;

use crate::escape::{unescape, unescape_attribute_value};
use crate::name::{is_name_continuation, is_name_start, is_valid_name};
use crate::{
    Attribute, Error, ErrorKind, Event, Position, QName, StartTag, XMLNS_NAMESPACE, XML_NAMESPACE,
};

/// Reads XML as a sequence of events.
#[derive(Debug)]
pub struct Reader<'a> {
    input: &'a str,
    offset: usize,
    /// Namespace bindings currently in scope, innermost last. A `None` prefix is
    /// the default namespace.
    bindings: Vec<(Option<&'a str>, &'a str)>,
    /// How many bindings were in scope when each open element started, so its
    /// declarations can be discarded when it closes.
    scopes: Vec<usize>,
    /// Written names of the elements currently open, for matching end tags.
    open: Vec<&'a str>,
    /// How many elements have appeared at the top level. A document must have
    /// exactly one.
    root_count: usize,
    /// Whether anything at all has been read, which decides if an XML
    /// declaration is still allowed.
    started: bool,
    finished: bool,
}

impl<'a> Reader<'a> {
    /// Starts reading already-decoded text.
    ///
    /// To read raw bytes, run them through [`crate::decode_to_utf8`] first.
    #[must_use]
    pub fn new(input: &'a str) -> Self {
        Self {
            input,
            offset: 0,
            bindings: Vec::new(),
            scopes: Vec::new(),
            open: Vec::new(),
            root_count: 0,
            started: false,
            finished: false,
        }
    }

    /// The current read position, for diagnostics.
    #[must_use]
    pub fn position(&self) -> Position {
        self.position_at(self.offset)
    }

    /// Produces the next event, or `None` at the end of a well-formed document.
    ///
    /// After an error the reader stops; a half-parsed document is never worth
    /// continuing, because everything after the error is guesswork.
    pub fn next_event(&mut self) -> Option<Result<Event<'a>, Error>> {
        if self.finished {
            return None;
        }
        match self.step() {
            Ok(Some(event)) => {
                self.started = true;
                Some(Ok(event))
            }
            Ok(None) => {
                self.finished = true;
                None
            }
            Err(error) => {
                self.finished = true;
                Some(Err(error))
            }
        }
    }

    /// Collects every event, failing on the first error.
    pub fn into_events(mut self) -> Result<Vec<Event<'a>>, Error> {
        let mut events = Vec::new();
        while let Some(event) = self.next_event() {
            events.push(event?);
        }
        Ok(events)
    }

    // --- Dispatch ----------------------------------------------------------

    fn step(&mut self) -> Result<Option<Event<'a>>, Error> {
        if self.offset >= self.input.len() {
            if !self.open.is_empty() {
                let names = self.open.iter().map(|name| (*name).to_owned()).collect();
                return Err(self.error_at(self.offset, ErrorKind::UnclosedElements(names)));
            }
            if self.root_count != 1 {
                return Err(
                    self.error_at(self.offset, ErrorKind::RootElementCount(self.root_count))
                );
            }
            return Ok(None);
        }

        if self.input[self.offset..].starts_with('<') {
            self.parse_markup().map(Some)
        } else {
            self.parse_text().map(Some)
        }
    }

    fn parse_markup(&mut self) -> Result<Event<'a>, Error> {
        let rest = &self.input[self.offset..];

        if !self.started && self.starts_declaration(rest) {
            return self.parse_declaration();
        }
        if rest.starts_with("<?") {
            return self.parse_processing_instruction();
        }
        if rest.starts_with("<!--") {
            return self.parse_comment();
        }
        if rest.starts_with("<![CDATA[") {
            return self.parse_cdata();
        }
        if rest.starts_with("<!DOCTYPE") {
            return self.parse_doctype();
        }
        if rest.starts_with("</") {
            return self.parse_end_tag();
        }
        self.parse_start_tag()
    }

    /// Distinguishes the XML declaration from a processing instruction whose
    /// target merely begins with "xml".
    fn starts_declaration(&self, rest: &str) -> bool {
        let Some(after) = rest.strip_prefix("<?xml") else {
            return false;
        };
        after.starts_with(char::is_whitespace) || after.starts_with("?>")
    }

    // --- Character data ----------------------------------------------------

    fn parse_text(&mut self) -> Result<Event<'a>, Error> {
        let start = self.offset;
        let end = match self.input[start..].find('<') {
            Some(index) => start + index,
            None => self.input.len(),
        };
        let raw = &self.input[start..end];

        // "]]>" is forbidden in content: it would otherwise be impossible to
        // tell where a CDATA section really ended.
        if let Some(index) = raw.find("]]>") {
            return Err(self.error_at(start + index, ErrorKind::CDataEndInText));
        }

        // Only whitespace may appear outside the root element.
        if self.open.is_empty() {
            if let Some((index, character)) =
                raw.char_indices().find(|(_, character)| !character.is_whitespace())
            {
                return Err(self.error_at(
                    start + index,
                    ErrorKind::UnexpectedCharacter { found: character, expected: "an element" },
                ));
            }
        }

        self.offset = end;
        let text = unescape(raw).map_err(|kind| self.error_at(start, kind))?;
        Ok(Event::Text(text))
    }

    fn parse_cdata(&mut self) -> Result<Event<'a>, Error> {
        const OPENING: &str = "<![CDATA[";
        let start = self.offset + OPENING.len();

        let Some(index) = self.input[start..].find("]]>") else {
            return Err(
                self.error_at(self.offset, ErrorKind::Unterminated { construct: "CDATA section" })
            );
        };

        let content = &self.input[start..start + index];
        self.offset = start + index + 3;
        Ok(Event::CData(content))
    }

    // --- Markup other than elements ----------------------------------------

    fn parse_declaration(&mut self) -> Result<Event<'a>, Error> {
        let start = self.offset;
        // The search starts past "<?xml" so that the "?" of the opening cannot
        // itself be mistaken for the start of the terminator.
        let body_start = start + "<?xml".len();
        let Some(index) = self.input[body_start..].find("?>") else {
            return Err(
                self.error_at(start, ErrorKind::Unterminated { construct: "XML declaration" })
            );
        };

        let body = &self.input[body_start..body_start + index];
        self.offset = body_start + index + 2;

        let version = pseudo_attribute(body, "version")
            .ok_or_else(|| self.error_at(start, ErrorKind::MalformedDeclaration("no version")))?;
        let encoding = pseudo_attribute(body, "encoding");
        let standalone = match pseudo_attribute(body, "standalone") {
            Some("yes") => Some(true),
            Some("no") => Some(false),
            None => None,
            Some(_) => {
                return Err(self.error_at(
                    start,
                    ErrorKind::MalformedDeclaration("standalone must be \"yes\" or \"no\""),
                ))
            }
        };

        Ok(Event::Declaration { version, encoding, standalone })
    }

    fn parse_processing_instruction(&mut self) -> Result<Event<'a>, Error> {
        let start = self.offset;
        // The search starts past the opening "<?", otherwise input such as "<?>"
        // finds its terminator inside the opening itself.
        let body_start = start + 2;
        let Some(index) = self.input[body_start..].find("?>") else {
            return Err(self
                .error_at(start, ErrorKind::Unterminated { construct: "processing instruction" }));
        };

        let body = &self.input[body_start..body_start + index];
        self.offset = body_start + index + 2;

        let split = body.find(char::is_whitespace).unwrap_or(body.len());
        let target = &body[..split];
        let data = body[split..].trim_start();

        if !is_valid_name(target) {
            return Err(self.error_at(body_start, ErrorKind::InvalidName(target.to_owned())));
        }
        // "xml" in any casing is reserved by the specification.
        if target.eq_ignore_ascii_case("xml") {
            return Err(self
                .error_at(start, ErrorKind::MalformedDeclaration("\"xml\" is a reserved target")));
        }

        Ok(Event::ProcessingInstruction { target, data })
    }

    fn parse_comment(&mut self) -> Result<Event<'a>, Error> {
        let start = self.offset + 4;
        let Some(index) = self.input[start..].find("-->") else {
            return Err(
                self.error_at(self.offset, ErrorKind::Unterminated { construct: "comment" })
            );
        };

        let content = &self.input[start..start + index];
        // XML forbids "--" inside a comment, and forbids one ending in "-".
        if content.contains("--") || content.ends_with('-') {
            return Err(self.error_at(self.offset, ErrorKind::IllegalComment));
        }

        self.offset = start + index + 3;
        Ok(Event::Comment(content))
    }

    fn parse_doctype(&mut self) -> Result<Event<'a>, Error> {
        let start = self.offset;
        let mut index = start + "<!DOCTYPE".len();
        let mut subset_depth = 0usize;

        // The internal subset is bracketed and may itself contain ">".
        let end = loop {
            let Some(character) = self.input[index..].chars().next() else {
                return Err(self.error_at(
                    start,
                    ErrorKind::Unterminated { construct: "document type declaration" },
                ));
            };
            match character {
                '[' => subset_depth += 1,
                ']' => subset_depth = subset_depth.saturating_sub(1),
                '>' if subset_depth == 0 => break index,
                _ => {}
            }
            index += character.len_utf8();
        };

        let content = &self.input[start + "<!DOCTYPE".len()..end];
        self.offset = end + 1;
        Ok(Event::DocType { content })
    }

    // --- Elements ----------------------------------------------------------

    fn parse_end_tag(&mut self) -> Result<Event<'a>, Error> {
        let tag_start = self.offset;
        let name_start = tag_start + 2;
        let (written, mut index) = self.read_name(name_start)?;

        index = skip_whitespace(self.input, index);
        if !self.input[index..].starts_with('>') {
            return Err(self.unexpected(index, "\">\""));
        }
        self.offset = index + 1;

        let Some(expected) = self.open.pop() else {
            return Err(self.error_at(tag_start, ErrorKind::UnexpectedEndTag(written.to_owned())));
        };
        if expected != written {
            return Err(self.error_at(
                tag_start,
                ErrorKind::MismatchedEndTag {
                    expected: expected.to_owned(),
                    found: written.to_owned(),
                },
            ));
        }

        let scope = self.scopes.pop().unwrap_or(0);
        self.bindings.truncate(scope);

        Ok(Event::End(QName::parse(written)))
    }

    fn parse_start_tag(&mut self) -> Result<Event<'a>, Error> {
        let tag_start = self.offset;
        let (written, mut index) = self.read_name(tag_start + 1)?;

        // Attributes are collected before any namespace work: a declaration on
        // this very tag is in scope for the tag itself, so nothing can be
        // resolved until they have all been seen.
        let mut raw_attributes: Vec<(QName<'a>, &'a str, usize)> = Vec::new();
        let is_empty;

        loop {
            let after_space = skip_whitespace(self.input, index);
            let had_space = after_space > index;
            index = after_space;

            if self.input[index..].starts_with("/>") {
                is_empty = true;
                index += 2;
                break;
            }
            if self.input[index..].starts_with('>') {
                is_empty = false;
                index += 1;
                break;
            }
            if index >= self.input.len() {
                return Err(
                    self.error_at(tag_start, ErrorKind::Unterminated { construct: "start tag" })
                );
            }
            if !had_space {
                return Err(self.unexpected(index, "whitespace before an attribute"));
            }

            let attribute_start = index;
            let (attribute_name, after_name) = self.read_name(index)?;
            index = skip_whitespace(self.input, after_name);

            if !self.input[index..].starts_with('=') {
                return Err(self.unexpected(index, "\"=\""));
            }
            index = skip_whitespace(self.input, index + 1);

            let Some(quote) =
                self.input[index..].chars().next().filter(|q| *q == '"' || *q == '\'')
            else {
                return Err(self.unexpected(index, "a quoted attribute value"));
            };
            let value_start = index + 1;
            let Some(closing) = self.input[value_start..].find(quote) else {
                return Err(self.error_at(
                    attribute_start,
                    ErrorKind::Unterminated { construct: "attribute value" },
                ));
            };
            let raw_value = &self.input[value_start..value_start + closing];
            // "<" in an attribute value is always an error, and is usually a
            // sign that a quote was left unclosed further back.
            if raw_value.contains('<') {
                return Err(self.unexpected(value_start, "an attribute value without \"<\""));
            }
            index = value_start + closing + 1;

            raw_attributes.push((QName::parse(attribute_name), raw_value, attribute_start));
        }

        self.offset = index;

        let scope = self.bindings.len();
        let declarations = self.push_declarations(&raw_attributes)?;
        let tag = self.build_start_tag(written, &raw_attributes, declarations, tag_start)?;

        if self.open.is_empty() {
            self.root_count += 1;
        }

        if is_empty {
            // An empty element opens and closes at once, so its declarations
            // leave scope immediately.
            self.bindings.truncate(scope);
            Ok(Event::Empty(tag))
        } else {
            self.scopes.push(scope);
            self.open.push(written);
            Ok(Event::Start(tag))
        }
    }

    /// Brings the `xmlns` attributes of a tag into scope and returns them.
    fn push_declarations(
        &mut self,
        raw_attributes: &[(QName<'a>, &'a str, usize)],
    ) -> Result<Vec<(Option<&'a str>, &'a str)>, Error> {
        let mut declarations = Vec::new();

        for (name, value, position) in raw_attributes {
            let prefix = match (name.prefix, name.local) {
                (Some("xmlns"), local) => Some(local),
                (None, "xmlns") => None,
                _ => continue,
            };

            // The specification makes these absolute, with no exceptions.
            if prefix == Some("xmlns") {
                return Err(self.error_at(
                    *position,
                    ErrorKind::IllegalNamespaceDeclaration(
                        "the prefix \"xmlns\" cannot be declared".to_owned(),
                    ),
                ));
            }
            if prefix == Some("xml") && *value != XML_NAMESPACE {
                return Err(self.error_at(
                    *position,
                    ErrorKind::IllegalNamespaceDeclaration(format!(
                        "the prefix \"xml\" is bound to {XML_NAMESPACE} and cannot be rebound"
                    )),
                ));
            }
            if *value == XMLNS_NAMESPACE {
                return Err(self.error_at(
                    *position,
                    ErrorKind::IllegalNamespaceDeclaration(format!(
                        "{XMLNS_NAMESPACE} cannot be bound to a prefix"
                    )),
                ));
            }
            // A prefix may not be undeclared in XML 1.0; the default may.
            if value.is_empty() && prefix.is_some() {
                return Err(self.error_at(
                    *position,
                    ErrorKind::IllegalNamespaceDeclaration(
                        "a prefix cannot be bound to an empty namespace".to_owned(),
                    ),
                ));
            }

            self.bindings.push((prefix, value));
            declarations.push((prefix, *value));
        }

        Ok(declarations)
    }

    /// Resolves the element and attribute names now that declarations are in scope.
    fn build_start_tag(
        &self,
        written: &'a str,
        raw_attributes: &[(QName<'a>, &'a str, usize)],
        declarations: Vec<(Option<&'a str>, &'a str)>,
        tag_start: usize,
    ) -> Result<StartTag<'a>, Error> {
        let name = QName::parse(written);
        let namespace = self.resolve_element(name, tag_start)?;

        let mut attributes = Vec::with_capacity(raw_attributes.len());
        for (attribute_name, raw_value, position) in raw_attributes {
            // Declarations are reported separately, not as ordinary attributes.
            if matches!(
                (attribute_name.prefix, attribute_name.local),
                (Some("xmlns"), _) | (None, "xmlns")
            ) {
                continue;
            }

            // An unprefixed attribute is in no namespace: unlike an element, it
            // does not pick up the default one.
            let attribute_namespace = match attribute_name.prefix {
                Some(prefix) => Some(self.resolve_prefix(prefix, *position)?),
                None => None,
            };

            let value = unescape_attribute_value(raw_value)
                .map_err(|kind| self.error_at(*position, kind))?;

            attributes.push(Attribute {
                name: *attribute_name,
                namespace: attribute_namespace,
                value,
            });
        }

        // Uniqueness is defined on the expanded name, so `a:x` and `b:x` clash
        // when both prefixes point at the same namespace.
        for (index, attribute) in attributes.iter().enumerate() {
            let clash = attributes[..index].iter().any(|earlier| {
                earlier.name.local == attribute.name.local
                    && earlier.namespace == attribute.namespace
            });
            if clash {
                return Err(self.error_at(
                    tag_start,
                    ErrorKind::DuplicateAttribute(attribute.name.to_written()),
                ));
            }
        }

        Ok(StartTag { name, namespace, attributes, declarations })
    }

    fn resolve_element(&self, name: QName<'a>, position: usize) -> Result<Option<&'a str>, Error> {
        match name.prefix {
            Some(prefix) => Ok(Some(self.resolve_prefix(prefix, position)?)),
            // An unprefixed element takes the default namespace, if one is bound.
            None => Ok(self.lookup(None).filter(|uri| !uri.is_empty())),
        }
    }

    fn resolve_prefix(&self, prefix: &str, position: usize) -> Result<&'a str, Error> {
        if prefix == "xml" {
            return Ok(XML_NAMESPACE);
        }
        self.lookup(Some(prefix))
            .ok_or_else(|| self.error_at(position, ErrorKind::UndeclaredPrefix(prefix.to_owned())))
    }

    /// Finds the innermost binding for a prefix.
    fn lookup(&self, prefix: Option<&str>) -> Option<&'a str> {
        self.bindings.iter().rev().find(|(bound, _)| *bound == prefix).map(|(_, uri)| *uri)
    }

    // --- Lexing helpers ----------------------------------------------------

    /// Reads a name starting at `index`, returning it and the offset after it.
    fn read_name(&self, index: usize) -> Result<(&'a str, usize), Error> {
        let rest = &self.input[index..];
        let Some(first) = rest.chars().next() else {
            return Err(self.error_at(index, ErrorKind::UnexpectedEof { expected: "a name" }));
        };
        if !is_name_start(first) {
            return Err(self.error_at(index, ErrorKind::InvalidName(first.to_string())));
        }

        let mut end = first.len_utf8();
        for character in rest[end..].chars() {
            if !is_name_continuation(character) {
                break;
            }
            end += character.len_utf8();
        }

        Ok((&rest[..end], index + end))
    }

    fn unexpected(&self, index: usize, expected: &'static str) -> Error {
        match self.input[index..].chars().next() {
            Some(found) => self.error_at(index, ErrorKind::UnexpectedCharacter { found, expected }),
            None => self.error_at(index, ErrorKind::UnexpectedEof { expected }),
        }
    }

    fn error_at(&self, offset: usize, kind: ErrorKind) -> Error {
        Error { kind, position: self.position_at(offset) }
    }

    /// Turns a byte offset into a line and column for an error message.
    ///
    /// Recomputed from the start each time. That is linear, but it happens only
    /// when something has already gone wrong, and keeping no running state means
    /// the hot path stays a plain scan.
    fn position_at(&self, offset: usize) -> Position {
        let offset = offset.min(self.input.len());
        let preceding = &self.input[..offset];
        let line = preceding.matches('\n').count() + 1;
        let column = match preceding.rfind('\n') {
            Some(index) => preceding[index + 1..].chars().count() + 1,
            None => preceding.chars().count() + 1,
        };
        Position { offset, line, column }
    }
}

/// Advances past any whitespace, returning the new offset.
fn skip_whitespace(input: &str, mut index: usize) -> usize {
    while let Some(character) = input[index..].chars().next() {
        if !character.is_whitespace() {
            break;
        }
        index += character.len_utf8();
    }
    index
}

/// Reads one `name="value"` pair out of an XML declaration.
///
/// The declaration is not an element and its pseudo-attributes are not parsed by
/// the general attribute code, so this small reader stands alone.
fn pseudo_attribute<'a>(body: &'a str, name: &str) -> Option<&'a str> {
    let after_name = body.split_once(name)?.1;
    let after_equals = after_name.trim_start().strip_prefix('=')?.trim_start();
    let quote = after_equals.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let value = &after_equals[quote.len_utf8()..];
    let end = value.find(quote)?;
    Some(&value[..end])
}

impl<'a> Iterator for Reader<'a> {
    type Item = Result<Event<'a>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        self.next_event()
    }
}

/// Convenience for callers that only want the text of a document.
impl<'a> Reader<'a> {
    /// Concatenates every text and CDATA node, ignoring markup.
    pub fn text_content(input: &'a str) -> Result<String, Error> {
        let mut out = String::new();
        let mut reader = Reader::new(input);
        while let Some(event) = reader.next_event() {
            match event? {
                Event::Text(Cow::Borrowed(text)) => out.push_str(text),
                Event::Text(Cow::Owned(text)) => out.push_str(&text),
                Event::CData(text) => out.push_str(text),
                _ => {}
            }
        }
        Ok(out)
    }
}
