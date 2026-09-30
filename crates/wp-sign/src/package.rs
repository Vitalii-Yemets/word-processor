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
//!
//! # What a reader believes
//!
//! A signature file holds more than the signature covers: the value itself,
//! the certificates beside it, the properties added after it was made. Any of
//! that can be added to without the arithmetic noticing, so a signature is
//! taken to say only what it signed — the elements its signed information
//! names, each of which hashed to what the signed information says it does.
//! The manifest, the time, the reason and the line are all read from those
//! and from nowhere else, and an identifier that names two elements is
//! refused rather than resolved, since which of the two was signed would be a
//! matter of which one the reader happened to find first.

use std::collections::BTreeMap;

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
    /// When they say they signed, as the signed package object says it.
    pub signed_at: String,
    /// What they said about why, as the signed Office object says it.
    pub reason: String,
    /// The parts the signature covers, in the order its manifest lists them.
    ///
    /// Only the manifests the signature itself covers are read: a manifest
    /// anywhere else in the file is one anybody could have put there.
    pub parts: Vec<String>,
    /// What it covers of each relationship part among `parts`.
    ///
    /// A relationship part is signed for the relationships its transform
    /// names, and a relationship added afterwards with an identifier of its
    /// own changes nothing that was signed — so whether a relationship is
    /// covered is a question of its own. See [`Signature::covers_relationship`].
    pub relationships: BTreeMap<String, Covering>,
    /// The signature line it was made for, or empty where it is about the
    /// document at large. Read from the signed Office object, like the
    /// reason.
    pub line: String,
    /// The signatures somebody else made over this one.
    pub counters: Vec<Counter>,
    /// Whether it holds, and what is wrong with it if it does not.
    pub standing: Standing,
}

/// How much of one relationship part a signature covers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Covering {
    /// Every relationship in it: the part was signed as it stands, or its
    /// transform named none and so signed them all.
    Every,
    /// Only these, by identifier. Any other relationship in the part is one
    /// the signature says nothing about.
    Only(Vec<String>),
}

impl Signature {
    /// Whether a part is among those the signature covers.
    ///
    /// Compared as the package compares part names, without regard to case:
    /// the part a reader is handed for a name is the one whose digest was
    /// checked under it.
    #[must_use]
    pub fn covers_part(&self, name: &str) -> bool {
        let name = name.trim_start_matches('/');
        self.parts.iter().any(|part| part.eq_ignore_ascii_case(name))
    }

    /// Whether one relationship of a part is covered: the part that holds
    /// the relationships of `source` is covered, and this one is among those
    /// it was signed for. `source` is empty for the package's own.
    #[must_use]
    pub fn covers_relationship(&self, source: &str, id: &str) -> bool {
        let holder = wp_opc::relationships_part_for(source);
        self.relationships.iter().find(|(name, _)| name.eq_ignore_ascii_case(&holder)).is_some_and(
            |(_, covering)| match covering {
                Covering::Every => true,
                Covering::Only(ids) => ids.iter().any(|one| one == id),
            },
        )
    }

    /// Whether what a part reaches by relationships of one kind is covered:
    /// every such relationship, and every part it reaches.
    ///
    /// The question a reader following those relationships has to ask
    /// before believing what it finds. A part added beside a signature
    /// arrives with a relationship to it, and neither was signed; a signature
    /// that holds says nothing about either. True where there are none,
    /// since nothing reached is nothing left uncovered.
    #[must_use]
    pub fn covers_reached(&self, package: &Package, source: &str, kind: &str) -> bool {
        let Ok(relationships) = package.relationships(source) else { return false };
        relationships.all().iter().filter(|one| one.kind == kind).all(|relationship| {
            self.covers_relationship(source, &relationship.id)
                && match relationship.resolved_target(source) {
                    Some(Ok(target)) => self.covers_part(&target),
                    // Outside the package, or nowhere: not something a
                    // signature over the package can vouch for.
                    _ => false,
                }
        })
    }

    /// Whether the signature covers the whole document: every part but the
    /// signatures themselves, and every relationship but the one that leads
    /// to them.
    ///
    /// Word's partial signature — "a portion of a file is signed" — is one
    /// that holds for what it covers and does not cover this much. The two
    /// left out are left out by the format: a signature cannot cover itself,
    /// and the content types are not a part.
    #[must_use]
    pub fn covers_whole(&self, package: &Package) -> bool {
        package
            .entries()
            .iter()
            .filter(|entry| !entry.is_directory())
            .map(|entry| entry.name.trim_start_matches('/'))
            .filter(|name| !name.starts_with("_xmlsignatures/"))
            .filter(|name| !name.eq_ignore_ascii_case("[Content_Types].xml"))
            .all(|name| {
                if !self.covers_part(name) {
                    return false;
                }
                if !is_a_relationship_part(name) {
                    return true;
                }
                let Some(source) = source_of(name) else { return false };
                let Ok(relationships) = package.relationships(&source) else { return false };
                relationships
                    .all()
                    .iter()
                    .filter(|relationship| relationship.kind != ORIGIN_RELATIONSHIP)
                    .all(|relationship| self.covers_relationship(&source, &relationship.id))
            })
    }
}

/// The part whose relationships a relationship part holds: the reverse of
/// [`wp_opc::relationships_part_for`], and empty for the package's own.
fn source_of(relationships_part: &str) -> Option<String> {
    let (directory, file) = match relationships_part.rsplit_once("_rels/") {
        Some((directory, file)) => (directory, file),
        None => return None,
    };
    let file = file.strip_suffix(".rels")?;
    Some(format!("{directory}{file}"))
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
    /// More than one element in the signature goes by the same name — an
    /// identifier used twice, or a second signed information or value — so
    /// which of them was signed cannot be told. Named.
    Ambiguous(String),
    /// Everything it signed holds, and none of it is what the signature is
    /// about: no manifest among what it signed, so no part of the document,
    /// or for a countersignature, not the value of the signature it is over.
    /// A signature over nothing vouches for nothing.
    CoversNothing,
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
            Self::Ambiguous(what) => {
                format!("cannot be checked: more than one thing in it is called {what}")
            }
            Self::CoversNothing => "it signs no part of the document".to_owned(),
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

/// The certificates one signature carries in its own key information: the
/// signer's first, which is where the format puts it, and then the chain.
///
/// Its own and nobody else's. A countersignature sits inside the signature
/// it is about and carries its countersigner's certificate, which is neither
/// the signer's nor part of the signer's chain.
fn certificates_in(
    signature: &Element,
) -> Option<(wp_asn1::Certificate, Vec<wp_asn1::Certificate>)> {
    let mut carried =
        along(signature, &[(DSIG, "KeyInfo"), (DSIG, "X509Data"), (DSIG, "X509Certificate")])
            .into_iter()
            .filter_map(|element| {
                let der = wp_text::base64::decode(element.text_content().trim().as_bytes());
                wp_asn1::Certificate::read(&der)
            });
    let signer = carried.next()?;
    Some((signer, carried.collect()))
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
    let (certificate, chain) = certificates_in(root)?;

    // Refused before anything is resolved: with two elements answering to one
    // name, every answer below would depend on which of them was met first.
    let ids = match identifiers(root) {
        Ok(ids) => ids,
        Err(twice) => {
            return Some(Signature {
                part: part.to_owned(),
                certificate,
                chain,
                signed_at: String::new(),
                reason: String::new(),
                parts: Vec::new(),
                relationships: BTreeMap::new(),
                line: String::new(),
                counters: Vec::new(),
                standing: Standing::Ambiguous(twice),
            })
        }
    };

    let covered = signed_elements(root, signed_info, &ids);
    let manifests = manifests_in(&covered.verified);
    let mut parts = Vec::new();
    let mut relationships = BTreeMap::new();
    for reference in manifests.iter().flat_map(|manifest| children(manifest, "Reference")) {
        let Some(name) = part_of(reference.attribute(None, "URI").unwrap_or_default()) else {
            continue;
        };
        if is_a_relationship_part(&name) {
            relationships.insert(name.clone(), covering_of(reference));
        }
        parts.push(name);
    }
    let signed_at = signed_property(&covered.verified, MDSSI, "SignatureTime", "Value");
    let reason = signed_property(&covered.verified, OFFICE, "SignatureInfoV1", "SignatureComments");
    // Which line it was made for. Word writes this as the setup identifier of
    // the signature line, and a signature about the document at large leaves
    // it empty — which is what an absent element amounts to as well.
    let line = signed_property(&covered.verified, OFFICE, "SignatureInfoV1", "SetupID");
    let counters = counters_of(root, &ids);

    let standing = check(package, root, signed_info, &certificate, &covered, &manifests);
    Some(Signature {
        part: part.to_owned(),
        certificate,
        chain,
        signed_at,
        reason,
        parts,
        relationships,
        line,
        counters,
        standing,
    })
}

/// What one manifest reference to a relationship part covers of it.
///
/// The same reading [`transformed`] makes when it checks the digest, so that
/// what is said to be covered is what was hashed: the identifiers each
/// relationship transform names, narrowed by each further one, and every
/// relationship where no transform names any.
fn covering_of(reference: &Element) -> Covering {
    let mut covering = Covering::Every;
    let Some(transforms) = child(reference, "Transforms") else { return covering };
    for transform in transforms.child_elements() {
        if transform.attribute(None, "Algorithm") != Some(RELATIONSHIP_TRANSFORM) {
            continue;
        }
        let named: Vec<String> = transform
            .child_elements()
            .filter(|child| child.local_name() == "RelationshipReference")
            .filter_map(|child| child.attribute(None, "SourceId"))
            .map(str::to_owned)
            .collect();
        if named.is_empty() {
            continue;
        }
        covering = match covering {
            Covering::Every => Covering::Only(named),
            Covering::Only(before) => {
                Covering::Only(before.into_iter().filter(|id| named.contains(id)).collect())
            }
        };
    }
    covering
}

/// Every identifier in a signature file and the one element it names, or the
/// first name that is not one element's.
///
/// Two elements with one identifier are two answers to "what does this
/// reference cover", and a reader that took the first would be taking
/// whichever one somebody put first. The same goes for a signature with two
/// signed informations, two values or two sets of keys: the one that is
/// checked and the one that is believed have to be the same one, and with
/// two there is no telling that they are.
fn identifiers(root: &Element) -> Result<BTreeMap<&str, &Element>, String> {
    for local in ["SignedInfo", "SignatureValue", "KeyInfo"] {
        if children(root, local).count() > 1 {
            return Err(local.to_owned());
        }
    }
    let mut out = BTreeMap::new();
    let mut waiting = vec![root];
    while let Some(element) = waiting.pop() {
        if let Some(id) = element.attribute(None, "Id") {
            if out.insert(id, element).is_some() {
                return Err(id.to_owned());
            }
        }
        waiting.extend(element.child_elements());
    }
    Ok(out)
}

/// What a signed information covers: the elements it names that hash to what
/// it says they hash to, and the first thing wrong with the rest.
///
/// Everything a signature is taken to say is read from `verified` and from
/// nothing else in the file.
struct Covered<'a> {
    verified: Vec<&'a Element>,
    fault: Option<Standing>,
}

fn signed_elements<'a>(
    root: &'a Element,
    signed_info: &Element,
    ids: &BTreeMap<&'a str, &'a Element>,
) -> Covered<'a> {
    // Each is canonicalised in what the whole signature declares, which is
    // the scope it was hashed in when it was written.
    let scope = c14n::context(&[root]);
    let mut verified = Vec::new();
    let mut fault = None;
    for reference in children(signed_info, "Reference") {
        match resolved(reference, ids, &scope) {
            Ok(element) => verified.push(element),
            Err(standing) => {
                fault.get_or_insert(standing);
            }
        }
    }
    Covered { verified, fault }
}

/// The element one reference names, provided it hashes to what the
/// reference says.
fn resolved<'a>(
    reference: &Element,
    ids: &BTreeMap<&'a str, &'a Element>,
    scope: &[(Option<String>, String)],
) -> Result<&'a Element, Standing> {
    let uri = reference.attribute(None, "URI").unwrap_or_default();
    let Some(id) = uri.strip_prefix('#') else {
        return Err(Standing::Unsupported(format!("a reference to {uri}")));
    };
    let Some(&element) = ids.get(id) else {
        return Err(Standing::Changed(uri.to_owned()));
    };
    let Some((algorithm, wanted)) = digest_of(reference) else {
        return Err(Standing::Unsupported(String::from("a digest this program has not")));
    };
    let bytes = c14n::canonical(element, scope);
    if algorithm.of(bytes.as_bytes()) == wanted {
        Ok(element)
    } else {
        Err(Standing::Changed(format!("the signature's own {id}")))
    }
}

/// The manifests among what was signed.
///
/// One that was signed itself, or one inside an object that was — which is
/// where the format puts the package's. A manifest anywhere else is one
/// nobody signed, however it came to be in the file.
fn manifests_in<'a>(verified: &[&'a Element]) -> Vec<&'a Element> {
    let mut out = Vec::new();
    for &element in verified {
        if element.is(Some(DSIG), "Manifest") {
            out.push(element);
        } else if element.is(Some(DSIG), "Object") {
            out.extend(children(element, "Manifest"));
        }
    }
    out
}

/// Something a signature says about itself, read from the objects it signed.
///
/// A signature's properties sit inside an object, in a
/// `SignatureProperties`, in a `SignatureProperty`; `holder` is the element
/// a property is written in and `local` the one wanted inside that.
fn signed_property(verified: &[&Element], namespace: &str, holder: &str, local: &str) -> String {
    verified
        .iter()
        .filter(|element| element.is(Some(DSIG), "Object"))
        .flat_map(|object| {
            along(
                object,
                &[
                    (DSIG, "SignatureProperties"),
                    (DSIG, "SignatureProperty"),
                    (namespace, holder),
                    (namespace, local),
                ],
            )
        })
        .map(Element::text_content)
        .next()
        .unwrap_or_default()
}

/// Every signature made over this one.
///
/// Where the standard puts them and nowhere else: among this signature's
/// unsigned properties. They are unsigned by this signature because they
/// came after it, and each is checked on its own against the value it is
/// about — so what one of them says is read from what it signed, as it is
/// for the signature itself.
fn counters_of(root: &Element, ids: &BTreeMap<&str, &Element>) -> Vec<Counter> {
    let Some(value) = child(root, "SignatureValue") else { return Vec::new() };
    along(
        root,
        &[
            (DSIG, "Object"),
            (XADES, "QualifyingProperties"),
            (XADES, "UnsignedProperties"),
            (XADES, "UnsignedSignatureProperties"),
            (XADES, "CounterSignature"),
            (DSIG, "Signature"),
        ],
    )
    .into_iter()
    .filter_map(|signature| counter(root, value, signature, ids))
    .collect()
}

/// One of them, and whether it holds.
///
/// The same arithmetic as a signature over a document, over less: there is no
/// manifest, because what a countersignature covers is one element of one
/// file and not a package — the value of the signature it is about. What it
/// points at is resolved in the file it sits in, because that is where the
/// value it signed is.
fn counter(
    root: &Element,
    value: &Element,
    signature: &Element,
    ids: &BTreeMap<&str, &Element>,
) -> Option<Counter> {
    let (certificate, _) = certificates_in(signature)?;
    let unsaid = |standing| Counter {
        certificate: certificate.clone(),
        signed_at: String::new(),
        role: String::new(),
        standing,
    };
    if let Some(doubled) = ["SignedInfo", "SignatureValue", "KeyInfo"]
        .into_iter()
        .find(|local| children(signature, local).count() > 1)
    {
        return Some(unsaid(Standing::Ambiguous(doubled.to_owned())));
    }
    let Some(signed_info) = child(signature, "SignedInfo") else {
        return Some(unsaid(Standing::Broken));
    };

    let covered = signed_elements(root, signed_info, ids);
    let said = |steps: &[(&str, &str)]| {
        covered
            .verified
            .iter()
            .filter(|element| element.is(Some(XADES), "SignedProperties"))
            .flat_map(|properties| along(properties, steps))
            .map(Element::text_content)
            .next()
            .unwrap_or_default()
    };
    let signed_at = said(&[(XADES, "SignedSignatureProperties"), (XADES, "SigningTime")]);
    let role = said(&[
        (XADES, "SignedSignatureProperties"),
        (XADES, "SignerRole"),
        (XADES, "ClaimedRoles"),
        (XADES, "ClaimedRole"),
    ]);
    let standing = check_counter(root, value, signature, signed_info, &certificate, &covered);
    Some(Counter { certificate, signed_at, role, standing })
}

/// Whether a countersignature holds over the value it is about.
fn check_counter(
    root: &Element,
    value: &Element,
    counter: &Element,
    signed_info: &Element,
    certificate: &wp_asn1::Certificate,
    covered: &Covered<'_>,
) -> Standing {
    let algorithm = match method_of(signed_info) {
        Ok(algorithm) => algorithm,
        Err(standing) => return standing,
    };
    if let Some(fault) = &covered.fault {
        return fault.clone();
    }
    // What makes it a countersignature at all. One that signed only what it
    // says about itself would hold in any document it was copied into.
    if !covered.verified.iter().any(|&element| std::ptr::eq(element, value)) {
        return Standing::CoversNothing;
    }
    if !names_its_certificate(&covered.verified, certificate) {
        return Standing::Broken;
    }
    made_by(root, signed_info, counter, certificate, algorithm)
}

/// Whether the certificate carried beside a signature is the one its signed
/// properties name.
///
/// XAdES writes a digest of the signer's certificate into what is signed,
/// because the certificate itself sits outside it: without the digest, a
/// certificate with the same key and anything else written on it could be
/// put in its place and the arithmetic would not notice. The signer's is the
/// one that has to be there; the others the list may hold are its chain. A
/// signature that says nothing about its certificate — one made before
/// XAdES, as Office 2007 made them — has nothing to be held to here.
fn names_its_certificate(verified: &[&Element], certificate: &wp_asn1::Certificate) -> bool {
    let digests: Vec<&Element> = verified
        .iter()
        .filter(|element| element.is(Some(XADES), "SignedProperties"))
        .flat_map(|properties| {
            along(
                properties,
                &[
                    (XADES, "SignedSignatureProperties"),
                    (XADES, "SigningCertificate"),
                    (XADES, "Cert"),
                    (XADES, "CertDigest"),
                ],
            )
        })
        .collect();
    digests.is_empty()
        || digests.iter().any(|digest| {
            digest_of(digest)
                .is_some_and(|(algorithm, wanted)| algorithm.of(&certificate.der) == wanted)
        })
}

/// Does the arithmetic.
fn check(
    package: &Package,
    root: &Element,
    signed_info: &Element,
    certificate: &wp_asn1::Certificate,
    covered: &Covered<'_>,
    manifests: &[&Element],
) -> Standing {
    let algorithm = match method_of(signed_info) {
        Ok(algorithm) => algorithm,
        Err(standing) => return standing,
    };

    // Every reference inside the signed information points at an element in
    // this same file, and each has to hash to what it says.
    if let Some(fault) = &covered.fault {
        return fault.clone();
    }

    // And every reference inside the manifests it signed points at a part of
    // the package. A signature whose manifests point at nothing, or that
    // signed no manifest at all, says nothing about the document, and saying
    // it holds would be vouching for whatever the document now is.
    let mut any = false;
    for reference in manifests.iter().flat_map(|manifest| children(manifest, "Reference")) {
        any = true;
        match part_standing(package, reference) {
            Standing::Good => {}
            other => return other,
        }
    }
    if !any {
        return Standing::CoversNothing;
    }

    // The certificate beside it has to be the one it says it was made with.
    if !names_its_certificate(&covered.verified, certificate) {
        return Standing::Broken;
    }

    // Then the signature itself, over the signed information as it stands.
    made_by(root, signed_info, root, certificate, algorithm)
}

/// The signature algorithm a signed information names, provided it is
/// canonicalised the one way this program knows.
fn method_of(signed_info: &Element) -> Result<Algorithm, Standing> {
    // The canonicalisation named in the file, which is the only one written.
    let named = child(signed_info, "CanonicalizationMethod")
        .and_then(|element| element.attribute(None, "Algorithm"))
        .unwrap_or(c14n::NAME);
    if named != c14n::NAME {
        return Err(Standing::Unsupported(named.to_owned()));
    }
    child(signed_info, "SignatureMethod")
        .and_then(|element| element.attribute(None, "Algorithm"))
        .and_then(Algorithm::named)
        .ok_or_else(|| Standing::Unsupported(String::from("an algorithm this program has not")))
}

/// Whether the value beside a signed information was made with the key of
/// the certificate, over that signed information as it stands.
fn made_by(
    root: &Element,
    signed_info: &Element,
    signature: &Element,
    certificate: &wp_asn1::Certificate,
    algorithm: Algorithm,
) -> Standing {
    let signed = c14n::canonical(signed_info, &c14n::context(&[root]));
    let Some(value) = child(signature, "SignatureValue") else { return Standing::Broken };
    let made = wp_text::base64::decode(value.text_content().trim().as_bytes());
    let key = PublicKey::new(&certificate.key.modulus, &certificate.key.exponent);
    if key.verifies(algorithm, signed.as_bytes(), &made) {
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

/// The first child of the signature's own namespace by its local name.
fn child<'a>(element: &'a Element, local: &'a str) -> Option<&'a Element> {
    children(element, local).next()
}

/// Every child of the signature's own namespace by its local name.
///
/// Children only, and never a search of everything below: what sits where in
/// a signature is what says whether it was signed, and an element found
/// somewhere else by its name alone may be one that nobody signed.
fn children<'a>(element: &'a Element, local: &'a str) -> impl Iterator<Item = &'a Element> + 'a {
    element.child_elements().filter(move |child| child.is(Some(DSIG), local))
}

/// Every element reached from this one by a path of children, each step a
/// namespace and a local name.
fn along<'a>(from: &'a Element, steps: &[(&str, &str)]) -> Vec<&'a Element> {
    let mut here = vec![from];
    for &(namespace, local) in steps {
        here = here
            .into_iter()
            .flat_map(|element| element.child_elements())
            .filter(|child| child.is(Some(namespace), local))
            .collect();
    }
    here
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

    // The value of that signature and not any other element of the name: a
    // countersignature in it carries a value of its own.
    let value = child(root, "SignatureValue")
        .ok_or_else(|| String::from("that signature has no value to sign"))?;
    // What is signed is the value as it stands in that signature, which means
    // canonicalised where it stands and not as this program would write it.
    let scope = c14n::context(&[root]);
    let canonical_value = c14n::canonical(value, &scope);
    let digest = algorithm.of(canonical_value.as_bytes());

    // A name for its properties that nothing in the file has already, since a
    // reader refuses a signature in which two elements share one — and a
    // signature that is refused already is not one to add a witness to.
    let taken = identifiers(root).map_err(|twice| {
        format!("that signature cannot be checked: more than one thing in it is called {twice}")
    })?;
    let properties_id = (1..)
        .map(|number| format!("idCounterSignedProperties{number}"))
        .find(|id| !taken.contains_key(id.as_str()))
        .unwrap_or_default();
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
