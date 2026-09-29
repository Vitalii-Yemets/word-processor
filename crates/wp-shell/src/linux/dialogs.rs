//! The desktop's own dialogs: open, save, a question, an error.
//!
//! A Linux desktop has no dialog the way Windows has `comdlg32`: each
//! desktop draws its own, through its own toolkit. What every desktop has
//! is a program that shows them — `zenity` on the GNOME side, `kdialog` on
//! the KDE side — and asking that program is how a program with no
//! toolkit of its own puts up the dialog the person already knows. Where
//! neither is installed, the question gets its safe answer and the file
//! dialog gives nothing, which the caller treats as cancelled.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::dialog::{Answer, FileFilter};

/// Which dialog program this desktop has, if either.
fn program() -> Option<&'static str> {
    for candidate in ["zenity", "kdialog"] {
        let found = Command::new(candidate)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success());
        if found {
            return Some(candidate);
        }
    }
    None
}

/// Runs the dialog program and gives back what it printed and whether it
/// said yes.
fn ask(arguments: &[String]) -> Option<(bool, String)> {
    let program = program()?;
    let output = Command::new(program).args(arguments).stderr(Stdio::null()).output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout).trim_end_matches('\n').to_owned();
    Some((output.status.success(), text))
}

/// The file filters as zenity and kdialog each write them.
fn filter_arguments(program: &str, filters: &[FileFilter]) -> Vec<String> {
    match program {
        "zenity" => filters
            .iter()
            .map(|filter| {
                let patterns: Vec<&str> = filter.pattern.split(';').collect();
                format!("--file-filter={} | {}", filter.label, patterns.join(" "))
            })
            .collect(),
        _ => {
            let joined: Vec<String> = filters
                .iter()
                .map(|filter| format!("{}|{}", filter.pattern.replace(';', " "), filter.label))
                .collect();
            vec![joined.join("\n")]
        }
    }
}

/// Asks for a file to open, or where to save one.
pub(crate) fn choose_file(
    title: &str,
    filters: &[FileFilter],
    suggested: Option<&Path>,
    saving: bool,
) -> Option<PathBuf> {
    let program = program()?;
    let mut arguments: Vec<String> = Vec::new();
    match program {
        "zenity" => {
            arguments.push("--file-selection".to_owned());
            arguments.push(format!("--title={title}"));
            if saving {
                arguments.push("--save".to_owned());
                arguments.push("--confirm-overwrite".to_owned());
            }
            if let Some(suggested) = suggested {
                arguments.push(format!("--filename={}", suggested.display()));
            }
            arguments.extend(filter_arguments(program, filters));
        }
        _ => {
            arguments
                .push(if saving { "--getsavefilename" } else { "--getopenfilename" }.to_owned());
            arguments
                .push(suggested.map_or_else(|| ".".to_owned(), |path| path.display().to_string()));
            arguments.extend(filter_arguments(program, filters));
            arguments.push("--title".to_owned());
            arguments.push(title.to_owned());
        }
    }
    let (chosen, path) = ask(&arguments)?;
    (chosen && !path.is_empty()).then(|| PathBuf::from(path))
}

/// Asks whether to save changes before throwing them away.
pub(crate) fn ask_to_save(name: &str) -> Answer {
    let Some(program) = program() else { return Answer::Cancel };
    let text = format!("Save the changes to {name}?");
    let arguments: Vec<String> = match program {
        "zenity" => vec![
            "--question".to_owned(),
            "--title=Word Processor".to_owned(),
            format!("--text={text}"),
            "--ok-label=Save".to_owned(),
            "--cancel-label=Don't Save".to_owned(),
            "--extra-button=Cancel".to_owned(),
        ],
        _ => vec![
            "--yesnocancel".to_owned(),
            text,
            "--yes-label".to_owned(),
            "Save".to_owned(),
            "--no-label".to_owned(),
            "Don't Save".to_owned(),
        ],
    };
    let output = Command::new(program).args(&arguments).stderr(Stdio::null()).output();
    let Ok(output) = output else { return Answer::Cancel };
    let printed = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    match (program, output.status.code()) {
        // zenity: the extra button prints its label and exits as cancel.
        ("zenity", Some(0)) => Answer::Yes,
        ("zenity", Some(1)) if printed == "Cancel" => Answer::Cancel,
        ("zenity", Some(1)) => Answer::No,
        // kdialog: yes is 0, no is 1, cancel is 2.
        (_, Some(0)) => Answer::Yes,
        (_, Some(1)) => Answer::No,
        _ => Answer::Cancel,
    }
}

/// Asks a question with two answers: yes, or anything else.
pub(crate) fn ask_yes_no(question: &str) -> bool {
    let Some(program) = program() else { return false };
    let arguments: Vec<String> = match program {
        "zenity" => vec![
            "--question".to_owned(),
            "--title=Word Processor".to_owned(),
            format!("--text={question}"),
        ],
        _ => vec!["--yesno".to_owned(), question.to_owned()],
    };
    ask(&arguments).is_some_and(|(yes, _)| yes)
}

/// Tells the user something and asks whether to go on.
pub(crate) fn ask_ok_cancel(message: &str) -> bool {
    let Some(program) = program() else { return true };
    let arguments: Vec<String> = match program {
        "zenity" => vec![
            "--question".to_owned(),
            "--title=Word Processor".to_owned(),
            format!("--text={message}"),
            "--ok-label=OK".to_owned(),
            "--cancel-label=Cancel".to_owned(),
        ],
        _ => vec!["--warningcontinuecancel".to_owned(), message.to_owned()],
    };
    ask(&arguments).is_some_and(|(yes, _)| yes)
}

/// Tells the user something that went as it should.
pub(crate) fn show_message(message: &str) {
    let Some(program) = program() else {
        println!("{message}");
        return;
    };
    let arguments: Vec<String> = match program {
        "zenity" => vec![
            "--info".to_owned(),
            "--title=Word Processor".to_owned(),
            format!("--text={message}"),
        ],
        _ => vec!["--msgbox".to_owned(), message.to_owned()],
    };
    let _ = ask(&arguments);
}

/// Tells the user something went wrong.
pub(crate) fn show_error(message: &str) {
    let Some(program) = program() else {
        eprintln!("error: {message}");
        return;
    };
    let arguments: Vec<String> = match program {
        "zenity" => vec![
            "--error".to_owned(),
            "--title=Word Processor".to_owned(),
            format!("--text={message}"),
        ],
        _ => vec!["--error".to_owned(), message.to_owned()],
    };
    let _ = ask(&arguments);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_are_written_the_way_each_program_reads_them() {
        let filters = [FileFilter { label: "Word Documents (*.docx)", pattern: "*.docx;*.docm" }];
        assert_eq!(
            filter_arguments("zenity", &filters),
            vec!["--file-filter=Word Documents (*.docx) | *.docx *.docm".to_owned()]
        );
        assert_eq!(
            filter_arguments("kdialog", &filters),
            vec!["*.docx *.docm|Word Documents (*.docx)".to_owned()]
        );
    }
}
