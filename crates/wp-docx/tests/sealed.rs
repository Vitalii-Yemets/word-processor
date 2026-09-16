//! A document that is encrypted, and stays encrypted when it is saved.

use wp_docx::model::{Block, Body, Paragraph};
use wp_docx::{Document, Error};

const SECRET: &str = "The quick brown fox jumps over the lazy dog";

fn document() -> Document {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text(SECRET)));
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

fn fresh() -> wp_crypt::Fresh {
    // The same every time, because a test that cannot be run twice with the
    // same answer is not a test. A document has this from the machine.
    wp_crypt::Fresh::from_bytes(&core::array::from_fn(|at| (at * 31 + 7) as u8))
}

#[test]
fn a_document_with_a_password_is_written_encrypted_and_opens_again() {
    let mut document = document();
    assert_eq!(document.password(), None);
    assert!(document.set_password(Some("Fenchurch")));
    assert_eq!(document.password(), Some("Fenchurch"));

    let sealed = document.save_sealed(&fresh()).expect("saving");
    assert!(wp_docx::sealing::is_sealed(&sealed), "it was written as a plain package");

    // What is in the file is not the document.
    assert!(
        !sealed.windows(SECRET.len()).any(|window| window == SECRET.as_bytes()),
        "the text is in the file in plain sight"
    );

    let reopened = Document::open_sealed(&sealed, "Fenchurch").expect("opening");
    assert_eq!(reopened.plain_text(), SECRET);
    // And it remembers, so that saving it again keeps it encrypted.
    assert_eq!(reopened.password(), Some("Fenchurch"));
    assert!(wp_docx::sealing::is_sealed(&reopened.save_sealed(&fresh()).expect("saving again")));
}

#[test]
fn an_encrypted_document_opened_the_ordinary_way_says_what_it_is() {
    let mut document = document();
    document.set_password(Some("Fenchurch"));
    let sealed = document.save_sealed(&fresh()).expect("saving");

    // Not "this is not a zip", which is what the package layer would have
    // said and which tells a person nothing they can act on.
    assert_eq!(Document::open(&sealed).err(), Some(Error::Sealed));
    assert!(matches!(
        Document::open_sealed(&sealed, "fenchurch"),
        Err(Error::Unsealing(wp_crypt::Error::WrongPassword))
    ));
}

#[test]
fn taking_the_password_off_writes_a_plain_package_again() {
    let mut document = document();
    document.set_password(Some("Fenchurch"));
    assert!(document.set_password(None));
    assert!(!document.set_password(None), "nothing changed the second time");

    let saved = document.save_sealed(&fresh()).expect("saving");
    assert!(!wp_docx::sealing::is_sealed(&saved));
    assert_eq!(Document::open(&saved).expect("opening").plain_text(), SECRET);
}

#[test]
fn an_empty_password_is_no_password() {
    let mut document = document();
    assert!(!document.set_password(Some("")), "an empty box encrypts nothing");
    assert_eq!(document.password(), None);
}

/// Two saves of the same document under the same password must not come out
/// the same, or the salt is doing nothing.
#[test]
fn two_saves_are_not_the_same_bytes() {
    let mut document = document();
    document.set_password(Some("Fenchurch"));
    let once = document.save_sealed(&fresh()).expect("saving");
    let again = document
        .save_sealed(&wp_crypt::Fresh::from_bytes(&core::array::from_fn(|at| (at * 17 + 3) as u8)))
        .expect("saving");
    assert_ne!(once, again);
    // And both open.
    for bytes in [once, again] {
        assert_eq!(
            Document::open_sealed(&bytes, "Fenchurch").expect("opening").plain_text(),
            SECRET
        );
    }
}

// --- What another program makes of it -----------------------------------------

/// What LibreOffice makes of a document this program encrypted.
///
/// It cannot be asked, and that is worth writing down rather than leaving to
/// be discovered. LibreOffice 7.4, which is what the build image carries,
/// takes no password on its command line and none through `--infilter`: it
/// converts a document it can open, and an encrypted one it cannot open
/// without asking a question there is nobody there to answer. So this is
/// kept, and skipped, against an image with something that can.
///
/// What stands in its place is `wp-crypt`'s own test, which lays an encrypted
/// document out by hand from the specification — with the key's arithmetic
/// done in the test rather than by the code being tested — and holds this
/// program to reading it.
#[test]
#[ignore = "LibreOffice takes no password on its command line"]
fn another_program_opens_what_this_one_encrypted() {
    let folder = std::env::temp_dir().join(format!("wp-docx-sealed-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&folder);
    let mut document = document();
    document.set_password(Some("Fenchurch"));
    let sealed = document.save_sealed(&fresh()).expect("saving");
    let path = folder.join("sealed.docx");
    std::fs::write(&path, sealed).expect("writing");

    let output = std::process::Command::new("soffice")
        .arg(format!("-env:UserInstallation=file://{}/profile", folder.display()))
        .args(["--headless", "--convert-to", "txt:Text"])
        .arg("--infilter=MS Word 2007 XML:Password=Fenchurch")
        .arg("--outdir")
        .arg(&folder)
        .arg(&path)
        .output()
        .expect("soffice is in the build image");

    let text = std::fs::read_to_string(folder.join("sealed.txt")).unwrap_or_else(|error| {
        panic!(
            "LibreOffice did not read it: {error}; it said {}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    let _ = std::fs::remove_dir_all(&folder);
    assert!(text.contains(SECRET), "LibreOffice read it as {text:?}");
}
