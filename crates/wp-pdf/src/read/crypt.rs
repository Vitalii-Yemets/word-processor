//! Undoing a PDF's encryption: the standard security handler.
//!
//! [ISO 32000-2] 7.6. A file's strings and streams are enciphered each
//! with a key of its own, made from the file's key and the object's
//! number — RC4 in the older revisions, AES-128 from revision 4 — or all
//! with the file's key as it is, AES-256, in revisions 5 and 6. The file's
//! key comes from a password: made from it with MD5 and checked against
//! what the file keeps, or, from revision 5, unlocked with a hash of it
//! from the key the file keeps enciphered. Most encrypted files have no
//! password to open them at all, only one to change them — the empty
//! password opens those. Only the standard handler is known: a file
//! encrypted for particular people's certificates says so and is not read.

use super::object::{Dictionary, Object};

/// The padding a password is made thirty-two bytes long with.
const PAD: [u8; 32] = [
    0x28, 0xBF, 0x4E, 0x5E, 0x4E, 0x75, 0x8A, 0x41, 0x64, 0x00, 0x4E, 0x56, 0xFF, 0xFA, 0x01, 0x08,
    0x2E, 0x2E, 0x00, 0xB6, 0xD0, 0x68, 0x3E, 0x80, 0x2F, 0x0C, 0xA9, 0xFE, 0x64, 0x53, 0x69, 0x7A,
];

/// Why a file's encryption could not be undone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    /// The password does not open it.
    Password,
    /// It is not the standard handler, or not a revision of it.
    Unsupported,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Method {
    Identity,
    Rc4,
    Aes128,
    Aes256,
}

/// What a file's strings and streams need to be read.
#[derive(Clone, Debug)]
pub struct Crypt {
    key: Vec<u8>,
    strings: Method,
    streams: Method,
    /// Whether the document's metadata stream is enciphered too.
    metadata: bool,
}

/// A password as the older revisions take it: its characters as bytes,
/// thirty-two of them at most.
fn legacy_password(password: &str) -> Vec<u8> {
    let mut out: Vec<u8> =
        password.chars().map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?')).collect();
    out.truncate(32);
    out
}

fn padded(password: &[u8]) -> [u8; 32] {
    let mut out = PAD;
    let length = password.len().min(32);
    out[..length].copy_from_slice(&password[..length]);
    out[length..].copy_from_slice(&PAD[..32 - length]);
    out
}

impl Crypt {
    /// The file's encryption undone with a password, the empty one if the
    /// file needs none.
    pub fn open(encrypt: &Dictionary, first_id: &[u8], password: &str) -> Result<Self, Refused> {
        let name = |key: &str| encrypt.get(key).and_then(Object::as_name);
        let number = |key: &str, default: i64| {
            encrypt.get(key).and_then(Object::as_integer).unwrap_or(default)
        };
        let bytes = |key: &str| match encrypt.get(key) {
            Some(Object::String(bytes)) => bytes.clone(),
            _ => Vec::new(),
        };
        if name("Filter") != Some("Standard") {
            return Err(Refused::Unsupported);
        }
        let version = number("V", 0);
        let revision = number("R", 2);
        let metadata = !matches!(encrypt.get("EncryptMetadata"), Some(Object::Bool(false)));
        let (strings, streams) = match version {
            1 | 2 => (Method::Rc4, Method::Rc4),
            4 | 5 => (crypt_filter(encrypt, "StrF"), crypt_filter(encrypt, "StmF")),
            _ => return Err(Refused::Unsupported),
        };
        let owner = bytes("O");
        let user = bytes("U");
        if revision >= 5 {
            let key = key_from_hash(encrypt, revision, password, &owner, &user)?;
            return Ok(Self { key, strings, streams, metadata });
        }
        let length = match version {
            1 => 5,
            4 => 16,
            _ => (number("Length", 40) / 8).clamp(5, 16) as usize,
        };
        let permissions = number("P", -1) as i32;
        let legacy = Legacy {
            revision,
            length,
            owner: &owner,
            user: &user,
            permissions,
            first_id,
            metadata,
        };
        let password = legacy_password(password);
        // As the user; then as the owner, whose password unlocks the
        // user's.
        if let Some(key) = legacy.try_user(&password) {
            return Ok(Self { key, strings, streams, metadata });
        }
        let user_password = legacy.user_password_from_owner(&password);
        match legacy.try_user(&user_password) {
            Some(key) => Ok(Self { key, strings, streams, metadata }),
            None => Err(Refused::Password),
        }
    }

    /// A string of object `number` deciphered.
    #[must_use]
    pub fn string(&self, number: u32, generation: u16, data: &[u8]) -> Vec<u8> {
        self.apply(self.strings, number, generation, data)
    }

    /// A stream's data deciphered; `kind` is its dictionary's `Type`, since
    /// cross-reference streams are never enciphered, and metadata may not
    /// be.
    #[must_use]
    pub fn stream(&self, number: u32, generation: u16, kind: Option<&str>, data: &[u8]) -> Vec<u8> {
        match kind {
            Some("XRef") => data.to_vec(),
            Some("Metadata") if !self.metadata => data.to_vec(),
            _ => self.apply(self.streams, number, generation, data),
        }
    }

    fn apply(&self, method: Method, number: u32, generation: u16, data: &[u8]) -> Vec<u8> {
        match method {
            Method::Identity => data.to_vec(),
            Method::Rc4 => wp_cipher::rc4(&self.object_key(number, generation, false), data),
            Method::Aes128 => aes_cbc(&self.object_key(number, generation, true), data),
            Method::Aes256 => aes_cbc(&self.key, data),
        }
    }

    /// An object's own key, [ISO 32000-2] algorithm 1.
    fn object_key(&self, number: u32, generation: u16, aes: bool) -> Vec<u8> {
        let mut input = self.key.clone();
        input.extend_from_slice(&number.to_le_bytes()[..3]);
        input.extend_from_slice(&generation.to_le_bytes());
        if aes {
            input.extend_from_slice(b"sAlT");
        }
        let hash = wp_hash::md5(&input);
        hash[..(self.key.len() + 5).min(16)].to_vec()
    }
}

/// The method a crypt filter named in the dictionary uses.
fn crypt_filter(encrypt: &Dictionary, which: &str) -> Method {
    let name = encrypt.get(which).and_then(Object::as_name).unwrap_or("Identity");
    if name == "Identity" {
        return Method::Identity;
    }
    let filter = encrypt
        .get("CF")
        .and_then(Object::as_dictionary)
        .and_then(|filters| filters.get(name))
        .and_then(Object::as_dictionary);
    match filter.and_then(|f| f.get("CFM")).and_then(Object::as_name) {
        Some("V2") => Method::Rc4,
        Some("AESV2") => Method::Aes128,
        Some("AESV3") => Method::Aes256,
        _ => Method::Identity,
    }
}

/// AES in CBC mode, the first sixteen bytes the starting value and the
/// padding taken off the end.
fn aes_cbc(key: &[u8], data: &[u8]) -> Vec<u8> {
    let (Some(start), Some(key)) = (data.get(..16), wp_cipher::Key::new(key)) else {
        return Vec::new();
    };
    let start: [u8; 16] = start.try_into().unwrap_or([0; 16]);
    let body = &data[16..];
    let whole = body.len() / 16 * 16;
    let mut plain = wp_cipher::decrypt_cbc(&key, &start, &body[..whole]);
    if let Some(&pad) = plain.last() {
        if (1..=16).contains(&pad) && plain.len() >= usize::from(pad) {
            plain.truncate(plain.len() - usize::from(pad));
        }
    }
    plain
}

/// What the revisions 2 to 4 need of the dictionary.
struct Legacy<'a> {
    revision: i64,
    length: usize,
    owner: &'a [u8],
    user: &'a [u8],
    permissions: i32,
    first_id: &'a [u8],
    metadata: bool,
}

impl Legacy<'_> {
    /// The file's key from a user password, [ISO 32000-2] algorithm 2.
    fn key(&self, password: &[u8]) -> Vec<u8> {
        let mut input = padded(password).to_vec();
        input.extend_from_slice(&self.owner[..self.owner.len().min(32)]);
        input.extend_from_slice(&self.permissions.to_le_bytes());
        input.extend_from_slice(self.first_id);
        if self.revision >= 4 && !self.metadata {
            input.extend_from_slice(&[0xFF; 4]);
        }
        let mut hash = wp_hash::md5(&input);
        if self.revision >= 3 {
            for _ in 0..50 {
                hash = wp_hash::md5(&hash[..self.length]);
            }
        }
        hash[..self.length].to_vec()
    }

    /// The key, if the password is the user's: algorithms 4 and 5, the
    /// user entry made again and compared.
    fn try_user(&self, password: &[u8]) -> Option<Vec<u8>> {
        let key = self.key(password);
        let matches = if self.revision == 2 {
            wp_cipher::rc4(&key, &PAD) == self.user.get(..32)?
        } else {
            let mut input = PAD.to_vec();
            input.extend_from_slice(self.first_id);
            let mut value = wp_cipher::rc4(&key, &wp_hash::md5(&input));
            for round in 1..=19u8 {
                let round_key: Vec<u8> = key.iter().map(|b| b ^ round).collect();
                value = wp_cipher::rc4(&round_key, &value);
            }
            value[..16] == *self.user.get(..16)?
        };
        matches.then_some(key)
    }

    /// The user's password the owner's unlocks, algorithm 7.
    fn user_password_from_owner(&self, password: &[u8]) -> Vec<u8> {
        let mut hash = wp_hash::md5(&padded(password));
        if self.revision >= 3 {
            for _ in 0..50 {
                hash = wp_hash::md5(&hash);
            }
        }
        let key = &hash[..self.length];
        let mut value = self.owner.get(..32).unwrap_or(self.owner).to_vec();
        if self.revision == 2 {
            value = wp_cipher::rc4(key, &value);
        } else {
            for round in (0..=19u8).rev() {
                let round_key: Vec<u8> = key.iter().map(|b| b ^ round).collect();
                value = wp_cipher::rc4(&round_key, &value);
            }
        }
        value
    }
}

/// Revisions 5 and 6: the file's key unlocked with a hash of the password,
/// as the owner or as the user, [ISO 32000-2] algorithms 2.A, 11 and 12.
fn key_from_hash(
    encrypt: &Dictionary,
    revision: i64,
    password: &str,
    owner: &[u8],
    user: &[u8],
) -> Result<Vec<u8>, Refused> {
    let bytes = |key: &str| match encrypt.get(key) {
        Some(Object::String(bytes)) => bytes.clone(),
        _ => Vec::new(),
    };
    if owner.len() < 48 || user.len() < 48 {
        return Err(Refused::Unsupported);
    }
    let mut password = password.as_bytes().to_vec();
    password.truncate(127);
    let hash = |salt: &[u8], extra: &[u8]| hash_2b(revision, &password, salt, extra);
    let (unlocking, sealed) = if hash(&owner[32..40], &user[..48]) == owner[..32] {
        (hash(&owner[40..48], &user[..48]), bytes("OE"))
    } else if hash(&user[32..40], &[]) == user[..32] {
        (hash(&user[40..48], &[]), bytes("UE"))
    } else {
        return Err(Refused::Password);
    };
    let key = wp_cipher::Key::new(&unlocking).ok_or(Refused::Unsupported)?;
    let sealed = sealed.get(..32).ok_or(Refused::Unsupported)?;
    Ok(wp_cipher::decrypt_cbc(&key, &[0; 16], sealed))
}

/// The password's hash, algorithm 2.B: plain SHA-256 in revision 5; in 6,
/// rounds of AES and the SHA-2 hashes until the last byte says to stop.
fn hash_2b(revision: i64, password: &[u8], salt: &[u8], extra: &[u8]) -> Vec<u8> {
    let mut input = password.to_vec();
    input.extend_from_slice(salt);
    input.extend_from_slice(extra);
    let mut k = wp_hash::sha256(&input).to_vec();
    if revision < 6 {
        return k;
    }
    let mut round = 0u32;
    loop {
        let mut one = password.to_vec();
        one.extend_from_slice(&k);
        one.extend_from_slice(extra);
        let repeated = one.repeat(64);
        let Some(key) = wp_cipher::Key::new(&k[..16]) else { return k };
        let start: [u8; 16] = k[16..32].try_into().unwrap_or([0; 16]);
        let e = wp_cipher::encrypt_cbc(&key, &start, &repeated);
        let choice = e[..16].iter().map(|&b| u32::from(b)).sum::<u32>() % 3;
        k = match choice {
            0 => wp_hash::sha256(&e).to_vec(),
            1 => wp_hash::sha384(&e).to_vec(),
            _ => wp_hash::sha512(&e).to_vec(),
        };
        round += 1;
        if round >= 64 && u32::from(*e.last().unwrap_or(&0)) + 32 <= round {
            break;
        }
    }
    k.truncate(32);
    k
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_password_is_padded_to_thirty_two_bytes() {
        let out = padded(b"ab");
        assert_eq!(&out[..2], b"ab");
        assert_eq!(&out[2..], &PAD[..30]);
        assert_eq!(padded(&[]), PAD);
    }

    #[test]
    fn an_object_key_takes_its_number_and_generation() {
        let crypt = Crypt {
            key: vec![1, 2, 3, 4, 5],
            strings: Method::Rc4,
            streams: Method::Rc4,
            metadata: true,
        };
        let mut input = vec![1, 2, 3, 4, 5, 7, 0, 0, 0, 0];
        assert_eq!(crypt.object_key(7, 0, false), wp_hash::md5(&input)[..10].to_vec());
        input.extend_from_slice(b"sAlT");
        assert_eq!(crypt.object_key(7, 0, true), wp_hash::md5(&input)[..10].to_vec());
    }
}
