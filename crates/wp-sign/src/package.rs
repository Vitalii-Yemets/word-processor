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

/// The transform that turns a relationship part into what it says.
const RELATIONSHIP_TRANSFORM: &str =
    "http://schemas.openxmlformats.org/package/2006/RelationshipTransform";

/// Where the signatures live.
pub const ORIGIN: &str = "_xmlsignatures/origin.sigs";
const ORIGIN_TYPE: &str = "application/vnd.openxmlformats-package.digital-signature-origin";
const SIGNATURE_TYPE: &str =
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
    /// When they say they signed.
    pub signed_at: String,
    /// What they said about why.
    pub reason: String,
    /// The parts the signature covers, in the order the manifest lists them.
    pub parts: Vec<String>,
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
    let certificate = wp_asn1::Certificate::read(&wp_text::base64::decode(
        find(root, "X509Certificate")?.text_content().trim().as_bytes(),
    ))?;
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

    let standing = check(package, root, signed_info, &certificate);
    Some(Signature { part: part.to_owned(), certificate, signed_at, reason, parts, standing })
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

/// Who is signing, and with what.
pub struct Signer {
    /// Their certificate, as it was written.
    pub certificate: Vec<u8>,
    pub key: PrivateKey,
    /// What they say about why, which Word shows.
    pub reason: String,
    /// When, as `YYYY-MM-DDThh:mm:ssZ`.
    pub at: String,
}

impl core::fmt::Debug for Signer {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "A signer with a certificate of {} bytes", self.certificate.len())
    }
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
            r#"<SignatureInfoV1 xmlns="{office}"><SetupID></SetupID>"#,
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
            r#"<SignatureType>1</SignatureType></SignatureInfoV1>"#,
            r#"</SignatureProperty></SignatureProperties></Object>"#,
        ),
        dsig = DSIG,
        office = OFFICE,
        reason = escaped(&signer.reason),
        hash = algorithm.uri(),
    );

    // Each object is hashed as it will stand inside the signature, which
    // means with the signature's own namespace on it — so they are written
    // with it and canonicalised on their own.
    let package_digest = digest_of_xml(&package_object, algorithm)?;
    let office_digest = digest_of_xml(&office_object, algorithm)?;

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
            r#"<DigestValue>{office}</DigestValue></Reference></SignedInfo>"#,
        ),
        dsig = DSIG,
        c14n = c14n::NAME,
        method = algorithm.signature_uri(),
        hash = algorithm.uri(),
        package = wp_text::base64::encode(&package_digest),
        office = wp_text::base64::encode(&office_digest),
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
            r#"<SignatureValue>{value}</SignatureValue>"#,
            r#"<KeyInfo><X509Data><X509Certificate>{certificate}</X509Certificate>"#,
            r#"</X509Data></KeyInfo>{package_object}{office_object}</Signature>"#,
        ),
        dsig = DSIG,
        // Written without its own namespace, since the Signature carries it.
        signed_info = signed_info.replacen(&format!(r#" xmlns="{DSIG}""#), "", 1),
        value = wp_text::base64::encode(&value),
        certificate = wp_text::base64::encode(&signer.certificate),
        package_object = package_object.replacen(&format!(r#" xmlns="{DSIG}""#), "", 1),
        office_object = office_object.replacen(&format!(r#" xmlns="{DSIG}""#), "", 1),
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
