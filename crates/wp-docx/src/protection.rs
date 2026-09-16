//! Who may edit the document, and the password that says so.
//!
//! # What a password on a `.docx` is, and is not
//!
//! It is not encryption. The text of a protected document is there in the file
//! for anybody to read, and any program at all may ignore the restriction: this
//! one could, Word could, a script that unzips the file certainly does. What
//! the password does is stop the person sitting in front of the document from
//! lifting the restriction by accident or by a click — the form they were sent
//! stays a form, the draft under review stays under review.
//!
//! That is worth having, and it is worth being exact about. Encryption, where
//! the bytes really are unreadable without the word, is a different feature and
//! a different file format; it is **J2** in the roadmap and it is not this.
//!
//! # What is written
//!
//! The password is not in the file. What is in the file is a salt, a number of
//! turns, and the result of hashing the password that many times — so a program
//! can tell a right answer from a wrong one without ever holding the right one.
//!
//! ```text
//! H₀ = SHA(salt ++ password as UTF-16LE)
//! Hₙ = SHA(Hₙ₋₁ ++ n-1 as four bytes, least significant first)
//! ```
//!
//! run for as many turns as `spinCount` says — Word writes a hundred thousand —
//! and the last one is what goes in the file, written in base 64.
//!
//! The turns are the point. One hash is instant, and a program that tries a
//! million words gets through them in a second; a hundred thousand hashes takes
//! a fraction of a second once, which nobody notices, and a fraction of a
//! second times a million is a fortnight.
//!
//! # Two sets of attributes for one password
//!
//! Word 2007 wrote `w:cryptAlgorithmSid`, `w:cryptSpinCount`, `w:hash` and
//! `w:salt`, naming its hash by a number out of the Windows cryptography
//! headers. The ISO edition of the format replaced them with `w:algorithmName`,
//! `w:spinCount`, `w:hashValue` and `w:saltValue`, naming the hash in words,
//! and Word has written those since 2010.
//!
//! Both are read, because documents of both ages exist. The newer set is
//! written, because that is what Word writes, and because a document this
//! program writes is a document of today.

use wp_xml::tree::Element;

use crate::{edit, read, Document};

/// Which hash stands behind a password.
///
/// Only the two that are met in practice. A document naming any other is read
/// and its restriction obeyed — the mode is in plain text and needs no hash to
/// be understood — but no password can be checked against it, and the program
/// says so rather than pretending to check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Algorithm {
    /// What Word 2007 wrote, and what a document made from an older one still
    /// carries.
    Sha1,
    /// What Word writes now.
    Sha512,
}

impl Algorithm {
    /// The name the ISO attributes use.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Sha1 => "SHA-1",
            Self::Sha512 => "SHA-512",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        match name {
            "SHA-1" | "SHA1" => Some(Self::Sha1),
            "SHA-512" | "SHA512" => Some(Self::Sha512),
            _ => None,
        }
    }

    /// Out of the Windows cryptography headers: `CALG_SHA1` is four and
    /// `CALG_SHA_512` is fourteen.
    fn from_identifier(number: &str) -> Option<Self> {
        match number {
            "4" => Some(Self::Sha1),
            "14" => Some(Self::Sha512),
            _ => None,
        }
    }

    /// The hash of some bytes, whichever it is.
    fn of(self, bytes: &[u8]) -> Vec<u8> {
        match self {
            Self::Sha1 => wp_hash::sha1(bytes).to_vec(),
            Self::Sha512 => wp_hash::sha512(bytes).to_vec(),
        }
    }
}

/// How many turns a password this program writes is hashed for.
///
/// Word's number since 2013. Following it is not deference: a document that
/// says a hundred thousand and a document that says a thousand look the same
/// to a person and are a hundred times apart to somebody guessing, and a
/// number nobody has to think about is best set where everybody else set it.
pub const SPINS: u32 = 100_000;

/// How many bytes of salt. Word writes sixteen.
pub const SALT_BYTES: usize = 16;

/// What has to be typed before a restriction is lifted.
///
/// Carries no password — see the module's own documentation for what it does
/// carry and why that is enough to tell a right answer from a wrong one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Password {
    /// `None` where the document named a hash this program does not have. The
    /// restriction still stands; it is the checking that cannot be done.
    algorithm: Option<Algorithm>,
    /// The name as the document wrote it, for saying what could not be done.
    named: String,
    salt: Vec<u8>,
    hash: Vec<u8>,
    spins: u32,
}

impl Password {
    /// The password `word`, salted with bytes the caller got from the machine.
    ///
    /// The salt is asked for rather than made here so that this crate stays
    /// arithmetic: a document is a thing that is read and written, and where
    /// unguessable bytes come from is a question for whoever is running.
    #[must_use]
    pub fn new(word: &str, salt: &[u8]) -> Self {
        Self::turned(word, salt, SPINS)
    }

    /// The same, hashed for as many turns as the caller says.
    ///
    /// Needed because a document may say any number and, once it has been
    /// opened, a password set on it has to be written back at the number it
    /// already carries. Tests use it too: a hundred thousand turns of
    /// arithmetic is the point of [`SPINS`] and not of a test.
    #[must_use]
    pub fn turned(word: &str, salt: &[u8], spins: u32) -> Self {
        let algorithm = Algorithm::Sha512;
        Self {
            algorithm: Some(algorithm),
            named: algorithm.name().to_owned(),
            hash: hashed(algorithm, word, salt, spins),
            salt: salt.to_vec(),
            spins,
        }
    }

    /// Whether the password can be checked at all.
    #[must_use]
    pub fn understood(&self) -> bool {
        self.algorithm.is_some()
    }

    /// What the document called its hash, whether or not this program has it.
    #[must_use]
    pub fn algorithm_name(&self) -> &str {
        &self.named
    }

    /// Whether what was typed is the password.
    ///
    /// False for a hash this program does not have: an answer it cannot check
    /// is an answer it must not accept.
    #[must_use]
    pub fn accepts(&self, attempt: &str) -> bool {
        let Some(algorithm) = self.algorithm else { return false };
        // Not a plain comparison: two hashes that differ in their first byte
        // take less time to compare than two that differ in their last, and a
        // program that is asked a million times can be told where it went
        // wrong by the clock alone.
        let attempt = hashed(algorithm, attempt, &self.salt, self.spins);
        if attempt.len() != self.hash.len() {
            return false;
        }
        let mut differences = 0u8;
        for (left, right) in attempt.iter().zip(&self.hash) {
            differences |= left ^ right;
        }
        differences == 0
    }

    /// Reads whichever set of attributes the element carries, newer first.
    fn read(element: &Element) -> Option<Self> {
        let attribute = |name: &str| element.attribute(Some(read::W), name);
        let (named, algorithm, hash, salt, spins) = match attribute("hashValue") {
            Some(hash) => {
                let named = attribute("algorithmName").unwrap_or_default();
                (
                    named.to_owned(),
                    Algorithm::from_name(named),
                    hash,
                    attribute("saltValue").unwrap_or_default(),
                    attribute("spinCount"),
                )
            }
            None => {
                let number = attribute("cryptAlgorithmSid").unwrap_or_default();
                let algorithm = Algorithm::from_identifier(number);
                (
                    algorithm.map_or_else(
                        || format!("algorithm {number}"),
                        |algorithm| algorithm.name().to_owned(),
                    ),
                    algorithm,
                    attribute("hash")?,
                    attribute("salt").unwrap_or_default(),
                    attribute("cryptSpinCount"),
                )
            }
        };
        Some(Self {
            algorithm,
            named,
            salt: wp_text::base64::decode(salt.as_bytes()),
            hash: wp_text::base64::decode(hash.as_bytes()),
            // A document that does not say ran the loop no times at all, which
            // is what the attribute being absent means.
            spins: spins.and_then(|text| text.parse().ok()).unwrap_or(0),
        })
    }

    /// Writes the ISO attributes onto an element.
    fn write(&self, element: &mut Element, prefix: Option<&str>) {
        let name = |local: &str| edit::name_with(prefix, local);
        element.set_namespaced_attribute(&name("algorithmName"), read::W, &self.named);
        element.set_namespaced_attribute(
            &name("hashValue"),
            read::W,
            &wp_text::base64::encode(&self.hash),
        );
        element.set_namespaced_attribute(
            &name("saltValue"),
            read::W,
            &wp_text::base64::encode(&self.salt),
        );
        element.set_namespaced_attribute(&name("spinCount"), read::W, &self.spins.to_string());
    }
}

/// The hash the format asks for.
///
/// The password goes in as UTF-16, two bytes a character, least significant
/// first — which is how Windows has held text since before this format, and
/// the reason a password with a letter outside the alphabet hashes to
/// different bytes here than it would in a program that used UTF-8.
fn hashed(algorithm: Algorithm, word: &str, salt: &[u8], spins: u32) -> Vec<u8> {
    let mut first = salt.to_vec();
    for unit in word.encode_utf16() {
        first.extend_from_slice(&unit.to_le_bytes());
    }
    let mut hash = algorithm.of(&first);
    for turn in 0..spins {
        let mut next = hash;
        next.extend_from_slice(&turn.to_le_bytes());
        hash = algorithm.of(&next);
    }
    hash
}

/// What a reader is allowed to do to a protected document.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EditMode {
    /// Nothing at all.
    #[default]
    ReadOnly,
    /// Only leave comments.
    Comments,
    /// Edit, but every change is recorded.
    TrackedChanges,
    /// Only fill in form fields.
    Forms,
}

impl EditMode {
    #[must_use]
    pub(crate) fn word(self) -> &'static str {
        match self {
            Self::ReadOnly => "readOnly",
            Self::Comments => "comments",
            Self::TrackedChanges => "trackedChanges",
            Self::Forms => "forms",
        }
    }

    #[must_use]
    pub(crate) fn from_word(word: &str) -> Option<Self> {
        match word {
            "readOnly" => Some(Self::ReadOnly),
            "comments" => Some(Self::Comments),
            "trackedChanges" => Some(Self::TrackedChanges),
            "forms" => Some(Self::Forms),
            _ => None,
        }
    }

    /// What a person is shown when picking one.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::ReadOnly => "No changes (Read only)",
            Self::Comments => "Comments",
            Self::TrackedChanges => "Tracked changes",
            Self::Forms => "Filling in forms",
        }
    }

    /// Every one that can be picked, in Word's order.
    pub const ALL: &'static [Self] =
        &[Self::ReadOnly, Self::Comments, Self::TrackedChanges, Self::Forms];
}

/// Everything a protected document says about who may change it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Protection {
    pub mode: EditMode,
    /// Whether formatting is restricted as well as editing: Word's "Limit
    /// formatting to a selection of styles", which forbids direct formatting
    /// and any style the document has marked locked.
    pub formatting: bool,
    pub password: Option<Password>,
}

impl Protection {
    /// A restriction with no password, which anybody may lift.
    #[must_use]
    pub fn new(mode: EditMode) -> Self {
        Self { mode, formatting: false, password: None }
    }

    /// The same with a password behind it.
    #[must_use]
    pub fn behind(mut self, word: &str, salt: &[u8]) -> Self {
        self.password = Some(Password::new(word, salt));
        self
    }

    /// Whether what was typed lifts it.
    ///
    /// A restriction with no password is lifted by anybody, which is what Word
    /// does and the reason its dialog leaves the password boxes empty: the
    /// restriction is then a reminder rather than a lock.
    #[must_use]
    pub fn opens_with(&self, attempt: &str) -> bool {
        self.password.as_ref().is_none_or(|password| password.accepts(attempt))
    }
}

impl Document {
    // --- Who may edit it ------------------------------------------------------

    /// What a reader is allowed to do, if the document says.
    #[must_use]
    pub fn protection(&self) -> Option<EditMode> {
        self.protection_rules().map(|rules| rules.mode)
    }

    /// The whole of it: the mode, whether formatting is restricted too, and
    /// the password.
    #[must_use]
    pub fn protection_rules(&self) -> Option<Protection> {
        let root = self.settings_root()?;
        let element = root.child(Some(read::W), "documentProtection")?;
        // Written but not enforced means Word ignores it, and so does this.
        if !read::attribute_is_on(element, "enforcement") {
            return None;
        }
        Some(Protection {
            mode: EditMode::from_word(
                element.attribute(Some(read::W), "edit").unwrap_or_default(),
            )?,
            formatting: read::attribute_is_on(element, "formatting"),
            password: Password::read(element),
        })
    }

    /// Restricts editing, or lifts the restriction.
    ///
    /// Nothing is checked here. A document does not refuse to have its own
    /// settings written; it is the program in front of the person that asks
    /// for the password first, and this is what it calls once it has one.
    pub fn set_protection(&mut self, wanted: Option<&Protection>) -> bool {
        if self.protection_rules().as_ref() == wanted {
            return false;
        }
        let Some(mut root) = self.settings_root() else { return false };
        root.remove_children_named(Some(read::W), "documentProtection");

        if let Some(wanted) = wanted {
            let prefix = self.prefix();
            let name = |local: &str| edit::name_with(prefix.as_deref(), local);
            let mut element = Element::new(&name("documentProtection"), Some(read::W));
            element.set_namespaced_attribute(&name("edit"), read::W, wanted.mode.word());
            if wanted.formatting {
                element.set_namespaced_attribute(&name("formatting"), read::W, "1");
            }
            element.set_namespaced_attribute(&name("enforcement"), read::W, "1");
            if let Some(password) = &wanted.password {
                password.write(&mut element, prefix.as_deref());
            }
            edit::insert_ordered(&mut root, element, crate::settings::SETTINGS_ORDER);
        }

        if !self.save_settings_root(root) {
            return false;
        }
        // A document restricted to tracked changes has to be recording them,
        // or the restriction is a label on an empty box.
        if matches!(wanted, Some(rules) if rules.mode == EditMode::TrackedChanges) {
            self.set_tracking_changes(true);
        }
        self.mark_modified();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Few enough turns that a test is arithmetic and not a wait. What the
    /// number is for is tested where it is used, not here.
    const FEW: u32 = 8;

    fn password(word: &str, salt: &[u8]) -> Password {
        Password::turned(word, salt, FEW)
    }

    #[test]
    fn every_edit_mode_survives_being_written_and_read_back() {
        for mode in EditMode::ALL {
            assert_eq!(EditMode::from_word(mode.word()), Some(*mode));
        }
    }

    #[test]
    fn an_unknown_word_is_not_a_mode() {
        assert_eq!(EditMode::from_word("something else"), None);
    }

    #[test]
    fn the_right_word_is_taken_and_a_wrong_one_is_not() {
        let password = password("Open Sesame", b"0123456789abcdef");
        assert!(password.accepts("Open Sesame"));
        assert!(!password.accepts("open sesame"), "the case matters");
        assert!(!password.accepts("Open Sesame "), "so does the space");
        assert!(!password.accepts(""));
    }

    #[test]
    fn the_same_word_with_a_different_salt_is_a_different_hash() {
        let one = password("secret", b"0123456789abcdef");
        let other = password("secret", b"fedcba9876543210");
        assert_ne!(one.hash, other.hash, "which is the whole point of a salt");
        assert!(other.accepts("secret"), "and both still open");
    }

    #[test]
    fn a_hash_this_program_has_not_got_accepts_nothing() {
        let mut password = password("secret", b"0123456789abcdef");
        password.algorithm = None;
        password.named = "MD5".to_owned();
        assert!(!password.understood());
        assert!(!password.accepts("secret"), "an answer it cannot check it must not take");
    }

    #[test]
    fn no_password_means_anybody_may_lift_it() {
        let plain = Protection::new(EditMode::ReadOnly);
        assert!(plain.opens_with(""), "which is what Word does");
        let locked = Protection {
            password: Some(password("shibboleth", b"salted")),
            ..Protection::new(EditMode::ReadOnly)
        };
        assert!(!locked.opens_with(""));
        assert!(locked.opens_with("shibboleth"));
    }
}
