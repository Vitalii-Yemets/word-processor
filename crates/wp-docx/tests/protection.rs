//! Who may edit a document, the password that says so, and the forms that
//! are the one thing a form-protected document lets through.
//!
//! The passwords here are hashed for as many turns as the format asks for
//! rather than a token few: what a hundred thousand turns of SHA-512 costs is
//! part of what is being claimed, and a test that skipped it would not have
//! shown that saving a document with a password is something a person can wait
//! for.

use std::path::Path;
use std::process::{Command, Output};
use std::sync::Mutex;

use wp_docx::forms::FormKind;
use wp_docx::model::{Block, Body, Paragraph};
use wp_docx::protection::{EditMode, Protection};
use wp_docx::{Document, TextPosition};

fn document() -> Document {
    let mut body = Body::default();
    body.blocks.push(Block::Paragraph(Paragraph::text("plain")));
    let bytes = Document::create(&body).expect("a document").save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

fn round_trip(document: &Document) -> Document {
    let bytes = document.save().expect("saving");
    Document::open(&bytes).expect("reopening")
}

fn settings_of(document: &Document) -> String {
    let bytes = document.save().expect("saving");
    let package = wp_opc::Package::open(&bytes).expect("a package");
    package.xml_part("word/settings.xml").expect("the settings").expect("readable settings")
}

// --- The restriction itself ---------------------------------------------------

#[test]
fn a_document_is_not_protected() {
    assert_eq!(document().protection(), None);
}

#[test]
fn every_kind_of_protection_survives_saving() {
    for mode in EditMode::ALL {
        let mut document = document();
        let wanted = Protection::new(*mode);
        assert!(document.set_protection(Some(&wanted)));
        assert_eq!(round_trip(&document).protection(), Some(*mode), "{}", mode.label());
    }
}

#[test]
fn protection_can_be_lifted() {
    let mut document = document();
    document.set_protection(Some(&Protection::new(EditMode::ReadOnly)));
    assert!(document.set_protection(None));
    assert_eq!(round_trip(&document).protection(), None);
}

#[test]
fn setting_the_same_protection_twice_changes_nothing() {
    let mut document = document();
    let wanted = Protection::new(EditMode::Comments);
    assert!(document.set_protection(Some(&wanted)));
    assert!(!document.set_protection(Some(&wanted)));
}

#[test]
fn protection_that_is_not_enforced_is_no_protection() {
    let mut document = document();
    document.set_protection(Some(&Protection::new(EditMode::ReadOnly)));

    // The same element with the enforcement turned off, which is what Word
    // writes when somebody sets a restriction and then does not apply it.
    let bytes = document.save().expect("saving");
    let mut package = wp_opc::Package::open(&bytes).expect("a package");
    let settings = package
        .xml_part("word/settings.xml")
        .expect("the settings")
        .expect("readable settings")
        .replace(r#"w:enforcement="1""#, r#"w:enforcement="0""#);
    package.set_part("word/settings.xml", settings.into_bytes());

    let bytes = package.save().expect("saving the package");
    assert_eq!(Document::open(&bytes).expect("reopening").protection(), None);
}

#[test]
fn a_document_restricted_to_tracked_changes_is_recording_them() {
    let mut document = document();
    assert!(!document.tracking_changes());
    document.set_protection(Some(&Protection::new(EditMode::TrackedChanges)));
    assert!(document.tracking_changes(), "the restriction did not start the recording");

    // And the recording cannot be switched off while it stands, which is
    // what Word's Lock Tracking is.
    assert!(!document.set_tracking_changes(false));
    assert!(document.tracking_changes());

    // A document that says nothing but the restriction is still recording
    // when it is opened again.
    let reopened = round_trip(&document);
    assert!(reopened.tracking_changes());
}

// --- The password -------------------------------------------------------------

#[test]
fn a_password_survives_saving_and_is_the_only_thing_that_lifts_it() {
    let mut document = document();
    let wanted = Protection::new(EditMode::Forms).behind("Fenchurch St Paul", b"0123456789abcdef");
    assert!(document.set_protection(Some(&wanted)));

    let rules = round_trip(&document).protection_rules().expect("still protected");
    assert_eq!(rules.mode, EditMode::Forms);
    assert!(rules.opens_with("Fenchurch St Paul"));
    assert!(!rules.opens_with("fenchurch st paul"));
    assert!(!rules.opens_with(""));
}

#[test]
fn what_is_written_is_the_set_of_attributes_word_writes_now() {
    let mut document = document();
    document.set_protection(Some(
        &Protection::new(EditMode::ReadOnly).behind("secret", b"0123456789abcdef"),
    ));
    let settings = settings_of(&document);

    for attribute in ["w:algorithmName=\"SHA-512\"", "w:hashValue=", "w:saltValue=", "w:spinCount="]
    {
        assert!(settings.contains(attribute), "no {attribute} in {settings}");
    }
    // And not the ones Word 2007 wrote, which are read and not written.
    assert!(!settings.contains("w:cryptAlgorithmSid"), "{settings}");
    assert!(!settings.contains("w:hash="), "{settings}");
}

#[test]
fn a_password_is_not_in_the_file() {
    let mut document = document();
    document.set_protection(Some(
        &Protection::new(EditMode::ReadOnly).behind("Fenchurch", b"0123456789abcdef"),
    ));
    let bytes = document.save().expect("saving");
    let needle = b"Fenchurch";
    assert!(
        !bytes.windows(needle.len()).any(|window| window == needle),
        "the password itself is in the saved file"
    );
}

/// A document carrying the attributes Word 2007 wrote, hashed the same way
/// with SHA-1.
fn word_2007_settings(word: &str, salt: &[u8], spins: u32) -> Document {
    let mut first = salt.to_vec();
    for unit in word.encode_utf16() {
        first.extend_from_slice(&unit.to_le_bytes());
    }
    let mut hash = wp_hash::sha1(&first).to_vec();
    for turn in 0..spins {
        let mut next = hash;
        next.extend_from_slice(&turn.to_le_bytes());
        hash = wp_hash::sha1(&next).to_vec();
    }

    let element = format!(
        r#"<w:documentProtection w:edit="readOnly" w:enforcement="1"
 w:cryptProviderType="rsaAES" w:cryptAlgorithmClass="hash" w:cryptAlgorithmType="typeAny"
 w:cryptAlgorithmSid="4" w:cryptSpinCount="{spins}" w:hash="{}" w:salt="{}"/>"#,
        wp_text::base64::encode(&hash),
        wp_text::base64::encode(salt),
    );
    replacing_the_protection(&element)
}

/// The same document with whatever `w:documentProtection` element is given.
fn replacing_the_protection(element: &str) -> Document {
    let mut document = document();
    // Written first so that there is one to replace, and so that it sits
    // where the schema puts it.
    document.set_protection(Some(&Protection::new(EditMode::Comments)));
    let bytes = document.save().expect("saving");

    let mut package = wp_opc::Package::open(&bytes).expect("a package");
    let settings =
        package.xml_part("word/settings.xml").expect("the settings").expect("readable settings");
    let start = settings.find("<w:documentProtection").expect("the element");
    let end = settings[start..].find("/>").expect("its end") + start + 2;
    let settings = format!("{}{element}{}", &settings[..start], &settings[end..]);
    package.set_part("word/settings.xml", settings.into_bytes());

    let bytes = package.save().expect("saving the package");
    Document::open(&bytes).expect("reopening")
}

#[test]
fn the_password_word_2007_wrote_is_read_and_checked() {
    // Word 2007's own number of turns, so that this is the document that
    // program made and not a shape of it.
    let document = word_2007_settings("Trumpington", b"sixteen bytes!!!", 50_000);
    let rules = document.protection_rules().expect("protected");
    assert_eq!(rules.mode, EditMode::ReadOnly);
    assert!(rules.opens_with("Trumpington"));
    assert!(!rules.opens_with("Trumpingtom"));
}

#[test]
fn a_hash_this_program_has_not_got_leaves_the_restriction_standing() {
    let document = replacing_the_protection(
        r#"<w:documentProtection w:edit="readOnly" w:enforcement="1"
 w:algorithmName="MD5" w:hashValue="2ju2iNGLNTMtb0K5MVnO1Q=="
 w:saltValue="c2l4dGVlbiBieXRlcyEhIQ==" w:spinCount="100000"/>"#,
    );
    let rules = document.protection_rules().expect("protected");
    assert_eq!(rules.mode, EditMode::ReadOnly, "the restriction was lost with the hash");

    let password = rules.password.as_ref().expect("a password is there");
    assert!(!password.understood());
    assert_eq!(password.algorithm_name(), "MD5");
    assert!(!rules.opens_with(""), "an answer it cannot check was taken");
    assert!(!rules.opens_with("anything"));
}

// --- Forms --------------------------------------------------------------------

/// A document with one text form field, written the way Word writes one.
fn a_form(before: &str, ffdata: &str, answer: &str) -> Document {
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:body><w:p>
<w:r><w:t>{before}</w:t></w:r>
<w:r><w:fldChar w:fldCharType="begin">{ffdata}</w:fldChar></w:r>
<w:r><w:instrText> FORMTEXT </w:instrText></w:r>
<w:r><w:fldChar w:fldCharType="separate"/></w:r>
<w:r><w:t>{answer}</w:t></w:r>
<w:r><w:fldChar w:fldCharType="end"/></w:r>
<w:r><w:t> after</w:t></w:r>
</w:p></w:body></w:document>"#
    );

    let mut package = wp_opc::Package::empty();
    package.add_part("word/document.xml", wp_opc::MAIN_DOCUMENT_CONTENT_TYPE, xml.into_bytes());
    let mut root = wp_opc::Relationships::new("");
    root.add(
        wp_opc::OFFICE_DOCUMENT_RELATIONSHIP,
        "word/document.xml",
        wp_opc::TargetMode::Internal,
    );
    package.set_relationships(&root).expect("the root relationships");
    let bytes = package.save().expect("a saved package");
    Document::open(&bytes).expect("reopening")
}

#[test]
fn a_text_form_field_is_found_and_its_answer_is_where_it_says() {
    let document = a_form(
        "Name: ",
        r#"<w:ffData><w:name w:val="Surname"/><w:enabled/><w:textInput/></w:ffData>"#,
        "Habgood",
    );
    let fields = document.form_fields();
    assert_eq!(fields.len(), 1, "{fields:?}");
    let field = &fields[0];
    assert_eq!(field.name, "Surname");
    assert_eq!(field.kind, FormKind::Text);
    assert!(field.enabled);

    // "Name: " is six characters, then the answer, then " after".
    assert_eq!(field.start, TextPosition::new(0, 6));
    assert_eq!(field.end, TextPosition::new(0, 13));
    assert!(field.covers(TextPosition::new(0, 9)));
    assert!(!field.covers(TextPosition::new(0, 5)), "the text before it is not in the field");
    assert!(!field.covers(TextPosition::new(0, 14)), "nor the text after");
    assert_eq!(document.form_field_at(TextPosition::new(0, 2)), None);
}

#[test]
fn the_other_two_kinds_are_told_apart_and_a_drop_down_says_what_it_offers() {
    let ticked = a_form("", r#"<w:ffData><w:name w:val="Agreed"/><w:checkBox/></w:ffData>"#, "");
    assert_eq!(ticked.form_fields()[0].kind, FormKind::CheckBox);

    let list = a_form(
        "",
        r#"<w:ffData><w:name w:val="Title"/><w:ddList>
<w:listEntry w:val="Mr"/><w:listEntry w:val="Ms"/><w:listEntry w:val="Dr"/></w:ddList></w:ffData>"#,
        "",
    );
    let field = &list.form_fields()[0];
    assert_eq!(field.kind, FormKind::DropDown);
    assert_eq!(field.items, vec!["Mr", "Ms", "Dr"]);
}

#[test]
fn a_field_that_may_not_be_filled_in_says_so() {
    let document = a_form(
        "",
        r#"<w:ffData><w:name w:val="Reference"/><w:enabled w:val="0"/><w:textInput/></w:ffData>"#,
        "AB-1",
    );
    assert!(!document.form_fields()[0].enabled);
}

#[test]
fn an_ordinary_field_is_not_a_form_field() {
    // The same shape with no `w:ffData`: a page number, a date, anything.
    let document = a_form("", "", "17");
    assert!(document.form_fields().is_empty(), "{:?}", document.form_fields());
}

#[test]
fn a_section_is_closed_unless_it_says_otherwise() {
    let document = document();
    assert!(document.section_is_form_protected(0), "a section that says nothing is protected");
}

// --- What another program makes of it -----------------------------------------

/// Two LibreOffices starting at once fall over each other even with profiles
/// of their own, so they take turns.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn run(command: &mut Command) -> std::io::Result<Output> {
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    command.output()
}

fn soffice(folder: &Path) -> Command {
    let mut command = Command::new("soffice");
    command
        .arg(format!("-env:UserInstallation=file://{}/profile", folder.display()))
        .args(["--headless", "--convert-to", "docx", "--outdir"])
        .arg(folder);
    command
}

#[test]
fn another_program_reads_the_restriction_and_keeps_it() {
    let folder = std::env::temp_dir().join(format!("wp-docx-protect-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&folder);

    let mut document = document();
    document.set_protection(Some(
        &Protection::new(EditMode::TrackedChanges).behind("Fenchurch", b"0123456789abcdef"),
    ));
    let ours = folder.join("protected.docx");
    std::fs::write(&ours, document.save().expect("saving")).expect("writing");
    let mine = settings_of(&document);

    let output = run(soffice(&folder).arg(&ours)).unwrap_or_else(|error| {
        panic!(
            "cannot run soffice: {error}\nthe build image should install libreoffice-writer-nogui"
        )
    });
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));

    // Converted into the same folder, so it lands beside the original under
    // the name LibreOffice gives it.
    let theirs = std::fs::read(folder.join("protected.docx")).expect("converted");
    let _ = std::fs::remove_dir_all(&folder);

    let reopened = Document::open(&theirs).expect("reopening what LibreOffice wrote");
    let rules = reopened.protection_rules().expect("LibreOffice dropped the restriction");
    assert_eq!(rules.mode, EditMode::TrackedChanges);

    // The password came back the same, which it can only do if the
    // attributes were read as a password rather than copied as unknown
    // rubbish — LibreOffice writes its own element, in its own order.
    assert!(rules.opens_with("Fenchurch"), "the password did not survive: {mine}");
    assert!(!rules.opens_with("something else"));
}
