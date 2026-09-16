//! What the desktop is told: the documents opened lately, and which
//! program opens which kind of file.
//!
//! Both are files in the person's own directories, so a test can point
//! those directories somewhere of its own and read what was written. The
//! Windows half is the registry and is not reachable from here; it is
//! named in the roadmap.
#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use wp_shell::files::{self, Kind};

/// The directories are named by the environment, which belongs to the whole
/// program: one test in them at a time.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

const KINDS: &[Kind] = &[
    Kind {
        extension: ".docx",
        description: "Word Document",
        media_type: "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        becomes_default: true,
    },
    Kind {
        extension: ".txt",
        description: "Text Document",
        media_type: "text/plain",
        becomes_default: false,
    },
];

/// Runs the work with the desktop's directories somewhere of this test's own.
fn in_a_home_of_its_own<R>(name: &str, work: impl FnOnce(&Path) -> R) -> R {
    let _held = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let home = std::env::temp_dir().join(format!("wp-desktop-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(home.join("data")).expect("somewhere to write");
    std::fs::create_dir_all(home.join("config")).expect("somewhere to write");
    let was = [
        ("XDG_DATA_HOME", std::env::var("XDG_DATA_HOME").ok()),
        ("XDG_CONFIG_HOME", std::env::var("XDG_CONFIG_HOME").ok()),
    ];
    std::env::set_var("XDG_DATA_HOME", home.join("data"));
    std::env::set_var("XDG_CONFIG_HOME", home.join("config"));
    let out = work(&home);
    for (name, value) in was {
        match value {
            Some(value) => std::env::set_var(name, value),
            None => std::env::remove_var(name),
        }
    }
    let _ = std::fs::remove_dir_all(&home);
    out
}

/// A document to be remembered, which has to exist: what goes on the list
/// is where the file really is.
fn a_document(home: &Path, name: &str) -> PathBuf {
    let path = home.join(name);
    std::fs::write(&path, b"not really a document").expect("writing it");
    path
}

#[test]
fn a_document_opened_goes_on_the_desktops_own_list() {
    in_a_home_of_its_own("recent", |home| {
        let letter = a_document(home, "Letter to the bank.docx");
        files::remember(&letter, KINDS[0].media_type);

        let list = home.join("data").join("recently-used.xbel");
        let written = std::fs::read_to_string(&list).expect("the desktop's list");
        assert!(written.starts_with("<?xml"), "it is the file the specification describes");
        assert!(
            written.contains("href=\"file://") && written.contains("Letter%20to%20the%20bank.docx"),
            "the document is on it, as an address: {written}"
        );
        assert!(
            written.contains(KINDS[0].media_type),
            "and what kind of document it is, which is how a file manager knows"
        );
        assert!(
            written.contains("bookmark:application name=\"Word Processor\""),
            "and which program used it"
        );

        // A second document goes above the first, and the first stays.
        let notes = a_document(home, "Notes.docx");
        files::remember(&notes, KINDS[0].media_type);
        let written = std::fs::read_to_string(&list).expect("the list again");
        let notes_at = written.find("Notes.docx").expect("the newer document");
        let letter_at = written.find("Letter%20to").expect("the older one");
        assert!(notes_at < letter_at, "the one just used is at the top");

        // And the same document again moves up rather than appearing twice.
        files::remember(&letter, KINDS[0].media_type);
        let written = std::fs::read_to_string(&list).expect("the list again");
        assert_eq!(written.matches("<bookmark ").count(), 2, "two documents, not three");
        assert!(
            written.find("Letter%20to").expect("the document")
                < written.find("Notes.docx").unwrap(),
            "and it is the one at the top now"
        );
    });
}

#[test]
fn a_program_that_says_which_kinds_it_opens_is_the_one_that_opens_them() {
    in_a_home_of_its_own("associate", |home| {
        assert!(!files::opens(&KINDS[0]), "nothing has been said yet");

        assert!(files::associate(KINDS, "Word Processor"), "the desktop took it");

        let entry = home.join("data").join("applications").join("word-processor.desktop");
        let written = std::fs::read_to_string(&entry).expect("the desktop entry");
        assert!(written.contains("[Desktop Entry]"));
        assert!(written.contains("Name=Word Processor"));
        assert!(
            written.contains(KINDS[0].media_type) && written.contains("text/plain"),
            "every kind it can open is named: {written}"
        );

        let list = std::fs::read_to_string(home.join("config").join("mimeapps.list"))
            .expect("which program opens what");
        let defaults = list.split("[Added Associations]").next().expect("the first half");
        assert!(
            defaults.contains(&format!("{}=word-processor.desktop", KINDS[0].media_type)),
            "a document opens in this program: {list}"
        );
        assert!(
            !defaults.contains("text/plain=word-processor.desktop"),
            "a plain text file is not taken from whatever had it: {list}"
        );
        assert!(
            list.contains("text/plain=word-processor.desktop"),
            "though this program is offered for one: {list}"
        );

        assert!(files::opens(&KINDS[0]), "and the program knows it is the one now");
        assert!(!files::opens(&KINDS[1]), "while the kind it did not ask for is somebody else's");
    });
}

#[test]
fn another_programs_choices_survive_this_one_registering() {
    in_a_home_of_its_own("politeness", |home| {
        let list = home.join("config").join("mimeapps.list");
        std::fs::write(
            &list,
            "[Default Applications]\nimage/png=viewer.desktop\ntext/plain=editor.desktop\n",
        )
        .expect("what was there before");

        assert!(files::associate(KINDS, "Word Processor"));

        let written = std::fs::read_to_string(&list).expect("the list");
        assert!(written.contains("image/png=viewer.desktop"), "left alone: {written}");
        assert!(written.contains("text/plain=editor.desktop"), "and so is this: {written}");
    });
}
