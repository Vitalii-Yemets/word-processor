//! Encrypted binary documents, and another program that agrees they are.
//!
//! LibreOffice, which is in the build image, writes a `.doc` with a password
//! the way Word 97 did — RC4 — and opens one encrypted any of the three ways
//! Word did: that, RC4 through the system's cryptography as Word 2002 did,
//! and Word 95's exclusive-or. It is asked through a macro of its own, put
//! in a profile made for the purpose, since its command line has no word
//! for a password.
//!
//! The first kind is made by LibreOffice and opened here. The other two are
//! made here from a document LibreOffice wrote, by the specification, with
//! the ciphers worked out in this file rather than asked of the code being
//! tested — and LibreOffice opening them with the password is what says
//! they were made right. Then they are opened here.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Mutex;

/// LibreOffice takes turns with itself.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn run(command: &mut Command) -> std::io::Result<Output> {
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    command.output()
}

const PASSWORD: &str = "secret";

const PAGE: &str = r##"<html><head><meta charset="utf-8"></head><body>
<p>Hello, <b>world</b>.</p>
<p>Привет, мир — a second paragraph, long enough that the streams run to several blocks of the cipher once the formatting and the tables are counted.</p>
</body></html>
"##;

/// What LibreOffice says the page is, as text.
const TEXT: &str = "Hello, world.";

/// The macros: one saving a file as Word 97 with a password, one opening a
/// file with a password and saving it as text.
const MODULE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE script:module PUBLIC "-//OpenOffice.org//DTD OfficeDocument 1.0//EN" "module.dtd">
<script:module xmlns:script="http://openoffice.org/2000/script" script:name="Module1" script:language="StarBasic">
Sub SaveWithPassword(source As String, target As String, secret As String)
  Dim open_args(0) As New com.sun.star.beans.PropertyValue
  open_args(0).Name = &quot;Hidden&quot;
  open_args(0).Value = True
  doc = StarDesktop.loadComponentFromURL(ConvertToURL(source), &quot;_blank&quot;, 0, open_args())
  Dim save(1) As New com.sun.star.beans.PropertyValue
  save(0).Name = &quot;FilterName&quot;
  save(0).Value = &quot;MS Word 97&quot;
  save(1).Name = &quot;Password&quot;
  save(1).Value = secret
  doc.storeToURL(ConvertToURL(target), save())
  doc.close(True)
End Sub

Sub OpenWithPassword(source As String, target As String, secret As String)
  Dim open_args(1) As New com.sun.star.beans.PropertyValue
  open_args(0).Name = &quot;Hidden&quot;
  open_args(0).Value = True
  open_args(1).Name = &quot;Password&quot;
  open_args(1).Value = secret
  doc = StarDesktop.loadComponentFromURL(ConvertToURL(source), &quot;_blank&quot;, 0, open_args())
  Dim save(0) As New com.sun.star.beans.PropertyValue
  save(0).Name = &quot;FilterName&quot;
  save(0).Value = &quot;Text&quot;
  doc.storeToURL(ConvertToURL(target), save())
  doc.close(True)
End Sub
</script:module>
"#;

/// A LibreOffice of its own: a folder, a profile with the macros in it, and
/// the page written as a plain Word 97 file, which making the profile does.
struct Office {
    folder: PathBuf,
}

impl Office {
    fn new(name: &str) -> Self {
        let folder = std::env::temp_dir().join(format!("wp-doc-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).expect("a folder");
        std::fs::write(folder.join("page.html"), PAGE).expect("the page");
        let office = Self { folder };
        let output = office.soffice(&[
            "--convert-to",
            "doc:MS Word 97",
            "--outdir",
            &office.path("").display().to_string(),
            &office.path("page.html").display().to_string(),
        ]);
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        let module = office.path("profile/user/basic/Standard/Module1.xba");
        assert!(module.exists(), "the profile has no Basic library");
        std::fs::write(module, MODULE).expect("the macros");
        office
    }

    fn path(&self, name: &str) -> PathBuf {
        self.folder.join(name)
    }

    /// LibreOffice, with this profile, given no more than two minutes: one
    /// asked for a password it cannot give waits for ever.
    fn soffice(&self, arguments: &[&str]) -> Output {
        run(Command::new("timeout")
            .arg("120")
            .arg("soffice")
            .arg(format!("-env:UserInstallation=file://{}", self.path("profile").display()))
            .arg("--headless")
            .args(arguments))
        .unwrap_or_else(|error| {
            panic!(
                "cannot run soffice: {error}\nthe build image should install libreoffice-writer-nogui"
            )
        })
    }

    fn macro_call(&self, name: &str, source: &Path, target: &Path) {
        let call = format!(
            "macro:///Standard.Module1.{name}(\"{}\",\"{}\",\"{PASSWORD}\")",
            source.display(),
            target.display()
        );
        let _ = self.soffice(&[&call]);
    }

    /// The page as LibreOffice writes it without a password.
    fn plain(&self) -> Vec<u8> {
        std::fs::read(self.path("page.doc")).expect("the plain document")
    }

    /// The page as LibreOffice writes it with the password.
    fn locked(&self) -> Vec<u8> {
        let target = self.path("locked.doc");
        self.macro_call("SaveWithPassword", &self.path("page.html"), &target);
        std::fs::read(target).expect("LibreOffice wrote no encrypted document")
    }

    /// What LibreOffice reads in a file, given the password: its text, or
    /// nothing if it could not open it.
    fn text_of(&self, bytes: &[u8]) -> Option<String> {
        let (source, target) = (self.path("given.doc"), self.path("given.txt"));
        let _ = std::fs::remove_file(&target);
        std::fs::write(&source, bytes).expect("the file");
        self.macro_call("OpenWithPassword", &source, &target);
        std::fs::read_to_string(target).ok()
    }
}

impl Drop for Office {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.folder);
    }
}

/// The streams of a compound file, by name.
fn streams(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let file = wp_ole::CompoundFile::open(bytes.to_vec()).expect("a compound file");
    file.entries()
        .iter()
        .filter(|entry| entry.kind == wp_ole::EntryKind::Stream)
        .map(|entry| (entry.name.clone(), file.stream(&entry.name).expect("its stream")))
        .collect()
}

fn build(streams: Vec<(String, Vec<u8>)>) -> Vec<u8> {
    let mut builder = wp_ole::Builder::new();
    for (name, bytes) in streams {
        builder.stream(&name, bytes);
    }
    builder.build()
}

/// Marks the main stream encrypted, keeping the first sixty-eight bytes
/// readable as the format does.
fn mark_encrypted(word: &mut [u8], obfuscated: bool, key: u32) {
    let mut flags = u16::from_le_bytes([word[10], word[11]]) | 0x0100;
    if obfuscated {
        flags |= 0x8000;
    }
    word[10..12].copy_from_slice(&flags.to_le_bytes());
    word[14..18].copy_from_slice(&key.to_le_bytes());
}

// --- Word 95's exclusive-or, from MS-OFFCRYPTO 2.3.7 ----------------------------------

const INITIAL_CODE: [u16; 15] = [
    0xE1F0, 0x1D0F, 0xCC9C, 0x84C0, 0x110C, 0x0E10, 0xF1CE, 0x313E, 0x1872, 0xE139, 0xD40F, 0x84F9,
    0x280C, 0xA96A, 0x4EC3,
];

/// The matrix, made here as the specification builds it: each row's first
/// number, and each after it doubled with the polynomial folded back in.
fn matrix() -> Vec<u16> {
    const FIRSTS: [u16; 15] = [
        0xAEFC, 0x7B61, 0x4563, 0x0375, 0xD849, 0x6F45, 0xEB23, 0x47D3, 0xB861, 0x45A0, 0xAA51,
        0x76B4, 0x3730, 0x3331, 0x1021,
    ];
    let mut out = Vec::with_capacity(105);
    for first in FIRSTS {
        let mut value = first;
        for _ in 0..7 {
            out.push(value);
            value = if value & 0x8000 == 0 { value << 1 } else { (value << 1) ^ 0x1021 };
        }
    }
    out
}

/// The key, the verifier and the sixteen bytes a password makes.
fn xor_of(password: &str) -> (u16, u16, [u8; 16]) {
    let bytes: Vec<u8> = password.bytes().collect();
    let matrix = matrix();
    let mut key = INITIAL_CODE[bytes.len() - 1];
    let mut element = 0x68usize;
    for &byte in bytes.iter().rev() {
        let mut bits = byte;
        for _ in 0..7 {
            if bits & 0x40 != 0 {
                key ^= matrix[element];
            }
            bits <<= 1;
            element = element.wrapping_sub(1);
        }
    }
    let mut verifier: u16 = 0;
    for byte in bytes.iter().rev().chain(core::iter::once(&(bytes.len() as u8))) {
        verifier = ((verifier >> 14) & 1 | (verifier << 1) & 0x7FFF) ^ u16::from(*byte);
    }
    verifier ^= 0xCE4B;
    const PAD: [u8; 15] =
        [0xBB, 0xFF, 0xFF, 0xBA, 0xFF, 0xFF, 0xB9, 0x80, 0x00, 0xBE, 0x0F, 0x00, 0xBF, 0x0F, 0x00];
    let halves = key.to_le_bytes();
    let array = core::array::from_fn(|at| {
        let source = bytes.get(at).copied().unwrap_or_else(|| PAD[at - bytes.len()]);
        (source ^ halves[at % 2]).rotate_right(1)
    });
    (key, verifier, array)
}

fn xor_apply(bytes: &mut [u8], array: &[u8; 16]) {
    for (at, byte) in bytes.iter_mut().enumerate() {
        let changed = *byte ^ array[at % 16];
        if *byte != 0 && changed != 0 {
            *byte = changed;
        }
    }
}

/// The plain document, obfuscated as Word 97 would with its weak option.
fn obfuscated(plain: &[u8]) -> Vec<u8> {
    let (key, verifier, array) = xor_of(PASSWORD);
    let streams = streams(plain)
        .into_iter()
        .map(|(name, mut bytes)| {
            match name.as_str() {
                "WordDocument" => {
                    mark_encrypted(&mut bytes, true, u32::from(verifier) | (u32::from(key) << 16));
                    let kept = bytes[..0x44].to_vec();
                    xor_apply(&mut bytes, &array);
                    bytes[..0x44].copy_from_slice(&kept);
                }
                "1Table" | "0Table" | "Data" => xor_apply(&mut bytes, &array),
                _ => {}
            }
            (name, bytes)
        })
        .collect();
    build(streams)
}

// --- Word 2002's RC4 through CryptoAPI, from MS-OFFCRYPTO 2.3.5 -----------------------

fn cryptoapi_key(salt: &[u8], block: u32) -> Vec<u8> {
    let mut first = salt.to_vec();
    for unit in PASSWORD.encode_utf16() {
        first.extend_from_slice(&unit.to_le_bytes());
    }
    let mut with_block = wp_hash::sha1(&first).to_vec();
    with_block.extend_from_slice(&block.to_le_bytes());
    wp_hash::sha1(&with_block)[..16].to_vec()
}

fn rc4_blocks(bytes: &mut [u8], salt: &[u8]) {
    for (number, block) in bytes.chunks_mut(512).enumerate() {
        wp_cipher::Rc4::new(&cryptoapi_key(salt, number as u32)).apply(block);
    }
}

/// The description at the head of the table stream: the version, the flags,
/// the header naming RC4 and SHA-1 and a key of a hundred and twenty-eight
/// bits, and the verifier.
fn cryptoapi_description(salt: &[u8]) -> Vec<u8> {
    let flags = 0x04u32;
    let mut header = Vec::new();
    for value in [flags, 0, 0x6801, 0x8004, 128, 1, 0, 0] {
        header.extend_from_slice(&value.to_le_bytes());
    }
    for unit in "Microsoft Enhanced Cryptographic Provider v1.0\0".encode_utf16() {
        header.extend_from_slice(&unit.to_le_bytes());
    }
    let plain = [0x5Au8; 16];
    let mut both = plain.to_vec();
    both.extend_from_slice(&wp_hash::sha1(&plain));
    wp_cipher::Rc4::new(&cryptoapi_key(salt, 0)).apply(&mut both);

    let mut out = Vec::new();
    out.extend_from_slice(&3u16.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&flags.to_le_bytes());
    out.extend_from_slice(&(header.len() as u32).to_le_bytes());
    out.extend_from_slice(&header);
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(salt);
    out.extend_from_slice(&both[..16]);
    out.extend_from_slice(&20u32.to_le_bytes());
    out.extend_from_slice(&both[16..]);
    out
}

/// The offset pairs of a Word 97 block: where they are in the stream.
fn pairs(word: &[u8]) -> (usize, usize) {
    let u16_at = |at: usize| usize::from(u16::from_le_bytes([word[at], word[at + 1]]));
    let mut at = 32;
    at += 2 + u16_at(at) * 2;
    at += 2 + u16_at(at) * 4;
    (at + 2, u16_at(at))
}

/// The plain document, encrypted as Word 2002 did: the description put at
/// the head of the table stream, everything the block finds there moved
/// along by its length, and every stream enciphered from its first byte but
/// the parts left readable.
fn cryptoapi(plain: &[u8]) -> Vec<u8> {
    let salt = [0x33u8; 16];
    let description = cryptoapi_description(&salt);
    let shift = description.len() as u32;
    let streams = streams(plain)
        .into_iter()
        .map(|(name, mut bytes)| {
            match name.as_str() {
                "WordDocument" => {
                    let (at, count) = pairs(&bytes);
                    for index in 0..count {
                        let place = at + index * 8;
                        let offset =
                            u32::from_le_bytes(bytes[place..place + 4].try_into().unwrap());
                        let length =
                            u32::from_le_bytes(bytes[place + 4..place + 8].try_into().unwrap());
                        if length > 0 {
                            bytes[place..place + 4]
                                .copy_from_slice(&(offset + shift).to_le_bytes());
                        }
                    }
                    mark_encrypted(&mut bytes, false, shift);
                    let kept = bytes[..0x44].to_vec();
                    rc4_blocks(&mut bytes, &salt);
                    bytes[..0x44].copy_from_slice(&kept);
                }
                "1Table" | "0Table" => {
                    let mut moved = description.clone();
                    moved.extend_from_slice(&bytes);
                    rc4_blocks(&mut moved, &salt);
                    moved[..description.len()].copy_from_slice(&description);
                    bytes = moved;
                }
                "Data" => rc4_blocks(&mut bytes, &salt),
                _ => {}
            }
            (name, bytes)
        })
        .collect();
    build(streams)
}

fn assert_opens(bytes: &[u8], what: &str) {
    assert!(wp_doc::is_encrypted(bytes), "{what} is not marked encrypted");
    assert_eq!(wp_doc::read(bytes).err(), Some(wp_doc::Error::Encrypted), "{what}");
    assert_eq!(
        wp_doc::read_with_password(bytes, Some("not it")).err(),
        Some(wp_doc::Error::WrongPassword),
        "{what}"
    );
    let reading = wp_doc::read_with_password(bytes, Some(PASSWORD)).expect(what);
    let text = reading.body.plain_text();
    assert!(text.starts_with(TEXT), "{what}: {text:?}");
    assert!(text.contains("Привет, мир"), "{what}: {text:?}");
    let document = wp_doc::open_with_password(bytes, Some(PASSWORD)).expect(what);
    assert_eq!(document.password(), Some(PASSWORD), "{what}: the password is not kept");
}

#[test]
fn a_document_libreoffice_encrypts_opens_with_its_password() {
    let office = Office::new("rc4");
    let locked = office.locked();
    assert_opens(&locked, "LibreOffice's RC4");
}

#[test]
fn a_document_obfuscated_the_old_way_opens_with_its_password() {
    let office = Office::new("xor");
    let file = obfuscated(&office.plain());
    let theirs = office.text_of(&file).expect("LibreOffice could not open the obfuscated file");
    assert!(theirs.contains(TEXT), "{theirs:?}");
    assert_opens(&file, "Word 95's exclusive-or");
}

#[test]
fn a_document_encrypted_through_cryptoapi_opens_with_its_password() {
    let office = Office::new("cryptoapi");
    let file = cryptoapi(&office.plain());
    let theirs = office.text_of(&file).expect("LibreOffice could not open the RC4 CryptoAPI file");
    assert!(theirs.contains(TEXT), "{theirs:?}");
    assert_opens(&file, "RC4 through CryptoAPI");
}
