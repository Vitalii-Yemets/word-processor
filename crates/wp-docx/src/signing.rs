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
