//! The installer run as a person runs it: packed, started, and asked to take
//! the program off again, in a home of its own.
//!
//! What it is held to is what the desktop then does, asked through the
//! desktop's own tool where the build image has it: `gio`, which reads the
//! desktop entries and `mimeapps.list` the way a file manager does, says
//! which program a Word document opens with, and opens one — which is a
//! double-click without the mouse. The program installed is a stand-in
//! that writes down what it was started with.
#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

const SETUP: &str = env!("CARGO_BIN_EXE_word-processor-setup");

const WORD: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";

/// A home of the test's own, which the installer is pointed at through the
/// environment of the process it runs in and nothing else.
fn home(name: &str) -> PathBuf {
    let home = std::env::temp_dir().join(format!("wp-setup-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(home.join("data")).expect("somewhere to write");
    std::fs::create_dir_all(home.join("config")).expect("somewhere to write");
    home
}

/// A program run in that home, with no display to ask questions on.
fn run(program: &Path, home: &Path, arguments: &[&str]) -> Output {
    command(program, home).args(arguments).output().expect("the program ran")
}

fn command(program: impl AsRef<std::ffi::OsStr>, home: &Path) -> Command {
    let mut command = Command::new(program);
    command
        .env("HOME", home)
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("DBUS_SESSION_BUS_ADDRESS")
        .stdin(Stdio::null());
    command
}

/// The stand-in for the program: it writes down the file it was given.
const STAND_IN: &[u8] = b"#!/bin/sh\nprintf '%s\\n' \"$@\" >> \"$HOME/started\"\n";

/// An installer carrying the stand-in, written into the home.
fn installer(home: &Path) -> PathBuf {
    let stub = std::fs::read(SETUP).expect("the setup program");
    let packed = wp_setup::pack(&stub, &[("word-processor", STAND_IN), ("wp", b"#!/bin/sh\n")])
        .expect("packed");
    let path = home.join("word-processor-setup");
    std::fs::write(&path, packed).expect("written");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("runnable");
    path
}

fn said(output: &Output) -> String {
    format!(
        "status {}\n{}{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// What `gio` says or does, where the image has it.
fn gio(home: &Path, arguments: &[&str]) -> Option<String> {
    let output = command("gio", home).args(arguments).stderr(Stdio::piped()).output().ok()?;
    Some(said(&output))
}

/// What the stand-in was started with, waited for a little: `gio` starts a
/// program and does not wait for it.
fn started(home: &Path) -> String {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(text) = std::fs::read_to_string(home.join("started")) {
            if !text.is_empty() {
                return text;
            }
        }
        if Instant::now() > deadline {
            return String::new();
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn installed_a_word_document_opens_in_it_and_uninstalled_nothing_is_left() {
    let home = home("whole");
    // What another program had said before this one was installed.
    std::fs::write(
        home.join("config").join("mimeapps.list"),
        concat!(
            "[Default Applications]\n",
            "image/png=viewer.desktop\n",
            "application/msword=other.desktop;\n",
            "\n",
            "[Added Associations]\n",
            "text/plain=editor.desktop;\n"
        ),
    )
    .expect("the list as it was");

    let setup = installer(&home);
    let output = run(&setup, &home, &["--quiet"]);
    assert!(output.status.success(), "{}", said(&output));

    let folder = home.join("data").join("word-processor");
    for name in ["word-processor", "wp", "uninstall", "installed.txt"] {
        assert!(folder.join(name).is_file(), "{name} is installed");
    }
    assert_eq!(
        std::fs::read(folder.join("word-processor")).expect("the program"),
        STAND_IN,
        "the program is the one carried"
    );
    assert_eq!(
        std::fs::read(folder.join("uninstall")).expect("the uninstaller"),
        std::fs::read(SETUP).expect("the setup program"),
        "what is left to uninstall is the installer without what it carried"
    );
    let entry = std::fs::read_to_string(home.join("data/applications/word-processor.desktop"))
        .expect("the desktop entry");
    assert!(
        entry.contains(&format!("Exec={} %f", folder.join("word-processor").display())),
        "the entry starts the installed program: {entry}"
    );
    let list = std::fs::read_to_string(home.join("config/mimeapps.list")).expect("the list");
    assert!(list.contains(&format!("{WORD}=word-processor.desktop;")), "{list}");
    assert!(list.contains("image/png=viewer.desktop"), "{list}");
    assert!(list.contains("application/msword=word-processor.desktop;other.desktop;"), "{list}");
    assert!(list.contains("text/plain=editor.desktop;word-processor.desktop;"), "{list}");

    // What the desktop makes of it, asked the desktop's way.
    let letter = home.join("Letter.docx");
    std::fs::write(&letter, b"not really a document").expect("a document");
    let template = home.join("Invoice.dotx");
    std::fs::write(&template, b"not really a template").expect("a template");
    if let Some(answer) = gio(&home, &["mime", WORD]) {
        assert!(
            answer.contains("Default application for") && answer.contains("word-processor.desktop"),
            "the desktop says a Word document opens here: {answer}"
        );
        let opened = gio(&home, &["open", &letter.display().to_string()]).unwrap_or_default();
        let with = started(&home);
        assert!(
            with.contains("Letter.docx"),
            "and opening one starts the installed program with it: {with} ({opened})"
        );
        let _ = std::fs::remove_file(home.join("started"));
        let _ = gio(&home, &["open", &template.display().to_string()]);
        let with = started(&home);
        assert!(with.contains("Invoice.dotx"), "and a template the same: {with}");
    } else {
        eprintln!("note: no gio here; what the desktop makes of it was not asked");
    }

    // Installed again over itself, which is what a newer version does.
    let output = run(&setup, &home, &["--quiet"]);
    assert!(output.status.success(), "{}", said(&output));
    let list = std::fs::read_to_string(home.join("config/mimeapps.list")).expect("the list");
    assert_eq!(list.matches(&format!("{WORD}=")).count(), 2, "not listed twice: {list}");

    // Something of the person's own in the folder, which is not the
    // uninstaller's to throw away.
    std::fs::write(folder.join("notes.txt"), b"mine").expect("a file of one's own");
    let output = run(&folder.join("uninstall"), &home, &["--uninstall", "--quiet"]);
    assert!(output.status.success(), "{}", said(&output));
    for name in ["word-processor", "wp", "uninstall", "installed.txt"] {
        assert!(!folder.join(name).exists(), "{name} is gone");
    }
    assert!(folder.join("notes.txt").is_file(), "and the person's own file is not");
    assert!(!home.join("data/applications/word-processor.desktop").exists());
    let list = std::fs::read_to_string(home.join("config/mimeapps.list")).expect("the list");
    assert!(!list.contains("word-processor.desktop"), "{list}");
    assert!(list.contains("application/msword=other.desktop;"), "given back: {list}");
    assert!(list.contains("image/png=viewer.desktop"), "{list}");
    assert!(list.contains("text/plain=editor.desktop;"), "{list}");
    if let Some(answer) = gio(&home, &["mime", WORD]) {
        assert!(!answer.contains("word-processor.desktop"), "{answer}");
    }

    // Taken off, it is not there to take off again, and says so.
    let output = run(&setup, &home, &["--uninstall", "--quiet"]);
    assert!(!output.status.success(), "nothing is installed now");
    assert!(said(&output).contains("not installed"), "{}", said(&output));
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn the_uninstaller_takes_away_the_folder_when_nothing_else_is_in_it() {
    let home = home("folder");
    let setup = installer(&home);
    assert!(run(&setup, &home, &["--quiet"]).status.success());
    let folder = home.join("data").join("word-processor");
    let output = run(&folder.join("uninstall"), &home, &["--uninstall", "--quiet"]);
    assert!(output.status.success(), "{}", said(&output));
    assert!(!folder.exists(), "the folder is gone with the files");
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn a_setup_that_carries_nothing_installs_nothing() {
    let home = home("empty");
    let output = run(Path::new(SETUP), &home, &["--quiet"]);
    assert!(!output.status.success());
    assert!(said(&output).contains("carries no program"), "{}", said(&output));
    assert!(!home.join("data").join("word-processor").exists());
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn packing_on_the_command_line_makes_an_installer() {
    let home = home("pack");
    std::fs::write(home.join("word-processor"), STAND_IN).expect("a program");
    std::fs::write(home.join("wp"), b"#!/bin/sh\n").expect("another");
    let out = home.join("packed");
    let output = run(
        Path::new(SETUP),
        &home,
        &[
            "--pack",
            SETUP,
            &out.display().to_string(),
            &home.join("word-processor").display().to_string(),
            &home.join("wp").display().to_string(),
        ],
    );
    assert!(output.status.success(), "{}", said(&output));
    let packed = std::fs::read(&out).expect("the installer");
    let files = wp_setup::carried(&packed).expect("what it carries");
    let names: Vec<&str> = files.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["word-processor", "wp"]);
    // And it installs.
    let output = run(&out, &home, &["--quiet"]);
    assert!(output.status.success(), "{}", said(&output));
    assert!(home.join("data/word-processor/word-processor").is_file());
    let _ = std::fs::remove_dir_all(&home);
}
