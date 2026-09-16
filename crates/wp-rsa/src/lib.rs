//! Numbers too big for a machine word, and the signature scheme built on them.
//!
//! # What a signature is
//!
//! A number. The document is hashed; the hash is wrapped in a fixed padding
//! the standard lays down; the result is read as one enormous number and
//! raised to the power of the private exponent, modulo the key's modulus.
//! Checking it is the same thing with the public exponent, which undoes the
//! first: what comes back is the padded hash, and the hash inside it is
//! compared with the hash of the document as it is now.
//!
//! That is the whole of RSA. Everything difficult about it is in making the
//! key, which this does not do — the keys here come from elsewhere, out of a
//! certificate or a key file.
//!
//! # What is here and what is not
//!
//! PKCS#1 version one point five, which is what every signed `.docx` uses and
//! what the XML signature standard names. Not the newer padding, PSS, which
//! Office does not write; not encryption with an RSA key, which nothing in
//! this program needs; and not key generation.
//!
//! # What this defends and what it does not
//!
//! Checking a signature is arithmetic over numbers that are public: the
//! document, the hash, the signature and the certificate are all in the file
//! for anybody. There is nothing to leak, so nothing here is written to hide
//! how long it took.
//!
//! Making one is different: the private exponent is a secret, and
//! [`Big::power_modulo`] takes a length of time that depends on its bits. On
//! a machine where somebody else can measure that, this would give the key
//! away. That is a real limitation and it is written down rather than papered
//! over: what this is built for is a person signing their own document on
//! their own machine.

mod big;
mod pkcs1;

pub use big::Big;
pub use pkcs1::{Algorithm, PrivateKey, PublicKey};
