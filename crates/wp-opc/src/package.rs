//! The package itself: the parts of a document and how they are saved back.

use wp_zip::{Compression, DosDateTime, ZipArchive, ZipWriter};

use crate::content_types::ContentTypes;
use crate::part_name::{is_relationships_part, normalize, relationships_part_for};
use crate::relationships::{Relationships, RELATIONSHIPS_CONTENT_TYPE};
use crate::{
    Error, CONTENT_TYPES_PART, MAIN_DOCUMENT_CONTENT_TYPE, MAIN_DOCUMENT_MACRO_CONTENT_TYPE,
    MAIN_DOCUMENT_TEMPLATE_CONTENT_TYPE, OFFICE_DOCUMENT_RELATIONSHIP,
};

/// One entry of the package, kept exactly as it was stored.
///
/// The compression method and timestamp travel with the data so that saving a
/// document that was not edited reproduces the original file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageEntry {
    /// Archive name, without a leading slash.
    pub name: String,
    pub data: Vec<u8>,
    pub compression: Compression,
    pub last_modified: DosDateTime,
}

impl PackageEntry {
    /// Whether this entry is a directory marker rather than content.
    #[must_use]
    pub fn is_directory(&self) -> bool {
        self.name.ends_with('/')
    }
}

/// An Office document package.
///
/// Every entry of the original archive is held, in its original order, whether
/// or not this program understands it. That is what makes saving safe: a part
/// nothing here models is written back byte for byte instead of being dropped.
#[derive(Clone, Debug)]
pub struct Package {
    entries: Vec<PackageEntry>,
    content_types: ContentTypes,
}

impl Package {
    /// Reads a package from the bytes of a `.docx` file.
    pub fn open(bytes: &[u8]) -> Result<Self, Error> {
        let archive = ZipArchive::open(bytes)?;

        let mut entries = Vec::with_capacity(archive.entries().len());
        for entry in archive.entries() {
            let data = if entry.is_directory() { Vec::new() } else { archive.read(entry)? };
            entries.push(PackageEntry {
                name: entry.name.clone(),
                data,
                compression: entry.compression,
                last_modified: entry.last_modified,
            });
        }

        let content_types_bytes = entries
            .iter()
            .find(|entry| entry.name.eq_ignore_ascii_case(CONTENT_TYPES_PART))
            .map(|entry| entry.data.clone())
            .ok_or(Error::MissingContentTypes)?;

        let content_types =
            parse_xml_part(CONTENT_TYPES_PART, &content_types_bytes).and_then(|text| {
                ContentTypes::parse(&text)
                    .map_err(|source| Error::Xml { part: CONTENT_TYPES_PART.to_owned(), source })
            })?;

        Ok(Self { entries, content_types })
    }

    /// Builds an empty package with no parts and no declared types.
    #[must_use]
    pub fn empty() -> Self {
        Self { entries: Vec::new(), content_types: ContentTypes::default() }
    }

    /// Every entry, in the order it appears in the archive.
    #[must_use]
    pub fn entries(&self) -> &[PackageEntry] {
        &self.entries
    }

    /// The declared content types of the package.
    #[must_use]
    pub fn content_types(&self) -> &ContentTypes {
        &self.content_types
    }

    /// The contents of a part.
    #[must_use]
    pub fn part(&self, name: &str) -> Option<&[u8]> {
        let name = normalize(name);
        self.entries
            .iter()
            .find(|entry| entry.name.eq_ignore_ascii_case(&name))
            .map(|entry| entry.data.as_slice())
    }

    /// The contents of a part, decoded as XML text.
    pub fn xml_part(&self, name: &str) -> Option<Result<String, Error>> {
        self.part(name).map(|bytes| parse_xml_part(name, bytes))
    }

    /// The content type of a part, if the package declares one.
    #[must_use]
    pub fn content_type(&self, name: &str) -> Option<&str> {
        self.content_types.of(name)
    }

    /// Replaces a part's contents, or adds it if absent.
    ///
    /// Adding a part does not declare its content type; that is a separate step,
    /// because a part whose type is undeclared makes the package invalid and
    /// silently inventing one would hide the mistake.
    pub fn set_part(&mut self, name: &str, data: Vec<u8>) {
        let name = normalize(name);
        match self.entries.iter_mut().find(|entry| entry.name.eq_ignore_ascii_case(&name)) {
            Some(entry) => entry.data = data,
            None => self.entries.push(PackageEntry {
                name,
                data,
                compression: Compression::Deflate,
                last_modified: DosDateTime::EPOCH,
            }),
        }
    }

    /// Removes a part and any override declaring its type.
    pub fn remove_part(&mut self, name: &str) {
        let name = normalize(name);
        self.entries.retain(|entry| !entry.name.eq_ignore_ascii_case(&name));
        self.content_types.remove_override(&name);
        self.rewrite_content_types();
    }

    /// Replaces the content type declarations.
    pub fn set_content_types(&mut self, content_types: ContentTypes) {
        self.content_types = content_types;
        self.rewrite_content_types();
    }

    /// Reads the relationships declared by a part.
    ///
    /// Pass an empty name for the package's own relationships. A part with no
    /// relationships part simply has none, which is not an error.
    pub fn relationships(&self, source_part: &str) -> Result<Relationships, Error> {
        let source = normalize(source_part);
        let rels_part = relationships_part_for(&source);

        match self.part(&rels_part) {
            Some(bytes) => {
                let text = parse_xml_part(&rels_part, bytes)?;
                Relationships::parse(&source, &text)
            }
            None => Ok(Relationships::new(&source)),
        }
    }

    /// Writes a part's relationships back into the package.
    pub fn set_relationships(&mut self, relationships: &Relationships) -> Result<(), Error> {
        let rels_part = relationships_part_for(relationships.source_part());
        let xml = relationships
            .to_xml()
            .map_err(|source| Error::Xml { part: rels_part.clone(), source })?;

        self.set_part(&rels_part, xml.into_bytes());
        // Relationships parts are covered by an extension default in every real
        // package; declaring it is harmless and makes a package we built from
        // nothing valid too.
        self.content_types.set_default("rels", RELATIONSHIPS_CONTENT_TYPE);
        self.rewrite_content_types();
        Ok(())
    }

    /// Finds the main document part.
    ///
    /// The specification's route is to follow the package relationship of type
    /// `officeDocument`. When a package has no such relationship — which does
    /// happen with documents produced by other tools — the content type is used
    /// instead, since that identifies the part just as definitely.
    pub fn main_document_part(&self) -> Result<String, Error> {
        let root = self.relationships("")?;
        if let Some(relationship) = root.single_by_type(OFFICE_DOCUMENT_RELATIONSHIP) {
            if let Some(target) = relationship.resolved_target("") {
                let target = target?;
                if self.part(&target).is_some() {
                    return Ok(target);
                }
                return Err(Error::MissingPart(target));
            }
        }

        for content_type in [
            MAIN_DOCUMENT_CONTENT_TYPE,
            MAIN_DOCUMENT_MACRO_CONTENT_TYPE,
            MAIN_DOCUMENT_TEMPLATE_CONTENT_TYPE,
        ] {
            if let Some(name) = self.content_types.parts_with_type(content_type).next() {
                if self.part(name).is_some() {
                    return Ok(name.to_owned());
                }
            }
        }

        Err(Error::NoMainDocument)
    }

    /// Every part that is content rather than packaging machinery.
    ///
    /// Directory markers, the content types stream and relationships parts are
    /// left out: they describe the package rather than belonging to the document.
    pub fn content_parts(&self) -> impl Iterator<Item = &PackageEntry> {
        self.entries.iter().filter(|entry| {
            !entry.is_directory()
                && !entry.name.eq_ignore_ascii_case(CONTENT_TYPES_PART)
                && !is_relationships_part(&entry.name)
        })
    }

    /// Checks the package against the rules that make it openable.
    ///
    /// Called on its own rather than during [`Self::open`], because a document
    /// with a small inconsistency is usually still worth showing to the user —
    /// refusing to open it would help nobody.
    pub fn validate(&self) -> Vec<Error> {
        let mut problems = Vec::new();

        for entry in self.content_parts() {
            if crate::part_name::validate(&entry.name).is_err() {
                problems.push(Error::InvalidPartName {
                    name: entry.name.clone(),
                    reason: "does not meet the part naming rules",
                });
            }
            if self.content_types.of(&entry.name).is_none() {
                problems.push(Error::MissingPart(entry.name.clone()));
            }
        }

        for (declared, _) in self.content_types.overrides() {
            if self.part(declared).is_none() {
                problems.push(Error::MissingPart(declared.to_owned()));
            }
        }

        problems
    }

    /// Writes the package back out as `.docx` bytes.
    pub fn save(&self) -> Result<Vec<u8>, Error> {
        let mut writer = ZipWriter::new();
        for entry in &self.entries {
            writer.add_with(&entry.name, &entry.data, entry.compression, entry.last_modified)?;
        }
        Ok(writer.finish()?)
    }

    /// Regenerates the content types stream after a change to it.
    fn rewrite_content_types(&mut self) {
        let Ok(xml) = self.content_types.to_xml() else {
            // to_xml only fails on malformed names, which cannot occur here:
            // every name comes from a parsed package or from this crate.
            return;
        };
        let bytes = xml.into_bytes();

        match self
            .entries
            .iter_mut()
            .find(|entry| entry.name.eq_ignore_ascii_case(CONTENT_TYPES_PART))
        {
            Some(entry) => entry.data = bytes,
            // The stream is first in every package Word writes, and putting it
            // first here keeps the archive layout conventional.
            None => self.entries.insert(
                0,
                PackageEntry {
                    name: CONTENT_TYPES_PART.to_owned(),
                    data: bytes,
                    compression: Compression::Deflate,
                    last_modified: DosDateTime::EPOCH,
                },
            ),
        }
    }
}

impl Default for Package {
    fn default() -> Self {
        Self::empty()
    }
}

/// Decodes a part that is supposed to be XML.
fn parse_xml_part(name: &str, bytes: &[u8]) -> Result<String, Error> {
    wp_xml::decode_to_utf8(bytes)
        .map(|text| text.into_owned())
        .map_err(|source| Error::Xml { part: name.to_owned(), source })
}

/// Convenience for building a package from scratch, used by tests and by the
/// "new document" path.
impl Package {
    /// Adds a part and declares its content type in one step.
    pub fn add_part(&mut self, name: &str, content_type: &str, data: Vec<u8>) {
        self.set_part(name, data);
        self.content_types.set_override(name, content_type);
        self.rewrite_content_types();
    }

    /// Adds a part covered by an extension default rather than an override.
    pub fn add_part_with_default_type(
        &mut self,
        name: &str,
        extension: &str,
        content_type: &str,
        data: Vec<u8>,
    ) {
        self.set_part(name, data);
        self.content_types.set_default(extension, content_type);
        self.rewrite_content_types();
    }
}
