//! The signatures a document carries.
//!
//! # What signing a document is here
//!
//! A thin layer over [`wp_sign`], which does the whole of it. What this adds
//! is the two things a document knows and a package does not: that saving an
//! edited document has to take the signatures off, and that the signatures
//! are read from the package the document was opened from rather than from
//! one built fresh.
//!
//! # Why an edited document loses its signatures
//!
//! Because a signature says "this document is what it was when I signed it",
//! and after an edit that is no longer true. Word marks such a signature
//! invalid and takes it off when the document is saved; so does this. Leaving
//! it in place would be leaving a claim in the file that the file itself
//! disproves.

use crate::{Document, Error};

impl Document {
    /// Every signature the document carries, and whether each one holds.
    #[must_use]
    pub fn signatures(&self) -> Vec<wp_sign::Signature> {
        wp_sign::signatures(self.package())
    }

    /// Whether it carries any at all.
    #[must_use]
    pub fn is_signed(&self) -> bool {
        wp_sign::is_signed(self.package())
    }

    /// Takes every signature off: the signature parts, and the relationship
    /// that points at them.
    ///
    /// What Word does when a signed document is edited anyway, having asked
    /// first — the asking is the program's. Not a step to take back, because
    /// Word's is not, and a change to the document: the file on disk still
    /// carries them, so saving it is offered. Says whether there were any.
    pub fn remove_signatures(&mut self) -> bool {
        if !wp_sign::unsign(self.package_mut()) {
            return false;
        }
        self.changed_off_the_record();
        true
    }

    /// The bytes of the document with a signature over them.
    ///
    /// Signed as it stands on disk: what a signature covers is a package, so
    /// the document is written out first and the signature put on that.
    pub fn save_signed(&self, signer: &wp_sign::Signer) -> Result<Vec<u8>, Error> {
        let bytes = self.save()?;
        let mut package = wp_opc::Package::open(&bytes)?;
        wp_sign::sign(&mut package, signer).map_err(Error::Signing)?;
        Ok(package.save()?)
    }

    /// Saves it and signs one of the signatures it already carries.
    ///
    /// The document is written out first for the same reason signing it is:
    /// what a countersignature is about is a signature in a file, and the file
    /// has to be the one on the disk. Nothing else about the document changes
    /// — a countersignature goes among a signature's unsigned properties, and
    /// every signature already there holds exactly as well afterwards.
    pub fn countersign_saved(
        &self,
        part: &str,
        signer: &wp_sign::Signer,
    ) -> Result<Vec<u8>, String> {
        let bytes = self.save().map_err(|error| error.to_string())?;
        let mut package = wp_opc::Package::open(&bytes).map_err(|error| error.to_string())?;
        wp_sign::countersign(&mut package, part, signer)?;
        package.save().map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use crate::model::{Block, Body, Paragraph};
    use crate::{Document, TextPosition};

    /// A document with a signature on it, as far as its parts go.
    ///
    /// Not a signature anybody could check — making one needs a key — but the
    /// parts and the relationships are where Word puts them, which is all that
    /// taking one off looks at.
    fn signed() -> Document {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("Agreed")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let mut package = wp_opc::Package::open(&bytes).expect("a package");

        package.add_part(
            wp_sign::package::ORIGIN,
            "application/vnd.openxmlformats-package.digital-signature-origin",
            Vec::new(),
        );
        package.add_part(
            "_xmlsignatures/sig1.xml",
            wp_sign::package::SIGNATURE_TYPE,
            b"<Signature/>".to_vec(),
        );
        let mut origin = wp_opc::Relationships::new(wp_sign::package::ORIGIN);
        origin.add(
            "http://schemas.openxmlformats.org/package/2006/relationships/digital-signature/signature",
            "sig1.xml",
            wp_opc::TargetMode::Internal,
        );
        package.set_relationships(&origin).expect("the origin's relationships");
        let mut root = package.relationships("").expect("the package's relationships");
        root.add(
            "http://schemas.openxmlformats.org/package/2006/relationships/digital-signature/origin",
            wp_sign::package::ORIGIN,
            wp_opc::TargetMode::Internal,
        );
        package.set_relationships(&root).expect("the package's relationships");

        Document::open(&package.save().expect("saving the package")).expect("reopening")
    }

    fn carries_a_signature(bytes: &[u8]) -> bool {
        let package = wp_opc::Package::open(bytes).expect("a package");
        package.entries().iter().any(|entry| entry.name.starts_with("_xmlsignatures/"))
    }

    #[test]
    fn an_edited_signed_document_is_written_unsigned_by_both_ways_of_saving() {
        let mut document = signed();
        assert!(document.is_signed(), "the fixture is not signed");
        let untouched = document.save().expect("saving");
        assert!(carries_a_signature(&untouched), "an untouched document lost its signature");

        document.set_caret(TextPosition::new(0, 6));
        document.type_text(" and signed");
        let first = document.save().expect("saving");
        assert!(!carries_a_signature(&first), "the edited document kept its signature");

        document.mark_saved().expect("marking it saved");
        assert!(!document.is_signed(), "the package still carries what the file does not");
        assert!(document.is_as_saved(), "the package is not the file that was written");
        let second = document.save().expect("saving again");
        assert!(!carries_a_signature(&second), "the second save put the signature back");
        assert_eq!(first, second, "saving again wrote something else");
    }

    #[test]
    fn taking_the_signatures_off_leaves_a_change_to_save_and_nothing_to_undo() {
        let mut document = signed();
        assert!(document.remove_signatures());
        assert!(!document.is_signed());
        assert!(document.is_modified(), "the file still carries them and nothing says so");
        assert!(!document.can_undo(), "Word's is not taken back, and neither is this");
        let written = document.save().expect("saving");
        assert!(!carries_a_signature(&written));
        assert!(!document.remove_signatures(), "there was nothing left to take off");
    }
}
