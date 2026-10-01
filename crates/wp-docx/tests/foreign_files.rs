//! Files another program wrote, saved again.
//!
//! A file this program wrote comes back byte for byte whatever is kept of it,
//! because the same writer twice writes the same bytes. The question whether
//! a save keeps what nobody changed can only be asked of a file somebody
//! else made: one put together by hand here, every choice in its archive
//! made otherwise than this program makes it, and one LibreOffice wrote.

#[path = "../../wp-zip/tests/foreign/mod.rs"]
mod foreign;

use std::process::Command;

use wp_docx::model::{Block, Body, Paragraph};
use wp_docx::{Document, TextPosition};
use wp_zip::{ZipArchive, ZipWriter};

/// A document as another program might write it: the parts in an order of
/// its own with the content types among them, stored and compressed, a
/// timestamp of nought, extra fields, comments, data descriptors with and
/// without their signature, a folder entry, and a comment on the whole.
fn written_elsewhere() -> Vec<u8> {
    written_elsewhere_with(&[])
}

/// The same, with more entries after the rest.
fn written_elsewhere_with(more: &[foreign::Entry]) -> Vec<u8> {
    let parts = foreign::document_parts("Written by hand");
    let [types, package_relationships, document, document_relationships, styles, core] =
        <[foreign::Entry; 6]>::try_from(parts).unwrap();
    let mut entries = vec![
        document.with_extra_fields().with_comment("the text"),
        styles.stored().at(0, 0),
        types.stored().described(true),
        foreign::Entry::new("docProps/", b"").stored(),
        core.described(false).with_extra_fields(),
        package_relationships,
        document_relationships,
    ];
    entries.extend_from_slice(more);
    foreign::archive(&entries, b"written by another program")
}

/// Every entry of the first archive that the second still has, held
/// against it: all but the parts named must be stored exactly as they were.
fn assert_copied_but(before: &[u8], after: &[u8], changed: &[&str]) {
    let before = ZipArchive::open(before).unwrap();
    let after = ZipArchive::open(after).unwrap();
    for entry in before.entries() {
        let name = &entry.name;
        let Some(kept) = after.entry(name) else { continue };
        let same = after.local_record(kept).unwrap() == before.local_record(entry).unwrap();
        if changed.contains(&name.as_str()) {
            assert!(!same, "{name} should have been written again");
        } else {
            assert!(same, "{name} was not copied as it was stored");
        }
    }
}

fn names(bytes: &[u8]) -> Vec<String> {
    let archive = ZipArchive::open(bytes).unwrap();
    archive.entries().iter().map(|entry| entry.name.clone()).collect()
}

#[test]
fn a_document_another_program_wrote_is_saved_as_it_came() {
    let original = written_elsewhere();
    let document = Document::open(&original).unwrap();
    assert_eq!(document.plain_text(), "Written by hand");
    assert!(!document.is_modified());
    assert_eq!(document.save().unwrap(), original, "the file came back different");
}

#[test]
fn saved_and_saved_again_after_an_edit_taken_back_it_is_still_the_file() {
    // The shortcut a save takes for a document that is the one on disk has
    // to reach the file's own bytes both times: straight after opening, and
    // after something was typed and taken back again, which leaves the
    // history at the state that was saved.
    let original = written_elsewhere();
    let mut document = Document::open(&original).unwrap();
    assert_eq!(document.save().unwrap(), original, "saved straight after opening");
    document.mark_saved().unwrap();

    document.set_caret(TextPosition::new(0, "Written by hand".len()));
    document.type_text(", and then some");
    assert!(document.is_modified());
    assert!(document.undo());
    assert!(!document.is_modified(), "the edit taken back is the saved document");
    assert_eq!(document.save().unwrap(), original, "saved after an edit taken back");
}

#[test]
fn an_edited_document_keeps_every_part_it_did_not_change_as_it_was_stored() {
    let original = written_elsewhere();
    let mut document = Document::open(&original).unwrap();
    document.set_caret(TextPosition::new(0, 0));
    document.type_text("Edited: ");
    let saved = document.save().unwrap();

    assert_eq!(names(&saved), names(&original), "the file's order was not kept");
    assert_copied_but(&original, &saved, &["word/document.xml"]);
    assert_eq!(ZipArchive::open(&saved).unwrap().comment(), b"written by another program");
    assert_eq!(Document::open(&saved).unwrap().plain_text(), "Edited: Written by hand");
}

#[test]
fn a_part_nothing_reaches_is_kept_when_nothing_changed_and_left_out_when_something_did() {
    // A picture no relationship reaches. A file passed through untouched is
    // the same file, the picture with it; a file saved after an edit leaves
    // it out, as Word does, and copies everything else as it was.
    let orphan = foreign::Entry::new("word/media/orphan.png", &[0x89, b'P', b'N', b'G']).stored();
    let original = written_elsewhere_with(&[orphan]);

    let mut document = Document::open(&original).unwrap();
    assert_eq!(document.save().unwrap(), original);

    document.set_caret(TextPosition::new(0, 0));
    document.type_text("Edited: ");
    let saved = document.save().unwrap();
    let mut expected = names(&original);
    expected.retain(|name| name != "word/media/orphan.png");
    assert_eq!(names(&saved), expected, "the part nothing reaches is still in the file");
    assert_copied_but(&original, &saved, &["word/document.xml"]);
}

// --- What LibreOffice wrote ---------------------------------------------------

/// The document, written here, converted by LibreOffice into a document of
/// its own making.
fn through_libreoffice() -> Vec<u8> {
    let folder = std::env::temp_dir().join(format!("wp-docx-foreign-{}", std::process::id()));
    let converted = folder.join("converted");
    let _ = std::fs::create_dir_all(&converted);

    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("Written here").with_style("Heading1")));
    body.blocks.push(Block::Paragraph(Paragraph::text("and saved by LibreOffice.")));
    let ours = folder.join("document.docx");
    std::fs::write(&ours, Document::create(&body).unwrap().save().unwrap()).unwrap();

    let output = Command::new("soffice")
        .arg(format!("-env:UserInstallation=file://{}/profile", folder.display()))
        .args(["--headless", "--convert-to", "docx", "--outdir"])
        .arg(&converted)
        .arg(&ours)
        .output()
        .unwrap_or_else(|error| {
            panic!(
                "cannot run soffice: {error}\n\
                 the build image should install libreoffice-writer-nogui"
            )
        });
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let theirs = std::fs::read(converted.join("document.docx")).expect("converted");
    let _ = std::fs::remove_dir_all(&folder);
    theirs
}

#[test]
fn a_file_libreoffice_wrote_is_saved_as_it_came_and_edited_keeps_what_was_not() {
    let theirs = through_libreoffice();

    // Writing every part again, which is what a save did, gives another
    // file; so the file coming back is a thing a save has to do on purpose.
    let archive = ZipArchive::open(&theirs).unwrap();
    let mut rebuilt = ZipWriter::new();
    for entry in archive.entries() {
        let data = archive.read(entry).unwrap();
        rebuilt.add_with(&entry.name, &data, entry.compression, entry.last_modified).unwrap();
    }
    let rebuilt = rebuilt.finish().unwrap();
    assert_ne!(rebuilt, theirs, "the parts written again are the file, which proves nothing");

    let mut document = Document::open(&theirs).unwrap();
    assert!(document.plain_text().contains("saved by LibreOffice"));
    assert_eq!(document.save().unwrap(), theirs, "LibreOffice's file came back different");

    document.set_caret(TextPosition::new(0, 0));
    document.type_text("Edited: ");
    let saved = document.save().unwrap();
    assert_eq!(names(&saved), names(&theirs), "a part was lost, or the order was not kept");
    assert_copied_but(&theirs, &saved, &["word/document.xml"]);
    assert!(Document::open(&saved).unwrap().plain_text().starts_with("Edited: Written here"));
}
