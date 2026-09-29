//! Documents that are encrypted, and stay encrypted when they are saved.
//!
//! # What this adds to [`wp_crypt`]
//!
//! Nothing about the encryption itself, which is all over there. What is here
//! is the one thing a document has to remember: that it came out of an
//! encrypted file, and under which word — so that saving it puts it back the
//! way it was found. A program that quietly wrote a plain copy of a document
//! somebody had encrypted would be doing the worst thing a word processor can
//! do, which is to undo a decision without saying so.
//!
//! # Why the salt is asked for on the way out
//!
//! [`Document::save_sealed`] takes the unguessable bytes rather than making
//! them. A document is arithmetic over bytes; where bytes nobody can guess
//! come from is a question for the machine, and the program that has a
//! machine is the one that calls this. It also means every save gets fresh
//! ones, which is the point of a salt: two saves of the same document under
//! the same password must not come out looking the same.

use crate::{Document, Error};

/// Whether these bytes are an encrypted document rather than a package.
///
/// Worth asking before [`Document::open`] rather than after: the answer
/// decides whether a program shows an error or asks for a password, and those
/// are not the same thing at all.
#[must_use]
pub fn is_sealed(bytes: &[u8]) -> bool {
    wp_crypt::is_encrypted(bytes)
}

impl Document {
    /// Opens an encrypted document.
    ///
    /// The password is kept for as long as the document is open, so that
    /// saving it writes it back encrypted under the same word.
    pub fn open_sealed(bytes: &[u8], password: &str) -> Result<Self, Error> {
        let package = wp_crypt::open(bytes, password)?;
        let mut document = Self::open(&package)?;
        document.password = Some(password.to_owned());
        Ok(document)
    }

    /// The password the document is written back under, if it has one.
    #[must_use]
    pub fn password(&self) -> Option<&str> {
        self.password.as_deref()
    }

    /// Puts a password on the document, or takes it off.
    ///
    /// Nothing is encrypted here: what this changes is what the next save
    /// writes. Says whether anything changed, like every other setter.
    ///
    /// Not a step to take back: Word keeps what is done on the File tab off
    /// its undo list, and this is kept beside the package rather than in it.
    /// So no undo comes back to the file on disk after it either.
    pub fn set_password(&mut self, password: Option<&str>) -> bool {
        let wanted = password.filter(|word| !word.is_empty()).map(str::to_owned);
        if self.password == wanted {
            return false;
        }
        self.password = wanted;
        self.changed_off_the_record();
        true
    }

    /// The bytes to write to the file: the package, sealed if the document
    /// has a password.
    ///
    /// `fresh` is only looked at where there is a password.
    pub fn save_sealed(&self, fresh: &wp_crypt::Fresh) -> Result<Vec<u8>, Error> {
        let package = self.save()?;
        Ok(match &self.password {
            Some(word) => wp_crypt::seal(&package, word, fresh),
            None => package,
        })
    }
}
