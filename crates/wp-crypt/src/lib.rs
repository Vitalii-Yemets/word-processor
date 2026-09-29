//! The encryption Office puts round a package.
//!
//! # What an encrypted `.docx` actually is
//!
//! Not a `.docx` at all, once it is encrypted. It is a compound file — the
//! old Office file system, see [`wp_ole`] — holding two streams: one called
//! `EncryptionInfo` saying how the encryption was done, and one called
//! `EncryptedPackage` holding the whole zip archive the document would
//! otherwise have been, enciphered. Opening one means reading the first,
//! working the key out of the password, and deciphering the second; what
//! comes out is an ordinary `.docx` in memory, and everything else in this
//! program can go on as though nothing had happened.
//!
//! # The two schemes, and why both are here
//!
//! **Agile**, which Office has written since 2010: the description is XML,
//! the cipher is AES in chaining mode, the hash is usually SHA-512, and the
//! password is hashed a hundred thousand times over before it becomes a key.
//! It carries an integrity check as well, so that a block changed in the
//! middle of a file nobody could read is noticed rather than deciphered into
//! nonsense. This is read and written.
//!
//! **Standard**, which Office 2007 wrote and Office still opens: the
//! description is a fixed run of bytes, the cipher is AES with no chaining at
//! all, and the hash is SHA-1. This is read, because documents written in it
//! exist; it is not written, because writing it would mean making a new
//! document weaker than it has to be.
//!
//! And the older ones, read and never written: the RC4 schemes Office used
//! before 2007 — see [`rc4`] — and, for the binary formats that enciphered
//! their streams where they lay rather than a package whole, those two again
//! and Word 95's exclusive-or — see [`binary`].
//!
//! # What the password does here, and what it did in [`wp_docx::protection`]
//!
//! Two different things, and they are worth keeping apart. A restriction's
//! password stops a person lifting the restriction; the text of the document
//! is there in the file for anybody. This one is the other kind: without it
//! the bytes are not readable at all, by this program or any other, and there
//! is no way round it but to try words until one works — which is what the
//! hundred thousand turns of hashing are there to make slow.

use wp_ole::CompoundFile;

mod agile;
pub mod binary;
mod rc4;

/// The key the oldest of the schemes makes, which a test builds a file with.
///
/// Lifted out of [`rc4`] rather than the whole module being opened: what a
/// caller outside this crate can want is to make one of these files, and the
/// key is the only part of it that cannot be worked out from the standard in
/// a page.
pub use rc4::binary_key;
mod spaces;
mod standard;

/// What can go wrong with an encrypted document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// Not an encrypted document: no compound file, or one without the two
    /// streams an encrypted document has.
    NotEncrypted,
    /// An encrypted document written a way this program does not read.
    Unsupported(String),
    /// The password is not the password.
    WrongPassword,
    /// The file says something the format does not allow, or stops short.
    Damaged(&'static str),
    /// The password was right and the bytes are not the bytes that were
    /// written: somebody has changed the file since.
    Tampered,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotEncrypted => write!(f, "not an encrypted document"),
            Self::Unsupported(how) => write!(f, "encrypted with {how}, which this program has not"),
            Self::WrongPassword => write!(f, "that is not the password"),
            Self::Damaged(what) => write!(f, "the encrypted document's {what} is damaged"),
            Self::Tampered => {
                write!(f, "the document has been changed since it was encrypted")
            }
        }
    }
}

impl std::error::Error for Error {}

/// The name of the stream holding the description.
pub(crate) const INFO: &str = "EncryptionInfo";
/// And the one holding the document.
pub(crate) const PACKAGE: &str = "EncryptedPackage";

/// Whether these bytes are an encrypted Office document.
///
/// Cheap, and asked of every file this program is given: a `.docx` that turns
/// out to be a compound file is an encrypted one, and the program has to ask
/// for a password rather than say the file is broken.
#[must_use]
pub fn is_encrypted(bytes: &[u8]) -> bool {
    CompoundFile::open(bytes.to_vec())
        .is_ok_and(|file| file.stream(INFO).is_some() && file.stream(PACKAGE).is_some())
}

/// The package inside, given the password.
///
/// What comes back is the bytes of an ordinary `.docx`.
pub fn open(bytes: &[u8], password: &str) -> Result<Vec<u8>, Error> {
    let file = CompoundFile::open(bytes.to_vec()).map_err(|_| Error::NotEncrypted)?;
    let info = file.stream(INFO).ok_or(Error::NotEncrypted)?;
    let package = file.stream(PACKAGE).ok_or(Error::NotEncrypted)?;
    // What the file says was done to the package. A file naming a transform
    // this program has not got is a file it must not claim to have opened -
    // a rights-managed one, whose key comes from a server nobody here can
    // ask. See [`spaces`].
    if !spaces::understood(&file) {
        return Err(Error::Unsupported(String::from("a transform this program has not got")));
    }
    if info.len() < 8 {
        return Err(Error::Damaged("description"));
    }

    let major = u16::from_le_bytes([info[0], info[1]]);
    let minor = u16::from_le_bytes([info[2], info[3]]);
    match (major, minor) {
        (4, 4) => agile::open(&info[8..], &package, password),
        (2..=4, 2) => standard::open(&info[8..], &package, password),
        (_, 3) => Err(Error::Unsupported(String::from("a key from a rights server"))),
        // Word 97's own, which is RC4 with a forty-bit key and no header at
        // all: the salt and the verifier follow the version and nothing
        // else does. See [`rc4`].
        (1, 1) => rc4::open_binary(&info[8..], &package, password),
        (2..=4, 1) => Err(Error::Unsupported(String::from("the RC4 cipher"))),
        _ => Err(Error::Unsupported(format!("version {major}.{minor} of the encryption"))),
    }
}

/// The unguessable bytes sealing a document needs.
///
/// Asked for rather than made here, so that this crate stays arithmetic:
/// where bytes nobody can guess come from is a question for the machine, and
/// the shell is where this program asks it.
#[derive(Clone, Debug)]
pub struct Fresh {
    /// The salt the package's initialisation vectors are made from.
    pub package_salt: [u8; 16],
    /// The salt under the password.
    pub password_salt: [u8; 16],
    /// The key the document is actually enciphered with. The password never
    /// becomes this key: it enciphers it, which is what lets a password be
    /// changed without the whole document being written again.
    pub key: [u8; 32],
    /// And the key its integrity check is made with.
    pub hmac_key: [u8; 64],
    /// The bytes a password is proved against: enciphered beside their own
    /// hash, so that a right answer can be told from a wrong one without the
    /// document being deciphered at all.
    pub verifier: [u8; 16],
}

impl Fresh {
    /// All of it out of one run of unguessable bytes.
    ///
    /// A hundred and forty-four of them, which is what the two salts, the
    /// verifier and the two keys come to, none of them sharing a byte with
    /// another. Asking the machine once rather than five times is not a
    /// saving worth having on its own; what it is for is that a caller has
    /// one thing to get right instead of five.
    #[must_use]
    pub fn from_bytes(bytes: &[u8; 144]) -> Self {
        let mut package_salt = [0u8; 16];
        let mut password_salt = [0u8; 16];
        let mut key = [0u8; 32];
        let mut hmac_key = [0u8; 64];
        let mut verifier = [0u8; 16];
        package_salt.copy_from_slice(&bytes[..16]);
        password_salt.copy_from_slice(&bytes[16..32]);
        verifier.copy_from_slice(&bytes[32..48]);
        key.copy_from_slice(&bytes[48..80]);
        hmac_key.copy_from_slice(&bytes[80..144]);
        Self { package_salt, password_salt, key, hmac_key, verifier }
    }
}

/// An encrypted document holding this package.
///
/// Written the way Office writes one now: AES with a two hundred and
/// fifty-six bit key, chained, with SHA-512 over a hundred thousand turns
/// behind the password.
#[must_use]
pub fn seal(package: &[u8], password: &str, fresh: &Fresh) -> Vec<u8> {
    agile::seal(package, password, fresh)
}

/// How many turns a password this program writes is hashed for. Office's
/// number.
pub const SPINS: u32 = 100_000;

/// The password as the formats hash it: two bytes a character, least
/// significant first.
pub(crate) fn utf16(password: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(password.len() * 2);
    for unit in password.encode_utf16() {
        out.extend_from_slice(&unit.to_le_bytes());
    }
    out
}

/// A run of bytes brought to a length: cut if it is too long, padded with the
/// filler the format names if it is too short.
///
/// Both formats say this in the same words and both mean `0x36`, which is the
/// inner padding of an authenticated message and is here for no better reason
/// than that somebody reached for a number they already had.
fn fitted(bytes: &[u8], length: usize) -> Vec<u8> {
    let mut out = bytes.to_vec();
    out.resize(length, 0x36);
    out
}

/// Whether two runs of bytes are the same, without saying how far along they
/// stopped agreeing.
fn same(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut differences = 0u8;
    for (one, other) in left.iter().zip(right) {
        differences |= one ^ other;
    }
    differences == 0
}

/// The eight bytes at the front of `EncryptedPackage`, which say how long the
/// package was before it was padded out to whole blocks.
pub(crate) fn declared_length(package: &[u8]) -> Result<usize, Error> {
    let eight: [u8; 8] =
        package.get(..8).ok_or(Error::Damaged("package"))?.try_into().expect("eight bytes");
    usize::try_from(u64::from_le_bytes(eight)).map_err(|_| Error::Damaged("package"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn something_that_is_not_encrypted_is_not_taken_for_it() {
        assert!(!is_encrypted(b"PK\x03\x04 an ordinary zip"));
        assert!(!is_encrypted(&[]));
        // A compound file with neither stream is a `.doc`, not an encrypted
        // document, and this must not take one for the other.
        let mut builder = wp_ole::Builder::new();
        builder.stream("WordDocument", vec![0; 100]);
        assert!(!is_encrypted(&builder.build()));
    }

    #[test]
    fn a_password_is_hashed_two_bytes_to_the_letter() {
        assert_eq!(utf16("AB"), vec![0x41, 0x00, 0x42, 0x00]);
        assert_eq!(utf16(""), Vec::<u8>::new());
        // Outside the alphabet it is still two bytes, which is why a password
        // with an accent in it hashes differently here than it would in a
        // program that reached for UTF-8.
        assert_eq!(utf16("é"), vec![0xE9, 0x00]);
    }

    #[test]
    fn nothing_unguessable_is_used_twice() {
        let bytes: [u8; 144] = core::array::from_fn(|at| at as u8);
        let fresh = Fresh::from_bytes(&bytes);
        let mut all = Vec::new();
        all.extend_from_slice(&fresh.package_salt);
        all.extend_from_slice(&fresh.password_salt);
        all.extend_from_slice(&fresh.verifier);
        all.extend_from_slice(&fresh.key);
        all.extend_from_slice(&fresh.hmac_key);
        assert_eq!(all.len(), 144, "one of them is shorter than it says");
        assert_eq!(all, bytes.to_vec(), "two of them share a byte");
    }

    #[test]
    fn a_run_of_bytes_is_cut_or_padded_with_the_formats_own_filler() {
        assert_eq!(fitted(&[1, 2, 3], 2), vec![1, 2]);
        assert_eq!(fitted(&[1, 2], 4), vec![1, 2, 0x36, 0x36]);
        assert_eq!(fitted(&[1, 2], 2), vec![1, 2]);
    }
}
