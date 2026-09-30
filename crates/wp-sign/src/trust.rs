//! Whether the certificate that signed a document is one to be trusted.
//!
//! # The question this answers, and the one it does not
//!
//! [`crate::package`] answers "is this signature the signature of this
//! certificate, over this document". That is arithmetic, and it comes out the
//! same everywhere. It is also not what a person wants to know, because
//! anybody can make a certificate saying anything: a signature that checks out
//! against a certificate reading "Microsoft Corporation" proves only that
//! whoever signed had the key of a certificate with those words in it.
//!
//! What makes a certificate worth anything is that somebody the machine
//! already trusts issued it, or issued the one that issued it, and so on up to
//! a root the machine was told about by whoever set it up. That is the chain,
//! and this module builds it and checks it.
//!
//! # What a link is
//!
//! A certificate carries the part of itself the issuer signed, the issuer's
//! signature over that part, and the issuer's name. The link holds when:
//!
//! * the issuer's name in the child equals the subject's name in the parent,
//!   compared as the bytes they were written in and not as the words they
//!   print to;
//! * the parent's key verifies the child's signature over the child's own
//!   signed part; and
//! * the parent says it may issue — `basicConstraints` with `cA` set. Without
//!   that check, anybody with any certificate could issue any other, which
//!   would make the whole chain worth nothing.
//!
//! A chain ends at a certificate that is in the trusted list. A certificate
//! that is itself in the list is trusted whatever it says about itself, which
//! is what being a root means.
//!
//! # What is deliberately not done here
//!
//! **Revocation.** Whether a certificate has been taken back since it was
//! issued cannot be answered from the certificate: it means asking the issuer,
//! over a network, through CRL or OCSP. This program does not, and says so
//! rather than implying it did — see [`Trust::Trusted`], which says the chain
//! reached a root and nothing about what has happened since.
//!
//! **Name constraints, policies, and the rest of the path rules.** RFC 5280
//! has a page of them. What is here is the three that carry the weight.

use wp_asn1::Certificate;
use wp_rsa::{Algorithm, PublicKey};

/// How far a certificate got.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Trust {
    /// The chain reached a certificate the machine trusts, which is named so
    /// that a person can see whose word they are taking.
    ///
    /// Says nothing about whether the certificate has been revoked since it
    /// was issued: see the module's own documentation.
    Trusted(String),
    /// Every link held, and the top of the chain is not one the machine
    /// trusts. The name of the certificate the chain ended at, which is who
    /// would have to be trusted for this to be.
    Unknown(String),
    /// A certificate in the chain was not valid at the moment asked about.
    /// The name, and the two dates.
    Expired(String, String, String),
    /// The chain is broken: a link whose signature does not come out, or
    /// whose issuer is not allowed to issue. The name of the certificate the
    /// chain broke at, and which of the two it was.
    Broken(String, Fault),
}

/// Why a link did not hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// The issuer's key does not verify the signature on this certificate.
    Signature,
    /// The issuer does not say it may issue certificates.
    NotAnAuthority,
    /// The hash the issuer signed with is not one this program has.
    UnknownHash,
}

impl Trust {
    /// Whether the chain reached a root the machine trusts.
    #[must_use]
    pub fn is_trusted(&self) -> bool {
        matches!(self, Self::Trusted(_))
    }

    /// What to tell a person, in one line.
    #[must_use]
    pub fn said(&self) -> String {
        match self {
            Self::Trusted(root) => format!("Issued under {root}, which this machine trusts"),
            Self::Unknown(top) => {
                format!("Nothing on this machine vouches for {top}")
            }
            Self::Expired(name, from, to) => {
                format!("{name} was only valid from {from} to {to}")
            }
            Self::Broken(name, Fault::Signature) => {
                format!("The signature on {name} does not come out")
            }
            Self::Broken(name, Fault::NotAnAuthority) => {
                format!("{name} was issued by somebody who may not issue certificates")
            }
            Self::Broken(name, Fault::UnknownHash) => {
                format!("{name} was signed with a hash this program has not got")
            }
        }
    }
}

/// How many certificates a chain may be before it is taken to be a ring.
///
/// Nothing real is anywhere near this. Two self-issued certificates naming
/// each other would otherwise be walked for ever.
const DEEPEST: usize = 16;

/// Follows a certificate up to a root, and says how far it got.
///
/// `others` are the certificates that came with the document, which is where
/// the middle of a chain usually is; `roots` are the ones the machine trusts.
/// `moment` is when the chain is being judged, written `YYYY-MM-DDTHH:MM:SSZ`
/// as [`Certificate::covers`] wants it — the present, usually, and the moment
/// a document was signed when that is known.
#[must_use]
pub fn chain(
    leaf: &Certificate,
    others: &[Certificate],
    roots: &[Certificate],
    moment: &str,
) -> Trust {
    let mut current = leaf.clone();
    let mut walked = 0usize;

    loop {
        if !current.covers(moment) {
            return Trust::Expired(
                current.subject.clone(),
                current.not_before.clone(),
                current.not_after.clone(),
            );
        }
        // A certificate the machine trusts is the end of it, wherever in the
        // chain it turns up: that is what being trusted means.
        if let Some(root) = roots.iter().find(|root| root.der == current.der) {
            return Trust::Trusted(root.subject.clone());
        }

        walked += 1;
        if walked > DEEPEST {
            return Trust::Unknown(current.subject.clone());
        }

        // Its issuer, wherever it is: among the roots first, since a chain
        // that can end sooner should.
        let Some(parent) = roots
            .iter()
            .chain(others.iter())
            .find(|other| other.subject_der == current.issuer_der && other.der != current.der)
        else {
            return Trust::Unknown(current.subject.clone());
        };

        if !parent.authority {
            return Trust::Broken(current.subject.clone(), Fault::NotAnAuthority);
        }
        match verify(&current, parent) {
            Ok(true) => {}
            Ok(false) => return Trust::Broken(current.subject.clone(), Fault::Signature),
            Err(fault) => return Trust::Broken(current.subject.clone(), fault),
        }
        current = parent.clone();
    }
}

/// Whether the parent's key made the signature on the child.
fn verify(child: &Certificate, parent: &Certificate) -> Result<bool, Fault> {
    let algorithm = hash_of(&child.signature_algorithm).ok_or(Fault::UnknownHash)?;
    let key = PublicKey::new(&parent.key.modulus, &parent.key.exponent);
    Ok(key.verifies(algorithm, &child.signed_part, &child.signature))
}

/// What one certificate is known by: the SHA-256 of it exactly as it was
/// written, in hex.
///
/// What a trusted publisher is remembered as, and matched by. The name on a
/// certificate is whatever its maker wrote there, and anybody can make a key
/// and a certificate with any name on it; a signature that holds proves only
/// that it was made with the key of the certificate beside it. The
/// certificate itself, byte for byte, is one thing and nobody else's, and
/// this names it. It is the same idea as the thumbprint Windows shows for a
/// certificate and keeps its Trusted Publishers by, with a hash nobody has
/// broken.
#[must_use]
pub fn fingerprint(certificate: &Certificate) -> String {
    wp_hash::to_hex(&wp_hash::sha256(&certificate.der))
}

/// Which hash an object identifier stands for.
///
/// The three that appear on certificates in use. MD5 and the rest are left
/// out on purpose: a program that checked an MD5 signature would be saying a
/// chain holds when anybody can forge a link of it.
#[must_use]
pub fn hash_of(identifier: &str) -> Option<Algorithm> {
    match identifier {
        // sha1WithRSAEncryption
        "1.2.840.113549.1.1.5" => Some(Algorithm::Sha1),
        // sha256WithRSAEncryption
        "1.2.840.113549.1.1.11" => Some(Algorithm::Sha256),
        // sha512WithRSAEncryption
        "1.2.840.113549.1.1.13" => Some(Algorithm::Sha512),
        _ => None,
    }
}
