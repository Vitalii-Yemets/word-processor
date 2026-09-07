//! Relationship parts.
//!
//! Parts of a package do not reference each other by file path. A part refers to
//! a relationship identifier, and the relationship — declared in a `.rels` part
//! beside the source — says what that identifier points at. This indirection is
//! what lets a document be rearranged without rewriting its contents, and it is
//! why finding the main document means following a relationship rather than
//! looking for a known filename.

use wp_xml::{Event, Reader, Writer};

use crate::part_name::resolve_target;
use crate::Error;

/// Namespace of a relationships part.
pub const RELATIONSHIPS_NAMESPACE: &str =
    "http://schemas.openxmlformats.org/package/2006/relationships";

/// Content type of a relationships part.
pub const RELATIONSHIPS_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-package.relationships+xml";

/// Whether a relationship points inside the package or out to the world.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TargetMode {
    /// Another part of this package.
    #[default]
    Internal,
    /// Something outside it — a hyperlink, or a linked rather than embedded image.
    External,
}

/// One relationship.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Relationship {
    /// Identifier used to refer to this relationship from the source part.
    pub id: String,
    /// The relationship type, a URI saying what the target *is*.
    pub kind: String,
    /// The target exactly as written, before resolution.
    pub target: String,
    pub mode: TargetMode,
}

impl Relationship {
    /// The part this relationship points at, resolved against its source.
    ///
    /// Returns `None` for an external target, which is not a part of the package
    /// and must never be treated as one.
    pub fn resolved_target(&self, source_part: &str) -> Option<Result<String, Error>> {
        match self.mode {
            TargetMode::Internal => Some(resolve_target(source_part, &self.target)),
            TargetMode::External => None,
        }
    }
}

/// The relationships declared by one part.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Relationships {
    /// The part these belong to, in archive form. Empty for the package itself.
    source_part: String,
    entries: Vec<Relationship>,
}

impl Relationships {
    /// An empty set belonging to the given part.
    #[must_use]
    pub fn new(source_part: &str) -> Self {
        Self { source_part: source_part.to_owned(), entries: Vec::new() }
    }

    /// Reads a `.rels` part.
    pub fn parse(source_part: &str, xml: &str) -> Result<Self, Error> {
        let mut relationships = Self::new(source_part);
        let mut reader = Reader::new(xml);

        while let Some(event) = reader.next_event() {
            let event = event.map_err(|source| Error::Xml {
                part: source_part.to_owned(),
                source,
            })?;
            let tag = match event {
                Event::Start(tag) | Event::Empty(tag) => tag,
                _ => continue,
            };
            if tag.namespace != Some(RELATIONSHIPS_NAMESPACE) || tag.name.local != "Relationship" {
                continue;
            }

            let id = tag.attribute(None, "Id").ok_or(Error::MissingAttribute {
                element: "Relationship",
                attribute: "Id",
            })?;
            let kind = tag.attribute(None, "Type").ok_or(Error::MissingAttribute {
                element: "Relationship",
                attribute: "Type",
            })?;
            let target = tag.attribute(None, "Target").ok_or(Error::MissingAttribute {
                element: "Relationship",
                attribute: "Target",
            })?;
            let mode = match tag.attribute(None, "TargetMode") {
                Some("External") => TargetMode::External,
                _ => TargetMode::Internal,
            };

            // Identifiers are how the document body addresses these; two with
            // the same name would make a reference ambiguous.
            if relationships.entries.iter().any(|existing| existing.id == id) {
                return Err(Error::DuplicateRelationshipId {
                    part: source_part.to_owned(),
                    id: id.to_owned(),
                });
            }

            relationships.entries.push(Relationship {
                id: id.to_owned(),
                kind: kind.to_owned(),
                target: target.to_owned(),
                mode,
            });
        }

        Ok(relationships)
    }

    /// The part these relationships belong to.
    #[must_use]
    pub fn source_part(&self) -> &str {
        &self.source_part
    }

    /// Every relationship, in the order declared.
    #[must_use]
    pub fn all(&self) -> &[Relationship] {
        &self.entries
    }

    /// Looks up a relationship by identifier.
    #[must_use]
    pub fn by_id(&self, id: &str) -> Option<&Relationship> {
        self.entries.iter().find(|relationship| relationship.id == id)
    }

    /// Every relationship of a given type.
    pub fn by_type<'a>(&'a self, kind: &'a str) -> impl Iterator<Item = &'a Relationship> {
        self.entries.iter().filter(move |relationship| relationship.kind == kind)
    }

    /// The single relationship of a given type, if there is exactly one.
    #[must_use]
    pub fn single_by_type(&self, kind: &str) -> Option<&Relationship> {
        // Filtered here rather than through `by_type` so that the returned
        // reference is tied to the relationships, not to the borrowed type name.
        let mut matches = self.entries.iter().filter(|relationship| relationship.kind == kind);
        let first = matches.next()?;
        matches.next().is_none().then_some(first)
    }

    /// Adds a relationship, giving it an identifier that is not yet in use.
    pub fn add(&mut self, kind: &str, target: &str, mode: TargetMode) -> &Relationship {
        let mut number = self.entries.len() + 1;
        let id = loop {
            let candidate = format!("rId{number}");
            if !self.entries.iter().any(|existing| existing.id == candidate) {
                break candidate;
            }
            number += 1;
        };

        self.entries.push(Relationship {
            id,
            kind: kind.to_owned(),
            target: target.to_owned(),
            mode,
        });
        self.entries.last().expect("just pushed")
    }

    /// Writes the part back out.
    pub fn to_xml(&self) -> Result<String, wp_xml::Error> {
        let mut writer = Writer::with_capacity(256 + self.entries.len() * 128);
        writer.write_declaration(Some(true));
        writer.write_start("Relationships", &[("xmlns", RELATIONSHIPS_NAMESPACE)])?;

        for relationship in &self.entries {
            let mut attributes = vec![
                ("Id", relationship.id.as_str()),
                ("Type", relationship.kind.as_str()),
                ("Target", relationship.target.as_str()),
            ];
            // The attribute is written only for external targets; internal is
            // the default and Word omits it.
            if relationship.mode == TargetMode::External {
                attributes.push(("TargetMode", "External"));
            }
            writer.write_empty("Relationship", &attributes)?;
        }

        writer.write_end("Relationships")?;
        writer.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
  <Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/>
</Relationships>"#;

    const DOCUMENT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>
  <Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/image1.png"/>
  <Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.org/" TargetMode="External"/>
</Relationships>"#;

    #[test]
    fn reads_the_package_relationships() {
        let relationships = Relationships::parse("", ROOT_RELS).unwrap();
        assert_eq!(relationships.all().len(), 2);

        let main = relationships.single_by_type(crate::OFFICE_DOCUMENT_RELATIONSHIP).unwrap();
        assert_eq!(main.id, "rId1");
        assert_eq!(main.resolved_target("").unwrap().unwrap(), "word/document.xml");
    }

    #[test]
    fn resolves_targets_relative_to_the_source_part() {
        let relationships = Relationships::parse("word/document.xml", DOCUMENT_RELS).unwrap();

        let styles = relationships.by_id("rId1").unwrap();
        assert_eq!(styles.resolved_target("word/document.xml").unwrap().unwrap(), "word/styles.xml");

        let image = relationships.by_id("rId2").unwrap();
        assert_eq!(
            image.resolved_target("word/document.xml").unwrap().unwrap(),
            "word/media/image1.png"
        );
    }

    #[test]
    fn an_external_target_is_not_a_part() {
        let relationships = Relationships::parse("word/document.xml", DOCUMENT_RELS).unwrap();
        let hyperlink = relationships.by_id("rId3").unwrap();

        assert_eq!(hyperlink.mode, TargetMode::External);
        // Treating a hyperlink as a part name is exactly the confusion that
        // would let a document reach outside the package.
        assert!(hyperlink.resolved_target("word/document.xml").is_none());
    }

    #[test]
    fn rejects_duplicate_identifiers() {
        let duplicated = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
            <Relationship Id="rId1" Type="urn:a" Target="a.xml"/>
            <Relationship Id="rId1" Type="urn:b" Target="b.xml"/>
        </Relationships>"#;

        assert!(matches!(
            Relationships::parse("", duplicated),
            Err(Error::DuplicateRelationshipId { .. })
        ));
    }

    #[test]
    fn added_relationships_get_unused_identifiers() {
        let mut relationships = Relationships::parse("word/document.xml", DOCUMENT_RELS).unwrap();
        let added = relationships.add("urn:test", "extra.xml", TargetMode::Internal).clone();

        assert!(!["rId1", "rId2", "rId3"].contains(&added.id.as_str()));
        assert_eq!(relationships.by_id(&added.id), Some(&added));
    }

    #[test]
    fn survives_a_round_trip() {
        for (source, xml) in [("", ROOT_RELS), ("word/document.xml", DOCUMENT_RELS)] {
            let original = Relationships::parse(source, xml).unwrap();
            let rewritten =
                Relationships::parse(source, &original.to_xml().unwrap()).unwrap();
            assert_eq!(original, rewritten, "source {source:?}");
        }
    }
}
