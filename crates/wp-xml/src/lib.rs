//! XML parsing and serialization.
//!
//! Every part of a `.docx` other than embedded media is XML, so this layer sits
//! directly under the document model. Two properties drive its design, and both
//! come from what the format is used for rather than from XML itself:
//!
//! * **Nothing is silently normalized.** Whitespace is reported exactly as it
//!   appears, because `xml:space="preserve"` makes it meaningful — the space
//!   between two words in a document is content, not formatting of the file.
//! * **Malformed input is an error, never a guess.** A document that fails to
//!   parse must be reported, not half-read: quietly dropping an element the
//!   parser did not understand would corrupt the user's work on the next save.
//!
//! # Example
//!
//! ```
//! use wp_xml::{Event, Reader};
//!
//! let source = r#"<w:p xmlns:w="urn:example"><w:r><w:t>Hello</w:t></w:r></w:p>"#;
//! let mut reader = Reader::new(source);
//! let mut text = String::new();
//! while let Some(event) = reader.next_event() {
//!     if let Event::Text(chunk) = event? {
//!         text.push_str(&chunk);
//!     }
//! }
//! assert_eq!(text, "Hello");
//! # Ok::<(), wp_xml::Error>(())
//! ```

#![forbid(unsafe_code)]

use std::borrow::Cow;

mod encoding;
mod escape;
mod name;
mod reader;
pub mod tree;
mod writer;

pub use encoding::{decode_to_utf8, Encoding};
pub use escape::{escape_attribute_value, escape_text, unescape};
pub use reader::Reader;
pub use writer::Writer;

/// The namespace bound to the reserved `xml` prefix. Always available, never
/// declared.
pub const XML_NAMESPACE: &str = "http://www.w3.org/XML/1998/namespace";

/// The namespace of the `xmlns` prefix itself.
pub const XMLNS_NAMESPACE: &str = "http://www.w3.org/2000/xmlns/";

/// A qualified name as it was written: an optional prefix and a local part.
///
/// The prefix is kept alongside the resolved namespace because a document must
/// be written back the way it came. Rewriting `w:p` as `ns0:p` is technically
/// equivalent XML, but it makes every saved file differ from the original for no
/// reason, which would make real changes impossible to spot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct QName<'a> {
    pub prefix: Option<&'a str>,
    pub local: &'a str,
}

impl<'a> QName<'a> {
    /// Splits a written name such as `w:pPr` into its parts.
    #[must_use]
    pub fn parse(text: &'a str) -> Self {
        match text.split_once(':') {
            Some((prefix, local)) if !prefix.is_empty() && !local.is_empty() => {
                Self { prefix: Some(prefix), local }
            }
            _ => Self { prefix: None, local: text },
        }
    }

    /// The name as it appears in the document, prefix included.
    #[must_use]
    pub fn to_written(&self) -> String {
        match self.prefix {
            Some(prefix) => format!("{prefix}:{}", self.local),
            None => self.local.to_owned(),
        }
    }
}

impl core::fmt::Display for QName<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.prefix {
            Some(prefix) => write!(f, "{prefix}:{}", self.local),
            None => f.write_str(self.local),
        }
    }
}

/// An attribute of a start tag, with its value already unescaped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attribute<'a> {
    pub name: QName<'a>,
    /// Namespace the attribute belongs to.
    ///
    /// An unprefixed attribute is in no namespace at all — it does *not* pick up
    /// the default namespace the way an element does. This trips people up
    /// often enough to be worth stating.
    pub namespace: Option<&'a str>,
    pub value: Cow<'a, str>,
}

/// A start tag, with namespaces resolved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartTag<'a> {
    pub name: QName<'a>,
    /// Namespace of the element, from its prefix or from the default namespace.
    pub namespace: Option<&'a str>,
    pub attributes: Vec<Attribute<'a>>,
    /// Namespace declarations written on this tag, kept so the element can be
    /// serialized exactly as it arrived.
    pub declarations: Vec<(Option<&'a str>, &'a str)>,
}

impl<'a> StartTag<'a> {
    /// Finds an attribute by namespace and local name.
    #[must_use]
    pub fn attribute(&self, namespace: Option<&str>, local: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|attribute| attribute.namespace == namespace && attribute.name.local == local)
            .map(|attribute| attribute.value.as_ref())
    }

    /// Finds an attribute by its written name, prefix included.
    #[must_use]
    pub fn attribute_by_written_name(&self, written: &str) -> Option<&str> {
        let wanted = QName::parse(written);
        self.attributes
            .iter()
            .find(|attribute| attribute.name == wanted)
            .map(|attribute| attribute.value.as_ref())
    }
}

/// One item from the document, in the order it appears.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event<'a> {
    /// The `<?xml ... ?>` declaration.
    Declaration { version: &'a str, encoding: Option<&'a str>, standalone: Option<bool> },
    /// A document type declaration, kept verbatim.
    ///
    /// Office documents do not use one, and an external entity reference inside
    /// a DTD is the classic way to turn a document parser into a file-disclosure
    /// tool. Nothing here is ever expanded or fetched.
    DocType { content: &'a str },
    /// `<tag ...>`
    Start(StartTag<'a>),
    /// `<tag ... />`, reported once rather than as an empty start/end pair.
    Empty(StartTag<'a>),
    /// `</tag>`
    End(QName<'a>),
    /// Character data, with entity references already expanded.
    Text(Cow<'a, str>),
    /// The contents of a `<![CDATA[ ... ]]>` section, unexpanded.
    CData(&'a str),
    /// The contents of `<!-- ... -->`.
    Comment(&'a str),
    /// `<?target data?>`
    ProcessingInstruction { target: &'a str, data: &'a str },
}

/// Where in the source an error occurred.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Position {
    /// Byte offset from the start of the input.
    pub offset: usize,
    /// One-based line number.
    pub line: usize,
    /// One-based column, counted in characters.
    pub column: usize,
}

impl core::fmt::Display for Position {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "line {}, column {}", self.line, self.column)
    }
}

/// What went wrong, and where.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub kind: ErrorKind,
    pub position: Position,
}

/// The specific problem found.
///
/// English by design: these are developer diagnostics. Messages shown to the
/// user are produced by the localized presentation layer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    /// The document ends in the middle of a construct.
    UnexpectedEof { expected: &'static str },
    /// A construct is not closed the way it must be.
    Unterminated { construct: &'static str },
    /// A character appears where the grammar does not allow it.
    UnexpectedCharacter { found: char, expected: &'static str },
    /// A name is empty or contains characters XML does not permit in names.
    InvalidName(String),
    /// A closing tag does not match the element it would close.
    MismatchedEndTag { expected: String, found: String },
    /// A closing tag appears with no element open.
    UnexpectedEndTag(String),
    /// The document ends with elements still open.
    UnclosedElements(Vec<String>),
    /// The document has no element, or more than one at the top level.
    RootElementCount(usize),
    /// An entity reference that is not one of the five XML built-ins and is not
    /// a character reference. Custom entities require a DTD, which is not
    /// processed.
    UnknownEntity(String),
    /// A character reference names a code point that is not a legal character.
    InvalidCharacterReference(String),
    /// The same attribute appears twice on one tag.
    DuplicateAttribute(String),
    /// A prefix was used that no enclosing element declares.
    UndeclaredPrefix(String),
    /// A namespace declaration breaks a rule the specification makes absolute,
    /// such as rebinding the reserved `xml` or `xmlns` prefixes.
    IllegalNamespaceDeclaration(String),
    /// The literal `]]>` appears in character data, where it is forbidden.
    CDataEndInText,
    /// A comment contains `--`, or ends with `-`.
    IllegalComment,
    /// The XML declaration is malformed or misplaced.
    MalformedDeclaration(&'static str),
    /// The declared character encoding is not one this parser handles.
    UnsupportedEncoding(String),
    /// Input claimed to be UTF-16 or UTF-8 is not valid in that encoding.
    MalformedEncoding(Encoding),
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{} at {}", self.kind, self.position)
    }
}

impl core::fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnexpectedEof { expected } => write!(f, "input ends while expecting {expected}"),
            Self::Unterminated { construct } => write!(f, "unterminated {construct}"),
            Self::UnexpectedCharacter { found, expected } => {
                write!(f, "unexpected {found:?} while expecting {expected}")
            }
            Self::InvalidName(name) => write!(f, "invalid name {name:?}"),
            Self::MismatchedEndTag { expected, found } => {
                write!(f, "closing tag </{found}> does not match open element <{expected}>")
            }
            Self::UnexpectedEndTag(name) => write!(f, "closing tag </{name}> with nothing open"),
            Self::UnclosedElements(names) => write!(f, "elements left open: {names:?}"),
            Self::RootElementCount(count) => {
                write!(f, "a document needs exactly one root element, found {count}")
            }
            Self::UnknownEntity(name) => write!(f, "unknown entity &{name};"),
            Self::InvalidCharacterReference(text) => {
                write!(f, "character reference {text:?} is not a legal character")
            }
            Self::DuplicateAttribute(name) => write!(f, "attribute {name:?} appears twice"),
            Self::UndeclaredPrefix(prefix) => {
                write!(f, "namespace prefix {prefix:?} is undeclared")
            }
            Self::IllegalNamespaceDeclaration(detail) => write!(f, "{detail}"),
            Self::CDataEndInText => f.write_str("the literal \"]]>\" is not allowed in text"),
            Self::IllegalComment => {
                f.write_str("a comment may not contain \"--\" or end with \"-\"")
            }
            Self::MalformedDeclaration(detail) => write!(f, "malformed XML declaration: {detail}"),
            Self::UnsupportedEncoding(name) => write!(f, "unsupported encoding {name:?}"),
            Self::MalformedEncoding(encoding) => write!(f, "input is not valid {encoding:?}"),
        }
    }
}

impl std::error::Error for Error {}
