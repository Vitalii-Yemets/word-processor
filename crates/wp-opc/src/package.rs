//! The package itself: the parts of a document and how they are saved back.

use std::sync::Arc;

use wp_zip::{Compression, DosDateTime, ZipArchive, ZipWriter};

use crate::content_types::ContentTypes;
use crate::part_name::{is_relationships_part, normalize, relationships_part_for};
use crate::relationships::{Relationship, Relationships, RELATIONSHIPS_CONTENT_TYPE};
use crate::{
    Error, CONTENT_TYPES_PART, MAIN_DOCUMENT_CONTENT_TYPE, MAIN_DOCUMENT_MACRO_CONTENT_TYPE,
    MAIN_DOCUMENT_MACRO_TEMPLATE_CONTENT_TYPE, MAIN_DOCUMENT_TEMPLATE_CONTENT_TYPE,
    OFFICE_DOCUMENT_RELATIONSHIP,
};

/// One entry of the package.
///
/// The compression method and timestamp travel with the data, for an entry
/// that has to be written again. One that has not is not written at all: it
/// is copied from the file as the file stores it. See [`Package::save`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageEntry {
    /// Archive name, without a leading slash.
    pub name: String,
    pub data: Vec<u8>,
    pub compression: Compression,
    pub last_modified: DosDateTime,
    /// Whether the data is still what the file the package was opened from
    /// holds under this name.
    as_opened: bool,
}

impl PackageEntry {
    /// Whether this entry is a directory marker rather than content.
    #[must_use]
    pub fn is_directory(&self) -> bool {
        self.name.ends_with('/')
    }

    /// An entry this program made, which no file holds.
    fn made(name: String, data: Vec<u8>) -> Self {
        Self {
            name,
            data,
            compression: Compression::Deflate,
            last_modified: DosDateTime::EPOCH,
            as_opened: false,
        }
    }

    /// Puts new contents in the entry.
    ///
    /// Contents the same as before leave it as it was opened: a part written
    /// back unchanged — the main document of a file this program wrote,
    /// serialized again — is still the file's, and is copied from it as it
    /// was rather than compressed again.
    fn replace(&mut self, data: Vec<u8>) {
        if data != self.data {
            self.data = data;
            self.as_opened = false;
        }
    }
}

/// The file a package was opened from.
#[derive(Clone)]
struct Original {
    /// Its bytes, shared rather than copied: a package is cloned for every
    /// save, and a copy of the whole file each time would cost more than
    /// the save.
    bytes: Arc<[u8]>,
    /// How many entries it holds, so that one taken out shows.
    entries: usize,
}

impl core::fmt::Debug for Original {
    /// The size and not the bytes, which are the file and say nothing a
    /// person reading this wants.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Original")
            .field("bytes", &self.bytes.len())
            .field("entries", &self.entries)
            .finish()
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
    /// The file the package was opened from, if it was: what a save that
    /// changed nothing gives back, and where an entry nothing changed is
    /// copied from by a save that changed something else.
    original: Option<Original>,
    content_types: ContentTypes,
    /// How many times a part has been written or taken away.
    ///
    /// Not saved: a count of edits, for anyone holding something worked out
    /// from the parts — a layout, say — to tell whether it still holds
    /// without reading them all again. Everything that changes a part goes
    /// through [`Self::set_part`], [`Self::remove_part`] or
    /// [`Self::set_content_types`], so the count is complete.
    generation: u64,
    /// The generation at which each part was last written or taken away.
    ///
    /// So that somebody who knows what they wrote can ask whether anything
    /// else was written since: a count says that something changed, and this
    /// says what. Not saved either.
    written: Vec<(String, u64)>,
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
                as_opened: true,
            });
        }
        let original = Original { bytes: Arc::from(bytes), entries: entries.len() };

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

        Ok(Self {
            entries,
            original: Some(original),
            content_types,
            generation: 0,
            written: Vec::new(),
        })
    }

    /// Builds an empty package with no parts and no declared types.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            entries: Vec::new(),
            original: None,
            content_types: ContentTypes::default(),
            generation: 0,
            written: Vec::new(),
        }
    }

    /// How many times a part has been written or taken away since the
    /// package was opened. Two readings that agree mean no part has changed
    /// between them.
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// The parts written or taken away after a given generation, by name.
    ///
    /// For a caller that writes parts of its own and needs to know whether
    /// anybody else wrote one in between: it reads the generation after its
    /// own writes and asks this later.
    pub fn written_since(&self, generation: u64) -> impl Iterator<Item = &str> {
        self.written.iter().filter(move |(_, at)| *at > generation).map(|(name, _)| name.as_str())
    }

    /// Notes that a part has just been written or taken away, at the present
    /// generation.
    fn stamp(&mut self, name: &str) {
        let at = self.generation;
        match self.written.iter_mut().find(|(known, _)| known.eq_ignore_ascii_case(name)) {
            Some(entry) => entry.1 = at,
            None => self.written.push((name.to_owned(), at)),
        }
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
        self.generation += 1;
        let name = normalize(name);
        self.stamp(&name);
        match self.entries.iter_mut().find(|entry| entry.name.eq_ignore_ascii_case(&name)) {
            Some(entry) => entry.replace(data),
            None => self.entries.push(PackageEntry::made(name, data)),
        }
    }

    /// Removes a part and any override declaring its type.
    pub fn remove_part(&mut self, name: &str) {
        self.generation += 1;
        let name = normalize(name);
        self.stamp(&name);
        self.entries.retain(|entry| !entry.name.eq_ignore_ascii_case(&name));
        self.declare(|types| types.remove_override(&name));
    }

    /// Replaces the content type declarations.
    ///
    /// A write like any other: the declarations are a part of the package,
    /// and a count of writes that missed this one would say a package whose
    /// kind had changed was the package it was.
    pub fn set_content_types(&mut self, content_types: ContentTypes) {
        if content_types == self.content_types {
            return;
        }
        self.generation += 1;
        self.content_types = content_types;
        self.rewrite_content_types();
    }

    /// Changes the declarations, and writes the stream again only if they
    /// did change.
    ///
    /// Adding a part whose type is declared already declares nothing, and
    /// writing the stream again the same would still be a write: a package
    /// that says a part was written when nothing in it changed cannot be
    /// reasoned about, and the stream a producer wrote is kept byte for byte.
    fn declare(&mut self, change: impl FnOnce(&mut ContentTypes)) {
        let before = self.content_types.clone();
        change(&mut self.content_types);
        if self.content_types != before {
            self.rewrite_content_types();
        }
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
        self.declare(|types| types.set_default("rels", RELATIONSHIPS_CONTENT_TYPE));
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
            MAIN_DOCUMENT_MACRO_TEMPLATE_CONTENT_TYPE,
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

    /// Takes out every part no relationship reaches, with the relationships
    /// part of each part taken out.
    ///
    /// Reached means reached from the package's own relationships,
    /// `/_rels/.rels`, through the relationships of every part reached in
    /// turn, internal targets only — which is how a reader finds anything in
    /// a package, so a part not reached that way is one nothing will ever
    /// read. `follows` says whether a relationship still counts; one that
    /// does not is taken out of its part's relationships as well, so that
    /// nothing is left pointing at a part that is gone.
    ///
    /// Nothing at all is taken out when a relationships part cannot be read,
    /// or when the walk does not come to the main document: then what reaches
    /// what is not known, and a part kept for nothing costs less than a part
    /// lost. Says whether anything was taken out.
    pub fn prune_unreachable(
        &mut self,
        follows: impl Fn(&Self, &str, &Relationship) -> bool,
    ) -> bool {
        let mut reached: Vec<String> = Vec::new();
        let mut dropped: Vec<(String, String)> = Vec::new();
        let mut waiting = vec![String::new()];
        while let Some(source) = waiting.pop() {
            let Ok(relationships) = self.relationships(&source) else { return false };
            for relationship in relationships.all() {
                // An address outside, or a target that leads nowhere, is not a
                // part, and is left as it was.
                let Some(Ok(target)) = relationship.resolved_target(&source) else { continue };
                let Some(target) = self.entry_reached_by(&target) else { continue };
                if !follows(self, &source, relationship) {
                    dropped.push((source.clone(), relationship.id.clone()));
                    continue;
                }
                if !reached.iter().any(|held| held.eq_ignore_ascii_case(&target)) {
                    reached.push(target.clone());
                    waiting.push(target);
                }
            }
        }
        let Ok(main) = self.main_document_part() else { return false };
        if !reached.iter().any(|held| held.eq_ignore_ascii_case(&main)) {
            return false;
        }

        let unreached: Vec<String> = self
            .content_parts()
            .map(|entry| entry.name.clone())
            .filter(|name| !reached.iter().any(|held| held.eq_ignore_ascii_case(name)))
            .collect();
        if dropped.is_empty() && unreached.is_empty() {
            return false;
        }

        // The relationships that no longer count, out of the parts that stay.
        let mut sources: Vec<&str> = dropped.iter().map(|(source, _)| source.as_str()).collect();
        sources.dedup();
        let sources: Vec<String> = sources.into_iter().map(str::to_owned).collect();
        for source in &sources {
            let Ok(mut relationships) = self.relationships(source) else { continue };
            for (_, id) in dropped.iter().filter(|(held, _)| held == source) {
                relationships.remove(id);
            }
            if self.set_relationships(&relationships).is_err() {
                // Not written back, it still points at its parts, so they
                // stay where they are.
                return true;
            }
        }
        for name in &unreached {
            self.remove_part(name);
            let own = relationships_part_for(name);
            if self.part(&own).is_some() {
                self.remove_part(&own);
            }
        }
        true
    }

    /// The name the archive holds a part under, for a target resolved to it.
    ///
    /// A target is a URI, and a character a URI cannot carry is written in
    /// it percent-encoded — `image%201.png` for `image 1.png` — while a
    /// producer may name the entry either way. The two spellings are one
    /// part, and a walk that told them apart would take out a part that is
    /// reached.
    fn entry_reached_by(&self, target: &str) -> Option<String> {
        let content = |entry: &&PackageEntry| !entry.is_directory();
        if let Some(entry) = self
            .entries
            .iter()
            .filter(content)
            .find(|entry| entry.name.eq_ignore_ascii_case(target))
        {
            return Some(entry.name.clone());
        }
        let target = percent_decoded(target);
        self.entries
            .iter()
            .filter(content)
            .find(|entry| percent_decoded(&entry.name).eq_ignore_ascii_case(&target))
            .map(|entry| entry.name.clone())
    }

    /// Writes the package back out as `.docx` bytes.
    ///
    /// # A package nothing changed is the file it was opened from
    ///
    /// Not a file with the same parts in it: the same file. A zip is more
    /// than its parts — an order, a compression of each part out of the
    /// many a compressor may choose, a timestamp, extra fields, a comment —
    /// and writing the parts again gives this program's choice of all of
    /// those, which is never quite the choice of whoever wrote the file, so
    /// that a file from another program came back a different size with
    /// nothing in it touched. So while every entry is the one opened, none
    /// taken out and none put in, the bytes opened are the bytes saved.
    ///
    /// # A package something changed keeps what nothing did
    ///
    /// Every entry still the one opened is copied from the file as the file
    /// stores it — local header, compressed bytes, data descriptor and its
    /// header in the central directory, with only where it starts changed —
    /// and only the entries written since are compressed here. The entries
    /// stay in the order the file had them, with what was added after them
    /// in the order it was added, and the file's comment is kept; the
    /// central directory and its end are this program's, as they have to be
    /// once an entry has moved. An entry whose records cannot be found whole
    /// is written again from its contents, which is how every entry was
    /// written before, and the one thing it costs is the producer's bytes.
    pub fn save(&self) -> Result<Vec<u8>, Error> {
        let untouched = |original: &Original| {
            self.entries.len() == original.entries
                && self.entries.iter().all(|entry| entry.as_opened)
        };
        if let Some(original) = self.original.as_ref().filter(|original| untouched(original)) {
            return Ok(original.bytes.to_vec());
        }

        // It opened once, so it opens again; and if it somehow did not, every
        // entry is written from its contents, as one that was never opened is.
        let archive =
            self.original.as_ref().and_then(|original| ZipArchive::open(&original.bytes).ok());
        let mut writer = ZipWriter::new();
        if let Some(archive) = &archive {
            writer.set_comment(archive.comment())?;
        }
        for entry in &self.entries {
            let copied = archive.as_ref().filter(|_| entry.as_opened).is_some_and(|archive| {
                archive
                    .entry(&entry.name)
                    .is_some_and(|stored| writer.copy_from(archive, stored).is_ok())
            });
            if !copied {
                writer.add_with(
                    &entry.name,
                    &entry.data,
                    entry.compression,
                    entry.last_modified,
                )?;
            }
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
        // Every caller counted the change it made; the stream is written as
        // part of that change, at the same generation.
        self.stamp(CONTENT_TYPES_PART);

        match self
            .entries
            .iter_mut()
            .find(|entry| entry.name.eq_ignore_ascii_case(CONTENT_TYPES_PART))
        {
            Some(entry) => entry.replace(bytes),
            // The stream is first in every package Word writes, and putting it
            // first here keeps the archive layout conventional.
            None => {
                self.entries.insert(0, PackageEntry::made(CONTENT_TYPES_PART.to_owned(), bytes));
            }
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

/// A part name with every `%` and two hex digits made the byte they stand
/// for. A name whose bytes are then not UTF-8 is left as it was: it was not
/// written by encoding a name, and it is compared as written.
fn percent_decoded(name: &str) -> String {
    let hex = |byte: Option<&u8>| byte.and_then(|byte| char::from(*byte).to_digit(16));
    let bytes = name.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' {
            if let (Some(high), Some(low)) = (hex(bytes.get(at + 1)), hex(bytes.get(at + 2))) {
                // Two hex digits make at most 255, so the byte is whole.
                decoded.push((high * 16 + low) as u8);
                at += 3;
                continue;
            }
        }
        decoded.push(bytes[at]);
        at += 1;
    }
    String::from_utf8(decoded).unwrap_or_else(|_| name.to_owned())
}

/// Convenience for building a package from scratch, used by tests and by the
/// "new document" path.
impl Package {
    /// Adds a part and declares its content type in one step.
    pub fn add_part(&mut self, name: &str, content_type: &str, data: Vec<u8>) {
        self.set_part(name, data);
        self.declare(|types| types.set_override(name, content_type));
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
        self.declare(|types| types.set_default(extension, content_type));
    }
}
