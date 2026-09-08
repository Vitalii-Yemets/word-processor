//! What the document says about itself: its title, who wrote it, and when.
//!
//! # Two parts, not one
//!
//! `docProps/core.xml` holds the properties the packaging standard defines —
//! title, author, keywords, the dates — in the Dublin Core vocabulary, which is
//! why its elements are `dc:` and not anything Microsoft invented.
//! `docProps/app.xml` holds the ones Word adds on top: the company, the program
//! that wrote the file, and a set of counts Word keeps up to date and every
//! other program ignores.
//!
//! Both are pointed at from the package's own relationships rather than from
//! the document, because they are properties of the *file*, not of the text in
//! it — which is how a search index reads a title without opening the document.
//!
//! # Why the counts are not written
//!
//! `app.xml` can say how many words and pages a document has. Word writes them
//! and then does not trust them: it counts again on opening, because the number
//! is stale the moment anybody types. Writing a number that is wrong is worse
//! than writing none, so this writes none.

use wp_opc::TargetMode;
use wp_xml::tree::{Element, XmlTree};

use crate::{Document, Error};

/// The name this program writes into `app.xml` as the one that saved the file.
///
/// Word writes "Microsoft Office Word" here. Writing that would be a lie about
/// what wrote the file, and the field exists to say what did.
pub const APPLICATION: &str = "Word Processor";

/// Where the core properties live, by convention and in every file Word writes.
const CORE_PART: &str = "docProps/core.xml";
/// And the extended ones.
const APP_PART: &str = "docProps/app.xml";

const CORE_CONTENT_TYPE: &str = "application/vnd.openxmlformats-package.core-properties+xml";
const APP_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.extended-properties+xml";

const CORE_RELATIONSHIP: &str =
    "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties";
const APP_RELATIONSHIP: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties";

/// The namespaces the two parts are written in.
const CP: &str = "http://schemas.openxmlformats.org/package/2006/metadata/core-properties";
const DC: &str = "http://purl.org/dc/elements/1.1/";
const DCTERMS: &str = "http://purl.org/dc/terms/";
const XSI: &str = "http://www.w3.org/2001/XMLSchema-instance";
const EXTENDED: &str = "http://schemas.openxmlformats.org/officeDocument/2006/extended-properties";

/// What the document says about itself.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Properties {
    pub title: String,
    pub subject: String,
    /// Who wrote it. `dc:creator` — several authors are separated by `; `.
    pub author: String,
    /// Words to find it by, separated however the person separated them.
    pub keywords: String,
    /// A sentence about what it is. Word calls this the comments.
    pub description: String,
    /// Who saved it last.
    pub last_modified_by: String,
    pub category: String,
    /// The company it was written at, which lives in the other part.
    pub company: String,
    /// When it was made and when it was last saved, as `2026-09-08T11:07:00Z`.
    pub created: String,
    pub modified: String,
}

impl Properties {
    /// Whether the document says nothing about itself at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// What to call the document when it has no title of its own.
    ///
    /// The file's name, which is what Word falls back to and what a person
    /// recognises — a document called "Untitled" is one nobody can find again.
    #[must_use]
    pub fn display_title(&self, file_name: &str) -> String {
        if self.title.trim().is_empty() {
            return file_name.to_owned();
        }
        self.title.clone()
    }
}

impl Document {
    /// What the document says about itself.
    #[must_use]
    pub fn properties(&self) -> Properties {
        let mut properties = Properties::default();

        if let Some(root) = self.part_root(CORE_PART) {
            let text = |namespace: &str, local: &str| {
                root.child(Some(namespace), local)
                    .map(|child| child.text_content())
                    .unwrap_or_default()
            };
            properties.title = text(DC, "title");
            properties.subject = text(DC, "subject");
            properties.author = text(DC, "creator");
            properties.description = text(DC, "description");
            properties.keywords = text(CP, "keywords");
            properties.last_modified_by = text(CP, "lastModifiedBy");
            properties.category = text(CP, "category");
            properties.created = text(DCTERMS, "created");
            properties.modified = text(DCTERMS, "modified");
        }

        if let Some(root) = self.part_root(APP_PART) {
            properties.company = root
                .child_elements()
                .find(|child| child.local_name() == "Company")
                .map(Element::text_content)
                .unwrap_or_default();
        }
        properties
    }

    /// Writes what the document says about itself.
    ///
    /// Returns whether anything changed, so a dialog that was opened and closed
    /// does not mark the document as edited.
    pub fn set_properties(&mut self, wanted: &Properties) -> Result<bool, Error> {
        if self.properties() == *wanted {
            return Ok(false);
        }

        self.write_core(wanted)?;
        self.write_app(wanted)?;
        self.mark_modified();
        Ok(true)
    }

    /// The parsed root of a part, if the package has one.
    fn part_root(&self, name: &str) -> Option<Element> {
        let text = self.package().xml_part(name)?.ok()?;
        XmlTree::parse(&text).ok().map(|tree| tree.root)
    }

    /// Writes `docProps/core.xml`.
    fn write_core(&mut self, wanted: &Properties) -> Result<(), Error> {
        let mut root = Element::new("cp:coreProperties", Some(CP));
        for (prefix, uri) in [
            ("cp", CP),
            ("dc", DC),
            ("dcterms", DCTERMS),
            ("dcmitype", "http://purl.org/dc/dcmitype/"),
            ("xsi", XSI),
        ] {
            root.declarations.push((Some(prefix.to_owned()), uri.to_owned()));
        }

        // In the order the schema lists them, which is the order Word writes.
        put(&mut root, "dc:title", DC, &wanted.title);
        put(&mut root, "dc:subject", DC, &wanted.subject);
        put(&mut root, "dc:creator", DC, &wanted.author);
        put(&mut root, "cp:keywords", CP, &wanted.keywords);
        put(&mut root, "dc:description", DC, &wanted.description);
        put(&mut root, "cp:lastModifiedBy", CP, &wanted.last_modified_by);

        // The two dates carry a type attribute saying which of the many ways of
        // writing a date this is. Word writes it, and reads a date without it
        // as no date at all.
        for (name, value) in
            [("dcterms:created", &wanted.created), ("dcterms:modified", &wanted.modified)]
        {
            if value.trim().is_empty() {
                continue;
            }
            let mut element = Element::new(name, Some(DCTERMS));
            element.set_namespaced_attribute("xsi:type", XSI, "dcterms:W3CDTF");
            element.set_text(value);
            root.push_element(element);
        }

        put(&mut root, "cp:category", CP, &wanted.category);
        self.save_part(CORE_PART, CORE_CONTENT_TYPE, CORE_RELATIONSHIP, root)
    }

    /// Writes `docProps/app.xml`.
    fn write_app(&mut self, wanted: &Properties) -> Result<(), Error> {
        let mut root = Element::new("Properties", Some(EXTENDED));
        root.declarations.push((None, EXTENDED.to_owned()));

        let mut application = Element::new("Application", Some(EXTENDED));
        application.set_text(APPLICATION);
        root.push_element(application);
        put(&mut root, "Company", EXTENDED, &wanted.company);

        self.save_part(APP_PART, APP_CONTENT_TYPE, APP_RELATIONSHIP, root)
    }

    /// Writes one of the two parts and points the package at it.
    fn save_part(
        &mut self,
        name: &str,
        content_type: &str,
        relationship: &str,
        root: Element,
    ) -> Result<(), Error> {
        let tree = XmlTree {
            standalone: Some(true),
            has_declaration: true,
            doctype: None,
            before_root: Vec::new(),
            root,
            after_root: Vec::new(),
        };
        let xml = tree.to_xml().map_err(|source| Error::Xml { part: name.to_owned(), source })?;
        self.package_mut().add_part(name, content_type, xml.into_bytes());

        // The properties belong to the package, so the relationship is the
        // package's own — not the document's.
        let mut relationships =
            self.package().relationships("").unwrap_or_else(|_| wp_opc::Relationships::new(""));
        if relationships.single_by_type(relationship).is_none() {
            relationships.add(relationship, name, TargetMode::Internal);
            self.package_mut().set_relationships(&relationships)?;
        }
        Ok(())
    }
}

/// Adds an element holding some text, unless there is no text to hold.
fn put(parent: &mut Element, name: &str, namespace: &str, value: &str) {
    if value.trim().is_empty() {
        return;
    }
    let mut element = Element::new(name, Some(namespace));
    element.set_text(value);
    parent.push_element(element);
}

#[cfg(test)]
mod tests {
    use super::Properties;

    #[test]
    fn a_document_that_says_nothing_says_nothing() {
        assert!(Properties::default().is_empty());
    }

    #[test]
    fn a_title_makes_it_no_longer_empty() {
        let properties = Properties { title: "A Report".to_owned(), ..Properties::default() };
        assert!(!properties.is_empty());
    }

    #[test]
    fn the_file_name_stands_in_for_a_missing_title() {
        assert_eq!(Properties::default().display_title("notes.docx"), "notes.docx");
    }

    #[test]
    fn a_title_it_has_is_used_instead() {
        let properties = Properties { title: "A Report".to_owned(), ..Properties::default() };
        assert_eq!(properties.display_title("notes.docx"), "A Report");
    }

    #[test]
    fn a_title_of_nothing_but_spaces_is_no_title() {
        let properties = Properties { title: "   ".to_owned(), ..Properties::default() };
        assert_eq!(properties.display_title("notes.docx"), "notes.docx");
    }
}
