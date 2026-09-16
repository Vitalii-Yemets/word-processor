//! DER, and the certificates and keys written in it.
//!
//! # What DER is
//!
//! A way of writing structured data as bytes, and the one every piece of
//! cryptography in the world outside this program is written in. Everything
//! is a tag saying what it is, a length, and that many bytes; a sequence is a
//! tag whose bytes are more of the same. Nothing is optional about the
//! encoding — hence the D, for distinguished: there is exactly one way to
//! write a given value, which is what lets a certificate be signed.
//!
//! # Why this is here
//!
//! Because a signed document carries a certificate, and a certificate is a
//! DER structure holding the public key the signature has to be checked
//! against, the name of whoever signed, and the dates it is good between.
//! None of that can be got at without reading DER.
//!
//! # What is read and what is not
//!
//! Enough of a certificate to say who signed and to check the signature: the
//! subject and issuer names, the dates, the serial number and the RSA public
//! key. And enough of a key file to sign with: the modulus and the private
//! exponent, out of either of the two shapes such a file comes in.
//!
//! What is **not** here is deciding whether a certificate is to be trusted.
//! This reads what one says — who it is about, who says so, between which
//! dates, and the bytes that prove it — and [`wp_sign::trust`] puts those
//! together into a chain. Where the roots of that chain come from is the
//! machine's own affair, and is asked of the system: a list of trusted
//! issuers written into this program would be a trust nobody granted.
//! Revocation is not here either: asking whether a certificate has been
//! taken back means asking somebody over a network.

#![forbid(unsafe_code)]

/// One value: what it is, and the bytes inside it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Value<'a> {
    pub tag: u8,
    pub bytes: &'a [u8],
    /// The value with its tag and length still on, for the places where what
    /// is signed is the bytes as they were written.
    pub whole: &'a [u8],
}

/// Reads one value from the front of some bytes, and says what is left.
#[must_use]
pub fn read(bytes: &[u8]) -> Option<(Value<'_>, &[u8])> {
    let tag = *bytes.first()?;
    let first = *bytes.get(1)?;
    let (length, from) = if first < 0x80 {
        (usize::from(first), 2)
    } else {
        // A long length says how many bytes the length itself takes.
        let count = usize::from(first & 0x7F);
        if count == 0 || count > 4 {
            return None;
        }
        let mut length = 0usize;
        for index in 0..count {
            length = (length << 8) | usize::from(*bytes.get(2 + index)?);
        }
        (length, 2 + count)
    };
    let value = bytes.get(from..from + length)?;
    Some((Value { tag, bytes: value, whole: &bytes[..from + length] }, &bytes[from + length..]))
}

/// Every value inside a sequence or a set.
#[must_use]
pub fn children<'a>(value: &Value<'a>) -> Vec<Value<'a>> {
    let mut out = Vec::new();
    let mut left = value.bytes;
    while let Some((child, rest)) = read(left) {
        out.push(child);
        left = rest;
    }
    out
}

/// The tags this reads.
pub const INTEGER: u8 = 0x02;
pub const BIT_STRING: u8 = 0x03;
pub const OCTET_STRING: u8 = 0x04;
pub const OBJECT_ID: u8 = 0x06;
pub const SEQUENCE: u8 = 0x30;
pub const SET: u8 = 0x31;

/// An unsigned number as the bytes it is written in.
///
/// DER writes a number that would start with a bit set with a nought in front
/// of it, so that it reads as positive. That nought is not part of the
/// number, and a program that kept it would have a modulus one byte too long.
#[must_use]
pub fn unsigned<'a>(value: &Value<'a>) -> &'a [u8] {
    let mut bytes = value.bytes;
    while bytes.first() == Some(&0) && bytes.len() > 1 {
        bytes = &bytes[1..];
    }
    bytes
}

/// What is inside a bit string, past the byte that says how many bits of the
/// last byte are padding.
#[must_use]
pub fn bits<'a>(value: &Value<'a>) -> &'a [u8] {
    value.bytes.get(1..).unwrap_or_default()
}

/// The two numbers of an RSA public key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RsaPublicKey {
    pub modulus: Vec<u8>,
    pub exponent: Vec<u8>,
}

/// The two numbers needed to sign with one.
#[derive(Clone, PartialEq, Eq)]
pub struct RsaPrivateKey {
    pub modulus: Vec<u8>,
    pub exponent: Vec<u8>,
}

impl core::fmt::Debug for RsaPrivateKey {
    /// How long it is, and not what it is.
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "A private key of {} bits", self.modulus.len() * 8)
    }
}

/// A certificate, as far as this program reads one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Certificate {
    /// Who the certificate is about, written the way a person reads it.
    pub subject: String,
    /// And who says so.
    pub issuer: String,
    /// The two dates it is good between, as `YYYY-MM-DDTHH:MM:SSZ`.
    pub not_before: String,
    pub not_after: String,
    /// The number the issuer knows it by, in hex.
    pub serial: String,
    pub key: RsaPublicKey,
    /// The whole thing as it was written, which is what goes into a document.
    pub der: Vec<u8>,
    /// The part of it the issuer signed, exactly as it was written.
    ///
    /// Kept as bytes rather than rebuilt from what was read: a signature is
    /// over the bytes, and a program that re-encoded what it understood would
    /// be checking a signature over something else.
    pub signed_part: Vec<u8>,
    /// The issuer's signature over that part.
    pub signature: Vec<u8>,
    /// Which hash the issuer signed with, by its object identifier.
    pub signature_algorithm: String,
    /// Whether the certificate says it may issue others: `basicConstraints`
    /// with `cA` set. A certificate that does not say so cannot stand in the
    /// middle of a chain, and a program that let one would let anybody with
    /// any certificate issue any other.
    pub authority: bool,
    /// Who it is about and who says so, as the bytes they were written in.
    ///
    /// The printed forms are for a person to read; these are what one
    /// certificate is matched to another by, because two names that print the
    /// same may be written differently and the issuer's name in a child is
    /// the subject's name in its parent byte for byte.
    pub subject_der: Vec<u8>,
    pub issuer_der: Vec<u8>,
}

impl Certificate {
    /// Reads one.
    #[must_use]
    pub fn read(der: &[u8]) -> Option<Self> {
        let (whole, _) = read(der)?;
        if whole.tag != SEQUENCE {
            return None;
        }
        let parts = children(&whole);
        let tbs = parts.first()?;
        let inside = children(tbs);

        // The version is optional and tagged, and everything after it shifts
        // by one when it is there. Every certificate written this century has
        // it; a certificate without it is version one and still has to read.
        let versioned = usize::from(inside.first().is_some_and(|value| value.tag == 0xA0));
        let serial = inside.get(versioned)?;
        let issuer = inside.get(versioned + 2)?;
        let validity = children(inside.get(versioned + 3)?);
        let subject = inside.get(versioned + 4)?;
        let key = inside.get(versioned + 5)?;

        // The two after the signed part: what the issuer signed with, and the
        // signature itself.
        let algorithm = children(parts.get(1)?);
        let signature = parts.get(2)?;

        Some(Self {
            subject: name(subject),
            issuer: name(issuer),
            not_before: time(validity.first()?),
            not_after: time(validity.get(1)?),
            serial: unsigned(serial).iter().map(|byte| format!("{byte:02x}")).collect(),
            key: public_key(key)?,
            der: der.to_vec(),
            signed_part: tbs.whole.to_vec(),
            signature: bits(signature).to_vec(),
            signature_algorithm: algorithm.first().map(object_id).unwrap_or_default(),
            authority: is_authority(&inside[versioned + 6..]),
            subject_der: subject.whole.to_vec(),
            issuer_der: issuer.whole.to_vec(),
        })
    }

    /// Whether a moment is inside the dates the certificate is good between.
    ///
    /// Both written the same way, so comparing the text is comparing the
    /// dates: that is what a date written year first is for.
    #[must_use]
    pub fn covers(&self, moment: &str) -> bool {
        moment >= self.not_before.as_str() && moment <= self.not_after.as_str()
    }
}

/// An object identifier written the way people write one: numbers with dots.
///
/// The first byte holds two numbers at once, which is the one oddity of the
/// encoding: forty times the first plus the second. After that each number is
/// seven bits at a time, most significant first, with the top bit set on
/// every byte but the last.
#[must_use]
pub fn object_id(value: &Value<'_>) -> String {
    if value.tag != OBJECT_ID || value.bytes.is_empty() {
        return String::new();
    }
    let first = u32::from(value.bytes[0]);
    let mut out = format!("{}.{}", first / 40, first % 40);
    let mut number = 0u32;
    for byte in &value.bytes[1..] {
        number = (number << 7) | u32::from(byte & 0x7F);
        if byte & 0x80 == 0 {
            out.push('.');
            out.push_str(&number.to_string());
            number = 0;
        }
    }
    out
}

/// `basicConstraints` with `cA` set, which is a certificate saying it may
/// issue others.
///
/// The extensions are the last thing in the signed part, inside a tag of
/// their own. Each is an identifier, an optional "you must understand this"
/// flag, and the extension itself wrapped in an octet string.
fn is_authority(rest: &[Value<'_>]) -> bool {
    const BASIC_CONSTRAINTS: &str = "2.5.29.19";
    const BOOLEAN: u8 = 0x01;

    let Some(tagged) = rest.iter().find(|value| value.tag == 0xA3) else { return false };
    let Some(list) = children(tagged).into_iter().next() else { return false };
    for extension in children(&list) {
        let parts = children(&extension);
        let Some(identifier) = parts.first() else { continue };
        if object_id(identifier) != BASIC_CONSTRAINTS {
            continue;
        }
        let Some(wrapped) = parts.last() else { continue };
        let Some((inside, _)) = read(wrapped.bytes) else { continue };
        // An empty sequence is `cA` left out, which means false.
        return children(&inside).first().is_some_and(|flag| {
            flag.tag == BOOLEAN && flag.bytes.first().is_some_and(|on| *on != 0)
        });
    }
    false
}

/// The public key out of a `SubjectPublicKeyInfo`.
fn public_key(value: &Value<'_>) -> Option<RsaPublicKey> {
    let parts = children(value);
    let inside = parts.get(1)?;
    if inside.tag != BIT_STRING {
        return None;
    }
    let (sequence, _) = read(bits(inside))?;
    let numbers = children(&sequence);
    Some(RsaPublicKey {
        modulus: unsigned(numbers.first()?).to_vec(),
        exponent: unsigned(numbers.get(1)?).to_vec(),
    })
}

/// A private key out of a key file, in either of the two shapes one comes in.
///
/// The older shape is the key itself: a sequence starting with a version and
/// then the numbers. The newer wraps that in another sequence naming the
/// algorithm, with the old shape inside an octet string. Telling them apart
/// is telling whether the thing after the version is a number or a sequence.
#[must_use]
pub fn private_key(der: &[u8]) -> Option<RsaPrivateKey> {
    let (whole, _) = read(der)?;
    let parts = children(&whole);
    let second = parts.get(1)?;
    if second.tag == SEQUENCE {
        // The newer shape: the key is inside the octet string after it.
        let inside = parts.get(2)?;
        if inside.tag != OCTET_STRING {
            return None;
        }
        return private_key(inside.bytes);
    }
    // The older shape: version, modulus, public exponent, private exponent.
    Some(RsaPrivateKey {
        modulus: unsigned(second).to_vec(),
        exponent: unsigned(parts.get(3)?).to_vec(),
    })
}

/// The public half of a key file, for the places where only that is wanted.
#[must_use]
pub fn public_key_of(der: &[u8]) -> Option<RsaPublicKey> {
    let (whole, _) = read(der)?;
    let parts = children(&whole);
    let second = parts.get(1)?;
    if second.tag == SEQUENCE {
        let inside = parts.get(2)?;
        return public_key_of(inside.bytes);
    }
    Some(RsaPublicKey {
        modulus: unsigned(second).to_vec(),
        exponent: unsigned(parts.get(2)?).to_vec(),
    })
}

/// A name, written the way a person reads one: `CN=A Signer, O=Nobody`.
///
/// The parts are given in the order the certificate writes them, which is
/// most general first — country, then organisation, then the name itself.
/// Every program that shows a certificate turns that round, and so does this.
fn name(value: &Value<'_>) -> String {
    let mut parts = Vec::new();
    for set in children(value) {
        for pair in children(&set) {
            let inside = children(&pair);
            let (Some(kind), Some(text)) = (inside.first(), inside.get(1)) else { continue };
            if kind.tag != OBJECT_ID {
                continue;
            }
            let label = match kind.bytes {
                [0x55, 0x04, 0x03] => "CN",
                [0x55, 0x04, 0x06] => "C",
                [0x55, 0x04, 0x07] => "L",
                [0x55, 0x04, 0x08] => "ST",
                [0x55, 0x04, 0x0A] => "O",
                [0x55, 0x04, 0x0B] => "OU",
                [0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x09, 0x01] => "E",
                _ => continue,
            };
            parts.push(format!("{label}={}", String::from_utf8_lossy(text.bytes)));
        }
    }
    parts.reverse();
    parts.join(", ")
}

/// A date, as `YYYY-MM-DDTHH:MM:SSZ` whichever of the two ways it was
/// written.
///
/// The older way writes the year in two digits, which is a decision from when
/// there was one century to think about. The rule for reading them is the
/// standard's: fifty and over is the nineteen hundreds, under fifty is the
/// two thousands.
fn time(value: &Value<'_>) -> String {
    let text = String::from_utf8_lossy(value.bytes);
    let digits: Vec<char> = text.chars().filter(char::is_ascii_digit).collect();
    let (year, rest) = if digits.len() >= 14 {
        (digits[..4].iter().collect::<String>(), &digits[4..])
    } else if digits.len() >= 12 {
        let two: String = digits[..2].iter().collect();
        let century = if two.as_str() >= "50" { "19" } else { "20" };
        (format!("{century}{two}"), &digits[2..])
    } else {
        return text.into_owned();
    };
    let at = |from: usize| rest.get(from..from + 2).map(|two| two.iter().collect::<String>());
    match (at(0), at(2), at(4), at(6), at(8)) {
        (Some(month), Some(day), Some(hour), Some(minute), Some(second)) => {
            format!("{year}-{month}-{day}T{hour}:{minute}:{second}Z")
        }
        _ => text.into_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_is_a_tag_a_length_and_that_many_bytes() {
        let (value, left) = read(&[0x02, 0x03, 1, 2, 3, 0x05, 0x00]).expect("a value");
        assert_eq!(value.tag, INTEGER);
        assert_eq!(value.bytes, &[1, 2, 3]);
        assert_eq!(value.whole, &[0x02, 0x03, 1, 2, 3]);
        assert_eq!(left, &[0x05, 0x00]);
    }

    #[test]
    fn a_long_length_says_how_many_bytes_it_takes() {
        let mut bytes = vec![0x04, 0x82, 0x01, 0x00];
        bytes.extend(std::iter::repeat_n(7u8, 256));
        let (value, left) = read(&bytes).expect("a value");
        assert_eq!(value.tag, OCTET_STRING);
        assert_eq!(value.bytes.len(), 256);
        assert!(left.is_empty());
    }

    #[test]
    fn a_value_that_runs_off_the_end_is_not_a_value() {
        assert!(read(&[0x30, 0x05, 1, 2]).is_none());
        assert!(read(&[0x30]).is_none());
        assert!(read(&[]).is_none());
        // A length claiming more bytes than a length may take.
        assert!(read(&[0x30, 0x88, 1, 1, 1, 1, 1, 1, 1, 1]).is_none());
    }

    #[test]
    fn a_sequence_is_read_through() {
        let bytes = [0x30, 0x06, 0x02, 0x01, 0x07, 0x02, 0x01, 0x09];
        let (value, _) = read(&bytes).expect("a sequence");
        let inside = children(&value);
        assert_eq!(inside.len(), 2);
        assert_eq!(inside[0].bytes, &[7]);
        assert_eq!(inside[1].bytes, &[9]);
    }

    #[test]
    fn the_nought_that_makes_a_number_positive_is_not_part_of_it() {
        let (value, _) = read(&[0x02, 0x03, 0x00, 0xFF, 0x01]).expect("a number");
        assert_eq!(unsigned(&value), &[0xFF, 0x01]);
        // And a number that really is nought stays one byte.
        let (nought, _) = read(&[0x02, 0x01, 0x00]).expect("a number");
        assert_eq!(unsigned(&nought), &[0x00]);
    }

    #[test]
    fn both_ways_of_writing_a_date() {
        let older = Value { tag: 0x17, bytes: b"240131235959Z", whole: b"" };
        assert_eq!(time(&older), "2024-01-31T23:59:59Z");
        let ancient = Value { tag: 0x17, bytes: b"991231000000Z", whole: b"" };
        assert_eq!(time(&ancient), "1999-12-31T00:00:00Z");
        let newer = Value { tag: 0x18, bytes: b"20991231000000Z", whole: b"" };
        assert_eq!(time(&newer), "2099-12-31T00:00:00Z");
    }
}
