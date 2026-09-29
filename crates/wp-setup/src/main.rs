//! The installer, as a person runs it.
//!
//! Run as it is, it asks whether to install and installs; with
//! `--uninstall` — which is how the list of installed programs runs the
//! copy it leaves behind — it asks whether to take the program off and
//! does; with `--quiet` as well it asks and says nothing. `--pack` is how
//! the installer is made: `--pack <installer> <out> <file>...` writes the
//! installer with the files after it.

// A release build has no console: an installer is double-clicked.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use wp_setup::{Failure, PROGRAM_NAME, QUIET_SWITCH, RECORD, UNINSTALL_SWITCH};

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let quiet = arguments.iter().any(|argument| argument == QUIET_SWITCH);
    let packing = arguments.first().map(String::as_str) == Some("--pack");
    let outcome = if packing {
        pack_files(&arguments[1..])
    } else if arguments.iter().any(|argument| argument == UNINSTALL_SWITCH) {
        take_off(quiet)
    } else {
        put_on(quiet)
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            if !quiet && !packing {
                wp_shell::dialog::show_error(&message);
            }
            ExitCode::FAILURE
        }
    }
}

/// Installs, having asked.
fn put_on(quiet: bool) -> Result<(), String> {
    let me = std::env::current_exe().map_err(|error| error.to_string())?;
    let installer =
        std::fs::read(&me).map_err(|error| format!("cannot read {}: {error}", me.display()))?;
    // Found out before anything is asked: an installer that carries nothing
    // has nothing to ask about.
    if wp_setup::payload(&installer).is_none() {
        return Err(Failure::NothingCarried.to_string());
    }
    let folder = wp_shell::install::folder().ok_or_else(|| Failure::NoFolder.to_string())?;
    if !quiet
        && !ask(&format!(
            "Install {PROGRAM_NAME}?\n\nIt is installed for you alone, in {}, and needs no \
             administrator. Word documents and templates can then be opened with it.",
            folder.display()
        ))
    {
        return Ok(());
    }
    let (installed, registered) =
        wp_setup::install(&installer, &folder).map_err(|failure| failure.to_string())?;
    let partly = format!(
        "{PROGRAM_NAME} is installed, but the desktop did not take all of what it was told: \
         some kinds of file may not open in it."
    );
    if quiet {
        return if registered { Ok(()) } else { Err(partly) };
    }
    if !registered {
        wp_shell::dialog::show_error(&partly);
    }
    let word = wp_shell::files::KINDS.iter().find(|kind| kind.extension == ".docx");
    if word.is_some_and(|word| wp_shell::install::opens(word, &installed.program)) {
        if confirm(&format!(
            "{PROGRAM_NAME} is installed, and Word documents open in it.\n\nOK starts it now."
        )) {
            let _ = std::process::Command::new(&installed.program).spawn();
        }
    } else if wp_shell::files::defaults_are_chosen_by_hand() {
        // Windows keeps which program opens a kind where a program cannot
        // write it, and that is the page where the person says.
        if confirm(&format!(
            "{PROGRAM_NAME} is installed. Word documents still open in the program they opened \
             in before.\n\nOK opens the page where you can choose {PROGRAM_NAME} for them instead."
        )) {
            wp_shell::files::choose_defaults();
        }
    } else {
        tell(&format!("{PROGRAM_NAME} is installed."));
    }
    Ok(())
}

/// Takes the program off, having asked.
fn take_off(quiet: bool) -> Result<(), String> {
    let me = std::env::current_exe().map_err(|error| error.to_string())?;
    // The folder this uninstaller is in, where it is the one the installer
    // left; otherwise the one programs are installed in.
    let folder = me
        .parent()
        .filter(|folder| folder.join(RECORD).is_file())
        .map(Path::to_owned)
        .or_else(wp_shell::install::folder)
        .ok_or_else(|| Failure::NoFolder.to_string())?;
    if !folder.join(RECORD).is_file() {
        return Err(format!("{PROGRAM_NAME} is not installed in {}.", folder.display()));
    }
    if !quiet
        && !ask(&format!(
            "Remove {PROGRAM_NAME} from this computer?\n\nYour documents and your settings are \
             left as they are."
        ))
    {
        return Ok(());
    }
    let unregistered = wp_setup::uninstall(&folder).map_err(|failure| failure.to_string())?;
    remove_when_stopped(&me, &folder);
    if !unregistered {
        return Err(format!(
            "{PROGRAM_NAME} is removed, but the desktop did not take back all it was told."
        ));
    }
    if !quiet {
        tell(&format!("{PROGRAM_NAME} has been removed."));
    }
    Ok(())
}

/// The uninstaller cannot remove its own file while it runs on Windows, nor
/// so the folder it is in. The command interpreter is asked to, once this
/// has stopped: a pause, then the file, then the folder — which goes only if
/// it is empty.
#[cfg(windows)]
fn remove_when_stopped(uninstaller: &Path, folder: &Path) {
    use std::os::windows::process::CommandExt;
    /// Started with no console window of its own.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    if uninstaller.parent() != Some(folder) {
        return;
    }
    let interpreter = std::env::var_os("ComSpec").unwrap_or_else(|| "cmd.exe".into());
    // Three pings a second apart are the pause: the one wait the command
    // interpreter has that does not want a console to read from.
    let line = format!(
        "/d /c ping -n 3 127.0.0.1 >nul & del /f /q \"{}\" & rmdir \"{}\"",
        uninstaller.display(),
        folder.display()
    );
    let _ = std::process::Command::new(interpreter)
        .raw_arg(line)
        .current_dir(folder.parent().unwrap_or(folder))
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
}

/// Elsewhere a running program's file can be removed, and has been.
#[cfg(not(windows))]
fn remove_when_stopped(_uninstaller: &Path, _folder: &Path) {}

/// Writes an installer: `--pack <installer> <out> <file>...`, the files put
/// after the installer, each under its own name.
fn pack_files(arguments: &[String]) -> Result<(), String> {
    let [installer, out, files @ ..] = arguments else {
        return Err("usage: --pack <installer> <out> <file>...".to_owned());
    };
    if files.is_empty() {
        return Err("usage: --pack <installer> <out> <file>...".to_owned());
    }
    let stub =
        std::fs::read(installer).map_err(|error| format!("cannot read {installer}: {error}"))?;
    let mut contents = Vec::new();
    for file in files {
        let path = PathBuf::from(file);
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .ok_or_else(|| format!("{file} is not a file"))?;
        let data = std::fs::read(&path).map_err(|error| format!("cannot read {file}: {error}"))?;
        contents.push((name, data));
    }
    let named: Vec<(&str, &[u8])> =
        contents.iter().map(|(name, data)| (name.as_str(), data.as_slice())).collect();
    let packed = wp_setup::pack(&stub, &named)?;
    std::fs::write(out, &packed).map_err(|error| format!("cannot write {out}: {error}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(out, std::fs::Permissions::from_mode(0o755));
    }
    println!("{out}: {} files, {} bytes", contents.len(), packed.len());
    Ok(())
}

/// Whether a question is better asked on the terminal the installer was
/// started from: on Linux, where it usually is, and where a desktop may have
/// no dialog program to ask with.
fn on_a_terminal() -> bool {
    use std::io::IsTerminal;
    cfg!(not(windows)) && std::io::stdin().is_terminal()
}

/// Asks a question with two answers.
fn ask(question: &str) -> bool {
    if on_a_terminal() {
        println!("{question} [y/N]");
        let mut answer = String::new();
        return std::io::stdin().read_line(&mut answer).is_ok()
            && answer.trim().to_lowercase().starts_with('y');
    }
    wp_shell::dialog::ask_yes_no(question)
}

/// Says something and asks whether to do the one thing it offers.
fn confirm(message: &str) -> bool {
    if on_a_terminal() {
        return ask(message);
    }
    wp_shell::dialog::ask_ok_cancel(message)
}

/// Says something.
fn tell(message: &str) {
    if on_a_terminal() {
        println!("{message}");
    } else {
        wp_shell::dialog::show_message(message);
    }
}
