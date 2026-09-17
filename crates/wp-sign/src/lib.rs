//! The XML signature a signed `.docx` carries, read and written.
//!
//! # What a signature over a document is
//!
//! A `.docx` is a zip of parts, and what is signed is not the zip: the
//! zip's own bytes change every time a program writes one. What is signed
//! is a list of every part with the hash of its bytes, and the signature is
//! over that list. See [`package`].
//!
//! # The two halves of it
//!
//! [`trust`] is the other question a signature raises, and the one that is
//! not arithmetic: whether the certificate that made it is one to be trusted.
//!
//! [`c14n`] is the one way of writing a piece of XML so that it can be
//! hashed at all, and [`package`] is the signature itself. The arithmetic
//! is elsewhere: [`wp_rsa`] for the signing, [`wp_hash`] for the hashes,
//! [`wp_asn1`] for the certificate.

pub mod c14n;
pub mod package;
pub mod trust;

pub use package::{
    countersign, is_signed, sign, signatures, unsign, Counter, Signature, Signer, Signs, Standing,
};
pub use trust::{chain, Trust};
