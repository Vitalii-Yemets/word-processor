//! The `[Content_Types].xml` stream.
//!
//! Every part of a package has a content type, and this stream is where they are
//! declared. There are two ways to declare one: a `Default` covers every part
//! with a given file extension, and an `Override` names a single part. An
//! override wins.
//!
//! Nothing infers a type from a file extension on its own. A part with no
//! declared type is an invalid package, not a part of unknown type — that is why
//! the lookup returns `Option` rather than a guess.

use wp_xml::{Event, Reader, Writer};

use crate::part_name::{extension, normalize};

/// Namespace of the content types stream.
pub const CONTENT_TYPES_NAMESPACE: &str =
    "http://schemas.openxmlformats.org/package/2006/content-types";

/// The content type declarations of a package.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ContentTypes {
    /// Extension (lowercased) to content type.
    defaults: Vec<(String, String)>,
    /// Part name (archive form) to content type.
    overrides: Vec<(String, String)>,
}

impl ContentTypes {
    /// Reads the stream.
    pub fn parse(xml: &str) -> Result<Self, wp_xml::Error> {
        let mut types = Self::default();
        let mut reader = Reader::new(xml);

        while let Some(event) = reader.next_event() {
            let tag = match event? {
                Event::Start(tag) | Event::Empty(tag) => tag,
                _ => continue,
            };

            // Match on the local name and namespace rather than the prefix: the
            // stream is usually written with a default namespace, but nothing
            // stops a producer from using a prefix instead.
            if tag.namespace != Some(CONTENT_TYPES_NAMESPACE) {
                continue;
            }

            match tag.name.local {
                "Default" => {
                    if let (Some(ext), Some(content_type)) =
                        (tag.attribute(None, "Extension"), tag.attribute(None, "ContentType"))
                    {
                        types.set_default(ext, content_type);
                    }
                }
                "Override" => {
                    if let (Some(part), Some(content_type)) =
                        (tag.attribute(None, "PartName"), tag.attribute(None, "ContentType"))
                    {
                        types.set_override(part, content_type);
                    }
                }
                _ => {}
            }
        }

        Ok(types)
    }

    /// The content type of a part, if the package declares one.
    #[must_use]
    pub fn of(&self, part: &str) -> Option<&str> {
        let part = normalize(part);

        // Part names compare without regard to ASCII case, so an override
        // written as /word/Document.xml still covers word/document.xml.
        if let Some((_, content_type)) =
            self.overrides.iter().find(|(name, _)| name.eq_ignore_ascii_case(&part))
        {
            return Some(content_type);
        }

        let extension = extension(&part)?;
        self.defaults
            .iter()
            .find(|(known, _)| *known == extension)
            .map(|(_, content_type)| content_type.as_str())
    }

    /// Declares the type of every part with a given extension.
    pub fn set_default(&mut self, extension: &str, content_type: &str) {
        let extension = extension.to_ascii_lowercase();
        match self.defaults.iter_mut().find(|(known, _)| *known == extension) {
            Some((_, existing)) => *existing = content_type.to_owned(),
            None => self.defaults.push((extension, content_type.to_owned())),
        }
    }

    /// Declares the type of one specific part.
    pub fn set_override(&mut self, part: &str, content_type: &str) {
        let part = normalize(part);
        match self.overrides.iter_mut().find(|(name, _)| name.eq_ignore_ascii_case(&part)) {
            Some((_, existing)) => *existing = content_type.to_owned(),
            None => self.overrides.push((part, content_type.to_owned())),
        }
    }

    /// Removes any override for a part.
    pub fn remove_override(&mut self, part: &str) {
        let part = normalize(part);
        self.overrides.retain(|(name, _)| !name.eq_ignore_ascii_case(&part));
    }

    /// Every extension default, as declared.
    pub fn defaults(&self) -> impl Iterator<Item = (&str, &str)> {
        self.defaults.iter().map(|(a, b)| (a.as_str(), b.as_str()))
    }

    /// Every part-specific override, as declared.
    pub fn overrides(&self) -> impl Iterator<Item = (&str, &str)> {
        self.overrides.iter().map(|(a, b)| (a.as_str(), b.as_str()))
    }

    /// Finds the parts declared to have a given content type.
    pub fn parts_with_type<'a>(&'a self, content_type: &'a str) -> impl Iterator<Item = &'a str> {
        self.overrides
            .iter()
            .filter(move |(_, declared)| declared == content_type)
            .map(|(name, _)| name.as_str())
    }

    /// Writes the stream back out.
    pub fn to_xml(&self) -> Result<String, wp_xml::Error> {
        let mut writer = Writer::with_capacity(512);
        writer.write_declaration(Some(true));
        writer.write_start("Types", &[("xmlns", CONTENT_TYPES_NAMESPACE)])?;

        for (extension, content_type) in &self.defaults {
            writer.write_empty(
                "Default",
                &[("Extension", extension.as_str()), ("ContentType", content_type.as_str())],
            )?;
        }
        for (part, content_type) in &self.overrides {
            // Overrides name parts in the specification's form, with a leading
            // slash, which is not how archive entries are named.
            let part_name = format!("/{part}");
            writer.write_empty(
                "Override",
                &[("PartName", part_name.as_str()), ("ContentType", content_type.as_str())],
            )?;
        }

        writer.write_end("Types")?;
        writer.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Default Extension="png" ContentType="image/png"/>
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
  <Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/>
</Types>"#;

    #[test]
    fn an_override_wins_over_a_default() {
        let types = ContentTypes::parse(SAMPLE).unwrap();
        // Both an "xml" default and an override apply; the override is the answer.
        assert_eq!(types.of("word/document.xml"), Some(crate::MAIN_DOCUMENT_CONTENT_TYPE));
        assert_eq!(types.of("word/settings.xml"), Some("application/xml"));
    }

    #[test]
    fn accepts_both_forms_of_a_part_name() {
        let types = ContentTypes::parse(SAMPLE).unwrap();
        assert_eq!(types.of("/word/styles.xml"), types.of("word/styles.xml"));
        assert!(types.of("word/styles.xml").is_some());
    }

    #[test]
    fn part_names_and_extensions_ignore_ascii_case() {
        let types = ContentTypes::parse(SAMPLE).unwrap();
        assert_eq!(types.of("word/Document.xml"), Some(crate::MAIN_DOCUMENT_CONTENT_TYPE));
        assert_eq!(types.of("word/media/IMAGE1.PNG"), Some("image/png"));
    }

    #[test]
    fn an_undeclared_part_has_no_type_rather_than_a_guessed_one() {
        let types = ContentTypes::parse(SAMPLE).unwrap();
        assert_eq!(types.of("word/media/movie.mp4"), None);
        assert_eq!(types.of("word/noextension"), None);
    }

    #[test]
    fn survives_a_round_trip() {
        let original = ContentTypes::parse(SAMPLE).unwrap();
        let rewritten = ContentTypes::parse(&original.to_xml().unwrap()).unwrap();
        assert_eq!(original, rewritten);
    }

    #[test]
    fn finds_parts_by_content_type() {
        let types = ContentTypes::parse(SAMPLE).unwrap();
        let found: Vec<&str> = types.parts_with_type(crate::MAIN_DOCUMENT_CONTENT_TYPE).collect();
        assert_eq!(found, ["word/document.xml"]);
    }

    #[test]
    fn reads_the_stream_when_it_uses_a_prefix_instead_of_a_default_namespace() {
        // Equally valid XML, and some producers write it this way.
        let prefixed = r#"<ct:Types xmlns:ct="http://schemas.openxmlformats.org/package/2006/content-types">
            <ct:Default Extension="xml" ContentType="application/xml"/>
        </ct:Types>"#;
        let types = ContentTypes::parse(prefixed).unwrap();
        assert_eq!(types.of("word/settings.xml"), Some("application/xml"));
    }
}
