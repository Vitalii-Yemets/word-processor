//! Open Packaging Conventions — the container layer beneath every Office document.
//!
//! ECMA-376 Part 2 describes a `.docx` as a package: a ZIP archive holding
//! *parts*, each with a declared content type, tied together by *relationships*
//! rather than by file paths. This crate turns an archive into that structure
//! and back.
//!
//! The design rule here is the one the whole editor depends on: **a part this
//! program does not understand is still carried, byte for byte, to the file it
//! writes.** A document contains far more than any one program models — tracked
//! changes from a colleague, a chart, an embedded font, a macro. Dropping them
//! on save would destroy work that was never ours to touch.
//!
//! # Example
//!
//! ```no_run
//! use wp_opc::Package;
//!
//! let bytes = std::fs::read("document.docx")?;
//! let package = Package::open(&bytes)?;
//!
//! let main = package.main_document_part().expect("every document has one");
//! println!("main part: {main}");
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

#![forbid(unsafe_code)]

mod content_types;
mod package;
mod part_name;
mod relationships;

pub use content_types::{ContentTypes, CONTENT_TYPES_NAMESPACE};
pub use package::{Package, PackageEntry};
pub use part_name::{relationships_part_for, resolve_target};
pub use relationships::{
    Relationship, Relationships, TargetMode, RELATIONSHIPS_CONTENT_TYPE, RELATIONSHIPS_NAMESPACE,
};

/// The content type of the main document part of a WordprocessingML document.
pub const MAIN_DOCUMENT_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml";

/// The same part in a macro-enabled document (`.docm`).
pub const MAIN_DOCUMENT_MACRO_CONTENT_TYPE: &str =
    "application/vnd.ms-word.document.macroEnabled.main+xml";

/// The same part in a template (`.dotx`).
pub const MAIN_DOCUMENT_TEMPLATE_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.template.main+xml";

/// Relationship type of the package's main document part.
pub const OFFICE_DOCUMENT_RELATIONSHIP: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";

/// Archive name of the content types stream.
///
/// It is not a part: it has no content type of its own and cannot be the target
/// of a relationship. Its name is fixed by the specification.
pub const CONTENT_TYPES_PART: &str = "[Content_Types].xml";

/// Archive name of the package-level relationships part.
pub const ROOT_RELATIONSHIPS_PART: &str = "_rels/.rels";

/// Why a package could not be read or written.
///
/// English by design: developer diagnostics. Text shown to the user is produced
/// by the localized presentation layer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The file is not a usable ZIP archive.
    Archive(wp_zip::Error),
    /// A part that should be XML could not be parsed.
    Xml { part: String, source: wp_xml::Error },
    /// The package has no `[Content_Types].xml`, so no part has a declared type.
    MissingContentTypes,
    /// A part named by the package is not in the archive.
    MissingPart(String),
    /// The package declares no main document, so there is nothing to open.
    NoMainDocument,
    /// A part name breaks the rules the specification sets for them.
    InvalidPartName { name: String, reason: &'static str },
    /// A relationship target points outside the package.
    InvalidTarget { source: String, target: String },
    /// Two relationships in one part share an identifier.
    DuplicateRelationshipId { part: String, id: String },
    /// A required attribute is missing from a package-level element.
    MissingAttribute { element: &'static str, attribute: &'static str },
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Archive(error) => write!(f, "the file is not a usable package: {error}"),
            Self::Xml { part, source } => write!(f, "part {part:?} is not valid XML: {source}"),
            Self::MissingContentTypes => {
                write!(f, "the package has no {CONTENT_TYPES_PART}")
            }
            Self::MissingPart(name) => write!(f, "part {name:?} is declared but absent"),
            Self::NoMainDocument => f.write_str("the package declares no main document part"),
            Self::InvalidPartName { name, reason } => {
                write!(f, "invalid part name {name:?}: {reason}")
            }
            Self::InvalidTarget { source, target } => {
                write!(f, "relationship in {source:?} points outside the package: {target:?}")
            }
            Self::DuplicateRelationshipId { part, id } => {
                write!(f, "relationship id {id:?} appears twice in {part:?}")
            }
            Self::MissingAttribute { element, attribute } => {
                write!(f, "<{element}> is missing its {attribute:?} attribute")
            }
        }
    }
}

impl std::error::Error for Error {}

impl From<wp_zip::Error> for Error {
    fn from(error: wp_zip::Error) -> Self {
        Self::Archive(error)
    }
}
