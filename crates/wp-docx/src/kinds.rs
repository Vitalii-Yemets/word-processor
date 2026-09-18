//! The four kinds of Word file, and turning one into another.
//!
//! # What tells them apart
//!
//! `.docx`, `.docm`, `.dotx` and `.dotm` are the same package with the same
//! parts. What differs is one line of `[Content_Types].xml`: the content type
//! of the main document part, which says whether the file is a document or a
//! template and whether it may hold macros. A macro-enabled file carries the
//! macros as a part of its own, `word/vbaProject.bin`, reached through a
//! relationship from the main part; the other two kinds cannot carry it, and
//! Word refuses to write it into them.
//!
//! # What a template is for
//!
//! A document is made from it. Word's shell verb on a template is New, not
//! Open: double-clicking `Letter.dotx` gives an untitled document with
//! everything the template held, and the template stays as it was. The new
//! document remembers where it came from — `w:attachedTemplate` in the
//! settings, pointing at the file — which is what lets styles be updated from
//! it later. File ▸ Open on a template opens the template itself, for editing
//! it.

use wp_opc::{TargetMode, MAIN_DOCUMENT_MACRO_TEMPLATE_CONTENT_TYPE};
use wp_xml::tree::Element;

use crate::{edit, read, settings, Document, Error};

/// Relationship type of the macros, from the main part.
const VBA_PROJECT: &str = "http://schemas.microsoft.com/office/2006/relationships/vbaProject";
/// And of the part beside them that says which macros to run when.
const VBA_DATA: &str = "http://schemas.microsoft.com/office/2006/relationships/wordVbaData";
/// Relationship type of the template a document was made from, from the
/// settings part.
const ATTACHED_TEMPLATE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/attachedTemplate";

/// Which of the four a file is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `.docx`: a document that cannot hold macros.
    Document,
    /// `.docm`: a document that can.
    MacroEnabledDocument,
    /// `.dotx`: a template that cannot hold macros.
    Template,
    /// `.dotm`: a template that can.
    MacroEnabledTemplate,
}

impl Kind {
    /// All four, in the order Word's Save As offers them.
    pub const ALL: [Self; 4] =
        [Self::Document, Self::MacroEnabledDocument, Self::Template, Self::MacroEnabledTemplate];

    /// The kind a main part's content type says, if it says one.
    #[must_use]
    pub fn of_content_type(content_type: &str) -> Option<Self> {
        match content_type {
            wp_opc::MAIN_DOCUMENT_CONTENT_TYPE => Some(Self::Document),
            wp_opc::MAIN_DOCUMENT_MACRO_CONTENT_TYPE => Some(Self::MacroEnabledDocument),
            wp_opc::MAIN_DOCUMENT_TEMPLATE_CONTENT_TYPE => Some(Self::Template),
            MAIN_DOCUMENT_MACRO_TEMPLATE_CONTENT_TYPE => Some(Self::MacroEnabledTemplate),
            _ => None,
        }
    }

    /// The content type of the main part of a file of this kind.
    #[must_use]
    pub fn content_type(self) -> &'static str {
        match self {
            Self::Document => wp_opc::MAIN_DOCUMENT_CONTENT_TYPE,
            Self::MacroEnabledDocument => wp_opc::MAIN_DOCUMENT_MACRO_CONTENT_TYPE,
            Self::Template => wp_opc::MAIN_DOCUMENT_TEMPLATE_CONTENT_TYPE,
            Self::MacroEnabledTemplate => MAIN_DOCUMENT_MACRO_TEMPLATE_CONTENT_TYPE,
        }
    }

    /// The kind a file name's extension says, if it says one. Case does not
    /// matter, because Windows does not care about it either.
    #[must_use]
    pub fn of_extension(extension: &str) -> Option<Self> {
        match extension.to_ascii_lowercase().as_str() {
            "docx" => Some(Self::Document),
            "docm" => Some(Self::MacroEnabledDocument),
            "dotx" => Some(Self::Template),
            "dotm" => Some(Self::MacroEnabledTemplate),
            _ => None,
        }
    }

    /// The extension a file of this kind is given.
    #[must_use]
    pub fn extension(self) -> &'static str {
        match self {
            Self::Document => "docx",
            Self::MacroEnabledDocument => "docm",
            Self::Template => "dotx",
            Self::MacroEnabledTemplate => "dotm",
        }
    }

    /// What Word calls it in the Save As list.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Document => "Word Document",
            Self::MacroEnabledDocument => "Word Macro-Enabled Document",
            Self::Template => "Word Template",
            Self::MacroEnabledTemplate => "Word Macro-Enabled Template",
        }
    }

    /// Whether it is a template: something documents are made from.
    #[must_use]
    pub fn is_template(self) -> bool {
        matches!(self, Self::Template | Self::MacroEnabledTemplate)
    }

    /// Whether it may hold macros.
    #[must_use]
    pub fn allows_macros(self) -> bool {
        matches!(self, Self::MacroEnabledDocument | Self::MacroEnabledTemplate)
    }
}

impl Document {
    /// Which of the four the document is, by its main part's content type.
    ///
    /// A document whose main part is typed in some other way — a package
    /// written by a tool that got the type wrong — is taken for a plain
    /// document, which is what Word takes it for.
    #[must_use]
    pub fn kind(&self) -> Kind {
        self.package()
            .content_type(&self.document_part)
            .and_then(Kind::of_content_type)
            .unwrap_or(Kind::Document)
    }

    /// Whether the document carries macros.
    #[must_use]
    pub fn has_macros(&self) -> bool {
        self.macro_parts().iter().any(|(_, part)| self.package().part(part).is_some())
    }

    /// The macro parts and the relationships that reach them.
    fn macro_parts(&self) -> Vec<(String, String)> {
        let Ok(relationships) = self.package().relationships(&self.document_part) else {
            return Vec::new();
        };
        let mut found = Vec::new();
        for kind in [VBA_PROJECT, VBA_DATA] {
            for relationship in relationships.by_type(kind) {
                if let Some(Ok(target)) = relationship.resolved_target(&self.document_part) {
                    found.push((relationship.id.clone(), target));
                }
            }
        }
        found
    }

    /// Makes the document one of the four.
    ///
    /// The content type of the main part changes, and — where the new kind
    /// cannot hold macros and the document has them — the macros go, because
    /// Word writes none into a macro-free file and says so first. Returns
    /// whether anything changed.
    pub fn set_kind(&mut self, kind: Kind) -> bool {
        let mut changed = false;
        if !kind.allows_macros() && self.has_macros() {
            changed |= self.remove_macros();
        }
        if self.kind() != kind {
            let mut content_types = self.package().content_types().clone();
            content_types.set_override(&self.document_part, kind.content_type());
            self.package_mut().set_content_types(content_types);
            changed = true;
        }
        if changed {
            self.mark_modified();
        }
        changed
    }

    /// Takes the macros out: the parts, the relationships to them, and the
    /// content types that declared them. Returns whether there were any.
    pub fn remove_macros(&mut self) -> bool {
        let parts = self.macro_parts();
        if parts.is_empty() {
            return false;
        }
        let Ok(mut relationships) = self.package().relationships(&self.document_part) else {
            return false;
        };
        for (id, part) in &parts {
            relationships.remove(id);
            self.package_mut().remove_part(part);
        }
        if self.package_mut().set_relationships(&relationships).is_err() {
            return false;
        }
        self.mark_modified();
        true
    }

    /// Where the document says it was made from, if it says.
    #[must_use]
    pub fn attached_template(&self) -> Option<String> {
        let root = self.settings_root()?;
        let element = root.child(Some(read::W), "attachedTemplate")?;
        let id = element.attribute(Some(read::RELATIONSHIPS), "id")?;
        let part = self.settings_part()?;
        let relationships = self.package().relationships(&part).ok()?;
        let relationship = relationships.by_id(id)?;
        Some(from_file_address(&relationship.target))
    }

    /// Says where the document was made from: a path on this machine, written
    /// as Word writes it, as an address with `file:///` in front.
    pub fn attach_template(&mut self, path: &str) -> bool {
        let Some(part) = self.settings_part() else { return false };
        let Some(mut root) = self.settings_root() else { return false };
        let mut relationships = self
            .package()
            .relationships(&part)
            .unwrap_or_else(|_| wp_opc::Relationships::new(&part));

        // The old one goes, relationship and all, so a document made from a
        // document made from a template does not point two ways.
        if let Some(old) = root
            .child(Some(read::W), "attachedTemplate")
            .and_then(|element| element.attribute(Some(read::RELATIONSHIPS), "id"))
            .map(str::to_owned)
        {
            relationships.remove(&old);
        }
        root.remove_children_named(Some(read::W), "attachedTemplate");

        let address = to_file_address(path);
        let id = relationships.add(ATTACHED_TEMPLATE, &address, TargetMode::External).id.clone();
        let prefix = self.prefix();
        let mut element =
            Element::new(&edit::name_with(prefix.as_deref(), "attachedTemplate"), Some(read::W));
        element.declarations.push((Some("r".to_owned()), read::RELATIONSHIPS.to_owned()));
        element.set_namespaced_attribute("r:id", read::RELATIONSHIPS, &id);
        edit::insert_ordered(&mut root, element, settings::SETTINGS_ORDER);

        if self.package_mut().set_relationships(&relationships).is_err() {
            return false;
        }
        if !self.save_settings_root(root) {
            return false;
        }
        self.mark_modified();
        true
    }

    /// A new document made from a template, which is what opening a template
    /// the ordinary way gives.
    ///
    /// Everything the template holds, as a plain document that remembers
    /// where it came from. The macros stay in the template: a document made
    /// from a macro-enabled template is a plain document, and the macros run
    /// from the template it is attached to. Not counted as changed, because
    /// nothing has been typed into it yet — which is how Word has it too.
    pub fn from_template(bytes: &[u8], path: Option<&str>) -> Result<Self, Error> {
        let mut document = Self::open(bytes)?;
        document.set_kind(Kind::Document);
        if let Some(path) = path {
            document.attach_template(path);
        }
        document.modified = false;
        Ok(document)
    }
}

/// A path as Word writes it into a relationship: `file:///C:\Users\...`.
fn to_file_address(path: &str) -> String {
    if path.contains("://") {
        return path.to_owned();
    }
    let mut address = String::from("file:///");
    for character in path.chars() {
        match character {
            ' ' => address.push_str("%20"),
            '%' => address.push_str("%25"),
            other => address.push(other),
        }
    }
    address
}

/// And back again.
fn from_file_address(address: &str) -> String {
    let path = address.strip_prefix("file:///").unwrap_or(address);
    path.replace("%20", " ").replace("%25", "%")
}

impl Document {
    /// Puts a Visual Basic project into the document, or replaces the one it
    /// has.
    ///
    /// A document that had none becomes macro-enabled by it: the part goes
    /// in, the relationship that reaches it goes in, and the kind changes,
    /// because a `.docx` that carries macros is a file Word refuses to open
    /// and this program must not write one.
    ///
    /// Nothing is looked at inside the bytes. What a project is made of is
    /// [`wp_vba`]'s business, and a document that carries one somebody else
    /// wrote should carry it back out untouched.
    pub fn set_macro_project(&mut self, bytes: Vec<u8>) -> bool {
        if !self.kind().allows_macros() {
            self.set_kind(match self.kind() {
                Kind::Template => Kind::MacroEnabledTemplate,
                _ => Kind::MacroEnabledDocument,
            });
        }

        let main = self.document_part.clone();
        let Ok(mut relationships) = self.package().relationships(&main) else { return false };
        if relationships.single_by_type(VBA_PROJECT).is_none() {
            relationships.add(VBA_PROJECT, "vbaProject.bin", TargetMode::Internal);
            if self.package_mut().set_relationships(&relationships).is_err() {
                return false;
            }
        }
        self.package_mut().add_part(
            "word/vbaProject.bin",
            "application/vnd.ms-office.vbaProject",
            bytes,
        );
        self.mark_modified();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Block, Body, Paragraph};

    fn document() -> Document {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("Hello")));
        Document::create(&body).expect("a document")
    }

    /// A document with a macro project in it, built by hand: the part, its
    /// content type, and the relationship that reaches it.
    fn macro_document() -> Document {
        let mut document = document();
        document.set_kind(Kind::MacroEnabledDocument);
        let main = document.document_part.clone();
        let mut relationships = document.package().relationships(&main).expect("relationships");
        relationships.add(VBA_PROJECT, "vbaProject.bin", TargetMode::Internal);
        document.package_mut().set_relationships(&relationships).expect("written");
        document.package_mut().add_part(
            "word/vbaProject.bin",
            "application/vnd.ms-office.vbaProject",
            vec![0xD0, 0xCF, 0x11, 0xE0],
        );
        document
    }

    #[test]
    fn a_new_document_is_a_document() {
        let document = document();
        assert_eq!(document.kind(), Kind::Document);
        assert!(!document.has_macros());
    }

    #[test]
    fn the_kind_is_the_content_type_of_the_main_part() {
        for kind in Kind::ALL {
            let mut document = document();
            assert_eq!(document.set_kind(kind), kind != Kind::Document);
            assert_eq!(document.kind(), kind);
            let bytes = document.save().expect("saved");
            let reopened = Document::open(&bytes).expect("reopened");
            assert_eq!(reopened.kind(), kind, "{kind:?} did not survive the file");
            assert_eq!(Kind::of_extension(kind.extension()), Some(kind));
        }
    }

    #[test]
    fn a_macro_enabled_document_keeps_its_macros_and_a_plain_one_cannot() {
        let document = macro_document();
        assert!(document.has_macros());
        let bytes = document.save().expect("saved");
        let mut reopened = Document::open(&bytes).expect("reopened");
        assert!(reopened.has_macros(), "the macros did not survive the file");
        assert_eq!(reopened.kind(), Kind::MacroEnabledDocument);

        // As a template that may hold them, they stay.
        reopened.set_kind(Kind::MacroEnabledTemplate);
        assert!(reopened.has_macros());

        // As a plain document, they go — the part, its type, and the way to it.
        reopened.set_kind(Kind::Document);
        assert!(!reopened.has_macros());
        assert!(reopened.package().part("word/vbaProject.bin").is_none());
        assert!(reopened.package().content_type("word/vbaProject.bin").is_none());
        let relationships = reopened.package().relationships("word/document.xml").unwrap();
        assert!(relationships.by_type(VBA_PROJECT).next().is_none());
        assert!(reopened.package().validate().is_empty(), "{:?}", reopened.package().validate());
    }

    #[test]
    fn a_document_made_from_a_template_remembers_it_and_is_a_document() {
        let mut template = document();
        template.set_kind(Kind::MacroEnabledTemplate);
        let bytes = template.save().expect("saved");

        let made =
            Document::from_template(&bytes, Some("C:\\Templates\\My Letter.dotm")).expect("made");
        assert_eq!(made.kind(), Kind::Document);
        assert!(!made.is_modified(), "nothing has been typed yet");
        assert_eq!(made.attached_template().as_deref(), Some("C:\\Templates\\My Letter.dotm"));
        assert_eq!(made.plain_text().trim_end(), "Hello");

        // And the way Word writes it: an address, with the space escaped.
        let part = made.settings_part().unwrap();
        let relationships = made.package().relationships(&part).unwrap();
        let relationship = relationships.by_type(ATTACHED_TEMPLATE).next().expect("a relationship");
        assert_eq!(relationship.target, "file:///C:\\Templates\\My%20Letter.dotm");
        assert_eq!(relationship.mode, TargetMode::External);

        // Survives the file, and is not doubled by attaching again.
        let mut reopened = Document::open(&made.save().unwrap()).unwrap();
        assert_eq!(reopened.attached_template().as_deref(), Some("C:\\Templates\\My Letter.dotm"));
        reopened.attach_template("D:\\Other.dotx");
        assert_eq!(reopened.attached_template().as_deref(), Some("D:\\Other.dotx"));
        let relationships = reopened.package().relationships(&part).unwrap();
        assert_eq!(relationships.by_type(ATTACHED_TEMPLATE).count(), 1);
    }

    #[test]
    fn the_labels_are_words() {
        assert_eq!(Kind::Document.label(), "Word Document");
        assert_eq!(Kind::MacroEnabledTemplate.label(), "Word Macro-Enabled Template");
        assert!(Kind::Template.is_template());
        assert!(!Kind::Template.allows_macros());
        assert!(Kind::MacroEnabledDocument.allows_macros());
        assert_eq!(Kind::of_extension("DOCM"), Some(Kind::MacroEnabledDocument));
        assert_eq!(Kind::of_extension("txt"), None);
    }
}
