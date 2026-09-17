//! The signature a signed `.docx` carries, read and written.
//!
//! # What signing a package means
//!
//! A `.docx` is a zip of parts, and a signature over it is not a signature
//! over the zip. The zip's own bytes change every time a program writes one —
//! the order of the entries, the compression, the timestamps — and none of
//! that is the document. So what is signed is a list: every part, with the
//! hash of its bytes and the content type it was written under. Change a part
//! and its hash changes; add one and it is not in the list; take one away and
//! the list names a part that is gone.
//!
//! That list is a `Manifest`, and the signature is over the manifest rather
//! than over the parts, which is what lets a program check one reference at a
//! time and say which part was changed.
//!
//! # The two objects
//!
//! `idPackageObject` holds the manifest and the time it was signed.
//! `idOfficeObject` holds what Word shows a person: who signed, and why. Both
//! are hashed and both hashes are inside the signed information, so neither
//! can be changed without the signature failing.
//!
//! # The relationship parts
//!
//! A `.rels` part cannot be signed as it stands. It holds an identifier for
//! each relationship that a program is free to renumber, and the order of the
//! relationships in the file is nobody's business. So the format lays down a
//! transform: take the relationships the signature names, put a `TargetMode`
//! on any that has none, sort them by identifier, and canonicalise that. What
//! is signed is what the relationships *say*, not how they were written down.

use wp_opc::{Package, Relationships};
use wp_rsa::{Algorithm, PrivateKey, PublicKey};
use wp_xml::tree::{Element, XmlTree};

use crate::c14n;

/// The namespaces a signature is written in.
const DSIG: &str = "http://www.w3.org/2000/09/xmldsig#";
const MDSSI: &str = "http://schemas.openxmlformats.org/package/2006/digital-signature";
const OFFICE: &str = "http://schemas.microsoft.com/office/2006/digsig";
/// And the one the signature's own properties are written in: XAdES, the
/// standard that says what a signature claims about itself beyond the
/// arithmetic — when it was made, and by which certificate.
const XADES: &str = "http://uri.etsi.org/01903/v1.3.2#";

/// What a reference to those properties is called, which is how a reader
/// tells them from a reference to a part of the document.
const XADES_SIGNED_PROPERTIES: &str = "http://uri.etsi.org/01903#SignedProperties";

/// And what a reference to the signature being countersigned is called.
const XADES_COUNTERSIGNED: &str = "http://uri.etsi.org/01903#CountersignedSignature";

/// The name a signature gives its own value, so that another signature can
/// point at it.
///
/// A signature does not cover its own value — what it covers is the signed
/// information — so naming the value afterwards takes nothing away from a
/// signature already made. That is what makes countersigning an old signature
/// possible at all.
const SIGNATURE_VALUE_ID: &str = "idSignatureValue";

/// What a signature calls the properties it makes about itself.
const SIGNED_PROPERTIES_ID: &str = "idSignedProperties";

/// The transform that turns a relationship part into what it says.
const RELATIONSHIP_TRANSFORM: &str =
    "http://schemas.openxmlformats.org/package/2006/RelationshipTransform";

/// Where the signatures live.
pub const ORIGIN: &str = "_xmlsignatures/origin.sigs";
const ORIGIN_TYPE: &str = "application/vnd.openxmlformats-package.digital-signature-origin";
/// What kind of part a signature is, which a test putting one back needs.
pub const SIGNATURE_TYPE: &str =
    "application/vnd.openxmlformats-package.digital-signature-xmlsignature+xml";
const ORIGIN_RELATIONSHIP: &str =
    "http://schemas.openxmlformats.org/package/2006/relationships/digital-signature/origin";
const SIGNATURE_RELATIONSHIP: &str =
    "http://schemas.openxmlformats.org/package/2006/relationships/digital-signature/signature";

/// What a signature says about itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signature {
    /// Which part of the package holds it.
    pub part: String,
    /// The certificate of whoever signed.
    pub certificate: wp_asn1::Certificate,
    /// The rest of the certificates the signature carries, which is usually
    /// the chain from the signer up towards a root.
    ///
    /// Kept because they are what [`crate::trust`] needs to follow that
    /// chain: a signature whose middle certificate is only in the file would
    /// otherwise reach nothing.
    pub chain: Vec<wp_asn1::Certificate>,
    /// When they say they signed.
    pub signed_at: String,
    /// What they said about why.
    pub reason: String,
    /// The parts the signature covers, in the order the manifest lists them.
    pub parts: Vec<String>,
    /// The signature line it was made for, or empty where it is about the
    /// document at large.
    pub line: String,
    /// The signatures somebody else made over this one.
    pub counters: Vec<Counter>,
    /// Whether it holds, and what is wrong with it if it does not.
    pub standing: Standing,
}

/// Whether a signature holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Standing {
    /// Everything it covers is as it was, and the signature is the
    /// certificate's.
    Good,
    /// A part it covers is not as it was. Named, because which part changed
    /// is the useful half of the answer.
    Changed(String),
    /// A part it covers is not in the package at all.
    Missing(String),
    /// The arithmetic does not come out: the signature was not made by the
    /// key in the certificate, or something outside the manifest changed.
    Broken,
    /// Written a way this program does not read.
    Unsupported(String),
}

impl Standing {
    #[must_use]
    pub fn is_good(&self) -> bool {
        *self == Self::Good
    }

    /// What a person is told.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Good => "valid".to_owned(),
            Self::Changed(part) => format!("the document has changed: {part}"),
            Self::Missing(part) => format!("a signed part is gone: {part}"),
            Self::Broken => "not the signature of this certificate".to_owned(),
            Self::Unsupported(what) => format!("cannot be checked: {what}"),
        }
    }
}

/// Every signature the package carries.
#[must_use]
pub fn signatures(package: &Package) -> Vec<Signature> {
    let mut out = Vec::new();
    for name in signature_parts(package) {
        if let Some(signature) = read(package, &name) {
            out.push(signature);
        }
    }
    out
}

/// Every `X509Certificate` in a signature, in the order they are written.
fn collect_certificates(element: &Element, out: &mut Vec<wp_asn1::Certificate>) {
    for child in element.child_elements() {
        if child.local_name() == "X509Certificate" {
            let der = wp_text::base64::decode(child.text_content().trim().as_bytes());
            if let Some(certificate) = wp_asn1::Certificate::read(&der) {
                out.push(certificate);
            }
        } else {
            collect_certificates(child, out);
        }
    }
}

/// The names of the parts holding signatures.
fn signature_parts(package: &Package) -> Vec<String> {
    let Ok(relationships) = package.relationships(ORIGIN) else { return Vec::new() };
    let mut out: Vec<String> = relationships
        .all()
        .iter()
        .filter(|relationship| relationship.kind == SIGNATURE_RELATIONSHIP)
        .filter_map(|relationship| relationship.resolved_target(ORIGIN)?.ok())
        .collect();
    out.sort();
    out
}

/// Reads one, and checks it.
fn read(package: &Package, part: &str) -> Option<Signature> {
    let text = package.xml_part(part)?.ok()?;
    let tree = XmlTree::parse(&text).ok()?;
    let root = &tree.root;

    let signed_info = child(root, "SignedInfo")?;
    // Every certificate the signature carries. The first is the signer's -
    // which is where the format puts it - and the rest are the chain.
    let mut carried = Vec::new();
    collect_certificates(root, &mut carried);
    let mut carried = carried.into_iter();
    let certificate = carried.next()?;
    let chain: Vec<wp_asn1::Certificate> = carried.collect();
    let signed_at = find(root, "Value").map(|value| value.text_content()).unwrap_or_default();
    let reason =
        find(root, "SignatureComments").map(|value| value.text_content()).unwrap_or_default();

    let mut parts = Vec::new();
    if let Some(manifest) = find(root, "Manifest") {
        for reference in manifest.child_elements() {
            if let Some(name) = part_of(reference.attribute(None, "URI").unwrap_or_default()) {
                parts.push(name);
            }
        }
    }

    // Which line it was made for. Word writes this as the setup identifier of
    // the signature line, and a signature about the document at large leaves
    // it empty — which is what an absent element amounts to as well.
    let line = find(root, "SetupID").map(|value| value.text_content()).unwrap_or_default();

    let mut counters = Vec::new();
    collect_counters(root, root, &mut counters);

    let standing = check(package, root, signed_info, &certificate);
    Some(Signature {
        part: part.to_owned(),
        certificate,
        chain,
        signed_at,
        reason,
        parts,
        line,
        counters,
        standing,
    })
}

/// Every signature made over this one, wherever it sits.
fn collect_counters(outer: &Element, element: &Element, out: &mut Vec<Counter>) {
    if element.local_name() == "CounterSignature" {
        for signature in element.child_elements() {
            if signature.local_name() != "Signature" {
                continue;
            }
            let mut carried = Vec::new();
            collect_certificates(signature, &mut carried);
            let Some(certificate) = carried.into_iter().next() else { continue };
            let signed_at =
                find(signature, "SigningTime").map(|at| at.text_content()).unwrap_or_default();
            let role =
                find(signature, "ClaimedRole").map(|what| what.text_content()).unwrap_or_default();
            let standing = check_counter(outer, signature, &certificate);
            out.push(Counter { certificate, signed_at, role, standing });
        }
        return;
    }
    for child in element.child_elements() {
        collect_counters(outer, child, out);
    }
}

/// Whether one of them holds.
///
/// The same arithmetic as a signature over a document, over less: there is no
/// manifest, because what a countersignature covers is one element of one
/// file and not a package. What it points at is resolved in the document it
/// sits in — which is the signature it is about — because that is where the
/// value it signed is.
fn check_counter(
    outer: &Element,
    counter: &Element,
    certificate: &wp_asn1::Certificate,
) -> Standing {
    let Some(signed_info) = child(counter, "SignedInfo") else { return Standing::Broken };
    let named = child(signed_info, "CanonicalizationMethod")
        .and_then(|element| element.attribute(None, "Algorithm"))
        .unwrap_or(c14n::NAME);
    if named != c14n::NAME {
        return Standing::Unsupported(named.to_owned());
    }
    let Some(algorithm) = child(signed_info, "SignatureMethod")
        .and_then(|element| element.attribute(None, "Algorithm"))
        .and_then(Algorithm::named)
    else {
        return Standing::Unsupported(String::from("an algorithm this program has not"));
    };

    let scope = c14n::context(&[outer]);
    for reference in signed_info.child_elements().filter(|child| child.local_name() == "Reference")
    {
        let uri = reference.attribute(None, "URI").unwrap_or_default();
        let Some(id) = uri.strip_prefix('#') else {
            return Standing::Unsupported(format!("a reference to {uri}"));
        };
        let Some(target) = by_id(outer, id) else {
            return Standing::Changed(uri.to_owned());
        };
        let Some(wanted) = digest_of(reference) else {
            return Standing::Unsupported(String::from("a digest this program has not"));
        };
        let bytes = c14n::canonical(target, &scope);
        if wanted.0.of(bytes.as_bytes()) != wanted.1 {
            return Standing::Changed(format!("the signature's own {id}"));
        }
    }

    let signed = c14n::canonical(signed_info, &scope);
    let Some(value) = child(counter, "SignatureValue") else { return Standing::Broken };
    let signature = wp_text::base64::decode(value.text_content().trim().as_bytes());
    let key = PublicKey::new(&certificate.key.modulus, &certificate.key.exponent);
    if key.verifies(algorithm, signed.as_bytes(), &signature) {
        Standing::Good
    } else {
        Standing::Broken
    }
}

/// Does the arithmetic.
fn check(
    package: &Package,
    root: &Element,
    signed_info: &Element,
    certificate: &wp_asn1::Certificate,
) -> Standing {
    // The canonicalisation named in the file, which is the only one written.
    let named = child(signed_info, "CanonicalizationMethod")
        .and_then(|element| element.attribute(None, "Algorithm"))
        .unwrap_or(c14n::NAME);
    if named != c14n::NAME {
        return Standing::Unsupported(named.to_owned());
    }
    let Some(algorithm) = child(signed_info, "SignatureMethod")
        .and_then(|element| element.attribute(None, "Algorithm"))
        .and_then(Algorithm::named)
    else {
        return Standing::Unsupported(String::from("an algorithm this program has not"));
    };

    // Every reference inside the signed information points at an object in
    // this same file. Each has to hash to what it says.
    for reference in signed_info.child_elements().filter(|child| child.local_name() == "Reference")
    {
        let uri = reference.attribute(None, "URI").unwrap_or_default();
        let Some(id) = uri.strip_prefix('#') else {
            return Standing::Unsupported(format!("a reference to {uri}"));
        };
        let Some(object) = by_id(root, id) else {
            return Standing::Changed(uri.to_owned());
        };
        let Some(wanted) = digest_of(reference) else {
            return Standing::Unsupported(String::from("a digest this program has not"));
        };
        let scope = c14n::context(&[root]);
        let bytes = c14n::canonical(object, &scope);
        if wanted.0.of(bytes.as_bytes()) != wanted.1 {
            return Standing::Changed(format!("the signature's own {id}"));
        }
    }

    // And every reference inside the manifest points at a part of the
    // package.
    if let Some(manifest) = find(root, "Manifest") {
        for reference in manifest.child_elements() {
            match part_standing(package, reference) {
                Standing::Good => {}
                other => return other,
            }
        }
    }

    // Then the signature itself, over the signed information as it stands.
    let scope = c14n::context(&[root]);
    let signed = c14n::canonical(signed_info, &scope);
    let Some(value) = find(root, "SignatureValue") else { return Standing::Broken };
    let signature = wp_text::base64::decode(value.text_content().trim().as_bytes());
    let key = PublicKey::new(&certificate.key.modulus, &certificate.key.exponent);
    if key.verifies(algorithm, signed.as_bytes(), &signature) {
        Standing::Good
    } else {
        Standing::Broken
    }
}

/// Whether one part is as the manifest says.
fn part_standing(package: &Package, reference: &Element) -> Standing {
    let uri = reference.attribute(None, "URI").unwrap_or_default();
    let Some(name) = part_of(uri) else {
        return Standing::Unsupported(format!("a reference to {uri}"));
    };
    let Some(bytes) = package.part(&name) else {
        return Standing::Missing(name);
    };
    let Some((algorithm, wanted)) = digest_of(reference) else {
        return Standing::Unsupported(String::from("a digest this program has not"));
    };

    let transformed = match transformed(reference, &name, bytes) {
        Ok(bytes) => bytes,
        Err(what) => return Standing::Unsupported(what),
    };
    if algorithm.of(&transformed) == wanted {
        Standing::Good
    } else {
        Standing::Changed(name)
    }
}

/// The bytes a reference actually covers, after whatever transforms it names.
fn transformed(reference: &Element, name: &str, bytes: &[u8]) -> Result<Vec<u8>, String> {
    let Some(transforms) = child(reference, "Transforms") else { return Ok(bytes.to_vec()) };
    let mut out = bytes.to_vec();
    for transform in transforms.child_elements() {
        let algorithm = transform.attribute(None, "Algorithm").unwrap_or_default();
        match algorithm {
            RELATIONSHIP_TRANSFORM => {
                let text = String::from_utf8(out)
                    .map_err(|_| String::from("a relationship part that is not text"))?;
                let kept: Vec<String> = transform
                    .child_elements()
                    .filter(|child| child.local_name() == "RelationshipReference")
                    .filter_map(|child| child.attribute(None, "SourceId"))
                    .map(str::to_owned)
                    .collect();
                out = relationships_as_signed(name, &text, &kept)?.into_bytes();
            }
            c14n::NAME => {
                let text = String::from_utf8(out)
                    .map_err(|_| String::from("a part that is not text being canonicalised"))?;
                let tree = XmlTree::parse(&text)
                    .map_err(|_| String::from("a part that is not XML being canonicalised"))?;
                out = c14n::canonical(&tree.root, &[]).into_bytes();
            }
            other => return Err(format!("the transform {other}")),
        }
    }
    Ok(out)
}

/// A relationship part as the format says to sign it.
///
/// Only the relationships named, each with a `TargetMode` whether it had one
/// or not, in the order of their identifiers. Nothing else about the file
/// survives: not the order it was written in, not the identifiers of the
/// relationships that were left out.
pub fn relationships_as_signed(name: &str, xml: &str, kept: &[String]) -> Result<String, String> {
    let relationships = Relationships::parse(name, xml)
        .map_err(|_| String::from("a relationship part that will not read"))?;
    let mut chosen: Vec<_> = relationships
        .all()
        .iter()
        .filter(|relationship| kept.is_empty() || kept.contains(&relationship.id))
        .collect();
    chosen.sort_by(|left, right| left.id.cmp(&right.id));

    let mut out = String::from(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
    );
    for relationship in chosen {
        let mode = match relationship.mode {
            wp_opc::TargetMode::External => "External",
            wp_opc::TargetMode::Internal => "Internal",
        };
        out.push_str(&format!(
            r#"<Relationship Id="{}" Type="{}" Target="{}" TargetMode="{mode}"></Relationship>"#,
            escaped(&relationship.id),
            escaped(&relationship.kind),
            escaped(&relationship.target),
        ));
    }
    out.push_str("</Relationships>");
    Ok(out)
}

fn escaped(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('"', "&quot;")
}

/// The digest a reference asks for, and the value it says.
fn digest_of(reference: &Element) -> Option<(Algorithm, Vec<u8>)> {
    let algorithm = Algorithm::named(
        child(reference, "DigestMethod")?.attribute(None, "Algorithm").unwrap_or_default(),
    )?;
    let value = child(reference, "DigestValue")?.text_content();
    Some((algorithm, wp_text::base64::decode(value.trim().as_bytes())))
}

/// The part a manifest's URI names, without the content type after it.
fn part_of(uri: &str) -> Option<String> {
    let name = uri.split('?').next()?;
    Some(name.trim_start_matches('/').to_owned())
}

/// A child by its local name.
fn child<'a>(element: &'a Element, local: &str) -> Option<&'a Element> {
    element.child_elements().find(|child| child.local_name() == local)
}

/// An element anywhere under this one, by its local name.
fn find<'a>(element: &'a Element, local: &str) -> Option<&'a Element> {
    if element.local_name() == local {
        return Some(element);
    }
    element.child_elements().find_map(|child| find(child, local))
}

/// An element anywhere under this one, by its `Id`.
fn by_id<'a>(element: &'a Element, id: &str) -> Option<&'a Element> {
    if element.attribute(None, "Id") == Some(id) {
        return Some(element);
    }
    element.child_elements().find_map(|child| by_id(child, id))
}

/// Something that can turn a message into a signature.
///
/// # Why this is not simply a key
///
/// Because on Windows the key is not something this program can have. A
/// person's certificates live in a store the system keeps, and the system
/// signs on their behalf without ever handing the key out — which is the
/// point of keeping it there, and is the only way a key on a smart card or in
/// a TPM can be used at all. What the program sends is the bytes to be
/// signed; what comes back is the signature.
///
/// So what signing needs is not a key but something that will sign. A key
/// this program read out of a file is one of those, the system is another,
/// and neither has to know about the other.
pub trait Signs {
    /// Signs a message, or nothing where the key would not or could not.
    fn sign(&self, algorithm: Algorithm, message: &[u8]) -> Option<Vec<u8>>;
}

/// A key this program can read is the simple case: it does the arithmetic
/// itself.
impl Signs for PrivateKey {
    fn sign(&self, algorithm: Algorithm, message: &[u8]) -> Option<Vec<u8>> {
        PrivateKey::sign(self, algorithm, message)
    }
}

/// Who is signing, and with what.
pub struct Signer {
    /// Their certificate, as it was written.
    pub certificate: Vec<u8>,
    /// The certificates between theirs and a root, if they have them.
    ///
    /// Written into the signature after their own, which is what lets
    /// somebody else follow the chain: a reader has the signer's certificate
    /// and the roots its machine trusts, and everything in between has to
    /// come with the document.
    pub chain: Vec<Vec<u8>>,
    /// Whatever will do the signing: a key out of a file, or the system.
    pub key: Box<dyn Signs>,
    /// What they say about why, which Word shows.
    pub reason: String,
    /// When, as `YYYY-MM-DDThh:mm:ssZ`.
    pub at: String,
    /// The signature line this signature is for, where it is for one.
    ///
    /// Empty is a signature about the document at large, which is what Word's
    /// Add a Digital Signature makes. A name here is a signature about a
    /// particular place in the document, and it is the identifier the line
    /// carries — see [`wp_docx::signature`]. The format calls it the setup
    /// identifier, and a reader that knows the line can pair the two.
    pub line: String,
}

impl core::fmt::Debug for Signer {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "A signer with a certificate of {} bytes", self.certificate.len())
    }
}

/// What a signature says about itself, in the shape XAdES gives it.
///
/// # Why this is worth writing
///
/// A signature on its own proves that whoever held a key signed some bytes.
/// It does not say *when*, and it does not say *which certificate* — the
/// certificate travels beside the signature and could be swapped for another
/// with the same key. XAdES is the standard answer to both: the signing time
/// and a digest of the signer's own certificate, written down and signed
/// along with everything else, so that neither can be changed by anybody who
/// has not got the key.
///
/// The time is the signer's own claim and nothing more. Making it worth more
/// than a claim means a timestamp from somebody else, which means a
/// timestamp authority, which means a network — and that is not here.
fn signed_properties(
    signer: &Signer,
    algorithm: Algorithm,
    id: &str,
    role: Option<&str>,
) -> String {
    let digest = algorithm.of(&signer.certificate);
    // The issuer and the number they know the certificate by, which is how
    // the standard names a certificate without carrying it.
    let (issuer, serial) = match wp_asn1::Certificate::read(&signer.certificate) {
        Some(certificate) => (certificate.issuer.clone(), decimal_of_hex(&certificate.serial)),
        // A certificate this program cannot read is still a certificate a key
        // signed with. Saying nothing about its issuer is worse than the
        // alternative only if the alternative is inventing one.
        None => (String::new(), String::from("0")),
    };

    format!(
        concat!(
            r#"<xd:SignedProperties xmlns="{dsig}" xmlns:xd="{xades}""#,
            r#" Id="{id}"><xd:SignedSignatureProperties>"#,
            r#"<xd:SigningTime>{at}</xd:SigningTime>"#,
            r#"<xd:SigningCertificate><xd:Cert><xd:CertDigest>"#,
            r#"<DigestMethod Algorithm="{hash}"></DigestMethod>"#,
            r#"<DigestValue>{digest}</DigestValue></xd:CertDigest>"#,
            r#"<xd:IssuerSerial><X509IssuerName>{issuer}</X509IssuerName>"#,
            r#"<X509SerialNumber>{serial}</X509SerialNumber></xd:IssuerSerial>"#,
            r#"</xd:Cert></xd:SigningCertificate>"#,
            // No policy: a signature made by a person about their own document
            // is not made under anybody's rules, and saying it was would be
            // saying something untrue.
            r#"<xd:SignaturePolicyIdentifier><xd:SignaturePolicyImplied>"#,
            r#"</xd:SignaturePolicyImplied></xd:SignaturePolicyIdentifier>{role}"#,
            r#"</xd:SignedSignatureProperties></xd:SignedProperties>"#,
        ),
        dsig = DSIG,
        xades = XADES,
        id = id,
        at = escaped(&signer.at),
        hash = algorithm.uri(),
        digest = wp_text::base64::encode(&digest),
        issuer = escaped(&issuer),
        serial = serial,
        role = match role.map(str::trim).filter(|said| !said.is_empty()) {
            Some(said) => format!(
                concat!(
                    r#"<xd:SignerRole><xd:ClaimedRoles>"#,
                    r#"<xd:ClaimedRole>{said}</xd:ClaimedRole>"#,
                    r#"</xd:ClaimedRoles></xd:SignerRole>"#,
                ),
                said = escaped(said),
            ),
            None => String::new(),
        },
    )
}

/// A number written in hex, written out in decimal.
///
/// A certificate's serial is read as hex because that is how everything shows
/// one, and XAdES writes it as a decimal integer. It can be longer than any
/// number this machine has, so it is done a digit at a time: multiply what is
/// there by sixteen, add the next, carry.
#[must_use]
fn decimal_of_hex(hex: &str) -> String {
    // Least significant first, which is the end carrying is done from.
    let mut digits: Vec<u8> = vec![0];
    for character in hex.chars() {
        let Some(value) = character.to_digit(16) else { continue };
        let mut carry = value;
        for digit in &mut digits {
            let next = u32::from(*digit) * 16 + carry;
            *digit = (next % 10) as u8;
            carry = next / 10;
        }
        while carry > 0 {
            digits.push((carry % 10) as u8);
            carry /= 10;
        }
    }
    while digits.len() > 1 && digits.last() == Some(&0) {
        digits.pop();
    }
    digits.iter().rev().map(|digit| char::from(b'0' + digit)).collect()
}

/// One signature made over another.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Counter {
    /// Who made it.
    pub certificate: wp_asn1::Certificate,
    /// When they say they did.
    pub signed_at: String,
    /// What they say they signed as — a witness, an approver — where they
    /// said anything.
    ///
    /// Not a reason: the signature above carries a reason, and what a person
    /// countersigning gives is the capacity they did it in, which is what the
    /// standard has a place for and what makes two countersignatures on one
    /// signature tell apart.
    pub role: String,
    /// Whether it holds.
    pub standing: Standing,
}

/// Signs somebody else's signature.
///
/// # What a countersignature is for
///
/// A second signature over the document says two people signed the same
/// thing. A countersignature says something stronger and different: that this
/// person saw *that signature* and signed it — a witness, an approval, a
/// second pair of eyes. What it covers is the first signature's value, so it
/// cannot be moved to another document or another signature without coming
/// apart.
///
/// # Where it goes
///
/// Inside the signature it is about, among that signature's *unsigned*
/// properties. That sounds alarming and is not: a signature never covers its
/// own value, so adding something beside that value takes nothing away from
/// it. The first signature holds exactly as well afterwards as before, and a
/// reader that knows nothing of countersignatures reads the first signature
/// and ignores the rest.
pub fn countersign(package: &mut Package, part: &str, signer: &Signer) -> Result<(), String> {
    let algorithm = Algorithm::Sha256;
    let text = package
        .xml_part(part)
        .ok_or_else(|| format!("{part} is not in this document"))?
        .map_err(|error| error.to_string())?;
    let tree = XmlTree::parse(&text).map_err(|_| format!("{part} is not a signature"))?;
    let root = &tree.root;

    let value = find(root, "SignatureValue")
        .ok_or_else(|| String::from("that signature has no value to sign"))?;
    // What is signed is the value as it stands in that signature, which means
    // canonicalised where it stands and not as this program would write it.
    let scope = c14n::context(&[root]);
    let canonical_value = c14n::canonical(value, &scope);
    let digest = algorithm.of(canonical_value.as_bytes());

    let properties_id = format!("idCounterSignedProperties{}", counters_in(root) + 1);
    let properties = signed_properties(signer, algorithm, &properties_id, Some(&signer.reason));
    let properties_digest = digest_of_xml(&properties, algorithm)?;

    let signed_info = format!(
        concat!(
            r#"<SignedInfo xmlns="{dsig}"><CanonicalizationMethod Algorithm="{c14n}">"#,
            r#"</CanonicalizationMethod><SignatureMethod Algorithm="{method}">"#,
            r#"</SignatureMethod>"#,
            r##"<Reference Type="{countersigned}" URI="#{value_id}">"##,
            r#"<DigestMethod Algorithm="{hash}"></DigestMethod>"#,
            r#"<DigestValue>{value}</DigestValue></Reference>"#,
            r##"<Reference Type="{xades}" URI="#{properties_id}">"##,
            r#"<DigestMethod Algorithm="{hash}"></DigestMethod>"#,
            r#"<DigestValue>{properties}</DigestValue></Reference></SignedInfo>"#,
        ),
        dsig = DSIG,
        c14n = c14n::NAME,
        method = algorithm.signature_uri(),
        countersigned = XADES_COUNTERSIGNED,
        xades = XADES_SIGNED_PROPERTIES,
        value_id = SIGNATURE_VALUE_ID,
        properties_id = properties_id,
        hash = algorithm.uri(),
        value = wp_text::base64::encode(&digest),
        properties = wp_text::base64::encode(&properties_digest),
    );
    let canonical_signed_info = canonical_of(&signed_info)?;
    let made = signer
        .key
        .sign(algorithm, canonical_signed_info.as_bytes())
        .ok_or_else(|| String::from("the key is too short to sign with"))?;

    // Its namespaces are written on it rather than inherited, for the reason
    // the signed properties are: what a canonical form renders is what the
    // element itself says, and this one is going to sit two wrappers deep.
    let countersignature = format!(
        concat!(
            r#"<Signature xmlns="{dsig}">{signed_info}"#,
            r#"<SignatureValue>{made}</SignatureValue>"#,
            r#"<KeyInfo><X509Data><X509Certificate>{certificate}</X509Certificate>"#,
            r#"{chain}</X509Data></KeyInfo><Object>"#,
            r##"<xd:QualifyingProperties xmlns:xd="{xades}" Target="">{properties}"##,
            r#"</xd:QualifyingProperties></Object></Signature>"#,
        ),
        dsig = DSIG,
        signed_info = signed_info.replacen(&format!(r#" xmlns="{DSIG}""#), "", 1),
        made = wp_text::base64::encode(&made),
        certificate = wp_text::base64::encode(&signer.certificate),
        chain = signer
            .chain
            .iter()
            .map(|der| {
                format!("<X509Certificate>{}</X509Certificate>", wp_text::base64::encode(der))
            })
            .collect::<String>(),
        xades = XADES,
        properties = properties.replacen(&format!(r#" xmlns="{DSIG}""#), "", 1),
    );

    let written = put_countersignature(&text, &countersignature)?;
    package.add_part(part, SIGNATURE_TYPE, written.into_bytes());
    Ok(())
}

/// How many countersignatures a signature already carries.
fn counters_in(root: &Element) -> usize {
    let mut found = 0;
    count_counters(root, &mut found);
    found
}

fn count_counters(element: &Element, found: &mut usize) {
    if element.local_name() == "CounterSignature" {
        *found += 1;
    }
    for child in element.child_elements() {
        count_counters(child, found);
    }
}

/// Puts one into the signature's unsigned properties.
///
/// Done to the text rather than to the parsed tree, because what goes back
/// into the package has to be the signature that was there with one thing
/// added: a tree written out afresh would be a different run of bytes, and
/// while the signature would still hold, nobody could see at a glance that
/// nothing else had moved.
fn put_countersignature(text: &str, countersignature: &str) -> Result<String, String> {
    let wrapped = format!("<xd:CounterSignature>{countersignature}</xd:CounterSignature>");

    // A second countersignature goes beside the first.
    if let Some(at) = text.rfind("</xd:UnsignedSignatureProperties>") {
        let mut out = String::with_capacity(text.len() + wrapped.len());
        out.push_str(&text[..at]);
        out.push_str(&wrapped);
        out.push_str(&text[at..]);
        return Ok(out);
    }

    // And the first makes the place they go.
    let at = text
        .rfind("</xd:QualifyingProperties>")
        .ok_or_else(|| String::from("that signature says nothing about itself to add to"))?;
    let mut out = String::with_capacity(text.len() + wrapped.len() + 64);
    out.push_str(&text[..at]);
    out.push_str("<xd:UnsignedProperties><xd:UnsignedSignatureProperties>");
    out.push_str(&wrapped);
    out.push_str("</xd:UnsignedSignatureProperties></xd:UnsignedProperties>");
    out.push_str(&text[at..]);
    Ok(out)
}

/// Signs a package, putting the signature into it.
///
/// Everything already in the package is covered, which is what signing a
/// document means; the signature parts themselves are not, since a signature
/// cannot cover itself.
pub fn sign(package: &mut Package, signer: &Signer) -> Result<String, String> {
    let algorithm = Algorithm::Sha256;
    let mut manifest = String::new();
    for name in covered(package) {
        manifest.push_str(&reference_for(package, &name, algorithm)?);
    }

    let package_object = format!(
        concat!(
            r#"<Object xmlns="{dsig}" Id="idPackageObject"><Manifest>{manifest}</Manifest>"#,
            r#"<SignatureProperties><SignatureProperty Id="idSignatureTime""#,
            r##" Target="#idPackageSignature"><mdssi:SignatureTime xmlns:mdssi="{mdssi}">"##,
            r#"<mdssi:Format>YYYY-MM-DDThh:mm:ssTZD</mdssi:Format>"#,
            r#"<mdssi:Value>{at}</mdssi:Value></mdssi:SignatureTime>"#,
            r#"</SignatureProperty></SignatureProperties></Object>"#,
        ),
        dsig = DSIG,
        manifest = manifest,
        mdssi = MDSSI,
        at = escaped(&signer.at),
    );
    let office_object = format!(
        concat!(
            r#"<Object xmlns="{dsig}" Id="idOfficeObject"><SignatureProperties>"#,
            r##"<SignatureProperty Id="idOfficeV1Details" Target="#idPackageSignature">"##,
            r#"<SignatureInfoV1 xmlns="{office}"><SetupID>{line}</SetupID>"#,
            r#"<SignatureText></SignatureText><SignatureImage></SignatureImage>"#,
            r#"<SignatureComments>{reason}</SignatureComments>"#,
            r#"<WindowsVersion>0.0</WindowsVersion><OfficeVersion>0.0</OfficeVersion>"#,
            r#"<ApplicationVersion>0.0</ApplicationVersion><Monitors>1</Monitors>"#,
            r#"<HorizontalResolution>0</HorizontalResolution>"#,
            r#"<VerticalResolution>0</VerticalResolution><ColorDepth>32</ColorDepth>"#,
            r#"<SignatureProviderId></SignatureProviderId>"#,
            r#"<SignatureProviderUrl></SignatureProviderUrl>"#,
            r#"<SignatureProviderDetails>0</SignatureProviderDetails>"#,
            r#"<ManifestHashAlgorithm>{hash}</ManifestHashAlgorithm>"#,
            // One is a signature about the document, two is one about a
            // signature line. A signature that named a line and called itself
            // the first kind would be telling a reader to ignore the name.
            r#"<SignatureType>{kind}</SignatureType></SignatureInfoV1>"#,
            r#"</SignatureProperty></SignatureProperties></Object>"#,
        ),
        dsig = DSIG,
        office = OFFICE,
        reason = escaped(&signer.reason),
        hash = algorithm.uri(),
        line = escaped(&signer.line),
        kind = if signer.line.trim().is_empty() { 1 } else { 2 },
    );

    // Each object is hashed as it will stand inside the signature, which
    // No role: a person signing their own document signs as themselves, and
    // what they say about it is the reason, which the signature carries above.
    let signed_properties = signed_properties(signer, algorithm, SIGNED_PROPERTIES_ID, None);

    // means with the signature's own namespace on it — so they are written
    // with it and canonicalised on their own.
    let package_digest = digest_of_xml(&package_object, algorithm)?;
    let office_digest = digest_of_xml(&office_object, algorithm)?;
    let properties_digest = digest_of_xml(&signed_properties, algorithm)?;

    let signed_info = format!(
        concat!(
            r#"<SignedInfo xmlns="{dsig}"><CanonicalizationMethod Algorithm="{c14n}">"#,
            r#"</CanonicalizationMethod><SignatureMethod Algorithm="{method}">"#,
            r#"</SignatureMethod>"#,
            r##"<Reference Type="{dsig}Object" URI="#idPackageObject">"##,
            r#"<DigestMethod Algorithm="{hash}"></DigestMethod>"#,
            r#"<DigestValue>{package}</DigestValue></Reference>"#,
            r##"<Reference Type="{dsig}Object" URI="#idOfficeObject">"##,
            r#"<DigestMethod Algorithm="{hash}"></DigestMethod>"#,
            r#"<DigestValue>{office}</DigestValue></Reference>"#,
            // What the signature says about itself, which is signed like
            // everything else it says: a signing time a reader has to take on
            // trust is worth as much as no signing time at all.
            r##"<Reference Type="{xades}" URI="#{properties_id}">"##,
            r#"<DigestMethod Algorithm="{hash}"></DigestMethod>"#,
            r#"<DigestValue>{properties}</DigestValue></Reference></SignedInfo>"#,
        ),
        dsig = DSIG,
        c14n = c14n::NAME,
        method = algorithm.signature_uri(),
        hash = algorithm.uri(),
        xades = XADES_SIGNED_PROPERTIES,
        properties_id = SIGNED_PROPERTIES_ID,
        package = wp_text::base64::encode(&package_digest),
        office = wp_text::base64::encode(&office_digest),
        properties = wp_text::base64::encode(&properties_digest),
    );
    let canonical_signed_info = canonical_of(&signed_info)?;
    let value = signer
        .key
        .sign(algorithm, canonical_signed_info.as_bytes())
        .ok_or_else(|| String::from("the key is too short to sign with"))?;

    let signature = format!(
        concat!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
            r#"<Signature xmlns="{dsig}" Id="idPackageSignature">{signed_info}"#,
            r#"<SignatureValue Id="{value_id}">{value}</SignatureValue>"#,
            r#"<KeyInfo><X509Data><X509Certificate>{certificate}</X509Certificate>"#,
            r#"{chain}</X509Data></KeyInfo>{package_object}{office_object}"#,
            r##"<Object><xd:QualifyingProperties xmlns:xd="{xades}""##,
            r##" Target="#idPackageSignature">{properties}"##,
            r#"</xd:QualifyingProperties></Object></Signature>"#,
        ),
        dsig = DSIG,
        // Written without its own namespace, since the Signature carries it.
        signed_info = signed_info.replacen(&format!(r#" xmlns="{DSIG}""#), "", 1),
        value = wp_text::base64::encode(&value),
        value_id = SIGNATURE_VALUE_ID,
        certificate = wp_text::base64::encode(&signer.certificate),
        chain = signer
            .chain
            .iter()
            .map(|der| {
                format!("<X509Certificate>{}</X509Certificate>", wp_text::base64::encode(der))
            })
            .collect::<String>(),
        package_object = package_object.replacen(&format!(r#" xmlns="{DSIG}""#), "", 1),
        office_object = office_object.replacen(&format!(r#" xmlns="{DSIG}""#), "", 1),
        xades = XADES,
        // The signature's own namespace comes off, because the Signature
        // above declares it and canonicalising this element renders it from
        // there. The XAdES one stays: it is declared on the wrapper as well,
        // but a canonical form is worked out from what is in scope, and what
        // is reliably in scope is what the element itself says.
        properties = signed_properties.replacen(&format!(r#" xmlns="{DSIG}""#), "", 1),
    );

    let name = next_signature_part(package);
    package.add_part(ORIGIN, ORIGIN_TYPE, Vec::new());
    package.add_part(&name, SIGNATURE_TYPE, signature.into_bytes());

    let mut root = package.relationships("").map_err(|error| error.to_string())?;
    if !root.all().iter().any(|relationship| relationship.kind == ORIGIN_RELATIONSHIP) {
        root.add(ORIGIN_RELATIONSHIP, ORIGIN, wp_opc::TargetMode::Internal);
        package.set_relationships(&root).map_err(|error| error.to_string())?;
    }
    let mut origin = package.relationships(ORIGIN).unwrap_or_else(|_| Relationships::new(ORIGIN));
    let target = name.rsplit('/').next().unwrap_or(&name).to_owned();
    origin.add(SIGNATURE_RELATIONSHIP, &target, wp_opc::TargetMode::Internal);
    package.set_relationships(&origin).map_err(|error| error.to_string())?;

    Ok(name)
}

/// What a new signature part is called: the first number nothing is using.
fn next_signature_part(package: &Package) -> String {
    for number in 1..1000 {
        let name = format!("_xmlsignatures/sig{number}.xml");
        if package.part(&name).is_none() {
            return name;
        }
    }
    String::from("_xmlsignatures/sig1000.xml")
}

/// Every part a new signature covers.
///
/// Everything in the package except the signatures, which cannot cover
/// themselves, and except the directory markers a zip may carry, which are
/// not parts at all.
fn covered(package: &Package) -> Vec<String> {
    let mut names: Vec<String> = package
        .entries()
        .iter()
        .filter(|entry| !entry.is_directory())
        .map(|entry| entry.name.clone())
        .filter(|name| !name.starts_with("_xmlsignatures/"))
        .filter(|name| name != "[Content_Types].xml")
        .collect();
    names.sort();
    names
}

/// One reference of the manifest.
fn reference_for(package: &Package, name: &str, algorithm: Algorithm) -> Result<String, String> {
    let bytes = package.part(name).ok_or_else(|| format!("no part {name}"))?;
    let content_type =
        package.content_type(name).ok_or_else(|| format!("no content type for {name}"))?.to_owned();

    if is_a_relationship_part(name) {
        let text =
            std::str::from_utf8(bytes).map_err(|_| format!("{name} is not text"))?.to_owned();
        let relationships = Relationships::parse(name, &text).map_err(|error| error.to_string())?;
        let mut ids: Vec<&str> =
            relationships.all().iter().map(|relationship| relationship.id.as_str()).collect();
        ids.sort_unstable();
        let mut named = String::new();
        for id in &ids {
            named.push_str(&format!(
                r#"<mdssi:RelationshipReference SourceId="{}"></mdssi:RelationshipReference>"#,
                escaped(id)
            ));
        }
        let signed = relationships_as_signed(name, &text, &[])?;
        let digest = algorithm.of(canonical_of(&signed)?.as_bytes());
        return Ok(format!(
            concat!(
                r#"<Reference URI="/{name}?ContentType={content_type}"><Transforms>"#,
                r#"<Transform Algorithm="{transform}" xmlns:mdssi="{mdssi}">{named}</Transform>"#,
                r#"<Transform Algorithm="{c14n}"></Transform></Transforms>"#,
                r#"<DigestMethod Algorithm="{hash}"></DigestMethod>"#,
                r#"<DigestValue>{value}</DigestValue></Reference>"#,
            ),
            name = name,
            content_type = content_type,
            transform = RELATIONSHIP_TRANSFORM,
            mdssi = MDSSI,
            named = named,
            c14n = c14n::NAME,
            hash = algorithm.uri(),
            value = wp_text::base64::encode(&digest),
        ));
    }

    Ok(format!(
        concat!(
            r#"<Reference URI="/{name}?ContentType={content_type}">"#,
            r#"<DigestMethod Algorithm="{hash}"></DigestMethod>"#,
            r#"<DigestValue>{value}</DigestValue></Reference>"#,
        ),
        name = name,
        content_type = content_type,
        hash = algorithm.uri(),
        value = wp_text::base64::encode(&algorithm.of(bytes)),
    ))
}

/// The canonical form of a piece of XML given as text.
fn canonical_of(xml: &str) -> Result<String, String> {
    let tree = XmlTree::parse(xml).map_err(|error| error.to_string())?;
    Ok(c14n::canonical(&tree.root, &[]))
}

fn digest_of_xml(xml: &str, algorithm: Algorithm) -> Result<Vec<u8>, String> {
    Ok(algorithm.of(canonical_of(xml)?.as_bytes()))
}

/// Takes every signature off a package.
///
/// What Word's Remove Signature does, and what has to happen when a signed
/// document is edited: a signature over a document that has changed is worse
/// than no signature, because it says the document is what it is not.
pub fn unsign(package: &mut Package) -> bool {
    let names: Vec<String> = package
        .entries()
        .iter()
        .map(|entry| entry.name.clone())
        .filter(|name| name.starts_with("_xmlsignatures/"))
        .collect();
    if names.is_empty() {
        return false;
    }
    for name in names {
        package.remove_part(&name);
    }
    if let Ok(mut root) = package.relationships("") {
        if remove_kind(&mut root, ORIGIN_RELATIONSHIP) {
            let _ = package.set_relationships(&root);
        }
    }
    true
}

/// Whether anything in the tree is a signature at all.
#[must_use]
pub fn is_signed(package: &Package) -> bool {
    !signature_parts(package).is_empty()
}

/// Everything in a manifest, for the tests: which parts a signature covers.
#[must_use]
pub fn covered_parts(package: &Package) -> Vec<String> {
    covered(package)
}

/// Whether a part is one of the files that hold relationships.
///
/// Asked by its name, which is what the format says it is: a part called
/// `.rels` inside a folder called `_rels`.
fn is_a_relationship_part(name: &str) -> bool {
    name.ends_with(".rels") && name.contains("_rels/")
}

/// Takes out every relationship of a kind, and says whether any went.
fn remove_kind(relationships: &mut Relationships, kind: &str) -> bool {
    let going: Vec<String> = relationships
        .all()
        .iter()
        .filter(|relationship| relationship.kind == kind)
        .map(|relationship| relationship.id.clone())
        .collect();
    let mut any = false;
    for id in going {
        any |= relationships.remove(&id);
    }
    any
}
