//! Encrypted PDFs read back with their passwords.
//!
//! LibreOffice, in the build image, encrypts a PDF it exports the way its
//! version does — RC4 with a 128-bit key — with a password to open it, or
//! with none to open it and one to change it. The other revisions of the
//! standard security handler, the 40-bit RC4 of the first and the AES of
//! the later ones, nothing to hand writes, so the test encrypts a page of
//! its own in each; poppler, which LibreOffice's PDF import runs in a
//! helper of its own, reads the same file with the same password, which
//! is what shows the encrypting right.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;

use wp_docx::Document;

static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn folder(name: &str) -> PathBuf {
    let folder =
        std::env::temp_dir().join(format!("wp-pdf-encrypted-{}-{name}", std::process::id()));
    let _ = std::fs::create_dir_all(&folder);
    folder
}

fn soffice(folder: &Path, format: &str, file: &Path) {
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let output = Command::new("soffice")
        .arg(format!("-env:UserInstallation=file://{}/profile", folder.display()))
        .args(["--headless", "--convert-to", format, "--outdir"])
        .arg(folder)
        .arg(file)
        .output()
        .expect("soffice runs");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}

/// A page LibreOffice exports with the export filter's options given.
fn exported_by_libreoffice(name: &str, options: &str) -> Vec<u8> {
    let folder = folder(name);
    // Through ODT: printed straight from HTML, the first paragraph is lost.
    std::fs::write(folder.join("page.html"), "<p>Hello secret world</p><p>A second line.</p>")
        .unwrap();
    soffice(&folder, "odt", &folder.join("page.html"));
    soffice(&folder, &format!("pdf:writer_pdf_Export:{options}"), &folder.join("page.odt"));
    let pdf = std::fs::read(folder.join("page.pdf")).expect("exported");
    let _ = std::fs::remove_dir_all(&folder);
    pdf
}

fn text_of(document: &Document) -> String {
    document.body().blocks.iter().map(|block| block.plain_text()).collect::<Vec<_>>().join("\n")
}

#[test]
fn a_pdf_libreoffice_encrypts_opens_with_its_password() {
    let pdf = exported_by_libreoffice(
        "open",
        r#"{"EncryptFile":{"type":"boolean","value":"true"},"DocumentOpenPassword":{"type":"string","value":"secret"}}"#,
    );
    assert!(matches!(wp_pdf::open(&pdf), Err(wp_pdf::ReadError::Encrypted)));
    assert!(matches!(wp_pdf::open_with_password(&pdf, "guess"), Err(wp_pdf::ReadError::Encrypted)));
    let document = wp_pdf::open_with_password(&pdf, "secret").expect("opened");
    let text = text_of(&document);
    assert!(text.contains("Hello secret world"), "{text}");
    assert!(text.contains("A second line."), "{text}");
}

#[test]
fn a_pdf_with_only_a_password_to_change_it_opens_without_one() {
    let pdf = exported_by_libreoffice(
        "permissions",
        r#"{"RestrictPermissions":{"type":"boolean","value":"true"},"PermissionPassword":{"type":"string","value":"owner"}}"#,
    );
    assert!(pdf.windows(8).any(|w| w == b"/Encrypt"), "encrypted after all");
    let text = text_of(&wp_pdf::open(&pdf).expect("opened without a password"));
    assert!(text.contains("Hello secret world"), "{text}");
    let text = text_of(&wp_pdf::open_with_password(&pdf, "owner").expect("opened as the owner"));
    assert!(text.contains("Hello secret world"), "{text}");
}

// ---------------------------------------------------------------------------
// The test's own encrypting.

const PAD: [u8; 32] = [
    0x28, 0xBF, 0x4E, 0x5E, 0x4E, 0x75, 0x8A, 0x41, 0x64, 0x00, 0x4E, 0x56, 0xFF, 0xFA, 0x01, 0x08,
    0x2E, 0x2E, 0x00, 0xB6, 0xD0, 0x68, 0x3E, 0x80, 0x2F, 0x0C, 0xA9, 0xFE, 0x64, 0x53, 0x69, 0x7A,
];

const PERMISSIONS: i32 = -4;
const ID: [u8; 16] = *b"0123456789ABCDEF";

fn padded(password: &[u8]) -> Vec<u8> {
    let mut out = password.to_vec();
    out.extend_from_slice(&PAD[..32 - password.len()]);
    out
}

fn hex(bytes: &[u8]) -> String {
    let digits: String = bytes.iter().map(|b| format!("{b:02X}")).collect();
    format!("<{digits}>")
}

/// A revision of the standard handler: its V and R, and the key's length
/// in bytes for the RC4 ones.
#[derive(Clone, Copy, Debug)]
enum Scheme {
    Rc4 { v: i64, r: i64, length: usize },
    Aes128,
    Aes256,
}

/// The file's key and the dictionary's entries, for a user and an owner
/// password.
fn security(scheme: Scheme, user: &str, owner: &str) -> (Vec<u8>, String) {
    match scheme {
        Scheme::Rc4 { v, r, length } => legacy(v, r, length, user, owner, ""),
        Scheme::Aes128 => legacy(
            4,
            4,
            16,
            user,
            owner,
            "/CF << /StdCF << /CFM /AESV2 /AuthEvent /DocOpen /Length 16 >> >> /StmF /StdCF /StrF /StdCF",
        ),
        Scheme::Aes256 => revision_6(user, owner),
    }
}

/// Revisions 2 to 4, [ISO 32000-2] algorithms 2, 3, 4 and 5.
fn legacy(
    v: i64,
    r: i64,
    length: usize,
    user: &str,
    owner: &str,
    filters: &str,
) -> (Vec<u8>, String) {
    let rounds = |mut hash: [u8; 16]| {
        if r >= 3 {
            for _ in 0..50 {
                hash = wp_hash::md5(&hash[..length]);
            }
        }
        hash
    };
    // The owner entry: the user's password under a key from the owner's.
    let owner_key = rounds(wp_hash::md5(&padded(owner.as_bytes())))[..length].to_vec();
    let mut o = wp_cipher::rc4(&owner_key, &padded(user.as_bytes()));
    if r >= 3 {
        for round in 1..=19u8 {
            let key: Vec<u8> = owner_key.iter().map(|b| b ^ round).collect();
            o = wp_cipher::rc4(&key, &o);
        }
    }
    let mut input = padded(user.as_bytes());
    input.extend_from_slice(&o);
    input.extend_from_slice(&PERMISSIONS.to_le_bytes());
    input.extend_from_slice(&ID);
    let key = rounds(wp_hash::md5(&input))[..length].to_vec();
    let u = if r == 2 {
        wp_cipher::rc4(&key, &PAD)
    } else {
        let mut input = PAD.to_vec();
        input.extend_from_slice(&ID);
        let mut u = wp_cipher::rc4(&key, &wp_hash::md5(&input));
        for round in 1..=19u8 {
            let round_key: Vec<u8> = key.iter().map(|b| b ^ round).collect();
            u = wp_cipher::rc4(&round_key, &u);
        }
        u.extend_from_slice(&[0; 16]);
        u
    };
    let entries = format!(
        "/Filter /Standard /V {v} /R {r} /Length {} /O {} /U {} /P {PERMISSIONS} {filters}",
        length * 8,
        hex(&o),
        hex(&u)
    );
    (key, entries)
}

/// The password's hash in revision 6, algorithm 2.B.
fn hash_2b(password: &[u8], salt: &[u8], extra: &[u8]) -> Vec<u8> {
    let mut input = password.to_vec();
    input.extend_from_slice(salt);
    input.extend_from_slice(extra);
    let mut k = wp_hash::sha256(&input).to_vec();
    let mut round = 0u32;
    loop {
        let mut one = password.to_vec();
        one.extend_from_slice(&k);
        one.extend_from_slice(extra);
        let key = wp_cipher::Key::new(&k[..16]).unwrap();
        let start: [u8; 16] = k[16..32].try_into().unwrap();
        let e = wp_cipher::encrypt_cbc(&key, &start, &one.repeat(64));
        k = match e[..16].iter().map(|&b| u32::from(b)).sum::<u32>() % 3 {
            0 => wp_hash::sha256(&e).to_vec(),
            1 => wp_hash::sha384(&e).to_vec(),
            _ => wp_hash::sha512(&e).to_vec(),
        };
        round += 1;
        if round >= 64 && u32::from(*e.last().unwrap()) + 32 <= round {
            break;
        }
    }
    k[..32].to_vec()
}

/// Revision 6: a key of the test's choosing, sealed under each password.
fn revision_6(user: &str, owner: &str) -> (Vec<u8>, String) {
    let key: Vec<u8> = (0..32u8).map(|i| i.wrapping_mul(37).wrapping_add(11)).collect();
    let seal = |unlocking: &[u8]| {
        wp_cipher::encrypt_cbc(&wp_cipher::Key::new(unlocking).unwrap(), &[0; 16], &key)
    };
    let (user_check, user_seal) = (*b"uvsaltuv", *b"uksaltuk");
    let mut u = hash_2b(user.as_bytes(), &user_check, &[]);
    u.extend_from_slice(&user_check);
    u.extend_from_slice(&user_seal);
    let ue = seal(&hash_2b(user.as_bytes(), &user_seal, &[]));
    let (owner_check, owner_seal) = (*b"ovsaltov", *b"oksaltok");
    let mut o = hash_2b(owner.as_bytes(), &owner_check, &u);
    o.extend_from_slice(&owner_check);
    o.extend_from_slice(&owner_seal);
    let oe = seal(&hash_2b(owner.as_bytes(), &owner_seal, &u));
    let mut perms = PERMISSIONS.to_le_bytes().to_vec();
    perms.extend_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF, b'T', b'a', b'd', b'b', 1, 2, 3, 4]);
    let perms = wp_cipher::encrypt_ecb(&wp_cipher::Key::new(&key).unwrap(), &perms);
    let entries = format!(
        "/Filter /Standard /V 5 /R 6 /Length 256 /CF << /StdCF << /CFM /AESV3 /AuthEvent /DocOpen /Length 32 >> >> /StmF /StdCF /StrF /StdCF /O {} /U {} /OE {} /UE {} /Perms {} /P {PERMISSIONS}",
        hex(&o),
        hex(&u),
        hex(&oe),
        hex(&ue),
        hex(&perms)
    );
    (key, entries)
}

/// A string or stream of object `number` enciphered.
fn encipher(scheme: Scheme, key: &[u8], number: u32, data: &[u8]) -> Vec<u8> {
    let object_key = |aes: bool| {
        let mut input = key.to_vec();
        input.extend_from_slice(&number.to_le_bytes()[..3]);
        input.extend_from_slice(&[0, 0]);
        if aes {
            input.extend_from_slice(b"sAlT");
        }
        wp_hash::md5(&input)[..(key.len() + 5).min(16)].to_vec()
    };
    let aes = |key: &[u8]| {
        let start = [number as u8; 16];
        let mut padded = data.to_vec();
        let pad = 16 - data.len() % 16;
        padded.extend(std::iter::repeat_n(pad as u8, pad));
        let mut out = start.to_vec();
        out.extend(wp_cipher::encrypt_cbc(&wp_cipher::Key::new(key).unwrap(), &start, &padded));
        out
    };
    match scheme {
        Scheme::Rc4 { .. } => wp_cipher::rc4(&object_key(false), data),
        Scheme::Aes128 => aes(&object_key(true)),
        Scheme::Aes256 => aes(key),
    }
}

/// A page of text with a title, encrypted.
fn encrypted_page(scheme: Scheme, user: &str, owner: &str) -> Vec<u8> {
    let (key, entries) = security(scheme, user, owner);
    let content = b"BT /F1 24 Tf 72 700 Td (Hello secret world) Tj ET";
    let content = encipher(scheme, &key, 4, content);
    let title = encipher(scheme, &key, 6, b"A secret title");
    let objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".to_vec(),
        {
            let mut stream = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
            stream.extend_from_slice(&content);
            stream.extend_from_slice(b"\nendstream");
            stream
        },
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_vec(),
        format!("<< /Title {} >>", hex(&title)).into_bytes(),
        format!("<< {entries} >>").into_bytes(),
    ];
    let mut pdf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        pdf.extend_from_slice(object);
        pdf.extend_from_slice(b"\nendobj\n");
    }
    let xref = pdf.len();
    pdf.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R /Info 6 0 R /Encrypt 7 0 R /ID [{} {}] >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1,
            hex(&ID),
            hex(&ID)
        )
        .as_bytes(),
    );
    pdf
}

/// The characters poppler draws of a PDF opened with a password, which
/// its helper reads from its input.
fn poppler_text(pdf: &[u8], password: &str, name: &str) -> String {
    let folder = folder(name);
    std::fs::write(folder.join("page.pdf"), pdf).unwrap();
    let mut child = Command::new("/usr/lib/libreoffice/program/xpdfimport")
        .arg(folder.join("page.pdf"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("LibreOffice's PDF import helper runs");
    {
        use std::io::Write;
        let mut input = child.stdin.take().unwrap();
        let _ = writeln!(input, "{password}");
    }
    let output = child.wait_with_output().unwrap();
    let _ = std::fs::remove_dir_all(&folder);
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| line.starts_with("drawChar "))
        .filter_map(|line| line.rsplit(' ').next())
        .collect()
}

#[test]
fn every_revision_of_the_standard_handler_is_undone() {
    let schemes = [
        Scheme::Rc4 { v: 1, r: 2, length: 5 },
        Scheme::Rc4 { v: 2, r: 3, length: 16 },
        Scheme::Aes128,
        Scheme::Aes256,
    ];
    for scheme in schemes {
        let pdf = encrypted_page(scheme, "user", "owner");
        let what = format!("{scheme:?}");
        for password in ["user", "owner"] {
            assert_eq!(
                poppler_text(&pdf, password, &format!("{what}-{password}")),
                "Hellosecretworld",
                "poppler, {what}, {password}"
            );
            let document = wp_pdf::open_with_password(&pdf, password).expect(&what);
            assert_eq!(text_of(&document), "Hello secret world", "{what}, {password}");
            assert_eq!(document.properties().title, "A secret title", "{what}, {password}");
        }
        assert!(matches!(wp_pdf::open(&pdf), Err(wp_pdf::ReadError::Encrypted)), "{what}");
        // The empty password opens a file whose user password is empty.
        let open = encrypted_page(scheme, "", "owner");
        let document = wp_pdf::open(&open).expect(&what);
        assert_eq!(text_of(&document), "Hello secret world", "{what}, no password");
    }
}
