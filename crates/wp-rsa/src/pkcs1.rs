//! The padding a signature is wrapped in, and the two keys it is made with.
//!
//! # What the padding is for
//!
//! A hash is thirty-two bytes and the number a signature is made of is two
//! hundred and fifty-six. Something has to fill the rest, and it cannot be
//! noughts: a number with noughts at the front is a shorter number, and a
//! scheme where the padding could be anything is a scheme where a forged
//! signature can be worked out without the key. So the standard lays down
//! exactly what goes there:
//!
//! ```text
//! 00 01 FF FF … FF 00 <the hash, in a DER wrapper naming which hash it is>
//! ```
//!
//! The wrapper matters as much as the padding. Without it a signature over a
//! SHA-1 hash and a signature over the first twenty bytes of a SHA-256 hash
//! would be the same number, and a program could be told the wrong one.

use crate::big::Big;

/// Which hash a signature is made over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Algorithm {
    Sha1,
    Sha256,
    Sha512,
}

impl Algorithm {
    #[must_use]
    pub fn of(self, message: &[u8]) -> Vec<u8> {
        match self {
            Self::Sha1 => wp_hash::sha1(message).to_vec(),
            Self::Sha256 => wp_hash::sha256(message).to_vec(),
            Self::Sha512 => wp_hash::sha512(message).to_vec(),
        }
    }

    /// The DER that says which hash this is, with the hash's own length left
    /// to be filled in.
    ///
    /// Written out as bytes because that is how the standard writes it: an
    /// appendix of three fixed runs, one for each hash, and a program that
    /// built them from an object identifier would be doing more work to
    /// arrive at the same constants.
    #[must_use]
    fn wrapper(self) -> &'static [u8] {
        match self {
            // SEQUENCE { SEQUENCE { OID 1.3.14.3.2.26, NULL }, OCTET STRING }
            Self::Sha1 => &[
                0x30, 0x21, 0x30, 0x09, 0x06, 0x05, 0x2b, 0x0e, 0x03, 0x02, 0x1a, 0x05, 0x00, 0x04,
                0x14,
            ],
            // The same with 2.16.840.1.101.3.4.2.1.
            Self::Sha256 => &[
                0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02,
                0x01, 0x05, 0x00, 0x04, 0x20,
            ],
            // And 2.16.840.1.101.3.4.2.3.
            Self::Sha512 => &[
                0x30, 0x51, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02,
                0x03, 0x05, 0x00, 0x04, 0x40,
            ],
        }
    }

    /// The name the XML signature standard calls it by.
    #[must_use]
    pub fn uri(self) -> &'static str {
        match self {
            Self::Sha1 => "http://www.w3.org/2000/09/xmldsig#sha1",
            Self::Sha256 => "http://www.w3.org/2001/04/xmlenc#sha256",
            Self::Sha512 => "http://www.w3.org/2001/04/xmlenc#sha512",
        }
    }

    /// And the name it calls a signature made with it by.
    #[must_use]
    pub fn signature_uri(self) -> &'static str {
        match self {
            Self::Sha1 => "http://www.w3.org/2000/09/xmldsig#rsa-sha1",
            Self::Sha256 => "http://www.w3.org/2001/04/xmldsig-more#rsa-sha256",
            Self::Sha512 => "http://www.w3.org/2001/04/xmldsig-more#rsa-sha512",
        }
    }

    /// Whichever hash a name stands for.
    #[must_use]
    pub fn named(uri: &str) -> Option<Self> {
        match uri {
            "http://www.w3.org/2000/09/xmldsig#sha1"
            | "http://www.w3.org/2000/09/xmldsig#rsa-sha1" => Some(Self::Sha1),
            "http://www.w3.org/2001/04/xmlenc#sha256"
            | "http://www.w3.org/2001/04/xmldsig-more#rsa-sha256" => Some(Self::Sha256),
            "http://www.w3.org/2001/04/xmlenc#sha512"
            | "http://www.w3.org/2001/04/xmldsig-more#rsa-sha512" => Some(Self::Sha512),
            _ => None,
        }
    }
}

/// The hash of a message, wrapped as the standard says, in `length` bytes.
fn padded(algorithm: Algorithm, message: &[u8], length: usize) -> Option<Vec<u8>> {
    let hash = algorithm.of(message);
    let wrapper = algorithm.wrapper();
    // Eleven is the least the padding can be: two bytes of marker at the
    // front, one nought between, and at least eight of filler. A key too
    // short for that is a key too short to sign with.
    if length < wrapper.len() + hash.len() + 11 {
        return None;
    }
    let mut out = vec![0xFFu8; length];
    out[0] = 0x00;
    out[1] = 0x01;
    let from = length - wrapper.len() - hash.len();
    out[from - 1] = 0x00;
    out[from..from + wrapper.len()].copy_from_slice(wrapper);
    out[from + wrapper.len()..].copy_from_slice(&hash);
    Some(out)
}

/// The half of a key that is written in a certificate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicKey {
    modulus: Big,
    exponent: Big,
    /// How many bytes a signature made with this key is.
    length: usize,
}

impl PublicKey {
    /// A key from the two numbers a certificate carries, each written most
    /// significant byte first.
    #[must_use]
    pub fn new(modulus: &[u8], exponent: &[u8]) -> Self {
        let modulus = Big::from_be_bytes(modulus);
        let length = modulus.byte_length();
        Self { modulus, exponent: Big::from_be_bytes(exponent), length }
    }

    /// How many bits the key is, which is what a person is shown.
    #[must_use]
    pub fn bits(&self) -> usize {
        self.length * 8
    }

    /// Whether this signature over this message was made with the matching
    /// private key.
    #[must_use]
    pub fn verifies(&self, algorithm: Algorithm, message: &[u8], signature: &[u8]) -> bool {
        if signature.len() != self.length {
            return false;
        }
        let Some(wanted) = padded(algorithm, message, self.length) else { return false };
        let undone = Big::from_be_bytes(signature).power_modulo(&self.exponent, &self.modulus);
        undone.to_be_bytes(self.length) == wanted
    }
}

/// The half that is kept.
#[derive(Clone, PartialEq, Eq)]
pub struct PrivateKey {
    modulus: Big,
    exponent: Big,
    length: usize,
}

impl core::fmt::Debug for PrivateKey {
    /// How long the key is, and not the key. A private exponent written into
    /// a log is a key somebody else has.
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "A private key of {} bits", self.length * 8)
    }
}

impl PrivateKey {
    #[must_use]
    pub fn new(modulus: &[u8], exponent: &[u8]) -> Self {
        let modulus = Big::from_be_bytes(modulus);
        let length = modulus.byte_length();
        Self { modulus, exponent: Big::from_be_bytes(exponent), length }
    }

    /// The signature over a message.
    ///
    /// `None` where the key is too short for the hash, which is the one thing
    /// that can go wrong here.
    #[must_use]
    pub fn sign(&self, algorithm: Algorithm, message: &[u8]) -> Option<Vec<u8>> {
        let padded = padded(algorithm, message, self.length)?;
        Some(
            Big::from_be_bytes(&padded)
                .power_modulo(&self.exponent, &self.modulus)
                .to_be_bytes(self.length),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_padding_is_the_shape_the_standard_lays_down() {
        let padded = padded(Algorithm::Sha256, b"anything", 256).expect("room for it");
        assert_eq!(padded.len(), 256);
        assert_eq!(&padded[..2], &[0x00, 0x01]);
        // Filler up to the nought, and then the wrapper and the hash.
        let nought = padded.iter().position(|byte| *byte == 0 && *byte != padded[0]);
        let _ = nought;
        let from = 256 - Algorithm::Sha256.wrapper().len() - 32;
        assert!(padded[2..from - 1].iter().all(|byte| *byte == 0xFF), "the filler is not filler");
        assert_eq!(padded[from - 1], 0x00);
        assert_eq!(&padded[from..from + 19], Algorithm::Sha256.wrapper());
        assert_eq!(&padded[from + 19..], &wp_hash::sha256(b"anything")[..]);
    }

    #[test]
    fn a_key_too_short_for_the_hash_cannot_sign_with_it() {
        // Sixty-four bits, which is not a key anybody would use and is a
        // useful thing to refuse rather than to write nonsense into.
        assert!(padded(Algorithm::Sha256, b"x", 8).is_none());
        // A SHA-512 signature needs the wrapper, the hash and eleven bytes
        // of padding, which comes to ninety-four.
        assert!(padded(Algorithm::Sha512, b"x", 93).is_none());
        assert!(padded(Algorithm::Sha512, b"x", 94).is_some());
    }

    /// A key small enough to work out by hand, so that the arithmetic can be
    /// held to something other than itself.
    ///
    /// Two primes, 61 and 53, which is the example every account of RSA uses:
    /// the modulus is 3233, the public exponent 17 and the private 413.
    #[test]
    fn the_textbook_key_signs_and_checks() {
        let modulus = 3233u32.to_be_bytes();
        let public = PublicKey::new(&modulus, &17u32.to_be_bytes());
        let private = PrivateKey::new(&modulus, &413u32.to_be_bytes());

        // Too short to pad a real hash into, so the arithmetic is checked
        // directly: a number raised to one exponent and then the other comes
        // back as itself.
        let message = Big::from_be_bytes(&[65]);
        let sealed = message.power_modulo(
            &Big::from_be_bytes(&413u32.to_be_bytes()),
            &Big::from_be_bytes(&modulus),
        );
        let back = sealed
            .power_modulo(&Big::from_be_bytes(&17u32.to_be_bytes()), &Big::from_be_bytes(&modulus));
        assert_eq!(back, message, "the two exponents do not undo each other");
        assert!(private.sign(Algorithm::Sha1, b"x").is_none(), "the key is far too short");
        assert!(!public.verifies(Algorithm::Sha1, b"x", &[0; 2]));
    }

    #[test]
    fn a_name_for_a_hash_and_back_again() {
        for algorithm in [Algorithm::Sha1, Algorithm::Sha256, Algorithm::Sha512] {
            assert_eq!(Algorithm::named(algorithm.uri()), Some(algorithm));
            assert_eq!(Algorithm::named(algorithm.signature_uri()), Some(algorithm));
        }
        assert_eq!(Algorithm::named("http://example.com/whatever"), None);
    }
}
